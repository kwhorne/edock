//! Snapshots: a container's filesystem, or a volume's files, as they are
//! now, to go back to later. Made by the app through the Docker API, so they
//! work on any engine.
//!
//! - A container's snapshot is `docker commit`: an image
//!   `edock-snapshots/<container>:<time>`, labelled. Restoring makes the
//!   container again from it, with its settings (ports, mounts, networks,
//!   environment); the one it replaces is stopped and kept, renamed.
//!   Volumes aren't part of it: they're snapshotted on their own.
//! - A volume's snapshot is a `.tar.gz` of its files, owners kept, in
//!   `~/.edock/snapshots/volumes/<volume>/` on the Mac, written and read by
//!   a short-lived helper container. Each engine (context) has its own
//!   folder of them there.

use std::path::PathBuf;

use bollard::query_parameters as q;
use elyra::{command, Ctx, EventBus};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::engine::Engine;

const LABEL: &str = "app.edock.snapshot";
const LABEL_OF: &str = "app.edock.snapshot.of";
const LABEL_NOTE: &str = "app.edock.snapshot.note";
const REPO: &str = "edock-snapshots";
/// What the volume helpers run in: it has tar, gzip and find.
const HELPER: &str = "alpine:latest";

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Local time as `20261114-221503`: a tag, and a file name, that sort.
fn stamp() -> String {
    // SAFETY: time and localtime_r with valid pointers; tm is plain data.
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        format!("{:04}{:02}{:02}-{:02}{:02}{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec)
    }
}

/// A container or volume name as an image repository part: lowercase, and
/// only what a repository may have.
fn repo_part(name: &str) -> String {
    let s: String = name.to_ascii_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '-' }).collect();
    let s = s.trim_matches(|c: char| !c.is_ascii_alphanumeric()).to_owned();
    if s.is_empty() { "container".into() } else { s }
}

/// A snapshot file's name, as `volume_snapshot` writes them.
fn valid_file(file: &str) -> Result<&str, String> {
    let ok = file.ends_with(".tar.gz") && !file.starts_with('.') && file.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b));
    if ok { Ok(file) } else { Err("That isn't a snapshot".into()) }
}

/// Where the engine the app shows keeps its volume snapshots:
/// `~/.edock/snapshots/volumes/<context>/`, since a `postgres` volume on
/// OrbStack isn't the one on eDock. The first snapshots (eDock 0.5.0, one
/// folder for all) were eDock's: moved into `edock/` the first time.
fn volumes_dir(ctx: &Ctx) -> Result<PathBuf, String> {
    let base = dirs::home_dir().ok_or("HOME is not set")?.join(".edock/snapshots/volumes");
    let context = ctx.get::<Engine>().context().0;
    let context: String = context.chars().map(|c| if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '_' }).collect();
    let mine = base.join(if context.is_empty() || context.starts_with('.') { "_".into() } else { context });
    migrate(&base);
    Ok(mine)
}

/// A volume's folder of snapshots straight under `base` is 0.5.0's: it
/// moves to `base/edock/`. A context's folder holds folders, not archives.
fn migrate(base: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(base) else { return };
    for e in entries.flatten() {
        let path = e.path();
        let old = std::fs::read_dir(&path).map(|f| f.flatten().any(|f| valid_file(&f.file_name().to_string_lossy()).is_ok())).unwrap_or(false);
        if old {
            let to = base.join(crate::engine::EDOCK).join(e.file_name());
            if std::fs::create_dir_all(base.join(crate::engine::EDOCK)).is_ok() && !to.exists() {
                let _ = std::fs::rename(&path, &to);
            }
        }
    }
}

fn changed(ctx: &Ctx, kind: &str) {
    let _ = ctx.get::<EventBus>().emit("docker:changed", &crate::streams::Changed { kind: kind.into(), action: "snapshot".into() });
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct Snapshot {
    /// container or volume.
    pub kind: String,
    /// The container or volume it was taken of.
    pub of: String,
    /// What identifies it: the image (`edock-snapshots/web:20261114-221503`)
    /// or the file name.
    pub id: String,
    /// Unix time, seconds.
    pub created: f64,
    pub size: f64,
    pub note: String,
}

/// Every snapshot, newest first: containers' (images) and volumes' (files).
#[command(can = "edock.view")]
pub async fn snapshots_list(ctx: Ctx) -> Result<Vec<Snapshot>, String> {
    let docker = ctx.get::<Engine>().docker()?;
    let mut filters = std::collections::HashMap::new();
    filters.insert("label".to_string(), vec![LABEL.to_string()]);
    let images = docker.list_images(Some(q::ListImagesOptionsBuilder::default().filters(&filters).build())).await.map_err(err)?;
    let mut out: Vec<Snapshot> = Vec::new();
    for img in images {
        let labels = &img.labels;
        for tag in img.repo_tags.iter().filter(|t| t.starts_with(&format!("{REPO}/"))) {
            out.push(Snapshot {
                kind: "container".into(),
                of: labels.get(LABEL_OF).cloned().unwrap_or_default(),
                id: tag.clone(),
                created: img.created as f64,
                size: img.size as f64,
                note: labels.get(LABEL_NOTE).cloned().unwrap_or_default(),
            });
        }
    }
    if let Ok(dir) = volumes_dir(&ctx) {
        if let Ok(mut vols) = tokio::fs::read_dir(&dir).await {
            while let Ok(Some(v)) = vols.next_entry().await {
                let volume = v.file_name().to_string_lossy().into_owned();
                let Ok(mut files) = tokio::fs::read_dir(v.path()).await else { continue };
                while let Ok(Some(f)) = files.next_entry().await {
                    let file = f.file_name().to_string_lossy().into_owned();
                    if valid_file(&file).is_err() {
                        continue;
                    }
                    let meta = f.metadata().await.ok();
                    let created = meta.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0.0, |d| d.as_secs_f64());
                    let note = tokio::fs::read_to_string(f.path().with_extension("").with_extension("note")).await.unwrap_or_default();
                    out.push(Snapshot { kind: "volume".into(), of: volume.clone(), id: file, created, size: meta.map_or(0.0, |m| m.len() as f64), note: note.trim().to_owned() });
                }
            }
        }
    }
    out.sort_by(|a, b| b.created.total_cmp(&a.created));
    Ok(out)
}

/// Snapshot a container's filesystem now (`docker commit`), with an
/// optional note. Its volumes aren't in it: snapshot those on their own.
/// Returns the snapshot's image.
#[command(can = "edock.manage")]
pub async fn container_snapshot(ctx: Ctx, container: String, note: Option<String>) -> Result<String, String> {
    let made = commit(&ctx, &container, note.as_deref()).await?;
    changed(&ctx, "image");
    Ok(made)
}

/// `docker commit` of `container` as a snapshot; its image.
async fn commit(ctx: &Ctx, container: &str, note: Option<&str>) -> Result<String, String> {
    let docker = ctx.get::<Engine>().docker()?;
    let info = docker.inspect_container(container, None::<q::InspectContainerOptions>).await.map_err(err)?;
    let name = info.name.as_deref().unwrap_or(container).trim_start_matches('/').to_owned();
    let repo = format!("{REPO}/{}", repo_part(&name));
    // Two in the same second: the second gets `-2`, never the first's tag.
    let mut tag = stamp();
    for n in 2.. {
        if docker.inspect_image(&format!("{repo}:{tag}")).await.is_err() {
            break;
        }
        tag = format!("{}-{n}", stamp_base(&tag));
    }
    let mut labels = info.config.as_ref().and_then(|c| c.labels.clone()).unwrap_or_default();
    labels.insert(LABEL.into(), "container".into());
    labels.insert(LABEL_OF.into(), name.clone());
    if let Some(n) = note.map(str::trim).filter(|n| !n.is_empty()) {
        labels.insert(LABEL_NOTE.into(), n.chars().take(200).collect());
    }
    let config = bollard::models::ContainerConfig { labels: Some(labels), ..Default::default() };
    let opts = q::CommitContainerOptionsBuilder::default().container(container).repo(&repo).tag(&tag).pause(true).build();
    docker.commit_container(opts, config).await.map_err(err)?;
    Ok(format!("{repo}:{tag}"))
}

/// A stamp without the `-2` a second snapshot in the same second gets.
fn stamp_base(s: &str) -> &str {
    &s[..s.len().min(15)]
}

/// The settings to make a container again, from its `inspect`: its config,
/// host config (ports, mounts, restart policy…) and networks, on `image`.
fn recreate_body(info: &Value, image: &str) -> Result<bollard::models::ContainerCreateBody, String> {
    let mut body = info["Config"].clone();
    body["Image"] = json!(image);
    // The image's own (the snapshot's) labels aren't the container's.
    if let Some(l) = body["Labels"].as_object_mut() {
        l.retain(|k, _| !k.starts_with(LABEL));
    }
    body["HostConfig"] = info["HostConfig"].clone();
    // An engine that doesn't keep them as asked (eDock before 0.5.0) still
    // says what's mounted and published: from that.
    let host = &mut body["HostConfig"];
    // Volumes the image declares (postgres' data) are in neither: an
    // anonymous volume each. The new container mounts the same ones, as
    // Compose does, or its data would seem gone.
    if !(host["Binds"].is_null() && host["Mounts"].is_null()) {
        let mut taken: Vec<String> = host["Binds"].as_array().into_iter().flatten().filter_map(|b| b.as_str()?.split(':').nth(1).map(str::to_owned)).collect();
        taken.extend(host["Mounts"].as_array().into_iter().flatten().filter_map(|m| m["Target"].as_str().map(str::to_owned)));
        taken.extend(host["Tmpfs"].as_object().into_iter().flatten().map(|(k, _)| k.clone()));
        let more: Vec<Value> = info["Mounts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|m| m["Type"] == "volume")
            .filter_map(|m| {
                let (name, dest) = (m["Name"].as_str()?, m["Destination"].as_str()?);
                let ro = if m["RW"] == false { ":ro" } else { "" };
                (!taken.iter().any(|t| t.trim_end_matches('/') == dest.trim_end_matches('/'))).then(|| json!(format!("{name}:{dest}{ro}")))
            })
            .collect();
        if !more.is_empty() {
            if !host["Binds"].is_array() {
                host["Binds"] = json!([]);
            }
            host["Binds"].as_array_mut().expect("an array").extend(more);
        }
    } else {
        let binds: Vec<String> = info["Mounts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| {
                let source = match m["Type"].as_str()? {
                    "volume" => m["Name"].as_str()?,
                    "bind" if m["Source"].as_str().is_some_and(|p| std::path::Path::new(p).exists()) => m["Source"].as_str()?,
                    _ => return None,
                };
                let ro = if m["RW"] == false { ":ro" } else { "" };
                Some(format!("{source}:{}{ro}", m["Destination"].as_str()?))
            })
            .collect();
        if !binds.is_empty() {
            host["Binds"] = json!(binds);
        }
    }
    if host["PortBindings"].as_object().is_none_or(|p| p.is_empty()) {
        if let Some(ports) = info["NetworkSettings"]["Ports"].as_object() {
            let bound: serde_json::Map<String, Value> = ports.iter().filter(|(_, v)| v.as_array().is_some_and(|a| !a.is_empty())).map(|(k, v)| (k.clone(), v.clone())).collect();
            if !bound.is_empty() {
                host["PortBindings"] = Value::Object(bound);
            }
        }
    }
    // Its address and the like are the old container's: keep the network,
    // its aliases and any fixed address only.
    let mut endpoints = serde_json::Map::new();
    for (net, ep) in info["NetworkSettings"]["Networks"].as_object().into_iter().flatten() {
        let mut e = json!({});
        if let Some(a) = ep["Aliases"].as_array() {
            e["Aliases"] = json!(a);
        }
        if !ep["IPAMConfig"].is_null() {
            e["IPAMConfig"] = ep["IPAMConfig"].clone();
        }
        endpoints.insert(net.clone(), e);
    }
    body["NetworkingConfig"] = json!({ "EndpointsConfig": endpoints });
    serde_json::from_value(body).map_err(|e| format!("couldn't read the container's settings: {e}"))
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct Restored {
    /// The container made from the snapshot.
    pub container: String,
    /// The one it replaced: stopped and renamed, or for a Compose service
    /// a snapshot (`edock-snapshots/…`); None when there was none.
    pub previous: Option<String>,
}

/// Go back to a container's snapshot: the container is made again from it,
/// with the same name and settings, and started if it was running. The
/// container it replaces is stopped and kept, renamed `<name>-before-<time>`,
/// so nothing is lost; remove it once happy.
#[command(can = "edock.manage")]
pub async fn container_snapshot_restore(ctx: Ctx, image: String) -> Result<Restored, String> {
    let docker = ctx.get::<Engine>().docker()?;
    let img = docker.inspect_image(&image).await.map_err(|_| format!("{image} isn't a snapshot here"))?;
    let labels = img.config.as_ref().and_then(|c| c.labels.clone()).unwrap_or_default();
    if labels.get(LABEL).map(String::as_str) != Some("container") {
        return Err(format!("{image} isn't a container snapshot"));
    }
    let name = labels.get(LABEL_OF).cloned().ok_or("The snapshot doesn't say which container it's of")?;
    let current = docker.inspect_container(&name, None::<q::InspectContainerOptions>).await.ok();
    let Some(current) = current else {
        // Its container is gone: a new one from the snapshot, with what the
        // image keeps (command, environment, ports declared).
        let body = bollard::models::ContainerCreateBody { image: Some(image.clone()), ..Default::default() };
        docker.create_container(Some(q::CreateContainerOptionsBuilder::default().name(&name).build()), body).await.map_err(err)?;
        changed(&ctx, "container");
        return Ok(Restored { container: name, previous: None });
    };
    let info = serde_json::to_value(&current).map_err(err)?;
    let body = recreate_body(&info, &image)?;
    let was_running = current.state.as_ref().and_then(|s| s.running).unwrap_or(false);
    if was_running {
        docker.stop_container(&name, None::<q::StopContainerOptions>).await.map_err(err)?;
    }
    // A Compose service's container: Compose would count the one kept as
    // the same service and replica, so it's kept as a snapshot instead.
    let compose = current.config.as_ref().and_then(|c| c.labels.as_ref()).is_some_and(|l| l.contains_key("com.docker.compose.project"));
    let kept = if compose { Some(commit(&ctx, &name, Some(&format!("Before restoring {image}"))).await.map_err(|e| format!("couldn't keep the container as it is: {e}"))?) } else { None };
    let previous = format!("{name}-before-{}", stamp());
    docker.rename_container(&name, q::RenameContainerOptionsBuilder::default().name(&previous).build()).await.map_err(err)?;
    let made = docker.create_container(Some(q::CreateContainerOptionsBuilder::default().name(&name).build()), body).await;
    if let Err(e) = made {
        // Put things back as they were.
        let _ = docker.rename_container(&previous, q::RenameContainerOptionsBuilder::default().name(&name).build()).await;
        if was_running {
            let _ = docker.start_container(&name, None::<q::StartContainerOptions>).await;
        }
        return Err(format!("couldn't make the container again: {e}"));
    }
    // Its volumes stay: the new container uses them.
    let previous = match kept {
        Some(image) => {
            docker.remove_container(&previous, None::<q::RemoveContainerOptions>).await.map_err(|e| format!("restored, but {previous} is still there: {e}"))?;
            changed(&ctx, "image");
            image
        }
        None => previous,
    };
    if was_running {
        docker.start_container(&name, None::<q::StartContainerOptions>).await.map_err(|e| format!("made again, but it didn't start: {e}"))?;
    }
    changed(&ctx, "container");
    Ok(Restored { container: name, previous: Some(previous) })
}

/// Runs a short-lived helper (alpine, no network) with `mounts` and a shell
/// `script`, waits for it, and removes it; its last error line when it
/// fails. Volume snapshots and clones use it.
pub(crate) async fn helper(ctx: &Ctx, mounts: Vec<String>, script: String) -> Result<(), String> {
    let docker = ctx.get::<Engine>().docker()?;
    crate::volumes::ensure_image(&docker).await?;
    let body = bollard::models::ContainerCreateBody {
        image: Some(HELPER.into()),
        cmd: Some(vec!["sh".into(), "-c".into(), script]),
        labels: Some([("app.edock.helper".to_string(), "task".to_string())].into()),
        host_config: Some(bollard::models::HostConfig { binds: Some(mounts), network_mode: Some("none".into()), ..Default::default() }),
        ..Default::default()
    };
    let id = docker.create_container(None::<q::CreateContainerOptions>, body).await.map_err(err)?.id;
    let run = async {
        docker.start_container(&id, None::<q::StartContainerOptions>).await.map_err(err)?;
        let _ = docker.wait_container(&id, None::<q::WaitContainerOptions>).collect::<Vec<_>>().await;
        let info = docker.inspect_container(&id, None::<q::InspectContainerOptions>).await.map_err(err)?;
        if info.state.and_then(|s| s.exit_code) == Some(0) {
            return Ok(());
        }
        let mut out = String::new();
        let mut logs = docker.logs(&id, Some(q::LogsOptionsBuilder::default().stdout(true).stderr(true).tail("5").build()));
        while let Some(Ok(l)) = logs.next().await {
            out.push_str(&l.to_string());
        }
        Err(out.trim().lines().last().unwrap_or("the helper failed").to_owned())
    }
    .await;
    let _ = docker.remove_container(&id, Some(q::RemoveContainerOptionsBuilder::default().force(true).build())).await;
    run
}

/// Snapshot a volume's files now, owners kept, to a `.tar.gz` on the Mac
/// (`~/.edock/snapshots/volumes/<volume>/`). Best with the containers that
/// write to it stopped, as with any copy of a database's files. Returns the
/// file's name.
#[command(can = "edock.manage")]
pub async fn volume_snapshot(ctx: Ctx, volume: String, note: Option<String>) -> Result<String, String> {
    let volume = crate::volumes::valid(&volume)?.to_owned();
    let docker = ctx.get::<Engine>().docker()?;
    docker.inspect_volume(&volume).await.map_err(|_| format!("There's no volume {volume}"))?;
    let dir = volumes_dir(&ctx)?.join(&volume);
    tokio::fs::create_dir_all(&dir).await.map_err(err)?;
    let mut file = format!("{}.tar.gz", stamp());
    for n in 2.. {
        if !dir.join(&file).exists() {
            break;
        }
        file = format!("{}-{n}.tar.gz", stamp_base(&file));
    }
    helper(
        &ctx,
        vec![format!("{volume}:/from:ro"), format!("{}:/out", dir.display())],
        format!("tar -C /from -czf /out/.{file}.part . && mv /out/.{file}.part /out/{file}"),
    )
    .await
    .map_err(|e| {
        let _ = std::fs::remove_file(dir.join(format!(".{file}.part")));
        format!("couldn't snapshot {volume}: {e}")
    })?;
    if let Some(n) = note.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        let _ = tokio::fs::write(dir.join(format!("{}.note", file.trim_end_matches(".tar.gz"))), n.chars().take(200).collect::<String>()).await;
    }
    changed(&ctx, "volume");
    Ok(file)
}

/// Put a volume back as it was in a snapshot: what's in it now is replaced.
/// Refused while a running container uses the volume: stop it first.
#[command(can = "edock.manage")]
pub async fn volume_snapshot_restore(ctx: Ctx, volume: String, file: String) -> Result<(), String> {
    let volume = crate::volumes::valid(&volume)?.to_owned();
    let file = valid_file(&file)?.to_owned();
    let dir = volumes_dir(&ctx)?.join(&volume);
    if !dir.join(&file).is_file() {
        return Err("That snapshot isn't there any more".into());
    }
    let docker = ctx.get::<Engine>().docker()?;
    let mut filters = std::collections::HashMap::new();
    filters.insert("volume".to_string(), vec![volume.clone()]);
    filters.insert("status".to_string(), vec!["running".to_string()]);
    let running = docker.list_containers(Some(q::ListContainersOptionsBuilder::default().filters(&filters).build())).await.map_err(err)?;
    if !running.is_empty() {
        let names: Vec<String> = running.iter().filter_map(|c| c.names.as_ref()?.first().map(|n| n.trim_start_matches('/').to_owned())).collect();
        return Err(format!("{} uses {volume} and is running: stop it first", names.join(", ")));
    }
    // Made if it's gone; emptied, then filled.
    let _ = docker.create_volume(bollard::models::VolumeCreateRequest { name: Some(volume.clone()), ..Default::default() }).await;
    helper(
        &ctx,
        vec![format!("{volume}:/to"), format!("{}:/in:ro", dir.display())],
        format!("find /to -mindepth 1 -maxdepth 1 -exec rm -rf {{}} + && tar -C /to -xzpf /in/{file}"),
    )
    .await
    .map_err(|e| format!("couldn't restore {volume}: {e}"))?;
    changed(&ctx, "volume");
    Ok(())
}

/// Delete a snapshot: a container's (its image) or a volume's (its file).
#[command]
pub async fn snapshot_delete(ctx: Ctx, kind: String, of: String, id: String) -> Result<(), String> {
    match kind.as_str() {
        "container" => {
            if !id.starts_with(&format!("{REPO}/")) {
                return Err("That isn't a snapshot".into());
            }
            let docker = ctx.get::<Engine>().docker()?;
            docker.remove_image(&id, None::<q::RemoveImageOptions>, None).await.map_err(err)?;
            changed(&ctx, "image");
        }
        "volume" => {
            let dir = volumes_dir(&ctx)?.join(crate::volumes::valid(&of)?);
            let file = valid_file(&id)?;
            tokio::fs::remove_file(dir.join(file)).await.map_err(err)?;
            let _ = tokio::fs::remove_file(dir.join(format!("{}.note", file.trim_end_matches(".tar.gz")))).await;
            // The volume's folder goes with its last snapshot.
            let _ = tokio::fs::remove_dir(&dir).await;
            changed(&ctx, "volume");
        }
        other => return Err(format!("{other} isn't a kind of snapshot")),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_first_snapshots_move_into_edocks_folder() {
        let base = std::env::temp_dir().join(format!("edock-snapmig-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("pgdata")).unwrap();
        std::fs::write(base.join("pgdata/20261114-221503.tar.gz"), "x").unwrap();
        std::fs::create_dir_all(base.join("orbstack/redis")).unwrap();
        std::fs::write(base.join("orbstack/redis/20261114-221503.tar.gz"), "x").unwrap();
        super::migrate(&base);
        assert!(base.join("edock/pgdata/20261114-221503.tar.gz").is_file());
        assert!(!base.join("pgdata").exists());
        assert!(base.join("orbstack/redis/20261114-221503.tar.gz").is_file(), "a context's folder stays");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn names_become_safe_repositories_and_files() {
        assert_eq!(super::repo_part("My_App.web-1"), "my_app.web-1");
        assert_eq!(super::repo_part("--x--"), "x");
        assert_eq!(super::repo_part("é"), "container");
        assert!(crate::volumes::valid("pg_data-1").is_ok());
        for bad in ["", "../x", "a/b", "-a", ".a"] {
            assert!(crate::volumes::valid(bad).is_err(), "{bad}");
        }
        assert!(super::valid_file("20261114-221503.tar.gz").is_ok());
        for bad in ["x.tar", "../a.tar.gz", ".20261114.tar.gz", "a b.tar.gz"] {
            assert!(super::valid_file(bad).is_err(), "{bad}");
        }
        assert_eq!(super::stamp().len(), 15);
    }

    #[test]
    fn a_recreated_container_keeps_its_settings_not_its_address() {
        let info = serde_json::json!({
            "Config": {"Image": "nginx", "Env": ["A=1"], "Labels": {"x": "1", "app.edock.snapshot.of": "web"}},
            "HostConfig": {"PortBindings": {"80/tcp": [{"HostPort": "8080"}]}, "RestartPolicy": {"Name": "always"}},
            "NetworkSettings": {"Networks": {"app_default": {"Aliases": ["web"], "IPAddress": "172.18.0.5", "MacAddress": "x", "IPAMConfig": null}}}
        });
        let b = super::recreate_body(&info, "edock-snapshots/web:1").unwrap();
        let v = serde_json::to_value(&b).unwrap();
        assert_eq!(v["Image"], "edock-snapshots/web:1");
        assert_eq!(v["Env"][0], "A=1");
        assert_eq!(v["Labels"], serde_json::json!({"x": "1"}));
        assert_eq!(v["HostConfig"]["PortBindings"]["80/tcp"][0]["HostPort"], "8080");
        assert_eq!(v["NetworkingConfig"]["EndpointsConfig"]["app_default"]["Aliases"][0], "web");
        assert!(v["NetworkingConfig"]["EndpointsConfig"]["app_default"]["IPAddress"].is_null());

        // Mounts and ports from what's reported, when HostConfig lacks them.
        let info = serde_json::json!({
            "Config": {"Image": "x"},
            "HostConfig": {"Binds": null, "PortBindings": {}},
            "Mounts": [{"Type": "volume", "Name": "pg", "Destination": "/var/lib/pg", "RW": true}, {"Type": "bind", "Source": "/nonexistent/x", "Destination": "/x", "RW": true}, {"Type": "volume", "Name": "ro", "Destination": "/ro", "RW": false}],
            "NetworkSettings": {"Ports": {"80/tcp": [{"HostIp": "0.0.0.0", "HostPort": "8080"}], "443/tcp": null}}
        });
        let v = serde_json::to_value(super::recreate_body(&info, "x").unwrap()).unwrap();
        assert_eq!(v["HostConfig"]["Binds"], serde_json::json!(["pg:/var/lib/pg", "ro:/ro:ro"]));
        assert_eq!(v["HostConfig"]["PortBindings"]["80/tcp"][0]["HostPort"], "8080");
        assert!(v["HostConfig"]["PortBindings"].get("443/tcp").is_none());

        // A bind mount given, and the image's VOLUME as an anonymous volume:
        // the new container mounts that same volume.
        let info = serde_json::json!({
            "Config": {"Image": "postgres"},
            "HostConfig": {"Binds": ["/Users/me/init:/docker-entrypoint-initdb.d"], "Mounts": null},
            "Mounts": [
                {"Type": "bind", "Source": "/Users/me/init", "Destination": "/docker-entrypoint-initdb.d", "RW": true},
                {"Type": "volume", "Name": "0f3a9c", "Destination": "/var/lib/postgresql/data", "RW": true}
            ],
            "NetworkSettings": {}
        });
        let v = serde_json::to_value(super::recreate_body(&info, "x").unwrap()).unwrap();
        assert_eq!(v["HostConfig"]["Binds"], serde_json::json!(["/Users/me/init:/docker-entrypoint-initdb.d", "0f3a9c:/var/lib/postgresql/data"]));
        // Given as a Mount: not twice.
        let info = serde_json::json!({
            "Config": {"Image": "x"},
            "HostConfig": {"Binds": null, "Mounts": [{"Type": "volume", "Source": "pg", "Target": "/data"}]},
            "Mounts": [{"Type": "volume", "Name": "pg", "Destination": "/data", "RW": true}],
            "NetworkSettings": {}
        });
        let v = serde_json::to_value(super::recreate_body(&info, "x").unwrap()).unwrap();
        assert!(v["HostConfig"]["Binds"].is_null(), "{v}");
    }
}

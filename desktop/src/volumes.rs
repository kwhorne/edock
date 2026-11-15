//! A volume's files without a container of the user's: browse them (the
//! Files view on a helper), clone the volume, export it to one file and
//! import it again. A small helper container does the work, with the volume
//! at `/v`: it has no network, is hidden among eDock's own, and removes
//! itself after half an hour. Owners and permissions are kept: tar runs
//! inside it, and its bytes come and go over exec. Any engine.

use std::collections::HashMap;

use bollard::container::LogOutput;
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::query_parameters as q;
use elyra::files::ChosenFile;
use elyra::{command, Ctx, EventBus};
use futures_util::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::engine::Engine;

const HELPER: &str = "app.edock.helper";
const IMAGE: &str = "alpine:latest";
/// Where the volume is, in the helper.
pub const MOUNT: &str = "/v";

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// A volume name as Docker allows them.
pub(crate) fn valid(name: &str) -> Result<&str, String> {
    let ok = !name.is_empty() && name.len() <= 255 && name.as_bytes()[0].is_ascii_alphanumeric() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b));
    if ok { Ok(name) } else { Err(format!("{name:?} isn't a volume's name")) }
}

/// alpine, which the helpers run in, pulled when it isn't here.
pub(crate) async fn ensure_image(docker: &bollard::Docker) -> Result<(), String> {
    if docker.inspect_image(IMAGE).await.is_ok() {
        return Ok(());
    }
    let mut pull = docker.create_image(Some(q::CreateImageOptionsBuilder::default().from_image("alpine").tag("latest").build()), None, None);
    while let Some(p) = pull.next().await {
        p.map_err(|e| format!("couldn't pull alpine for the helper: {e}"))?;
    }
    Ok(())
}

/// The running helper for `volume` (made if there's none): its id, and
/// whether it was made now (so whoever made it removes it when done).
async fn helper(ctx: &Ctx, volume: &str) -> Result<String, String> {
    made_helper(ctx, volume).await.map(|(id, _)| id)
}

async fn made_helper(ctx: &Ctx, volume: &str) -> Result<(String, bool), String> {
    let docker = ctx.get::<Engine>().docker()?;
    docker.inspect_volume(volume).await.map_err(|_| format!("There's no volume {volume}"))?;
    let tag = format!("volume:{volume}");
    let mut filters = HashMap::new();
    filters.insert("label".to_string(), vec![format!("{HELPER}={tag}")]);
    filters.insert("status".to_string(), vec!["running".to_string()]);
    let found = docker.list_containers(Some(q::ListContainersOptionsBuilder::default().filters(&filters).build())).await.map_err(err)?;
    if let Some(id) = found.into_iter().find_map(|c| c.id) {
        return Ok((id, false));
    }
    ensure_image(&docker).await?;
    let body = bollard::models::ContainerCreateBody {
        image: Some(IMAGE.into()),
        // Gone by itself after half an hour.
        cmd: Some(vec!["sleep".into(), "1800".into()]),
        labels: Some([(HELPER.to_string(), tag)].into()),
        host_config: Some(bollard::models::HostConfig {
            binds: Some(vec![format!("{volume}:{MOUNT}")]),
            network_mode: Some("none".into()),
            auto_remove: Some(true),
            ..Default::default()
        }),
        ..Default::default()
    };
    let id = docker.create_container(None::<q::CreateContainerOptions>, body).await.map_err(err)?.id;
    docker.start_container(&id, None::<q::StartContainerOptions>).await.map_err(err)?;
    // Up, not only started: its first command answers. A view of its files
    // asked a moment too soon saw the folder under the volume, not the volume.
    let start = std::time::Instant::now();
    while exec(ctx, &id, &["true"], None, None).await.is_err() {
        if start.elapsed() > std::time::Duration::from_secs(5) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    Ok((id, true))
}

/// Browse a volume's files: a helper with the volume at `/v`, for the
/// Files view (`container_files` and the rest, under `/v`). It removes
/// itself after half an hour; `volume_browse_end` sooner.
#[command]
pub async fn volume_browse(ctx: Ctx, volume: String) -> Result<String, String> {
    helper(&ctx, valid(&volume)?).await
}

/// Done browsing: the volume's helper goes.
#[command]
pub async fn volume_browse_end(ctx: Ctx, volume: String) -> Result<(), String> {
    volume_browse_end_inner(&ctx, valid(&volume)?).await
}

/// Runs `cmd` in `container`: what it writes to stdout goes to `out` (if
/// any), `input` (if any) is its stdin; its stderr is the error when it fails.
async fn exec(
    ctx: &Ctx,
    container: &str,
    cmd: &[&str],
    mut input: Option<tokio::fs::File>,
    mut out: Option<&mut tokio::fs::File>,
) -> Result<(), String> {
    let docker = ctx.get::<Engine>().docker()?;
    let ex = docker
        .create_exec(
            container,
            CreateExecOptions {
                attach_stdin: Some(input.is_some()),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                cmd: Some(cmd.iter().map(|s| (*s).to_owned()).collect()),
                ..Default::default()
            },
        )
        .await
        .map_err(err)?;
    let mut errs = String::new();
    if let StartExecResults::Attached { output: mut stream, input: mut stdin } = docker.start_exec(&ex.id, None).await.map_err(err)? {
        let feed = async {
            if let Some(f) = input.as_mut() {
                let mut buf = vec![0u8; 256 * 1024];
                loop {
                    let n = f.read(&mut buf).await.map_err(err)?;
                    if n == 0 {
                        break;
                    }
                    stdin.write_all(&buf[..n]).await.map_err(err)?;
                }
                stdin.shutdown().await.map_err(err)?;
            }
            Ok::<_, String>(())
        };
        let drain = async {
            while let Some(chunk) = stream.next().await {
                match chunk.map_err(err)? {
                    LogOutput::StdErr { message } => errs.push_str(&String::from_utf8_lossy(&message)),
                    LogOutput::StdOut { message } | LogOutput::Console { message } => {
                        if let Some(o) = out.as_mut() {
                            o.write_all(&message).await.map_err(err)?;
                        }
                    }
                    LogOutput::StdIn { .. } => {}
                }
            }
            Ok::<_, String>(())
        };
        let (a, b) = tokio::join!(feed, drain);
        a?;
        b?;
    }
    // No exit code (the helper died, the engine went): a failure, never a
    // half-written archive taken for a whole one.
    let code = crate::engine::exec_exit_code(&docker, &ex.id).await;
    if code == 0 {
        Ok(())
    } else {
        Err(errs.trim().lines().last().map_or_else(|| format!("exit {code}"), str::to_owned))
    }
}

fn changed(ctx: &Ctx) {
    let _ = ctx.get::<EventBus>().emit("docker:changed", &crate::streams::Changed { kind: "volume".into(), action: "create".into() });
}

/// Copy a volume into a new one, owners and all: `target` mustn't exist
/// yet. Best with the containers that write to `source` stopped.
#[command(can = "edock.manage")]
pub async fn volume_clone(ctx: Ctx, source: String, target: String) -> Result<(), String> {
    let (source, target) = (valid(&source)?.to_owned(), valid(&target)?.to_owned());
    let docker = ctx.get::<Engine>().docker()?;
    if docker.inspect_volume(&target).await.is_ok() {
        return Err(format!("There's a volume {target} already"));
    }
    let labels = docker.inspect_volume(&source).await.map_err(|_| format!("There's no volume {source}"))?.labels;
    // Its labels, but not Compose's: the copy isn't the project's.
    let labels: HashMap<String, String> = labels.into_iter().filter(|(k, _)| !k.starts_with("com.docker.compose.")).collect();
    docker.create_volume(bollard::models::VolumeCreateRequest { name: Some(target.clone()), labels: Some(labels), ..Default::default() }).await.map_err(err)?;
    // One helper with both: the bytes go from volume to volume in the VM,
    // never through the Mac.
    let copied = crate::snapshots::helper(&ctx, vec![format!("{source}:/from:ro"), format!("{target}:/to")], "set -o pipefail; tar -C /from -cf - . | tar -C /to -xpf -".into()).await;
    if let Err(e) = copied {
        let _ = docker.remove_volume(&target, None::<q::RemoveVolumeOptions>).await;
        return Err(format!("couldn't copy {source}: {e}"));
    }
    changed(&ctx);
    Ok(())
}

pub(crate) async fn volume_browse_end_inner(ctx: &Ctx, volume: &str) -> Result<(), String> {
    let docker = ctx.get::<Engine>().docker()?;
    let mut filters = HashMap::new();
    filters.insert("label".to_string(), vec![format!("{HELPER}=volume:{volume}")]);
    for c in docker.list_containers(Some(q::ListContainersOptionsBuilder::default().all(true).filters(&filters).build())).await.map_err(err)? {
        if let Some(id) = c.id {
            let _ = docker.remove_container(&id, Some(q::RemoveContainerOptionsBuilder::default().force(true).build())).await;
        }
    }
    Ok(())
}

/// Save a volume's files to one `.tar` where the user chose, owners kept,
/// to import later or on another Mac.
#[command]
pub async fn volume_export(ctx: Ctx, volume: String, file: ChosenFile) -> Result<(), String> {
    let volume = valid(&volume)?.to_owned();
    let (id, made) = made_helper(&ctx, &volume).await?;
    let mut out = tokio::fs::File::create(file.path()).await.map_err(|e| format!("{}: {e}", file.name()))?;
    let r = exec(&ctx, &id, &["tar", "-C", MOUNT, "-cf", "-", "."], None, Some(&mut out)).await;
    let _ = out.flush().await;
    if made {
        let _ = volume_browse_end_inner(&ctx, &volume).await;
    }
    if r.is_err() {
        let _ = tokio::fs::remove_file(file.path()).await;
    }
    r.map_err(|e| format!("couldn't export {volume}: {e}"))
}

/// Make a new volume from a `.tar` (as Export writes them, or any tar of
/// a folder's contents). `volume` mustn't exist yet.
#[command]
pub async fn volume_import(ctx: Ctx, volume: String, file: ChosenFile) -> Result<(), String> {
    let volume = valid(&volume)?.to_owned();
    let docker = ctx.get::<Engine>().docker()?;
    if docker.inspect_volume(&volume).await.is_ok() {
        return Err(format!("There's a volume {volume} already: pick another name"));
    }
    let input = tokio::fs::File::open(file.path()).await.map_err(|e| format!("{}: {e}", file.name()))?;
    docker.create_volume(bollard::models::VolumeCreateRequest { name: Some(volume.clone()), ..Default::default() }).await.map_err(err)?;
    let r = match helper(&ctx, &volume).await {
        Ok(id) => {
            let r = exec(&ctx, &id, &["tar", "-C", MOUNT, "-xpf", "-"], Some(input), None).await;
            let _ = volume_browse_end_inner(&ctx, &volume).await;
            r
        }
        Err(e) => Err(e),
    };
    if let Err(e) = r {
        let _ = docker.remove_volume(&volume, None::<q::RemoveVolumeOptions>).await;
        return Err(format!("couldn't import {}: {e}", file.name()));
    }
    changed(&ctx);
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn volume_names() {
        assert!(super::valid("myapp_pgdata").is_ok());
        for bad in ["", "-x", "a/b", "../x", ".x"] {
            assert!(super::valid(bad).is_err(), "{bad}");
        }
    }
}

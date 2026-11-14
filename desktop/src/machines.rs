//! Linux machines (`edock machine`): long-lived Linux environments on eDock's
//! engine, with the Mac user's name and uid, passwordless sudo, and `/Users`
//! at the same path. A machine is the container `machine-<name>`, labelled
//! `app.edock.machine=<name>`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use bollard::query_parameters as q;
use elyra::{command, Ctx, EventBus};
use serde::{Deserialize, Serialize};

use crate::engine::{Engine, EDOCK};

const LABEL: &str = "app.edock.machine";

/// The distributions `edock machine create --distro` knows, and their images.
pub const DISTROS: [(&str, &str, &str); 7] = [
    ("ubuntu", "Ubuntu 24.04", "ubuntu:24.04"),
    ("debian", "Debian 13", "debian:trixie"),
    ("alpine", "Alpine 3.22", "alpine:3.22"),
    ("fedora", "Fedora 42", "fedora:42"),
    ("arch", "Arch Linux", "archlinux:latest"),
    ("rocky", "Rocky Linux 9", "rockylinux:9"),
    ("opensuse", "openSUSE Tumbleweed", "opensuse/tumbleweed:latest"),
];

/// `edock machine create` is running.
#[derive(Default)]
pub struct Creating(AtomicBool);

fn on_edock(ctx: &Ctx) -> Result<(), String> {
    if ctx.get::<Engine>().context().0 == EDOCK {
        Ok(())
    } else {
        Err("Linux machines run on eDock's engine: switch to it in the toolbar".into())
    }
}

/// A machine name, as `edock machine` takes it: letters, digits, dashes.
fn valid(name: &str) -> Result<&str, String> {
    if !name.is_empty() && name.len() <= 48 && !name.starts_with('-') && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        Ok(name)
    } else {
        Err("A machine's name is letters, digits and dashes".into())
    }
}

fn container(name: &str) -> String {
    format!("machine-{name}")
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct Distro {
    pub id: String,
    pub label: String,
}

#[command]
pub async fn machine_distros(_ctx: Ctx) -> Vec<Distro> {
    DISTROS.iter().map(|(id, label, _)| Distro { id: (*id).into(), label: (*label).into() }).collect()
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct MachineRow {
    pub name: String,
    /// The distribution's name when it's one of ours, else the image.
    pub distro: String,
    pub image: String,
    pub state: String,
    pub status: String,
    /// Runs systemd as its init.
    pub systemd: bool,
    pub created: f64,
}

#[command]
pub async fn machines_list(ctx: Ctx) -> Result<Vec<MachineRow>, String> {
    on_edock(&ctx)?;
    let docker = ctx.get::<Engine>().docker()?;
    let mut filters = HashMap::new();
    filters.insert("label".to_string(), vec![LABEL.to_string()]);
    let list = docker
        .list_containers(Some(q::ListContainersOptionsBuilder::default().all(true).filters(&filters).build()))
        .await
        .map_err(|e| e.to_string())?;
    let mut rows: Vec<MachineRow> = list
        .iter()
        .map(|c| {
            let v = serde_json::to_value(c).unwrap_or_default();
            let image = v["Image"].as_str().unwrap_or("").to_owned();
            let distro = DISTROS.iter().find(|(_, _, i)| *i == image).map_or_else(|| image.clone(), |(_, l, _)| (*l).to_owned());
            MachineRow {
                name: v["Labels"][LABEL].as_str().unwrap_or("").into(),
                systemd: v["Command"].as_str().is_some_and(|c| c.contains("systemd")),
                state: v["State"].as_str().unwrap_or("").into(),
                status: v["Status"].as_str().unwrap_or("").into(),
                created: v["Created"].as_f64().unwrap_or(0.0),
                distro,
                image,
            }
        })
        .collect();
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(rows)
}

/// Make a machine: `edock machine create`, its lines on `machine:progress`.
#[command]
pub async fn machine_create(ctx: Ctx, name: String, distro: String, systemd: bool) -> Result<(), String> {
    on_edock(&ctx)?;
    valid(&name)?;
    if !DISTROS.iter().any(|(id, _, _)| *id == distro) {
        return Err(format!("{distro} isn't a distribution eDock knows"));
    }
    let bin = crate::daemon::engine_binary().ok_or("The eDock engine (edock) isn't installed")?;
    let creating = ctx.get::<Creating>();
    if creating.0.swap(true, Ordering::SeqCst) {
        return Err("A machine is being made already".into());
    }
    let mut args = vec!["machine".to_string(), "create".into(), name, "--distro".into(), distro];
    if systemd {
        args.push("--systemd".into());
    }
    let c2 = creating.clone();
    let bus = ctx.get::<EventBus>();
    let b2 = bus.clone();
    let started = crate::daemon::run_edock(&bin, args, bus, "machine:progress", move || {
        c2.0.store(false, Ordering::SeqCst);
        let _ = b2.emit("docker:changed", &crate::streams::Changed { kind: "container".into(), action: "create".into() });
    });
    if started.is_err() {
        creating.0.store(false, Ordering::SeqCst);
    }
    started
}

/// start, stop, restart or delete a machine. Delete removes it and what's
/// only in it (its own files, not `/Users`).
#[command]
pub async fn machine_action(ctx: Ctx, name: String, action: String) -> Result<(), String> {
    on_edock(&ctx)?;
    let id = container(valid(&name)?);
    let docker = ctx.get::<Engine>().docker()?;
    match action.as_str() {
        "start" => docker.start_container(&id, None::<q::StartContainerOptions>).await,
        "stop" => docker.stop_container(&id, None::<q::StopContainerOptions>).await,
        "restart" => docker.restart_container(&id, None::<q::RestartContainerOptions>).await,
        "delete" => docker.remove_container(&id, Some(q::RemoveContainerOptionsBuilder::default().force(true).v(true).build())).await,
        other => return Err(format!("Unknown action {other}")),
    }
    .map_err(|e| e.to_string())
}

/// The Mac user, as machines have them: name, and home under /Users.
fn mac_user() -> (String, String) {
    // SAFETY: getuid cannot fail.
    let uid = unsafe { libc::getuid() };
    let name = if uid == 0 { "root".to_owned() } else { std::env::var("USER").ok().filter(|u| !u.is_empty()).unwrap_or_else(|| "user".to_owned()) };
    let home = if name == "root" { "/root".to_owned() } else { format!("/Users/{name}") };
    (name, home)
}

/// A login shell in a machine, as the Mac user, in their home: on
/// `exec:<id>` like any terminal.
#[command]
pub async fn machine_shell(ctx: Ctx, name: String, cols: u32, rows: u32) -> Result<u32, String> {
    on_edock(&ctx)?;
    let id = container(valid(&name)?);
    let (user, home) = mac_user();
    let cmd = vec!["sh".to_owned(), "-c".into(), "exec $(command -v bash || command -v sh) -l".into()];
    let how = crate::streams::Exec { user: Some(user), working_dir: Some(home.clone()), env: vec![format!("HOME={home}")] };
    crate::streams::open_terminal_as(&ctx, &id, cmd, cols, rows, how).await
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_are_what_edock_machine_takes() {
        for ok in ["dev", "web-2", "Ubuntu24"] {
            assert!(super::valid(ok).is_ok(), "{ok}");
        }
        for bad in ["", "-x", "a b", "a/b", "a.b", "a_b", "--systemd"] {
            assert!(super::valid(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_distros_match_the_engines() {
        let engine = include_str!("../../engine/crates/edock/src/machine.rs");
        for (id, _, image) in super::DISTROS {
            assert!(engine.contains(&format!("(\"{id}\", \"{image}\")")), "{id} {image}");
        }
    }
}

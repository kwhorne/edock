//! Starting and stopping eDock's own engine (`edock daemon`) from the app.

use std::path::PathBuf;
use std::process::Stdio;

use elyra::{command, Ctx};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::engine::{edock_host, Engine, EDOCK};


/// The engine process this app started, if it did.
#[derive(Default)]
pub struct Daemon {
    child: Mutex<Option<std::process::Child>>,
}

/// Where the `edock` engine binary is: `$EDOCK_ENGINE`, beside the app's
/// own binary (inside eDock.app), `~/.edock/bin/edock`, or a checkout's
/// `engine/target/release/edock` when run with cargo.
pub fn engine_binary() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = vec![];
    if let Some(p) = std::env::var_os("EDOCK_ENGINE") {
        candidates.push(p.into());
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("edock"));
        }
    }
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".edock/bin/edock"));
    }
    let checkout = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../engine/target/release/edock");
    candidates.push(checkout);
    candidates.into_iter().find(|p| p.is_file())
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct DaemonInfo {
    /// The engine binary, when the app found one.
    pub binary: Option<String>,
    /// The engine's socket answers.
    pub running: bool,
    /// This app started it (and can stop it).
    pub ours: bool,
}

async fn alive() -> bool {
    match bollard::Docker::connect_with_unix(&edock_host(), 5, bollard::API_DEFAULT_VERSION) {
        Ok(d) => d.ping().await.is_ok(),
        Err(_) => false,
    }
}

#[command]
pub async fn daemon_info(ctx: Ctx) -> DaemonInfo {
    let ours = {
        let daemon = ctx.get::<Daemon>();
        let mut child = daemon.child.lock();
        if let Some(c) = child.as_mut() {
            if !matches!(c.try_wait(), Ok(None)) {
                *child = None;
            }
        }
        child.is_some()
    };
    DaemonInfo { binary: engine_binary().map(|p| p.display().to_string()), running: alive().await, ours }
}

/// Start eDock's engine, and switch the app to it once it answers.
#[command]
pub async fn daemon_start(ctx: Ctx) -> Result<(), String> {
    if alive().await {
        return ctx.get::<Engine>().switch(EDOCK);
    }
    let bin = engine_binary().ok_or("The eDock engine (edock) isn't installed. Build it with `cargo xtask host` in engine/, or set EDOCK_ENGINE.")?;
    let log_dir = dirs::home_dir().unwrap_or_default().join(".edock");
    let _ = std::fs::create_dir_all(&log_dir);
    let log = std::fs::OpenOptions::new().create(true).append(true).open(log_dir.join("desktop-engine.log")).map_err(|e| e.to_string())?;
    let child = std::process::Command::new(&bin)
        .arg("daemon")
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log)
        .spawn()
        .map_err(|e| format!("Couldn't start {}: {e}", bin.display()))?;
    *ctx.get::<Daemon>().child.lock() = Some(child);
    // The socket comes up at once; the VM boots on first use.
    for _ in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if alive().await {
            return ctx.get::<Engine>().switch(EDOCK);
        }
    }
    Err("The engine started but its socket didn't answer within 10 s. See ~/.edock/desktop-engine.log.".into())
}

/// Stop the engine this app started.
#[command]
pub async fn daemon_stop(ctx: Ctx) -> Result<(), String> {
    let child = ctx.get::<Daemon>().child.lock().take();
    let Some(mut child) = child else {
        return Err("eDock didn't start this engine, so it leaves it running.".into());
    };
    // SIGTERM: the engine shuts its VM down cleanly.
    unsafe_kill(child.id());
    for _ in 0..100 {
        if !matches!(child.try_wait(), Ok(None)) {
            ctx.get::<Engine>().reset();
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let _ = child.kill();
    ctx.get::<Engine>().reset();
    Ok(())
}

fn unsafe_kill(pid: u32) {
    let _ = std::process::Command::new("kill").arg("-TERM").arg(pid.to_string()).status();
}

impl Drop for Daemon {
    fn drop(&mut self) {
        if let Some(c) = self.child.get_mut().as_mut() {
            unsafe_kill(c.id());
        }
    }
}

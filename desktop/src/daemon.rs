//! eDock's own engine (`edock daemon`): running it, and turning it on and
//! off from the app.
//!
//! Stop is the engine's own: `POST /edock/stop` powers the VM off and keeps
//! it off (across engine restarts too) until `POST /edock/wake`. The daemon
//! process stays, at about 15 MB, and answers that it's stopped. So Stop
//! works on any eDock engine, however it was started: by this app, an
//! earlier run of it, the LaunchAgent from `edock setup`, or a terminal.
//! The app only starts the daemon process when none runs.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use elyra::{command, Ctx};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

use elyra::EventBus;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use crate::engine::{edock_host, Engine, EDOCK};

/// eDock's engine as this app runs it. A daemon the app starts is reaped
/// by a thread of its own when it exits, so it never lingers as a zombie
/// (which `kill(pid, 0)` would still find, and Restart would wait on).
#[derive(Default)]
pub struct Daemon;

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

fn socket() -> PathBuf {
    PathBuf::from(edock_host().trim_start_matches("unix://"))
}

/// One request to eDock's socket, for its own endpoints (`/edock/…`) and
/// the fields of `/info` a Docker client library drops (`EdockStopped`).
/// `None` when nothing answers.
pub(crate) async fn engine_request(method: &str, path: &str) -> Option<(u16, Value)> {
    engine_call(method, path, None).await
}

/// `engine_request` with a JSON body.
pub(crate) async fn engine_call(method: &str, path: &str, body: Option<&Value>) -> Option<(u16, Value)> {
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    let fut = async {
        let mut s = tokio::net::UnixStream::connect(socket()).await.ok()?;
        // A look (GET) isn't use: it mustn't keep the VM from sleeping.
        let background = if method == "GET" { "X-Edock-Background: 1\r\n" } else { "" };
        let kind = if body.is_empty() { "" } else { "Content-Type: application/json\r\n" };
        let req = format!(
            "{method} {path} HTTP/1.1\r\nHost: edock\r\n{background}{kind}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        s.write_all(req.as_bytes()).await.ok()?;
        // The engine keeps the connection open: read the head, then the
        // body by its length or its chunks, not to the end.
        let mut r = BufReader::new(s);
        let mut status_line = String::new();
        r.read_line(&mut status_line).await.ok()?;
        let status: u16 = status_line.split_whitespace().nth(1)?.parse().ok()?;
        let (mut length, mut chunked) = (None, false);
        loop {
            let mut line = String::new();
            if r.read_line(&mut line).await.ok()? == 0 || line == "\r\n" {
                break;
            }
            let lower = line.to_ascii_lowercase();
            if let Some(v) = lower.strip_prefix("content-length:") {
                length = v.trim().parse::<usize>().ok();
            } else if lower.starts_with("transfer-encoding:") && lower.contains("chunked") {
                chunked = true;
            }
        }
        let mut body = Vec::new();
        if status == 204 || status == 304 || (100..200).contains(&status) {
            // No body, by definition.
        } else if chunked {
            loop {
                let mut size = String::new();
                r.read_line(&mut size).await.ok()?;
                let n = usize::from_str_radix(size.trim(), 16).ok()?;
                if n == 0 {
                    break;
                }
                let mut chunk = vec![0; n + 2];
                r.read_exact(&mut chunk).await.ok()?;
                body.extend_from_slice(&chunk[..n]);
            }
        } else if let Some(n) = length {
            body.resize(n, 0);
            r.read_exact(&mut body).await.ok()?;
        } else {
            // Neither: the body ends with the connection.
            r.read_to_end(&mut body).await.ok()?;
        }
        Some((status, serde_json::from_slice(&body).unwrap_or(Value::Null)))
    };
    // Reads answer at once; Stop waits for the VM to power off (up to 10 s
    // in the engine). The limit is for an engine that hangs.
    let limit = if method == "GET" { 10 } else { 40 };
    tokio::time::timeout(Duration::from_secs(limit), fut).await.ok().flatten()
}

/// eDock's engine as the app sees it.
#[derive(Serialize, Deserialize, specta::Type, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum EngineState {
    /// No daemon answers on the socket.
    Off,
    /// Turned off with Stop: the daemon answers, the VM stays off until Start.
    Stopped,
    /// Running, its VM asleep (it wakes on the next request).
    Asleep,
    /// Running, its VM up.
    Running,
}

pub(crate) async fn engine_state() -> EngineState {
    match engine_request("GET", "/info").await {
        None => EngineState::Off,
        Some((_, info)) if info["EdockStopped"] == true => EngineState::Stopped,
        Some((_, info)) if info["EdockVmAwake"] == false => EngineState::Asleep,
        Some(_) => EngineState::Running,
    }
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct DaemonInfo {
    /// The engine binary, when the app found one.
    pub binary: Option<String>,
    pub state: EngineState,
}

#[command]
pub async fn daemon_info(_ctx: Ctx) -> DaemonInfo {
    DaemonInfo { binary: engine_binary().map(|p| p.display().to_string()), state: engine_state().await }
}

/// Start eDock's engine: run the daemon if none runs, turn it on if it was
/// stopped, wake its VM, and switch the app to it.
#[command]
pub async fn daemon_start(ctx: Ctx) -> Result<(), String> {
    if engine_state().await == EngineState::Off {
        spawn(&ctx)?;
        let mut up = false;
        for _ in 0..100 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if engine_state().await != EngineState::Off {
                up = true;
                break;
            }
        }
        if !up {
            return Err("The engine started but its socket didn't answer within 10 s. See ~/.edock/desktop-engine.log.".into());
        }
    }
    // Wake: no longer stopped (if it was), and the VM up.
    match engine_request("POST", "/edock/wake").await {
        Some((s, _)) if s < 300 => {}
        Some((s, body)) => return Err(body["message"].as_str().map(str::to_owned).unwrap_or_else(|| format!("The engine said {s}"))),
        None => return Err("The engine didn't answer".into()),
    }
    let engine = ctx.get::<Engine>();
    engine.reset();
    engine.switch(EDOCK)
}

fn spawn(_ctx: &Ctx) -> Result<(), String> {
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
    let mut child = child;
    std::thread::Builder::new()
        .name("edock-daemon-reaper".into())
        .spawn(move || {
            let _ = child.wait();
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// The engine process behind eDock's socket: the peer of a connection to
/// it (`LOCAL_PEERPID`), so it's that process, however it was started.
pub(crate) fn engine_pid() -> Option<i32> {
    use std::os::fd::AsRawFd;
    let s = std::os::unix::net::UnixStream::connect(socket()).ok()?;
    let mut pid: libc::pid_t = 0;
    let mut len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    // SAFETY: a valid socket, and a pid_t-sized buffer with its length.
    let rc = unsafe { libc::getsockopt(s.as_raw_fd(), libc::SOL_LOCAL, libc::LOCAL_PEERPID, (&mut pid as *mut libc::pid_t).cast(), &mut len) };
    (rc == 0 && pid > 0).then_some(pid)
}

/// Run the engine again from this app's `edock`: after an update, the
/// engine that ran keeps the old version until it starts again. A stopped
/// engine stays stopped; a running one comes back running.
#[command]
pub async fn daemon_restart(ctx: Ctx) -> Result<(), String> {
    let was = engine_state().await;
    if was != EngineState::Off {
        let pid = engine_pid().ok_or("Couldn't tell which process the engine is")?;
        // SIGTERM: the engine stops its VM cleanly and exits.
        // SAFETY: sending a signal to a pid.
        unsafe { libc::kill(pid, libc::SIGTERM) };
        let mut gone = false;
        for _ in 0..300 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            // SAFETY: signal 0 only asks whether the process is there.
            if unsafe { libc::kill(pid, 0) } != 0 {
                gone = true;
                break;
            }
        }
        if !gone {
            return Err("The engine didn't stop within 30 s".into());
        }
    }
    spawn(&ctx)?;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if engine_state().await != EngineState::Off {
            break;
        }
    }
    if was != EngineState::Stopped {
        let _ = engine_request("POST", "/edock/wake").await;
    }
    let engine = ctx.get::<Engine>();
    engine.reset();
    let _ = ctx.get::<EventBus>().emit("docker:changed", &crate::streams::Changed { kind: "engine".into(), action: "restarted".into() });
    engine.switch(EDOCK)
}

/// Stop eDock's engine: its containers stop and its VM powers off, until
/// Start. Works whoever started the engine.
#[command]
pub async fn daemon_stop(ctx: Ctx) -> Result<(), String> {
    let result = match engine_request("POST", "/edock/stop").await {
        Some((s, _)) if s < 300 => Ok(()),
        Some((s, body)) => Err(body["message"].as_str().map(str::to_owned).unwrap_or_else(|| format!("The engine said {s}"))),
        None => Err("eDock's engine isn't running".into()),
    };
    let _ = ctx.get::<elyra::EventBus>().emit("docker:changed", &crate::streams::Changed { kind: "engine".into(), action: "stopped".into() });
    result
}

// ── `edock …` commands, with their output as progress ──────────────────

/// A line of an `edock` command's output, on its progress channel; the
/// last has `done`, and `error` when it failed.
#[derive(Serialize, Deserialize, specta::Type, Clone)]
pub struct Progress {
    pub line: String,
    pub done: bool,
    pub error: Option<String>,
}

/// Run `edock <args>` in the background: each line of its output on
/// `channel`, then a last `done` (with its error), then `finished()`.
pub(crate) fn run_edock(
    bin: &std::path::Path,
    args: Vec<String>,
    bus: Arc<EventBus>,
    channel: &'static str,
    finished: impl FnOnce() + Send + 'static,
) -> Result<(), String> {
    let mut child = tokio::process::Command::new(bin)
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| format!("Couldn't run edock {}: {e}", args.join(" ")))?;
    let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
    tokio::spawn(async move {
        // stderr's last line is the error, when it fails.
        let tail = Arc::new(Mutex::new(String::new()));
        let t2 = tail.clone();
        let err_task = tokio::spawn(async move {
            if let Some(e) = stderr {
                let mut lines = BufReader::new(e).lines();
                while let Ok(Some(l)) = lines.next_line().await {
                    if !l.trim().is_empty() {
                        *t2.lock() = l;
                    }
                }
            }
        });
        if let Some(o) = stdout {
            let mut lines = BufReader::new(o).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = bus.emit(channel, &Progress { line, done: false, error: None });
            }
        }
        let _ = err_task.await;
        let error = match child.wait().await {
            Ok(s) if s.success() => None,
            Ok(_) => Some(tail.lock().trim_start_matches("edock: ").to_owned()).filter(|e| !e.is_empty()).or(Some("edock failed".into())),
            Err(e) => Some(e.to_string()),
        };
        let _ = bus.emit(channel, &Progress { line: String::new(), done: true, error });
        finished();
    });
    Ok(())
}

// ── The command line: `docker` and `edock` in the terminal ─────────────

#[derive(Serialize, Deserialize, specta::Type)]
pub struct CliStatus {
    /// `~/.edock/bin/docker` is there (`edock setup` linked it).
    pub docker: bool,
    /// The `edock` docker context exists.
    pub context: bool,
    /// The LaunchAgent that starts the engine at login.
    pub agent: bool,
    /// `edock setup` is running.
    pub busy: bool,
    /// Another `edock` comes first on the login shell's PATH (an older copy
    /// in /usr/local/bin, ~/.local/bin…): terminals run it, not this one.
    pub stale: Option<String>,
}

/// The first `edock` on the login shell's PATH, when it isn't the app's:
/// what makes an update seem not to have happened in a terminal. The login
/// shell's PATH, since the app's own has only the system's. Reported, never
/// removed. None when no `edock` is on PATH at all.
/// The shell runs at most once a minute (the Engine page asks on every
/// change), and is killed when it takes too long.
static STALE_LAST: parking_lot::Mutex<Option<(std::time::Instant, Option<String>)>> = parking_lot::Mutex::new(None);

async fn stale_edock() -> Option<String> {
    if let Some((at, found)) = STALE_LAST.lock().as_ref() {
        if at.elapsed() < Duration::from_secs(60) {
            return found.clone();
        }
    }
    let found = login_shell_edock().await;
    *STALE_LAST.lock() = Some((std::time::Instant::now(), found.clone()));
    found
}

async fn login_shell_edock() -> Option<String> {
    let ours = std::fs::canonicalize(engine_binary()?).ok()?;
    let shell = std::env::var("SHELL").ok().filter(|s| s.starts_with('/')).unwrap_or_else(|| "/bin/zsh".into());
    let run = tokio::process::Command::new(&shell)
        .args(["-ilc", "command -v edock"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let out = tokio::time::timeout(Duration::from_secs(5), run).await.ok()?.ok()?;
    let found = String::from_utf8_lossy(&out.stdout).lines().rev().map(str::trim).find(|l| l.starts_with('/'))?.to_owned();
    stale_unless_ours(&found, &ours)
}

fn stale_unless_ours(found: &str, ours: &std::path::Path) -> Option<String> {
    match std::fs::canonicalize(found) {
        Ok(real) if real == ours => None,
        // ~/.edock/bin/edock is the setup's link to an app: one that moved
        // is set up again by Set up again, and named here too.
        _ => Some(found.to_owned()),
    }
}

/// `edock setup` is running.
#[derive(Default)]
pub struct Setup {
    busy: std::sync::atomic::AtomicBool,
}

#[command]
pub async fn cli_status(ctx: Ctx) -> CliStatus {
    let home = dirs::home_dir().unwrap_or_default();
    let docker_dir = std::env::var_os("DOCKER_CONFIG").map(PathBuf::from).unwrap_or_else(|| home.join(".docker"));
    let context = std::fs::read_dir(docker_dir.join("contexts/meta"))
        .map(|d| {
            d.flatten().any(|e| {
                std::fs::read_to_string(e.path().join("meta.json"))
                    .ok()
                    .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                    .is_some_and(|m| m["Name"] == EDOCK)
            })
        })
        .unwrap_or(false);
    CliStatus {
        docker: home.join(".edock/bin/docker").exists(),
        context,
        agent: home.join("Library/LaunchAgents/app.edock.daemon.plist").is_file(),
        busy: ctx.get::<Setup>().busy.load(std::sync::atomic::Ordering::SeqCst),
        stale: stale_edock().await,
    }
}

// ── Which builder `docker build` uses (`edock build-with`) ─────────────

#[derive(Serialize, Deserialize, specta::Type)]
pub struct BuildWith {
    /// docker (builds follow docker's context) or edock (every build on eDock).
    pub with: String,
    /// Another engine on this Mac (OrbStack, Docker Desktop…), if any.
    pub engine: Option<String>,
    /// eDock's builder is made, so "edock" takes effect.
    pub ready: bool,
}

async fn build_with_run(args: &[&str]) -> Result<BuildWith, String> {
    let bin = engine_binary().ok_or("The eDock engine (edock) isn't installed")?;
    let out = tokio::process::Command::new(bin).arg("build-with").args(args).arg("--json").output().await.map_err(|e| e.to_string())?;
    if !out.status.success() {
        let e = String::from_utf8_lossy(&out.stderr);
        return Err(e.trim().lines().last().unwrap_or("edock build-with failed").trim_start_matches("edock: ").to_owned());
    }
    let v: Value = serde_json::from_slice(&out.stdout).map_err(|_| "This eDock engine can't say which builder docker uses: update eDock".to_string())?;
    Ok(BuildWith { with: v["with"].as_str().unwrap_or("docker").into(), engine: v["engine"].as_str().map(str::to_owned), ready: v["ready"] == true })
}

/// Which builder plain `docker build` and `docker compose build` use.
#[command]
pub async fn build_with(_ctx: Ctx) -> Result<BuildWith, String> {
    build_with_run(&[]).await
}

/// Send every `docker build` to eDock ("edock"), or let builds follow
/// docker's context ("docker", which puts back what eDock changed).
#[command]
pub async fn build_with_set(_ctx: Ctx, choice: String) -> Result<BuildWith, String> {
    if choice != "docker" && choice != "edock" {
        return Err("docker or edock".into());
    }
    build_with_run(&[&choice]).await
}

/// `edock setup`: the `edock` docker context, `docker` and `edock` on PATH
/// (the docker CLI, Compose and buildx downloaded the first time, checked
/// against their SHA-256), the LaunchAgent and the build builder. Output on
/// `setup:progress`. `use_context` makes `edock` docker's current context.
/// `*.edock.local` names (which need an administrator) are left to
/// `edock setup --domains`.
#[command]
pub async fn cli_setup(ctx: Ctx, use_context: bool) -> Result<(), String> {
    let bin = engine_binary().ok_or("The eDock engine (edock) isn't installed")?;
    let setup = ctx.get::<Setup>();
    if setup.busy.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Err("Setting up already".into());
    }
    let mut args = vec!["setup".to_string(), "--no-domains".into()];
    if use_context {
        args.push("--use".into());
    }
    let s2 = setup.clone();
    let started = run_edock(&bin, args, ctx.get::<EventBus>(), "setup:progress", move || {
        s2.busy.store(false, std::sync::atomic::Ordering::SeqCst);
        // Setup may have changed which `edock` the shell finds.
        *STALE_LAST.lock() = None;
    });
    if started.is_err() {
        setup.busy.store(false, std::sync::atomic::Ordering::SeqCst);
    }
    started
}

/// A streamed answer from eDock's own API (`/edock/images/scan`): each JSON
/// line as it comes, then the status. For work that takes minutes (the
/// first scan downloads its scanner), so no time limit.
pub(crate) async fn engine_stream(method: &str, path: &str, mut each: impl FnMut(Value) + Send) -> Result<u16, String> {
    let mut s = tokio::net::UnixStream::connect(socket()).await.map_err(|_| "eDock's engine isn't running".to_string())?;
    let req = format!("{method} {path} HTTP/1.1\r\nHost: edock\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).await.map_err(|e| e.to_string())?;
    let mut r = BufReader::new(s);
    let mut status_line = String::new();
    r.read_line(&mut status_line).await.map_err(|e| e.to_string())?;
    let status: u16 = status_line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).ok_or("The engine's answer made no sense")?;
    let mut chunked = false;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).await.map_err(|e| e.to_string())? == 0 || line == "\r\n" {
            break;
        }
        if line.to_ascii_lowercase().starts_with("transfer-encoding:") && line.to_ascii_lowercase().contains("chunked") {
            chunked = true;
        }
    }
    // Chunks carry lines, but a line may span chunks: keep the rest.
    let mut pending: Vec<u8> = Vec::new();
    let flush = |pending: &mut Vec<u8>, each: &mut dyn FnMut(Value)| {
        while let Some(nl) = pending.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = pending.drain(..=nl).collect();
            if let Ok(v) = serde_json::from_slice::<Value>(&line) {
                each(v);
            }
        }
    };
    if chunked {
        loop {
            let mut size = String::new();
            if r.read_line(&mut size).await.map_err(|e| e.to_string())? == 0 {
                break;
            }
            let n = usize::from_str_radix(size.trim(), 16).map_err(|_| "The engine's answer made no sense".to_string())?;
            if n == 0 {
                break;
            }
            let mut chunk = vec![0; n + 2];
            r.read_exact(&mut chunk).await.map_err(|e| e.to_string())?;
            pending.extend_from_slice(&chunk[..n]);
            flush(&mut pending, &mut each);
        }
    } else {
        r.read_to_end(&mut pending).await.map_err(|e| e.to_string())?;
        pending.push(b'\n');
        flush(&mut pending, &mut each);
    }
    Ok(status)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_apps_own_edock_isnt_stale() {
        let dir = std::env::temp_dir().join(format!("edock-stale-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ours = dir.join("edock");
        std::fs::write(&ours, "").unwrap();
        let link = dir.join("link");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&ours, &link).unwrap();
        let ours = std::fs::canonicalize(&ours).unwrap();
        assert_eq!(super::stale_unless_ours(link.to_str().unwrap(), &ours), None);
        assert_eq!(super::stale_unless_ours("/usr/local/bin/nope-edock", &ours).as_deref(), Some("/usr/local/bin/nope-edock"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

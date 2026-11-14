//! What streams from the engine to the window: a container's log, its
//! stats, a terminal in it, and the engine's events (so the lists change
//! the moment a container does, with no polling).
//!
//! Each stream the page opens gets an id and its own channel
//! (`logs:<id>`, `stats:<id>`, `exec:<id>`); `stream_stop` ends it.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bollard::container::LogOutput;
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::query_parameters as q;
use elyra::{command, Ctx, EventBus};
use futures_util::StreamExt;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

use crate::engine::Engine;


#[derive(Default)]
pub struct Streams {
    next: AtomicU64,
    tasks: Mutex<HashMap<u64, tokio::task::AbortHandle>>,
    /// A terminal's keystrokes and size, by its stream id.
    terms: Mutex<HashMap<u64, Term>>,
}

struct Term {
    input: mpsc::UnboundedSender<Vec<u8>>,
    exec_id: String,
}

impl Streams {
    fn id(&self) -> u64 {
        self.next.fetch_add(1, Ordering::SeqCst) + 1
    }
    fn keep(&self, id: u64, handle: tokio::task::AbortHandle) {
        self.tasks.lock().insert(id, handle);
    }
    fn done(&self, id: u64) {
        self.tasks.lock().remove(&id);
        self.terms.lock().remove(&id);
    }
}

/// End a stream the page opened.
#[command]
pub async fn stream_stop(ctx: Ctx, id: u32) {
    let streams = ctx.get::<Streams>();
    if let Some(h) = streams.tasks.lock().remove(&(id as u64)) {
        h.abort();
    }
    streams.terms.lock().remove(&(id as u64));
}

// ── Logs ───────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, specta::Type, Clone)]
pub struct LogLine {
    /// "out" or "err".
    pub stream: String,
    pub text: String,
}

#[derive(Serialize, Deserialize, specta::Type, Clone)]
pub struct LogBatch {
    pub lines: Vec<LogLine>,
    pub ended: bool,
}

/// Follow a container's log, from its last `tail` lines. Lines arrive on
/// `logs:<id>` in batches.
#[command]
pub async fn logs_follow(ctx: Ctx, container: String, tail: u32, timestamps: bool) -> Result<u32, String> {
    let docker = ctx.get::<Engine>().docker()?;
    let o = q::LogsOptionsBuilder::default()
        .follow(true)
        .stdout(true)
        .stderr(true)
        .timestamps(timestamps)
        .tail(&tail.to_string())
        .build();
    let logs = docker.logs(&container, Some(o));
    Ok(follow_lines(&ctx, logs) as u32)
}

/// Send what a stream of output says, line by line, in batches on
/// `logs:<id>`: a container's log, or `kubectl logs -f`'s.
pub(crate) fn follow_lines<S>(ctx: &Ctx, mut logs: S) -> u64
where
    S: futures_util::Stream<Item = Result<LogOutput, bollard::errors::Error>> + Send + Unpin + 'static,
{
    let bus = ctx.get::<EventBus>();
    let streams = ctx.get::<Streams>();
    let id = streams.id();
    let channel = format!("logs:{id}");
    let s2 = streams.clone();
    let task = tokio::spawn(async move {
        let mut pending: Vec<LogLine> = vec![];
        // A line may come in pieces, and a character across two reads:
        // each stream keeps its unfinished end, as bytes.
        let mut partial: [Vec<u8>; 2] = [vec![], vec![]];
        const NAMES: [&str; 2] = ["out", "err"];
        let mut tick = tokio::time::interval(Duration::from_millis(60));
        loop {
            tokio::select! {
                item = logs.next() => match item {
                    Some(Ok(out)) => {
                        let (i, bytes) = match out {
                            LogOutput::StdErr { message } => (1, message),
                            LogOutput::StdOut { message } | LogOutput::Console { message } => (0, message),
                            LogOutput::StdIn { .. } => continue,
                        };
                        partial[i].extend_from_slice(&bytes);
                        while let Some(nl) = partial[i].iter().position(|&b| b == b'\n') {
                            let line: Vec<u8> = partial[i].drain(..=nl).collect();
                            let text = String::from_utf8_lossy(&line).trim_end_matches(['\n', '\r']).to_owned();
                            pending.push(LogLine { stream: NAMES[i].into(), text });
                        }
                        if pending.len() > 2000 {
                            let _ = bus.emit(&channel, &LogBatch { lines: std::mem::take(&mut pending), ended: false });
                        }
                    }
                    Some(Err(e)) => {
                        pending.push(LogLine { stream: "err".into(), text: format!("[eDock] log ended: {e}") });
                        break;
                    }
                    None => break,
                },
                _ = tick.tick() => {
                    if !pending.is_empty() {
                        let _ = bus.emit(&channel, &LogBatch { lines: std::mem::take(&mut pending), ended: false });
                    }
                }
            }
        }
        for (i, rest) in partial.iter().enumerate() {
            if !rest.is_empty() {
                pending.push(LogLine { stream: NAMES[i].into(), text: String::from_utf8_lossy(rest).into_owned() });
            }
        }
        let _ = bus.emit(&channel, &LogBatch { lines: pending, ended: true });
        s2.done(id);
    });
    streams.keep(id, task.abort_handle());
    id
}

// ── Stats ──────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, specta::Type, Clone)]
pub struct StatsPoint {
    /// Percent of one CPU (200 = two CPUs busy).
    pub cpu: f64,
    pub cpus: u32,
    pub memory: f64,
    pub memory_limit: f64,
    pub net_rx: f64,
    pub net_tx: f64,
    pub block_read: f64,
    pub block_write: f64,
    pub pids: u32,
}

fn stats_point(v: &serde_json::Value) -> StatsPoint {
    let f = |p: &serde_json::Value| p.as_f64().unwrap_or(0.0);
    let cpu_delta = f(&v["cpu_stats"]["cpu_usage"]["total_usage"]) - f(&v["precpu_stats"]["cpu_usage"]["total_usage"]);
    let sys_delta = f(&v["cpu_stats"]["system_cpu_usage"]) - f(&v["precpu_stats"]["system_cpu_usage"]);
    let cpus = v["cpu_stats"]["online_cpus"].as_u64().filter(|n| *n > 0).unwrap_or(1) as f64;
    let cpu = if sys_delta > 0.0 && cpu_delta > 0.0 { cpu_delta / sys_delta * cpus * 100.0 } else { 0.0 };
    // As `docker stats`: usage less the page cache.
    let mem = &v["memory_stats"];
    let cache = mem["stats"]["inactive_file"].as_f64().or_else(|| mem["stats"]["total_inactive_file"].as_f64()).unwrap_or(0.0);
    let memory = (f(&mem["usage"]) - cache).max(0.0);
    let (mut rx, mut tx) = (0.0, 0.0);
    if let Some(nets) = v["networks"].as_object() {
        for n in nets.values() {
            rx += f(&n["rx_bytes"]);
            tx += f(&n["tx_bytes"]);
        }
    }
    let (mut br, mut bw) = (0.0, 0.0);
    if let Some(io) = v["blkio_stats"]["io_service_bytes_recursive"].as_array() {
        for e in io {
            match e["op"].as_str().map(str::to_ascii_lowercase).as_deref() {
                Some("read") => br += f(&e["value"]),
                Some("write") => bw += f(&e["value"]),
                _ => {}
            }
        }
    }
    StatsPoint {
        cpu,
        cpus: cpus as u32,
        memory,
        memory_limit: f(&mem["limit"]),
        net_rx: rx,
        net_tx: tx,
        block_read: br,
        block_write: bw,
        pids: v["pids_stats"]["current"].as_u64().unwrap_or(0) as u32,
    }
}

/// Follow a container's CPU, memory, network and disk, about once a
/// second, on `stats:<id>`.
#[command]
pub async fn stats_follow(ctx: Ctx, container: String) -> Result<u32, String> {
    let docker = ctx.get::<Engine>().docker()?;
    let bus = ctx.get::<EventBus>();
    let streams = ctx.get::<Streams>();
    let id = streams.id();
    let channel = format!("stats:{id}");
    let s2 = streams.clone();
    let task = tokio::spawn(async move {
        let o = q::StatsOptionsBuilder::default().stream(true).build();
        let mut stats = docker.stats(&container, Some(o));
        while let Some(Ok(item)) = stats.next().await {
            let v = serde_json::to_value(&item).unwrap_or_default();
            let _ = bus.emit(&channel, &stats_point(&v));
        }
        s2.done(id);
    });
    streams.keep(id, task.abort_handle());
    Ok(id as u32)
}

// ── Terminal ───────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, specta::Type, Clone)]
pub struct TermChunk {
    pub data: String,
    pub ended: bool,
}

/// Split off the end of `buf` that's an unfinished UTF-8 character, so a
/// character cut across two reads isn't shown as two � .
fn utf8_split(buf: &mut Vec<u8>) -> String {
    let cut = match std::str::from_utf8(buf) {
        Ok(_) => buf.len(),
        Err(e) if e.error_len().is_none() => e.valid_up_to(),
        Err(_) => buf.len(),
    };
    let rest = buf.split_off(cut);
    let text = String::from_utf8_lossy(buf).into_owned();
    *buf = rest;
    text
}

/// Open a shell in a running container (bash when it has one), sized
/// `cols`×`rows`. Output arrives on `exec:<id>`; send keys with
/// `exec_input`.
#[command]
pub async fn exec_start(ctx: Ctx, container: String, cols: u32, rows: u32, command: Option<String>) -> Result<u32, String> {
    let cmd = match command.filter(|c| !c.trim().is_empty()) {
        Some(c) => vec!["/bin/sh".to_owned(), "-c".into(), c],
        None => vec!["/bin/sh".to_owned(), "-c".into(), "if command -v bash >/dev/null 2>&1; then exec bash -l; else exec sh -l; fi".into()],
    };
    open_terminal(&ctx, &container, cmd, cols, rows).await
}

/// Run `cmd` in `container` on a terminal, for the page's xterm: a shell,
/// or `kubectl exec -it` in the Kubernetes node.
pub(crate) async fn open_terminal(ctx: &Ctx, container: &str, cmd: Vec<String>, cols: u32, rows: u32) -> Result<u32, String> {
    open_terminal_as(ctx, container, cmd, cols, rows, Exec::default()).await
}

/// Who a terminal runs as, and where.
#[derive(Default)]
pub(crate) struct Exec {
    pub user: Option<String>,
    pub working_dir: Option<String>,
    pub env: Vec<String>,
}

/// `open_terminal`, as a user, in a directory, with more environment: a
/// Linux machine's login shell.
pub(crate) async fn open_terminal_as(ctx: &Ctx, container: &str, cmd: Vec<String>, cols: u32, rows: u32, how: Exec) -> Result<u32, String> {
    let docker = ctx.get::<Engine>().docker()?;
    let bus = ctx.get::<EventBus>();
    let streams = ctx.get::<Streams>();
    let exec = docker
        .create_exec(
            container,
            CreateExecOptions {
                attach_stdin: Some(true),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                tty: Some(true),
                env: Some(std::iter::once("TERM=xterm-256color".to_owned()).chain(how.env).collect()),
                cmd: Some(cmd),
                user: how.user,
                working_dir: how.working_dir,
                ..Default::default()
            },
        )
        .await
        .map_err(|e| e.to_string())?;
    let started = docker
        .start_exec(&exec.id, Some(bollard::exec::StartExecOptions { detach: false, tty: true, output_capacity: None }))
        .await
        .map_err(|e| e.to_string())?;
    let StartExecResults::Attached { mut output, mut input } = started else {
        return Err("The engine started the shell detached".into());
    };
    let _ = docker
        .resize_exec(&exec.id, bollard::exec::ResizeExecOptions { width: cols.max(1) as u16, height: rows.max(1) as u16 })
        .await;

    let id = streams.id();
    let channel = format!("exec:{id}");
    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
    streams.terms.lock().insert(id, Term { input: tx, exec_id: exec.id.clone() });
    let s2 = streams.clone();
    let task = tokio::spawn(async move {
        let mut buf: Vec<u8> = vec![];
        let mut last = Instant::now();
        let mut tick = tokio::time::interval(Duration::from_millis(16));
        loop {
            tokio::select! {
                item = output.next() => match item {
                    Some(Ok(out)) => {
                        buf.extend_from_slice(&out.into_bytes());
                        // Flush big output at once; small bursts on the tick.
                        if buf.len() > 32 * 1024 || last.elapsed() > Duration::from_millis(16) {
                            let data = utf8_split(&mut buf);
                            if !data.is_empty() { let _ = bus.emit(&channel, &TermChunk { data, ended: false }); }
                            last = Instant::now();
                        }
                    }
                    _ => break,
                },
                keys = rx.recv() => match keys {
                    Some(k) => { if input.write_all(&k).await.is_err() { break; } let _ = input.flush().await; }
                    None => break,
                },
                _ = tick.tick() => {
                    if !buf.is_empty() {
                        let data = utf8_split(&mut buf);
                        if !data.is_empty() { let _ = bus.emit(&channel, &TermChunk { data, ended: false }); }
                        last = Instant::now();
                    }
                }
            }
        }
        let data = String::from_utf8_lossy(&buf).into_owned();
        let _ = bus.emit(&channel, &TermChunk { data, ended: true });
        s2.done(id);
    });
    streams.keep(id, task.abort_handle());
    Ok(id as u32)
}

/// Keys typed into a terminal.
#[command]
pub async fn exec_input(ctx: Ctx, id: u32, data: String) -> Result<(), String> {
    let streams = ctx.get::<Streams>();
    let terms = streams.terms.lock();
    let t = terms.get(&(id as u64)).ok_or("The terminal has ended")?;
    t.input.send(data.into_bytes()).map_err(|_| "The terminal has ended".to_string())
}

/// The terminal's new size.
#[command]
pub async fn exec_resize(ctx: Ctx, id: u32, cols: u32, rows: u32) -> Result<(), String> {
    let exec_id = ctx.get::<Streams>().terms.lock().get(&(id as u64)).map(|t| t.exec_id.clone());
    let Some(exec_id) = exec_id else { return Ok(()) };
    let docker = ctx.get::<Engine>().docker()?;
    docker
        .resize_exec(&exec_id, bollard::exec::ResizeExecOptions { width: cols.max(1) as u16, height: rows.max(1) as u16 })
        .await
        .map_err(|e| e.to_string())
}

// ── The engine's events ────────────────────────────────────────────────

#[derive(Serialize, Deserialize, specta::Type, Clone)]
pub struct Changed {
    /// container, image, volume, network, or "engine" when the engine
    /// came or went.
    pub kind: String,
    pub action: String,
}

/// Watches the engine's events for as long as the app runs, and says on
/// `docker:changed` what kind of thing changed (at most every 150 ms per
/// kind). Reconnects when the engine goes, or the context changes.
pub fn watch(engine: Arc<Engine>, bus: Arc<EventBus>, on_change: impl Fn(bool) + Send + Sync + 'static) {
    tokio::spawn(async move {
        let mut connected: Option<bool> = None;
        loop {
            let generation = engine.generation();
            let docker = match engine.docker() {
                Ok(d) if d.ping().await.is_ok() => d,
                _ => {
                    if connected != Some(false) {
                        connected = Some(false);
                        let _ = bus.emit("docker:changed", &Changed { kind: "engine".into(), action: "down".into() });
                        on_change(false);
                    }
                    engine.reset();
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };
            if connected != Some(true) {
                connected = Some(true);
                let _ = bus.emit("docker:changed", &Changed { kind: "engine".into(), action: "up".into() });
                on_change(true);
            }
            let mut events = docker.events(None::<q::EventsOptions>);
            // At most one change per kind every 150 ms: the first at once,
            // the rest of a burst (compose up of ten services) as one
            // trailing change. A burst that made or removed a container
            // says so, since that changes what images and volumes are used by.
            let mut last: HashMap<String, Instant> = HashMap::new();
            let mut pending: HashMap<String, bool> = HashMap::new();
            let mut check = tokio::time::interval(Duration::from_millis(500));
            let mut flush = tokio::time::interval(Duration::from_millis(50));
            let window = Duration::from_millis(150);
            loop {
                tokio::select! {
                    ev = events.next() => match ev {
                        Some(Ok(msg)) => {
                            let v = serde_json::to_value(&msg).unwrap_or_default();
                            let kind = v["Type"].as_str().unwrap_or("").to_owned();
                            let action = v["Action"].as_str().unwrap_or("").to_owned();
                            // exec_* (health checks run as execs too) change no list;
                            // health_status does: the badge goes healthy or unhealthy.
                            if action.starts_with("exec_") || action == "top" {
                                continue;
                            }
                            let structural = action == "create" || action == "destroy";
                            let now = Instant::now();
                            if last.get(&kind).is_some_and(|t| now.duration_since(*t) < window) {
                                let flag = pending.entry(kind).or_insert(false);
                                *flag |= structural;
                                continue;
                            }
                            last.insert(kind.clone(), now);
                            let _ = bus.emit("docker:changed", &Changed { kind: kind.clone(), action });
                            if kind == "container" { on_change(true); }
                        }
                        _ => break,
                    },
                    _ = flush.tick() => {
                        let now = Instant::now();
                        let due: Vec<String> = pending
                            .keys()
                            .filter(|k| last.get(*k).is_none_or(|t| now.duration_since(*t) >= window))
                            .cloned()
                            .collect();
                        for kind in due {
                            let structural = pending.remove(&kind).unwrap_or(false);
                            last.insert(kind.clone(), now);
                            let action = if structural { "create" } else { "update" };
                            let _ = bus.emit("docker:changed", &Changed { kind: kind.clone(), action: action.into() });
                            if kind == "container" { on_change(true); }
                        }
                    },
                    _ = check.tick() => {
                        if engine.generation() != generation { break; }
                    }
                }
            }
            // The stream ended: the engine went, or the context changed
            // (then say "up" again, for the other engine's lists).
            if engine.generation() == generation {
                tokio::time::sleep(Duration::from_millis(500)).await;
            } else {
                connected = None;
            }
        }
    });
}

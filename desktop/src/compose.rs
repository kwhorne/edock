//! Compose projects as a whole: one log for all their services, their
//! folder opened in Finder, Terminal or an editor, and `docker compose up`
//! again (with a build) from their own files. Projects are remembered
//! (in `~/.edock/desktop.json`), so one that is down still shows, with Up.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use bollard::container::LogOutput;
use bollard::query_parameters as q;
use elyra::{command, Ctx, EventBus};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::engine::{Engine, Settings};
use crate::streams::{LogBatch, LogLine, Streams};

const PROJECT: &str = "com.docker.compose.project";
const DIR: &str = "com.docker.compose.project.working_dir";
const FILES: &str = "com.docker.compose.project.config_files";
const ENV_FILES: &str = "com.docker.compose.project.environment_file";

/// A Compose project eDock remembers: its folder and files, and the docker
/// context it ran on.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct SavedProject {
    pub name: String,
    pub context: String,
    pub dir: String,
    pub files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_files: Vec<String>,
}

/// A comma-separated label, as a list.
fn split(v: Option<&String>) -> Vec<String> {
    v.map(String::as_str).unwrap_or("").split(',').map(str::trim).filter(|f| !f.is_empty()).map(str::to_owned).collect()
}

impl SavedProject {
    /// The project a container's labels name, if Compose said where it is.
    fn from_labels(context: &str, labels: &HashMap<String, String>) -> Option<Self> {
        let name = labels.get(PROJECT).filter(|n| !n.is_empty())?;
        let dir = labels.get(DIR).filter(|d| !d.is_empty())?;
        Some(Self { name: name.clone(), context: context.to_owned(), dir: dir.clone(), files: split(labels.get(FILES)), env_files: split(labels.get(ENV_FILES)) })
    }

    /// As its containers' labels would say it.
    fn labels(&self) -> HashMap<String, String> {
        let mut l = HashMap::from([(PROJECT.to_string(), self.name.clone()), (DIR.to_string(), self.dir.clone()), (FILES.to_string(), self.files.join(","))]);
        if !self.env_files.is_empty() {
            l.insert(ENV_FILES.to_string(), self.env_files.join(","));
        }
        l
    }
}

/// Remember these projects, or what changed about them.
fn remember(found: Vec<SavedProject>) {
    if found.is_empty() {
        return;
    }
    Settings::update(|s| {
        let mut changed = false;
        for p in found {
            match s.projects.iter_mut().find(|q| q.context == p.context && q.name == p.name) {
                Some(q) if *q == p => {}
                Some(q) => {
                    *q = p;
                    changed = true;
                }
                None => {
                    s.projects.push(p);
                    changed = true;
                }
            }
        }
        (changed, ())
    })
}

/// A container was made: remember its Compose project, if it has one.
pub(crate) fn remember_labels(context: &str, labels: &HashMap<String, String>) {
    if let Some(p) = SavedProject::from_labels(context, labels) {
        remember(vec![p]);
    }
}

fn saved(context: &str, project: &str) -> Option<SavedProject> {
    Settings::load().projects.into_iter().find(|p| p.context == context && p.name == project)
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct ProjectRow {
    pub name: String,
    pub dir: String,
    pub files: Vec<String>,
    /// Why Up can't run it (its folder or a file is gone), when it can't.
    pub problem: Option<String>,
}

/// The Compose projects remembered on the engine the app shows, those with
/// containers now among them (they're remembered as they're listed).
#[command(can = "edock.view")]
pub async fn compose_projects(ctx: Ctx) -> Vec<ProjectRow> {
    let engine = ctx.get::<Engine>();
    let context = engine.context().0;
    if let Ok(docker) = engine.docker() {
        let mut filters = HashMap::new();
        filters.insert("label".to_string(), vec![PROJECT.to_string()]);
        if let Ok(list) = docker.list_containers(Some(q::ListContainersOptionsBuilder::default().all(true).filters(&filters).build())).await {
            let mut seen = std::collections::HashSet::new();
            remember(list.into_iter().filter_map(|c| SavedProject::from_labels(&context, &c.labels.unwrap_or_default())).filter(|p| seen.insert(p.name.clone())).collect());
        }
    }
    let mut rows: Vec<ProjectRow> = Settings::load()
        .projects
        .into_iter()
        .filter(|p| p.context == context)
        .map(|p| ProjectRow { problem: problem(&p), name: p.name, dir: p.dir, files: p.files })
        .collect();
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    rows
}

/// Why Up can't run a remembered project, short, what's wrong first.
fn problem(p: &SavedProject) -> Option<String> {
    if !Path::new(&p.dir).is_dir() {
        return Some("Its folder isn't there any more".into());
    }
    if let Some(f) = p.files.iter().chain(&p.env_files).find(|f| !Path::new(f).is_file()) {
        let name = Path::new(f).file_name().map_or(f.clone(), |n| n.to_string_lossy().into_owned());
        return Some(format!("{name} isn't there any more"));
    }
    compose_args(&p.name, &p.labels()).err()
}

/// The Compose files `docker compose` itself would use in `dir`: the first
/// of its file names there, and the first override.
fn compose_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let first = |names: &[&str]| names.iter().map(|n| dir.join(n)).find(|p| p.is_file());
    let main = first(&["compose.yaml", "compose.yml", "docker-compose.yml", "docker-compose.yaml"])
        .ok_or_else(|| format!("{} has no compose.yaml (or docker-compose.yml)", dir.display()))?;
    Ok(std::iter::once(main).chain(first(&["compose.override.yml", "compose.override.yaml", "docker-compose.override.yml", "docker-compose.override.yaml"])).collect())
}

/// Remember the Compose project in a folder, for the engine the app shows,
/// without starting it. Returns its name, as `docker compose` gives it.
#[command(can = "edock.manage")]
pub async fn compose_project_open(ctx: Ctx, path: String) -> Result<String, String> {
    let dir = real_folder(&path)?;
    let files = compose_files(&dir)?;
    let docker = docker_cli().ok_or("The docker command isn't installed: Set up on the Engine page installs it")?;
    let mut cmd = tokio::process::Command::new(docker);
    cmd.args(["compose", "--project-directory"]).arg(&dir);
    for f in &files {
        cmd.arg("-f").arg(f);
    }
    // The name as the folder's own files and .env give it. Every profile:
    // without, a service that depends on one in a profile fails the check.
    let out = cmd
        .args(["--profile", "*", "config", "--format", "json"])
        .current_dir(&dir)
        .env_remove("COMPOSE_PROJECT_NAME")
        .env_remove("COMPOSE_FILE")
        .env_remove("COMPOSE_PROFILES")
        .env("DOCKER_HOST", ctx.get::<Engine>().context().1)
        .env_remove("DOCKER_CONTEXT")
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| format!("Couldn't run docker compose: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let last = err.lines().map(str::trim).rev().find(|l| !l.is_empty()).unwrap_or("it failed");
        return Err(format!("docker compose can't read {}: {last}", files[0].display()));
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|_| "docker compose said something eDock can't read".to_string())?;
    let name = v["name"].as_str().filter(|n| !n.is_empty()).ok_or("docker compose gave the project no name")?.to_owned();
    let context = ctx.get::<Engine>().context().0;
    let files = files.iter().map(|f| f.display().to_string()).collect();
    remember(vec![SavedProject { name: name.clone(), context, dir: dir.display().to_string(), files, env_files: Vec::new() }]);
    Ok(name)
}

/// Forget a remembered project (its containers, if any, stay).
#[command(can = "edock.manage")]
pub async fn compose_project_forget(ctx: Ctx, project: String) {
    let context = ctx.get::<Engine>().context().0;
    Settings::update(|s| {
        let n = s.projects.len();
        s.projects.retain(|p| !(p.context == context && p.name == project));
        (s.projects.len() != n, ())
    })
}

/// A project's containers: id, service, and its labels.
async fn members(ctx: &Ctx, project: &str) -> Result<Vec<(String, String, HashMap<String, String>)>, String> {
    let docker = ctx.get::<Engine>().docker()?;
    let mut filters = HashMap::new();
    filters.insert("label".to_string(), vec![format!("{PROJECT}={project}")]);
    let list = docker.list_containers(Some(q::ListContainersOptionsBuilder::default().all(true).filters(&filters).build())).await.map_err(|e| e.to_string())?;
    if list.is_empty() {
        return Err(format!("There's no Compose project {project}"));
    }
    Ok(list
        .into_iter()
        .map(|c| {
            let labels = c.labels.unwrap_or_default();
            let name = c.names.and_then(|n| n.first().map(|n| n.trim_start_matches('/').to_owned())).unwrap_or_default();
            let service = labels.get("com.docker.compose.service").cloned().unwrap_or(name);
            (c.id.unwrap_or_default(), service, labels)
        })
        .collect())
}

/// One line of a service's log, its time taken off, to the merge.
fn send(tx: &tokio::sync::mpsc::UnboundedSender<(String, LogLine)>, service: &str, stream: usize, line: &[u8]) {
    let text = String::from_utf8_lossy(line).trim_end_matches(['\n', '\r']).to_owned();
    let (t, rest) = split_time(&text);
    let stream = if stream == 1 { "err" } else { "out" };
    let _ = tx.send((t.to_owned(), LogLine { stream: stream.into(), text: rest.to_owned(), service: Some(service.to_owned()) }));
}

/// Docker's timestamp off the front of a line (`2026-11-14T21:45:22.1Z text`).
pub(crate) fn split_time(line: &str) -> (&str, &str) {
    match line.split_once(' ') {
        Some((t, rest)) if t.len() >= 20 && t.as_bytes()[4] == b'-' && t.contains('T') => (t, rest),
        _ => ("", line),
    }
}

/// Follow every service's log in a Compose project, as one: each line
/// with its service, from the last `tail` lines of each, in time order.
/// Lines arrive on `logs:<id>` like a container's.
#[command]
pub async fn project_logs_follow(ctx: Ctx, project: String, tail: u32, timestamps: bool) -> Result<u32, String> {
    let tail = tail.clamp(1, 5000);
    let members = members(&ctx, &project).await?;
    let docker = ctx.get::<Engine>().docker()?;
    let streams = ctx.get::<Streams>();
    let bus = ctx.get::<EventBus>();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(String, LogLine)>();
    let mut readers = tokio::task::JoinSet::new();
    for (id, service, _) in members {
        let o = q::LogsOptionsBuilder::default().follow(true).stdout(true).stderr(true).timestamps(true).tail(&tail.to_string()).build();
        let mut logs = docker.logs(&id, Some(o));
        let tx = tx.clone();
        readers.spawn(async move {
            let mut partial: [Vec<u8>; 2] = [vec![], vec![]];
            while let Some(Ok(out)) = logs.next().await {
                let (i, bytes) = match out {
                    LogOutput::StdErr { message } => (1, message),
                    LogOutput::StdOut { message } | LogOutput::Console { message } => (0, message),
                    LogOutput::StdIn { .. } => continue,
                };
                partial[i].extend_from_slice(&bytes);
                while let Some(nl) = partial[i].iter().position(|&b| b == b'\n') {
                    let line: Vec<u8> = partial[i].drain(..=nl).collect();
                    send(&tx, &service, i, &line);
                }
            }
            // A last line without its newline, when the service stopped.
            for (i, rest) in partial.iter().enumerate() {
                if !rest.is_empty() {
                    send(&tx, &service, i, rest);
                }
            }
        });
    }
    drop(tx);
    let (id, channel) = streams.open("logs");
    let s2 = streams.clone();
    let task = tokio::spawn(async move {
        // The readers live as long as this task: stopping it stops them.
        let _readers = readers;
        let stamp = |t: &str, mut l: LogLine| {
            if timestamps && !t.is_empty() {
                l.text = format!("{t} {}", l.text);
            }
            l
        };
        // The tails come one container after another: gathered for a moment
        // and put in time order, then lines as they come.
        let mut first: Vec<(String, LogLine)> = Vec::new();
        let gather = tokio::time::sleep(Duration::from_millis(400));
        tokio::pin!(gather);
        let mut open = true;
        while open {
            tokio::select! {
                got = rx.recv() => match got { Some(l) => first.push(l), None => open = false },
                () = &mut gather => break,
            }
        }
        first.sort_by(|a, b| a.0.cmp(&b.0));
        let _ = bus.emit(&channel, &LogBatch { lines: first.into_iter().map(|(t, l)| stamp(&t, l)).collect(), ended: false });
        let mut pending = Vec::new();
        let mut tick = tokio::time::interval(Duration::from_millis(60));
        while open {
            tokio::select! {
                got = rx.recv() => match got {
                    Some((t, l)) => pending.push(stamp(&t, l)),
                    None => open = false,
                },
                _ = tick.tick() => if !pending.is_empty() {
                    let _ = bus.emit(&channel, &LogBatch { lines: std::mem::take(&mut pending), ended: false });
                },
            }
        }
        let _ = bus.emit(&channel, &LogBatch { lines: pending, ended: true });
        s2.close(id);
    });
    streams.keep_task(id, task.abort_handle());
    Ok(id as u32)
}

/// A folder a Compose project names: absolute, and there.
fn real_folder(path: &str) -> Result<PathBuf, String> {
    let p = Path::new(path);
    if !p.is_absolute() || path.contains('\0') {
        return Err("That isn't a folder on this Mac".into());
    }
    let p = std::fs::canonicalize(p).map_err(|_| format!("{path} isn't on this Mac (any more)"))?;
    if p.is_dir() { Ok(p) } else { Err(format!("{path} isn't a folder")) }
}

/// Editors eDock knows, in the order it tries them.
const EDITORS: [(&str, &str); 6] =
    [("Cursor", "Cursor.app"), ("Visual Studio Code", "Visual Studio Code.app"), ("Zed", "Zed.app"), ("Windsurf", "Windsurf.app"), ("Sublime Text", "Sublime Text.app"), ("Nova", "Nova.app")];

fn editor() -> Option<&'static str> {
    let home = dirs::home_dir().unwrap_or_default();
    EDITORS.iter().find(|(_, app)| Path::new("/Applications").join(app).exists() || home.join("Applications").join(app).exists()).map(|(n, _)| *n)
}

/// The editor "Open in editor" uses, if the Mac has one eDock knows.
#[command]
pub async fn editor_name(_ctx: Ctx) -> Option<String> {
    editor().map(str::to_owned)
}

/// Open a project's folder: `finder`, `terminal`, or `editor` (Cursor, VS
/// Code, Zed… whichever is installed). Returns what opened it.
#[command]
pub async fn folder_open(_ctx: Ctx, path: String, app: String) -> Result<String, String> {
    let dir = real_folder(&path)?;
    let (args, name): (Vec<&str>, String) = match app.as_str() {
        "finder" => (vec![], "Finder".into()),
        "terminal" => (vec!["-a", "Terminal"], "Terminal".into()),
        "editor" => {
            let e = editor().ok_or("No editor eDock knows is installed (Cursor, VS Code, Zed, Windsurf, Sublime Text, Nova)")?;
            (vec!["-a", e], e.into())
        }
        other => return Err(format!("{other}: finder, terminal or editor")),
    };
    let ok = tokio::process::Command::new("/usr/bin/open").args(&args).arg(&dir).status().await.map_err(|e| e.to_string())?.success();
    if ok { Ok(name) } else { Err(format!("{name} couldn't open {}", dir.display())) }
}

/// The docker CLI: eDock's own, then the usual places.
fn docker_cli() -> Option<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    [
        home.join(".edock/bin/docker"),
        PathBuf::from("/usr/local/bin/docker"),
        PathBuf::from("/opt/homebrew/bin/docker"),
        PathBuf::from("/Applications/OrbStack.app/Contents/MacOS/xbin/docker"),
        PathBuf::from("/Applications/Docker.app/Contents/Resources/bin/docker"),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

#[derive(Serialize, Deserialize, specta::Type, Clone)]
pub struct ComposeProgress {
    pub project: String,
    pub line: String,
    pub done: bool,
    pub error: Option<String>,
}

/// The `docker compose` flags that name a project as Compose made it: its
/// name, folder and files, from its containers' labels.
fn compose_args(project: &str, labels: &HashMap<String, String>) -> Result<Vec<String>, String> {
    let dir = labels.get(DIR).ok_or("Compose didn't say where this project's folder is")?;
    let dir = real_folder(dir)?;
    let mut args = vec!["compose".to_string(), "--ansi".into(), "never".into(), "--progress".into(), "plain".into(), "-p".into(), project.to_owned(), "--project-directory".into(), dir.display().to_string()];
    for (flag, what) in [("-f", FILES), ("--env-file", ENV_FILES)] {
        for f in split(labels.get(what)) {
            // As the folder: absolute, never read against eDock's own folder.
            if !Path::new(&f).is_absolute() || f.contains('\0') {
                return Err(format!("{f} isn't a file eDock can give docker compose"));
            }
            if !Path::new(&f).is_file() {
                return Err(format!("{f} isn't there any more"));
            }
            args.push(flag.into());
            args.push(f);
        }
    }
    Ok(args)
}

/// The `docker compose` flags for a project: from its containers' labels,
/// or, when it's down, from what eDock remembers of it.
async fn project_args(ctx: &Ctx, project: &str) -> Result<Vec<String>, String> {
    match members(ctx, project).await {
        Ok(members) => compose_args(project, &members[0].2),
        Err(none) => {
            let context = ctx.get::<Engine>().context().0;
            compose_args(project, &saved(&context, project).ok_or(none)?.labels())
        }
    }
}

/// `docker compose up -d` for a project, from its own files and folder,
/// with `--build` when `build` (Rebuild & up). On the engine the app shows;
/// a project that is down comes up from what eDock remembers of it.
/// Output on `compose:progress`.
#[command(can = "edock.manage")]
pub async fn project_up(ctx: Ctx, project: String, build: bool) -> Result<(), String> {
    let mut args = project_args(&ctx, &project).await?;
    args.extend(["up".into(), "-d".into(), "--remove-orphans".into()]);
    if build {
        args.push("--build".into());
    }
    run_compose(&ctx, &project, args).await
}

/// A service's name, as Compose takes it (and never a flag).
fn valid_service(name: &str) -> Result<&str, String> {
    let ok = !name.is_empty() && name.len() <= 128 && name.as_bytes()[0].is_ascii_alphanumeric() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
    if ok { Ok(name) } else { Err(format!("{name:?} isn't a Compose service's name")) }
}

/// One service of a project made again, the rest left as they are:
/// `docker compose up -d --no-deps --force-recreate SERVICE`, with
/// `--build` when `build` (its image built again first). After a change
/// to its part of compose.yaml or .env, or to its code. Output on
/// `compose:progress`.
#[command(can = "edock.manage")]
pub async fn service_up(ctx: Ctx, project: String, service: String, build: bool) -> Result<(), String> {
    let service = valid_service(&service)?.to_owned();
    let mut args = project_args(&ctx, &project).await?;
    args.extend(["up".into(), "-d".into(), "--no-deps".into(), "--force-recreate".into()]);
    if build {
        args.push("--build".into());
    }
    args.push(service);
    run_compose(&ctx, &project, args).await
}

/// Runs `docker compose ARGS` on the engine the app shows, its lines on
/// `compose:progress` for `project`, and says how it ended there too.
async fn run_compose(ctx: &Ctx, project: &str, args: Vec<String>) -> Result<(), String> {
    let project = project.to_owned();
    let docker = docker_cli().ok_or("The docker command isn't installed: Set up on the Engine page installs it")?;
    let host = ctx.get::<Engine>().context().1;
    let mut child = tokio::process::Command::new(docker)
        .args(&args)
        .env("DOCKER_HOST", &host)
        .env_remove("DOCKER_CONTEXT")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Couldn't run docker compose: {e}"))?;
    let bus: Arc<EventBus> = ctx.get::<EventBus>();
    let (out, err) = (child.stdout.take(), child.stderr.take());
    let last = Arc::new(parking_lot::Mutex::new(String::new()));
    let send = |r: Box<dyn tokio::io::AsyncRead + Unpin + Send>, bus: Arc<EventBus>, project: String, last: Arc<parking_lot::Mutex<String>>| async move {
        let mut lines = BufReader::new(r).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if !line.trim().is_empty() {
                *last.lock() = line.clone();
            }
            let _ = bus.emit("compose:progress", &ComposeProgress { project: project.clone(), line, done: false, error: None });
        }
    };
    let a = out.map(|o| tokio::spawn(send(Box::new(o), bus.clone(), project.clone(), last.clone())));
    let b = err.map(|e| tokio::spawn(send(Box::new(e), bus.clone(), project.clone(), last.clone())));
    for t in [a, b].into_iter().flatten() {
        let _ = t.await;
    }
    let status = child.wait().await.map_err(|e| e.to_string())?;
    let error = (!status.success()).then(|| last.lock().clone()).map(|e| if e.is_empty() { "docker compose failed".into() } else { e });
    let _ = bus.emit("compose:progress", &ComposeProgress { project, line: String::new(), done: true, error: error.clone() });
    let _ = bus.emit("docker:changed", &crate::streams::Changed { kind: "container".into(), action: "create".into() });
    error.map_or(Ok(()), Err)
}

// ── What a project is made of ──────────────────────────────────────────

#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, PartialEq)]
pub struct ServiceView {
    pub name: String,
    pub image: Option<String>,
    /// Its build context, when Compose builds its image.
    pub build: Option<String>,
    /// `8080:80/tcp` and the like, as published on the Mac.
    pub ports: Vec<String>,
    /// `source:target` (a volume's name or a folder), `:ro` when read-only.
    pub volumes: Vec<String>,
    /// Its environment variables' names; never their values.
    pub env: Vec<String>,
    pub depends_on: Vec<String>,
    pub profiles: Vec<String>,
}

#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, PartialEq)]
pub struct EnvFileView {
    pub path: String,
    /// Its variables' names; never their values.
    pub keys: Vec<String>,
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct ComposeView {
    pub dir: String,
    pub files: Vec<String>,
    pub services: Vec<ServiceView>,
    /// The project's `.env`, and the env files it was started with.
    pub env_files: Vec<EnvFileView>,
}

/// The services as `docker compose config --format json` gives them.
fn services(config: &serde_json::Value) -> Vec<ServiceView> {
    let strs = |v: &serde_json::Value| v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect::<Vec<_>>()).unwrap_or_default();
    let mut out: Vec<ServiceView> = config["services"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, s)| {
            let mut env: Vec<String> = match &s["environment"] {
                serde_json::Value::Object(m) => m.keys().cloned().collect(),
                serde_json::Value::Array(a) => a.iter().filter_map(|x| x.as_str()).map(|x| x.split('=').next().unwrap_or(x).to_owned()).collect(),
                _ => Vec::new(),
            };
            env.sort();
            let ports = s["ports"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|p| {
                    let host = p["published"].as_str().map(str::to_owned).or_else(|| p["published"].as_u64().map(|n| n.to_string()));
                    let target = p["target"].as_u64().map(|n| n.to_string()).unwrap_or_default();
                    let proto = p["protocol"].as_str().unwrap_or("tcp");
                    let ip = p["host_ip"].as_str().filter(|i| !i.is_empty()).map(|i| format!("{i}:")).unwrap_or_default();
                    match host {
                        Some(h) if !h.is_empty() => format!("{ip}{h}:{target}/{proto}"),
                        _ => format!("{target}/{proto}"),
                    }
                })
                .collect();
            let volumes = s["volumes"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|v| {
                    let ro = if v["read_only"] == true { ":ro" } else { "" };
                    match v["source"].as_str().filter(|x| !x.is_empty()) {
                        Some(src) => format!("{src}:{}{ro}", v["target"].as_str().unwrap_or("")),
                        None => format!("{}{ro}", v["target"].as_str().unwrap_or("")),
                    }
                })
                .collect();
            let mut depends_on: Vec<String> = match &s["depends_on"] {
                serde_json::Value::Object(m) => m.keys().cloned().collect(),
                other => strs(other),
            };
            depends_on.sort();
            ServiceView {
                name: name.clone(),
                image: s["image"].as_str().map(str::to_owned),
                build: s["build"]["context"].as_str().or(s["build"].as_str()).map(str::to_owned),
                ports,
                volumes,
                env,
                depends_on,
                profiles: strs(&s["profiles"]),
            }
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// A dotenv file's variable names, in order, once each; comments and
/// `export ` left out. Never the values.
fn env_keys(text: &str) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        if let Some((k, _)) = line.split_once('=') {
            let k = k.trim().to_owned();
            if !k.is_empty() && !keys.contains(&k) {
                keys.push(k);
            }
        }
    }
    keys
}

/// What a project is made of, as Compose reads its files: each service's
/// image or build, ports, volumes, dependencies and the names of its
/// variables, and the names in its `.env` and env files. Values never
/// leave the Mac's files: they can be passwords.
#[command(can = "edock.view")]
pub async fn compose_view(ctx: Ctx, project: String) -> Result<ComposeView, String> {
    let args = project_args(&ctx, &project).await?;
    let dir = args.iter().position(|a| a == "--project-directory").and_then(|i| args.get(i + 1)).cloned().unwrap_or_default();
    let files: Vec<String> = args.windows(2).filter(|w| w[0] == "-f").map(|w| w[1].clone()).collect();
    let env_paths: Vec<String> = args.windows(2).filter(|w| w[0] == "--env-file").map(|w| w[1].clone()).collect();
    let docker = docker_cli().ok_or("The docker command isn't installed: Set up on the Engine page installs it")?;
    // Every profile's services, each with its profiles shown.
    let mut cmd = args.clone();
    cmd.extend(["--profile".into(), "*".into(), "config".into(), "--format".into(), "json".into()]);
    let out = tokio::process::Command::new(docker)
        .args(&cmd)
        .env("DOCKER_HOST", ctx.get::<Engine>().context().1)
        .env_remove("DOCKER_CONTEXT")
        .env_remove("COMPOSE_PROJECT_NAME")
        .env_remove("COMPOSE_PROFILES")
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| format!("Couldn't run docker compose: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("docker compose can't read the project: {}", err.lines().map(str::trim).rev().find(|l| !l.is_empty()).unwrap_or("it failed")));
    }
    let config: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|_| "docker compose said something eDock can't read".to_string())?;
    // The project's own .env (unless env files were given instead), then those.
    let mut env_files = Vec::new();
    let default_env = Path::new(&dir).join(".env");
    let paths: Vec<PathBuf> = if env_paths.is_empty() { vec![default_env] } else { env_paths.iter().map(PathBuf::from).collect() };
    for p in paths {
        if let Ok(text) = std::fs::read_to_string(&p) {
            env_files.push(EnvFileView { path: p.display().to_string(), keys: env_keys(&text) });
        }
    }
    Ok(ComposeView { dir, files, services: services(&config), env_files })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_reads_without_its_values() {
        let config = serde_json::json!({"services": {
            "web": {"build": {"context": "/x/web"}, "ports": [{"target": 80, "published": "8080", "protocol": "tcp", "host_ip": "127.0.0.1"}],
                    "environment": {"DB_PASSWORD": "s3cret", "APP_ENV": "dev"}, "depends_on": {"db": {"condition": "service_healthy"}},
                    "volumes": [{"type": "bind", "source": "/x/web", "target": "/app", "read_only": true}]},
            "db": {"image": "postgres:17", "volumes": [{"type": "volume", "source": "pgdata", "target": "/var/lib/postgresql/data"}], "profiles": ["full"],
                   "environment": ["POSTGRES_PASSWORD=hunter2"]}
        }});
        let s = services(&config);
        assert_eq!(s.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["db", "web"]);
        let (db, web) = (&s[0], &s[1]);
        assert_eq!((db.image.as_deref(), db.build.as_deref()), (Some("postgres:17"), None));
        assert_eq!(db.volumes, ["pgdata:/var/lib/postgresql/data"]);
        assert_eq!(db.env, ["POSTGRES_PASSWORD"]);
        assert_eq!(db.profiles, ["full"]);
        assert_eq!(web.build.as_deref(), Some("/x/web"));
        assert_eq!(web.ports, ["127.0.0.1:8080:80/tcp"]);
        assert_eq!(web.volumes, ["/x/web:/app:ro"]);
        assert_eq!(web.env, ["APP_ENV", "DB_PASSWORD"]);
        assert_eq!(web.depends_on, ["db"]);
        let all = format!("{s:?}");
        assert!(!all.contains("s3cret") && !all.contains("hunter2"), "no values");
        assert_eq!(env_keys("# c\nA=1\nexport B=\"two\"\n\nA=again\nnot a line\n C = 3"), ["A", "B", "C"]);
        assert!(valid_service("web-1.api").is_ok());
        for bad in ["", "-d", "a b", "../x"] {
            assert!(valid_service(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn timestamps_come_off_the_front() {
        assert_eq!(split_time("2026-11-14T21:45:22.123456789Z hello world"), ("2026-11-14T21:45:22.123456789Z", "hello world"));
        assert_eq!(split_time("no time here"), ("", "no time here"));
    }

    #[test]
    fn compose_is_called_with_the_projects_own_files() {
        let dir = std::env::temp_dir().join(format!("edock-compose-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("compose.yaml");
        std::fs::write(&file, "services: {}\n").unwrap();
        let labels: HashMap<String, String> = [
            ("com.docker.compose.project.working_dir".to_string(), dir.display().to_string()),
            ("com.docker.compose.project.config_files".to_string(), file.display().to_string()),
        ]
        .into();
        let a = compose_args("shop", &labels).unwrap();
        assert!(a.windows(2).any(|w| w[0] == "-p" && w[1] == "shop"));
        assert!(a.windows(2).any(|w| w[0] == "-f" && w[1] == file.display().to_string()));
        let gone: HashMap<String, String> = [("com.docker.compose.project.working_dir".to_string(), "/nope/x".to_string())].into();
        assert!(compose_args("shop", &gone).is_err());
        let relative: HashMap<String, String> = [
            ("com.docker.compose.project.working_dir".to_string(), dir.display().to_string()),
            ("com.docker.compose.project.config_files".to_string(), "compose.yaml".to_string()),
        ]
        .into();
        assert!(compose_args("shop", &relative).is_err(), "a relative file");
        assert!(real_folder("relative").is_err());
        let env = dir.join("local.env");
        std::fs::write(&env, "X=1\n").unwrap();
        let mut with_env = labels.clone();
        with_env.insert(ENV_FILES.to_string(), env.display().to_string());
        let a = compose_args("shop", &with_env).unwrap();
        assert!(a.windows(2).any(|w| w[0] == "--env-file" && w[1] == env.display().to_string()));
        with_env.insert(ENV_FILES.to_string(), "/nope/.env".into());
        assert!(compose_args("shop", &with_env).is_err(), "a missing env file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_project_is_remembered_as_its_labels_say() {
        let labels: HashMap<String, String> =
            [(PROJECT, "shop"), (DIR, "/x"), (FILES, "/x/compose.yaml, /x/o.yaml"), (ENV_FILES, "/x/a.env,/x/b.env")].map(|(k, v)| (k.to_string(), v.to_string())).into();
        let p = SavedProject::from_labels("edock", &labels).unwrap();
        assert_eq!((p.name.as_str(), p.context.as_str(), p.dir.as_str()), ("shop", "edock", "/x"));
        assert_eq!(p.files, ["/x/compose.yaml", "/x/o.yaml"]);
        assert_eq!(p.env_files, ["/x/a.env", "/x/b.env"]);
        assert_eq!(SavedProject::from_labels("edock", &p.labels()), Some(p));
        let no_dir: HashMap<String, String> = [(PROJECT.to_string(), "shop".to_string())].into();
        assert!(SavedProject::from_labels("edock", &no_dir).is_none());
        assert_eq!(problem(&SavedProject::from_labels("edock", &labels).unwrap()).as_deref(), Some("Its folder isn't there any more"));
        let here = std::env::temp_dir().display().to_string();
        let gone = SavedProject { name: "shop".into(), context: "edock".into(), dir: here.clone(), files: vec![format!("{here}/nope-compose.yaml")], env_files: vec![] };
        assert_eq!(problem(&gone).as_deref(), Some("nope-compose.yaml isn't there any more"));
    }

    #[test]
    fn compose_files_as_compose_finds_them() {
        let dir = std::env::temp_dir().join(format!("edock-compose-files-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(compose_files(&dir).is_err(), "none");
        std::fs::write(dir.join("docker-compose.yml"), "").unwrap();
        std::fs::write(dir.join("compose.override.yaml"), "").unwrap();
        assert_eq!(compose_files(&dir).unwrap(), [dir.join("docker-compose.yml"), dir.join("compose.override.yaml")]);
        std::fs::write(dir.join("compose.yaml"), "").unwrap();
        assert_eq!(compose_files(&dir).unwrap()[0], dir.join("compose.yaml"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

//! The Docker engine the app talks to: a socket chosen from the docker
//! contexts (eDock's own first), held behind a lock so switching context
//! swaps it for every command at once.

use std::path::PathBuf;
use std::sync::Arc;

use bollard::{Docker, API_DEFAULT_VERSION};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// The context eDock's own engine is known by.
pub const EDOCK: &str = "edock";

/// A docker context: a name and the socket behind it.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub struct Context {
    pub name: String,
    pub description: String,
    /// `unix:///…/docker.sock`
    pub host: String,
    /// The socket exists on disk (the engine may still be stopped).
    pub available: bool,
    /// The one `docker` itself uses (`docker context use`).
    pub docker_current: bool,
}

pub struct Engine {
    inner: RwLock<Current>,
    /// Bumped on every switch, so long-running streams notice and end.
    generation: std::sync::atomic::AtomicU64,
}

struct Current {
    context: String,
    host: String,
    docker: Option<Docker>,
}

impl Engine {
    pub fn new() -> Arc<Self> {
        let saved = Settings::load().context;
        let contexts = list_contexts();
        // The saved one, else eDock's own when its socket is there, else
        // docker's current context, else the first we know.
        let pick = saved
            .and_then(|s| contexts.iter().find(|c| c.name == s).cloned())
            .or_else(|| contexts.iter().find(|c| c.name == EDOCK && c.available).cloned())
            .or_else(|| contexts.iter().find(|c| c.docker_current).cloned())
            .or_else(|| contexts.first().cloned());
        let (context, host) = pick.map(|c| (c.name, c.host)).unwrap_or_else(|| (EDOCK.into(), edock_host()));
        Arc::new(Engine {
            inner: RwLock::new(Current { docker: connect(&host), context, host }),
            generation: Default::default(),
        })
    }

    /// An engine on one socket, for tests.
    #[cfg(test)]
    pub fn on(context: &str, host: &str) -> Arc<Self> {
        Arc::new(Engine {
            inner: RwLock::new(Current { docker: connect(host), context: context.into(), host: host.into() }),
            generation: Default::default(),
        })
    }

    /// The client for the current context; reconnects when the socket has
    /// appeared since (an engine that was started after the app).
    pub fn docker(&self) -> Result<Docker, String> {
        if let Some(d) = self.inner.read().docker.clone() {
            return Ok(d);
        }
        let mut cur = self.inner.write();
        cur.docker = connect(&cur.host);
        cur.docker.clone().ok_or_else(|| format!("No engine at {}", cur.host))
    }

    pub fn context(&self) -> (String, String) {
        let c = self.inner.read();
        (c.context.clone(), c.host.clone())
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn switch(&self, name: &str) -> Result<(), String> {
        let ctx = list_contexts()
            .into_iter()
            .find(|c| c.name == name)
            .ok_or_else(|| format!("No docker context named {name}"))?;
        {
            let mut cur = self.inner.write();
            cur.docker = connect(&ctx.host);
            cur.context = ctx.name.clone();
            cur.host = ctx.host;
        }
        self.generation.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Settings::update(|s| (true, s.context = Some(ctx.name)));
        Ok(())
    }

    /// Drop the client, so the next call connects afresh (after the
    /// engine restarted and its socket was made again).
    pub fn reset(&self) {
        let mut cur = self.inner.write();
        cur.docker = connect(&cur.host);
    }
}

fn connect(host: &str) -> Option<Docker> {
    let path = host.strip_prefix("unix://")?;
    if !std::path::Path::new(path).exists() {
        return None;
    }
    Docker::connect_with_unix(host, 120, API_DEFAULT_VERSION).ok()
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

/// eDock's engine socket: `~/.edock/run/docker.sock`.
pub fn edock_host() -> String {
    format!("unix://{}", home().join(".edock/run/docker.sock").display())
}

fn exists(host: &str) -> bool {
    host.strip_prefix("unix://").is_some_and(|p| std::path::Path::new(p).exists())
}

/// eDock's engine, docker's default socket, and every context in
/// `~/.docker/contexts` with a unix socket.
pub fn list_contexts() -> Vec<Context> {
    let docker_dir = home().join(".docker");
    let current = std::fs::read_to_string(docker_dir.join("config.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["currentContext"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "default".into());

    let mut out = vec![];
    let edock = edock_host();
    out.push(Context {
        name: EDOCK.into(),
        description: "eDock engine".into(),
        available: exists(&edock),
        docker_current: current == EDOCK,
        host: edock,
    });
    let default = std::env::var("DOCKER_HOST")
        .ok()
        .filter(|h| h.starts_with("unix://"))
        .unwrap_or_else(|| "unix:///var/run/docker.sock".into());
    out.push(Context {
        name: "default".into(),
        description: "Docker's default socket".into(),
        available: exists(&default),
        docker_current: current == "default",
        host: default,
    });

    if let Ok(entries) = std::fs::read_dir(docker_dir.join("contexts/meta")) {
        let mut found = vec![];
        for e in entries.flatten() {
            let Ok(text) = std::fs::read_to_string(e.path().join("meta.json")) else { continue };
            let Ok(meta) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
            let Some(name) = meta["Name"].as_str() else { continue };
            let Some(host) = meta["Endpoints"]["docker"]["Host"].as_str() else { continue };
            if !host.starts_with("unix://") || out.iter().any(|c: &Context| c.name == name) {
                continue;
            }
            found.push(Context {
                name: name.into(),
                description: meta["Metadata"]["Description"].as_str().unwrap_or("").into(),
                available: exists(host),
                docker_current: current == name,
                host: host.into(),
            });
        }
        found.sort_by(|a, b| a.name.cmp(&b.name));
        out.extend(found);
    }
    out
}

/// The desktop app's own settings, in `~/.edock/desktop.json`.
#[derive(Default, Serialize, Deserialize)]
pub struct Settings {
    pub context: Option<String>,
    /// The welcome was shown (or skipped).
    #[serde(default)]
    pub onboarded: bool,
    /// No macOS notification when a container runs out of memory or turns
    /// unhealthy.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub alerts_off: bool,
    /// Compose projects seen on an engine or opened here, so one that is
    /// down (`docker compose down`) still shows, with Up.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<crate::compose::SavedProject>,
}

/// One change to the settings at a time: each reads, changes and writes
/// the whole file.
static SETTINGS: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

impl Settings {
    fn path() -> PathBuf {
        // Tests keep theirs to themselves, never the user's.
        if cfg!(test) {
            return std::env::temp_dir().join(format!("edock-desktop-test-{}.json", std::process::id()));
        }
        home().join(".edock/desktop.json")
    }
    pub fn load() -> Self {
        std::fs::read_to_string(Self::path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }
    fn save(&self) {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // Written beside it and moved in place: never half a file.
        let tmp = path.with_extension("json.tmp");
        if let Ok(text) = serde_json::to_string_pretty(self) {
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
    }
    /// Change the settings: `change` says whether it changed anything,
    /// and only then is the file written.
    pub fn update<R>(change: impl FnOnce(&mut Self) -> (bool, R)) -> R {
        let _one = SETTINGS.lock();
        let mut s = Self::load();
        let (changed, r) = change(&mut s);
        if changed {
            s.save();
        }
        r
    }
}

/// An exec's exit code once it has one. The output stream can end just
/// before the engine records it (Docker's does): asked again for up to two
/// seconds. -1 when it never has one (the process or the engine went).
pub async fn exec_exit_code(docker: &Docker, id: &str) -> i64 {
    for _ in 0..20 {
        match docker.inspect_exec(id).await {
            Ok(i) if i.running != Some(true) => return i.exit_code.unwrap_or(-1),
            Ok(_) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
            Err(_) => return -1,
        }
    }
    -1
}

/// Err(`why`) unless the app shows eDock's own engine.
pub fn only_on_edock(ctx: &elyra::Ctx, why: &str) -> Result<(), String> {
    if ctx.get::<Engine>().context().0 == EDOCK { Ok(()) } else { Err(why.into()) }
}

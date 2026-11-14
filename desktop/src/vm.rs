//! The Engine page's settings for eDock's own engine: the VM's CPUs and
//! memory (and what it uses now), the BuildKit builder's cache, importing
//! from OrbStack or Docker Desktop, and a diagnostics report.

use std::sync::atomic::{AtomicBool, Ordering};

use elyra::files::ChosenFile;
use elyra::{command, Ctx, EventBus};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::daemon::{engine_call, engine_request};
use crate::engine::{Engine, EDOCK};

fn on_edock(ctx: &Ctx) -> Result<(), String> {
    if ctx.get::<Engine>().context().0 == EDOCK {
        Ok(())
    } else {
        Err("This is eDock's own engine's: switch to it in the toolbar".into())
    }
}

fn answer(r: Option<(u16, Value)>) -> Result<Value, String> {
    match r {
        Some((s, v)) if s < 300 => Ok(v),
        Some((s, v)) => Err(v["message"].as_str().map(str::to_owned).unwrap_or_else(|| format!("The engine said {s}"))),
        None => Err("eDock's engine isn't running".into()),
    }
}

fn n(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

// ── The VM ─────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, specta::Type)]
pub struct VmInfo {
    /// What the VM starts with (`~/.edock/vm.json`).
    pub cpus: u32,
    pub memory_mib: u32,
    /// What it has now, while awake.
    pub running_cpus: u32,
    pub running_memory_mib: u32,
    pub awake: bool,
    pub max_cpus: u32,
    pub max_memory_mib: u32,
    /// EDOCK_CPUS or EDOCK_MEMORY_MIB fix them.
    pub overridden: bool,
    /// Memory the containers use now, and the most the VM may take (the
    /// balloon gives the rest back to macOS), in bytes.
    pub memory_used: f64,
    pub memory_ceiling: f64,
    /// The engine process's memory on the Mac, in bytes.
    pub engine_memory: f64,
    /// The guest agent's (PID 1 in the VM), in bytes.
    pub agent_memory: f64,
    pub cpu_percent: f64,
    pub oom_kills: u32,
}

#[command]
pub async fn vm_info(ctx: Ctx) -> Result<VmInfo, String> {
    on_edock(&ctx)?;
    let r = answer(engine_request("GET", "/edock/resources").await)?;
    let i = answer(engine_request("GET", "/info").await)?;
    Ok(VmInfo {
        cpus: n(&r["cpus"]) as u32,
        memory_mib: n(&r["memory_mib"]) as u32,
        running_cpus: n(&r["running_cpus"]) as u32,
        running_memory_mib: n(&r["running_memory_mib"]) as u32,
        awake: r["awake"] == true,
        max_cpus: n(&r["max_cpus"]) as u32,
        max_memory_mib: n(&r["max_memory_mib"]) as u32,
        overridden: r["overridden"] == true,
        memory_used: n(&i["EdockMemoryUsed"]),
        memory_ceiling: n(&i["EdockMemoryCeiling"]),
        engine_memory: daemon_rss().await,
        agent_memory: n(&i["EdockAgentMemory"]),
        cpu_percent: n(&i["EdockCpuPercent"]),
        oom_kills: n(&i["EdockOomKills"]) as u32,
    })
}

/// The engine process's resident memory, in bytes (`ps` reports KiB).
async fn daemon_rss() -> f64 {
    let Some(pid) = crate::daemon::engine_pid() else { return 0.0 };
    let out = tokio::process::Command::new("/bin/ps").args(["-o", "rss=", "-p", &pid.to_string()]).output().await;
    out.ok().and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<f64>().ok()).map_or(0.0, |kib| kib * 1024.0)
}

/// The VM's CPUs and memory, from its next start.
#[command]
pub async fn vm_set_resources(ctx: Ctx, cpus: u32, memory_mib: u32) -> Result<VmInfo, String> {
    on_edock(&ctx)?;
    answer(engine_call("POST", "/edock/resources", Some(&json!({"cpus": cpus, "memory_mib": memory_mib}))).await)?;
    vm_info::run(ctx).await
}

/// Start the VM again now, with its new size: running containers stop
/// (those with a restart policy come back).
#[command]
pub async fn vm_restart(ctx: Ctx) -> Result<(), String> {
    on_edock(&ctx)?;
    answer(engine_call("POST", "/edock/sleep", None).await)?;
    answer(engine_call("POST", "/edock/wake", None).await)?;
    let _ = ctx.get::<EventBus>().emit("docker:changed", &crate::streams::Changed { kind: "engine".into(), action: "restarted".into() });
    Ok(())
}

// ── The builder ────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, specta::Type)]
pub struct BuilderInfo {
    /// The cache's cap in GB; none: the default.
    pub cache_gb: Option<u32>,
    pub default_cache_gb: u32,
    /// Seconds without a build before BuildKit stops.
    pub idle_secs: u32,
    /// "running", "exited", "none"…
    pub state: String,
    pub overridden: bool,
}

#[command]
pub async fn builder_info(ctx: Ctx) -> Result<BuilderInfo, String> {
    on_edock(&ctx)?;
    let b = answer(engine_request("GET", "/edock/builder").await)?;
    Ok(BuilderInfo {
        cache_gb: b["cache_gb"].as_u64().filter(|g| *g > 0).map(|g| g as u32),
        default_cache_gb: n(&b["default_cache_gb"]) as u32,
        idle_secs: n(&b["idle_secs"]) as u32,
        state: b["state"].as_str().unwrap_or("none").into(),
        overridden: b["overridden"] == true,
    })
}

/// The builder's cache cap (none: the default) and idle time, from its
/// next start.
#[command]
pub async fn builder_set(ctx: Ctx, cache_gb: Option<u32>, idle_secs: u32) -> Result<BuilderInfo, String> {
    on_edock(&ctx)?;
    answer(engine_call("POST", "/edock/builder", Some(&json!({"cache_gb": cache_gb, "idle_secs": idle_secs}))).await)?;
    builder_info::run(ctx).await
}

/// Empty the build cache. Returns the bytes freed.
#[command]
pub async fn builder_prune(ctx: Ctx) -> Result<f64, String> {
    on_edock(&ctx)?;
    let v = answer(engine_call("POST", "/build/prune?all=1", None).await)?;
    Ok(n(&v["SpaceReclaimed"]))
}

// ── Proxy ──────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, specta::Type)]
pub struct ProxyInfo {
    /// "system" (macOS's settings), "custom" or "none".
    pub mode: String,
    pub http: String,
    pub https: String,
    pub no_proxy: String,
    /// New containers get HTTP_PROXY, HTTPS_PROXY and NO_PROXY too.
    pub containers: bool,
    /// What's used now (the system's, when the mode is "system").
    pub effective_http: Option<String>,
    pub effective_https: Option<String>,
    pub pac: bool,
}

fn proxy_from(v: &Value) -> ProxyInfo {
    let st = &v["settings"];
    let e = &v["effective"];
    ProxyInfo {
        mode: st["mode"].as_str().unwrap_or("system").into(),
        http: st["http"].as_str().unwrap_or("").into(),
        https: st["https"].as_str().unwrap_or("").into(),
        no_proxy: st["no_proxy"].as_str().unwrap_or("").into(),
        containers: st["containers"] == true,
        effective_http: e["http"].as_str().map(str::to_owned),
        effective_https: e["https"].as_str().map(str::to_owned),
        pac: e["pac"] == true,
    }
}

/// The proxy for pulls, pushes and logins (and, when chosen, containers).
#[command]
pub async fn proxy_info(ctx: Ctx) -> Result<ProxyInfo, String> {
    on_edock(&ctx)?;
    Ok(proxy_from(&answer(engine_request("GET", "/edock/proxy").await)?))
}

#[command]
pub async fn proxy_set(ctx: Ctx, mode: String, http: String, https: String, no_proxy: String, containers: bool) -> Result<ProxyInfo, String> {
    on_edock(&ctx)?;
    let body = json!({"mode": mode, "http": http, "https": https, "no_proxy": no_proxy, "containers": containers});
    Ok(proxy_from(&answer(engine_call("POST", "/edock/proxy", Some(&body)).await)?))
}

// ── Import from another engine ─────────────────────────────────────────

#[derive(Serialize, Deserialize, specta::Type)]
pub struct ImportSource {
    /// `orbstack` or `desktop-linux`, as `edock import` names them.
    pub id: String,
    pub label: String,
    /// Its engine answers on its socket.
    pub available: bool,
}

#[command]
pub async fn import_sources(_ctx: Ctx) -> Vec<ImportSource> {
    let contexts = crate::engine::list_contexts();
    [("orbstack", "OrbStack"), ("desktop-linux", "Docker Desktop")]
        .into_iter()
        .map(|(id, label)| ImportSource { id: id.into(), label: label.into(), available: contexts.iter().any(|c| c.name == id && c.available) })
        .collect()
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct ImportItem {
    pub name: String,
    /// eDock has it already.
    pub have: bool,
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct ImportList {
    pub images: Vec<ImportItem>,
    pub volumes: Vec<ImportItem>,
    pub containers: Vec<ImportItem>,
}

fn valid_source(source: &str) -> Result<(), String> {
    matches!(source, "orbstack" | "desktop-linux").then_some(()).ok_or_else(|| format!("{source} isn't an engine eDock imports from"))
}

/// What the other engine has (`edock import <source> --list`).
#[command]
pub async fn import_list(ctx: Ctx, source: String) -> Result<ImportList, String> {
    on_edock(&ctx)?;
    valid_source(&source)?;
    let bin = crate::daemon::engine_binary().ok_or("The eDock engine (edock) isn't installed")?;
    let out = tokio::process::Command::new(bin).args(["import", &source, "--list"]).output().await.map_err(|e| e.to_string())?;
    if !out.status.success() {
        let e = String::from_utf8_lossy(&out.stderr);
        return Err(e.trim().lines().last().unwrap_or("edock import failed").trim_start_matches("edock: ").to_owned());
    }
    Ok(parse_list(&String::from_utf8_lossy(&out.stdout)))
}

/// `edock import --list`'s text: a heading per kind, then indented names,
/// `(already in eDock)` after the ones eDock has.
fn parse_list(text: &str) -> ImportList {
    let mut list = ImportList { images: vec![], volumes: vec![], containers: vec![] };
    let mut section = "";
    for line in text.lines() {
        if let Some(h) = line.strip_suffix(':').filter(|_| !line.starts_with(' ')) {
            section = match h { "images" => "images", "volumes" => "volumes", "containers" => "containers", _ => "" };
            continue;
        }
        let item = line.trim();
        if item.is_empty() {
            continue;
        }
        let (name, have) = match item.strip_suffix("(already in eDock)") {
            Some(n) => (n.trim().to_owned(), true),
            None => (item.to_owned(), false),
        };
        let row = ImportItem { name, have };
        match section {
            "images" => list.images.push(row),
            "volumes" => list.volumes.push(row),
            "containers" => list.containers.push(row),
            _ => {}
        }
    }
    list
}

#[derive(Default)]
pub struct Importing(AtomicBool);

/// Bring everything over from the other engine (`edock import <source>`):
/// its lines on `import:progress`. The source is only read.
#[command]
pub async fn import_run(ctx: Ctx, source: String) -> Result<(), String> {
    on_edock(&ctx)?;
    valid_source(&source)?;
    let bin = crate::daemon::engine_binary().ok_or("The eDock engine (edock) isn't installed")?;
    let importing = ctx.get::<Importing>();
    if importing.0.swap(true, Ordering::SeqCst) {
        return Err("Importing already".into());
    }
    let i2 = importing.clone();
    let bus = ctx.get::<EventBus>();
    let b2 = bus.clone();
    let started = crate::daemon::run_edock(&bin, vec!["import".into(), source], bus, "import:progress", move || {
        i2.0.store(false, Ordering::SeqCst);
        let _ = b2.emit("docker:changed", &crate::streams::Changed { kind: "engine".into(), action: "imported".into() });
    });
    if started.is_err() {
        importing.0.store(false, Ordering::SeqCst);
    }
    started
}

// ── Diagnostics ────────────────────────────────────────────────────────

/// A diagnostics report (`edock diag report`), with tokens taken out,
/// saved where the user chose. Nothing is sent anywhere.
#[command]
pub async fn diag_report(_ctx: Ctx, file: ChosenFile) -> Result<(), String> {
    let bin = crate::daemon::engine_binary().ok_or("The eDock engine (edock) isn't installed")?;
    let out = tokio::process::Command::new(bin)
        .args(["diag", "report", "--out"])
        .arg(file.path())
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        let e = String::from_utf8_lossy(&out.stderr);
        Err(e.trim().lines().last().unwrap_or("edock diag report failed").trim_start_matches("edock: ").to_owned())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_import_list_reads_as_edock_prints_it() {
        let l = super::parse_list("images:\n  nginx:alpine  (already in eDock)\n  redis:7\nvolumes:\n  data\ncontainers:\n");
        assert_eq!(l.images.len(), 2);
        assert!(l.images[0].have && l.images[0].name == "nginx:alpine");
        assert!(!l.images[1].have);
        assert_eq!(l.volumes[0].name, "data");
        assert!(l.containers.is_empty());
    }

    #[test]
    fn only_known_engines_are_imported_from() {
        assert!(super::valid_source("orbstack").is_ok());
        assert!(super::valid_source("--help").is_err());
    }
}

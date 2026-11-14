//! What only eDock's own engine offers, through its `/edock/…` API:
//! container domains and their HTTPS, the ports a container really listens
//! on, publishing ports and changing the environment of a running container.

use elyra::{command, Ctx};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::daemon::{engine_call, engine_request};
use crate::engine::{Engine, EDOCK};

fn on_edock(ctx: &Ctx) -> Result<(), String> {
    if ctx.get::<Engine>().context().0 == EDOCK {
        Ok(())
    } else {
        Err("This needs eDock's engine: switch to it in the toolbar".into())
    }
}

/// An engine answer: its JSON on success, its message otherwise.
fn answer(r: Option<(u16, Value)>) -> Result<Value, String> {
    match r {
        Some((s, v)) if s < 300 => Ok(v),
        Some((s, v)) => Err(v["message"].as_str().map(str::to_owned).unwrap_or_else(|| format!("The engine said {s}"))),
        None => Err("eDock's engine isn't running".into()),
    }
}

/// A container id or name, as a path segment: the engine's own names and ids
/// only (letters, digits, `_`, `.`, `-`), so nothing can reach another path.
fn segment(id: &str) -> Result<&str, String> {
    if !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)) {
        Ok(id)
    } else {
        Err(format!("“{id}” isn't a container"))
    }
}

// ── Domains ────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, specta::Type)]
pub struct DomainsInfo {
    /// The app is on eDock's engine, and it answers.
    pub available: bool,
    pub http_port: u32,
    pub https_port: u32,
    /// The routers serve (off when another server has their port).
    pub http_on: bool,
    pub https_on: bool,
    /// `edock.localhost`, or `edock.local` once its resolver is set up.
    pub suffix: String,
    /// Browsers trust the HTTPS certificates.
    pub trusted: bool,
}

#[command]
pub async fn domains_info(ctx: Ctx) -> DomainsInfo {
    let mut d = DomainsInfo { available: false, http_port: 80, https_port: 443, http_on: false, https_on: false, suffix: "edock.localhost".into(), trusted: false };
    if on_edock(&ctx).is_err() {
        return d;
    }
    if let Some((200, v)) = engine_request("GET", "/edock/domains").await {
        d.available = true;
        d.http_port = v["http"]["port"].as_u64().unwrap_or(80) as u32;
        d.https_port = v["https"]["port"].as_u64().unwrap_or(443) as u32;
        d.http_on = v["http"]["running"] == true;
        d.https_on = v["https"]["running"] == true;
        d.suffix = v["suffix"].as_str().unwrap_or("edock.localhost").into();
        d.trusted = v["trusted"] == true;
    }
    d
}

/// Move the container domains to other ports, at once (kept in
/// `~/.edock/domains.json`).
#[command]
pub async fn domains_set_ports(ctx: Ctx, http: u32, https: u32) -> Result<DomainsInfo, String> {
    on_edock(&ctx)?;
    answer(engine_call("POST", "/edock/domains", Some(&json!({"http": http, "https": https}))).await)?;
    Ok(domains_info::run(ctx).await)
}

/// Trust eDock's certificate authority for websites, so
/// `https://<name>.edock.localhost` has a padlock. macOS asks for the
/// password. The authority can only vouch for eDock's own names.
#[command]
pub async fn tls_trust(_ctx: Ctx, trust: bool) -> Result<(), String> {
    let bin = crate::daemon::engine_binary().ok_or("The eDock engine (edock) isn't installed")?;
    let out = tokio::process::Command::new(bin)
        .args(["tls", if trust { "trust" } else { "untrust" }])
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(err.trim().lines().last().unwrap_or("edock tls failed").trim_start_matches("edock: ").to_owned())
    }
}

// ── Ports ──────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, PartialEq)]
pub struct PortBinding {
    pub container_port: u32,
    /// 0: a free one, chosen by the engine.
    pub host_port: u32,
    /// "tcp" or "udp".
    pub proto: String,
    /// "" for every interface, "127.0.0.1" for this Mac only.
    pub host_ip: String,
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct ContainerPorts {
    pub published: Vec<PortBinding>,
    /// TCP ports the container listens on, published or not.
    pub listening: Vec<u32>,
    /// …of those, the ones bound to its own loopback, which nothing outside
    /// it can reach.
    pub local: Vec<u32>,
}

/// What a container publishes, and what it really listens on.
#[command]
pub async fn container_ports(ctx: Ctx, id: String) -> Result<ContainerPorts, String> {
    on_edock(&ctx)?;
    let id = segment(&id)?;
    let docker = ctx.get::<Engine>().docker()?;
    let info = docker
        .inspect_container(id, None::<bollard::query_parameters::InspectContainerOptions>)
        .await
        .map_err(|e| e.to_string())?;
    let v = serde_json::to_value(&info).unwrap_or_default();
    // What it publishes now (a change while it runs shows here), else, when
    // stopped, what it was made with. The engine lists a binding on every
    // interface once per family (0.0.0.0 and ::): one binding here.
    let live = v["NetworkSettings"]["Ports"].as_object().filter(|m| m.values().any(|b| b.is_array()));
    let mut published = vec![];
    if let Some(map) = live.or(v["HostConfig"]["PortBindings"].as_object()) {
        for (key, binds) in map {
            let (port, proto) = key.split_once('/').unwrap_or((key, "tcp"));
            let Ok(container_port) = port.parse::<u32>() else { continue };
            for b in binds.as_array().into_iter().flatten() {
                let ip = b["HostIp"].as_str().unwrap_or("");
                published.push(PortBinding {
                    container_port,
                    host_port: b["HostPort"].as_str().and_then(|p| p.parse().ok()).unwrap_or(0),
                    proto: proto.into(),
                    host_ip: if matches!(ip, "0.0.0.0" | "::") { String::new() } else { ip.into() },
                });
            }
        }
    }
    published.sort_by_key(|p| (p.container_port, p.proto.clone(), p.host_port, p.host_ip.clone()));
    published.dedup();
    let running = v["State"]["Running"] == true;
    let (listening, local) = if running {
        let l = answer(engine_request("GET", &format!("/edock/containers/{id}/listening")).await)?;
        let ports = |k: &str| l[k].as_array().into_iter().flatten().filter_map(|p| p.as_u64()).map(|p| p as u32).collect::<Vec<_>>();
        (ports("tcp"), ports("local"))
    } else {
        (vec![], vec![])
    };
    Ok(ContainerPorts { published, listening, local })
}

/// Set a container's published ports, all of them, while it runs.
#[command]
pub async fn container_set_ports(ctx: Ctx, id: String, ports: Vec<PortBinding>) -> Result<(), String> {
    on_edock(&ctx)?;
    let id = segment(&id)?;
    for p in &ports {
        if !(1..=65535).contains(&p.container_port) || p.host_port > 65535 || !matches!(p.proto.as_str(), "tcp" | "udp") {
            return Err(format!("{}/{} isn't a port", p.container_port, p.proto));
        }
    }
    let body = json!({ "ports": ports });
    answer(engine_call("POST", &format!("/edock/containers/{id}/ports"), Some(&body)).await)?;
    let _ = ctx.get::<elyra::EventBus>().emit("docker:changed", &crate::streams::Changed { kind: "container".into(), action: "update".into() });
    Ok(())
}

/// Replace a container's environment (`NAME=value` each). A running
/// container restarts with it; its files are kept.
#[command]
pub async fn container_set_env(ctx: Ctx, id: String, env: Vec<String>) -> Result<(), String> {
    on_edock(&ctx)?;
    let id = segment(&id)?;
    if let Some(bad) = env.iter().find(|e| !e.split_once('=').is_some_and(|(k, _)| !k.is_empty() && !k.contains(char::is_whitespace))) {
        return Err(format!("“{bad}” isn't NAME=value"));
    }
    answer(engine_call("POST", &format!("/edock/containers/{id}/env"), Some(&json!({ "env": env }))).await)?;
    let _ = ctx.get::<elyra::EventBus>().emit("docker:changed", &crate::streams::Changed { kind: "container".into(), action: "update".into() });
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_container_segment_is_a_name_or_an_id() {
        for ok in ["web", "shop-web-1", "a1b2c3", "my_app.v2"] {
            assert!(super::segment(ok).is_ok(), "{ok}");
        }
        for bad in ["", "../images", "a/b", "web?x=1", "a b", "%2e%2e"] {
            assert!(super::segment(bad).is_err(), "{bad}");
        }
    }
}

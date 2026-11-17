//! Run image…: a container from an image, as `docker run -d` makes one,
//! with a name, published ports, Mac folders and volumes, environment
//! variables and a command.

use std::collections::HashMap;
use std::path::Path;

use bollard::models::{ContainerCreateBody, HostConfig, Mount, MountType, PortBinding, RestartPolicy, RestartPolicyNameEnum};
use bollard::query_parameters as q;
use elyra::{command, Ctx};
use serde::{Deserialize, Serialize};

use crate::engine::Engine;

#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, PartialEq)]
pub struct RunPort {
    /// The port on the Mac; none for one the engine picks.
    pub host: Option<u16>,
    pub container: u16,
    pub udp: bool,
}

#[derive(Serialize, Deserialize, specta::Type, Clone, Debug)]
pub struct RunMount {
    /// A Mac folder or file (an absolute path), or a volume's name (made
    /// when it isn't there).
    pub source: String,
    /// Where it shows in the container: an absolute path.
    pub target: String,
    pub read_only: bool,
}

#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, PartialEq)]
pub struct RunEnv {
    pub key: String,
    pub value: String,
}

#[derive(Serialize, Deserialize, specta::Type, Clone, Debug)]
pub struct RunSpec {
    pub image: String,
    /// The image's name when not given (`nginx`, then `nginx-2`…).
    pub name: Option<String>,
    pub ports: Vec<RunPort>,
    /// Ports open to other devices on the network, not this Mac only.
    pub lan: bool,
    pub mounts: Vec<RunMount>,
    pub env: Vec<RunEnv>,
    /// In place of the image's command, as shell words.
    pub command: Option<String>,
    /// Started again with the engine (`--restart unless-stopped`).
    pub restart: bool,
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct RunDefaults {
    /// Whether the image is here; when not, Run pulls it.
    pub local: bool,
    /// The name Run gives the container when none is given.
    pub name: String,
    /// The image's exposed ports, each on a free port of the Mac.
    pub ports: Vec<RunPort>,
    /// Where the image keeps data (its VOLUMEs).
    pub volumes: Vec<String>,
    /// Its command, for the placeholder.
    pub command: Option<String>,
}

#[derive(Serialize, Deserialize, specta::Type, Debug)]
pub struct RunStarted {
    pub id: String,
    pub name: String,
}

/// A container name from an image's: `ghcr.io/acme/my_app:1.2` → `my_app`.
fn name_from_image(image: &str) -> String {
    let no_digest = image.split('@').next().unwrap_or(image);
    let last = no_digest.rsplit('/').next().unwrap_or(no_digest);
    let repo = last.split(':').next().unwrap_or(last);
    let s: String = repo.chars().map(|c| if c.is_ascii_alphanumeric() || "_.-".contains(c) { c.to_ascii_lowercase() } else { '-' }).collect();
    let s = s.trim_matches(|c: char| !c.is_ascii_alphanumeric()).to_owned();
    if s.is_empty() || image.starts_with("sha256:") { "container".into() } else { s }
}

/// `want`, or `want-2`, `want-3`… when a container has it.
fn free_name(want: &str, taken: &[String]) -> String {
    if !taken.iter().any(|t| t == want) {
        return want.to_owned();
    }
    (2..).map(|i| format!("{want}-{i}")).find(|n| !taken.contains(n)).expect("some name is free")
}

/// The port on the Mac to suggest for a container port: the same one, or
/// 8000 and up for the privileged ones (80 → 8080), and the next free.
fn suggest_port(container: u16, taken: &[u16]) -> u16 {
    let want = if container < 1024 { 8000 + container } else { container };
    crate::recipes::free_port(want, taken)
}

/// Shell words, as `sh` splits them: quotes ('…' and "…") and backslashes,
/// no expansion.
fn words(s: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut started = false;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => cur.push(c),
                        None => return Err("A ' isn't closed in the command".into()),
                    }
                }
            }
            '"' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '\\' | '$' | '`')) => cur.push(c),
                            Some(c) => {
                                cur.push('\\');
                                cur.push(c);
                            }
                            None => return Err("A \" isn't closed in the command".into()),
                        },
                        Some(c) => cur.push(c),
                        None => return Err("A \" isn't closed in the command".into()),
                    }
                }
            }
            '\\' => {
                started = true;
                if let Some(c) = chars.next() {
                    cur.push(c);
                }
            }
            c if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            c => {
                started = true;
                cur.push(c);
            }
        }
    }
    if started {
        out.push(cur);
    }
    Ok(out)
}

/// A command as the dialog shows it: words with spaces or quotes quoted.
fn shown(cmd: &[String]) -> String {
    cmd.iter()
        .map(|w| if !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || "-_./=:,@%+".contains(c)) { w.clone() } else { format!("'{}'", w.replace('\'', "'\\''")) })
        .collect::<Vec<_>>()
        .join(" ")
}

/// What Run suggests for an image: a name, its ports on free ports of the
/// Mac, where it keeps data, and its command.
#[command(can = "edock.view")]
pub async fn image_run_defaults(ctx: Ctx, image: String) -> Result<RunDefaults, String> {
    let docker = ctx.get::<Engine>().docker()?;
    let taken: Vec<String> = container_names(&docker).await?;
    let name = free_name(&name_from_image(image.trim()), &taken);
    let Ok(info) = docker.inspect_image(image.trim()).await else {
        return Ok(RunDefaults { local: false, name, ports: Vec::new(), volumes: Vec::new(), command: None });
    };
    let config = info.config.unwrap_or_default();
    let mut exposed: Vec<(u16, bool)> = config
        .exposed_ports
        .unwrap_or_default()
        .iter()
        .filter_map(|p| {
            let (port, proto) = p.split_once('/').unwrap_or((p.as_str(), "tcp"));
            Some((port.parse().ok()?, proto == "udp"))
        })
        .collect();
    exposed.sort();
    let mut used = Vec::new();
    let ports = exposed
        .into_iter()
        .map(|(container, udp)| {
            let host = suggest_port(container, &used);
            used.push(host);
            RunPort { host: Some(host), container, udp }
        })
        .collect();
    let mut volumes: Vec<String> = config.volumes.unwrap_or_default().into_iter().collect();
    volumes.sort();
    let command = config.cmd.filter(|c| !c.is_empty()).map(|c| shown(&c));
    Ok(RunDefaults { local: true, name, ports, volumes, command })
}

async fn container_names(docker: &bollard::Docker) -> Result<Vec<String>, String> {
    Ok(docker
        .list_containers(Some(q::ListContainersOptionsBuilder::default().all(true).build()))
        .await
        .map_err(|e| e.to_string())?
        .iter()
        .filter_map(|c| c.names.as_ref()?.first().map(|n| n.trim_start_matches('/').to_owned()))
        .collect())
}

/// The container `spec` asks for, checked: everything but the name.
fn body(spec: &RunSpec) -> Result<ContainerCreateBody, String> {
    let image = spec.image.trim();
    if image.is_empty() || image.contains(char::is_whitespace) {
        return Err("Which image? For example nginx:alpine".into());
    }
    let mut bindings: HashMap<String, Option<Vec<PortBinding>>> = HashMap::new();
    let mut exposed = Vec::new();
    let mut hosts = Vec::new();
    for p in &spec.ports {
        if p.container == 0 {
            return Err("A port in the container is 1 to 65535".into());
        }
        let host = p.host.filter(|h| *h != 0);
        if let Some(h) = host {
            if hosts.contains(&(h, p.udp)) {
                return Err(format!("Port {h} on the Mac is given twice"));
            }
            hosts.push((h, p.udp));
        }
        let key = format!("{}/{}", p.container, if p.udp { "udp" } else { "tcp" });
        let binding = PortBinding { host_ip: Some(if spec.lan { "" } else { "127.0.0.1" }.into()), host_port: Some(host.map(|h| h.to_string()).unwrap_or_default()) };
        bindings.entry(key.clone()).or_insert_with(|| Some(Vec::new())).get_or_insert_with(Vec::new).push(binding);
        exposed.push(key);
    }
    let mut mounts = Vec::new();
    for m in &spec.mounts {
        let (source, target) = (m.source.trim(), m.target.trim());
        if !target.starts_with('/') || target.contains('\0') {
            return Err(format!("{target:?}: where it shows in the container is an absolute path, like /data"));
        }
        if mounts.iter().any(|x: &Mount| x.target.as_deref() == Some(target)) {
            return Err(format!("Two things are mounted at {target}"));
        }
        let (typ, source) = if source.starts_with('/') {
            (MountType::BIND, mac_path(source)?)
        } else {
            (MountType::VOLUME, crate::volumes::valid(source).map_err(|_| format!("{source:?} is neither a folder on your Mac (an absolute path) nor a volume's name"))?.to_owned())
        };
        mounts.push(Mount { typ: Some(typ), source: Some(source), target: Some(target.to_owned()), read_only: Some(m.read_only), ..Default::default() });
    }
    let mut env = Vec::new();
    for e in &spec.env {
        let key = e.key.trim();
        if key.is_empty() && e.value.is_empty() {
            continue;
        }
        if key.is_empty() || key.contains('=') || key.contains(char::is_whitespace) {
            return Err(format!("{key:?} isn't a variable's name"));
        }
        env.push(format!("{key}={}", e.value));
    }
    let cmd = match spec.command.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) => Some(words(c)?),
        None => None,
    };
    Ok(ContainerCreateBody {
        image: Some(image.to_owned()),
        env: (!env.is_empty()).then_some(env),
        cmd,
        exposed_ports: (!exposed.is_empty()).then_some(exposed),
        host_config: Some(HostConfig {
            port_bindings: (!bindings.is_empty()).then_some(bindings),
            mounts: (!mounts.is_empty()).then_some(mounts),
            restart_policy: spec.restart.then_some(RestartPolicy { name: Some(RestartPolicyNameEnum::UNLESS_STOPPED), maximum_retry_count: None }),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// A folder or file on the Mac that the engine shares: its real path.
fn mac_path(path: &str) -> Result<String, String> {
    let real = std::fs::canonicalize(Path::new(path)).map_err(|_| format!("{path} isn't there"))?;
    let shown = real.to_string_lossy().into_owned();
    if !crate::recipes::SHARED.iter().any(|s| shown.starts_with(s)) {
        return Err(format!("{path} isn't under /Users or /Volumes, which containers can reach"));
    }
    Ok(shown)
}

/// Make and start a container from an image (pulled when it isn't here),
/// as `docker run -d` does. A container that doesn't start is removed, so
/// Run can be tried again with the same name. Returns its ID and name.
#[command(can = "edock.manage")]
pub async fn image_run(ctx: Ctx, spec: RunSpec) -> Result<RunStarted, String> {
    let body = body(&spec)?;
    let docker = ctx.get::<Engine>().docker()?;
    let taken = container_names(&docker).await?;
    let name = match spec.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => {
            if !n.as_bytes()[0].is_ascii_alphanumeric() || !n.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)) {
                return Err("A name is letters, digits, dashes, dots and underscores".into());
            }
            if taken.iter().any(|t| t == n) {
                return Err(format!("There's a container named {n} already"));
            }
            n.to_owned()
        }
        None => free_name(&name_from_image(spec.image.trim()), &taken),
    };
    let image = spec.image.trim();
    if docker.inspect_image(image).await.is_err() {
        crate::resources::pull(&ctx, image.to_owned()).await?;
    }
    let id = docker.create_container(Some(q::CreateContainerOptionsBuilder::default().name(&name).build()), body).await.map_err(|e| e.to_string())?.id;
    if let Err(e) = docker.start_container(&id, None::<q::StartContainerOptions>).await {
        let _ = docker.remove_container(&id, Some(q::RemoveContainerOptionsBuilder::default().force(true).build())).await;
        return Err(format!("{name} didn't start: {e}"));
    }
    Ok(RunStarted { id, name })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_come_from_the_image() {
        assert_eq!(name_from_image("nginx:alpine"), "nginx");
        assert_eq!(name_from_image("ghcr.io/acme/My_App:1.2"), "my_app");
        assert_eq!(name_from_image("localhost:5000/shop/web@sha256:abc"), "web");
        assert_eq!(name_from_image("sha256:0123456789ab"), "container");
        assert_eq!(free_name("nginx", &["nginx".into(), "nginx-2".into()]), "nginx-3");
    }

    #[test]
    fn commands_split_as_sh_splits_them() {
        assert_eq!(words("sh -c 'echo hi; sleep 1'").unwrap(), ["sh", "-c", "echo hi; sleep 1"]);
        assert_eq!(words(r#"echo "a \"b\" c" d\ e ''"#).unwrap(), ["echo", "a \"b\" c", "d e", ""]);
        assert_eq!(words("  ").unwrap(), Vec::<String>::new());
        assert!(words("echo 'open").is_err());
        assert_eq!(shown(&["sh".into(), "-c".into(), "echo it's".into()]), r"sh -c 'echo it'\''s'");
        assert_eq!(words(&shown(&["sh".into(), "-c".into(), "echo it's".into()])).unwrap(), ["sh", "-c", "echo it's"]);
    }

    #[test]
    fn privileged_ports_are_suggested_high() {
        let p = suggest_port(80, &[]);
        assert!(p >= 8080, "{p}");
        assert!(suggest_port(5432, &[]) >= 5432);
    }

    fn spec() -> RunSpec {
        RunSpec { image: "nginx:alpine".into(), name: None, ports: vec![], lan: false, mounts: vec![], env: vec![], command: None, restart: false }
    }

    #[test]
    fn a_spec_becomes_a_container() {
        let home = std::env::var("HOME").unwrap();
        let s = RunSpec {
            ports: vec![RunPort { host: Some(8080), container: 80, udp: false }, RunPort { host: None, container: 53, udp: true }],
            mounts: vec![RunMount { source: home.clone(), target: "/src".into(), read_only: true }, RunMount { source: "data".into(), target: "/data".into(), read_only: false }],
            env: vec![RunEnv { key: "A".into(), value: "1=2".into() }, RunEnv { key: "".into(), value: "".into() }],
            command: Some("sh -c 'echo hi'".into()),
            restart: true,
            ..spec()
        };
        let b = body(&s).unwrap();
        let hc = b.host_config.unwrap();
        let pb = hc.port_bindings.unwrap();
        let web = pb["80/tcp"].as_ref().unwrap();
        assert_eq!((web[0].host_ip.as_deref(), web[0].host_port.as_deref()), (Some("127.0.0.1"), Some("8080")));
        assert_eq!(pb["53/udp"].as_ref().unwrap()[0].host_port.as_deref(), Some(""));
        let m = hc.mounts.unwrap();
        assert_eq!((m[0].typ, m[0].read_only), (Some(MountType::BIND), Some(true)));
        assert_eq!((m[1].typ, m[1].source.as_deref()), (Some(MountType::VOLUME), Some("data")));
        assert_eq!(b.env.unwrap(), ["A=1=2"]);
        assert_eq!(b.cmd.unwrap(), ["sh", "-c", "echo hi"]);
        assert_eq!(hc.restart_policy.unwrap().name, Some(RestartPolicyNameEnum::UNLESS_STOPPED));
        let lan = body(&RunSpec { lan: true, ports: vec![RunPort { host: Some(8080), container: 80, udp: false }], ..spec() }).unwrap();
        assert_eq!(lan.host_config.unwrap().port_bindings.unwrap()["80/tcp"].as_ref().unwrap()[0].host_ip.as_deref(), Some(""));
    }

    #[test]
    fn a_bad_spec_says_what_is_wrong() {
        let bad = |s: RunSpec| body(&s).unwrap_err();
        assert!(bad(RunSpec { image: " ".into(), ..spec() }).contains("Which image"));
        assert!(bad(RunSpec { ports: vec![RunPort { host: Some(8080), container: 80, udp: false }, RunPort { host: Some(8080), container: 81, udp: false }], ..spec() }).contains("twice"));
        assert!(bad(RunSpec { mounts: vec![RunMount { source: "data".into(), target: "data".into(), read_only: false }], ..spec() }).contains("absolute"));
        assert!(bad(RunSpec { mounts: vec![RunMount { source: "/etc".into(), target: "/x".into(), read_only: false }], ..spec() }).contains("/Users"));
        assert!(bad(RunSpec { mounts: vec![RunMount { source: "no good".into(), target: "/x".into(), read_only: false }], ..spec() }).contains("volume"));
        assert!(bad(RunSpec { env: vec![RunEnv { key: "A B".into(), value: "".into() }], ..spec() }).contains("variable"));
        assert!(bad(RunSpec { command: Some("echo 'x".into()), ..spec() }).contains("isn't closed"));
    }
}

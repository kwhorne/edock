//! Kubernetes: the engine's single-node k3s cluster (`edock k8s`), in the
//! `edock-k8s` container on eDock's engine.
//!
//! Starting, stopping and deleting it is the engine's own `edock k8s …`,
//! which writes the `edock` kubectl context. Everything the page reads or
//! does in the cluster is `kubectl` inside the node container, through the
//! engine's exec API, so the Mac needs neither kubectl nor a kubeconfig.

use bollard::container::LogOutput;
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::query_parameters as q;
use elyra::{command, Ctx, EventBus};
use futures_util::StreamExt;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::engine::{Engine, EDOCK};

/// The node container `edock k8s start` makes.
const NODE: &str = "edock-k8s";

/// What `edock k8s` is doing now, if anything.
#[derive(Default)]
pub struct Cluster {
    busy: Mutex<Option<String>>,
}

fn on_edock(ctx: &Ctx) -> bool {
    ctx.get::<Engine>().context().0 == EDOCK
}

/// A Kubernetes name (namespace, pod, container): what a DNS label or
/// subdomain may be. Also keeps a name from being read as a flag.
fn valid(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 253
        && name.bytes().next().is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
}

fn check(names: &[&str]) -> Result<(), String> {
    match names.iter().find(|n| !valid(n)) {
        Some(n) => Err(format!("“{n}” isn't a Kubernetes name")),
        None => Ok(()),
    }
}

/// `cmd` in the node, without a terminal: its exit code and output. A
/// starved API server can leave kubectl waiting for good, so it runs
/// under `timeout`.
async fn node_exec(ctx: &Ctx, cmd: &[&str], secs: u32) -> Result<(i64, String, String), String> {
    let docker = ctx.get::<Engine>().docker()?;
    let mut full: Vec<String> = vec!["timeout".into(), "-s".into(), "KILL".into(), secs.to_string()];
    full.extend(cmd.iter().map(|s| s.to_string()));
    let exec = docker
        .create_exec(NODE, CreateExecOptions { attach_stdout: Some(true), attach_stderr: Some(true), cmd: Some(full), ..Default::default() })
        .await
        .map_err(|e| e.to_string())?;
    let (mut out, mut err) = (Vec::new(), Vec::new());
    if let StartExecResults::Attached { mut output, .. } = docker.start_exec(&exec.id, None).await.map_err(|e| e.to_string())? {
        while let Some(Ok(chunk)) = output.next().await {
            match chunk {
                LogOutput::StdErr { message } => err.extend_from_slice(&message),
                other => out.extend_from_slice(&other.into_bytes()),
            }
        }
    }
    let code = crate::engine::exec_exit_code(&docker, &exec.id).await;
    Ok((code, String::from_utf8_lossy(&out).into_owned(), String::from_utf8_lossy(&err).into_owned()))
}

/// kubectl's own complaint, as one line.
fn complaint(code: i64, err: &str, out: &str) -> String {
    if code == 137 {
        return "kubectl got no answer in time: the cluster may still be starting".into();
    }
    let text = if err.trim().is_empty() { out } else { err };
    text.trim().lines().last().unwrap_or("kubectl failed").trim_start_matches("error: ").to_owned()
}

// ── The cluster ────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, specta::Type)]
pub struct K8sStatus {
    /// The app is on eDock's engine (the cluster lives there).
    pub on_edock: bool,
    /// "absent", "running", "exited", "created", …: the node container's state.
    pub state: String,
    /// The node answers and is Ready.
    pub ready: bool,
    /// The Mac port of the API server.
    pub port: Option<u32>,
    pub version: String,
    /// "start", "stop" or "delete" while `edock k8s` runs one.
    pub busy: Option<String>,
}

/// eDock's Kubernetes cluster: whether it exists, runs and is ready, its
/// version, and the API server's port on the Mac.
#[command(can = "edock.view")]
pub async fn k8s_status(ctx: Ctx) -> K8sStatus {
    let busy = ctx.get::<Cluster>().busy.lock().clone();
    let mut st = K8sStatus { on_edock: on_edock(&ctx), state: "absent".into(), ready: false, port: None, version: String::new(), busy };
    if !st.on_edock {
        return st;
    }
    let Ok(docker) = ctx.get::<Engine>().docker() else { return st };
    let Ok(info) = docker.inspect_container(NODE, None::<q::InspectContainerOptions>).await else { return st };
    let v = serde_json::to_value(&info).unwrap_or_default();
    st.state = v["State"]["Status"].as_str().unwrap_or("unknown").to_owned();
    st.version = v["Config"]["Image"].as_str().and_then(|i| i.rsplit_once(':')).map(|(_, t)| t.to_owned()).unwrap_or_default();
    st.port = v["NetworkSettings"]["Ports"]["6443/tcp"][0]["HostPort"]
        .as_str()
        .or_else(|| v["Config"]["Labels"]["app.edock.k8s.port"].as_str())
        .and_then(|p| p.parse().ok());
    if st.state == "running" {
        if let Ok((0, out, _)) = node_exec(&ctx, &["kubectl", "get", "nodes", "--no-headers"], 8).await {
            st.ready = out.split_whitespace().nth(1) == Some("Ready");
        }
    }
    st
}

/// Start, stop or delete the cluster: `edock k8s <action>`, with its lines
/// on `k8s:progress`. Returns at once; the last line says how it ended.
/// `use_context` (start) makes `edock` kubectl's current context.
#[command]
pub async fn k8s_cluster(ctx: Ctx, action: String, use_context: bool) -> Result<(), String> {
    if !matches!(action.as_str(), "start" | "stop" | "delete") {
        return Err(format!("Unknown action {action}"));
    }
    if !on_edock(&ctx) {
        return Err("Kubernetes runs on the eDock engine: switch to it first".into());
    }
    let bin = crate::daemon::engine_binary().ok_or("The eDock engine (edock) isn't installed")?;
    let cluster = ctx.get::<Cluster>();
    {
        let mut busy = cluster.busy.lock();
        if let Some(b) = busy.as_ref() {
            return Err(format!("Kubernetes is busy: {b}"));
        }
        *busy = Some(action.clone());
    }
    let mut args = vec!["k8s".to_string(), action.clone()];
    if action == "start" && use_context {
        args.push("--use".into());
    }
    let bus = ctx.get::<EventBus>();
    let c2 = cluster.clone();
    let started = crate::daemon::run_edock(&bin, args, bus.clone(), "k8s:progress", move || {
        *c2.busy.lock() = None;
        let _ = bus.emit("docker:changed", &crate::streams::Changed { kind: "k8s".into(), action: "done".into() });
    });
    if let Err(e) = started {
        *cluster.busy.lock() = None;
        return Err(e);
    }
    let _ = ctx.get::<EventBus>().emit("docker:changed", &crate::streams::Changed { kind: "k8s".into(), action });
    Ok(())
}

// ── What runs in it ────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, specta::Type, Clone)]
pub struct PodContainer {
    pub name: String,
    pub image: String,
    pub ready: bool,
    pub restarts: u32,
    /// running, waiting or terminated.
    pub state: String,
    pub reason: String,
    /// What Kubernetes says about it (why it waits, how it ended).
    pub message: String,
    /// How it ended last time, when it keeps restarting: "Error, exit code 1".
    pub last: Option<String>,
}

#[derive(Serialize, Deserialize, specta::Type, Clone)]
pub struct PodRow {
    pub namespace: String,
    pub name: String,
    /// What `kubectl get pods` says: Running, Pending, CrashLoopBackOff, …
    pub status: String,
    pub ready: String,
    pub restarts: u32,
    /// RFC 3339.
    pub created: String,
    pub ip: String,
    /// Its Deployment, StatefulSet, DaemonSet or Job, as `Kind/name`.
    pub owner: Option<String>,
    pub containers: Vec<PodContainer>,
    /// Why it's stuck, in a line, when it is (it can't be scheduled, an
    /// image can't be pulled, a container keeps exiting).
    pub why: Option<String>,
    /// The image it can't pull, when that's why: one built or pulled on
    /// eDock can be loaded into the cluster (`k8s_load`).
    pub pull_failed: Option<String>,
}

#[derive(Serialize, Deserialize, specta::Type, Clone)]
pub struct ServicePort {
    pub name: String,
    pub port: u32,
    pub target: String,
    pub node_port: Option<u32>,
    pub protocol: String,
}

#[derive(Serialize, Deserialize, specta::Type, Clone)]
pub struct ServiceRow {
    pub namespace: String,
    pub name: String,
    /// ClusterIP, NodePort, LoadBalancer, ExternalName.
    pub kind: String,
    pub cluster_ip: String,
    pub ports: Vec<ServicePort>,
    pub created: String,
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct K8sItems {
    pub pods: Vec<PodRow>,
    pub services: Vec<ServiceRow>,
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or("").to_owned()
}

/// A pod as `kubectl get pods` shows it.
fn pod(p: &Value) -> PodRow {
    let (meta, spec, st) = (&p["metadata"], &p["spec"], &p["status"]);
    let statuses = st["containerStatuses"].as_array().cloned().unwrap_or_default();
    let containers: Vec<PodContainer> = spec["containers"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| {
            let name = s(&c["name"]);
            let cs = statuses.iter().find(|x| x["name"] == c["name"]).cloned().unwrap_or(Value::Null);
            let (state, reason, message) = match cs["state"].as_object().and_then(|o| o.iter().next()) {
                Some((k, v)) => (k.clone(), s(&v["reason"]), s(&v["message"])),
                None => ("waiting".into(), String::new(), String::new()),
            };
            let ended = &cs["lastState"]["terminated"];
            let last = ended.is_object().then(|| {
                let why = ended["reason"].as_str().filter(|r| !r.is_empty()).unwrap_or("Ended");
                format!("{why}, exit code {}", ended["exitCode"].as_i64().unwrap_or(-1))
            });
            PodContainer {
                image: s(&c["image"]),
                ready: cs["ready"].as_bool().unwrap_or(false),
                restarts: cs["restartCount"].as_u64().unwrap_or(0) as u32,
                state,
                reason,
                message,
                last,
                name,
            }
        })
        .collect();
    let ready = containers.iter().filter(|c| c.ready).count();
    // kubectl's STATUS: being deleted, then a container's waiting or
    // terminated reason, then the phase.
    let status = if meta["deletionTimestamp"].is_string() {
        "Terminating".to_owned()
    } else if let Some(r) = containers.iter().find(|c| c.state == "waiting" && !c.reason.is_empty()).map(|c| c.reason.clone()) {
        r
    } else if let Some(r) = containers.iter().find(|c| c.state == "terminated" && !c.reason.is_empty()).map(|c| c.reason.clone()) {
        r
    } else {
        st["reason"].as_str().map(str::to_owned).unwrap_or_else(|| s(&st["phase"]))
    };
    // A ReplicaSet's pod belongs to its Deployment: the name less the hash.
    let owner = meta["ownerReferences"][0].as_object().map(|o| {
        let kind = o.get("kind").and_then(Value::as_str).unwrap_or("");
        let name = o.get("name").and_then(Value::as_str).unwrap_or("");
        match kind {
            "ReplicaSet" => format!("Deployment/{}", name.rsplit_once('-').map(|(d, _)| d).unwrap_or(name)),
            _ => format!("{kind}/{name}"),
        }
    });
    let (why, pull_failed) = stuck(st, &containers);
    PodRow {
        namespace: s(&meta["namespace"]),
        name: s(&meta["name"]),
        why,
        pull_failed,
        status,
        ready: format!("{ready}/{}", containers.len()),
        restarts: containers.iter().map(|c| c.restarts).sum(),
        created: s(&meta["creationTimestamp"]),
        ip: s(&st["podIP"]),
        owner,
        containers,
    }
}

/// Why a pod is stuck, in a line, and the image it can't pull if that's it.
fn stuck(st: &Value, containers: &[PodContainer]) -> (Option<String>, Option<String>) {
    const PULL: [&str; 4] = ["ErrImagePull", "ImagePullBackOff", "ErrImageNeverPull", "InvalidImageName"];
    if let Some(c) = containers.iter().find(|c| c.state == "waiting" && PULL.contains(&c.reason.as_str())) {
        let said = c.message.lines().next().unwrap_or("").trim();
        let why = if said.is_empty() { format!("{} can't be pulled", c.image) } else { format!("{} can't be pulled: {said}", c.image) };
        return (Some(why), Some(c.image.clone()));
    }
    if let Some(c) = containers.iter().find(|c| c.state == "waiting" && c.reason == "CrashLoopBackOff") {
        let how = c.last.as_deref().map(|l| format!(" ({l})")).unwrap_or_default();
        return (Some(format!("{} keeps exiting{how}: its log says why", c.name)), None);
    }
    if let Some(c) = containers.iter().find(|c| c.state == "waiting" && !c.reason.is_empty() && c.reason != "ContainerCreating" && c.reason != "PodInitializing") {
        let said = c.message.lines().next().unwrap_or("").trim();
        return (Some(if said.is_empty() { format!("{}: {}", c.name, c.reason) } else { format!("{}: {said}", c.name) }), None);
    }
    // Not placed on the node: no room, a missing volume, a taint…
    let unscheduled = st["conditions"].as_array().into_iter().flatten().find(|c| c["type"] == "PodScheduled" && c["status"] == "False");
    if let Some(c) = unscheduled {
        let said = c["message"].as_str().or(c["reason"].as_str()).unwrap_or("it can't be placed");
        return (Some(format!("Not scheduled: {said}")), None);
    }
    (None, None)
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct K8sEvent {
    /// Normal or Warning.
    pub kind: String,
    pub reason: String,
    pub message: String,
    pub count: u32,
    /// RFC 3339: when it last happened.
    pub last: String,
}

/// What Kubernetes said about a pod (scheduled, pulling, backing off…),
/// oldest first, as `kubectl describe pod` lists it.
#[command(can = "edock.view")]
pub async fn k8s_pod_events(ctx: Ctx, namespace: String, pod: String) -> Result<Vec<K8sEvent>, String> {
    check(&[&namespace, &pod])?;
    let selector = format!("involvedObject.kind=Pod,involvedObject.name={pod}");
    let (code, out, err) = node_exec(&ctx, &["kubectl", "get", "events", "-n", &namespace, "--field-selector", &selector, "-o", "json", "--request-timeout=20s"], 30).await?;
    if code != 0 {
        return Err(complaint(code, &err, &out));
    }
    let list: Value = serde_json::from_str(&out).map_err(|e| format!("kubectl's answer: {e}"))?;
    Ok(events(&list))
}

fn events(list: &Value) -> Vec<K8sEvent> {
    let mut out: Vec<K8sEvent> = list["items"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| K8sEvent {
            kind: s(&e["type"]),
            reason: s(&e["reason"]),
            message: s(&e["message"]),
            count: e["count"].as_u64().unwrap_or(1) as u32,
            last: [&e["lastTimestamp"], &e["eventTime"], &e["firstTimestamp"], &e["metadata"]["creationTimestamp"]].into_iter().find_map(|t| t.as_str()).unwrap_or("").to_owned(),
        })
        .collect();
    out.sort_by(|a, b| a.last.cmp(&b.last));
    out
}

/// Load an image from eDock (built or pulled here) into the cluster, where
/// pods find it without a registry: `edock k8s load`. Returns what it said.
#[command(can = "edock.manage")]
pub async fn k8s_load(ctx: Ctx, image: String) -> Result<String, String> {
    if !on_edock(&ctx) {
        return Err("Kubernetes runs on the eDock engine: switch to it first".into());
    }
    let image = image.trim().to_owned();
    if image.is_empty() || image.starts_with('-') || image.contains(char::is_whitespace) {
        return Err(format!("{image:?} isn't an image"));
    }
    let bin = crate::daemon::engine_binary().ok_or("The eDock engine (edock) isn't installed")?;
    let out = tokio::process::Command::new(bin).args(["k8s", "load", &image]).stdin(std::process::Stdio::null()).output().await.map_err(|e| e.to_string())?;
    let last = |b: &[u8]| String::from_utf8_lossy(b).lines().map(str::trim).rev().find(|l| !l.is_empty()).map(str::to_owned);
    if out.status.success() {
        Ok(last(&out.stdout).unwrap_or_else(|| format!("{image} is in Kubernetes")))
    } else {
        Err(last(&out.stderr).map(|e| e.trim_start_matches("edock: ").to_owned()).unwrap_or_else(|| "edock k8s load failed".into()))
    }
}

fn service(v: &Value) -> ServiceRow {
    let (meta, spec) = (&v["metadata"], &v["spec"]);
    ServiceRow {
        namespace: s(&meta["namespace"]),
        name: s(&meta["name"]),
        kind: s(&spec["type"]),
        cluster_ip: s(&spec["clusterIP"]),
        ports: spec["ports"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| ServicePort {
                name: s(&p["name"]),
                port: p["port"].as_u64().unwrap_or(0) as u32,
                target: match &p["targetPort"] {
                    Value::Number(n) => n.to_string(),
                    other => s(other),
                },
                node_port: p["nodePort"].as_u64().map(|n| n as u32),
                protocol: s(&p["protocol"]),
            })
            .collect(),
        created: s(&meta["creationTimestamp"]),
    }
}

/// Every pod and service, in every namespace.
#[command(can = "edock.view")]
pub async fn k8s_items(ctx: Ctx) -> Result<K8sItems, String> {
    let (code, out, err) = node_exec(&ctx, &["kubectl", "get", "pods,services", "--all-namespaces", "-o", "json", "--request-timeout=20s"], 30).await?;
    if code != 0 {
        return Err(complaint(code, &err, &out));
    }
    let list: Value = serde_json::from_str(&out).map_err(|e| format!("kubectl's answer: {e}"))?;
    let items = list["items"].as_array().cloned().unwrap_or_default();
    let mut pods: Vec<PodRow> = items.iter().filter(|i| i["kind"] == "Pod").map(pod).collect();
    let mut services: Vec<ServiceRow> = items.iter().filter(|i| i["kind"] == "Service").map(service).collect();
    pods.sort_by(|a, b| a.namespace.cmp(&b.namespace).then(a.name.cmp(&b.name)));
    services.sort_by(|a, b| a.namespace.cmp(&b.namespace).then(a.name.cmp(&b.name)));
    Ok(K8sItems { pods, services })
}

/// Delete a pod; its Deployment (if any) makes a new one.
#[command]
pub async fn k8s_pod_delete(ctx: Ctx, namespace: String, pod: String) -> Result<(), String> {
    check(&[&namespace, &pod])?;
    let (code, out, err) = node_exec(&ctx, &["kubectl", "delete", "pod", "-n", &namespace, &pod, "--wait=false"], 30).await?;
    if code != 0 {
        return Err(complaint(code, &err, &out));
    }
    Ok(())
}

/// Restart a Deployment's pods, one by one (`kubectl rollout restart`).
#[command]
pub async fn k8s_rollout_restart(ctx: Ctx, namespace: String, deployment: String) -> Result<(), String> {
    check(&[&namespace, &deployment])?;
    let target = format!("deployment/{deployment}");
    let (code, out, err) = node_exec(&ctx, &["kubectl", "rollout", "restart", "-n", &namespace, &target], 30).await?;
    if code != 0 {
        return Err(complaint(code, &err, &out));
    }
    Ok(())
}

/// Follow a pod container's log (`kubectl logs -f`), on `logs:<id>` like a
/// container's.
#[command]
pub async fn k8s_logs_follow(ctx: Ctx, namespace: String, pod: String, container: String, tail: u32, timestamps: bool) -> Result<u32, String> {
    check(&[&namespace, &pod, &container])?;
    let docker = ctx.get::<Engine>().docker()?;
    let mut cmd: Vec<String> = ["kubectl", "logs", "-f", "-n", &namespace, &pod, "-c", &container].map(String::from).to_vec();
    cmd.push(format!("--tail={tail}"));
    if timestamps {
        cmd.push("--timestamps".into());
    }
    let exec = docker
        .create_exec(NODE, CreateExecOptions { attach_stdout: Some(true), attach_stderr: Some(true), cmd: Some(cmd), ..Default::default() })
        .await
        .map_err(|e| e.to_string())?;
    let StartExecResults::Attached { output, .. } = docker.start_exec(&exec.id, None).await.map_err(|e| e.to_string())? else {
        return Err("The engine started kubectl detached".into());
    };
    // Stopping the stream drops the connection, and with it kubectl.
    Ok(crate::streams::follow_lines(&ctx, output) as u32)
}

/// A shell in a pod's container (`kubectl exec -it`), on `exec:<id>` like
/// a container's terminal.
#[command]
pub async fn k8s_exec_start(ctx: Ctx, namespace: String, pod: String, container: String, cols: u32, rows: u32) -> Result<u32, String> {
    check(&[&namespace, &pod, &container])?;
    let shell = "if command -v bash >/dev/null 2>&1; then exec bash; else exec sh; fi";
    let cmd: Vec<String> = ["kubectl", "exec", "-it", "-n", &namespace, &pod, "-c", &container, "--", "sh", "-c", shell].map(String::from).to_vec();
    crate::streams::open_terminal(&ctx, NODE, cmd, cols, rows).await
}

// ── Forward to the Mac ─────────────────────────────────────────────────

/// A port on the Mac (127.0.0.1) that reaches an address in the cluster.
/// The engine holds them, so they're the same whenever the app asks.
#[derive(Serialize, Deserialize, specta::Type, Clone)]
pub struct Forward {
    pub ip: String,
    pub port: u32,
    pub local: u32,
}

/// Reach a ClusterIP service (or a pod) from the Mac: a port on
/// 127.0.0.1 forwarding to `ip:port` in the cluster. Asking again for the
/// same address gives the same port.
#[command]
pub async fn k8s_forward(ctx: Ctx, ip: String, port: u32) -> Result<Forward, String> {
    if !on_edock(&ctx) {
        return Err("Kubernetes runs on the eDock engine: switch to it first".into());
    }
    let addr: std::net::Ipv4Addr = ip.parse().map_err(|_| format!("{ip} isn't an address in the cluster"))?;
    if !(1..=65535).contains(&port) {
        return Err(format!("{port} isn't a port"));
    }
    let body = serde_json::json!({"ip": addr.to_string(), "port": port});
    match crate::daemon::engine_call("POST", "/edock/k8s/forward", Some(&body)).await {
        Some((s, v)) if s < 300 => Ok(Forward { ip, port, local: v["port"].as_u64().unwrap_or(0) as u32 }),
        Some((_, v)) => Err(v["message"].as_str().unwrap_or("The engine refused").to_owned()),
        None => Err("eDock's engine isn't running".into()),
    }
}

#[command]
pub async fn k8s_unforward(_ctx: Ctx, local: u32) -> Result<(), String> {
    match crate::daemon::engine_call("DELETE", &format!("/edock/k8s/forward/{local}"), None).await {
        Some((s, _)) if s < 300 => Ok(()),
        Some((_, v)) => Err(v["message"].as_str().unwrap_or("The engine refused").to_owned()),
        None => Err("eDock's engine isn't running".into()),
    }
}

/// The forwards open now, as the engine has them.
#[command]
pub async fn k8s_forwards(ctx: Ctx) -> Vec<Forward> {
    if !on_edock(&ctx) {
        return vec![];
    }
    match crate::daemon::engine_request("GET", "/edock/k8s/forward").await {
        Some((200, v)) => v
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| Forward { ip: f["ip"].as_str().unwrap_or("").into(), port: f["port"].as_u64().unwrap_or(0) as u32, local: f["local"].as_u64().unwrap_or(0) as u32 })
            .collect(),
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn names_are_kubernetes_names_and_never_flags() {
        for ok in ["default", "kube-system", "web-7d9f8c-x2b4z", "a.b"] {
            assert!(valid(ok), "{ok}");
        }
        for bad in ["", "--all", "-n", "Web", "a b", "a/b", "ä"] {
            assert!(!valid(bad), "{bad}");
        }
    }

    #[test]
    fn a_stuck_pod_says_why() {
        let waiting = |reason: &str, message: &str| {
            json!({"metadata": {"name": "web-1", "namespace": "default"}, "spec": {"containers": [{"name": "web", "image": "myapp:dev"}]},
                   "status": {"phase": "Pending", "containerStatuses": [{"name": "web", "ready": false, "restartCount": 0, "state": {"waiting": {"reason": reason, "message": message}}}]}})
        };
        let p = pod(&waiting("ErrImagePull", "failed to pull and unpack image \"docker.io/library/myapp:dev\": not found"));
        assert_eq!(p.pull_failed.as_deref(), Some("myapp:dev"));
        assert!(p.why.as_deref().unwrap().starts_with("myapp:dev can't be pulled: failed to pull"), "{:?}", p.why);
        assert_eq!(pod(&waiting("ImagePullBackOff", "")).why.as_deref(), Some("myapp:dev can't be pulled"));
        assert!(pod(&waiting("ContainerCreating", "")).why.is_none(), "starting isn't stuck");

        let crash = json!({"metadata": {"name": "api", "namespace": "default"}, "spec": {"containers": [{"name": "api", "image": "api:1"}]},
            "status": {"phase": "Running", "containerStatuses": [{"name": "api", "ready": false, "restartCount": 4,
                "state": {"waiting": {"reason": "CrashLoopBackOff"}}, "lastState": {"terminated": {"reason": "Error", "exitCode": 3}}}]}});
        let p = pod(&crash);
        assert_eq!(p.why.as_deref(), Some("api keeps exiting (Error, exit code 3): its log says why"));
        assert_eq!(p.containers[0].last.as_deref(), Some("Error, exit code 3"));
        assert!(p.pull_failed.is_none());

        let unplaced = json!({"metadata": {"name": "big", "namespace": "default"}, "spec": {"containers": [{"name": "big", "image": "big:1"}]},
            "status": {"phase": "Pending", "conditions": [{"type": "PodScheduled", "status": "False", "reason": "Unschedulable", "message": "0/1 nodes are available: 1 Insufficient memory."}]}});
        assert_eq!(pod(&unplaced).why.as_deref(), Some("Not scheduled: 0/1 nodes are available: 1 Insufficient memory."));
    }

    #[test]
    fn events_come_oldest_first() {
        let list = json!({"items": [
            {"type": "Warning", "reason": "Failed", "message": "pull failed", "count": 3, "lastTimestamp": "2026-11-17T10:00:05Z"},
            {"type": "Normal", "reason": "Scheduled", "message": "assigned", "eventTime": "2026-11-17T10:00:00.000000Z"},
        ]});
        let e = events(&list);
        assert_eq!(e.iter().map(|e| e.reason.as_str()).collect::<Vec<_>>(), ["Scheduled", "Failed"]);
        assert_eq!((e[1].kind.as_str(), e[1].count), ("Warning", 3));
        assert_eq!(e[0].count, 1);
    }

    #[test]
        fn a_pod_reads_as_kubectl_prints_it() {
        let p = json!({
            "metadata": {"namespace": "default", "name": "web-5d8f-abcde", "creationTimestamp": "2026-11-14T10:00:00Z",
                         "ownerReferences": [{"kind": "ReplicaSet", "name": "web-5d8f"}]},
            "spec": {"containers": [{"name": "nginx", "image": "nginx:alpine"}, {"name": "side", "image": "busybox"}]},
            "status": {"phase": "Running", "podIP": "10.42.0.9", "containerStatuses": [
                {"name": "nginx", "ready": true, "restartCount": 1, "state": {"running": {"startedAt": "x"}}},
                {"name": "side", "ready": false, "restartCount": 4, "state": {"waiting": {"reason": "CrashLoopBackOff"}}}
            ]}
        });
        let row = pod(&p);
        assert_eq!(row.status, "CrashLoopBackOff");
        assert_eq!(row.ready, "1/2");
        assert_eq!(row.restarts, 5);
        assert_eq!(row.owner.as_deref(), Some("Deployment/web"));
        let ending = json!({"metadata": {"name": "x", "deletionTimestamp": "t"}, "spec": {"containers": []}, "status": {"phase": "Running"}});
        assert_eq!(pod(&ending).status, "Terminating");
    }

    #[test]
    fn a_service_keeps_its_ports() {
        let v = json!({"metadata": {"namespace": "default", "name": "web"}, "spec": {"type": "LoadBalancer", "clusterIP": "10.43.0.5",
            "ports": [{"port": 8080, "targetPort": 80, "nodePort": 31000, "protocol": "TCP"}]}});
        let row = service(&v);
        assert_eq!(row.kind, "LoadBalancer");
        assert_eq!((row.ports[0].port, row.ports[0].target.as_str(), row.ports[0].node_port), (8080, "80", Some(31000)));
    }
}

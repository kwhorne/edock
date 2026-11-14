//! Containers, images, volumes and networks: what the lists show, and what
//! the buttons do. Every command works on the current context's engine.

use std::collections::HashMap;

use bollard::query_parameters as q;
use elyra::{command, Ctx};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::engine::Engine;


fn docker(ctx: &Ctx) -> Result<bollard::Docker, String> {
    ctx.get::<Engine>().docker()
}

fn err(e: impl std::fmt::Display) -> String {
    let s = e.to_string();
    // bollard's "Docker responded with status code 409: <message>" → the message.
    match s.split_once(": ") {
        Some((head, msg)) if head.starts_with("Docker responded") => msg.to_owned(),
        _ => s,
    }
}

fn json<T: Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or("").to_owned()
}

// ── Engine ─────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, specta::Type)]
pub struct EngineStatus {
    pub context: String,
    pub host: String,
    pub connected: bool,
    /// "running", "asleep" (eDock's VM sleeps until needed), "stopped"
    /// (eDock turned off with Stop) or "off" (nothing answers).
    pub state: String,
    /// This app's version; eDock's engine should run the same.
    pub app_version: String,
    pub error: Option<String>,
    pub name: String,
    pub version: String,
    pub os: String,
    pub arch: String,
    pub cpus: u32,
    pub memory: f64,
    pub containers: u32,
    pub running: u32,
    pub images: u32,
}

/// Which engine the app is on, and whether it answers.
#[command]
pub async fn engine_status(ctx: Ctx) -> EngineStatus {
    let engine = ctx.get::<Engine>();
    let (context, host) = engine.context();
    let mut st = EngineStatus {
        state: "off".into(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        context,
        host,
        connected: false,
        error: None,
        name: String::new(),
        version: String::new(),
        os: String::new(),
        arch: String::new(),
        cpus: 0,
        memory: 0.0,
        containers: 0,
        running: 0,
        images: 0,
    };
    // eDock's own engine is asked as a background look, which doesn't
    // keep its VM awake; others through the Docker client.
    let info: Result<Value, String> = if st.context == crate::engine::EDOCK {
        match crate::daemon::engine_request("GET", "/info").await {
            Some((200, v)) => Ok(v),
            Some((s, v)) => Err(v["message"].as_str().map(str::to_owned).unwrap_or_else(|| format!("The engine said {s}"))),
            None => Err(format!("No engine at {}", st.host)),
        }
    } else {
        match engine.docker() {
            Ok(d) => d.info().await.map(|i| json(&i)).map_err(err),
            Err(e) => Err(e),
        }
    };
    match info {
        Ok(i) => {
            st.connected = true;
            st.name = s(&i["Name"]);
            st.version = s(&i["ServerVersion"]);
            st.os = s(&i["OperatingSystem"]);
            st.arch = s(&i["Architecture"]);
            st.cpus = i["NCPU"].as_u64().unwrap_or(0) as u32;
            st.memory = i["MemTotal"].as_f64().unwrap_or(0.0);
            st.containers = i["Containers"].as_u64().unwrap_or(0) as u32;
            st.running = i["ContainersRunning"].as_u64().unwrap_or(0) as u32;
            st.images = i["Images"].as_u64().unwrap_or(0) as u32;
            st.state = "running".into();
        }
        Err(e) => {
            st.error = Some(e);
            engine.reset();
        }
    }
    // eDock's own engine says more than Docker's API does.
    if st.context == crate::engine::EDOCK {
        use crate::daemon::EngineState::*;
        st.state = match crate::daemon::engine_state().await {
            Off => "off",
            Stopped => "stopped",
            Asleep => "asleep",
            Running => "running",
        }
        .into();
    }
    st
}

/// The docker contexts the app can switch between.
#[command]
pub async fn contexts_list(_ctx: Ctx) -> Vec<crate::engine::Context> {
    crate::engine::list_contexts()
}

/// Switch the app to another context's engine.
#[command]
pub async fn context_use(ctx: Ctx, name: String) -> Result<(), String> {
    ctx.get::<Engine>().switch(&name)?;
    let _ = ctx.get::<elyra::EventBus>().emit("docker:changed", &crate::streams::Changed { kind: "context".into(), action: "switched".into() });
    Ok(())
}

// ── Containers ─────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, specta::Type, Clone)]
pub struct PortRow {
    pub ip: String,
    pub private: u32,
    pub public: Option<u32>,
    pub proto: String,
}

#[derive(Serialize, Deserialize, specta::Type, Clone)]
pub struct ContainerRow {
    pub id: String,
    pub name: String,
    pub image: String,
    pub command: String,
    pub state: String,
    pub status: String,
    pub health: Option<String>,
    pub created: f64,
    pub ports: Vec<PortRow>,
    /// `com.docker.compose.project`, when Compose made it.
    pub project: Option<String>,
    pub service: Option<String>,
    pub working_dir: Option<String>,
    pub size: Option<f64>,
    /// On eDock's engine: the name it answers to at `<domain>.edock.localhost`
    /// (its `app.edock.domain` label, `<service>.<project>`, or its name).
    pub domain: Option<String>,
}

/// Every container, running or not.
#[command]
pub async fn containers_list(ctx: Ctx, size: bool) -> Result<Vec<ContainerRow>, String> {
    let docker = docker(&ctx)?;
    let on_edock = ctx.get::<Engine>().context().0 == crate::engine::EDOCK;
    let opts = q::ListContainersOptionsBuilder::default().all(true).size(size).build();
    let list = docker.list_containers(Some(opts)).await.map_err(err)?;
    Ok(list
        .iter()
        .map(|c| {
            let v = json(c);
            let labels = &v["Labels"];
            let label = |k: &str| labels[k].as_str().map(str::to_owned);
            let mut ports: Vec<PortRow> = v["Ports"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|p| PortRow {
                            ip: s(&p["IP"]),
                            private: p["PrivatePort"].as_u64().unwrap_or(0) as u32,
                            public: p["PublicPort"].as_u64().map(|n| n as u32),
                            proto: s(&p["Type"]),
                        })
                        .collect()
                })
                .unwrap_or_default();
            // Docker lists a published port once for IPv4 and once for IPv6.
            ports.sort_by_key(|p| (p.public, p.private));
            ports.dedup_by(|a, b| a.public == b.public && a.private == b.private && a.proto == b.proto);
            let name = v["Names"][0].as_str().unwrap_or("").trim_start_matches('/').to_owned();
            let domain = on_edock.then(|| {
                let chosen = label("app.edock.domain").and_then(|d| d.split(',').next().map(|d| d.trim().to_owned())).filter(|d| !d.is_empty());
                let service = label("com.docker.compose.service").zip(label("com.docker.compose.project")).map(|(s, p)| format!("{s}.{p}"));
                chosen.or(service).unwrap_or_else(|| name.clone())
            });
            let status = s(&v["Status"]);
            let health = ["healthy", "unhealthy", "starting"]
                .into_iter()
                .find(|h| status.contains(&format!("({h})")) || status.contains(&format!("(health: {h})")))
                .map(str::to_owned);
            ContainerRow {
                id: s(&v["Id"]),
                name,
                image: s(&v["Image"]),
                command: s(&v["Command"]),
                state: s(&v["State"]),
                status,
                health,
                created: v["Created"].as_f64().unwrap_or(0.0),
                ports,
                project: label("com.docker.compose.project"),
                service: label("com.docker.compose.service"),
                working_dir: label("com.docker.compose.project.working_dir"),
                size: v["SizeRw"].as_f64(),
                domain,
            }
        })
        .collect())
}

async fn act(docker: &bollard::Docker, id: &str, action: &str) -> Result<(), String> {
    match action {
        "start" => docker.start_container(id, None::<q::StartContainerOptions>).await,
        "stop" => docker.stop_container(id, None::<q::StopContainerOptions>).await,
        "restart" => docker.restart_container(id, None::<q::RestartContainerOptions>).await,
        "pause" => docker.pause_container(id).await,
        "unpause" => docker.unpause_container(id).await,
        "kill" => docker.kill_container(id, None::<q::KillContainerOptions>).await,
        "remove" => {
            let o = q::RemoveContainerOptionsBuilder::default().force(true).v(false).build();
            docker.remove_container(id, Some(o)).await
        }
        other => return Err(format!("Unknown action {other}")),
    }
    .map_err(err)
}

/// start, stop, restart, pause, unpause, kill or remove one container.
#[command]
pub async fn container_action(ctx: Ctx, id: String, action: String) -> Result<(), String> {
    act(&docker(&ctx)?, &id, &action).await
}

/// The same action on every container of a Compose project.
#[command]
pub async fn project_action(ctx: Ctx, project: String, action: String) -> Result<(), String> {
    let docker = docker(&ctx)?;
    let mut filters = HashMap::new();
    filters.insert("label".to_string(), vec![format!("com.docker.compose.project={project}")]);
    let opts = q::ListContainersOptionsBuilder::default().all(true).filters(&filters).build();
    let list = docker.list_containers(Some(opts)).await.map_err(err)?;
    let mut errors = vec![];
    for c in list {
        let id = c.id.unwrap_or_default();
        let running = json(&c.state) == "running";
        // Don't start what runs or stop what's stopped: no errors for those.
        let skip = matches!((action.as_str(), running), ("start", true) | ("stop", false));
        if !skip {
            if let Err(e) = act(&docker, &id, &action).await {
                errors.push(e);
            }
        }
    }
    if errors.is_empty() { Ok(()) } else { Err(errors.join("\n")) }
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct KeyValue {
    pub key: String,
    pub value: String,
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct MountRow {
    pub kind: String,
    pub source: String,
    pub destination: String,
    pub rw: bool,
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct ContainerDetail {
    pub id: String,
    pub name: String,
    /// On eDock's engine: its name at `<domain>.edock.localhost`.
    pub domain: Option<String>,
    pub image: String,
    pub state: String,
    pub started_at: String,
    pub created: String,
    pub cmd: String,
    pub entrypoint: String,
    pub working_dir: String,
    pub restart_policy: String,
    pub tty: bool,
    pub env: Vec<KeyValue>,
    pub labels: Vec<KeyValue>,
    pub mounts: Vec<MountRow>,
    pub networks: Vec<KeyValue>,
    pub ports: Vec<KeyValue>,
    /// The whole `docker inspect`, pretty-printed.
    pub raw: String,
}

fn join(v: &Value) -> String {
    match v {
        Value::Array(a) => a.iter().map(|x| x.as_str().unwrap_or("")).collect::<Vec<_>>().join(" "),
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}

/// `docker inspect` for a container, in parts.
#[command]
pub async fn container_inspect(ctx: Ctx, id: String) -> Result<ContainerDetail, String> {
    let docker = docker(&ctx)?;
    let info = docker.inspect_container(&id, None::<q::InspectContainerOptions>).await.map_err(err)?;
    let v = json(&info);
    let cfg = &v["Config"];
    let mut env: Vec<KeyValue> = cfg["Env"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| e.as_str())
                .map(|e| {
                    let (k, val) = e.split_once('=').unwrap_or((e, ""));
                    KeyValue { key: k.into(), value: val.into() }
                })
                .collect()
        })
        .unwrap_or_default();
    env.sort_by(|a, b| a.key.cmp(&b.key));
    let kv = |m: &Value| -> Vec<KeyValue> {
        let mut out: Vec<KeyValue> = m
            .as_object()
            .map(|o| o.iter().map(|(k, v)| KeyValue { key: k.clone(), value: v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string()) }).collect())
            .unwrap_or_default();
        out.sort_by(|a, b| a.key.cmp(&b.key));
        out
    };
    let mounts = v["Mounts"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|m| MountRow {
                    kind: s(&m["Type"]),
                    source: m["Name"].as_str().filter(|n| !n.is_empty()).map(str::to_owned).unwrap_or_else(|| s(&m["Source"])),
                    destination: s(&m["Destination"]),
                    rw: m["RW"].as_bool().unwrap_or(true),
                })
                .collect()
        })
        .unwrap_or_default();
    let networks = v["NetworkSettings"]["Networks"]
        .as_object()
        .map(|o| o.iter().map(|(k, n)| KeyValue { key: k.clone(), value: s(&n["IPAddress"]) }).collect())
        .unwrap_or_default();
    let ports = v["NetworkSettings"]["Ports"]
        .as_object()
        .map(|o| {
            o.iter()
                .map(|(k, b)| KeyValue {
                    key: k.clone(),
                    value: b
                        .as_array()
                        .map(|a| a.iter().map(|x| format!("{}:{}", s(&x["HostIp"]), s(&x["HostPort"]))).collect::<Vec<_>>().join(", "))
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default();
    let name = s(&v["Name"]).trim_start_matches('/').to_owned();
    let domain = (ctx.get::<Engine>().context().0 == crate::engine::EDOCK).then(|| {
        let label = |k: &str| cfg["Labels"][k].as_str().filter(|v| !v.is_empty()).map(str::to_owned);
        let chosen = label("app.edock.domain").and_then(|d| d.split(',').next().map(|d| d.trim().to_owned())).filter(|d| !d.is_empty());
        let service = label("com.docker.compose.service").zip(label("com.docker.compose.project")).map(|(s, p)| format!("{s}.{p}"));
        chosen.or(service).unwrap_or_else(|| name.clone())
    });
    Ok(ContainerDetail {
        domain,
        id: s(&v["Id"]),
        name,
        image: s(&cfg["Image"]),
        state: s(&v["State"]["Status"]),
        started_at: s(&v["State"]["StartedAt"]),
        created: s(&v["Created"]),
        cmd: join(&cfg["Cmd"]),
        entrypoint: join(&cfg["Entrypoint"]),
        working_dir: s(&cfg["WorkingDir"]),
        restart_policy: s(&v["HostConfig"]["RestartPolicy"]["Name"]),
        tty: cfg["Tty"].as_bool().unwrap_or(false),
        env,
        labels: kv(&cfg["Labels"]),
        mounts,
        networks,
        ports,
        raw: serde_json::to_string_pretty(&v).unwrap_or_default(),
    })
}

/// Remove stopped containers.
#[command]
pub async fn containers_prune(ctx: Ctx) -> Result<f64, String> {
    let r = docker(&ctx)?.prune_containers(None::<q::PruneContainersOptions>).await.map_err(err)?;
    Ok(r.space_reclaimed.unwrap_or(0) as f64)
}

// ── Images ─────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, specta::Type)]
pub struct ImageRow {
    pub id: String,
    pub repository: String,
    pub tag: String,
    pub size: f64,
    pub created: f64,
    /// Containers using it (-1: unknown).
    pub containers: i32,
}

/// Every image, one row per tag (untagged ones as `<none>`).
#[command]
pub async fn images_list(ctx: Ctx) -> Result<Vec<ImageRow>, String> {
    let docker = docker(&ctx)?;
    let opts = q::ListImagesOptionsBuilder::default().all(false).build();
    let images = docker.list_images(Some(opts)).await.map_err(err)?;
    // Which images containers use, by id (the list's own count is often -1).
    let used: Vec<String> = docker
        .list_containers(Some(q::ListContainersOptionsBuilder::default().all(true).build()))
        .await
        .map(|cs| cs.into_iter().filter_map(|c| c.image_id).collect())
        .unwrap_or_default();
    let mut rows = vec![];
    for img in images {
        let v = json(&img);
        let id = s(&v["Id"]);
        let containers = used.iter().filter(|u| **u == id).count() as i32;
        let tags: Vec<String> = v["RepoTags"]
            .as_array()
            .map(|a| a.iter().filter_map(|t| t.as_str()).filter(|t| *t != "<none>:<none>").map(str::to_owned).collect())
            .unwrap_or_default();
        let base = |repository: String, tag: String| ImageRow {
            id: id.clone(),
            repository,
            tag,
            size: v["Size"].as_f64().unwrap_or(0.0),
            created: v["Created"].as_f64().unwrap_or(0.0),
            containers,
        };
        if tags.is_empty() {
            let repo = v["RepoDigests"][0].as_str().and_then(|d| d.split('@').next()).unwrap_or("<none>").to_owned();
            rows.push(base(repo, "<none>".into()));
        }
        for t in tags {
            let (repo, tag) = t.rsplit_once(':').filter(|(_, tag)| !tag.contains('/')).map(|(r, t)| (r.to_owned(), t.to_owned())).unwrap_or((t.clone(), "latest".into()));
            rows.push(base(repo, tag));
        }
    }
    rows.sort_by(|a, b| a.repository.cmp(&b.repository).then(a.tag.cmp(&b.tag)));
    Ok(rows)
}

/// Remove an image (by `repo:tag`, or id).
#[command]
pub async fn image_remove(ctx: Ctx, reference: String, force: bool) -> Result<(), String> {
    let o = q::RemoveImageOptionsBuilder::default().force(force).build();
    docker(&ctx)?.remove_image(&reference, Some(o), None).await.map_err(err)?;
    Ok(())
}

/// Remove dangling images (or every unused one with `all`).
#[command]
pub async fn images_prune(ctx: Ctx, all: bool) -> Result<f64, String> {
    let mut filters = HashMap::new();
    filters.insert("dangling".to_string(), vec![(!all).to_string()]);
    let o = q::PruneImagesOptionsBuilder::default().filters(&filters).build();
    let r = docker(&ctx)?.prune_images(Some(o)).await.map_err(err)?;
    Ok(r.space_reclaimed.unwrap_or(0) as f64)
}

#[derive(Serialize, Deserialize, specta::Type, Clone)]
pub struct PullProgress {
    pub reference: String,
    pub status: String,
    pub done: bool,
    pub error: Option<String>,
}

/// Pull an image. Progress goes out on the `pull` channel; the promise
/// settles when the pull is done.
#[command]
pub async fn image_pull(ctx: Ctx, reference: String) -> Result<(), String> {
    use futures_util::StreamExt;
    let docker = docker(&ctx)?;
    let bus = ctx.get::<elyra::EventBus>();
    let (image, tag) = match reference.rsplit_once(':') {
        Some((i, t)) if !t.contains('/') => (i.to_owned(), t.to_owned()),
        _ => (reference.clone(), "latest".to_owned()),
    };
    let o = q::CreateImageOptionsBuilder::default().from_image(&image).tag(&tag).build();
    let mut stream = docker.create_image(Some(o), None, None);
    let mut last = std::time::Instant::now() - std::time::Duration::from_secs(1);
    while let Some(item) = stream.next().await {
        match item {
            Ok(info) => {
                if last.elapsed().as_millis() > 150 {
                    last = std::time::Instant::now();
                    let v = json(&info);
                    let status = format!("{} {}", s(&v["status"]), s(&v["progress"])).trim().to_owned();
                    let _ = bus.emit("pull", &PullProgress { reference: reference.clone(), status, done: false, error: None });
                }
            }
            Err(e) => {
                let e = err(e);
                let _ = bus.emit("pull", &PullProgress { reference: reference.clone(), status: String::new(), done: true, error: Some(e.clone()) });
                return Err(e);
            }
        }
    }
    let _ = bus.emit("pull", &PullProgress { reference, status: "Pulled".into(), done: true, error: None });
    Ok(())
}

// ── Volumes ────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, specta::Type)]
pub struct VolumeRow {
    pub name: String,
    pub driver: String,
    pub mountpoint: String,
    pub created: String,
    pub project: Option<String>,
    /// Containers mounting it.
    pub used_by: Vec<String>,
    pub size: Option<f64>,
}

/// Every volume, and the containers that mount it.
#[command]
pub async fn volumes_list(ctx: Ctx) -> Result<Vec<VolumeRow>, String> {
    let docker = docker(&ctx)?;
    let vols = docker.list_volumes(None::<q::ListVolumesOptions>).await.map_err(err)?;
    let containers = docker
        .list_containers(Some(q::ListContainersOptionsBuilder::default().all(true).build()))
        .await
        .unwrap_or_default();
    // Sizes come from `system df`, which can be slow; best effort.
    let sizes: HashMap<String, f64> = match tokio::time::timeout(std::time::Duration::from_secs(3), docker.df(None::<q::DataUsageOptions>)).await {
        Ok(Ok(df)) => json(&df)["Volumes"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| Some((s(&v["Name"]), v["UsageData"]["Size"].as_f64().filter(|n| *n >= 0.0)?))).collect())
            .unwrap_or_default(),
        _ => HashMap::new(),
    };
    let mut rows: Vec<VolumeRow> = vols
        .volumes
        .unwrap_or_default()
        .iter()
        .map(|vol| {
            let v = json(vol);
            let name = s(&v["Name"]);
            let used_by = containers
                .iter()
                .filter(|c| c.mounts.as_ref().is_some_and(|ms| ms.iter().any(|m| m.name.as_deref() == Some(&name))))
                .map(|c| c.names.as_ref().and_then(|n| n.first()).map(|n| n.trim_start_matches('/').to_owned()).unwrap_or_default())
                .collect();
            VolumeRow {
                size: sizes.get(&name).copied(),
                driver: s(&v["Driver"]),
                mountpoint: s(&v["Mountpoint"]),
                created: s(&v["CreatedAt"]),
                project: v["Labels"]["com.docker.compose.project"].as_str().map(str::to_owned),
                used_by,
                name,
            }
        })
        .collect();
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(rows)
}

#[command]
pub async fn volume_create(ctx: Ctx, name: String) -> Result<(), String> {
    let o = bollard::models::VolumeCreateRequest { name: Some(name), ..Default::default() };
    docker(&ctx)?.create_volume(o).await.map_err(err)?;
    Ok(())
}

#[command]
pub async fn volume_remove(ctx: Ctx, name: String) -> Result<(), String> {
    docker(&ctx)?.remove_volume(&name, None::<q::RemoveVolumeOptions>).await.map_err(err)
}

/// Remove volumes no container uses (named ones too).
#[command]
pub async fn volumes_prune(ctx: Ctx) -> Result<f64, String> {
    let mut filters = HashMap::new();
    filters.insert("all".to_string(), vec!["true".to_string()]);
    let o = q::PruneVolumesOptionsBuilder::default().filters(&filters).build();
    let r = docker(&ctx)?.prune_volumes(Some(o)).await.map_err(err)?;
    Ok(r.space_reclaimed.unwrap_or(0) as f64)
}

// ── Networks ───────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, specta::Type)]
pub struct NetworkRow {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub scope: String,
    pub subnet: String,
    pub gateway: String,
    pub internal: bool,
    pub project: Option<String>,
    /// Docker's own: bridge, host, none.
    pub builtin: bool,
    pub containers: Vec<String>,
}

#[command]
pub async fn networks_list(ctx: Ctx) -> Result<Vec<NetworkRow>, String> {
    let docker = docker(&ctx)?;
    let nets = docker.list_networks(None::<q::ListNetworksOptions>).await.map_err(err)?;
    let mut rows = vec![];
    for n in nets {
        let v = json(&n);
        let id = s(&v["Id"]);
        // The list leaves out the containers; inspect has them.
        let containers = docker
            .inspect_network(&id, None::<q::InspectNetworkOptions>)
            .await
            .ok()
            .map(|i| json(&i)["Containers"].as_object().map(|o| o.values().map(|c| s(&c["Name"])).collect()).unwrap_or_default())
            .unwrap_or_default();
        let name = s(&v["Name"]);
        rows.push(NetworkRow {
            builtin: matches!(name.as_str(), "bridge" | "host" | "none"),
            driver: s(&v["Driver"]),
            scope: s(&v["Scope"]),
            subnet: s(&v["IPAM"]["Config"][0]["Subnet"]),
            gateway: s(&v["IPAM"]["Config"][0]["Gateway"]),
            internal: v["Internal"].as_bool().unwrap_or(false),
            project: v["Labels"]["com.docker.compose.project"].as_str().map(str::to_owned),
            containers,
            name,
            id,
        });
    }
    rows.sort_by(|a, b| b.builtin.cmp(&a.builtin).then(a.name.cmp(&b.name)));
    Ok(rows)
}

#[command]
pub async fn network_create(ctx: Ctx, name: String) -> Result<(), String> {
    let o = bollard::models::NetworkCreateRequest { name, driver: Some("bridge".into()), ..Default::default() };
    docker(&ctx)?.create_network(o).await.map_err(err)?;
    Ok(())
}

#[command]
pub async fn network_remove(ctx: Ctx, id: String) -> Result<(), String> {
    docker(&ctx)?.remove_network(&id).await.map_err(err)
}

#[command]
pub async fn networks_prune(ctx: Ctx) -> Result<u32, String> {
    let r = docker(&ctx)?.prune_networks(None::<q::PruneNetworksOptions>).await.map_err(err)?;
    Ok(r.networks_deleted.map(|d| d.len() as u32).unwrap_or(0))
}

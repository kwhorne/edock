//! The commands against a real engine. They need one, so they're ignored
//! by default: `cargo test -- --ignored`, against docker's current context,
//! or `EDOCK_TEST_HOST=unix:///…/docker.sock`.

use std::time::{Duration, Instant};

use elyra::testing::TestApp;

use crate::engine::{list_contexts, Engine};
use crate::resources::ContainerRow;
use crate::streams::{LogBatch, TermChunk};

/// The tests share one engine, and Stop turns it off: they hold this lock
/// to read, and the stop test holds it alone.
static ENGINE: tokio::sync::RwLock<()> = tokio::sync::RwLock::const_new(());

fn app() -> TestApp {
    let host = std::env::var("EDOCK_TEST_HOST").ok().unwrap_or_else(|| {
        let cs = list_contexts();
        cs.iter().find(|c| c.docker_current && c.available).or_else(|| cs.iter().find(|c| c.available)).expect("no docker engine to test against").host.clone()
    });
    // On eDock's socket the app is on the `edock` context, as Kubernetes wants.
    let context = if host.contains("/.edock/") { crate::engine::EDOCK } else { "test" };
    TestApp::new(crate::core(Engine::on(context, &host)))
}

/// `image` in the engine: pulled only when it isn't there, so a run of the
/// suite doesn't spend Docker Hub's pull quota on images it has.
async fn ensure_image(app: &TestApp, image: &str) {
    let docker = app.ctx().get::<Engine>().docker().unwrap();
    if docker.inspect_image(image).await.is_err() {
        let _: () = app.invoke("image_pull", (image,)).await.unwrap();
    }
}

/// A container of our own, removed when the test ends.
struct Scratch {
    docker: bollard::Docker,
    id: String,
}

impl Scratch {
    async fn new(app: &TestApp, name: &str) -> Scratch {
        use bollard::query_parameters as q;
        let docker = app.ctx().get::<Engine>().docker().unwrap();
        let _ = docker.remove_container(name, Some(q::RemoveContainerOptionsBuilder::default().force(true).build())).await;
        ensure_image(app, "alpine:latest").await;
        let body = bollard::models::ContainerCreateBody {
            image: Some("alpine:latest".into()),
            cmd: Some(vec!["sh".into(), "-c".into(), "i=0; while true; do echo tick $i; i=$((i+1)); sleep 0.2; done".into()]),
            labels: Some([("com.docker.compose.project".to_string(), "edock-test".to_string())].into()),
            ..Default::default()
        };
        let id = docker.create_container(Some(q::CreateContainerOptionsBuilder::default().name(name).build()), body).await.unwrap().id;
        docker.start_container(&id, None::<q::StartContainerOptions>).await.unwrap();
        Scratch { docker, id }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let (docker, id) = (self.docker.clone(), self.id.clone());
        let _ = std::thread::spawn(move || {
            tokio::runtime::Runtime::new().unwrap().block_on(async move {
                let o = bollard::query_parameters::RemoveContainerOptionsBuilder::default().force(true).build();
                let _ = docker.remove_container(&id, Some(o)).await;
            })
        })
        .join();
    }
}

async fn wait_for<T: serde::de::DeserializeOwned>(app: &TestApp, channel: &str, mut done: impl FnMut(&[T]) -> bool) -> Vec<T> {
    // events_on drains what's pending: keep what came before.
    let start = Instant::now();
    let mut got: Vec<T> = vec![];
    loop {
        got.extend(app.events_on::<T>(channel).await);
        if done(&got) {
            return got;
        }
        assert!(start.elapsed() < Duration::from_secs(20), "nothing on {channel} in time");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a docker engine"]
async fn lists_actions_logs_and_a_terminal() {
    let _shared = ENGINE.read().await;
    let app = app();
    app.listen();
    let c = Scratch::new(&app, "edock-desktop-test").await;

    // The list has it, under its Compose project.
    let rows: Vec<ContainerRow> = app.invoke("containers_list", (false,)).await.unwrap();
    let row = rows.iter().find(|r| r.id == c.id).expect("listed");
    assert_eq!(row.state, "running");
    assert_eq!(row.project.as_deref(), Some("edock-test"));

    // Its log streams.
    let logs: u32 = app.invoke("logs_follow", (c.id.clone(), 10u32, false)).await.unwrap();
    let batches: Vec<LogBatch> = wait_for(&app, &format!("logs:{logs}"), |b: &[LogBatch]| b.iter().flat_map(|b| &b.lines).filter(|l| l.text.starts_with("tick")).count() >= 3).await;
    assert!(batches.iter().flat_map(|b| &b.lines).all(|l| l.stream == "out"));
    let _: () = app.invoke("stream_stop", (logs,)).await.unwrap();

    // A terminal: what's typed runs, and its output comes back.
    let term: u32 = app.invoke("exec_start", (c.id.clone(), 80u32, 24u32, None::<String>)).await.unwrap();
    let _: () = app.invoke("exec_resize", (term, 100u32, 30u32)).await.unwrap();
    let _: () = app.invoke("exec_input", (term, "echo edock-$((40+2))\r".to_string())).await.unwrap();
    wait_for(&app, &format!("exec:{term}"), |c: &[TermChunk]| c.iter().map(|c| c.data.as_str()).collect::<String>().contains("edock-42")).await;
    let _: () = app.invoke("exec_input", (term, "stty size\r".to_string())).await.unwrap();
    wait_for(&app, &format!("exec:{term}"), |c: &[TermChunk]| c.iter().map(|c| c.data.as_str()).collect::<String>().contains("30 100")).await;
    let _: () = app.invoke("exec_input", (term, "exit\r".to_string())).await.unwrap();
    wait_for(&app, &format!("exec:{term}"), |c: &[TermChunk]| c.iter().any(|c| c.ended)).await;

    // Stop, then the Compose project's start.
    let _: () = app.invoke("container_action", (c.id.clone(), "stop".to_string())).await.unwrap();
    let detail: crate::resources::ContainerDetail = app.invoke("container_inspect", (c.id.clone(),)).await.unwrap();
    assert_eq!(detail.state, "exited");
    let _: () = app.invoke("project_action", ("edock-test".to_string(), "start".to_string())).await.unwrap();
    let detail: crate::resources::ContainerDetail = app.invoke("container_inspect", (c.id.clone(),)).await.unwrap();
    assert_eq!(detail.state, "running");

    // An unknown action is refused, not sent.
    assert!(app.invoke_err("container_action", (c.id.clone(), "explode".to_string())).await.contains("Unknown action"));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a docker engine"]
async fn images_volumes_and_networks_list() {
    let _shared = ENGINE.read().await;
    let app = app();
    let _: Vec<crate::resources::ImageRow> = app.invoke("images_list", ()).await.unwrap();
    let _: () = app.invoke("volume_create", ("edock-desktop-test-vol".to_string(),)).await.unwrap();
    let vols: Vec<crate::resources::VolumeRow> = app.invoke("volumes_list", ()).await.unwrap();
    assert!(vols.iter().any(|v| v.name == "edock-desktop-test-vol"));
    let _: () = app.invoke("volume_remove", ("edock-desktop-test-vol".to_string(),)).await.unwrap();
    let nets: Vec<crate::resources::NetworkRow> = app.invoke("networks_list", ()).await.unwrap();
    assert!(nets.iter().any(|n| n.name == "bridge" && n.builtin));
    let status: crate::resources::EngineStatus = app.invoke("engine_status", ()).await.unwrap();
    assert!(status.connected, "{:?}", status.error);
}

/// Restart engine: the containers that ran run again after it.
/// `EDOCK_TEST_RESTART=1`, against eDock's engine; it restarts the engine.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "restarts eDock's engine"]
async fn restarting_the_engine_starts_its_containers_again() {
    use bollard::query_parameters as q;
    if std::env::var("EDOCK_TEST_RESTART").is_err() {
        eprintln!("skipped: set EDOCK_TEST_RESTART=1");
        return;
    }
    let _alone = ENGINE.write().await;
    let app = app();
    let docker = app.ctx().get::<Engine>().docker().unwrap();
    ensure_image(&app, "alpine:latest").await;
    let name = format!("edock-test-restart-{}", std::process::id());
    let body = bollard::models::ContainerCreateBody {
        image: Some("alpine:latest".into()),
        cmd: Some(vec!["sleep".into(), "300".into()]),
        labels: Some([("app.edock.test".to_string(), "1".to_string())].into()),
        ..Default::default()
    };
    docker.create_container(Some(q::CreateContainerOptionsBuilder::default().name(&name).build()), body).await.unwrap();
    docker.start_container(&name, None::<q::StartContainerOptions>).await.unwrap();
    let r: Result<(), _> = app.invoke("daemon_restart", ()).await;
    let docker = app.ctx().get::<Engine>().docker().unwrap();
    let running = docker.inspect_container(&name, None::<q::InspectContainerOptions>).await.ok().and_then(|i| i.state).and_then(|s| s.running);
    let _ = docker.remove_container(&name, Some(q::RemoveContainerOptionsBuilder::default().force(true).build())).await;
    r.unwrap();
    assert_eq!(running, Some(true), "it runs again");
}

/// Kubernetes on eDock's engine, from nothing to a deployment's pod and
/// back: `EDOCK_TEST_HOST=unix://$HOME/.edock/run/docker.sock
/// EDOCK_TEST_K8S=1 cargo test kubernetes -- --ignored`. It deletes the
/// cluster at the end, so don't run it against one you keep.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs eDock's engine, and makes and deletes a cluster"]
async fn kubernetes() {
    use crate::daemon::Progress as K8sProgress;
    use crate::k8s::{K8sItems, K8sStatus};
    if std::env::var("EDOCK_TEST_K8S").is_err() {
        eprintln!("skipped: set EDOCK_TEST_K8S=1");
        return;
    }
    let _shared = ENGINE.read().await;
    let app = app();
    app.listen();
    let st: K8sStatus = app.invoke("k8s_status", ()).await.unwrap();
    assert!(st.on_edock, "run against eDock's engine");
    assert_eq!(st.state, "absent", "a cluster is there already; this test would delete it");

    // Start: the engine's own `edock k8s start`, its lines as progress.
    let _: () = app.invoke("k8s_cluster", ("start".to_string(), false)).await.unwrap();
    assert!(app.invoke_err("k8s_cluster", ("stop".to_string(), false)).await.contains("busy"));
    let done = wait_long::<K8sProgress>(&app, "k8s:progress", 240, |p| p.iter().any(|p| p.done)).await;
    let last = done.iter().find(|p| p.done).unwrap();
    assert!(last.error.is_none(), "{:?}", last.error);
    assert!(done.iter().any(|p| p.line.contains("ready")), "no progress lines");
    let st: K8sStatus = app.invoke("k8s_status", ()).await.unwrap();
    assert!(st.ready && st.state == "running" && st.port.is_some());

    // A deployment, as `kubectl apply` would make it.
    let docker = app.ctx().get::<Engine>().docker().unwrap();
    let kubectl = |args: &[&str]| {
        let docker = docker.clone();
        let mut cmd = vec!["kubectl".to_string()];
        cmd.extend(args.iter().map(|a| a.to_string()));
        async move {
            use bollard::exec::{CreateExecOptions, StartExecResults};
            let e = docker.create_exec("edock-k8s", CreateExecOptions { attach_stdout: Some(true), attach_stderr: Some(true), cmd: Some(cmd), ..Default::default() }).await.unwrap();
            if let StartExecResults::Attached { mut output, .. } = docker.start_exec(&e.id, None).await.unwrap() {
                use futures_util::StreamExt;
                while output.next().await.is_some() {}
            }
        }
    };
    kubectl(&["create", "deployment", "web", "--image=nginx:alpine"]).await;
    kubectl(&["expose", "deployment", "web", "--port=8080", "--target-port=80", "--type=ClusterIP"]).await;

    let start = Instant::now();
    let pod = loop {
        let items: K8sItems = app.invoke("k8s_items", ()).await.unwrap();
        if let Some(p) = items.pods.iter().find(|p| p.namespace == "default" && p.name.starts_with("web-") && p.status == "Running") {
            assert_eq!(p.owner.as_deref(), Some("Deployment/web"));
            assert!(items.services.iter().any(|s| s.name == "web" && s.ports[0].port == 8080 && s.ports[0].target == "80"));
            assert!(items.pods.iter().any(|p| p.namespace == "kube-system"), "the system pods are listed too");
            break p.clone();
        }
        assert!(start.elapsed() < Duration::from_secs(180), "web never ran: {:?}", items.pods.iter().map(|p| (&p.name, &p.status)).collect::<Vec<_>>());
        tokio::time::sleep(Duration::from_secs(2)).await;
    };

    // Its log, and a shell in it.
    let logs: u32 = app.invoke("k8s_logs_follow", ("default".to_string(), pod.name.clone(), "nginx".to_string(), 50u32, false)).await.unwrap();
    wait_for(&app, &format!("logs:{logs}"), |b: &[LogBatch]| b.iter().flat_map(|b| &b.lines).any(|l| l.text.contains("nginx"))).await;
    let _: () = app.invoke("stream_stop", (logs,)).await.unwrap();
    let term: u32 = app.invoke("k8s_exec_start", ("default".to_string(), pod.name.clone(), "nginx".to_string(), 80u32, 24u32)).await.unwrap();
    let _: () = app.invoke("exec_input", (term, "echo pod-$((6*7)) && hostname\r".to_string())).await.unwrap();
    wait_for(&app, &format!("exec:{term}"), |c: &[TermChunk]| {
        let all: String = c.iter().map(|c| c.data.as_str()).collect();
        all.contains("pod-42") && all.contains(&pod.name)
    })
    .await;
    let _: () = app.invoke("exec_input", (term, "exit\r".to_string())).await.unwrap();

    // Names that aren't Kubernetes names never reach kubectl.
    assert!(app.invoke_err("k8s_pod_delete", ("default".to_string(), "--all".to_string())).await.contains("isn't a Kubernetes name"));

    // Forward the ClusterIP service to the Mac: it answers there.
    let svc = loop {
        let items: K8sItems = app.invoke("k8s_items", ()).await.unwrap();
        if let Some(s) = items.services.iter().find(|s| s.name == "web" && !s.cluster_ip.is_empty()) { break s.clone(); }
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    let fw: crate::k8s::Forward = app.invoke("k8s_forward", (svc.cluster_ip.clone(), 8080u32)).await.unwrap();
    // The same address again: the same port, not a second forward.
    let again: crate::k8s::Forward = app.invoke("k8s_forward", (svc.cluster_ip.clone(), 8080u32)).await.unwrap();
    assert_eq!(again.local, fw.local);
    let curl = |port: u32| std::process::Command::new("curl").args(["-s", "-o", "/dev/null", "-w", "%{http_code}", "--max-time", "5", &format!("http://127.0.0.1:{port}/")]).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    assert_eq!(curl(fw.local), "200", "the forward didn't answer");
    let listed: Vec<crate::k8s::Forward> = app.invoke("k8s_forwards", ()).await.unwrap();
    assert_eq!(listed.iter().filter(|f| f.ip == svc.cluster_ip && f.port == 8080).map(|f| f.local).collect::<Vec<_>>(), vec![fw.local]);
    assert!(app.invoke_err("k8s_forward", ("8.8.8.8".to_string(), 53u32)).await.contains("Kubernetes"));
    let _: () = app.invoke("k8s_unforward", (fw.local,)).await.unwrap();
    let listed: Vec<crate::k8s::Forward> = app.invoke("k8s_forwards", ()).await.unwrap();
    assert!(listed.iter().all(|f| f.local != fw.local));
    assert_ne!(curl(fw.local), "200");

    // Restart the deployment, then delete the pod: a new one takes its place.
    let _: () = app.invoke("k8s_rollout_restart", ("default".to_string(), "web".to_string())).await.unwrap();
    let _: () = app.invoke("k8s_pod_delete", ("default".to_string(), pod.name.clone())).await.unwrap();
    let start = Instant::now();
    loop {
        let items: K8sItems = app.invoke("k8s_items", ()).await.unwrap();
        if items.pods.iter().any(|p| p.name.starts_with("web-") && p.name != pod.name && p.status == "Running") {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(120), "no new web pod");
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    // An image only eDock has: the pod can't pull it and says so, with
    // what Kubernetes said; loaded from eDock, a new pod runs it.
    ensure_image(&app, "alpine:latest").await;
    let local = format!("edock-test-local:{}", std::process::id());
    let (repo, tag) = local.split_once(':').unwrap();
    docker.tag_image("alpine:latest", Some(bollard::query_parameters::TagImageOptionsBuilder::default().repo(repo).tag(tag).build())).await.unwrap();
    kubectl(&["create", "deployment", "local", &format!("--image={local}"), "--", "sleep", "600"]).await;
    let start = Instant::now();
    let stuck = loop {
        let items: K8sItems = app.invoke("k8s_items", ()).await.unwrap();
        if let Some(p) = items.pods.iter().find(|p| p.name.starts_with("local-") && p.pull_failed.is_some()) {
            break p.clone();
        }
        assert!(start.elapsed() < Duration::from_secs(120), "the pod never said it can't pull");
        tokio::time::sleep(Duration::from_secs(2)).await;
    };
    assert_eq!(stuck.pull_failed.as_deref(), Some(local.as_str()));
    assert!(stuck.why.as_deref().unwrap_or("").contains("can't be pulled"), "{:?}", stuck.why);
    let events: Vec<crate::k8s::K8sEvent> = app.invoke("k8s_pod_events", ("default".to_string(), stuck.name.clone())).await.unwrap();
    assert!(events.iter().any(|e| e.reason == "Scheduled") && events.iter().any(|e| e.kind == "Warning"), "{:?}", events.iter().map(|e| &e.reason).collect::<Vec<_>>());
    // Needs the engine's `edock k8s load`: EDOCK_ENGINE=…/engine/target/release/edock.
    let said: String = app.invoke("k8s_load", (local.clone(),)).await.unwrap();
    assert!(said.contains("is in Kubernetes"), "{said}");
    assert!(app.invoke_err("k8s_load", ("edock-test-not-here:1".to_string(),)).await.contains("isn't on eDock"));
    let _: () = app.invoke("k8s_pod_delete", ("default".to_string(), stuck.name.clone())).await.unwrap();
    let start = Instant::now();
    loop {
        let items: K8sItems = app.invoke("k8s_items", ()).await.unwrap();
        if items.pods.iter().any(|p| p.name.starts_with("local-") && p.name != stuck.name && p.status == "Running" && p.why.is_none()) {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(120), "the loaded image never ran");
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    let _ = docker.remove_image(&local, None::<bollard::query_parameters::RemoveImageOptions>, None).await;

    // Delete the cluster, with its state.
    let _: () = app.invoke("k8s_cluster", ("delete".to_string(), false)).await.unwrap();
    let done = wait_long::<K8sProgress>(&app, "k8s:progress", 120, |p| p.iter().any(|p| p.done)).await;
    assert!(done.iter().find(|p| p.done).unwrap().error.is_none());
    let st: K8sStatus = app.invoke("k8s_status", ()).await.unwrap();
    assert_eq!(st.state, "absent");
}

async fn wait_long<T: serde::de::DeserializeOwned>(app: &TestApp, channel: &str, secs: u64, mut done: impl FnMut(&[T]) -> bool) -> Vec<T> {
    let start = Instant::now();
    let mut got: Vec<T> = vec![];
    loop {
        got.extend(app.events_on::<T>(channel).await);
        if done(&got) {
            return got;
        }
        assert!(start.elapsed() < Duration::from_secs(secs), "nothing done on {channel} in {secs} s");
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// A running container with a restart policy (`unless-stopped`) or none,
/// labelled as the tests' own.
async fn policy_container(docker: &bollard::Docker, name: &str, policy: Option<&str>) {
    use bollard::query_parameters as q;
    let body = bollard::models::ContainerCreateBody {
        image: Some("alpine:latest".into()),
        cmd: Some(vec!["sleep".into(), "600".into()]),
        labels: Some([("app.edock.test".to_string(), "1".to_string())].into()),
        host_config: Some(bollard::models::HostConfig {
            restart_policy: policy.map(|p| bollard::models::RestartPolicy { name: Some(p.parse().unwrap()), maximum_retry_count: None }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let _ = docker.remove_container(name, Some(q::RemoveContainerOptionsBuilder::default().force(true).build())).await;
    docker.create_container(Some(q::CreateContainerOptionsBuilder::default().name(name).build()), body).await.unwrap();
    docker.start_container(name, None::<q::StartContainerOptions>).await.unwrap();
}

/// Whether `name` runs, within a few seconds (held containers start on
/// their own thread after Start).
async fn runs_soon(docker: &bollard::Docker, name: &str) -> bool {
    for _ in 0..50 {
        if docker.inspect_container(name, None::<bollard::query_parameters::InspectContainerOptions>).await.ok().and_then(|i| i.state).and_then(|s| s.running) == Some(true) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    false
}

/// Stop and Start on an engine the app didn't start (as after a restart
/// of the app, or with the LaunchAgent): `EDOCK_TEST_HOST=unix://$HOME/.edock/run/docker.sock`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs eDock's engine"]
async fn stop_and_start_an_engine_the_app_did_not_start() {
    use crate::daemon::{DaemonInfo, EngineState};
    use crate::resources::EngineStatus;
    if !std::env::var("EDOCK_TEST_HOST").is_ok_and(|h| h.contains("/.edock/")) {
        eprintln!("skipped: needs EDOCK_TEST_HOST on eDock's socket");
        return;
    }
    let _alone = ENGINE.write().await;
    let app = app();
    let before: DaemonInfo = app.invoke("daemon_info", ()).await.unwrap();
    assert!(matches!(before.state, EngineState::Running | EngineState::Asleep), "start eDock's engine first: {:?}", before.state);

    // What runs with a restart policy comes back with Start; what has none doesn't.
    ensure_image(&app, "alpine:latest").await;
    let d0 = app.ctx().get::<Engine>().docker().unwrap();
    let (kept, plain) = (format!("edock-test-policy-{}", std::process::id()), format!("edock-test-nopolicy-{}", std::process::id()));
    policy_container(&d0, &kept, Some("unless-stopped")).await;
    policy_container(&d0, &plain, None).await;

    let _: () = app.invoke("daemon_stop", ()).await.unwrap();
    let info: DaemonInfo = app.invoke("daemon_info", ()).await.unwrap();
    assert_eq!(info.state, EngineState::Stopped);
    let st: EngineStatus = app.invoke("engine_status", ()).await.unwrap();
    assert_eq!(st.state, "stopped");
    // Nothing runs while it's stopped.
    let docker = app.ctx().get::<Engine>().docker().unwrap();
    let body = bollard::models::ContainerCreateBody { image: Some("alpine:latest".into()), cmd: Some(vec!["true".into()]), ..Default::default() };
    let made = docker.create_container(None::<bollard::query_parameters::CreateContainerOptions>, body.clone()).await;
    let ran = match made {
        Ok(c) => {
            let r = docker.start_container(&c.id, None::<bollard::query_parameters::StartContainerOptions>).await;
            let _ = docker.remove_container(&c.id, Some(bollard::query_parameters::RemoveContainerOptionsBuilder::default().force(true).build())).await;
            r.is_ok()
        }
        Err(_) => false,
    };
    assert!(!ran, "a container started while eDock was stopped");

    // Start: on again, with the VM up.
    let _: () = app.invoke("daemon_start", ()).await.unwrap();
    let info: DaemonInfo = app.invoke("daemon_info", ()).await.unwrap();
    assert_eq!(info.state, EngineState::Running);
    let d1 = app.ctx().get::<Engine>().docker().unwrap();
    let came_back = runs_soon(&d1, &kept).await;
    let plain_runs = d1.inspect_container(&plain, None::<bollard::query_parameters::InspectContainerOptions>).await.ok().and_then(|i| i.state).and_then(|s| s.running);
    for n in [&kept, &plain] {
        let _ = d1.remove_container(n, Some(bollard::query_parameters::RemoveContainerOptionsBuilder::default().force(true).build())).await;
    }
    assert!(came_back, "unless-stopped came back with Start");
    assert_eq!(plain_runs, Some(false), "no policy: it stays stopped");
    let c = Scratch::new(&app, "edock-desktop-test-restart").await;
    let rows: Vec<ContainerRow> = app.invoke("containers_list", (false,)).await.unwrap();
    assert_eq!(rows.iter().find(|r| r.id == c.id).map(|r| r.state.as_str()), Some("running"));
    drop(c);

    // Restart: a new engine process (as after an update), running again.
    let before = engine_pid();
    let _: () = app.invoke("daemon_restart", ()).await.unwrap();
    let after = engine_pid();
    assert!(before.is_some() && after.is_some() && before != after, "{before:?} → {after:?}");
    // On, not stopped: its VM boots on the first request that needs it.
    let info: DaemonInfo = app.invoke("daemon_info", ()).await.unwrap();
    assert!(matches!(info.state, EngineState::Running | EngineState::Asleep), "{:?}", info.state);
    let st: EngineStatus = app.invoke("engine_status", ()).await.unwrap();
    assert!(st.connected && !st.version.is_empty());
    let c = Scratch::new(&app, "edock-desktop-test-restart").await;
    let rows: Vec<ContainerRow> = app.invoke("containers_list", (false,)).await.unwrap();
    assert_eq!(rows.iter().find(|r| r.id == c.id).map(|r| r.state.as_str()), Some("running"));
    drop(c);

    // Again, now that this app started the engine: its child must be
    // reaped when it exits, or Restart waits 30 s on a zombie.
    let before = engine_pid();
    let t0 = std::time::Instant::now();
    let _: () = app.invoke("daemon_restart", ()).await.unwrap();
    assert!(t0.elapsed() < std::time::Duration::from_secs(15), "restart took {:?}", t0.elapsed());
    assert_ne!(before, engine_pid());
}

/// The engine's process, as `lsof` sees the socket's listener.
fn engine_pid() -> Option<String> {
    let sock = format!("{}/.edock/run/docker.sock", std::env::var("HOME").unwrap());
    let out = std::process::Command::new("/usr/sbin/lsof").args(["-t", "-a", "-c", "edock", &sock]).output().ok()?;
    String::from_utf8_lossy(&out.stdout).lines().next().map(str::to_owned)
}

/// Ports and environment on eDock's engine, on a running container:
/// `EDOCK_TEST_HOST=unix://$HOME/.edock/run/docker.sock`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs eDock's engine"]
async fn ports_domains_and_env_on_a_running_container() {
    use crate::edock::{ContainerPorts, DomainsInfo, PortBinding};
    use bollard::query_parameters as q;
    if !std::env::var("EDOCK_TEST_HOST").is_ok_and(|h| h.contains("/.edock/")) {
        eprintln!("skipped: needs EDOCK_TEST_HOST on eDock's socket");
        return;
    }
    let _shared = ENGINE.read().await;
    let app = app();
    let docker = app.ctx().get::<Engine>().docker().unwrap();
    let name = "edock-desktop-test-ports";
    let _ = docker.remove_container(name, Some(q::RemoveContainerOptionsBuilder::default().force(true).build())).await;
    ensure_image(&app, "nginx:alpine").await;
    let body = bollard::models::ContainerCreateBody { image: Some("nginx:alpine".into()), env: Some(vec!["GREETING=hello".into()]), ..Default::default() };
    let id = docker.create_container(Some(q::CreateContainerOptionsBuilder::default().name(name).build()), body).await.unwrap().id;
    docker.start_container(&id, None::<q::StartContainerOptions>).await.unwrap();
    struct Gone(bollard::Docker, String);
    impl Drop for Gone {
        fn drop(&mut self) {
            let (d, id) = (self.0.clone(), self.1.clone());
            let _ = std::thread::spawn(move || tokio::runtime::Runtime::new().unwrap().block_on(async move {
                let _ = d.remove_container(&id, Some(q::RemoveContainerOptionsBuilder::default().force(true).build())).await;
            })).join();
        }
    }
    let _gone = Gone(docker.clone(), id.clone());

    // It listens on 80, which nothing publishes.
    let start = Instant::now();
    let ports: ContainerPorts = loop {
        let p: ContainerPorts = app.invoke("container_ports", (id.clone(),)).await.unwrap();
        if p.listening.contains(&80) {
            break p;
        }
        assert!(start.elapsed() < Duration::from_secs(20), "80 never showed as listening");
        tokio::time::sleep(Duration::from_millis(300)).await;
    };
    assert!(ports.published.is_empty());

    // Publish it while it runs: the Mac reaches it.
    let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port() as u32;
    let bind = PortBinding { container_port: 80, host_port: free, proto: "tcp".into(), host_ip: "".into() };
    let _: () = app.invoke("container_set_ports", (id.clone(), vec![bind.clone()])).await.unwrap();
    let get = |port: u32| std::process::Command::new("curl").args(["-s", "-o", "/dev/null", "-w", "%{http_code}", "--max-time", "3", &format!("http://127.0.0.1:{port}/")]).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let mut code = String::new();
    for _ in 0..20 {
        code = get(free);
        if code == "200" { break; }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    assert_eq!(code, "200", "published port didn't answer");
    let p: ContainerPorts = app.invoke("container_ports", (id.clone(),)).await.unwrap();
    assert_eq!(p.published, vec![bind]);

    // Its domain, through the router.
    let d: DomainsInfo = app.invoke("domains_info", ()).await.unwrap();
    if d.http_on {
        let url = format!("http://{name}.{}:{}/", d.suffix, d.http_port);
        let out = std::process::Command::new("curl").args(["-s", "-o", "/dev/null", "-w", "%{http_code}", "--max-time", "5", &url]).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "200", "{url}");
    }

    // Unpublish: gone again.
    let _: () = app.invoke("container_set_ports", (id.clone(), Vec::<PortBinding>::new())).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_ne!(get(free), "200");

    // A new environment: it restarts with it, and keeps its files.
    let exec = |cmd: &'static str| {
        let docker = docker.clone();
        let id = id.clone();
        async move {
            use bollard::exec::{CreateExecOptions, StartExecResults};
            let e = docker.create_exec(&id, CreateExecOptions { attach_stdout: Some(true), cmd: Some(vec!["sh", "-c", cmd]), ..Default::default() }).await.unwrap();
            let mut out = String::new();
            if let StartExecResults::Attached { mut output, .. } = docker.start_exec(&e.id, None).await.unwrap() {
                use futures_util::StreamExt;
                while let Some(Ok(o)) = output.next().await { out.push_str(&o.to_string()); }
            }
            out
        }
    };
    exec("echo kept > /tmp/marker").await;
    let key = "-----BEGIN KEY-----\nabc\n-----END KEY-----";
    let env = vec!["GREETING=hei ".to_string(), format!("KEY={key}"), "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".to_string()];
    let _: () = app.invoke("container_set_env", (id.clone(), env)).await.unwrap();
    // Exactly as given: the space at the end, and every line of the key.
    assert_eq!(exec("printf '[%s]' \"$GREETING\"").await, "[hei ]");
    assert_eq!(exec("printf '%s' \"$KEY\"").await, key);
    assert_eq!(exec("cat /tmp/marker").await.trim(), "kept");
    assert!(app.invoke_err("container_set_env", (id.clone(), vec!["not an assignment".to_string()])).await.contains("NAME=value"));
    assert!(app.invoke_err("container_ports", ("../images".to_string(),)).await.contains("isn't a container"));
}

/// A Linux machine on eDock's engine, made, used and deleted through the
/// app: `EDOCK_TEST_HOST=unix://$HOME/.edock/run/docker.sock`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs eDock's engine"]
async fn a_linux_machine_from_create_to_delete() {
    use crate::daemon::Progress;
    use crate::machines::MachineRow;
    if !std::env::var("EDOCK_TEST_HOST").is_ok_and(|h| h.contains("/.edock/")) {
        eprintln!("skipped: needs EDOCK_TEST_HOST on eDock's socket");
        return;
    }
    let _shared = ENGINE.read().await;
    let app = app();
    app.listen();
    let name = "edock-test-machine";
    let _ = app.invoke::<()>("machine_action", (name.to_string(), "delete".to_string())).await;

    let _: () = app.invoke("machine_create", (name.to_string(), "alpine".to_string(), false, true)).await.unwrap();
    assert!(app.invoke_err("machine_create", ("other".to_string(), "alpine".to_string(), false, false)).await.contains("being made"));
    let done = wait_long::<Progress>(&app, "machine:progress", 180, |p| p.iter().any(|p| p.done)).await;
    assert!(done.iter().find(|p| p.done).unwrap().error.is_none(), "{:?}", done.iter().map(|p| &p.line).collect::<Vec<_>>());

    let rows: Vec<MachineRow> = app.invoke("machines_list", ()).await.unwrap();
    let m = rows.iter().find(|m| m.name == name).expect("listed");
    assert_eq!((m.state.as_str(), m.distro.as_str()), ("running", "Alpine 3.22"));

    // A login shell: the Mac user, in their home, with /Users there.
    let user = std::env::var("USER").unwrap();
    let term: u32 = app.invoke("machine_shell", (name.to_string(), 80u32, 24u32)).await.unwrap();
    let _: () = app.invoke("exec_input", (term, "echo \"who=$(id -un) at=$(pwd)\"; ls -d /Users/*/ | head -1\r".to_string())).await.unwrap();
    let want = format!("who={user} at=/Users/{user}");
    wait_for(&app, &format!("exec:{term}"), |c: &[TermChunk]| c.iter().map(|c| c.data.as_str()).collect::<String>().contains(&want)).await;
    // The Mac's SSH agent, for login shells.
    let _: () = app.invoke("exec_input", (term, "echo \"agent=$SSH_AUTH_SOCK\"; [ -S \"$SSH_AUTH_SOCK\" ] && echo agent-is-a-socket\r".to_string())).await.unwrap();
    wait_for(&app, &format!("exec:{term}"), |c: &[TermChunk]| c.iter().map(|c| c.data.as_str()).collect::<String>().contains("agent-is-a-socket")).await;
    let _: () = app.invoke("exec_input", (term, "exit\r".to_string())).await.unwrap();

    // It's one of eDock's own in the container list.
    let all: Vec<ContainerRow> = app.invoke("containers_list", (false,)).await.unwrap();
    let row = all.iter().find(|c| c.name == format!("machine-{name}")).expect("in the list");
    assert_eq!((row.system.as_deref(), row.domain.as_deref()), (Some("machine"), None));

    // Stopped, it survives a prune: its whole system is in it.
    let _: () = app.invoke("machine_action", (name.to_string(), "stop".to_string())).await.unwrap();
    // A prune aimed at this machine alone (its label), so the test never
    // touches anyone else's stopped containers: still, the engine keeps it.
    let docker = app.ctx().get::<Engine>().docker().unwrap();
    let mut only = std::collections::HashMap::new();
    only.insert("label".to_string(), vec![format!("app.edock.machine={name}")]);
    docker.prune_containers(Some(bollard::query_parameters::PruneContainersOptionsBuilder::default().filters(&only).build())).await.unwrap();
    let rows: Vec<MachineRow> = app.invoke("machines_list", ()).await.unwrap();
    assert!(rows.iter().any(|m| m.name == name), "a prune took the stopped machine");

    // Start, stop, start, delete.
    for action in ["start", "stop", "start"] {
        let _: () = app.invoke("machine_action", (name.to_string(), action.to_string())).await.unwrap();
    }
    let rows: Vec<MachineRow> = app.invoke("machines_list", ()).await.unwrap();
    assert_eq!(rows.iter().find(|m| m.name == name).map(|m| m.state.as_str()), Some("running"));
    assert!(app.invoke_err("machine_action", ("../x".to_string(), "delete".to_string())).await.contains("letters, digits"));
    let _: () = app.invoke("machine_action", (name.to_string(), "delete".to_string())).await.unwrap();
    let rows: Vec<MachineRow> = app.invoke("machines_list", ()).await.unwrap();
    assert!(rows.iter().all(|m| m.name != name));
}

/// An image's layers, a vulnerability scan and its SBOM, on eDock's engine:
/// `EDOCK_TEST_HOST=… EDOCK_TEST_SCAN=1` (the first scan downloads Grype
/// and its database, about 200 MB).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs eDock's engine"]
async fn an_images_layers_scan_and_sbom() {
    use crate::images::{ImageLayers, ScanProgress, ScanResult, ScanSummary};
    if !std::env::var("EDOCK_TEST_HOST").is_ok_and(|h| h.contains("/.edock/")) {
        eprintln!("skipped: needs EDOCK_TEST_HOST on eDock's socket");
        return;
    }
    let _shared = ENGINE.read().await;
    let app = app();
    app.listen();
    ensure_image(&app, "alpine:3.20").await;

    let l: ImageLayers = app.invoke("image_layers", ("alpine:3.20".to_string(),)).await.unwrap();
    assert!(l.platform.contains("arm64"), "{}", l.platform);
    assert!(l.layers.iter().any(|x| x.by.starts_with("ADD") && x.size > 1_000_000.0));
    assert!(app.invoke_err("image_layers", ("no-such-image:0".to_string(),)).await.contains("no image"));

    if std::env::var("EDOCK_TEST_SCAN").is_err() {
        eprintln!("scan skipped: set EDOCK_TEST_SCAN=1");
        return;
    }
    let _: () = app.invoke("image_scan", ("alpine:3.20".to_string(),)).await.unwrap();
    assert!(app.invoke_err("image_scan", ("alpine:3.20".to_string(),)).await.contains("being scanned"));
    let p = wait_long::<ScanProgress>(&app, "scan:progress", 900, |p| p.iter().any(|p| p.done)).await;
    let last = p.iter().find(|p| p.done).unwrap();
    assert!(last.error.is_none(), "{:?}", last.error);
    assert!(p.iter().any(|p| p.phase == "scanning"));

    let scans: Vec<ScanSummary> = app.invoke("image_scans", ("alpine:3.20".to_string(),)).await.unwrap();
    let newest = scans.first().expect("a scan");
    assert!(newest.packages > 5);
    let r: Option<ScanResult> = app.invoke("image_scan_result", ("alpine:3.20".to_string(), Some(newest.scanned))).await.unwrap();
    let r = r.expect("findings");
    assert_eq!(r.vulns.len() as u32, newest.vulns);
    assert!(r.vulns.iter().all(|v| !v.id.is_empty() && !v.severity.is_empty()));

    let out = std::env::temp_dir().join(format!("edock-sbom-{}.json", std::process::id()));
    let file = elyra::files::ChosenFile::for_saving(&out);
    let _: () = app.invoke("image_sbom_save", ("alpine:3.20".to_string(), newest.scanned, file)).await.unwrap();
    let sbom: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(sbom["bomFormat"], "CycloneDX");
    let _ = std::fs::remove_file(out);
}

/// The Engine page's settings on eDock's engine. Changes the VM's size and
/// the builder's settings, then puts them back: `EDOCK_TEST_HOST=…`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs eDock's engine"]
async fn resources_builder_import_list_and_diagnostics() {
    use crate::vm::{BuilderInfo, ImportList, ImportSource, VmInfo};
    if !std::env::var("EDOCK_TEST_HOST").is_ok_and(|h| h.contains("/.edock/")) {
        eprintln!("skipped: needs EDOCK_TEST_HOST on eDock's socket");
        return;
    }
    // A VM restart stops containers: alone.
    let _alone = ENGINE.write().await;
    let app = app();

    let before: VmInfo = app.invoke("vm_info", ()).await.unwrap();
    assert!(before.max_cpus >= 1 && before.max_memory_mib >= 1024 && before.cpus >= 1);
    assert!(["none", "some", "high"].contains(&before.memory_pressure.as_str()) && before.memory_stall >= 0.0, "{}", before.memory_pressure);
    // The user's own size, as it was (no file: eDock's defaults): put back
    // whatever happens below, or a failed run leaves their VM small.
    let file = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join(".edock/vm.json");
    let saved = std::fs::read(&file).ok();
    let cpus = if before.cpus > 1 { before.cpus - 1 } else { 2.min(before.max_cpus) };
    // Restart VM: what runs with a restart policy runs again after it.
    ensure_image(&app, "alpine:latest").await;
    let kept = format!("edock-test-vmrestart-{}", std::process::id());
    policy_container(&app.ctx().get::<Engine>().docker().unwrap(), &kept, Some("unless-stopped")).await;
    let tried = async {
        let after: VmInfo = app.invoke("vm_set_resources", (cpus, 4096u32)).await.map_err(|e| format!("{e:?}"))?;
        let bad = app.invoke_err("vm_set_resources", (0u32, 4096u32)).await;
        // Restart now: the VM comes back with the new size.
        let _: () = app.invoke("vm_restart", ()).await.map_err(|e| format!("{e:?}"))?;
        let docker = app.ctx().get::<Engine>().docker().map_err(|e| e.to_string())?;
        let _ = docker.info().await; // wakes it
        let now: VmInfo = app.invoke("vm_info", ()).await.map_err(|e| format!("{e:?}"))?;
        Ok::<_, String>((after, bad, now))
    }
    .await;
    let _: Result<VmInfo, _> = app.invoke("vm_set_resources", (before.cpus, before.memory_mib)).await;
    match &saved {
        Some(bytes) => std::fs::write(&file, bytes).unwrap(),
        None => {
            let _ = std::fs::remove_file(&file);
        }
    }
    let _: () = app.invoke("vm_restart", ()).await.unwrap();
    let d = app.ctx().get::<Engine>().docker().unwrap();
    let came_back = runs_soon(&d, &kept).await;
    let _ = d.remove_container(&kept, Some(bollard::query_parameters::RemoveContainerOptionsBuilder::default().force(true).build())).await;
    assert!(came_back, "unless-stopped runs again after Restart VM");
    let (after, bad, now) = tried.unwrap();
    assert_eq!((after.cpus, after.memory_mib), (cpus, 4096));
    assert!(bad.contains("cpus"));
    assert!(now.awake, "the VM is up again");
    assert_eq!(now.running_cpus, cpus);

    let b: BuilderInfo = app.invoke("builder_info", ()).await.unwrap();
    let set: BuilderInfo = app.invoke("builder_set", (Some(7u32), 420u32)).await.unwrap();
    assert_eq!((set.cache_gb, set.idle_secs), (Some(7), 420));
    let back: BuilderInfo = app.invoke("builder_set", (b.cache_gb, b.idle_secs)).await.unwrap();
    assert_eq!((back.cache_gb, back.idle_secs), (b.cache_gb, b.idle_secs));
    let _freed: f64 = app.invoke("builder_prune", ()).await.unwrap();

    // What another engine has: listed, not imported (that would copy it all).
    let sources: Vec<ImportSource> = app.invoke("import_sources", ()).await.unwrap();
    if let Some(src) = sources.iter().find(|s| s.available) {
        let list: ImportList = app.invoke("import_list", (src.id.clone(),)).await.unwrap();
        eprintln!("{}: {} images, {} volumes, {} containers", src.label, list.images.len(), list.volumes.len(), list.containers.len());
    }
    assert!(app.invoke_err("import_run", ("--all".to_string(),)).await.contains("isn't an engine"));

    let out = std::env::temp_dir().join(format!("edock-diag-{}.txt", std::process::id()));
    let _: () = app.invoke("diag_report", (elyra::files::ChosenFile::for_saving(&out),)).await.unwrap();
    assert!(std::fs::read_to_string(&out).unwrap().contains("eDock diagnostics report"));
    let _ = std::fs::remove_file(out);
}

/// A build on eDock, then its history: listed, read step by step, removed.
/// Its own DOCKER_CONFIG (an `edock` context and builder), so the user's
/// ~/.docker isn't touched: `EDOCK_TEST_HOST=… EDOCK_TEST_BUILD=1` (pulls
/// BuildKit the first time).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs eDock's engine"]
async fn a_build_and_its_history() {
    use crate::builds::{BuildDetail, BuildRow};
    if !std::env::var("EDOCK_TEST_HOST").is_ok_and(|h| h.contains("/.edock/")) || std::env::var("EDOCK_TEST_BUILD").is_err() {
        eprintln!("skipped: needs EDOCK_TEST_HOST on eDock's socket and EDOCK_TEST_BUILD=1");
        return;
    }
    // Alone: this process's DOCKER_CONFIG changes for a while.
    let _alone = ENGINE.write().await;
    let home = std::env::var("HOME").unwrap();
    let cfg = std::env::temp_dir().join(format!("edock-build-test-{}", std::process::id()));
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(cfg.join("config.json"), serde_json::json!({"cliPluginsExtraDirs": [format!("{home}/.docker/cli-plugins")]}).to_string()).unwrap();
    // SAFETY: the test holds the engine lock alone; nothing else reads it now.
    unsafe { std::env::set_var("DOCKER_CONFIG", &cfg) };
    let docker = |args: &[&str]| {
        let out = std::process::Command::new("docker").args(args).output().unwrap();
        assert!(out.status.success(), "docker {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let sock = format!("host=unix://{home}/.edock/run/docker.sock");
    docker(&["context", "create", "edock", "--docker", &sock]);
    docker(&["buildx", "create", "--name", "edock-build", "--driver", "docker-container", "--driver-opt", "default-load=true", "edock"]);
    let dir = cfg.join("app");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Dockerfile"), "FROM alpine:3.20\nRUN echo built-by-edock-test > /hello\n").unwrap();
    docker(&["--context", "edock", "buildx", "build", "--builder", "edock-build", "--load", "-t", "edock-build-test:1", dir.to_str().unwrap()]);

    let app = app();
    let rows: Vec<BuildRow> = app.invoke("builds_list", ()).await.unwrap();
    let b = rows.iter().find(|r| r.builder == "edock-build").expect("the build is in the history");
    assert_eq!(b.status, "Completed");
    let d: BuildDetail = app.invoke("build_detail", (b.builder.clone(), b.id.clone())).await.unwrap();
    assert!(d.steps.iter().any(|s| s.name.contains("RUN echo built-by-edock-test")), "{:?}", d.steps.iter().map(|s| &s.name).collect::<Vec<_>>());
    assert!(d.error.is_none());
    assert!(app.invoke_err("build_detail", ("edock-build".to_string(), "../x".to_string())).await.contains("isn't a build"));

    // The image it made: in eDock under its tag, with no build before it.
    let first: Option<crate::builds::BuildImage> = app.invoke("build_image", (b.builder.clone(), b.id.clone())).await.unwrap();
    let first = first.expect("the build made an image");
    assert_eq!(first.reference.as_deref(), Some("edock-build-test:1"));
    assert!(first.size > 1e6 && first.image_id.starts_with("sha256:"));
    assert!(first.previous_size.is_none());
    // Built again, ~2 MB bigger: compared with the first.
    std::fs::write(dir.join("Dockerfile"), "FROM alpine:3.20\nRUN echo built-by-edock-test > /hello\nRUN head -c 2000000 /dev/urandom > /blob\n").unwrap();
    docker(&["--context", "edock", "buildx", "build", "--builder", "edock-build", "--load", "-t", "edock-build-test:1", dir.to_str().unwrap()]);
    let rows: Vec<BuildRow> = app.invoke("builds_list", ()).await.unwrap();
    let second = rows.iter().filter(|r| r.builder == "edock-build" && r.id != b.id).max_by(|a, b| a.started.cmp(&b.started)).expect("the second build");
    let img: Option<crate::builds::BuildImage> = app.invoke("build_image", (second.builder.clone(), second.id.clone())).await.unwrap();
    let img = img.unwrap();
    assert_eq!(img.previous_id.as_deref(), Some(b.id.as_str()));
    let grew = img.size - img.previous_size.unwrap();
    assert!(grew > 1.9e6 && grew < 2.2e6, "grew {grew}");
    assert_eq!(img.reference.as_deref(), Some("edock-build-test:1"));
    // The first's image is untagged now: no name to open.
    let first: Option<crate::builds::BuildImage> = app.invoke("build_image", (b.builder.clone(), b.id.clone())).await.unwrap();
    assert!(first.unwrap().reference.is_none());
    let _: () = app.invoke("build_remove", (second.builder.clone(), second.id.clone())).await.unwrap();

    let _: () = app.invoke("build_remove", (b.builder.clone(), b.id.clone())).await.unwrap();
    let rows: Vec<BuildRow> = app.invoke("builds_list", ()).await.unwrap();
    assert!(!rows.iter().any(|r| r.id == b.id));

    // Leave nothing: the builder (its container), the image, the config.
    let _ = std::process::Command::new("docker").args(["buildx", "rm", "edock-build"]).output();
    let _ = std::process::Command::new("docker").args(["--context", "edock", "rmi", "edock-build-test:1"]).output();
    unsafe { std::env::remove_var("DOCKER_CONFIG") };
    let _ = std::fs::remove_dir_all(&cfg);
}

/// The proxy setting, changed and put back: `EDOCK_TEST_HOST=…`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs eDock's engine"]
async fn the_proxy_setting() {
    use crate::vm::ProxyInfo;
    if !std::env::var("EDOCK_TEST_HOST").is_ok_and(|h| h.contains("/.edock/")) {
        eprintln!("skipped: needs EDOCK_TEST_HOST on eDock's socket");
        return;
    }
    let _alone = ENGINE.write().await;
    let file = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join(".edock/proxy.json");
    let had = std::fs::read(&file).ok();
    let app = app();
    let before: ProxyInfo = app.invoke("proxy_info", ()).await.unwrap();
    let set: ProxyInfo = app
        .invoke("proxy_set", ("custom".to_string(), "http://proxy.test:3128".to_string(), String::new(), "localhost,.local".to_string(), false))
        .await
        .unwrap();
    assert_eq!((set.mode.as_str(), set.http.as_str(), set.no_proxy.as_str()), ("custom", "http://proxy.test:3128", "localhost,.local"));
    assert_eq!(set.effective_http.as_deref(), Some("http://proxy.test:3128"));
    assert!(app.invoke_err("proxy_set", ("sometimes".to_string(), String::new(), String::new(), String::new(), false)).await.contains("mode"));
    let _: ProxyInfo = app.invoke("proxy_set", (before.mode, before.http, before.https, before.no_proxy, before.containers)).await.unwrap();
    match had {
        Some(bytes) => std::fs::write(&file, bytes).unwrap(),
        None => {
            let _ = std::fs::remove_file(&file);
        }
    }
}

/// USB: a software serial device (the engine's `edock usb fake serial`)
/// goes into eDock, shows in the list with its /dev path, a container sees
/// it, and it's given back: `EDOCK_TEST_HOST=…`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs eDock's engine"]
async fn a_usb_device_in_and_out() {
    use crate::usb::UsbList;
    if !std::env::var("EDOCK_TEST_HOST").is_ok_and(|h| h.contains("/.edock/")) {
        eprintln!("skipped: needs EDOCK_TEST_HOST on eDock's socket");
        return;
    }
    let _alone = ENGINE.write().await;
    let app = app();
    let bin = crate::daemon::engine_binary().unwrap();
    let mut fake = std::process::Command::new(bin).args(["usb", "fake", "serial", "--location", "t-serial"]).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
    let start = Instant::now();
    let dev = loop {
        let l: UsbList = app.invoke("usb_list", ()).await.unwrap();
        if let Some(d) = l.devices.into_iter().find(|d| d.key == "t-serial" && !d.paths.is_empty()) {
            break d;
        }
        assert!(start.elapsed() < Duration::from_secs(30), "the fake device never showed");
        tokio::time::sleep(Duration::from_millis(300)).await;
    };
    assert_eq!(dev.state, "edock");
    let tty = dev.paths.iter().find(|p| p.path.starts_with("/dev/tty")).map(|p| p.path.clone()).expect("a tty");

    // A container with --device sees it.
    let docker = app.ctx().get::<Engine>().docker().unwrap();
    use bollard::query_parameters as q;
    let body = bollard::models::ContainerCreateBody {
        image: Some("alpine:latest".into()),
        cmd: Some(vec!["ls".into(), tty.clone()]),
        host_config: Some(bollard::models::HostConfig {
            devices: Some(vec![bollard::models::DeviceMapping { path_on_host: Some(tty.clone()), path_in_container: Some(tty.clone()), cgroup_permissions: Some("rwm".into()) }]),
            ..Default::default()
        }),
        ..Default::default()
    };
    let id = docker.create_container(None::<q::CreateContainerOptions>, body).await.unwrap().id;
    docker.start_container(&id, None::<q::StartContainerOptions>).await.unwrap();
    let _ = docker.wait_container(&id, None::<q::WaitContainerOptions>).collect::<Vec<_>>().await;
    use futures_util::StreamExt;
    let logs: Vec<_> = docker.logs(&id, Some(q::LogsOptionsBuilder::default().stdout(true).stderr(true).build())).collect().await;
    let out: String = logs.into_iter().filter_map(Result::ok).map(|l| l.to_string()).collect();
    let _ = docker.remove_container(&id, Some(q::RemoveContainerOptionsBuilder::default().force(true).build())).await;
    assert!(out.contains(&tty), "the container didn't see {tty}: {out}");

    // Given back: gone from eDock, and the device's process ends.
    let _: () = app.invoke("usb_detach", ("t-serial".to_string(),)).await.unwrap();
    let start = Instant::now();
    loop {
        let l: UsbList = app.invoke("usb_list", ()).await.unwrap();
        if !l.devices.iter().any(|d| d.key == "t-serial") { break; }
        assert!(start.elapsed() < Duration::from_secs(15), "still listed after detach");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let _ = fake.kill();
    let _ = fake.wait();
    assert!(app.invoke_err("usb_attach", ("--all".to_string(),)).await.contains("isn't a USB device"));
}

/// The HTTP inspector: requests through a container's domain are recorded,
/// read back whole, replayed and cleared; recording is left as it was.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs eDock's engine"]
async fn the_http_inspector() {
    use crate::inspector::{HttpExchange, HttpLog};
    use bollard::query_parameters as q;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    if !std::env::var("EDOCK_TEST_HOST").is_ok_and(|h| h.contains("/.edock/")) {
        eprintln!("skipped: needs EDOCK_TEST_HOST on eDock's socket");
        return;
    }
    let _alone = ENGINE.write().await;
    let app = app();
    let was: HttpLog = app.invoke("http_requests", (Some(f64::MAX),)).await.unwrap();
    let _: () = app.invoke("http_record", (true,)).await.unwrap();

    let docker = app.ctx().get::<Engine>().docker().unwrap();
    let name = format!("edock-test-http-{}", std::process::id());
    let body = bollard::models::ContainerCreateBody {
        image: Some("nginx:alpine".into()),
        labels: Some([("app.edock.test".to_string(), "1".to_string())].into()),
        ..Default::default()
    };
    ensure_image(&app, "nginx:alpine").await;
    docker.create_container(Some(q::CreateContainerOptionsBuilder::default().name(&name).build()), body).await.unwrap();
    docker.start_container(&name, None::<q::StartContainerOptions>).await.unwrap();

    let port = crate::daemon::engine_request("GET", "/edock/domains").await.unwrap().1["http"]["port"].as_u64().unwrap() as u16;
    let ask = |path: &'static str, payload: &'static str| {
        let host = format!("{name}.edock.localhost");
        async move {
            let mut c = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            let req = format!("POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}", payload.len());
            c.write_all(req.as_bytes()).await.unwrap();
            let mut out = Vec::new();
            let _ = c.read_to_end(&mut out).await;
            String::from_utf8_lossy(&out).into_owned()
        }
    };
    // nginx needs a moment before it answers.
    let start = Instant::now();
    while !ask("/", "{}").await.starts_with("HTTP/1.1 405") {
        assert!(start.elapsed() < Duration::from_secs(20), "nginx never answered");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    // Well before the request: its log lines stay out of its window.
    tokio::time::sleep(Duration::from_secs(2)).await;
    ask("/api/orders", r#"{"n":1}"#).await;

    let mine = |l: &HttpLog| l.items.iter().filter(|r| r.container == name && r.url == "/api/orders").map(|r| r.seq).collect::<Vec<_>>();
    let log: HttpLog = app.invoke("http_requests", (None::<f64>,)).await.unwrap();
    let seqs = mine(&log);
    assert_eq!(seqs.len(), 1, "one request to /api/orders");
    let d: HttpExchange = app.invoke("http_request", (seqs[0],)).await.unwrap();
    assert_eq!((d.row.method.as_str(), d.row.status, d.row.done), ("POST", Some(404), true));
    assert_eq!(d.req_body.and_then(|b| b.text).as_deref(), Some(r#"{"n":1}"#));
    assert!(d.res_headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("server") && v.starts_with("nginx")));

    // What nginx wrote while it answered: its access line (stdout) and its
    // error line (stderr), not the earlier requests to /.
    let during: crate::inspector::DuringLogs = app.invoke("http_request_logs", (seqs[0],)).await.unwrap();
    assert_eq!(during.containers, [name.clone()]);
    assert!(during.lines.iter().any(|l| !l.stderr && l.text.contains("POST /api/orders") && l.text.contains(" 404 ")), "{:?}", during.lines);
    assert!(during.lines.iter().any(|l| l.stderr && l.text.contains("/api/orders")), "nginx's error line: {:?}", during.lines);
    assert!(during.lines.iter().all(|l| !l.text.contains("POST / HTTP")), "an earlier request's line: {:?}", during.lines);
    assert!(during.lines.iter().all(|l| l.offset >= -0.5 && l.offset <= during.to - during.from), "{:?}", during.lines);
    assert!(during.lines.windows(2).all(|w| w[0].offset <= w[1].offset), "in time order");

    // Only what changed since: nothing.
    let since: HttpLog = app.invoke("http_requests", (Some(log.version),)).await.unwrap();
    assert!(mine(&since).is_empty());

    // Replayed: a second, the same.
    let _: () = app.invoke("http_replay", (seqs[0],)).await.unwrap();
    let log: HttpLog = app.invoke("http_requests", (None::<f64>,)).await.unwrap();
    let again = mine(&log);
    assert_eq!(again.len(), 2, "the replay shows");
    let d: HttpExchange = app.invoke("http_request", (again[1],)).await.unwrap();
    assert_eq!(d.req_body.and_then(|b| b.text).as_deref(), Some(r#"{"n":1}"#));

    let _ = docker.remove_container(&name, Some(q::RemoveContainerOptionsBuilder::default().force(true).build())).await;
    let _: () = app.invoke("http_clear", ()).await.unwrap();
    let log: HttpLog = app.invoke("http_requests", (None::<f64>,)).await.unwrap();
    assert!(log.items.is_empty());
    let _: () = app.invoke("http_record", (was.on,)).await.unwrap();
}

/// Snapshots: a container's files and a volume's, changed and put back.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a Docker engine"]
async fn snapshots_of_a_container_and_a_volume() {
    use crate::snapshots::{Restored, Snapshot};
    use bollard::query_parameters as q;
    let _alone = ENGINE.read().await;
    let app = app();
    let docker = app.ctx().get::<Engine>().docker().unwrap();
    let tag = std::process::id();
    let (name, vol) = (format!("edock-test-snap-{tag}"), format!("edock-test-snapvol-{tag}"));
    ensure_image(&app, "alpine:latest").await;
    let labels: std::collections::HashMap<String, String> = [("app.edock.test".to_string(), "1".to_string())].into();
    docker.create_volume(bollard::models::VolumeCreateRequest { name: Some(vol.clone()), labels: Some(labels.clone()), ..Default::default() }).await.unwrap();
    let body = bollard::models::ContainerCreateBody {
        image: Some("alpine:latest".into()),
        cmd: Some(vec!["sleep".into(), "300".into()]),
        env: Some(vec!["KEPT=yes".into()]),
        labels: Some(labels),
        host_config: Some(bollard::models::HostConfig { binds: Some(vec![format!("{vol}:/data")]), ..Default::default() }),
        ..Default::default()
    };
    docker.create_container(Some(q::CreateContainerOptionsBuilder::default().name(&name).build()), body).await.unwrap();
    docker.start_container(&name, None::<q::StartContainerOptions>).await.unwrap();
    let sh = |cmd: &str| {
        let (app, name, cmd) = (&app, name.clone(), cmd.to_owned());
        async move {
            let r: crate::agent::RunResult = app.invoke("container_exec", (name, cmd, None::<u32>)).await.unwrap();
            r.output.trim().to_owned()
        }
    };
    sh("echo before > /etc/marker; echo v1 > /data/v; chown 70:70 /data/v").await;

    let image: String = app.invoke("container_snapshot", (name.clone(), Some("first".to_string()))).await.unwrap();
    // A volume is best snapshotted with its writers stopped, but it works anyway.
    let file: String = app.invoke("volume_snapshot", (vol.clone(), None::<String>)).await.unwrap();
    let list: Vec<Snapshot> = app.invoke("snapshots_list", ()).await.unwrap();
    assert!(list.iter().any(|s| s.kind == "container" && s.id == image && s.of == name && s.note == "first"), "{image} listed");
    assert!(list.iter().any(|s| s.kind == "volume" && s.id == file && s.of == vol && s.size > 0.0));

    sh("echo after > /etc/marker; echo v2 > /data/v; touch /data/new").await;

    // The volume can't be restored under a running container.
    let refused = app.invoke::<()>("volume_snapshot_restore", (vol.clone(), file.clone())).await;
    assert!(format!("{:?}", refused.unwrap_err()).contains("running"));

    let r: Restored = app.invoke("container_snapshot_restore", (image.clone(),)).await.unwrap();
    let previous = r.previous.expect("the replaced one is kept");
    assert_eq!(sh("cat /etc/marker; echo $KEPT").await, "before\nyes", "files and settings back");
    let old = docker.inspect_container(&previous, None::<q::InspectContainerOptions>).await.unwrap();
    assert_eq!(old.state.and_then(|s| s.running), Some(false));
    let _ = docker.remove_container(&previous, Some(q::RemoveContainerOptionsBuilder::default().force(true).build())).await;

    docker.stop_container(&name, Some(q::StopContainerOptionsBuilder::default().t(1).build())).await.unwrap();
    let _: () = app.invoke("volume_snapshot_restore", (vol.clone(), file.clone())).await.unwrap();
    docker.start_container(&name, None::<q::StartContainerOptions>).await.unwrap();
    assert_eq!(sh("cat /data/v; ls /data; stat -c %u /data/v").await, "v1\nv\n70", "the volume as it was, owner kept");

    let _ = docker.remove_container(&name, Some(q::RemoveContainerOptionsBuilder::default().force(true).build())).await;
    let _: () = app.invoke("snapshot_delete", ("container".to_string(), name.clone(), image.clone())).await.unwrap();
    let _: () = app.invoke("snapshot_delete", ("volume".to_string(), vol.clone(), file.clone())).await.unwrap();
    let _ = docker.remove_volume(&vol, None::<q::RemoveVolumeOptions>).await;
    let list: Vec<Snapshot> = app.invoke("snapshots_list", ()).await.unwrap();
    assert!(!list.iter().any(|s| s.id == image || s.id == file), "deleted");
}

/// Restoring a Compose service's container: its anonymous volume (an
/// image's VOLUME) stays with it, and the one replaced is kept as a
/// snapshot, never as a second container of the service. Two snapshots in
/// the same second are two.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a Docker engine"]
async fn restoring_keeps_anonymous_volumes_and_compose_whole() {
    use crate::snapshots::{Restored, Snapshot};
    use bollard::query_parameters as q;
    let _alone = ENGINE.read().await;
    let app = app();
    let docker = app.ctx().get::<Engine>().docker().unwrap();
    let tag = std::process::id();
    let (name, vol, project) = (format!("edock-test-snapc-{tag}"), format!("edock-test-snapcvol-{tag}"), format!("edocktest{tag}"));
    ensure_image(&app, "alpine:latest").await;
    let test: std::collections::HashMap<String, String> = [("app.edock.test".to_string(), "1".to_string())].into();
    docker.create_volume(bollard::models::VolumeCreateRequest { name: Some(vol.clone()), labels: Some(test.clone()), ..Default::default() }).await.unwrap();
    let mut labels = test.clone();
    labels.insert("com.docker.compose.project".into(), project.clone());
    labels.insert("com.docker.compose.service".into(), "db".into());
    labels.insert("com.docker.compose.container-number".into(), "1".into());
    let body = bollard::models::ContainerCreateBody {
        image: Some("alpine:latest".into()),
        cmd: Some(vec!["sleep".into(), "300".into()]),
        labels: Some(labels),
        volumes: Some(vec!["/anon".to_string()]),
        host_config: Some(bollard::models::HostConfig { binds: Some(vec![format!("{vol}:/named")]), ..Default::default() }),
        ..Default::default()
    };
    docker.create_container(Some(q::CreateContainerOptionsBuilder::default().name(&name).build()), body).await.unwrap();
    docker.start_container(&name, None::<q::StartContainerOptions>).await.unwrap();
    let sh = |cmd: &str| {
        let (app, name, cmd) = (&app, name.clone(), cmd.to_owned());
        async move {
            let r: crate::agent::RunResult = app.invoke("container_exec", (name, cmd, None::<u32>)).await.unwrap();
            r.output.trim().to_owned()
        }
    };
    sh("echo data > /anon/a").await;
    let anon = docker.inspect_container(&name, None::<q::InspectContainerOptions>).await.unwrap().mounts.unwrap_or_default().into_iter().find(|m| m.destination.as_deref() == Some("/anon")).and_then(|m| m.name).expect("an anonymous volume");

    let one: String = app.invoke("container_snapshot", (name.clone(), None::<String>)).await.unwrap();
    let two: String = app.invoke("container_snapshot", (name.clone(), None::<String>)).await.unwrap();
    assert_ne!(one, two, "two snapshots, two names");
    let f1: String = app.invoke("volume_snapshot", (vol.clone(), None::<String>)).await.unwrap();
    let f2: String = app.invoke("volume_snapshot", (vol.clone(), None::<String>)).await.unwrap();
    assert_ne!(f1, f2);

    let r: Restored = app.invoke("container_snapshot_restore", (one.clone(),)).await.unwrap();
    let kept = r.previous.expect("the one before is kept");
    assert!(kept.starts_with("edock-snapshots/"), "kept as a snapshot: {kept}");
    assert_eq!(sh("cat /anon/a").await, "data", "the anonymous volume came along");
    let mut filters = std::collections::HashMap::new();
    filters.insert("label".to_string(), vec![format!("com.docker.compose.project={project}")]);
    let service = docker.list_containers(Some(q::ListContainersOptionsBuilder::default().all(true).filters(&filters).build())).await.unwrap();
    assert_eq!(service.len(), 1, "one container in the service");
    let list: Vec<Snapshot> = app.invoke("snapshots_list", ()).await.unwrap();
    assert!(list.iter().any(|s| s.id == kept && s.note.starts_with("Before restoring")));

    let _ = docker.remove_container(&name, Some(q::RemoveContainerOptionsBuilder::default().force(true).v(true).build())).await;
    for image in [&one, &two, &kept] {
        let _: () = app.invoke("snapshot_delete", ("container".to_string(), name.clone(), image.clone())).await.unwrap();
    }
    for file in [&f1, &f2] {
        let _: () = app.invoke("snapshot_delete", ("volume".to_string(), vol.clone(), file.clone())).await.unwrap();
    }
    let _ = docker.remove_volume(&vol, None::<q::RemoveVolumeOptions>).await;
    let _ = docker.remove_volume(&anon, None::<q::RemoveVolumeOptions>).await;
}

/// The archive API (`docker cp`) as Docker has it: `dir/.` is the folder's
/// contents, and an upload keeps the owners its archive gives.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a Docker engine"]
async fn the_archive_api_matches_dockers() {
    use bollard::query_parameters as q;
    let _alone = ENGINE.read().await;
    let app = app();
    let docker = app.ctx().get::<Engine>().docker().unwrap();
    ensure_image(&app, "alpine:latest").await;
    let name = format!("edock-test-archive-{}", std::process::id());
    let body = bollard::models::ContainerCreateBody {
        image: Some("alpine:latest".into()),
        cmd: Some(vec!["sh".into(), "-c".into(), "mkdir -p /v/sub && echo a > /v/a && echo b > /v/sub/b && sleep 300".into()]),
        labels: Some([("app.edock.test".to_string(), "1".to_string())].into()),
        ..Default::default()
    };
    docker.create_container(Some(q::CreateContainerOptionsBuilder::default().name(&name).build()), body).await.unwrap();
    docker.start_container(&name, None::<q::StartContainerOptions>).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let names = |path: &'static str| {
        let (docker, name) = (docker.clone(), name.clone());
        async move {
            use futures_util::StreamExt;
            let bytes: Vec<u8> = docker
                .download_from_container(&name, Some(q::DownloadFromContainerOptionsBuilder::default().path(path).build()))
                .map(|b| b.unwrap().to_vec())
                .concat()
                .await;
            let mut a = tar::Archive::new(&bytes[..]);
            let mut v: Vec<String> = a.entries().unwrap().map(|e| e.unwrap().path().unwrap().to_string_lossy().trim_start_matches("./").trim_end_matches('/').to_owned()).filter(|n| !n.is_empty() && n != ".").collect();
            v.sort();
            v
        }
    };
    assert_eq!(names("/v/.").await, ["a", "sub", "sub/b"], "dir/. is its contents");
    assert_eq!(names("/v").await, ["v", "v/a", "v/sub", "v/sub/b"]);

    let mut t = tar::Builder::new(Vec::new());
    let mut h = tar::Header::new_ustar();
    h.set_size(3);
    h.set_mode(0o640);
    h.set_uid(70);
    h.set_gid(71);
    t.append_data(&mut h, "owned", &b"hi\n"[..]).unwrap();
    let tarball = t.into_inner().unwrap();
    docker
        .upload_to_container(&name, Some(q::UploadToContainerOptionsBuilder::default().path("/tmp").build()), bollard::body_full(tarball.into()))
        .await
        .unwrap();
    let r: crate::agent::RunResult = app.invoke("container_exec", (name.clone(), "stat -c %u:%g /tmp/owned".to_string(), None::<u32>)).await.unwrap();
    let _ = docker.remove_container(&name, Some(q::RemoveContainerOptionsBuilder::default().force(true).build())).await;
    assert_eq!(r.output.trim(), "70:71", "the archive's owners kept");
}

/// The Files tab: list, preview, save, upload and remove in a container.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a Docker engine"]
async fn a_containers_files() {
    use crate::files::{FileEntry, FilePreview};
    use bollard::query_parameters as q;
    use elyra::files::ChosenFile;
    let _alone = ENGINE.read().await;
    let app = app();
    let docker = app.ctx().get::<Engine>().docker().unwrap();
    let name = format!("edock-test-files-{}", std::process::id());
    ensure_image(&app, "nginx:alpine").await;
    let body = bollard::models::ContainerCreateBody {
        image: Some("nginx:alpine".into()),
        labels: Some([("app.edock.test".to_string(), "1".to_string())].into()),
        ..Default::default()
    };
    docker.create_container(Some(q::CreateContainerOptionsBuilder::default().name(&name).build()), body).await.unwrap();
    docker.start_container(&name, None::<q::StartContainerOptions>).await.unwrap();

    let ls = |path: &str| {
        let (app, name, path) = (&app, name.clone(), path.to_owned());
        async move { app.invoke::<Vec<FileEntry>>("container_files", (name, path)).await }
    };
    let etc = ls("/etc/nginx").await.unwrap();
    assert_eq!(etc[0].kind, "dir", "folders first");
    assert!(etc.iter().any(|e| e.name == "nginx.conf" && e.kind == "file" && e.size > 100.0));
    assert!(ls("/nope").await.is_err());

    let p: FilePreview = app.invoke("container_file_preview", (name.clone(), "/etc/nginx/nginx.conf".to_string())).await.unwrap();
    assert!(p.text.unwrap().contains("worker_processes") && !p.cut && !p.binary);
    let p: FilePreview = app.invoke("container_file_preview", (name.clone(), "/usr/sbin/nginx".to_string())).await.unwrap();
    assert!(p.binary && p.size > 1000.0);
    let p: FilePreview = app.invoke("container_file_preview", (name.clone(), "/bin/sh".to_string())).await.unwrap();
    assert!(p.link.is_some(), "/bin/sh is a link in alpine");
    assert!(app.invoke::<FilePreview>("container_file_preview", (name.clone(), "/etc".to_string())).await.is_err());

    let dir = std::env::temp_dir().join(format!("edock-files-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("up/sub")).unwrap();
    let out = dir.join("nginx.conf");
    let _: () = app.invoke("container_file_save", (name.clone(), "/etc/nginx/nginx.conf".to_string(), ChosenFile::for_saving(&out))).await.unwrap();
    assert!(std::fs::read_to_string(&out).unwrap().contains("worker_processes"));
    let tar = dir.join("conf.tar");
    let _: () = app.invoke("container_file_save", (name.clone(), "/etc/nginx".to_string(), ChosenFile::for_saving(&tar))).await.unwrap();
    let listing = std::process::Command::new("tar").arg("tf").arg(&tar).output().unwrap();
    assert!(String::from_utf8_lossy(&listing.stdout).contains("nginx/nginx.conf"));

    // Up: a file and a folder with a file in it, with a name past tar's 100.
    let long = "l".repeat(110);
    std::fs::write(dir.join("up/hello.txt"), "hi from the mac").unwrap();
    std::fs::write(dir.join("up/sub").join(&long), "deep").unwrap();
    let n: u32 = app.invoke("container_files_upload", (name.clone(), "/tmp".to_string(), vec![ChosenFile::from_path(dir.join("up"))])).await.unwrap();
    assert_eq!(n, 2);
    let up = ls("/tmp/up").await.unwrap();
    assert_eq!(up.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["sub", "hello.txt"]);
    let p: FilePreview = app.invoke("container_file_preview", (name.clone(), format!("/tmp/up/sub/{long}"))).await.unwrap();
    assert_eq!(p.text.as_deref(), Some("deep"));

    let _: () = app.invoke("container_file_remove", (name.clone(), "/tmp/up".to_string())).await.unwrap();
    assert!(!ls("/tmp").await.unwrap().iter().any(|e| e.name == "up"));
    assert!(app.invoke::<()>("container_file_remove", (name.clone(), "/".to_string())).await.is_err());

    // Stopped: no listing, but a preview still works.
    docker.stop_container(&name, Some(q::StopContainerOptionsBuilder::default().t(1).build())).await.unwrap();
    assert!(format!("{:?}", ls("/").await.unwrap_err()).contains("Start the container"));
    let p: FilePreview = app.invoke("container_file_preview", (name.clone(), "/etc/nginx/nginx.conf".to_string())).await.unwrap();
    assert!(p.text.is_some());

    let _ = docker.remove_container(&name, Some(q::RemoveContainerOptionsBuilder::default().force(true).build())).await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// Recipes: Redis answers on its Mac port; Mailpit's UI on its own.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a Docker engine"]
async fn recipes_start_services() {
    use crate::recipes::Started;
    use bollard::query_parameters as q;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let _alone = ENGINE.read().await;
    let app = app();
    let docker = app.ctx().get::<Engine>().docker().unwrap();
    let tag = std::process::id();
    let wait_for = |port: u16, req: &'static [u8], want: &'static str| async move {
        let start = Instant::now();
        loop {
            if let Ok(mut c) = tokio::net::TcpStream::connect(("127.0.0.1", port)).await {
                let _ = c.write_all(req).await;
                let mut buf = vec![0u8; 512];
                if let Ok(Ok(n)) = tokio::time::timeout(Duration::from_secs(2), c.read(&mut buf)).await {
                    if String::from_utf8_lossy(&buf[..n]).contains(want) {
                        return;
                    }
                }
            }
            assert!(start.elapsed() < Duration::from_secs(60), "nothing answered {want} on {port}");
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    };

    let redis: Started = app.invoke("recipe_start", ("redis".to_string(), Some(format!("edock-test-redis-{tag}")), None::<String>)).await.unwrap();
    assert_eq!(redis.connect, format!("redis://localhost:{}", redis.port));
    wait_for(redis.port, b"PING\r\n", "+PONG").await;
    let info = docker.inspect_container(&redis.container, None::<q::InspectContainerOptions>).await.unwrap();
    assert_eq!(info.host_config.and_then(|h| h.restart_policy).and_then(|r| r.name).map(|n| n.to_string()), Some("unless-stopped".into()));

    // The same name again: the next one.
    let again: Started = app.invoke("recipe_start", ("redis".to_string(), Some(format!("edock-test-redis-{tag}")), None::<String>)).await.unwrap();
    assert_eq!(again.container, format!("edock-test-redis-{tag}-2"));
    assert_ne!(again.port, redis.port);

    let mail: Started = app.invoke("recipe_start", ("mailpit".to_string(), Some(format!("edock-test-mail-{tag}")), None::<String>)).await.unwrap();
    let ui: u16 = mail.ui.as_deref().and_then(|u| u.rsplit(':').next()).and_then(|p| p.parse().ok()).unwrap();
    wait_for(ui, b"GET / HTTP/1.0\r\n\r\n", "200 OK").await;
    wait_for(mail.port, b"", "220").await;

    // ElyraSQL: MySQL's greeting, with its own version in it, on 3307 (or the next free).
    let esql: Started = app.invoke("recipe_start", ("elyrasql".to_string(), Some(format!("edock-test-esql-{tag}")), None::<String>)).await.unwrap();
    assert_eq!(esql.connect, format!("mysql://root:elyrasql@localhost:{}/elyra", esql.port));
    wait_for(esql.port, b"", "ElyraSQL").await;
    // A scratch image, no find in it: on eDock the engine shows its volume's
    // files as the container sees them.
    if std::env::var("EDOCK_TEST_HOST").is_ok_and(|h| h.contains("/.edock/")) {
        let data: Vec<crate::files::FileEntry> = app.invoke("container_files", (esql.container.clone(), "/var/lib/elyrasql".to_string())).await.unwrap();
        assert!(data.iter().any(|e| e.name == "elyra.edb"), "the volume's file: {:?}", data.iter().map(|e| &e.name).collect::<Vec<_>>());
    }

    // SQL Anywhere: sqld's health and version over HTTP.
    let sqla: Started = app.invoke("recipe_start", ("sqlanywhere".to_string(), Some(format!("edock-test-sqla-{tag}")), None::<String>)).await.unwrap();
    assert_eq!(sqla.connect, format!("http://localhost:{}", sqla.port));
    wait_for(sqla.port, b"GET /version HTTP/1.0\r\n\r\n", "sqld 0.11").await;

    // Askr: a PHP folder from the Mac, served and written as the Mac user,
    // named after the folder.
    let dir = std::env::temp_dir().join(format!("edock-test-askr-{tag}"));
    std::fs::create_dir_all(dir.join("public")).unwrap();
    std::fs::write(dir.join("public/index.php"), "<?php file_put_contents(__DIR__.'/../written.txt', 'ok'); echo 'askr-says-hi uid=', getmyuid();").unwrap();
    assert!(app.invoke::<Started>("recipe_start", ("askr".to_string(), None::<String>, None::<String>)).await.is_err(), "it needs a folder");
    let askr: Started = app.invoke("recipe_start", ("askr".to_string(), None::<String>, Some(dir.display().to_string()))).await.unwrap();
    assert_eq!(askr.container, format!("edock-test-askr-{tag}"));
    // SAFETY: getuid cannot fail.
    let uid = unsafe { libc::getuid() };
    wait_for(askr.port, b"GET / HTTP/1.0\r\n\r\n", "askr-says-hi").await;
    wait_for(askr.port, b"GET / HTTP/1.0\r\n\r\n", Box::leak(format!("uid={uid}").into_boxed_str())).await;
    assert_eq!(std::fs::read_to_string(dir.join("written.txt")).unwrap(), "ok");
    let info = docker.inspect_container(&askr.container, None::<q::InspectContainerOptions>).await.unwrap();
    assert_eq!(info.config.and_then(|c| c.labels).and_then(|l| l.get("app.edock.http.port").cloned()).as_deref(), Some("8080"));

    assert!(app.invoke::<Started>("recipe_start", ("nope".to_string(), None::<String>, None::<String>)).await.is_err());
    let _ = std::fs::remove_dir_all(&dir);
    for c in [&redis.container, &again.container, &mail.container, &esql.container, &sqla.container, &askr.container] {
        let _ = docker.remove_container(c, Some(q::RemoveContainerOptionsBuilder::default().force(true).build())).await;
        let _ = docker.remove_volume(&format!("{c}-data"), None::<q::RemoveVolumeOptions>).await;
    }
}

/// Engine › Builds' "docker build uses", read (switching it changes
/// ~/.docker, so the engine's own tests cover that), and the command line's
/// check for another edock on PATH.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs eDock's engine binary"]
async fn build_with_and_the_path_check() {
    let app = app();
    let b: crate::daemon::BuildWith = app.invoke("build_with", ()).await.unwrap();
    assert!(b.with == "docker" || b.with == "edock", "{}", b.with);
    assert!(app.invoke::<crate::daemon::BuildWith>("build_with_set", ("podman".to_string(),)).await.is_err());
    let cli: crate::daemon::CliStatus = app.invoke("cli_status", ()).await.unwrap();
    // Whatever this Mac has: a path, when there is one, is absolute.
    assert!(cli.stale.as_deref().is_none_or(|p| p.starts_with('/')));
}

/// The engine's log, and the speed test on every engine running here,
/// leaving no container behind.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs eDock's engine"]
async fn engine_log_and_speed_test() {
    use crate::bench::BenchRow;
    let _alone = ENGINE.read().await;
    let app = app();
    let log: String = app.invoke("engine_log", (Some(20u32), None::<bool>)).await.unwrap();
    assert!(!log.trim().is_empty() && log.lines().count() <= 25, "{log}");
    let rows: Vec<BenchRow> = app.invoke("speed_test", (Some(3u32),)).await.unwrap();
    let edock = rows.iter().find(|r| r.engine == "eDock").expect("eDock runs");
    assert!(edock.error.is_none() && edock.p50 > 0.0 && edock.p90 >= edock.p50 && edock.runs == 3, "{:?}", edock.error);
    let home = std::env::var("HOME").unwrap();
    for r in &rows {
        assert!(r.error.is_none(), "{}: {:?}", r.engine, r.error);
        let sock = match r.engine.as_str() {
            "eDock" => ".edock/run/docker.sock",
            "OrbStack" => ".orbstack/run/docker.sock",
            _ => ".docker/run/docker.sock",
        };
        let out = std::process::Command::new("docker").args(["-H", &format!("unix://{home}/{sock}"), "ps", "-aq", "--filter", "label=app.edock.bench"]).output().unwrap();
        assert!(out.stdout.is_empty(), "{} has bench containers left", r.engine);
    }
}

/// A Compose project as a whole: up again from its own files, and one log
/// for its services, each line with its service.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a Docker engine and docker compose"]
async fn a_compose_project_as_a_whole() {
    use crate::streams::LogBatch;
    let _alone = ENGINE.read().await;
    let app = app();
    // Its events queued from now: the tails come once, before anything polls.
    app.listen();
    let host = app.ctx().get::<Engine>().context().1;
    let project = format!("edock-test-proj-{}", std::process::id());
    let dir = std::env::temp_dir().join(&project);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("compose.yaml"),
        "services:\n  api:\n    image: alpine:latest\n    command: sh -c 'echo api-says-hi; sleep 300'\n    environment:\n      API_SECRET: hunter2\n      FROM_DOTENV: ${DOTENV_VALUE}\n    labels: [app.edock.test=1]\n  worker:\n    image: alpine:latest\n    command: sh -c 'echo worker-says-hi; sleep 300'\n    labels: [app.edock.test=1]\n  tools:\n    image: alpine:latest\n    profiles: [debug]\n    labels: [app.edock.test=1]\n",
    )
    .unwrap();
    std::fs::write(dir.join(".env"), "# for the test\nDOTENV_VALUE=s3cret\n").unwrap();
    let compose = |args: &[&str]| std::process::Command::new("docker").args(["compose", "-p", &project]).args(args).current_dir(&dir).env("DOCKER_HOST", &host).env_remove("DOCKER_CONTEXT").output().unwrap();
    let up = compose(&["up", "-d"]);
    assert!(up.status.success(), "{}", String::from_utf8_lossy(&up.stderr));

    // Up again, as the app does it: from the labels alone.
    let _: () = app.invoke("project_up", (project.clone(), false)).await.unwrap();

    let id: u32 = app.invoke("project_logs_follow", (project.clone(), 50u32, false)).await.unwrap();
    let batches: Vec<LogBatch> = wait_for(&app, &format!("logs:{id}"), |b: &[LogBatch]| {
        let all: Vec<_> = b.iter().flat_map(|b| &b.lines).collect();
        all.iter().any(|l| l.text == "api-says-hi") && all.iter().any(|l| l.text == "worker-says-hi")
    })
    .await;
    let lines: Vec<_> = batches.iter().flat_map(|b| &b.lines).collect();
    assert!(lines.iter().any(|l| l.text == "api-says-hi" && l.service.as_deref() == Some("api")));
    assert!(lines.iter().any(|l| l.text == "worker-says-hi" && l.service.as_deref() == Some("worker")));
    let _: () = app.invoke("stream_stop", (id,)).await.unwrap();

    assert!(app.invoke::<String>("folder_open", ("relative/path".to_string(), "finder".to_string())).await.is_err());

    // What it's made of: names, never values.
    let view: crate::compose::ComposeView = app.invoke("compose_view", (project.clone(),)).await.unwrap();
    let api = view.services.iter().find(|s| s.name == "api").expect("api");
    assert_eq!(api.env, ["API_SECRET", "FROM_DOTENV"]);
    assert_eq!(view.env_files.len(), 1);
    assert_eq!(view.env_files[0].keys, ["DOTENV_VALUE"]);
    let shown = serde_json::to_string(&view.services).unwrap() + &serde_json::to_string(&view.env_files).unwrap();
    assert!(!shown.contains("hunter2") && !shown.contains("s3cret"), "no values: {shown}");
    // A service in a profile is there too, with its profile.
    let tools = view.services.iter().find(|s| s.name == "tools").expect("a service in a profile");
    assert_eq!(tools.profiles, ["debug"]);

    // One service made again; the other stays as it was.
    let docker = app.ctx().get::<Engine>().docker().unwrap();
    let ids = |docker: bollard::Docker, project: String| async move {
        let mut filters = std::collections::HashMap::new();
        filters.insert("label".to_string(), vec![format!("com.docker.compose.project={project}")]);
        let list = docker.list_containers(Some(bollard::query_parameters::ListContainersOptionsBuilder::default().filters(&filters).build())).await.unwrap();
        list.into_iter().map(|c| (c.labels.unwrap_or_default().get("com.docker.compose.service").cloned().unwrap_or_default(), c.id.unwrap_or_default())).collect::<std::collections::HashMap<_, _>>()
    };
    let before = ids(docker.clone(), project.clone()).await;
    let _: () = app.invoke("service_up", (project.clone(), "api".to_string(), false)).await.unwrap();
    let after = ids(docker.clone(), project.clone()).await;
    assert_ne!(before["api"], after["api"], "api made again");
    assert_eq!(before["worker"], after["worker"], "worker left alone");
    assert!(app.invoke_err("service_up", (project.clone(), "--all".to_string(), false)).await.contains("isn't a Compose service"));

    // Remembered, and still there after `docker compose down`, with Up.
    use crate::compose::ProjectRow;
    let listed = |rows: Vec<ProjectRow>| rows.into_iter().find(|p| p.name == project);
    let p = listed(app.invoke("compose_projects", ()).await.unwrap()).expect("remembered while up");
    assert_eq!((p.dir.as_str(), p.problem.as_deref()), (std::fs::canonicalize(&dir).unwrap().to_str().unwrap(), None));
    let down = compose(&["down", "-t", "1"]);
    assert!(down.status.success());
    let p = listed(app.invoke("compose_projects", ()).await.unwrap()).expect("remembered when down");
    assert!(p.problem.is_none(), "{:?}", p.problem);
    let _: () = app.invoke("project_up", (project.clone(), false)).await.unwrap();
    let docker = app.ctx().get::<Engine>().docker().unwrap();
    let mut filters = std::collections::HashMap::new();
    filters.insert("label".to_string(), vec![format!("com.docker.compose.project={project}")]);
    let up = docker.list_containers(Some(bollard::query_parameters::ListContainersOptionsBuilder::default().filters(&filters).build())).await.unwrap();
    assert_eq!(up.len(), 2, "both services up again from what was remembered");
    let down = compose(&["down", "-t", "1"]);
    assert!(down.status.success());

    // Forgotten, then opened from its folder: the name Compose gives it.
    let _: () = app.invoke("compose_project_forget", (project.clone(),)).await.unwrap();
    assert!(listed(app.invoke("compose_projects", ()).await.unwrap()).is_none());
    let named: String = app.invoke("compose_project_open", (dir.display().to_string(),)).await.unwrap();
    assert_eq!(named, project, "the folder's name");
    assert!(listed(app.invoke("compose_projects", ()).await.unwrap()).is_some());
    let _: () = app.invoke("compose_project_forget", (project.clone(),)).await.unwrap();
    assert!(app.invoke::<String>("compose_project_open", (std::env::temp_dir().display().to_string(),)).await.is_err(), "no compose file");
    let _ = std::fs::remove_dir_all(&dir);

    // A service that needs one in a profile: Compose's check fails without
    // every profile, so the project couldn't be opened.
    let other = std::env::temp_dir().join(format!("{project}-profiles"));
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("compose.yaml"), "services:\n  web:\n    image: alpine:latest\n    depends_on: [db]\n  db:\n    image: alpine:latest\n    profiles: [data]\n").unwrap();
    let named: String = app.invoke("compose_project_open", (other.display().to_string(),)).await.unwrap();
    assert_eq!(named, format!("{project}-profiles"));
    let _: () = app.invoke("compose_project_forget", (named,)).await.unwrap();
    let _ = std::fs::remove_dir_all(&other);
}

/// A volume's files without a container of the user's: browsed through a
/// helper, cloned, exported and imported, owners kept.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a Docker engine"]
async fn a_volume_browsed_cloned_exported_imported() {
    use crate::files::{FileEntry, FilePreview};
    use futures_util::StreamExt;
    use bollard::query_parameters as q;
    use elyra::files::ChosenFile;
    let _alone = ENGINE.read().await;
    let app = app();
    let docker = app.ctx().get::<Engine>().docker().unwrap();
    let tag = std::process::id();
    let (vol, copy, back) = (format!("edock-test-vol-{tag}"), format!("edock-test-vol-{tag}-copy"), format!("edock-test-vol-{tag}-back"));
    docker.create_volume(bollard::models::VolumeCreateRequest { name: Some(vol.clone()), ..Default::default() }).await.unwrap();
    let r: crate::agent::RunResult = app
        .invoke("container_run", ("alpine:latest".to_string(), "true".to_string(), None::<bool>, None::<u32>))
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0);
    // Fill it, with an owner that isn't root.
    let fill = bollard::models::ContainerCreateBody {
        image: Some("alpine:latest".into()),
        cmd: Some(vec!["sh".into(), "-c".into(), "mkdir -p /d/sub && echo hello > /d/sub/a.txt && chown -R 70:70 /d/sub".into()]),
        labels: Some([("app.edock.test".to_string(), "1".to_string())].into()),
        host_config: Some(bollard::models::HostConfig { binds: Some(vec![format!("{vol}:/d")]), auto_remove: Some(true), ..Default::default() }),
        ..Default::default()
    };
    let id = docker.create_container(None::<q::CreateContainerOptions>, fill).await.unwrap().id;
    docker.start_container(&id, None::<q::StartContainerOptions>).await.unwrap();
    let _ = docker.wait_container(&id, None::<q::WaitContainerOptions>).collect::<Vec<_>>().await;

    // Browse: a helper, hidden among eDock's own.
    let helper: String = app.invoke("volume_browse", (vol.clone(),)).await.unwrap();
    let again: String = app.invoke("volume_browse", (vol.clone(),)).await.unwrap();
    assert_eq!(helper, again, "one helper per volume");
    let rows: Vec<crate::resources::ContainerRow> = app.invoke("containers_list", (false,)).await.unwrap();
    assert_eq!(rows.iter().find(|c| c.id == helper).and_then(|c| c.system.clone()).as_deref(), Some("helper"));
    // The volume's own top (where a volume is mounted, not the empty folder under it).
    let top: Vec<FileEntry> = app.invoke("container_files", (helper.clone(), "/v".to_string())).await.unwrap();
    assert_eq!(top.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["sub"], "the volume at /v");
    let list: Vec<FileEntry> = app.invoke("container_files", (helper.clone(), "/v/sub".to_string())).await.unwrap();
    assert_eq!(list.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["a.txt"]);
    let p: FilePreview = app.invoke("container_file_preview", (helper.clone(), "/v/sub/a.txt".to_string())).await.unwrap();
    assert_eq!(p.text.as_deref(), Some("hello\n"));
    let _: () = app.invoke("volume_browse_end", (vol.clone(),)).await.unwrap();

    // What's in a volume, and its owner.
    let check = |v: String| {
        let docker = docker.clone();
        async move {
            let body = bollard::models::ContainerCreateBody {
                image: Some("alpine:latest".into()),
                cmd: Some(vec!["sh".into(), "-c".into(), "cat /m/sub/a.txt; stat -c %u /m/sub/a.txt".into()]),
                labels: Some([("app.edock.test".to_string(), "1".to_string())].into()),
                host_config: Some(bollard::models::HostConfig { binds: Some(vec![format!("{v}:/m:ro")]), ..Default::default() }),
                ..Default::default()
            };
            let id = docker.create_container(None::<q::CreateContainerOptions>, body).await.unwrap().id;
            docker.start_container(&id, None::<q::StartContainerOptions>).await.unwrap();
            let _ = docker.wait_container(&id, None::<q::WaitContainerOptions>).collect::<Vec<_>>().await;
            let logs: Vec<_> = docker.logs(&id, Some(q::LogsOptionsBuilder::default().stdout(true).stderr(true).build())).collect().await;
            let _ = docker.remove_container(&id, Some(q::RemoveContainerOptionsBuilder::default().force(true).build())).await;
            logs.into_iter().filter_map(Result::ok).map(|l| l.to_string()).collect::<String>()
        }
    };

    let _: () = app.invoke("volume_clone", (vol.clone(), copy.clone())).await.unwrap();
    assert_eq!(check(copy.clone()).await, "hello\n70\n");
    assert!(app.invoke::<()>("volume_clone", (vol.clone(), copy.clone())).await.is_err(), "not over an existing one");

    let file = std::env::temp_dir().join(format!("{vol}.tar"));
    let _: () = app.invoke("volume_export", (vol.clone(), ChosenFile::for_saving(&file))).await.unwrap();
    let listing = std::process::Command::new("tar").arg("tvf").arg(&file).output().unwrap();
    assert!(String::from_utf8_lossy(&listing.stdout).contains("sub/a.txt"));
    let _: () = app.invoke("volume_import", (back.clone(), ChosenFile::from_path(&file))).await.unwrap();
    assert_eq!(check(back.clone()).await, "hello\n70\n");

    // No helper left.
    let mut filters = std::collections::HashMap::new();
    filters.insert("label".to_string(), vec!["app.edock.helper".to_string()]);
    // (A helper removing itself may still show for a moment.)
    let start = Instant::now();
    loop {
        let helpers = docker.list_containers(Some(q::ListContainersOptionsBuilder::default().all(true).filters(&filters).build())).await.unwrap();
        if helpers.iter().all(|c| !c.labels.as_ref().is_some_and(|l| l.values().any(|v| v.contains(&format!("edock-test-vol-{tag}"))))) {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "a helper is left");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    for v in [&vol, &copy, &back] {
        let _ = docker.remove_volume(v, None::<q::RemoveVolumeOptions>).await;
    }
    let _ = std::fs::remove_file(&file);
}

/// A container killed for memory and one turning unhealthy: the engine's
/// events become alerts (each once), and eDock's own say nothing.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a Docker engine"]
async fn alerts_for_out_of_memory_and_unhealthy() {
    use crate::alerts::{Alert, What};
    use bollard::query_parameters as q;
    let _alone = ENGINE.read().await;
    let app = app();
    let engine = app.ctx().get::<Engine>();
    let docker = engine.docker().unwrap();
    let got: std::sync::Arc<parking_lot::Mutex<Vec<Alert>>> = std::sync::Arc::default();
    let g2 = got.clone();
    crate::streams::watch(engine.clone(), app.ctx().get::<elyra::EventBus>(), |_| {}, move |a| g2.lock().push(a));
    tokio::time::sleep(Duration::from_millis(800)).await;

    let tag = std::process::id();
    let run = |name: String, cmd: &str, host: bollard::models::HostConfig, health: Option<bollard::models::HealthConfig>| {
        let docker = docker.clone();
        let cmd = cmd.to_owned();
        async move {
            let body = bollard::models::ContainerCreateBody {
                image: Some("alpine:latest".into()),
                cmd: Some(vec!["sh".into(), "-c".into(), cmd]),
                labels: Some([("app.edock.test".to_string(), "1".to_string())].into()),
                healthcheck: health,
                host_config: Some(host),
                ..Default::default()
            };
            docker.create_container(Some(q::CreateContainerOptionsBuilder::default().name(&name).build()), body).await.unwrap();
            docker.start_container(&name, None::<q::StartContainerOptions>).await.unwrap();
            name
        }
    };
    let hungry = run(
        format!("edock-test-oom-{tag}"),
        "head -c 400m /dev/zero | tail > /dev/null",
        bollard::models::HostConfig { memory: Some(32 * 1024 * 1024), memory_swap: Some(32 * 1024 * 1024), ..Default::default() },
        None,
    )
    .await;
    let sick = run(
        format!("edock-test-sick-{tag}"),
        "sleep 120",
        bollard::models::HostConfig::default(),
        Some(bollard::models::HealthConfig { test: Some(vec!["CMD-SHELL".into(), "false".into()]), interval: Some(500_000_000), timeout: Some(1_000_000_000), retries: Some(1), start_period: None, start_interval: None }),
    )
    .await;

    let start = Instant::now();
    loop {
        let g = got.lock();
        let oom = g.iter().filter(|a| a.name == hungry && a.what == What::OutOfMemory).count();
        let unhealthy = g.iter().filter(|a| a.name == sick && a.what == What::Unhealthy).count();
        if oom >= 1 && unhealthy >= 1 {
            assert_eq!((oom, unhealthy), (1, 1), "each once");
            break;
        }
        drop(g);
        assert!(start.elapsed() < Duration::from_secs(30), "alerts so far: {:?}", got.lock());
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let info = docker.inspect_container(&hungry, None::<q::InspectContainerOptions>).await.unwrap();
    assert_eq!(info.state.and_then(|s| s.oom_killed), Some(true));
    for c in [&hungry, &sick] {
        let _ = docker.remove_container(c, Some(q::RemoveContainerOptionsBuilder::default().force(true).build())).await;
    }
}

/// Run image…: suggestions from the image, then a container with a port,
/// a Mac folder, a volume, a variable and a command, served on the Mac.
/// One that doesn't start is removed.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a Docker engine"]
async fn run_image_as_docker_run() {
    use crate::run::{RunDefaults, RunEnv, RunMount, RunPort, RunSpec, RunStarted};
    use bollard::query_parameters as q;
    let _alone = ENGINE.read().await;
    let app = app();
    ensure_image(&app, "nginx:alpine").await;
    let docker = app.ctx().get::<Engine>().docker().unwrap();
    let tag = std::process::id();
    let d: RunDefaults = app.invoke("image_run_defaults", ("nginx:alpine".to_string(),)).await.unwrap();
    assert!(d.local && d.name.starts_with("nginx"), "{}", d.name);
    let web = d.ports.iter().find(|p| p.container == 80 && !p.udp).expect("nginx exposes 80");
    assert!(web.host.unwrap() >= 8080, "80 is suggested high: {:?}", web.host);
    let gone: RunDefaults = app.invoke("image_run_defaults", ("edock-test/not-here:1".to_string(),)).await.unwrap();
    assert!(!gone.local && gone.ports.is_empty());

    let dir = std::env::temp_dir().join(format!("edock-test-run-{tag}"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), "hello from the Mac\n").unwrap();
    let (name, vol) = (format!("edock-test-run-{tag}"), format!("edock-test-run-{tag}"));
    let port = crate::recipes::free_port(18000 + (tag % 1000) as u16, &[]);
    let spec = RunSpec {
        image: "nginx:alpine".into(),
        name: Some(name.clone()),
        ports: vec![RunPort { host: Some(port), container: 80, udp: false }],
        lan: false,
        mounts: vec![
            RunMount { source: dir.display().to_string(), target: "/usr/share/nginx/html".into(), read_only: true },
            RunMount { source: vol.clone(), target: "/data".into(), read_only: false },
        ],
        env: vec![RunEnv { key: "EDOCK_TEST".into(), value: "a b=c".into() }],
        command: Some("nginx -g 'daemon off;'".into()),
        restart: true,
    };
    let s: RunStarted = app.invoke("image_run", (spec.clone(),)).await.unwrap();
    assert_eq!(s.name, name);
    let info = docker.inspect_container(&s.id, None::<q::InspectContainerOptions>).await.unwrap();
    let cfg = info.config.unwrap();
    assert!(cfg.env.unwrap().contains(&"EDOCK_TEST=a b=c".to_string()));
    assert_eq!(cfg.cmd.unwrap(), ["nginx", "-g", "daemon off;"]);
    let mounts = info.mounts.unwrap();
    assert!(mounts.iter().any(|m| m.destination.as_deref() == Some("/data") && m.name.as_deref() == Some(vol.as_str())), "{mounts:?}");
    assert!(mounts.iter().any(|m| m.destination.as_deref() == Some("/usr/share/nginx/html") && m.rw == Some(false)), "{mounts:?}");
    // Served from the Mac's folder, on the Mac's port.
    let mut body = String::new();
    for _ in 0..50 {
        if let Ok(mut c) = std::net::TcpStream::connect(("127.0.0.1", port)) {
            use std::io::{Read, Write};
            let _ = c.write_all(b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n");
            body.clear();
            let _ = c.read_to_string(&mut body);
            if body.contains("hello from the Mac") {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(body.contains("hello from the Mac"), "{body}");
    // Files › Remove never empties the Mac folder behind a mount, or a
    // folder that has one under it.
    for at in ["/usr/share/nginx/html", "/usr/share/nginx"] {
        let e = app.invoke_err("container_file_remove", (s.id.clone(), at.to_string())).await;
        assert!(e.contains("mounted"), "{at}: {e}");
    }
    assert!(dir.join("index.html").is_file(), "the Mac's file is still there");

    // The same name again: said, not a second container.
    let again = app.invoke::<RunStarted>("image_run", (spec.clone(),)).await.unwrap_err();
    assert!(format!("{again:?}").contains("already"), "{again:?}");
    let _ = docker.remove_container(&s.id, Some(q::RemoveContainerOptionsBuilder::default().force(true).build())).await;
    let _ = docker.remove_volume(&vol, None::<q::RemoveVolumeOptions>).await;

    // A command that isn't there: when the engine says so on start (eDock,
    // Docker), no container is left behind. OrbStack starts it, and it exits.
    let broken = RunSpec { name: Some(format!("{name}-broken")), ports: vec![], mounts: vec![], command: Some("/nope/not-here".into()), restart: false, ..spec };
    match app.invoke::<RunStarted>("image_run", (broken,)).await {
        Err(_) => assert!(docker.inspect_container(&format!("{name}-broken"), None::<q::InspectContainerOptions>).await.is_err(), "removed"),
        Ok(s) => {
            let _ = docker.remove_container(&s.id, Some(q::RemoveContainerOptionsBuilder::default().force(true).build())).await;
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

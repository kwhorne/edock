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
        let _: () = app.invoke("image_pull", ("alpine:latest",)).await.unwrap();
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
    let kubectl = |args: &'static [&'static str]| {
        let docker = docker.clone();
        async move {
            use bollard::exec::{CreateExecOptions, StartExecResults};
            let mut cmd = vec!["kubectl".to_string()];
            cmd.extend(args.iter().map(|a| a.to_string()));
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
}

/// The engine's process, as `lsof` sees the socket's listener.
fn engine_pid() -> Option<String> {
    let sock = format!("{}/.edock/run/docker.sock", std::env::var("HOME").unwrap());
    let out = std::process::Command::new("/usr/sbin/lsof").args(["-t", "-a", "-c", "edock", &sock]).output().ok()?;
    String::from_utf8_lossy(&out.stdout).lines().next().map(str::to_owned)
}

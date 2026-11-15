//! eDock — the desktop app: containers, images, volumes and networks on
//! eDock's engine (or any Docker engine), a terminal and logs, and a menu
//! bar item.

mod daemon;
mod edock;
mod agent;
mod alerts;
mod inspector;
mod snapshots;
mod files;
mod recipes;
mod bench;
mod compose;
mod volumes;
mod builds;
mod engine;
mod images;
mod k8s;
mod machines;
mod resources;
mod streams;
mod usb;
mod vm;

use std::sync::Arc;

use elyra::{command, commands, AboutInfo, App, Ctx, EventBus, Provider, Tray, TrayConfig, WindowConfig, Windows};

/// The built Svelte frontend, embedded from memory. Empty until `npm run build`
/// — the shell serves its built-in fallback page in that case.
#[derive(rust_embed::RustEmbed)]
#[folder = "app/dist"]
struct Assets;

/// Starts watching the engine once the app is up, and keeps the menu
/// bar's title on the number of running containers.
struct Watcher;

impl Provider for Watcher {
    fn boot(&self, ctx: &Ctx) {
        let engine = ctx.get::<engine::Engine>();
        let bus = ctx.get::<EventBus>();
        let tray = ctx.try_get::<Tray>();
        let e2 = engine.clone();
        let (alerts, notifier, b2) = (ctx.get::<alerts::Alerts>(), ctx.get::<elyra::notifier::Notifier>(), bus.clone());
        let on_alert = move |a| alerts::show(&alerts, &notifier, &b2, a);
        streams::watch(engine, bus, move |connected| {
            let Some(tray) = tray.clone() else { return };
            let engine = e2.clone();
            tokio::spawn(async move {
                let (context, _) = engine.context();
                // eDock's engine: one background look at /info, which
                // doesn't keep its VM awake, says both. Others: Docker's.
                // The user's running containers (not eDock's own: machines,
                // Kubernetes, BuildKit), and whether eDock is stopped; both
                // as background looks on eDock's engine.
                let stopped = connected
                    && context == engine::EDOCK
                    && matches!(daemon::engine_request("GET", "/info").await, Some((200, i)) if i["EdockStopped"] == true);
                let running = if connected { resources::user_counts(&engine).await.map_or(0, |(r, _)| r) } else { 0 };
                tray.set_title(if running > 0 { Some(running.to_string()) } else { None }.as_deref());
                tray.set_tooltip(Some(&if stopped {
                    "eDock — engine stopped".to_owned()
                } else if connected {
                    format!("eDock — {running} running ({context})")
                } else {
                    format!("eDock — {context} engine not running")
                }));
            });
        }, on_alert);
    }
}

/// Show the main window (from the menu bar's popover), on `path` when
/// given: the main window's page navigates on the `navigate` channel.
#[command]
async fn window_open(ctx: Ctx, path: Option<String>) {
    let windows = ctx.get::<Windows>();
    windows.hide_popover();
    windows.show(Some("main"));
    windows.focus(Some("main"));
    if let Some(path) = path {
        let _ = ctx.get::<EventBus>().emit("navigate", &path);
    }
}

/// The app's services and commands, without its windows: what `main`
/// runs, and what the tests drive.
fn core(engine: Arc<engine::Engine>) -> App {
    App::new()
        .bind_as::<engine::Engine>(engine)
        .bind_as::<streams::Streams>(Arc::new(streams::Streams::default()))
        .bind_as::<daemon::Daemon>(Arc::new(daemon::Daemon::default()))
        .bind_as::<k8s::Cluster>(Arc::new(k8s::Cluster::default()))
        .bind_as::<daemon::Setup>(Arc::new(daemon::Setup::default()))
        .bind_as::<machines::Creating>(Arc::new(machines::Creating::default()))
        .bind_as::<images::Scanning>(Arc::new(images::Scanning::default()))
        .bind_as::<vm::Importing>(Arc::new(vm::Importing::default()))
        .bind_as::<alerts::Alerts>(Arc::new(alerts::Alerts::default()))
        .commands(commands![
            resources::engine_status,
            resources::contexts_list,
            resources::context_use,
            resources::containers_list,
            resources::container_action,
            resources::project_action,
            resources::container_inspect,
            resources::containers_prune,
            resources::images_list,
            resources::image_remove,
            resources::images_prune,
            resources::image_pull,
            resources::volumes_list,
            resources::volume_create,
            resources::volume_remove,
            resources::volumes_prune,
            resources::networks_list,
            resources::network_create,
            resources::network_remove,
            resources::networks_prune,
            streams::stream_stop,
            streams::logs_follow,
            streams::stats_follow,
            streams::exec_start,
            streams::exec_input,
            streams::exec_resize,
            daemon::daemon_info,
            daemon::daemon_start,
            daemon::daemon_stop,
            daemon::daemon_restart,
            daemon::cli_status,
            daemon::cli_setup,
            daemon::build_with,
            daemon::build_with_set,
            k8s::k8s_status,
            k8s::k8s_cluster,
            k8s::k8s_items,
            k8s::k8s_pod_delete,
            k8s::k8s_rollout_restart,
            k8s::k8s_logs_follow,
            k8s::k8s_exec_start,
            edock::domains_info,
            edock::domains_set_ports,
            edock::tls_trust,
            edock::container_ports,
            edock::container_set_ports,
            edock::container_set_env,
            machines::machine_distros,
            machines::machines_list,
            machines::machine_create,
            machines::machine_action,
            machines::machine_shell,
            images::image_layers,
            images::image_scans,
            images::image_scan_result,
            images::image_scan,
            images::image_sbom_save,
            vm::vm_info,
            vm::vm_set_resources,
            vm::vm_restart,
            vm::builder_info,
            vm::builder_set,
            vm::builder_prune,
            vm::import_sources,
            vm::import_list,
            vm::import_run,
            vm::diag_report,
            vm::proxy_info,
            vm::proxy_set,
            k8s::k8s_forward,
            k8s::k8s_unforward,
            k8s::k8s_forwards,
            builds::builds_list,
            builds::build_detail,
            builds::build_image,
            builds::build_remove,
            mcp_command,
            welcome_needed,
            welcome_done,
            usb::usb_list,
            usb::usb_attach,
            usb::usb_detach,
            agent::container_logs,
            agent::container_run,
            agent::container_exec,
            inspector::http_requests,
            inspector::http_request,
            inspector::http_record,
            inspector::http_clear,
            inspector::http_replay,
            snapshots::snapshots_list,
            snapshots::container_snapshot,
            snapshots::container_snapshot_restore,
            snapshots::volume_snapshot,
            snapshots::volume_snapshot_restore,
            snapshots::snapshot_delete,
            files::container_files,
            files::container_file_preview,
            files::container_file_save,
            files::container_files_upload,
            files::container_file_remove,
            files::container_file_reveal,
            recipes::recipes_list,
            recipes::recipe_start,
            bench::engine_log,
            bench::speed_test,
            compose::project_logs_follow,
            compose::editor_name,
            compose::folder_open,
            compose::project_up,
            volumes::volume_browse,
            volumes::volume_browse_end,
            volumes::volume_clone,
            volumes::volume_export,
            volumes::volume_import,
            alerts::alerts_on,
            alerts::alerts_set,
            window_open,
        ])
}

/// The public half of the key that signs eDock's updates
/// (`scripts/update-key`). An update must carry its signature and be
/// signed with eDock's Developer ID, or it isn't installed.
const UPDATE_KEY: &str = "LUuLU5jdxCT3273HDlkFi9kCrcDDR+hOh+TIRhAOiQ0=";
/// The newest release's manifest: GitHub sends `latest/download/…` to the
/// newest release's file.
const UPDATE_MANIFEST: &str = "https://github.com/kwhorne/edock/releases/latest/download/latest.json";
/// Who signs eDock (the Developer ID's team), and its bundle id.
const TEAM_ID: &str = "52AT44QFCM";
const BUNDLE_ID: &str = "app.edock.app";

/// The command that makes this app an MCP server for AI agents: its own
/// binary, with `--mcp`.
#[command]
async fn mcp_command(_ctx: Ctx) -> String {
    std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "/Applications/eDock.app/Contents/MacOS/edock-desktop".into())
}

/// Whether to show the welcome: the first time the app opens.
#[command]
async fn welcome_needed(_ctx: Ctx) -> bool {
    !engine::Settings::load().onboarded
}

/// The welcome was done or skipped.
#[command]
async fn welcome_done(_ctx: Ctx) {
    let mut s = engine::Settings::load();
    s.onboarded = true;
    s.save();
}

fn main() -> elyra::Result<()> {
    core(engine::Engine::new())
        .title("eDock")
        .identifier(BUNDLE_ID)
        .size(1240.0, 800.0)
        .min_size(900.0, 560.0)
        .about(
            AboutInfo::new("eDock", env!("CARGO_PKG_VERSION"))
                .description("A tiny, fast container runtime. Engine forked from Kiri (Apache-2.0).")
                .icon("/icon.svg"),
        )
        .batch_window(std::time::Duration::from_millis(8))
        // A newer release is checked for at launch; the toast offers it,
        // installs it (verified) and restarts when the user says so.
        .updater(
            elyra::UpdaterConfig::new(UPDATE_KEY, UPDATE_MANIFEST, env!("CARGO_PKG_VERSION"))
                .developer_id(TEAM_ID, BUNDLE_ID)
                .relaunch_when_asked(true)
                .auto_check(true),
        )
        .allow_frontend(elyra::security::Capability::UpdaterInstall)
        // The window may call every eDock command; agents (MCP, below)
        // only what's granted to them.
        .allow_abilities(["edock.*"])
        // AI agents: `eDock --mcp` gives Claude, Cursor or VS Code eDock's
        // commands as tools. Looking is free; changing anything is asked
        // in the agent's client first.
        .mcp(
            elyra::Mcp::new()
                .allow_abilities(["edock.view", "edock.manage", "edock.run", "edock.exec"])
                .confirm("edock.manage")
                .confirm("edock.run")
                .confirm("edock.exec"),
        )
        .stay_in_tray()
        .tray(
            TrayConfig::new()
                .tooltip("eDock")
                .popover(WindowConfig::new("quick").title("eDock").path("#/quick").size(340.0, 460.0))
                .item("open", "Open eDock")
                .item("update", "Check for Updates…")
                .separator()
                .item("engine:start", "Start eDock engine")
                .item("engine:stop", "Stop eDock engine")
                .separator()
                .quit("Quit eDock"),
        )
        .provider(Watcher)
        .assets(elyra::asset_resolver::<Assets>())
        .run()
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod update_tests {
    use elyra::updater::{UpdateStatus, Updater};

    /// What `scripts/update-key sign` made of this text with eDock's
    /// update key: the key the app trusts is the key releases are signed
    /// with.
    const TEXT: &[u8] = b"eDock update key test vector\n";
    const SIGNATURE: &str = "stI5CuLpkiIqKRlGu+iUucXdgjf+LI+LcGXGZgBC99JUCoaBJVCf69rM2EKHwE97rfxQpf3ft2JYIorm+JozDQ==";

    #[test]
    fn the_app_trusts_the_release_key() {
        let updater = Updater::new(super::UPDATE_KEY, "0.1.0").unwrap();
        assert!(updater.verify(TEXT, SIGNATURE).is_ok());
        assert!(updater.verify(b"something else", SIGNATURE).is_err());
    }

    #[test]
    fn a_newer_manifest_is_an_update_and_an_older_one_is_not() {
        let manifest = |v: &str| {
            format!(r#"{{"version":"{v}","notes":"n","platforms":{{"macos-aarch64":{{"url":"https://github.com/kwhorne/edock/releases/download/v{v}/eDock-{v}-arm64.zip","signature":"{SIGNATURE}"}}}}}}"#)
        };
        let updater = Updater::new(super::UPDATE_KEY, "0.2.0").unwrap();
        assert!(matches!(updater.evaluate(&manifest("0.3.0"), "macos-aarch64").unwrap(), UpdateStatus::Available(_)));
        assert!(matches!(updater.evaluate(&manifest("0.2.0"), "macos-aarch64").unwrap(), UpdateStatus::UpToDate));
        assert!(matches!(updater.evaluate(&manifest("0.1.0"), "macos-aarch64").unwrap(), UpdateStatus::UpToDate));
    }

    /// After a release: what an installed eDock does, short of installing.
    /// It fetches the published manifest, downloads the zip, and checks its
    /// signature against the app's key, and its SHA-256.
    /// `cargo test published_release -- --ignored`.
    #[test]
    #[ignore = "fetches the published release"]
    fn the_published_release_verifies() {
        let updater = Updater::new(super::UPDATE_KEY, "0.0.1").unwrap();
        let UpdateStatus::Available(info) = updater.check(super::UPDATE_MANIFEST, "macos-aarch64").unwrap() else {
            panic!("the published manifest offers nothing");
        };
        let staged = updater.download_verified(&info).expect("the published zip verifies");
        assert!(std::fs::metadata(&staged).unwrap().len() > 1_000_000);
        let _ = std::fs::remove_file(staged);
    }
}

//! eDock — the desktop app: containers, images, volumes and networks on
//! eDock's engine (or any Docker engine), a terminal and logs, and a menu
//! bar item.

mod daemon;
mod engine;
mod resources;
mod streams;

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
        streams::watch(engine, bus, move |connected| {
            let Some(tray) = tray.clone() else { return };
            let engine = e2.clone();
            tokio::spawn(async move {
                let running = if connected {
                    match engine.docker() {
                        Ok(d) => d.info().await.ok().and_then(|i| i.containers_running).unwrap_or(0),
                        Err(_) => 0,
                    }
                } else {
                    0
                };
                let (context, _) = engine.context();
                tray.set_title(if running > 0 { Some(running.to_string()) } else { None }.as_deref());
                tray.set_tooltip(Some(&if connected {
                    format!("eDock — {running} running ({context})")
                } else {
                    format!("eDock — {context} engine not running")
                }));
            });
        });
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
            window_open,
        ])
}

fn main() -> elyra::Result<()> {
    core(engine::Engine::new())
        .title("eDock")
        .identifier("app.edock.app")
        .size(1240.0, 800.0)
        .min_size(900.0, 560.0)
        .about(
            AboutInfo::new("eDock", env!("CARGO_PKG_VERSION"))
                .description("A tiny, fast container runtime. Engine forked from Kiri (Apache-2.0).")
                .icon("/icon.svg"),
        )
        .batch_window(std::time::Duration::from_millis(8))
        .stay_in_tray()
        .tray(
            TrayConfig::new()
                .tooltip("eDock")
                .popover(WindowConfig::new("quick").title("eDock").path("#/quick").size(340.0, 460.0))
                .item("open", "Open eDock")
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

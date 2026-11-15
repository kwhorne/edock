//! The `edock` command.

use std::process::ExitCode;

#[cfg(target_os = "macos")]
mod bench;
#[cfg(target_os = "macos")]
mod boot;
// builds, cli_pins, scan and tools on Linux too: the daemon reads an
// image's layers and scans it for edock-win, whose window can't reach the
// store inside WSL (daemon/api.rs).
#[cfg(unix)]
// Some helpers here served only the removed eDock window; kept for the eDock desktop.
#[allow(dead_code)]
mod builds;
// The engine stops when its app is deleted under it (edock-win includes it
// for Windows).
#[cfg(unix)]
mod appwatch;
// Which builder plain `docker build` uses: Docker's or eDock's.
#[cfg(unix)]
// Some helpers here served only the removed eDock window; kept for the eDock desktop.
#[allow(dead_code)]
mod buildwith;
// Linux only, but its tests run anywhere with unix sockets.
#[cfg(any(target_os = "linux", all(test, unix)))]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod bridge;
// Linux only (WSL's end of edock-win's proxy relay); its tests run anywhere.
#[cfg(unix)]
mod hostrelay;
#[cfg(unix)]
mod cli_pins;
#[cfg(unix)]
// Some helpers here served only the removed eDock window; kept for the eDock desktop.
#[allow(dead_code)]
mod client;
#[cfg(unix)]
mod daemon;
#[cfg(unix)]
mod diag;
#[cfg(any(unix, windows))]
mod docker;
mod flavor;
mod images;
#[cfg(unix)]
mod guest;
#[cfg(target_os = "linux")]
mod hostaccess;
#[cfg(target_os = "macos")]
// Some helpers here served only the removed eDock window; kept for the eDock desktop.
#[allow(dead_code)]
mod import;
#[cfg(unix)]
mod k8s;
// kubectl's own file, which `edock k8s` puts its context in.
#[cfg(unix)]
mod kubeconfig;
#[cfg(unix)]
mod machine;
#[cfg(unix)]
mod run;
#[cfg(unix)]
// Some helpers here served only the removed eDock window; kept for the eDock desktop.
#[allow(dead_code)]
mod scan;
#[cfg(target_os = "macos")]
// Some helpers here served only the removed eDock window; kept for the eDock desktop.
#[allow(dead_code)]
mod setup;
#[cfg(unix)]
// Some helpers here served only the removed eDock window; kept for the eDock desktop.
#[allow(dead_code)]
mod usb;
#[cfg(unix)]
// Some helpers here served only the removed eDock window; kept for the eDock desktop.
#[allow(dead_code)]
mod stale;
#[cfg(windows)]
mod wsl;
#[cfg(unix)]
// Some helpers here served only the removed eDock window; kept for the eDock desktop.
#[allow(dead_code)]
mod tools;

const USAGE: &str = "\
usage: edock COMMAND          docker's commands, on eDock: edock run, ps, exec,
                             logs, stop, rm, images, pull, build, compose…
       edock daemon
       edock setup [--use] [--no-agent] | edock setup --uninstall
       edock build-with [docker|edock] [--json]   which builder docker build uses
       edock diag logs [-f] [--engine|--cli] [--errors] [-n N]
       edock diag report [--out PATH]   (a local file, tokens taken out)
       edock k8s start [--use] | stop | status | delete | set [--lan on|off] [--tls-san NAME,…] [--port N]
       edock builds [list] | show|layers|rm BUILDER ID   build history, as JSON
       edock tls status | trust | untrust   HTTPS for *.edock.local(host) in browsers
       edock usb [list] | attach DEVICE | detach DEVICE   USB devices for containers, beta (Mac: pass through to the VM)
       edock machine create NAME [--distro D] | list | shell NAME | stop | delete NAME
       edock import [orbstack | desktop-linux] [--list]
       edock bench [--runs N] [--image IMAGE] [--network none|bridge]
       edock boot [--console] [--pings N]
       edock raw run [--rm] [-e KEY=VALUE]... IMAGE [COMMAND...]   (its own guest, no daemon)
       edock raw pull <image> | raw images | raw rmi <image>...";

fn main() -> ExitCode {
    flavor::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Which process this is, for its log and crash reports.
    #[cfg(unix)]
    diag::init(match args.first().map(String::as_str) {
        Some("daemon") => diag::Part::Daemon,
        _ => diag::Part::Cli,
    });
    let result: std::io::Result<()> = match args.first().map(String::as_str) {
        #[cfg(unix)]
        Some("daemon") => daemon::run(&args[1..]),
        #[cfg(target_os = "linux")]
        Some("bridge") => bridge::run(&args[1..]),
        #[cfg(target_os = "linux")]
        Some("hostrelay") => hostrelay::run(),
        // Linux hosts: what the window's onboarding needs root for (through pkexec).
        #[cfg(target_os = "linux")]
        Some("host-access") => match hostaccess::run(&args[1..]) {
            Ok(code) => return ExitCode::from(code),
            Err(e) => Err(e),
        },
        #[cfg(unix)]
        Some("raw") => match args.get(1).map(String::as_str) {
            Some("run") => run::run(&args[2..]),
            Some("pull") => images::pull(&args[2..]),
            Some("images") => images::list(&args[2..]),
            Some("rmi") => images::remove(&args[2..]),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::FAILURE;
            }
        },
        #[cfg(target_os = "macos")]
        Some("bench") => bench::run(&args[1..]),
        #[cfg(target_os = "macos")]
        Some("boot") => boot::run(&args[1..]),
        #[cfg(target_os = "macos")]
        Some("setup") => setup::run(&args[1..]),
        #[cfg(unix)]
        Some("build-with") => buildwith::cli(&args[1..]),
        #[cfg(unix)]
        Some("k8s") => k8s::run(&args[1..]),
        #[cfg(unix)]
        Some("machine") => machine::run(&args[1..]),
        #[cfg(unix)]
        Some("usb") => usb::run(&args[1..]),
        #[cfg(unix)]
        Some("tls") => tls_cli(&args[1..]),
        #[cfg(target_os = "macos")]
        Some("import") => import::run(&args[1..]),
        #[cfg(unix)]
        Some("builds") => builds::run(&args[1..]),
        #[cfg(unix)]
        Some("diag") => diag::run(&args[1..]),
        // Run from an app bundle with no arguments: first-launch setup only
        // (the desktop app is the window).
        #[cfg(target_os = "macos")]
        None if setup::launched_from_app() => setup::first_launch(),
        #[cfg(any(unix, windows))]
        Some(c) if docker::handles(c) => {
            engine_version_check();
            docker::run(&args)
        }
        // Windows: the engine's commands, by the `edock` in WSL.
        #[cfg(windows)]
        Some(c) if wsl::COMMANDS.contains(&c) => wsl::run(&args),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // The subcommand only: arguments can carry values.
            #[cfg(unix)]
            if !e.to_string().starts_with("usage:") {
                diag::warn!("command failed", command = args.first().filter(|a| a.len() <= 20 && a.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')).map_or("", String::as_str), error = e);
            }
            eprintln!("edock: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(unix))]
fn engine_version_check() {}

/// Before a docker command: when the engine runs another version than this
/// `edock`, say so (on stderr, so output stays clean). Either an update whose
/// engine restart was put off, or another `edock` earlier on PATH.
#[cfg(unix)]
fn engine_version_check() {
    let me = env!("CARGO_PKG_VERSION");
    if me == "0.0.0" || !client::alive() {
        return;
    }
    let Some(engine) = client::request("GET", "/version", None).ok().filter(|r| r.ok()).and_then(|r| r.json()["Version"].as_str().map(str::to_owned)) else { return };
    if engine != me && engine != "0.0.0" {
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "edock".into());
        eprintln!("edock: this edock ({exe}) is {me}, but the engine runs {engine}. Restart the engine (`edock daemon`) to use one version, or remove the older edock.");
    }
}

/// `edock tls status | trust | untrust`: whether macOS trusts eDock's
/// certificate authority for websites, so `https://<name>.edock.localhost`
/// shows a padlock. Trusting asks for the user's password, in macOS's own
/// prompt (it names eDock when run from eDock.app); the authority's name
/// constraints let it vouch for `*.edock.local` and `*.edock.localhost` only.
#[cfg(unix)]
fn tls_cli(args: &[String]) -> std::io::Result<()> {
    use daemon::tls;
    match args.first().map(String::as_str) {
        Some("status") | None => {
            println!("{}", if tls::trusted() { "trusted" } else { "not trusted" });
            Ok(())
        }
        Some("trust") => tls::trust().map(|()| println!("trusted")),
        Some("untrust") => tls::untrust().map(|()| println!("not trusted")),
        _ => Err(std::io::Error::other("usage: edock tls status | trust | untrust")),
    }
}

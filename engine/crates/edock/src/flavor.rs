//! eDock Dev: a second eDock beside the installed one, for trying changes
//! without touching it (`cargo xtask app-dev` builds `eDock Dev.app`).
//!
//! It is the same binary; running from a bundle whose identifier is
//! `app.edock.app.dev` makes it Dev. Everything two eDocks would share gets
//! its own name: `~/.edock-dev` (socket, settings, logs),
//! `~/Library/Application Support/eDock Dev` (images, volumes, the VM's
//! disk), the `app.edock.daemon.dev` LaunchAgent, the `edock-dev` docker and
//! kubectl contexts and `~/eDock Dev` in Finder. It leaves to the installed
//! eDock what there is one of on a Mac: `*.edock.local`, the container-name
//! routers on ports 80 and 443, docker's default context, the shell PATH
//! and updates.
//!
//! A feature app (`cargo xtask app-dev builds` builds `eDock Builds.app`,
//! identifier `app.edock.app.builds`) is a eDock Dev of its own, for
//! trying one unreleased feature: every Dev name with `dev` swapped for the
//! feature's (`~/.edock-builds`, the `edock-builds` context, and so on).

// Some names serve only the Mac (the app, its LaunchAgent).
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::fmt;
use std::sync::OnceLock;

/// eDock Dev.app's bundle identifier.
pub const DEV_APP_ID: &str = "app.edock.app.dev";

/// Whether this binary is eDock Dev (or a feature app): it runs from a
/// bundle (directly, or through `~/.edock-dev/bin/edock`, a link into it)
/// whose identifier is `DEV_APP_ID`, or `app.edock.app.<feature>`.
pub fn dev() -> bool {
    flavor().is_some()
}

/// `Some("")` for eDock Dev, `Some("builds")` for eDock Builds, `None` for eDock.
fn flavor() -> Option<&'static str> {
    static FLAVOR: OnceLock<Option<String>> = OnceLock::new();
    FLAVOR
        .get_or_init(|| {
            let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
            let plist = std::fs::read_to_string(exe.parent()?.parent()?.join("Info.plist")).ok()?;
            let id = plist.split("<key>CFBundleIdentifier</key>").nth(1)?.split("<string>").nth(1)?.split("</string>").next()?.trim();
            if id == DEV_APP_ID {
                return Some(String::new());
            }
            let f = id.strip_prefix("app.edock.app.")?;
            (!f.is_empty() && f.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())).then(|| f.to_owned())
        })
        .as_deref()
}

/// A Dev name made a feature's: `eDock Dev` → `eDock Builds`, `edock-dev` →
/// `edock-builds`, `app.edock.daemon.dev` → `app.edock.daemon.builds`.
fn featured(dev_name: &str, feature: &str) -> String {
    let title = feature[..1].to_ascii_uppercase() + &feature[1..];
    if let Some(head) = dev_name.strip_suffix(".dev") {
        return format!("{head}.{feature}");
    }
    dev_name.replacen(" Dev", &format!(" {title}"), 1).replacen("-dev", &format!("-{feature}"), 1)
}

/// A name with a Dev counterpart. Displays as the one this binary uses.
pub struct Name(pub &'static str, pub &'static str);

impl Name {
    pub fn get(&self) -> &'static str {
        match flavor() {
            None => self.0,
            Some("") => self.1,
            Some(f) => {
                static NAMES: std::sync::Mutex<Vec<(&str, &str)>> = std::sync::Mutex::new(Vec::new());
                let mut names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
                if let Some((_, n)) = names.iter().find(|(d, _)| *d == self.1) {
                    return n;
                }
                let n: &'static str = Box::leak(featured(self.1, f).into_boxed_str());
                names.push((self.1, n));
                n
            }
        }
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.get())
    }
}

/// The app's name, as the window and Finder show it.
pub const APP: Name = Name("eDock", "eDock Dev");
/// Under HOME: the daemon's socket and state, the window's settings.
pub const STATE: Name = Name(".edock", ".edock-dev");
/// The docker context, and the kubectl one.
pub const CONTEXT: Name = Name("edock", "edock-dev");
pub const AGENT: Name = Name("app.edock.daemon", "app.edock.daemon.dev");
pub const APP_ID: Name = Name("app.edock.app", DEV_APP_ID);

/// eDock Dev keeps its images, volumes and VM disk apart: `EDOCK_DATA_DIR`,
/// which `Store::default_root` reads, set before any thread starts.
pub fn init() {
    if dev() && std::env::var_os("EDOCK_DATA_DIR").is_none() {
        if let Some(home) = std::env::var_os("HOME") {
            // SAFETY: called first thing in main, before any other thread.
            unsafe { std::env::set_var("EDOCK_DATA_DIR", std::path::Path::new(&home).join("Library/Application Support").join(APP.get())) };
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_test_binary_is_not_dev() {
        assert!(!super::dev());
        assert_eq!((super::STATE.get(), super::CONTEXT.to_string()), (".edock", "edock".to_owned()));
    }

    #[test]
    fn a_feature_renames_every_dev_name() {
        use super::featured;
        assert_eq!(featured("eDock Dev", "builds"), "eDock Builds");
        assert_eq!(featured(".edock-dev", "builds"), ".edock-builds");
        assert_eq!(featured("edock-dev", "builds"), "edock-builds");
        assert_eq!(featured("edock-dev-build", "builds"), "edock-builds-build");
        assert_eq!(featured("app.edock.daemon.dev", "builds"), "app.edock.daemon.builds");
        assert_eq!(featured("app.edock.app.dev", "builds"), "app.edock.app.builds");
    }
}

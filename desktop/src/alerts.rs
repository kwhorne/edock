//! A note from macOS when a container runs out of memory (the kernel killed
//! it) or turns unhealthy, so a service dying in the background isn't a
//! mystery. A click opens its log. At most one note per container and kind
//! every ten minutes, and none for eDock's own (machines, Kubernetes,
//! helpers). Off with Engine › Overview › Notifications. Either way, the
//! menu bar's popover lists the last hour's, and the menu bar item has a
//! dot until the popover is opened.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use elyra::notifier::{Notifier, Shown};
use elyra::{command, Ctx, EventBus};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::engine::Settings;

/// What happened to a container.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum What {
    OutOfMemory,
    Unhealthy,
}

#[derive(Debug, PartialEq, Clone)]
pub struct Alert {
    pub id: String,
    pub name: String,
    pub what: What,
}

/// The alert a Docker event means, if any.
pub fn from_event(v: &Value) -> Option<Alert> {
    if v["Type"] != "container" {
        return None;
    }
    let what = match v["Action"].as_str()? {
        "oom" => What::OutOfMemory,
        "health_status: unhealthy" => What::Unhealthy,
        _ => return None,
    };
    let attrs = &v["Actor"]["Attributes"];
    let id = v["Actor"]["ID"].as_str()?.to_owned();
    let name = attrs["name"].as_str().map_or_else(|| id.chars().take(12).collect(), str::to_owned);
    // eDock's own aren't the user's: BuildKit's (a big build running out
    // of memory says so in the build), helpers, machines, Kubernetes.
    if crate::resources::system_kind(|k| !attrs[k].is_null(), &name).is_some() {
        return None;
    }
    Some(Alert { id, name, what })
}

/// The last note per container and kind, what happened lately, and whether
/// the popover has been opened since.
#[derive(Default)]
pub struct Alerts {
    last: Mutex<HashMap<(String, u8), Instant>>,
    recent: Mutex<Vec<(Alert, Instant)>>,
    unseen: AtomicBool,
}

const QUIET: Duration = Duration::from_secs(600);
/// How long the popover lists an alert.
const RECENT: Duration = Duration::from_secs(3600);

impl Alerts {
    /// Keep it for the popover (once per container and kind, the latest),
    /// and mark the menu bar item.
    fn record(&self, a: &Alert) {
        let mut recent = self.recent.lock();
        recent.retain(|(r, at)| at.elapsed() < RECENT && !(r.id == a.id && r.what == a.what));
        recent.push((a.clone(), Instant::now()));
        if recent.len() > 20 {
            recent.remove(0);
        }
        self.unseen.store(true, Ordering::SeqCst);
    }

    /// Something happened that the popover hasn't shown yet.
    pub fn unseen(&self) -> bool {
        self.unseen.load(Ordering::SeqCst)
    }

    /// The last hour's, newest first.
    fn list(&self) -> Vec<(Alert, Duration)> {
        let recent = self.recent.lock();
        recent.iter().rev().filter(|(_, at)| at.elapsed() < RECENT).map(|(a, at)| (a.clone(), at.elapsed())).collect()
    }

    /// Whether to tell about it now: not within ten minutes of the last.
    fn due(&self, a: &Alert) -> bool {
        let key = (a.id.clone(), a.what as u8);
        let mut last = self.last.lock();
        if last.get(&key).is_some_and(|t| t.elapsed() < QUIET) {
            return false;
        }
        last.insert(key, Instant::now());
        true
    }
}

/// Shows the note for `a`, unless notes are off or it was said lately. A
/// click shows the window, on the container's log.
pub fn show(alerts: &Alerts, notifier: &Notifier, bus: &Arc<EventBus>, a: Alert) {
    alerts.record(&a);
    if Settings::load().alerts_off || !alerts.due(&a) {
        return;
    }
    let (title, body) = match a.what {
        What::OutOfMemory => (format!("{} ran out of memory", a.name), "The kernel stopped it: it used all the memory it may have. Click for its last log lines."),
        What::Unhealthy => (format!("{} is unhealthy", a.name), "Its health check keeps failing. Click for its log."),
    };
    let bus = bus.clone();
    let path = format!("/containers/{}?tab=logs", a.id);
    let on_click = Box::new(move || {
        let _ = bus.emit("tray", &"open");
        let _ = bus.emit("navigate", &path);
    });
    let _ = notifier.show(Shown { title, body: Some(body.to_owned()) }, Some(on_click));
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct RecentAlert {
    pub id: String,
    pub name: String,
    /// "oom" (the kernel stopped it: out of memory) or "unhealthy".
    pub what: String,
    /// How long ago, in seconds.
    pub ago: u32,
}

/// What happened to containers in the last hour (out of memory, unhealthy),
/// newest first.
#[command(can = "edock.view")]
pub async fn alerts_recent(ctx: Ctx) -> Vec<RecentAlert> {
    ctx.get::<Alerts>()
        .list()
        .into_iter()
        .map(|(a, ago)| RecentAlert { id: a.id, name: a.name, what: if a.what == What::OutOfMemory { "oom" } else { "unhealthy" }.into(), ago: ago.as_secs() as u32 })
        .collect()
}

/// The popover was opened: the menu bar item loses its dot. With `clear`,
/// the list is emptied too.
#[command]
pub async fn alerts_seen(ctx: Ctx, clear: bool) {
    let alerts = ctx.get::<Alerts>();
    if clear {
        alerts.recent.lock().clear();
    }
    if alerts.unseen.swap(false, Ordering::SeqCst) {
        crate::refresh_tray(&ctx, None).await;
    }
}

/// Whether the notes are on.
#[command]
pub async fn alerts_on(_ctx: Ctx) -> bool {
    !Settings::load().alerts_off
}

/// Notes on or off.
#[command]
pub async fn alerts_set(_ctx: Ctx, on: bool) {
    Settings::update(|s| (true, s.alerts_off = !on));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn events_that_mean_an_alert() {
        let ev = |action: &str, attrs: Value| json!({"Type": "container", "Action": action, "Actor": {"ID": "abc123def456789", "Attributes": attrs}});
        assert_eq!(from_event(&ev("oom", json!({"name": "worker"}))), Some(Alert { id: "abc123def456789".into(), name: "worker".into(), what: What::OutOfMemory }));
        assert_eq!(from_event(&ev("health_status: unhealthy", json!({"name": "api"}))).map(|a| a.what), Some(What::Unhealthy));
        assert!(from_event(&ev("health_status: healthy", json!({"name": "api"}))).is_none());
        assert!(from_event(&ev("die", json!({"name": "api", "exitCode": "137"}))).is_none());
        assert!(from_event(&ev("oom", json!({"name": "k", "app.edock.k8s": "1"}))).is_none(), "eDock's own");
        assert!(from_event(&ev("oom", json!({"name": "buildx_buildkit_edock-build0"}))).is_none(), "BuildKit");
        assert!(from_event(&ev("oom", json!({"name": "x", "app.edock.helper": "agent-run"}))).is_none(), "a helper");
        assert!(from_event(&json!({"Type": "image", "Action": "oom"})).is_none());
    }

    #[test]
    fn once_per_ten_minutes() {
        let alerts = Alerts::default();
        let a = || Alert { id: "x".into(), name: "x".into(), what: What::Unhealthy };
        assert!(alerts.due(&a()));
        assert!(!alerts.due(&a()));
        assert!(alerts.due(&Alert { what: What::OutOfMemory, ..a() }), "another kind");
    }

    #[test]
    fn the_last_hours_are_kept_once_each() {
        let alerts = Alerts::default();
        let a = |id: &str, what| Alert { id: id.into(), name: id.into(), what };
        assert!(!alerts.unseen());
        alerts.record(&a("db", What::Unhealthy));
        alerts.record(&a("web", What::OutOfMemory));
        alerts.record(&a("db", What::Unhealthy));
        assert!(alerts.unseen());
        let names: Vec<_> = alerts.list().into_iter().map(|(a, _)| a.name).collect();
        assert_eq!(names, ["db", "web"], "newest first, once each");
        for i in 0..30 {
            alerts.record(&a(&format!("c{i}"), What::OutOfMemory));
        }
        assert_eq!(alerts.list().len(), 20);
    }
}

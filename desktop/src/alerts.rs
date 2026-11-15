//! A note from macOS when a container runs out of memory (the kernel killed
//! it) or turns unhealthy, so a service dying in the background isn't a
//! mystery. A click opens its log. At most one note per container and kind
//! every ten minutes, and none for eDock's own (machines, Kubernetes,
//! helpers). Off with Engine › Overview › Notifications.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use elyra::notifier::{Notifier, Shown};
use elyra::{command, Ctx, EventBus};
use parking_lot::Mutex;
use serde_json::Value;

use crate::engine::Settings;

/// What happened to a container.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum What {
    OutOfMemory,
    Unhealthy,
}

#[derive(Debug, PartialEq)]
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
    if ["app.edock.machine", "app.edock.k8s", "app.edock.helper"].iter().any(|k| !attrs[*k].is_null()) {
        return None;
    }
    let id = v["Actor"]["ID"].as_str()?.to_owned();
    let name = attrs["name"].as_str().map_or_else(|| id.chars().take(12).collect(), str::to_owned);
    Some(Alert { id, name, what })
}

/// The last note per container and kind.
#[derive(Default)]
pub struct Alerts {
    last: Mutex<HashMap<(String, u8), Instant>>,
}

const QUIET: Duration = Duration::from_secs(600);

impl Alerts {
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

/// Whether the notes are on.
#[command]
pub async fn alerts_on(_ctx: Ctx) -> bool {
    !Settings::load().alerts_off
}

/// Notes on or off.
#[command]
pub async fn alerts_set(_ctx: Ctx, on: bool) {
    let mut s = Settings::load();
    s.alerts_off = !on;
    s.save();
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
}

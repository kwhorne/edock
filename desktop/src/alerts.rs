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
    /// Containers keep waiting on memory (`id` "vm"): the VM is nearly
    /// full, or a container's own limit is too low.
    MemoryPressure,
}

/// How short of memory eDock's VM is, from PSI's "full avg10" (the share of
/// the last 10 s in which every task waited on memory, in percent) and how
/// much of the most it may take its containers use. One rule for the Engine
/// page, the popover and the warning.
pub fn pressure(stall: f64, used: f64, ceiling: f64) -> &'static str {
    let share = if ceiling > 0.0 { used / ceiling } else { 0.0 };
    if stall >= 10.0 || share >= 0.95 {
        "high"
    } else if stall >= 2.0 || share >= 0.85 {
        "some"
    } else {
        "none"
    }
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
/// The VM's memory: once in half an hour, while it lasts.
const QUIET_MEMORY: Duration = Duration::from_secs(1800);
/// How long the popover lists an alert.
const RECENT: Duration = Duration::from_secs(3600);

impl Alerts {
    /// Keep it for the popover (once per container and kind, the latest),
    /// and mark the menu bar item when it's news (`due`).
    fn record(&self, a: &Alert, news: bool) {
        let mut recent = self.recent.lock();
        recent.retain(|(r, at)| at.elapsed() < RECENT && !(r.id == a.id && r.what == a.what));
        recent.push((a.clone(), Instant::now()));
        if recent.len() > 20 {
            recent.remove(0);
        }
        if news {
            self.unseen.store(true, Ordering::SeqCst);
        }
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
        let quiet = if a.what == What::MemoryPressure { QUIET_MEMORY } else { QUIET };
        if last.get(&key).is_some_and(|t| t.elapsed() < quiet) {
            return false;
        }
        last.insert(key, Instant::now());
        true
    }
}

/// Shows the note for `a`, unless notes are off or it was said lately. A
/// click shows the window, on the container's log. True when it was news
/// (the menu bar item gets its dot).
pub fn show(alerts: &Alerts, notifier: &Notifier, bus: &Arc<EventBus>, a: Alert) -> bool {
    // The same again within its quiet time is not news: the popover's line
    // is kept fresh, but the menu bar item gets no dot and there's no note.
    let due = alerts.due(&a);
    alerts.record(&a, due);
    if Settings::load().alerts_off || !due {
        return due;
    }
    let (title, body) = match a.what {
        What::OutOfMemory => (format!("{} ran out of memory", a.name), "The kernel stopped it: it used all the memory it may have. Click for its last log lines."),
        What::Unhealthy => (format!("{} is unhealthy", a.name), "Its health check keeps failing. Click for its log."),
        What::MemoryPressure => (
            "Containers are waiting for memory".to_owned(),
            "The kernel may soon stop one. A container's own memory limit may be too low, or the VM too small: the Engine page says which.",
        ),
    };
    let bus = bus.clone();
    let path = if a.what == What::MemoryPressure { "/engine".to_owned() } else { format!("/containers/{}?tab=logs", a.id) };
    let on_click = Box::new(move || {
        let _ = bus.emit("tray", &"open");
        let _ = bus.emit("navigate", &path);
    });
    let _ = notifier.show(Shown { title, body: Some(body.to_owned()) }, Some(on_click));
    true
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct RecentAlert {
    pub id: String,
    pub name: String,
    /// "oom" (the kernel stopped it: out of memory), "unhealthy", or
    /// "memory" (containers kept waiting on it; `id` is "vm").
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
        .map(|(a, ago)| RecentAlert {
            id: a.id,
            name: a.name,
            what: match a.what {
                What::OutOfMemory => "oom",
                What::Unhealthy => "unhealthy",
                What::MemoryPressure => "memory",
            }
            .into(),
            ago: ago.as_secs() as u32,
        })
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
    fn memory_pressure_from_stall_and_use() {
        assert_eq!(pressure(0.0, 1e9, 8e9), "none");
        assert_eq!(pressure(3.0, 1e9, 8e9), "some", "stalled a little");
        assert_eq!(pressure(0.0, 7e9, 8e9), "some", "most of it used");
        assert_eq!(pressure(12.0, 1e9, 8e9), "high", "stalled a lot");
        assert_eq!(pressure(0.0, 7.7e9, 8e9), "high", "nearly all used");
        assert_eq!(pressure(0.0, 0.0, 0.0), "none", "asleep");
    }

    #[test]
    fn memory_is_told_once_in_half_an_hour() {
        let alerts = Alerts::default();
        let vm = Alert { id: "vm".into(), name: "eDock's VM".into(), what: What::MemoryPressure };
        assert!(alerts.due(&vm));
        assert!(!alerts.due(&vm));
        *alerts.last.lock().get_mut(&("vm".to_string(), What::MemoryPressure as u8)).unwrap() -= Duration::from_secs(900);
        assert!(!alerts.due(&vm), "still within half an hour");
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
        alerts.record(&a("db", What::Unhealthy), true);
        alerts.record(&a("web", What::OutOfMemory), true);
        alerts.record(&a("db", What::Unhealthy), false);
        assert!(alerts.unseen());
        let names: Vec<_> = alerts.list().into_iter().map(|(a, _)| a.name).collect();
        assert_eq!(names, ["db", "web"], "newest first, once each");
        for i in 0..30 {
            alerts.record(&a(&format!("c{i}"), What::OutOfMemory), true);
        }
        assert_eq!(alerts.list().len(), 20);
    }

    /// Memory that stays tight: told again each half hour, its line kept
    /// fresh in between, and the dot only when it's told.
    #[test]
    fn lasting_memory_pressure_is_news_each_half_hour() {
        let alerts = Alerts::default();
        let vm = Alert { id: "vm".into(), name: "Containers".into(), what: What::MemoryPressure };
        let seen = |alerts: &Alerts| alerts.unseen.swap(false, Ordering::SeqCst);
        let look = |alerts: &Alerts| {
            let due = alerts.due(&vm);
            alerts.record(&vm, due);
            due
        };
        assert!(look(&alerts) && seen(&alerts));
        assert!(!look(&alerts) && !seen(&alerts), "15 s later: no dot");
        *alerts.recent.lock().last_mut().map(|(_, at)| at).unwrap() -= Duration::from_secs(3000);
        assert!(!look(&alerts));
        assert!(alerts.list()[0].1 < Duration::from_secs(5), "its line kept fresh");
        *alerts.last.lock().get_mut(&("vm".to_string(), What::MemoryPressure as u8)).unwrap() -= Duration::from_secs(1800);
        assert!(look(&alerts) && seen(&alerts), "half an hour on: told again");
    }
}

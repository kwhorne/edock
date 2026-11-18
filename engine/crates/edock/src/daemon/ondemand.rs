//! On demand's own settings: what `edock on-demand` and the window set, by
//! Compose project or container name, in `~/.edock/on-demand.json`. They
//! go before the `app.edock.on-demand` label: a time turns it on (or
//! changes it), `off` turns it off whatever the label says.

use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// `app.edock.on-demand=15m` on a container (on any service of a Compose
/// project, for all of it): parked after that long without traffic, woken
/// by the next connection to its ports or its name.
pub const LABEL: &str = "app.edock.on-demand";

/// What `on-demand.json` holds: a setting per project and per container
/// that isn't in one, `"15m"` or `"off"`.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub projects: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub containers: BTreeMap<String, String>,
}

/// A setting, read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Choice {
    After(Duration),
    Off,
}

impl Settings {
    pub fn project(&self, name: &str) -> Option<Choice> {
        self.projects.get(name).and_then(|v| choice(v))
    }

    pub fn container(&self, name: &str) -> Option<Choice> {
        self.containers.get(name).and_then(|v| choice(v))
    }
}

fn path() -> Option<PathBuf> {
    super::state_dir().ok().map(|d| d.join("on-demand.json"))
}

/// The saved settings; none when there's no file, or it can't be read.
pub fn load() -> Settings {
    let Some(p) = path() else { return Settings::default() };
    match std::fs::read(&p) {
        Ok(b) => serde_json::from_slice(&b).unwrap_or_else(|e| {
            crate::diag::warn!("ignoring unreadable On demand settings", file = p.display(), error = e);
            Settings::default()
        }),
        Err(_) => Settings::default(),
    }
}

/// Writes them whole, or removes the file when there's nothing in it.
pub fn save(s: &Settings) -> io::Result<()> {
    let p = path().ok_or_else(|| io::Error::other("HOME is not set"))?;
    if s.projects.is_empty() && s.containers.is_empty() {
        return match std::fs::remove_file(&p) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(s)?)?;
    std::fs::rename(&tmp, &p)
}

/// `15m`, `off`, and the like.
pub fn choice(v: &str) -> Option<Choice> {
    if v.trim().eq_ignore_ascii_case("off") {
        return Some(Choice::Off);
    }
    parse_idle(v).map(Choice::After)
}

/// `30s`, `15m`, `1h`, or seconds alone; nothing for 0 or anything else.
pub fn parse_idle(v: &str) -> Option<Duration> {
    let v = v.trim();
    let (n, unit) = match v.char_indices().last()? {
        (i, 's') => (&v[..i], 1),
        (i, 'm') => (&v[..i], 60),
        (i, 'h') => (&v[..i], 3600),
        _ => (v, 1),
    };
    let n: u64 = n.parse().ok().filter(|&n| n > 0)?;
    Some(Duration::from_secs(n.checked_mul(unit)?))
}

/// A time as it would be written: `1h`, `15m`, `90s`.
pub fn shown(d: Duration) -> String {
    let s = d.as_secs();
    if s.is_multiple_of(3600) {
        format!("{}h", s / 3600)
    } else if s.is_multiple_of(60) {
        format!("{}m", s / 60)
    } else {
        format!("{s}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_times_read_as_docker_users_write_them() {
        let p = parse_idle;
        assert_eq!(p("30s"), Some(Duration::from_secs(30)));
        assert_eq!(p("15m"), Some(Duration::from_secs(900)));
        assert_eq!(p("1h"), Some(Duration::from_secs(3600)));
        assert_eq!(p(" 45 "), Some(Duration::from_secs(45)));
        for no in ["", "0", "0m", "m", "soon", "-5m", "1.5m", "99999999999999999999h"] {
            assert_eq!(p(no), None, "{no:?}");
        }
        assert_eq!(choice("OFF"), Some(Choice::Off));
        assert_eq!(choice("2m"), Some(Choice::After(Duration::from_secs(120))));
        assert_eq!(choice("never"), None);
    }

    #[test]
    fn times_are_shown_as_written() {
        assert_eq!(shown(Duration::from_secs(3600)), "1h");
        assert_eq!(shown(Duration::from_secs(900)), "15m");
        assert_eq!(shown(Duration::from_secs(90)), "90s");
    }

    #[test]
    fn settings_read_by_name_and_round_trip() {
        let mut s = Settings::default();
        s.projects.insert("shop".into(), "15m".into());
        s.containers.insert("redis".into(), "off".into());
        s.containers.insert("odd".into(), "soon".into());
        assert_eq!(s.project("shop"), Some(Choice::After(Duration::from_secs(900))));
        assert_eq!(s.container("redis"), Some(Choice::Off));
        assert_eq!(s.container("odd"), None, "unreadable: as if not set");
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        assert_eq!(serde_json::to_string(&Settings::default()).unwrap(), "{}");
    }
}

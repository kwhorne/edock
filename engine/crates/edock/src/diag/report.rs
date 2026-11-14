//! `edock diag`: eDock's own log, read or followed, and a local report file.
//!
//! The report is one text file: versions and the system, whether the
//! engine runs, the last lines of every log, recent crash files and macOS's
//! own crash reports for eDock, all through [`Redactor::for_user`].
//!
//!
//! eDock never uploads it anywhere: the report is a local file only.

use std::fmt::Write as _;
use std::io::{self, IsTerminal, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::Value;

use super::redact::Redactor;
use super::Part;
use crate::daemon::time;

const USAGE: &str = "\
usage: edock diag logs [-f] [--engine|--cli] [--errors] [--op ID] [-n N]
       edock diag report [--out PATH]

logs     eDock's own log, oldest first: the engine (--engine) and the
         command line (--cli), both by default.
         --errors keeps warnings and errors; --op ID one window action's
         lines (the id an error's Details show); -n N lines (default 50);
         -f keeps printing new ones.
report   One local file: versions, whether the engine runs, recent log
         lines and crashes, with your home folder and any tokens taken out.
         Saved to the Desktop (or --out PATH). Nothing is sent anywhere.";

/// Lines of each log in a report.
const REPORT_LINES: usize = 300;
/// How much of one log a report or `edock diag logs` reads, from the end.
const TAIL_BYTES: u64 = 512 << 10;
/// How much of each log a report keeps.
const LOG_BUDGET: usize = 120 << 10;
const CRASHES: usize = 5;
const CRASH_BUDGET: usize = 48 << 10;
const MAC_REPORTS: usize = 3;
const MAC_REPORT_BUDGET: usize = 24 << 10;

pub fn run(args: &[String]) -> io::Result<()> {
    match args.first().map(String::as_str) {
        Some("logs") => logs_cli(&args[1..]),
        Some("report") => report_cli(&args[1..]),
        _ => Err(io::Error::other(USAGE)),
    }
}

// ---------------------------------------------------------------- reading logs

fn log_file(state: &Path, part: Part) -> PathBuf {
    state.join("logs").join(format!("{}.log", part.name()))
}

/// The last `max` bytes of a file, from the first whole line.
fn tail(path: &Path, max: u64) -> String {
    let Ok(mut f) = std::fs::File::open(path) else { return String::new() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let from = len.saturating_sub(max);
    if f.seek(SeekFrom::Start(from)).is_err() {
        return String::new();
    }
    let mut buf = Vec::new();
    let _ = f.take(max).read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf).into_owned();
    match (from > 0, text.find('\n')) {
        (true, Some(i)) => text[i + 1..].to_owned(),
        (true, None) => String::new(),
        (false, _) => text,
    }
}

/// Which lines to show: warnings and errors only, one action's only.
#[derive(Debug, Default, Clone, Copy)]
pub struct Filter<'a> {
    pub errors: bool,
    pub op: Option<&'a str>,
}

impl Filter<'_> {
    fn keep(&self, v: &Value) -> bool {
        (!self.errors || matches!(v["level"].as_str(), Some("error" | "warn"))) && self.op.is_none_or(|op| v["fields"]["op"].as_str() == Some(op))
    }
}

/// The last `n` events of these parts' logs (their moved-aside `.1` too)
/// that pass `filter`, oldest first.
pub fn read_lines(state: &Path, parts: &[Part], filter: Filter<'_>, n: usize) -> Vec<Value> {
    let mut all: Vec<Value> = Vec::new();
    for &part in parts {
        let path = log_file(state, part);
        for p in [super::rotated(&path, 1), path] {
            all.extend(tail(&p, TAIL_BYTES).lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter(|v| v.is_object() && filter.keep(v)));
        }
    }
    // RFC 3339 in UTC sorts as text; the sort is stable within a log.
    all.sort_by(|a, b| a["time"].as_str().cmp(&b["time"].as_str()));
    let skip = all.len().saturating_sub(n);
    all.drain(..skip);
    all
}

/// The local time of a log line's UTC one: `2026-10-28 10:41:07.123`.
fn local(t: &str) -> String {
    let secs = time::parse_unix(t);
    if secs == 0 {
        return t.to_owned();
    }
    let ms = t.get(20..23).filter(|m| m.bytes().all(|b| b.is_ascii_digit())).unwrap_or("000");
    let at = secs as libc::time_t;
    // SAFETY: localtime_r fills the tm it is given and keeps no pointer.
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&at, &mut tm).is_null() {
            return t.to_owned();
        }
        tm
    };
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{ms}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec)
}

/// A log line for people, fields by name: `<time> WARN  daemon container exited exit_code=137 id=3f2a`.
/// `utc` keeps the line's own time (reports, read elsewhere); `color` for a terminal.
pub fn render(v: &Value, utc: bool, color: bool) -> String {
    let time = v["time"].as_str().unwrap_or("");
    let level = v["level"].as_str().unwrap_or("?");
    let tag = format!("{:<5}", level.to_ascii_uppercase());
    let tag = match (color, level) {
        (true, "error") => format!("\x1b[31m{tag}\x1b[0m"),
        (true, "warn") => format!("\x1b[33m{tag}\x1b[0m"),
        (true, "debug") => format!("\x1b[2m{tag}\x1b[0m"),
        _ => tag,
    };
    let mut s = format!("{} {tag} {:<6} {}", if utc { time.to_owned() } else { local(time) }, v["part"].as_str().unwrap_or("?"), v["msg"].as_str().unwrap_or(""));
    if let Some(fields) = v["fields"].as_object() {
        for (k, val) in fields {
            let val = val.as_str().map(str::to_owned).unwrap_or_else(|| val.to_string());
            if val.contains(char::is_whitespace) || val.is_empty() {
                let _ = write!(s, " {k}={val:?}");
            } else {
                let _ = write!(s, " {k}={val}");
            }
        }
    }
    s
}

fn logs_cli(args: &[String]) -> io::Result<()> {
    let (mut parts, mut follow, mut errors, mut n, mut op) = (Vec::new(), false, false, 50usize, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-f" | "--follow" => follow = true,
            "--ui" | "--window" => parts.push(Part::Ui),
            "--engine" | "--daemon" => parts.push(Part::Daemon),
            "--cli" => parts.push(Part::Cli),
            "--errors" => errors = true,
            "--op" => op = Some(it.next().ok_or_else(|| io::Error::other("--op needs an id"))?.clone()),
            "-n" => n = it.next().and_then(|v| v.parse().ok()).ok_or_else(|| io::Error::other("-n needs a number"))?,
            s if s.strip_prefix("-n").is_some_and(|v| v.parse::<usize>().is_ok()) => n = s[2..].parse().unwrap_or(n),
            _ => return Err(io::Error::other(USAGE)),
        }
    }
    if parts.is_empty() {
        parts = Part::ALL.to_vec();
    }
    let state = super::state_dir().ok_or_else(|| io::Error::other("HOME is not set"))?;
    let color = io::stdout().is_terminal();
    let mut out = io::stdout().lock();
    let filter = Filter { errors, op: op.as_deref() };
    let lines = read_lines(&state, &parts, filter, n);
    if lines.is_empty() && !follow {
        eprintln!("nothing logged yet in {}", Redactor::home_only().apply(&state.join("logs").display().to_string()));
    }
    for v in &lines {
        writeln!(out, "{}", render(v, false, color))?;
    }
    if !follow {
        return Ok(());
    }
    out.flush()?;
    // From the end of each log as it is now; a log that shrank was moved
    // aside, and is read again from its start.
    let mut at: Vec<(PathBuf, u64, Vec<u8>)> = parts.iter().map(|&p| log_file(&state, p)).map(|p| {
        let len = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
        (p, len, Vec::new())
    }).collect();
    loop {
        std::thread::sleep(Duration::from_millis(400));
        for (path, offset, partial) in &mut at {
            let len = std::fs::metadata(&*path).map(|m| m.len()).unwrap_or(0);
            if len < *offset {
                *offset = 0;
                partial.clear();
            }
            if len == *offset {
                continue;
            }
            let Ok(mut f) = std::fs::File::open(&*path) else { continue };
            f.seek(SeekFrom::Start(*offset))?;
            let mut got = Vec::new();
            f.take(len - *offset).read_to_end(&mut got)?;
            *offset += got.len() as u64;
            partial.extend_from_slice(&got);
            let Some(end) = partial.iter().rposition(|&b| b == b'\n') else { continue };
            let whole: Vec<u8> = partial.drain(..=end).collect();
            for line in String::from_utf8_lossy(&whole).lines() {
                if let Ok(v) = serde_json::from_str::<Value>(line)
                    && filter.keep(&v)
                {
                    writeln!(out, "{}", render(&v, false, color))?;
                }
            }
            out.flush()?;
        }
    }
}

// ---------------------------------------------------------------- the report

/// What the report says beyond the files.
#[derive(Debug)]
pub struct Facts {
    pub engine: String,
    pub created: SystemTime,
    /// The container list, when the person ticked it in the window.
    pub containers: Option<String>,
}

/// The engine as the report describes it, asked without waking the VM.
fn engine_line() -> String {
    if !crate::client::alive() {
        return "not running".into();
    }
    let version = crate::client::get_background("/version").unwrap_or_default();
    let info = crate::client::get_background("/info").unwrap_or_default();
    format!(
        "running (engine {}, VM {}, {} of {} containers running, {} images)",
        version["Version"].as_str().unwrap_or("?"),
        if info["EdockVmAwake"] == true { "awake" } else { "asleep" },
        info["ContainersRunning"].as_u64().unwrap_or(0),
        info["Containers"].as_u64().unwrap_or(0),
        info["Images"].as_u64().unwrap_or(0),
    )
}

/// The engine's containers, one a line: name, image and status. Nothing
/// from inside them. Asked without waking the VM.
fn container_lines() -> String {
    if !crate::client::alive() {
        return "(the engine is not running)\n".into();
    }
    let list = crate::client::get_background("/containers/json?all=1").unwrap_or_default();
    let mut s = String::new();
    for c in list.as_array().into_iter().flatten() {
        let name = c["Names"][0].as_str().unwrap_or("?").trim_start_matches('/');
        let _ = writeln!(s, "{name}  {}  {}", c["Image"].as_str().unwrap_or("?"), c["Status"].as_str().unwrap_or("?"));
    }
    if s.is_empty() {
        s.push_str("(none)\n");
    }
    s
}

/// At most `max` bytes of `text`, from its end, starting at a whole line.
fn last_bytes(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut from = text.len() - max;
    while !text.is_char_boundary(from) {
        from += 1;
    }
    let cut = &text[from..];
    cut.find('\n').map_or(cut, |i| &cut[i + 1..])
}

/// At most `max` bytes of `text`, from its start, marked when cut.
fn first_bytes(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut to = max;
    while !text.is_char_boundary(to) {
        to -= 1;
    }
    format!("{}\n[… {} more bytes]\n", &text[..to], text.len() - to)
}

/// Files in `dir` whose names pass `want`, newest first by modification time.
fn newest(dir: &Path, want: impl Fn(&str) -> bool) -> Vec<(PathBuf, SystemTime)> {
    let mut found: Vec<(PathBuf, SystemTime)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| want(&e.file_name().to_string_lossy()))
        .filter_map(|e| Some((e.path(), e.metadata().ok()?.modified().ok()?)))
        .collect();
    found.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.0.cmp(&a.0)));
    found
}

/// The crash files in `dir`, sent or not, newest first by their time.
fn crash_files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| crash_name(n).is_some())
        .collect();
    names.sort_by_key(|n| std::cmp::Reverse(crash_name(n).map_or(0, |(t, _)| t)));
    names
}

/// `1792800000-daemon.txt` (or `-1.txt`, `.sent.txt`, `.txt.sending`):
/// its time and part.
fn crash_name(name: &str) -> Option<(u64, Part)> {
    let stem = name.strip_suffix(".sending").unwrap_or(name).strip_suffix(".txt")?;
    let stem = stem.strip_suffix(".sent").unwrap_or(stem);
    let mut bits = stem.split('-');
    let secs = bits.next()?.parse().ok()?;
    let part = Part::parse(bits.next()?)?;
    Some((secs, part))
}

/// The report's text, redacted. `home` holds `Library/Logs/DiagnosticReports`;
/// `state` is `~/.edock`.
pub fn assemble(home: &Path, state: &Path, facts: &Facts, redactor: &Redactor) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "eDock diagnostics report");
    let _ = writeln!(s, "Created:  {}", time::rfc3339(facts.created));
    let _ = writeln!(s, "App:      {} {} ({}), made by the {}", crate::flavor::APP, env!("CARGO_PKG_VERSION"), crate::flavor::CONTEXT, super::part().name());
    let _ = writeln!(s, "System:   {}", super::system().line());
    let _ = writeln!(s, "Engine:   {}", facts.engine);

    for (part, title) in [(Part::Daemon, "engine"), (Part::Ui, "window"), (Part::Cli, "command line")] {
        let path = log_file(state, part);
        let lines: Vec<String> = read_lines(state, &[part], Filter::default(), REPORT_LINES).iter().map(|v| render(v, true, false)).collect();
        let _ = writeln!(s, "\n==== Log: {title} ({}) ====", path.display());
        if lines.is_empty() {
            let _ = writeln!(s, "(empty)");
        } else {
            s.push_str(last_bytes(&(lines.join("\n") + "\n"), LOG_BUDGET));
        }
    }

    // The engine's own stderr, as launchd (or the window) keeps it.
    let raw = state.join("daemon.log");
    let _ = writeln!(s, "\n==== Engine output ({}) ====", raw.display());
    let text = tail(&raw, LOG_BUDGET as u64);
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        let _ = writeln!(s, "(empty)");
    } else {
        let _ = writeln!(s, "{}", lines[lines.len().saturating_sub(200)..].join("\n"));
    }

    if let Some(list) = &facts.containers {
        let _ = writeln!(s, "\n==== Containers (names, images and status only) ====");
        s.push_str(list);
    }

    let crashes = state.join("crashes");
    let names = crash_files(&crashes);
    let _ = writeln!(s, "\n==== Crashes ({}, {} in all) ====", crashes.display(), names.len());
    for name in names.iter().take(CRASHES) {
        let text = std::fs::read_to_string(crashes.join(name)).unwrap_or_default();
        let _ = writeln!(s, "--- {name} ---");
        s.push_str(&first_bytes(&text, CRASH_BUDGET));
    }

    // What macOS wrote down itself: crashes our hook can't see (a signal, a
    // hang the system killed, an abort inside a framework).
    let reports = home.join("Library/Logs/DiagnosticReports");
    let month = SystemTime::now() - Duration::from_secs(30 * 86_400);
    let ips: Vec<_> = newest(&reports, |n| n.to_ascii_lowercase().starts_with("edock") && n.ends_with(".ips")).into_iter().filter(|(_, t)| *t >= month).collect();
    let _ = writeln!(s, "\n==== macOS crash reports ({}, last 30 days: {}) ====", reports.display(), ips.len());
    for (path, _) in ips.iter().take(MAC_REPORTS) {
        let text = String::from_utf8_lossy(&std::fs::read(path).unwrap_or_default()).into_owned();
        let _ = writeln!(s, "--- {} ---", path.file_name().unwrap_or_default().to_string_lossy());
        s.push_str(&first_bytes(&text, MAC_REPORT_BUDGET));
        s.push('\n');
    }
    redactor.apply(&s)
}

/// This Mac's report, as `edock diag report` and the window make it; the
/// window may add the container list.
pub fn report_text(containers: bool) -> io::Result<String> {
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| io::Error::other("HOME is not set"))?;
    let state = home.join(crate::flavor::STATE.get());
    let facts = Facts { engine: engine_line(), created: SystemTime::now(), containers: containers.then(container_lines) };
    Ok(assemble(&home, &state, &facts, &Redactor::for_user()))
}

/// `~/Desktop/edock-report-20261028-104107.txt` (the home folder when there
/// is no Desktop).
fn default_out() -> io::Result<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| io::Error::other("HOME is not set"))?;
    let desktop = home.join("Desktop");
    let stamp: String = local(&super::now()).chars().filter(|c| c.is_ascii_digit() || *c == ' ').take(15).collect::<String>().replace(' ', "-");
    Ok(if desktop.is_dir() { desktop } else { home }.join(format!("edock-report-{stamp}.txt")))
}

/// Writes the report, readable only by this user. Returns where.
pub fn save_report(text: &str, out: Option<PathBuf>) -> io::Result<PathBuf> {
    let path = match out {
        Some(p) => p,
        None => default_out()?,
    };
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&path)?;
    f.write_all(text.as_bytes())?;
    Ok(path)
}

/// Shows a file in Finder.
pub fn reveal(path: &Path) {
    if cfg!(target_os = "macos") {
        let _ = std::process::Command::new("/usr/bin/open").arg("-R").arg(path).spawn();
    }
}

fn report_cli(args: &[String]) -> io::Result<()> {
    let mut out = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" | "-o" => out = Some(PathBuf::from(it.next().ok_or_else(|| io::Error::other("--out needs a path"))?)),
            "--send" => return Err(io::Error::other("sending reports is not available in this build: share the saved file yourself")),
            _ => return Err(io::Error::other(USAGE)),
        }
    }
    let text = report_text(false)?;
    let chosen = out.is_some();
    let path = save_report(&text, out)?;
    println!("saved the report to {}", Redactor::home_only().apply(&path.display().to_string()));
    if !chosen {
        reveal(&path);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::Level;
    use super::*;

    fn temp_home(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("edock-report-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn line(time: &str, level: Level, part: Part, msg: &str) -> String {
        super::super::json_line(time, level, part, msg, &[])
    }

    #[test]
    fn reads_the_last_lines_of_several_logs_in_time_order() {
        let home = temp_home("read");
        let state = home.join(".edock");
        std::fs::create_dir_all(state.join("logs")).unwrap();
        let tagged = |t: &str, part: Part, msg: &str| super::super::json_line(t, Level::Info, part, msg, &[("op", "run-7c2e".into())]);
        let daemon = [tagged("2026-10-28T09:00:01.000Z", Part::Daemon, "a"), line("2026-10-28T09:00:03.000Z", Level::Error, Part::Daemon, "c")].join("\n");
        std::fs::write(state.join("logs/daemon.log"), daemon + "\nnot json\n").unwrap();
        std::fs::write(state.join("logs/daemon.log.1"), line("2026-10-28T08:00:00.000Z", Level::Warn, Part::Daemon, "old") + "\n").unwrap();
        std::fs::write(state.join("logs/ui.log"), super::super::json_line("2026-10-28T09:00:02.000Z", Level::Warn, Part::Ui, "b", &[("op", "run-7c2e".into())]) + "\n").unwrap();
        let msgs = |v: Vec<Value>| v.iter().map(|l| l["msg"].as_str().unwrap().to_owned()).collect::<Vec<_>>().join(",");
        assert_eq!(msgs(read_lines(&state, &Part::ALL, Filter::default(), 50)), "old,a,b,c");
        assert_eq!(msgs(read_lines(&state, &Part::ALL, Filter::default(), 2)), "b,c");
        assert_eq!(msgs(read_lines(&state, &Part::ALL, Filter { errors: true, op: None }, 50)), "old,b,c");
        assert_eq!(msgs(read_lines(&state, &[Part::Ui], Filter::default(), 50)), "b");
        assert_eq!(msgs(read_lines(&state, &Part::ALL, Filter { errors: false, op: Some("run-7c2e") }, 50)), "a,b");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn renders_lines_for_people() {
        let v: Value = serde_json::from_str(&super::super::json_line("2026-10-28T09:41:07.123Z", Level::Warn, Part::Daemon, "container exited", &[("id", "3f2a".into()), ("error", "no such file".into())])).unwrap();
        assert_eq!(render(&v, true, false), r#"2026-10-28T09:41:07.123Z WARN  daemon container exited error="no such file" id=3f2a"#);
        assert!(render(&v, false, false).ends_with(".123 WARN  daemon container exited error=\"no such file\" id=3f2a"));
    }

    #[test]
    fn names_crash_files() {
        assert_eq!(crash_name("1792800000-daemon.txt"), Some((1_792_800_000, Part::Daemon)));
        assert_eq!(crash_name("1792800000-ui-1.txt"), Some((1_792_800_000, Part::Ui)));
        assert_eq!(crash_name("1792800000-cli.sent.txt"), Some((1_792_800_000, Part::Cli)));
        assert_eq!(crash_name("1792800000-cli.txt.sending"), Some((1_792_800_000, Part::Cli)));
        assert_eq!(crash_name(".window-opened"), None);
        assert_eq!(crash_name("notes.txt"), None);
    }

    #[test]
    fn assembles_a_redacted_report() {
        let home = temp_home("assemble");
        let state = home.join(".edock");
        let h = home.display().to_string();
        std::fs::create_dir_all(state.join("logs")).unwrap();
        std::fs::create_dir_all(state.join("crashes")).unwrap();
        let reports = home.join("Library/Logs/DiagnosticReports");
        std::fs::create_dir_all(&reports).unwrap();
        let mut log = String::new();
        for i in 0..400 {
            log += &line(&format!("2026-10-28T09:{:02}:{:02}.000Z", i / 60, i % 60), Level::Info, Part::Daemon, &format!("event {i} in {h}/.edock"));
            log.push('\n');
        }
        std::fs::write(state.join("logs/daemon.log"), log).unwrap();
        std::fs::write(state.join("daemon.log"), format!("2026-10-28T09:00:00.000Z edock daemon: ready on unix://{h}/.edock/run/docker.sock\n")).unwrap();
        std::fs::write(state.join("crashes/1792800000-daemon.txt"), "eDock crash report\nMessage:  boom with Bearer abcdefghijklmnop\n").unwrap();
        std::fs::write(state.join("crashes/1792700000-ui.sent.txt"), "eDock crash report\nMessage:  older\n").unwrap();
        std::fs::write(reports.join("edock-2026-10-28-094107.ips"), format!("{{\"app_name\":\"edock\",\"crashReporterKey\":\"ABCD-1234\"}}\n{{\"procPath\":\"{h}/Applications/eDock.app\"}}")).unwrap();
        std::fs::write(reports.join("Safari-2026-10-28-094107.ips"), "not ours").unwrap();
        let facts = Facts { engine: "not running".into(), created: std::time::UNIX_EPOCH + Duration::from_secs(1_792_800_000), containers: Some("web  nginx:alpine  Up 2 hours\n".into()) };
        let text = assemble(&home, &state, &facts, &Redactor::new(Some(h.clone()), Vec::new()));

        assert!(text.starts_with("eDock diagnostics report\nCreated:  2026-10-24T00:00:00"), "{text}");
        assert!(text.contains("Engine:   not running"));
        assert!(text.contains("==== Log: engine (~/.edock/logs/daemon.log) ===="));
        // The last 300 of 400 lines.
        assert!(text.contains("event 399 in ~/.edock") && text.contains("event 100 in") && !text.contains("event 99 in"));
        assert!(text.contains("==== Log: window") && text.contains("(empty)"));
        assert!(text.contains("edock daemon: ready on unix://~/.edock/run/docker.sock"));
        assert!(text.contains("==== Containers (names, images and status only) ====\nweb  nginx:alpine  Up 2 hours\n"));
        assert!(text.contains("2 in all") && text.contains("--- 1792800000-daemon.txt ---") && text.contains("boom with Bearer [redacted]"));
        assert!(text.find("1792800000-daemon").unwrap() < text.find("1792700000-ui").unwrap(), "newest crash first");
        assert!(text.contains("--- edock-2026-10-28-094107.ips ---") && text.contains("~/Applications/eDock.app") && !text.contains("Safari"));
        assert!(!text.contains("ABCD-1234") && !text.contains(&h), "{text}");
        let _ = std::fs::remove_dir_all(&home);
    }
}

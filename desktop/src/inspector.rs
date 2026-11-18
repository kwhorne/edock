//! The HTTP inspector: requests through `*.edock.localhost` and their
//! responses, as eDock's engine records them while it's on
//! (`/edock/http`). The engine keeps the last few hundred in memory.

use elyra::{command, Ctx};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::daemon::{engine_call, engine_request};

fn on_edock(ctx: &Ctx) -> Result<(), String> {
    crate::engine::only_on_edock(ctx, "The HTTP inspector is eDock's engine's: switch to it in the toolbar")
}

async fn ask(method: &str, path: &str, body: Option<&Value>) -> Result<Value, String> {
    match engine_call(method, path, body).await {
        Some((s, v)) if s < 300 => Ok(v),
        Some((404, _)) if path.starts_with("/edock/http/") => Err("That request is no longer kept".into()),
        Some((404, _)) => Err("This eDock engine has no HTTP inspector: press Restart engine on the Engine page".into()),
        Some((_, v)) => Err(v["message"].as_str().unwrap_or("The engine refused").to_owned()),
        None => Err("eDock's engine isn't running".into()),
    }
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or("").to_owned()
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct HttpRow {
    pub seq: u32,
    /// Unix time, seconds.
    pub at: f64,
    /// http or https.
    pub scheme: String,
    /// As the client sent it, with its port.
    pub host: String,
    /// The container that answered.
    pub container: String,
    pub method: String,
    /// The path and query.
    pub url: String,
    /// None until the response's head arrives.
    pub status: Option<u16>,
    pub reason: String,
    /// The response's Content-Type.
    pub content_type: String,
    pub req_size: f64,
    pub res_size: f64,
    /// Milliseconds until the response's head.
    pub wait_ms: Option<f64>,
    /// Milliseconds until the response's end.
    pub ms: Option<f64>,
    pub done: bool,
    /// Why it ended without a whole response.
    pub error: Option<String>,
}

fn row(v: &Value) -> HttpRow {
    HttpRow {
        seq: v["seq"].as_u64().unwrap_or(0) as u32,
        at: v["at"].as_f64().unwrap_or(0.0),
        scheme: s(&v["scheme"]),
        host: s(&v["host"]),
        container: s(&v["container"]),
        method: s(&v["method"]),
        url: s(&v["url"]),
        status: v["status"].as_u64().map(|n| n as u16),
        reason: s(&v["reason"]),
        content_type: s(&v["type"]),
        req_size: v["req_size"].as_f64().unwrap_or(0.0),
        res_size: v["res_size"].as_f64().unwrap_or(0.0),
        wait_ms: v["wait_ms"].as_f64(),
        ms: v["ms"].as_f64(),
        done: v["done"].as_bool().unwrap_or(false),
        error: v["error"].as_str().map(str::to_owned),
    }
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct HttpLog {
    /// Recording.
    pub on: bool,
    /// Ask from this next time, for what changed since.
    pub version: f64,
    /// The oldest kept: rows before it are gone.
    pub first: Option<u32>,
    /// New or changed since the version asked from, oldest first.
    pub items: Vec<HttpRow>,
}

/// HTTP requests to containers through `*.edock.localhost` (and
/// `https://…`), with their status, timing and size, oldest first, while
/// the inspector records. `since` (a version from an earlier answer) gives
/// only what's new or changed since. The quick way to see what a web app in
/// a container answers, and which requests fail.
#[command(can = "edock.view")]
pub async fn http_requests(ctx: Ctx, since: Option<f64>) -> Result<HttpLog, String> {
    on_edock(&ctx)?;
    let v = ask("GET", &format!("/edock/http?since={}", since.unwrap_or(0.0) as u64), None).await?;
    Ok(HttpLog {
        on: v["on"].as_bool().unwrap_or(false),
        version: v["version"].as_f64().unwrap_or(0.0),
        first: v["first"].as_u64().map(|n| n as u32),
        items: v["items"].as_array().map(|a| a.iter().map(row).collect()).unwrap_or_default(),
    })
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct HttpBody {
    /// The body as text (gzip and deflate undone), when it is text.
    pub text: Option<String>,
    pub binary: bool,
    /// Longer than the inspector keeps (32 KB): this is its start.
    pub cut: bool,
}

fn body(v: &Value) -> Option<HttpBody> {
    (!v.is_null()).then(|| HttpBody { text: v["text"].as_str().map(str::to_owned), binary: v["binary"] == true, cut: v["cut"] == true })
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct HttpExchange {
    pub row: HttpRow,
    pub req_headers: Vec<(String, String)>,
    pub res_headers: Vec<(String, String)>,
    pub req_body: Option<HttpBody>,
    pub res_body: Option<HttpBody>,
}

fn headers(v: &Value) -> Vec<(String, String)> {
    v.as_array().map(|a| a.iter().map(|h| (s(&h[0]), s(&h[1]))).collect()).unwrap_or_default()
}

/// One recorded request in full: its headers and the response's, and both
/// bodies (their first 32 KB, as text when they are text).
#[command(can = "edock.view")]
pub async fn http_request(ctx: Ctx, seq: u32) -> Result<HttpExchange, String> {
    on_edock(&ctx)?;
    let v = ask("GET", &format!("/edock/http/{seq}"), None).await?;
    Ok(HttpExchange {
        row: row(&v),
        req_headers: headers(&v["req_headers"]),
        res_headers: headers(&v["res_headers"]),
        req_body: body(&v["req_body"]),
        res_body: body(&v["res_body"]),
    })
}

#[derive(Serialize, Deserialize, specta::Type, Clone, Debug)]
pub struct DuringLine {
    /// The container that wrote it.
    pub container: String,
    /// Its Compose service, in a project.
    pub service: Option<String>,
    pub stderr: bool,
    /// Seconds after the request arrived; negative just before it.
    pub offset: f64,
    pub text: String,
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct DuringLogs {
    /// The window, in Unix seconds: from a moment before the request
    /// arrived to a moment after its response ended.
    pub from: f64,
    pub to: f64,
    /// The containers read: the one that answered, and the rest of its
    /// Compose project.
    pub containers: Vec<String>,
    /// In time order.
    pub lines: Vec<DuringLine>,
    /// More lines than shown (the first ones are kept).
    pub cut: bool,
}

/// How far before the request, and after its end, the window reaches:
/// lines are stamped as they reach the engine, a little after they're written.
const BEFORE: f64 = 0.5;
const AFTER: f64 = 1.0;
const MAX_LINES: usize = 1000;

/// Unix seconds from Docker's log timestamp (`2026-11-17T21:45:22.123456789Z`,
/// always UTC).
fn unix_time(t: &str) -> Option<f64> {
    let t = t.strip_suffix('Z')?;
    let (date, clock) = t.split_once('T')?;
    let mut d = date.split('-').map(|n| n.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let (hms, frac) = clock.split_once('.').unwrap_or((clock, ""));
    let mut c = hms.split(':').map(|n| n.parse::<i64>().ok());
    let (h, min, sec) = (c.next()??, c.next()??, c.next()??);
    // Days since 1970-01-01, by Howard Hinnant's days_from_civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let frac: f64 = if frac.is_empty() { 0.0 } else { format!("0.{frac}").parse().ok()? };
    Some((days * 86_400 + h * 3600 + min * 60 + sec) as f64 + frac)
}

/// What the containers wrote while a request was handled: the one that
/// answered, and the rest of its Compose project, from a moment before
/// the request arrived to a moment after its response ended (or now, while
/// it hasn't). Each line with its offset from the request's start.
#[command(can = "edock.view")]
pub async fn http_request_logs(ctx: Ctx, seq: u32) -> Result<DuringLogs, String> {
    use bollard::query_parameters as q;
    use futures_util::StreamExt;
    on_edock(&ctx)?;
    let r = row(&ask("GET", &format!("/edock/http/{seq}"), None).await?);
    let end = match r.ms {
        Some(ms) if r.done => r.at + ms / 1000.0,
        _ => now(),
    };
    let (from, to) = (r.at - BEFORE, end + AFTER);
    let docker = ctx.get::<crate::engine::Engine>().docker()?;
    // The project's containers, when it's in one; else it alone.
    let mut who: Vec<(String, Option<String>)> = vec![(r.container.clone(), None)];
    if let Ok(info) = docker.inspect_container(&r.container, None::<q::InspectContainerOptions>).await {
        let labels = info.config.and_then(|c| c.labels).unwrap_or_default();
        who[0].1 = labels.get("com.docker.compose.service").cloned();
        if let Some(project) = labels.get("com.docker.compose.project") {
            let mut filters = std::collections::HashMap::new();
            filters.insert("label".to_string(), vec![format!("com.docker.compose.project={project}")]);
            let list = docker.list_containers(Some(q::ListContainersOptionsBuilder::default().all(true).filters(&filters).build())).await.unwrap_or_default();
            for c in list {
                let name = c.names.and_then(|n| n.first().map(|n| n.trim_start_matches('/').to_owned())).unwrap_or_default();
                if !name.is_empty() && name != r.container {
                    who.push((name, c.labels.unwrap_or_default().get("com.docker.compose.service").cloned()));
                }
            }
        }
    }
    let mut lines = Vec::new();
    for (name, service) in &who {
        // Whole seconds for the API; the lines are cut to the window below.
        let opts = q::LogsOptionsBuilder::default().stdout(true).stderr(true).timestamps(true).since(from.floor() as i32).until(to.ceil() as i32 + 1).build();
        let mut logs = docker.logs(name, Some(opts));
        let mut lines_of = Joiner::default();
        while let Some(Ok(chunk)) = logs.next().await {
            let stderr = matches!(chunk, bollard::container::LogOutput::StdErr { .. });
            lines_of.add(stderr, &String::from_utf8_lossy(&chunk.into_bytes()));
        }
        for (stderr, t, text) in lines_of.finish() {
            if t >= from && t <= to {
                lines.push(DuringLine { container: name.clone(), service: service.clone(), stderr, offset: t - r.at, text });
            }
        }
    }
    lines.sort_by(|a, b| a.offset.total_cmp(&b.offset));
    let cut = lines.len() > MAX_LINES;
    lines.truncate(MAX_LINES);
    Ok(DuringLogs { from, to, containers: who.into_iter().map(|(n, _)| n).collect(), lines, cut })
}

/// Log lines from chunks, a stream each: a line can come in pieces (a
/// terminal's output isn't framed, and an engine may split a long line),
/// whose first has the time; a later piece's own time, if it has one, goes.
#[derive(Default)]
struct Joiner {
    open: [Option<(f64, String)>; 2],
    done: Vec<(bool, f64, String)>,
}

impl Joiner {
    fn add(&mut self, stderr: bool, chunk: &str) {
        let i = stderr as usize;
        let mut rest = chunk;
        while !rest.is_empty() {
            let (piece, ended) = match rest.find('\n') {
                Some(n) => (&rest[..n], true),
                None => (rest, false),
            };
            rest = if ended { &rest[piece.len() + 1..] } else { "" };
            let piece = piece.strip_suffix('\r').unwrap_or(piece);
            let (stamp, text) = crate::compose::split_time(piece);
            let at = unix_time(stamp);
            match self.open[i].take() {
                Some((t, mut so_far)) => {
                    so_far.push_str(if at.is_some() { text } else { piece });
                    self.open[i] = Some((t, so_far));
                }
                // A piece without a time and nothing before it: not a line of its own.
                None => match at {
                    Some(t) => self.open[i] = Some((t, text.to_owned())),
                    None => continue,
                },
            }
            if ended {
                if let Some((t, text)) = self.open[i].take() {
                    self.done.push((stderr, t, text));
                }
            }
        }
    }

    fn finish(mut self) -> Vec<(bool, f64, String)> {
        for (i, open) in self.open.iter_mut().enumerate() {
            if let Some((t, text)) = open.take() {
                self.done.push((i == 1, t, text));
            }
        }
        self.done
    }
}

fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

/// Start or stop recording.
#[command]
pub async fn http_record(ctx: Ctx, on: bool) -> Result<(), String> {
    on_edock(&ctx)?;
    ask("POST", "/edock/http", Some(&json!({"on": on}))).await.map(|_| ())
}

/// Forget what was recorded.
#[command]
pub async fn http_clear(ctx: Ctx) -> Result<(), String> {
    on_edock(&ctx)?;
    ask("DELETE", "/edock/http", None).await.map(|_| ())
}

/// Headers a replay leaves out: the connection's own, and the length (the
/// body is sent whole, unchunked).
const HOP: [&str; 6] = ["connection", "keep-alive", "content-length", "transfer-encoding", "upgrade", "proxy-connection"];

/// The request again, as recorded, through the HTTP router; it shows up as
/// a new request. Not when its body was cut, or it was a WebSocket.
#[command]
pub async fn http_replay(ctx: Ctx, seq: u32) -> Result<(), String> {
    on_edock(&ctx)?;
    let v = ask("GET", &format!("/edock/http/{seq}"), None).await?;
    let body = &v["req_body"];
    if body["cut"] == true || body["binary"] == true {
        return Err("This request's body isn't kept whole, so it can't be sent again".into());
    }
    let reqh = headers(&v["req_headers"]);
    if reqh.iter().any(|(k, val)| k.eq_ignore_ascii_case("upgrade") && !val.is_empty()) {
        return Err("A WebSocket can't be replayed".into());
    }
    let encoded = reqh.iter().any(|(k, val)| k.eq_ignore_ascii_case("content-encoding") && !val.eq_ignore_ascii_case("identity"));
    let text = body["text"].as_str().unwrap_or("");
    if encoded && !text.is_empty() {
        return Err("This request's body was compressed; eDock keeps it uncompressed, so it can't be sent again as it was".into());
    }
    let port = match engine_request("GET", "/edock/domains").await {
        Some((200, d)) if d["http"]["serving"] == true => d["http"]["port"].as_u64().unwrap_or(80),
        _ => return Err("The container-domain router isn't serving".into()),
    };
    let mut req = format!("{} {} HTTP/1.1\r\n", s(&v["method"]), s(&v["url"]));
    for (k, val) in &reqh {
        if !HOP.contains(&k.to_ascii_lowercase().as_str()) {
            req.push_str(&format!("{k}: {val}\r\n"));
        }
    }
    req.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len()));
    let fut = async {
        let mut c = tokio::net::TcpStream::connect(("127.0.0.1", port as u16)).await.map_err(|e| e.to_string())?;
        c.write_all(req.as_bytes()).await.map_err(|e| e.to_string())?;
        // Read the answer to its end, so the inspector sees it whole.
        let mut sink = Vec::new();
        let _ = c.read_to_end(&mut sink).await;
        Ok::<_, String>(())
    };
    tokio::time::timeout(std::time::Duration::from_secs(60), fut).await.map_err(|_| "The container took over a minute to answer".to_string())?
}

#[cfg(test)]
mod tests {
    /// A line in pieces is one line, at its first piece's time: a terminal's
    /// chunks cut anywhere, and an engine's pieces each with a time.
    #[test]
    fn a_line_in_pieces_is_one_line() {
        let mut j = super::Joiner::default();
        // Cut mid-line, the rest without a time.
        j.add(false, "2026-11-18T10:00:00.100Z first\n2026-11-18T10:00:00.200Z {\"error\":\"long");
        j.add(false, " and longer\"}\n");
        // Pieces of one line, each with its own time (eDock's long lines).
        j.add(true, "2026-11-18T10:00:00.300Z Traceback: ");
        j.add(true, "2026-11-18T10:00:00.301Z at main.py:3\r\n");
        // A piece with no time and nothing before it isn't a line.
        j.add(false, "stray\n");
        // Unfinished at the end: kept.
        j.add(false, "2026-11-18T10:00:00.400Z last");
        let got: Vec<(bool, String)> = j.finish().into_iter().map(|(e, _, t)| (e, t)).collect();
        assert_eq!(
            got,
            [
                (false, "first".into()),
                (false, "{\"error\":\"long and longer\"}".into()),
                (true, "Traceback: at main.py:3".into()),
                (false, "last".into()),
            ]
        );
    }

    #[test]
    fn docker_timestamps_read_as_unix_seconds() {
        assert_eq!(super::unix_time("1970-01-01T00:00:00Z"), Some(0.0));
        assert_eq!(super::unix_time("2000-03-01T00:00:00Z"), Some(951_868_800.0));
        let t = super::unix_time("2026-11-17T21:45:22.123456789Z").unwrap();
        assert!((t - 1_794_951_922.123_456_789).abs() < 1e-6, "{t}");
        assert_eq!(super::unix_time("2024-02-29T12:00:00.5Z"), Some(1_709_208_000.5));
        assert!(super::unix_time("not a time").is_none());
        assert!(super::unix_time("2026-11-17T21:45:22+01:00").is_none(), "UTC only, as Docker writes them");
    }
}

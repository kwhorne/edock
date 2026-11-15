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

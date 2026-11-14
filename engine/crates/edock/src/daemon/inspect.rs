//! The HTTP inspector: while it's on, the container-domain routers
//! (domains.rs) record each request through `*.edock.localhost` and its
//! response, as the bytes pass: method, URL, headers, status, timing, and
//! the start of each body. Nothing is changed on the way, and nothing is
//! kept on disk: the last few hundred exchanges live in memory, for
//! `GET /edock/http`. Off, it costs nothing.
//!
//! The parser is passive and forgiving: it follows HTTP/1.x framing
//! (Content-Length, chunked, until close, HEAD, 1xx, 204, 304) and stops
//! looking at a connection it can't follow (an upgrade to WebSocket, HTTP/2,
//! garbage), which keeps flowing as before.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

/// How many exchanges are kept.
const KEEP: usize = 300;
/// How much of each body is kept.
const BODY_CAP: usize = 32 * 1024;
/// Largest head parsed; a longer one stops the parser.
const MAX_HEAD: usize = 64 * 1024;

static ON: AtomicBool = AtomicBool::new(false);
static LOG: Mutex<Log> = Mutex::new(Log { next: 1, version: 0, items: VecDeque::new() });

struct Log {
    next: u64,
    /// Goes up with each change, so a reader asks only for what changed.
    version: u64,
    items: VecDeque<Exchange>,
}

impl Log {
    fn get(&mut self, seq: u64) -> Option<&mut Exchange> {
        let first = self.items.front()?.seq;
        let i = usize::try_from(seq.checked_sub(first)?).ok()?;
        self.items.get_mut(i)
    }
}

fn log() -> std::sync::MutexGuard<'static, Log> {
    LOG.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

fn settings_path() -> Option<std::path::PathBuf> {
    super::state_dir().ok().map(|d| d.join("inspector.json"))
}

/// At engine start: on again if it was on.
pub fn load() {
    let on = settings_path()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .is_some_and(|v| v["on"] == true);
    ON.store(on, Ordering::Relaxed);
}

pub fn set_on(on: bool) -> std::io::Result<()> {
    ON.store(on, Ordering::Relaxed);
    let path = settings_path().ok_or_else(|| std::io::Error::other("HOME is not set"))?;
    std::fs::write(path, json!({"on": on}).to_string())
}

pub fn clear() {
    let mut l = log();
    l.items.clear();
    l.version += 1;
}

#[derive(Default)]
struct Exchange {
    seq: u64,
    version: u64,
    /// Unix time, seconds.
    at: f64,
    started: Option<Instant>,
    scheme: &'static str,
    host: String,
    container: String,
    method: String,
    target: String,
    req_headers: Vec<(String, String)>,
    req_body: Vec<u8>,
    req_size: u64,
    status: Option<u16>,
    reason: String,
    res_headers: Vec<(String, String)>,
    res_body: Vec<u8>,
    res_size: u64,
    /// Until the response head.
    wait_ms: Option<f64>,
    /// Until the response's end.
    ms: Option<f64>,
    done: bool,
    /// Why it ended without a whole response.
    error: Option<String>,
}

fn header<'a>(h: &'a [(String, String)], name: &str) -> Option<&'a str> {
    h.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

impl Exchange {
    fn summary(&self) -> Value {
        json!({
            "seq": self.seq,
            "at": self.at,
            "scheme": self.scheme,
            "host": self.host,
            "container": self.container,
            "method": self.method,
            "url": self.target,
            "status": self.status,
            "reason": self.reason,
            "type": header(&self.res_headers, "content-type").unwrap_or(""),
            "req_size": self.req_size,
            "res_size": self.res_size,
            "wait_ms": self.wait_ms,
            "ms": self.ms,
            "done": self.done,
            "error": self.error,
        })
    }

    fn full(&self) -> Value {
        let mut v = self.summary();
        let headers = |h: &[(String, String)]| h.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>();
        v["req_headers"] = headers(&self.req_headers).into();
        v["res_headers"] = headers(&self.res_headers).into();
        v["req_body"] = body(&self.req_body, &self.req_headers, self.req_size);
        v["res_body"] = body(&self.res_body, &self.res_headers, self.res_size);
        v
    }
}

/// A body for the app: text when it reads as text (gzip and deflate
/// undone), else its size and that it's binary.
fn body(raw: &[u8], headers: &[(String, String)], size: u64) -> Value {
    if raw.is_empty() {
        return Value::Null;
    }
    let cut = size > raw.len() as u64;
    let decoded = match header(headers, "content-encoding").map(str::trim) {
        Some(e) if e.eq_ignore_ascii_case("gzip") && !cut => inflate(flate2::read::GzDecoder::new(raw)),
        Some(e) if e.eq_ignore_ascii_case("deflate") && !cut => inflate(flate2::read::ZlibDecoder::new(raw)),
        Some(e) if !e.eq_ignore_ascii_case("identity") => return json!({"binary": true, "encoding": e, "cut": cut}),
        _ => Some(raw.to_vec()),
    };
    let Some(bytes) = decoded else { return json!({"binary": true, "cut": cut}) };
    match String::from_utf8(bytes) {
        Ok(text) => json!({"text": text, "cut": cut}),
        // Cut in the middle of a character: the part before it.
        Err(e) if cut && e.utf8_error().error_len().is_none() => {
            let n = e.utf8_error().valid_up_to();
            json!({"text": String::from_utf8_lossy(&e.into_bytes()[..n]), "cut": true})
        }
        Err(_) => json!({"binary": true, "cut": cut}),
    }
}

fn inflate(r: impl std::io::Read) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::new();
    r.take(4 * BODY_CAP as u64).read_to_end(&mut out).ok()?;
    Some(out)
}

/// Exchanges changed after `since`, oldest first, with the version to ask
/// from next time.
pub fn list(since: u64) -> Value {
    let l = log();
    let items: Vec<Value> = l.items.iter().filter(|e| e.version > since).map(Exchange::summary).collect();
    json!({"on": on(), "version": l.version, "first": l.items.front().map(|e| e.seq), "items": items})
}

pub fn get(seq: u64) -> Option<Value> {
    log().get(seq).map(|e| e.full())
}

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

fn ms(since: Option<Instant>) -> Option<f64> {
    since.map(|s| (s.elapsed().as_secs_f64() * 1000.0 * 10.0).round() / 10.0)
}

/// Changes one exchange, if it's still kept.
fn update(seq: u64, f: impl FnOnce(&mut Exchange)) {
    let mut l = log();
    l.version += 1;
    let v = l.version;
    if let Some(e) = l.get(seq) {
        f(e);
        e.version = v;
    }
}

fn add(mut e: Exchange) -> u64 {
    let mut l = log();
    l.version += 1;
    e.seq = l.next;
    e.version = l.version;
    l.next += 1;
    let seq = e.seq;
    l.items.push_back(e);
    while l.items.len() > KEEP {
        l.items.pop_front();
    }
    seq
}

/// The client's side or the container's.
#[derive(Clone, Copy, PartialEq)]
pub enum Dir {
    Request,
    Response,
}

/// One connection being watched: a parser each way, and the requests
/// waiting for their response, in order.
pub struct Tap {
    inner: Mutex<TapState>,
}

struct TapState {
    scheme: &'static str,
    host: String,
    container: String,
    req: Parser,
    res: Parser,
    /// Requests sent and not yet answered: seq, and whether it was HEAD.
    waiting: VecDeque<(u64, bool)>,
    /// The one each side's body belongs to.
    req_seq: Option<u64>,
    res_seq: Option<u64>,
    /// Stopped following this connection.
    lost: bool,
}

impl Tap {
    /// A tap for a connection to `container` through `host`; `None` while
    /// the inspector is off.
    pub fn new(scheme: &'static str, host: &str, container: &str) -> Option<Arc<Tap>> {
        on().then(|| {
            Arc::new(Tap {
                inner: Mutex::new(TapState {
                    scheme,
                    host: host.to_owned(),
                    container: container.to_owned(),
                    req: Parser::new(Dir::Request),
                    res: Parser::new(Dir::Response),
                    waiting: VecDeque::new(),
                    req_seq: None,
                    res_seq: None,
                    lost: false,
                }),
            })
        })
    }

    /// Bytes that went past, one way.
    pub fn saw(&self, dir: Dir, bytes: &[u8]) {
        let mut st = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        if st.lost {
            return;
        }
        let mut rest = bytes;
        while !rest.is_empty() && !st.lost {
            let parser = if dir == Dir::Request { &mut st.req } else { &mut st.res };
            let (used, events) = parser.feed(rest);
            rest = &rest[used..];
            for event in events {
                if !st.lost {
                    st.handle(dir, event);
                }
            }
        }
    }

    /// That side closed: a body read until close is complete; a request
    /// still waiting gets no answer.
    pub fn closed(&self, dir: Dir) {
        let mut st = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        if dir == Dir::Response {
            if st.res.until_close()
                && let Some(seq) = st.res_seq.take()
            {
                let started = st.started(seq);
                update(seq, |e| {
                    e.done = true;
                    e.ms = ms(started);
                });
            }
            let waiting: Vec<u64> = st.waiting.drain(..).map(|(s, _)| s).collect();
            for seq in waiting.into_iter().chain(st.res_seq.take()) {
                update(seq, |e| {
                    if !e.done {
                        e.done = true;
                        e.error = Some(if e.status.is_some() { "the connection closed mid-response" } else { "no response: the container closed the connection" }.into());
                    }
                });
            }
        }
    }
}

impl Drop for Tap {
    fn drop(&mut self) {
        self.closed(Dir::Response);
    }
}

impl TapState {
    fn started(&self, seq: u64) -> Option<Instant> {
        log().get(seq).and_then(|e| e.started)
    }

    fn handle(&mut self, dir: Dir, event: Event) {
        match (dir, event) {
            (_, Event::Lost) => {
                self.lost = true;
            }
            (Dir::Request, Event::Head { first, headers }) => {
                let mut parts = first.splitn(3, ' ');
                let method = parts.next().unwrap_or("").to_owned();
                let target = parts.next().unwrap_or("").to_owned();
                let host = header(&headers, "host").map_or_else(|| self.host.clone(), str::to_owned);
                let head = method.eq_ignore_ascii_case("HEAD");
                let seq = add(Exchange {
                    at: now(),
                    started: Some(Instant::now()),
                    scheme: self.scheme,
                    host,
                    container: self.container.clone(),
                    method,
                    target,
                    req_headers: headers,
                    ..Default::default()
                });
                self.waiting.push_back((seq, head));
                self.req_seq = Some(seq);
            }
            (Dir::Request, Event::Body(bytes)) => {
                if let Some(seq) = self.req_seq {
                    update(seq, |e| keep(&mut e.req_body, &mut e.req_size, &bytes));
                }
            }
            (Dir::Request, Event::End) => self.req_seq = None,
            (Dir::Response, Event::Head { first, headers }) => {
                let mut parts = first.splitn(3, ' ');
                let _version = parts.next();
                let status: u16 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                let reason = parts.next().unwrap_or("").to_owned();
                // 1xx before the real answer (100 Continue): not the answer.
                if (100..200).contains(&status) && status != 101 {
                    self.res.expect_head();
                    return;
                }
                let Some((seq, head)) = self.waiting.pop_front() else {
                    self.lost = true;
                    return;
                };
                let started = self.started(seq);
                if status == 101 {
                    // An upgrade (WebSocket): what follows isn't HTTP.
                    update(seq, |e| {
                        e.status = Some(101);
                        e.reason = reason;
                        e.res_headers = headers;
                        e.wait_ms = ms(started);
                        e.ms = e.wait_ms;
                        e.done = true;
                    });
                    self.lost = true;
                    return;
                }
                self.res.set_body(&headers, head || status == 204 || status == 304);
                // No body to wait for: it's whole now.
                let whole = self.res.state == State::Head;
                if !whole {
                    self.res_seq = Some(seq);
                }
                update(seq, |e| {
                    e.status = Some(status);
                    e.reason = reason;
                    e.res_headers = headers;
                    e.wait_ms = ms(started);
                    if whole {
                        e.ms = e.wait_ms;
                        e.done = true;
                    }
                });
            }
            (Dir::Response, Event::Body(bytes)) => {
                if let Some(seq) = self.res_seq {
                    update(seq, |e| keep(&mut e.res_body, &mut e.res_size, &bytes));
                }
            }
            (Dir::Response, Event::End) => {
                if let Some(seq) = self.res_seq.take() {
                    let started = self.started(seq);
                    update(seq, |e| {
                        e.done = true;
                        e.ms = ms(started);
                    });
                }
            }
        }
    }
}

fn keep(body: &mut Vec<u8>, size: &mut u64, bytes: &[u8]) {
    *size += bytes.len() as u64;
    let room = BODY_CAP.saturating_sub(body.len());
    body.extend_from_slice(&bytes[..bytes.len().min(room)]);
}

enum Event {
    Head { first: String, headers: Vec<(String, String)> },
    Body(Vec<u8>),
    End,
    /// Not HTTP/1.x as the parser knows it: stop following.
    Lost,
}

#[derive(Debug, PartialEq)]
enum State {
    Head,
    /// Bytes of a Content-Length body left.
    Length(u64),
    /// A chunk-size line, being read.
    ChunkSize,
    /// Bytes of a chunk left, then its CRLF.
    Chunk(u64),
    ChunkEnd(u8),
    /// Trailer lines, until an empty one.
    Trailers,
    /// Everything until the connection closes.
    UntilClose,
}

/// One direction's HTTP/1.x framing, fed in whatever pieces arrive.
struct Parser {
    dir: Dir,
    state: State,
    buf: Vec<u8>,
}

impl Parser {
    fn new(dir: Dir) -> Self {
        Parser { dir, state: State::Head, buf: Vec::new() }
    }

    fn until_close(&self) -> bool {
        self.state == State::UntilClose
    }

    fn expect_head(&mut self) {
        self.state = State::Head;
    }

    /// After a head: how its body is framed. A request's body is set by
    /// `feed` itself; a response's needs to know its request.
    fn set_body(&mut self, headers: &[(String, String)], bodyless: bool) {
        let chunked = header(headers, "transfer-encoding").is_some_and(|t| t.to_ascii_lowercase().contains("chunked"));
        let length = header(headers, "content-length").and_then(|l| l.trim().parse::<u64>().ok());
        self.state = if bodyless {
            State::Head
        } else if chunked {
            State::ChunkSize
        } else if let Some(n) = length {
            if n == 0 { State::Head } else { State::Length(n) }
        } else if self.dir == Dir::Request {
            State::Head
        } else {
            State::UntilClose
        };
    }

    /// Reads from `bytes`: how many it used, and what it found.
    fn feed(&mut self, bytes: &[u8]) -> (usize, Vec<Event>) {
        match self.state {
            State::Head => {
                self.buf.extend_from_slice(bytes);
                // Blank lines between messages are allowed.
                while self.buf.starts_with(b"\r\n") {
                    self.buf.drain(..2);
                }
                let Some(end) = self.buf.windows(4).position(|w| w == b"\r\n\r\n") else {
                    if self.buf.len() > MAX_HEAD {
                        return (bytes.len(), vec![Event::Lost]);
                    }
                    return (bytes.len(), Vec::new());
                };
                // The head ends in these bytes: what follows it is theirs.
                let used = bytes.len() - (self.buf.len() - (end + 4));
                let head = String::from_utf8_lossy(&self.buf[..end]).into_owned();
                self.buf.clear();
                let mut lines = head.split("\r\n");
                let first = lines.next().unwrap_or("").to_owned();
                let http = if self.dir == Dir::Request { first.rsplit(' ').next().is_some_and(|v| v.starts_with("HTTP/1.")) } else { first.starts_with("HTTP/1.") };
                if !http {
                    return (bytes.len(), vec![Event::Lost]);
                }
                let headers: Vec<(String, String)> = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned())).collect();
                if self.dir == Dir::Request {
                    self.set_body(&headers, false);
                    let bodyless = self.state == State::Head;
                    let mut events = vec![Event::Head { first, headers }];
                    if bodyless {
                        events.push(Event::End);
                    }
                    return (used, events);
                }
                // A response's framing is set by the tap, once it knows the request.
                (used, vec![Event::Head { first, headers }])
            }
            State::Length(left) => {
                let n = (left.min(bytes.len() as u64)) as usize;
                let left = left - n as u64;
                let mut events = vec![Event::Body(bytes[..n].to_vec())];
                if left == 0 {
                    self.state = State::Head;
                    events.push(Event::End);
                } else {
                    self.state = State::Length(left);
                }
                (n, events)
            }
            State::UntilClose => (bytes.len(), vec![Event::Body(bytes.to_vec())]),
            State::ChunkSize | State::Trailers => {
                let Some(nl) = bytes.iter().position(|&b| b == b'\n') else {
                    self.buf.extend_from_slice(bytes);
                    if self.buf.len() > 4096 {
                        return (bytes.len(), vec![Event::Lost]);
                    }
                    return (bytes.len(), Vec::new());
                };
                self.buf.extend_from_slice(&bytes[..nl]);
                let line = String::from_utf8_lossy(&self.buf).trim().to_owned();
                self.buf.clear();
                if self.state == State::Trailers {
                    if line.is_empty() {
                        self.state = State::Head;
                        return (nl + 1, vec![Event::End]);
                    }
                    return (nl + 1, Vec::new());
                }
                let size = line.split(';').next().unwrap_or("").trim();
                match u64::from_str_radix(size, 16) {
                    Ok(0) => self.state = State::Trailers,
                    Ok(n) => self.state = State::Chunk(n),
                    Err(_) => return (nl + 1, vec![Event::Lost]),
                }
                (nl + 1, Vec::new())
            }
            State::Chunk(left) => {
                let n = (left.min(bytes.len() as u64)) as usize;
                let left = left - n as u64;
                self.state = if left == 0 { State::ChunkEnd(2) } else { State::Chunk(left) };
                (n, vec![Event::Body(bytes[..n].to_vec())])
            }
            State::ChunkEnd(left) => {
                let n = (left as usize).min(bytes.len());
                self.state = if n == left as usize { State::ChunkSize } else { State::ChunkEnd(left - n as u8) };
                (n, Vec::new())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tap fed in pieces of `step` bytes; the exchanges it recorded.
    fn run(container: &str, req: &[u8], res: &[u8], step: usize) -> Vec<Value> {
        ON.store(true, Ordering::Relaxed);
        let tap = Tap::new("http", "x.edock.localhost", container).unwrap();
        for c in req.chunks(step) {
            tap.saw(Dir::Request, c);
        }
        for c in res.chunks(step) {
            tap.saw(Dir::Response, c);
        }
        drop(tap);
        log().items.iter().filter(|e| e.container == container).map(Exchange::full).collect()
    }

    fn req(lines: &str) -> Vec<u8> {
        lines.replace('\n', "\r\n").into_bytes()
    }

    #[test]
    fn keep_alive_requests_pair_with_their_responses() {
        for step in [1, 3, 7, 4096] {
            let r = req("GET /a HTTP/1.1\nHost: x\n\nPOST /b?x=1 HTTP/1.1\nHost: x\nContent-Length: 5\n\nhello");
            let s = req("HTTP/1.1 200 OK\nContent-Type: text/plain\nContent-Length: 2\n\nhiHTTP/1.1 201 Created\nTransfer-Encoding: chunked\n\n3;x\nabc\n2\nde\n0\nX-T: 1\n\n");
            let got = run(&format!("ka{step}"), &r, &s, step);
            assert_eq!(got.len(), 2, "step {step}");
            assert_eq!((got[0]["method"].as_str(), got[0]["url"].as_str(), got[0]["status"].as_u64()), (Some("GET"), Some("/a"), Some(200)));
            assert_eq!(got[0]["res_body"]["text"], "hi");
            assert_eq!(got[1]["req_body"]["text"], "hello");
            assert_eq!((got[1]["status"].as_u64(), got[1]["res_body"]["text"].as_str()), (Some(201), Some("abcde")), "step {step}");
            assert!(got.iter().all(|e| e["done"] == true && e["error"].is_null()), "step {step}: {got:?}");
        }
    }

    #[test]
    fn head_304_and_until_close() {
        let host = "hc.edock.localhost";
        let r = req(&format!("HEAD / HTTP/1.1\nHost: {host}\n\nGET /x HTTP/1.1\nHost: {host}\n\nGET /y HTTP/1.0\nHost: {host}\n\n"));
        let s = req("HTTP/1.1 200 OK\nContent-Length: 999\n\nHTTP/1.1 304 Not Modified\n\nHTTP/1.0 200 OK\n\nuntil the end");
        ON.store(true, Ordering::Relaxed);
        let tap = Tap::new("http", host, "hc").unwrap();
        tap.saw(Dir::Request, &r);
        tap.saw(Dir::Response, &s);
        tap.closed(Dir::Response);
        drop(tap);
        let l = log();
        let got: Vec<Value> = l.items.iter().filter(|e| e.container == "hc").map(Exchange::full).collect();
        assert_eq!(got.len(), 3);
        assert_eq!(got[0]["status"], 200);
        assert_eq!(got[1]["status"], 304);
        assert_eq!(got[2]["res_body"]["text"], "until the end");
        assert!(got.iter().all(|e| e["done"] == true && e["error"].is_null()), "{got:?}");
    }

    #[test]
    fn a_websocket_upgrade_stops_the_parser() {
        let host = "ws.edock.localhost";
        let r = req(&format!("GET /ws HTTP/1.1\nHost: {host}\nUpgrade: websocket\n\n"));
        let mut s = req("HTTP/1.1 101 Switching Protocols\nUpgrade: websocket\n\n");
        s.extend_from_slice(&[0x81, 0x02, b'h', b'i']);
        ON.store(true, Ordering::Relaxed);
        let tap = Tap::new("http", host, "ws").unwrap();
        tap.saw(Dir::Request, &r);
        tap.saw(Dir::Response, &s);
        tap.saw(Dir::Request, b"\x81\x82garbage");
        drop(tap);
        let l = log();
        let got: Vec<Value> = l.items.iter().filter(|e| e.container == "ws").map(Exchange::full).collect();
        assert_eq!(got.len(), 1);
        assert_eq!((got[0]["status"].as_u64(), got[0]["done"].as_bool()), (Some(101), Some(true)));
    }

    #[test]
    fn gzip_bodies_are_shown_as_text() {
        use std::io::Write;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(b"{\"ok\":true}").unwrap();
        let gz = gz.finish().unwrap();
        let headers = vec![("Content-Encoding".to_owned(), "gzip".to_owned())];
        assert_eq!(body(&gz, &headers, gz.len() as u64)["text"], "{\"ok\":true}");
        assert_eq!(body(&[0xff, 0xfe, 0x00], &[], 3)["binary"], true);
        // Cut inside a character: the text before it.
        assert_eq!(body("aø".as_bytes().get(..2).unwrap(), &[], 3)["text"], "a");
    }

    #[test]
    fn no_response_is_an_error() {
        let host = "nr.edock.localhost";
        ON.store(true, Ordering::Relaxed);
        let tap = Tap::new("http", host, "nr").unwrap();
        tap.saw(Dir::Request, &req(&format!("GET / HTTP/1.1\nHost: {host}\n\n")));
        drop(tap);
        let l = log();
        let e = l.items.iter().find(|e| e.container == "nr").unwrap();
        assert!(e.done && e.error.is_some());
    }
}

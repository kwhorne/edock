//! A container's files, for the Files tab: browse folders, preview a file,
//! save one (or a folder, as a .tar) to the Mac, put files from the Mac in,
//! and remove one.
//!
//! On eDock's engine a running container's folders come from the guest
//! directly (`/edock/containers/{id}/files`, as the Finder view reads
//! them), so it needs nothing in the image. On other engines, `find` and
//! `stat` run in the container. Reading and writing files goes through the
//! Docker archive API (`docker cp`), which every engine has, running or not.

use std::path::{Path, PathBuf};

use bollard::query_parameters as q;
use bytes::Bytes;
use elyra::files::ChosenFile;
use elyra::{command, Ctx, EventBus};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use crate::engine::{Engine, EDOCK};

/// How much of a file a preview shows.
const PREVIEW: usize = 256 * 1024;
/// The largest long-name or pax header read (they're a path and a few keys).
const MAX_EXTRA_HEADER: u64 = 1024 * 1024;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// A path in the container, as the tab sends it: absolute, without `..`,
/// NUL or newlines.
fn valid_path(path: &str) -> Result<&str, String> {
    if path.starts_with('/') && !path.contains(['\0', '\n']) && !path.split('/').any(|p| p == "..") {
        Ok(path)
    } else {
        Err(format!("{path:?} isn't a path in the container"))
    }
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-_.~/".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

#[derive(Serialize, Deserialize, specta::Type, Debug, PartialEq)]
pub struct FileEntry {
    pub name: String,
    /// dir, file, link or other.
    pub kind: String,
    /// Permission bits, 0o755 and the like.
    pub mode: u32,
    pub size: f64,
    /// Unix time, seconds.
    pub mtime: f64,
}

fn kind(mode: u32) -> &'static str {
    match mode & 0o170000 {
        0o040000 => "dir",
        0o120000 => "link",
        0o100000 => "file",
        _ => "other",
    }
}

/// `stat -c '%f|%s|%Y|%n'` lines: mode in hex, size, mtime, path.
fn parse_stat(out: &str) -> Vec<FileEntry> {
    let mut v: Vec<FileEntry> = out
        .lines()
        .filter_map(|l| {
            let mut p = l.splitn(4, '|');
            let mode = u32::from_str_radix(p.next()?, 16).ok()?;
            let size = p.next()?.parse::<f64>().ok()?;
            let mtime = p.next()?.parse::<f64>().ok()?;
            let name = p.next()?.rsplit('/').next()?.to_owned();
            Some(FileEntry { name, kind: kind(mode).into(), mode: mode & 0o7777, size, mtime })
        })
        .collect();
    v.sort_by(|a, b| (b.kind == "dir").cmp(&(a.kind == "dir")).then_with(|| a.name.cmp(&b.name)));
    v
}

/// A command in the container, without a shell: its stdout, or its stderr
/// as the error.
async fn exec(ctx: &Ctx, container: &str, cmd: Vec<String>) -> Result<String, String> {
    use bollard::container::LogOutput;
    use bollard::exec::{CreateExecOptions, StartExecResults};
    let docker = ctx.get::<Engine>().docker()?;
    let ex = docker
        .create_exec(container, CreateExecOptions { attach_stdout: Some(true), attach_stderr: Some(true), cmd: Some(cmd), ..Default::default() })
        .await
        .map_err(err)?;
    let (mut out, mut errs) = (String::new(), String::new());
    if let StartExecResults::Attached { output: mut stream, .. } = docker.start_exec(&ex.id, None).await.map_err(err)? {
        while let Some(Ok(chunk)) = stream.next().await {
            match chunk {
                LogOutput::StdErr { message } => errs.push_str(&String::from_utf8_lossy(&message)),
                other => out.push_str(&String::from_utf8_lossy(&other.into_bytes())),
            }
        }
    }
    let code = docker.inspect_exec(&ex.id).await.ok().and_then(|i| i.exit_code).unwrap_or(-1);
    if code != 0 && out.trim().is_empty() {
        let e = errs.trim();
        return Err(if e.contains("executable file not found") || e.contains("no such file or directory") && e.contains("exec") {
            "This image has no find and stat to list folders with".into()
        } else if e.is_empty() {
            format!("exit {code}")
        } else {
            e.lines().last().unwrap_or(e).to_owned()
        });
    }
    Ok(out)
}

/// A folder's entries in a container: folders first, then by name. The
/// container must be running.
#[command(can = "edock.view")]
pub async fn container_files(ctx: Ctx, container: String, path: String) -> Result<Vec<FileEntry>, String> {
    let path = valid_path(&path)?;
    if ctx.get::<Engine>().context().0 == EDOCK {
        // The engine reads the image's layers: at or under a mount (a
        // volume, a bind mount, a tmpfs) it would show the folder under it.
        // There, the container's own `find` looks, when it has one.
        if under_a_mount(&ctx, &container, path).await {
            if let Ok(list) = by_find(&ctx, &container, path).await {
                return Ok(list);
            }
        }
        let from_engine = match crate::daemon::engine_request("GET", &format!("/edock/containers/{}/files?path={}", enc(&container), enc(path))).await {
            Some((200, v)) => serde_json::from_value::<Vec<serde_json::Value>>(v).map_err(err).map(|list| {
                list.iter()
                    .map(|e| FileEntry {
                        name: e["name"].as_str().unwrap_or("").into(),
                        kind: e["kind"].as_str().unwrap_or("other").into(),
                        mode: e["mode"].as_u64().unwrap_or(0) as u32,
                        size: e["size"].as_f64().unwrap_or(0.0),
                        mtime: e["mtime"].as_f64().unwrap_or(0.0),
                    })
                    .collect()
            }),
            Some((409, _)) => Err("Start the container to browse its files".into()),
            Some((404, _)) => Err("This eDock engine can't list a container's files: press Restart engine on the Engine page".into()),
            Some((_, v)) => Err(v["message"].as_str().map(|m| m.split(": open: ").last().unwrap_or(m).to_owned()).unwrap_or_else(|| "The engine refused".into())),
            None => Err("eDock's engine isn't running".into()),
        };
        // The engine reads the container's own files: a volume or bind
        // mounted in it (a database's data) isn't there, so `find` looks.
        return match from_engine {
            Ok(list) => Ok(list),
            Err(e) if e.starts_with("Start the container") || e.contains("Restart engine") || e.contains("isn't running") => Err(e),
            Err(e) => by_find(&ctx, &container, path).await.map_err(|_| e),
        };
    }
    let running = ctx.get::<Engine>().docker()?.inspect_container(&container, None::<q::InspectContainerOptions>).await.map_err(err)?;
    if running.state.and_then(|s| s.running) != Some(true) {
        return Err("Start the container to browse its files".into());
    }
    by_find(&ctx, &container, path).await
}

/// Whether `path` is a mount point in the container, or under one.
async fn under_a_mount(ctx: &Ctx, container: &str, path: &str) -> bool {
    let Ok(docker) = ctx.get::<Engine>().docker() else { return false };
    let Ok(info) = docker.inspect_container(container, None::<q::InspectContainerOptions>).await else { return false };
    let path = path.trim_end_matches('/');
    info.mounts.unwrap_or_default().iter().filter_map(|m| m.destination.as_deref()).chain(
        info.host_config.as_ref().and_then(|h| h.tmpfs.as_ref()).into_iter().flat_map(|t| t.keys().map(String::as_str)),
    ).any(|d| {
        let d = d.trim_end_matches('/');
        !d.is_empty() && (path == d || path.starts_with(&format!("{d}/")))
    })
}

/// A folder's entries from `find` and `stat` in the running container.
async fn by_find(ctx: &Ctx, container: &str, path: &str) -> Result<Vec<FileEntry>, String> {
    let cmd = ["find", path, "-mindepth", "1", "-maxdepth", "1", "-exec", "stat", "-c", "%f|%s|%Y|%n", "{}", "+"];
    Ok(parse_stat(&exec(ctx, container, cmd.iter().map(|s| (*s).to_owned()).collect()).await?))
}

/// One tar entry's header, as much as the tab needs.
struct Header {
    name: String,
    /// b'0' a file, b'5' a folder, b'2' a link…
    kind: u8,
    size: u64,
    link: String,
}

fn octal(b: &[u8]) -> u64 {
    // GNU's base-256 for large sizes.
    if b.first().is_some_and(|c| c & 0x80 != 0) {
        return b[1..].iter().fold(0u64, |n, c| n << 8 | u64::from(*c));
    }
    let s = String::from_utf8_lossy(b);
    u64::from_str_radix(s.trim_matches(|c: char| c == '\0' || c == ' '), 8).unwrap_or(0)
}

fn cstr(b: &[u8]) -> String {
    String::from_utf8_lossy(&b[..b.iter().position(|c| *c == 0).unwrap_or(b.len())]).into_owned()
}

/// Reads a tar stream piece by piece.
struct TarReader<S> {
    stream: S,
    buf: Vec<u8>,
    /// Every byte read, while a folder's raw archive is wanted.
    keep: Option<Vec<u8>>,
}

impl<S: futures_util::Stream<Item = Result<Bytes, bollard::errors::Error>> + Unpin> TarReader<S> {
    /// At least `n` bytes buffered, unless the stream ends first.
    async fn fill(&mut self, n: usize) -> Result<(), String> {
        while self.buf.len() < n {
            match self.stream.next().await {
                Some(Ok(b)) => {
                    if let Some(k) = &mut self.keep {
                        k.extend_from_slice(&b);
                    }
                    self.buf.extend_from_slice(&b);
                }
                Some(Err(e)) => return Err(e.to_string().replace("Docker responded with status code 404: ", "")),
                None => break,
            }
        }
        Ok(())
    }

    fn take(&mut self, n: usize) -> Vec<u8> {
        let n = n.min(self.buf.len());
        self.buf.drain(..n).collect()
    }

    /// The next real entry's header, past GNU long names and pax headers.
    async fn header(&mut self) -> Result<Header, String> {
        let mut long_name = None;
        loop {
            self.fill(512).await?;
            if self.buf.len() < 512 {
                return Err("The archive ended early".into());
            }
            let h = self.take(512);
            let size = octal(&h[124..136]);
            let padded = size.div_ceil(512) * 512;
            match h[156] {
                // A long name or pax header is small: a huge one isn't read into memory.
                b'L' | b'x' | b'g' if padded > MAX_EXTRA_HEADER => return Err("The archive has a header too big to read".into()),
                b'L' | b'x' | b'g' => {
                    self.fill(padded as usize).await?;
                    let data = self.take(padded as usize);
                    let data = &data[..(size as usize).min(data.len())];
                    if h[156] == b'L' {
                        long_name = Some(cstr(data));
                    } else if let Some(p) = String::from_utf8_lossy(data).lines().find_map(|l| l.split_once(" path=").map(|(_, p)| p.to_owned())) {
                        long_name = Some(p);
                    }
                }
                kind => {
                    let prefix = cstr(&h[345..500]);
                    let name = long_name.unwrap_or_else(|| if prefix.is_empty() { cstr(&h[..100]) } else { format!("{prefix}/{}", cstr(&h[..100])) });
                    return Ok(Header { name, kind: if kind == 0 { b'0' } else { kind }, size, link: cstr(&h[157..257]) });
                }
            }
        }
    }
}

fn archive(ctx: &Ctx, container: &str, path: &str) -> Result<impl futures_util::Stream<Item = Result<Bytes, bollard::errors::Error>> + Unpin, String> {
    let docker = ctx.get::<Engine>().docker()?;
    let opts = q::DownloadFromContainerOptionsBuilder::default().path(path).build();
    Ok(Box::pin(docker.download_from_container(container, Some(opts))))
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct FilePreview {
    /// The start of the file, when it reads as text.
    pub text: Option<String>,
    pub binary: bool,
    pub size: f64,
    /// Longer than the preview: this is its start.
    pub cut: bool,
    /// Where a link points.
    pub link: Option<String>,
}

/// Text that reads as text: UTF-8 (a character cut at the end is fine),
/// no NULs.
fn as_text(bytes: &[u8], cut: bool) -> Option<String> {
    if bytes.iter().take(8192).any(|b| *b == 0) {
        return None;
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => Some(s.to_owned()),
        Err(e) if cut && e.error_len().is_none() => Some(String::from_utf8_lossy(&bytes[..e.valid_up_to()]).into_owned()),
        Err(_) => None,
    }
}

/// The start of a file in a container (256 KB), as text when it is text.
/// Works on stopped containers too.
#[command(can = "edock.view")]
pub async fn container_file_preview(ctx: Ctx, container: String, path: String) -> Result<FilePreview, String> {
    let path = valid_path(&path)?;
    let mut tar = TarReader { stream: archive(&ctx, &container, path)?, buf: Vec::new(), keep: None };
    let h = tar.header().await?;
    match h.kind {
        b'2' => return Ok(FilePreview { text: None, binary: false, size: 0.0, cut: false, link: Some(h.link) }),
        b'5' => return Err("That's a folder".into()),
        b'0' | b'7' => {}
        _ => return Err("That's not a regular file".into()),
    }
    let want = (h.size as usize).min(PREVIEW);
    tar.fill(want).await?;
    let bytes = tar.take(want);
    let cut = h.size as usize > PREVIEW;
    let text = as_text(&bytes, cut);
    Ok(FilePreview { binary: text.is_none(), text, size: h.size as f64, cut, link: None })
}

/// Save a file from a container where the user chose; a folder is saved
/// as a .tar of it. Works on stopped containers too.
#[command]
pub async fn container_file_save(ctx: Ctx, container: String, path: String, file: ChosenFile) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    let path = valid_path(&path)?;
    let mut tar = TarReader { stream: archive(&ctx, &container, path)?, buf: Vec::new(), keep: Some(Vec::new()) };
    let h = tar.header().await?;
    let mut out = tokio::fs::File::create(file.path()).await.map_err(|e| format!("{}: {e}", file.name()))?;
    if h.kind == b'5' {
        // A folder: the archive as it comes.
        let so_far = tar.keep.take().unwrap_or_default();
        out.write_all(&so_far).await.map_err(err)?;
        while let Some(b) = tar.stream.next().await {
            out.write_all(&b.map_err(err)?).await.map_err(err)?;
        }
    } else {
        if !matches!(h.kind, b'0' | b'7') {
            return Err(format!("{} isn't a regular file", h.name));
        }
        tar.keep = None;
        let mut left = h.size as usize;
        while left > 0 {
            tar.fill(1.max(left.min(1 << 20))).await?;
            if tar.buf.is_empty() {
                return Err("The archive ended early".into());
            }
            let chunk = tar.take(left.min(tar.buf.len()));
            left -= chunk.len();
            out.write_all(&chunk).await.map_err(err)?;
        }
    }
    out.flush().await.map_err(err)
}

/// A tar header for `name` (GNU long-name entry first when it's long).
fn tar_header(name: &str, kind: u8, size: u64, mode: u32, mtime: u64) -> Vec<u8> {
    let mut out = Vec::new();
    let raw = |name: &[u8], kind: u8, size: u64| {
        let mut h = [0u8; 512];
        h[..name.len().min(100)].copy_from_slice(&name[..name.len().min(100)]);
        let field = |h: &mut [u8; 512], at: usize, len: usize, v: u64| {
            let s = format!("{v:0w$o}\0", w = len - 1);
            h[at..at + len].copy_from_slice(s.as_bytes());
        };
        field(&mut h, 100, 8, u64::from(mode & 0o7777));
        field(&mut h, 108, 8, 0);
        field(&mut h, 116, 8, 0);
        field(&mut h, 124, 12, size);
        field(&mut h, 136, 12, mtime);
        h[156] = kind;
        h[257..263].copy_from_slice(b"ustar\0");
        h[263..265].copy_from_slice(b"00");
        h[148..156].copy_from_slice(b"        ");
        let sum: u32 = h.iter().map(|b| u32::from(*b)).sum();
        h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        h
    };
    if name.len() > 100 {
        let long = format!("{name}\0");
        out.extend_from_slice(&raw(b"././@LongLink", b'L', long.len() as u64));
        out.extend_from_slice(long.as_bytes());
        out.resize(out.len().div_ceil(512) * 512, 0);
    }
    out.extend_from_slice(&raw(name.as_bytes(), kind, size));
    out
}

/// The files and folders to send, each with its name in the archive.
fn walk(root: &Path, name: String, out: &mut Vec<(PathBuf, String)>) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(root)?;
    if meta.is_dir() {
        out.push((root.to_owned(), name.clone()));
        let mut entries: Vec<_> = std::fs::read_dir(root)?.filter_map(Result::ok).collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            walk(&e.path(), format!("{name}/{}", e.file_name().to_string_lossy()), out)?;
        }
    } else if meta.is_file() {
        out.push((root.to_owned(), name));
    }
    Ok(())
}

/// Put files (and folders) from the Mac into a folder of a container. Its
/// owner is root, as with `docker cp`. Returns how many files went in.
#[command]
pub async fn container_files_upload(ctx: Ctx, container: String, dir: String, files: Vec<ChosenFile>) -> Result<u32, String> {
    use std::os::unix::fs::MetadataExt;
    let dir = valid_path(&dir)?.to_owned();
    let mut items = Vec::new();
    for f in &files {
        let name = f.path().file_name().map(|n| n.to_string_lossy().into_owned()).ok_or("A file without a name")?;
        walk(f.path(), name, &mut items).map_err(|e| format!("{}: {e}", f.name()))?;
    }
    let count = items.iter().filter(|(p, _)| p.is_file()).count() as u32;
    let (tx, rx) = tokio::sync::mpsc::channel::<Bytes>(8);
    // The archive is made as it's sent, so a big file isn't held in memory.
    let writer = tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        for (path, name) in items {
            let meta = std::fs::metadata(&path)?;
            let mtime = meta.mtime().max(0) as u64;
            if meta.is_dir() {
                tx.send(Bytes::from(tar_header(&format!("{name}/"), b'5', 0, meta.mode(), mtime))).await.map_err(std::io::Error::other)?;
                continue;
            }
            let size = meta.len();
            tx.send(Bytes::from(tar_header(&name, b'0', size, meta.mode(), mtime))).await.map_err(std::io::Error::other)?;
            let mut f = tokio::fs::File::open(&path).await?;
            let mut sent = 0u64;
            let mut buf = vec![0u8; 256 * 1024];
            while sent < size {
                let n = f.read(&mut buf).await?;
                if n == 0 {
                    return Err(std::io::Error::other(format!("{} got shorter while it was sent", path.display())));
                }
                let n = n.min((size - sent) as usize);
                sent += n as u64;
                tx.send(Bytes::copy_from_slice(&buf[..n])).await.map_err(std::io::Error::other)?;
            }
            let pad = (512 - (size % 512) as usize) % 512;
            if pad > 0 {
                tx.send(Bytes::from(vec![0u8; pad])).await.map_err(std::io::Error::other)?;
            }
        }
        tx.send(Bytes::from(vec![0u8; 1024])).await.map_err(std::io::Error::other)?;
        Ok::<_, std::io::Error>(())
    });
    let body = futures_util::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|b| (b, rx)) });
    let docker = ctx.get::<Engine>().docker()?;
    let opts = q::UploadToContainerOptionsBuilder::default().path(&dir).build();
    let sent = docker.upload_to_container(&container, Some(opts), bollard::body_stream(body)).await;
    let wrote = writer.await.map_err(err)?;
    sent.map_err(|e| e.to_string().replace("Docker responded with status code ", ""))?;
    wrote.map_err(err)?;
    let _ = ctx.get::<EventBus>().emit("files:changed", &container);
    Ok(count)
}

/// Remove a file or folder in a container (`rm -rf`). The container must be
/// running, with `rm` in it.
#[command]
pub async fn container_file_remove(ctx: Ctx, container: String, path: String) -> Result<(), String> {
    let path = valid_path(&path)?;
    if path.trim_end_matches('/').is_empty() {
        return Err("Not the whole filesystem".into());
    }
    exec(&ctx, &container, vec!["rm".into(), "-rf".into(), "--".into(), path.into()]).await.map(|_| ())
}

/// Show a running container's file in Finder, through `~/eDock` (eDock's
/// engine only).
#[command]
pub async fn container_file_reveal(ctx: Ctx, container: String, path: String) -> Result<(), String> {
    let path = valid_path(&path)?;
    if ctx.get::<Engine>().context().0 != EDOCK {
        return Err("The Finder view is eDock's engine's".into());
    }
    let info = ctx.get::<Engine>().docker()?.inspect_container(&container, None::<q::InspectContainerOptions>).await.map_err(err)?;
    let name = info.name.unwrap_or_default().trim_start_matches('/').to_owned();
    let home = dirs::home_dir().ok_or("HOME is not set")?;
    let at = home.join("eDock").join(&name).join(path.trim_start_matches('/'));
    if !home.join("eDock").join(&name).exists() {
        return Err("~/eDock isn't mounted: eDock's Finder view shows running containers there".into());
    }
    let ok = tokio::process::Command::new("/usr/bin/open").arg("-R").arg(&at).status().await.map_err(err)?.success();
    if ok { Ok(()) } else { Err(format!("Finder couldn't show {}", at.display())) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_checked() {
        assert!(valid_path("/etc/nginx").is_ok());
        for bad in ["etc", "/etc/../root", "/a\nb", ""] {
            assert!(valid_path(bad).is_err(), "{bad:?}");
        }
        assert_eq!(enc("/a b&c"), "/a%20b%26c");
    }

    #[test]
    fn stat_lines_are_read() {
        let out = "81a4|1077|1792605285|/etc/nginx/fastcgi.conf\n41ed|4096|1794692250|/etc/nginx/conf.d\na1ff|11|1|/etc/nginx/modules\n";
        let v = parse_stat(out);
        assert_eq!(v[0], FileEntry { name: "conf.d".into(), kind: "dir".into(), mode: 0o755, size: 4096.0, mtime: 1794692250.0 });
        assert_eq!((v[1].name.as_str(), v[1].kind.as_str(), v[1].mode), ("fastcgi.conf", "file", 0o644));
        assert_eq!(v[2].kind, "link");
    }

    #[tokio::test]
    async fn a_tar_made_here_reads_back() {
        let long = format!("{}/file.txt", "d".repeat(120));
        let mut tar = tar_header(&long, b'0', 5, 0o644, 1);
        tar.extend_from_slice(b"hello");
        tar.resize(tar.len().div_ceil(512) * 512, 0);
        tar.extend_from_slice(&[0u8; 1024]);
        // In odd pieces, as a stream brings them.
        let pieces: Vec<Result<Bytes, bollard::errors::Error>> = tar.chunks(77).map(|c| Ok(Bytes::copy_from_slice(c))).collect();
        let mut r = TarReader { stream: futures_util::stream::iter(pieces), buf: Vec::new(), keep: None };
        let h = r.header().await.unwrap();
        assert_eq!((h.name, h.kind, h.size), (long, b'0', 5));
        r.fill(5).await.unwrap();
        assert_eq!(r.take(5), b"hello");
        // The checksum is the one tar computes.
        let h = tar_header("a", b'0', 0, 0o644, 0);
        let sum: u32 = h.iter().enumerate().map(|(i, b)| if (148..156).contains(&i) { 32 } else { u32::from(*b) }).sum();
        assert_eq!(octal(&h[148..155]), u64::from(sum));
    }

    #[test]
    fn text_or_not() {
        assert_eq!(as_text(b"hi", false).as_deref(), Some("hi"));
        assert!(as_text(b"a\0b", false).is_none());
        assert_eq!(as_text(&"aø".as_bytes()[..2], true).as_deref(), Some("a"));
        assert!(as_text(&"aø".as_bytes()[..2], false).is_none());
    }
}

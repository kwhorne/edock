//! Commands made for AI agents (over MCP: `eDock --mcp`): answers in one
//! piece rather than streams. A container's recent log as text, a command
//! run once in a fresh container, a command run in a running one.

use std::time::Duration;

use bollard::container::LogOutput;
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::query_parameters as q;
use elyra::{command, Ctx};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use crate::engine::Engine;

/// The most output an answer carries; the rest is cut, with a note.
const MAX_OUTPUT: usize = 64 * 1024;

fn cap(mut s: String) -> String {
    if s.len() > MAX_OUTPUT {
        let mut cut = MAX_OUTPUT;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push_str("\n[… cut: the output went on]");
    }
    s
}

fn looks_like_error(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    ["error", "fatal", "panic", "exception", "traceback", "failed", "refused", "denied", "critical"].iter().any(|w| l.contains(w))
}

/// A container's recent log, as text: its last `tail` lines (200 when not
/// given, at most 2000). With `errors_only`, only lines written to stderr
/// or that read like an error (error, fatal, panic, exception, failed…):
/// the quick way to see why a service is unhappy.
#[command(can = "edock.view")]
pub async fn container_logs(ctx: Ctx, container: String, tail: Option<u32>, errors_only: Option<bool>) -> Result<String, String> {
    let docker = ctx.get::<Engine>().docker()?;
    let tail = tail.unwrap_or(200).clamp(1, 2000);
    let o = q::LogsOptionsBuilder::default().stdout(true).stderr(true).tail(&tail.to_string()).build();
    let mut logs = docker.logs(&container, Some(o));
    let mut out = String::new();
    while let Some(item) = logs.next().await {
        let (err, bytes) = match item.map_err(|e| e.to_string())? {
            LogOutput::StdErr { message } => (true, message),
            LogOutput::StdOut { message } | LogOutput::Console { message } => (false, message),
            LogOutput::StdIn { .. } => continue,
        };
        for line in String::from_utf8_lossy(&bytes).lines() {
            if errors_only.unwrap_or(false) && !err && !looks_like_error(line) {
                continue;
            }
            out.push_str(line);
            out.push('\n');
        }
    }
    Ok(cap(out))
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct RunResult {
    pub exit_code: i64,
    /// stdout and stderr, in the order written.
    pub output: String,
    /// It ran past its time and was stopped.
    pub timed_out: bool,
}

/// Run a shell command once in a fresh, throwaway container from `image`
/// (pulled if needed), and return its exit code and output. The container
/// is removed afterwards. It has no network unless `network` is true, and
/// stops after `timeout_secs` (60 when not given, at most 600). A safe place
/// to try something: it touches nothing on the Mac.
#[command(can = "edock.run")]
pub async fn container_run(ctx: Ctx, image: String, command: String, network: Option<bool>, timeout_secs: Option<u32>) -> Result<RunResult, String> {
    let docker = ctx.get::<Engine>().docker()?;
    let timeout = Duration::from_secs(timeout_secs.unwrap_or(60).clamp(1, 600) as u64);
    if docker.inspect_image(&image).await.is_err() {
        let (repo, tag) = match image.rsplit_once(':') {
            Some((r, t)) if !t.contains('/') => (r.to_owned(), t.to_owned()),
            _ => (image.clone(), "latest".to_owned()),
        };
        let mut pull = docker.create_image(Some(q::CreateImageOptionsBuilder::default().from_image(&repo).tag(&tag).build()), None, None);
        while let Some(p) = pull.next().await {
            p.map_err(|e| format!("couldn't pull {image}: {e}"))?;
        }
    }
    let body = bollard::models::ContainerCreateBody {
        image: Some(image.clone()),
        cmd: Some(vec!["/bin/sh".into(), "-c".into(), command]),
        labels: Some([("app.edock.agent-run".to_string(), "1".to_string())].into()),
        host_config: Some(bollard::models::HostConfig {
            network_mode: Some(if network.unwrap_or(false) { "bridge" } else { "none" }.into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let id = docker.create_container(None::<q::CreateContainerOptions>, body).await.map_err(|e| e.to_string())?.id;
    let result = async {
        docker.start_container(&id, None::<q::StartContainerOptions>).await.map_err(|e| e.to_string())?;
        let waited = tokio::time::timeout(timeout, docker.wait_container(&id, None::<q::WaitContainerOptions>).collect::<Vec<_>>()).await;
        let timed_out = waited.is_err();
        if timed_out {
            let _ = docker.kill_container(&id, None::<q::KillContainerOptions>).await;
        }
        let info = docker.inspect_container(&id, None::<q::InspectContainerOptions>).await.map_err(|e| e.to_string())?;
        let exit_code = info.state.and_then(|s| s.exit_code).unwrap_or(-1);
        let mut output = String::new();
        let mut logs = docker.logs(&id, Some(q::LogsOptionsBuilder::default().stdout(true).stderr(true).build()));
        while let Some(Ok(l)) = logs.next().await {
            output.push_str(&String::from_utf8_lossy(&l.into_bytes()));
        }
        Ok::<_, String>(RunResult { exit_code, output: cap(output), timed_out })
    }
    .await;
    let _ = docker.remove_container(&id, Some(q::RemoveContainerOptionsBuilder::default().force(true).build())).await;
    result
}

/// Run a shell command in a running container, as its default user, and
/// return its exit code and output. Stops waiting after `timeout_secs` (60
/// when not given, at most 600). For looking around (`ls`, `cat`, `env`,
/// `ps`) or a quick fix; it changes the container for real.
#[command(can = "edock.exec")]
pub async fn container_exec(ctx: Ctx, container: String, command: String, timeout_secs: Option<u32>) -> Result<RunResult, String> {
    let docker = ctx.get::<Engine>().docker()?;
    let timeout = Duration::from_secs(timeout_secs.unwrap_or(60).clamp(1, 600) as u64);
    let exec = docker
        .create_exec(&container, CreateExecOptions { attach_stdout: Some(true), attach_stderr: Some(true), cmd: Some(vec!["/bin/sh".to_owned(), "-c".into(), command]), ..Default::default() })
        .await
        .map_err(|e| e.to_string())?;
    let mut output = String::new();
    let mut timed_out = false;
    if let StartExecResults::Attached { output: mut stream, .. } = docker.start_exec(&exec.id, None).await.map_err(|e| e.to_string())? {
        let read = async {
            while let Some(Ok(chunk)) = stream.next().await {
                output.push_str(&String::from_utf8_lossy(&chunk.into_bytes()));
                if output.len() > MAX_OUTPUT * 2 {
                    break;
                }
            }
        };
        timed_out = tokio::time::timeout(timeout, read).await.is_err();
    }
    let exit_code = docker.inspect_exec(&exec.id).await.ok().and_then(|i| i.exit_code).unwrap_or(-1);
    Ok(RunResult { exit_code, output: cap(output), timed_out })
}

#[cfg(test)]
mod tests {
    #[test]
    fn long_output_is_cut_on_a_character() {
        let s = "ø".repeat(super::MAX_OUTPUT);
        let c = super::cap(s);
        assert!(c.len() <= super::MAX_OUTPUT + 40 && c.ends_with("went on]"));
    }

    #[test]
    fn error_lines_are_found() {
        assert!(super::looks_like_error("ERROR: connection refused"));
        assert!(super::looks_like_error("thread 'main' panicked at"));
        assert!(!super::looks_like_error("GET / 200 OK"));
    }
}

//! The engine's own log, and a speed test: how long a container takes on
//! this Mac, from create to removed, on eDock and on the other engines
//! running here (`edock bench`'s Docker API row, in the app).

use std::time::{Duration, Instant};

use bollard::query_parameters as q;
use elyra::{command, Ctx, EventBus};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

/// eDock's engine log (`edock diag logs --engine`): its last `lines`
/// (200 when not given, at most 2000), oldest first. `errors_only` keeps
/// warnings and errors.
#[command(can = "edock.view")]
pub async fn engine_log(_ctx: Ctx, lines: Option<u32>, errors_only: Option<bool>) -> Result<String, String> {
    let bin = crate::daemon::engine_binary().ok_or("The eDock engine (edock) isn't installed")?;
    let n = lines.unwrap_or(200).clamp(1, 2000).to_string();
    let mut args = vec!["diag", "logs", "--engine", "-n", n.as_str()];
    if errors_only.unwrap_or(false) {
        args.push("--errors");
    }
    let out = tokio::process::Command::new(bin).args(&args).output().await.map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().trim_start_matches("edock: ").to_owned());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The engines a speed test tries: name, socket under $HOME.
const ENGINES: [(&str, &str); 3] = [("eDock", ".edock/run/docker.sock"), ("OrbStack", ".orbstack/run/docker.sock"), ("Docker Desktop", ".docker/run/docker.sock")];
const IMAGE: &str = "alpine:latest";
const WARMUP: usize = 3;
const LABEL: &str = "app.edock.bench";

#[derive(Serialize, Deserialize, specta::Type, Clone)]
pub struct BenchRow {
    pub engine: String,
    /// Milliseconds, create → start → exit → removed.
    pub p50: f64,
    pub p90: f64,
    pub mean: f64,
    pub runs: u32,
    /// Why there's no time.
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, specta::Type, Clone)]
pub struct BenchProgress {
    pub engine: String,
    pub done: u32,
    pub of: u32,
}

/// One container's whole life, through the Docker API.
async fn once(docker: &bollard::Docker) -> Result<Duration, String> {
    let body = bollard::models::ContainerCreateBody {
        image: Some(IMAGE.into()),
        cmd: Some(vec!["true".into()]),
        labels: Some([(LABEL.to_string(), "1".to_string()), ("app.edock.helper".to_string(), "bench".to_string())].into()),
        host_config: Some(bollard::models::HostConfig { network_mode: Some("none".into()), ..Default::default() }),
        ..Default::default()
    };
    let t = Instant::now();
    let id = docker.create_container(None::<q::CreateContainerOptions>, body).await.map_err(|e| e.to_string())?.id;
    let run = async {
        docker.start_container(&id, None::<q::StartContainerOptions>).await.map_err(|e| e.to_string())?;
        let _ = docker.wait_container(&id, None::<q::WaitContainerOptions>).collect::<Vec<_>>().await;
        Ok::<_, String>(())
    }
    .await;
    let removed = docker.remove_container(&id, Some(q::RemoveContainerOptionsBuilder::default().force(true).build())).await;
    run?;
    removed.map_err(|e| e.to_string())?;
    Ok(t.elapsed())
}

fn pct(sorted: &[f64], p: usize) -> f64 {
    sorted.get((sorted.len() * p / 100).min(sorted.len().saturating_sub(1))).copied().unwrap_or(0.0)
}

/// How long a container takes, create to removed, on each engine running on
/// this Mac: eDock, OrbStack, Docker Desktop. `runs` times each (20 when not
/// given, at most 100), after a few warm-up runs; alpine with no network.
/// Pulls alpine where it's missing. Progress on `bench:progress`.
#[command]
pub async fn speed_test(ctx: Ctx, runs: Option<u32>) -> Result<Vec<BenchRow>, String> {
    let runs = runs.unwrap_or(20).clamp(1, 100) as usize;
    let home = dirs::home_dir().ok_or("HOME is not set")?;
    let bus = ctx.get::<EventBus>();
    let mut rows = Vec::new();
    for (name, socket) in ENGINES {
        let path = home.join(socket);
        if tokio::net::UnixStream::connect(&path).await.is_err() {
            continue;
        }
        let row = async {
            let docker = bollard::Docker::connect_with_unix(&path.to_string_lossy(), 60, bollard::API_DEFAULT_VERSION).map_err(|e| e.to_string())?;
            if docker.inspect_image(IMAGE).await.is_err() {
                let mut pull = docker.create_image(Some(q::CreateImageOptionsBuilder::default().from_image("alpine").tag("latest").build()), None, None);
                while let Some(p) = pull.next().await {
                    p.map_err(|e| format!("couldn't pull alpine: {e}"))?;
                }
            }
            let mut times = Vec::with_capacity(runs);
            for i in 0..WARMUP + runs {
                let d = once(&docker).await?;
                if i >= WARMUP {
                    times.push(d.as_secs_f64() * 1000.0);
                    let _ = bus.emit("bench:progress", &BenchProgress { engine: name.into(), done: times.len() as u32, of: runs as u32 });
                }
            }
            times.sort_by(f64::total_cmp);
            let round = |v: f64| (v * 10.0).round() / 10.0;
            Ok::<_, String>(BenchRow {
                engine: name.into(),
                p50: round(pct(&times, 50)),
                p90: round(pct(&times, 90)),
                mean: round(times.iter().sum::<f64>() / times.len() as f64),
                runs: runs as u32,
                error: None,
            })
        }
        .await;
        rows.push(row.unwrap_or_else(|e| BenchRow { engine: name.into(), p50: 0.0, p90: 0.0, mean: 0.0, runs: 0, error: Some(e) }));
    }
    if rows.is_empty() {
        return Err("No engine is running".into());
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    #[test]
    fn percentiles() {
        let v: Vec<f64> = (1..=10).map(f64::from).collect();
        assert_eq!(super::pct(&v, 50), 6.0);
        assert_eq!(super::pct(&v, 90), 10.0);
        assert_eq!(super::pct(&[], 50), 0.0);
    }
}

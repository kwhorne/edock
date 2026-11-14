//! The build history: BuildKit's record of each `docker build` on eDock
//! (`edock builds`, which reads `docker buildx history`). Asking wakes the
//! builder, so the app asks only while the Builds page is open.

use elyra::{command, Ctx};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::engine::{Engine, EDOCK};

fn on_edock(ctx: &Ctx) -> Result<(), String> {
    if ctx.get::<Engine>().context().0 == EDOCK {
        Ok(())
    } else {
        Err("The build history is eDock's engine's: switch to it in the toolbar".into())
    }
}

/// A builder's name and a build's id, as `edock builds` takes them.
fn check(builder: &str, id: &str) -> Result<(), String> {
    let builder_ok = !builder.is_empty() && !builder.starts_with('-') && builder.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b));
    let id_ok = !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric());
    if builder_ok && id_ok { Ok(()) } else { Err("That isn't a build".into()) }
}

async fn edock(args: &[&str]) -> Result<String, String> {
    let bin = crate::daemon::engine_binary().ok_or("The eDock engine (edock) isn't installed")?;
    let out = tokio::process::Command::new(bin).arg("builds").args(args).output().await.map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let e = String::from_utf8_lossy(&out.stderr);
        Err(e.trim().lines().last().unwrap_or("edock builds failed").trim_start_matches("edock: ").to_owned())
    }
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or("").to_owned()
}

fn n(v: &Value) -> u32 {
    v.as_u64().unwrap_or(0) as u32
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct BuildRow {
    pub builder: String,
    pub id: String,
    /// The build's name (its tag, or its Dockerfile's folder).
    pub name: String,
    /// Completed, Running, Error or Canceled.
    pub status: String,
    /// RFC 3339.
    pub started: String,
    pub completed: String,
    pub steps: u32,
    pub cached: u32,
}

#[command]
pub async fn builds_list(ctx: Ctx) -> Result<Vec<BuildRow>, String> {
    on_edock(&ctx)?;
    let all: Vec<Value> = serde_json::from_str(&edock(&["list"]).await?).map_err(|e| e.to_string())?;
    Ok(all
        .iter()
        .map(|b| BuildRow {
            builder: s(&b["builder"]),
            id: s(&b["id"]),
            name: s(&b["name"]),
            status: s(&b["status"]),
            started: s(&b["started"]),
            completed: s(&b["completed"]),
            steps: n(&b["steps"]),
            cached: n(&b["cached"]),
        })
        .collect())
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct BuildStep {
    pub name: String,
    pub started: String,
    pub completed: String,
    pub cached: bool,
    pub error: Option<String>,
    pub log: String,
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct BuildWarning {
    pub text: String,
    pub detail: String,
    pub url: String,
    pub line: Option<u32>,
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct BuildDetail {
    pub builder: String,
    pub id: String,
    pub name: String,
    pub status: String,
    pub started: String,
    pub completed: String,
    pub context: String,
    pub dockerfile: String,
    pub total: u32,
    pub cached: u32,
    /// Why it failed: the message, the step, and the Dockerfile lines.
    pub error: Option<String>,
    pub error_step: Option<String>,
    pub error_source: Option<String>,
    pub steps: Vec<BuildStep>,
    pub warnings: Vec<BuildWarning>,
}

#[command]
pub async fn build_detail(ctx: Ctx, builder: String, id: String) -> Result<BuildDetail, String> {
    on_edock(&ctx)?;
    check(&builder, &id)?;
    let d: Value = serde_json::from_str(&edock(&["show", &builder, &id]).await?).map_err(|e| e.to_string())?;
    let e = &d["error"];
    Ok(BuildDetail {
        builder: s(&d["builder"]),
        id: s(&d["id"]),
        name: s(&d["name"]),
        status: s(&d["status"]),
        started: s(&d["started"]),
        completed: s(&d["completed"]),
        context: s(&d["context"]),
        dockerfile: s(&d["dockerfile"]),
        total: n(&d["total"]),
        cached: n(&d["cached"]),
        error: e["message"].as_str().map(str::to_owned),
        error_step: e["step"].as_str().map(str::to_owned),
        error_source: e["source"].as_str().filter(|x| !x.is_empty()).map(str::to_owned),
        steps: d["steps"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|st| BuildStep {
                name: s(&st["name"]),
                started: s(&st["started"]),
                completed: s(&st["completed"]),
                cached: st["cached"] == true,
                error: st["error"].as_str().map(str::to_owned),
                log: s(&st["log"]),
            })
            .collect(),
        warnings: d["warnings"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|w| BuildWarning { text: s(&w["text"]), detail: s(&w["detail"]), url: s(&w["url"]), line: w["line"].as_u64().map(|l| l as u32) })
            .collect(),
    })
}

/// Take a build off the history.
#[command]
pub async fn build_remove(ctx: Ctx, builder: String, id: String) -> Result<(), String> {
    on_edock(&ctx)?;
    check(&builder, &id)?;
    edock(&["rm", &builder, &id]).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    #[test]
    fn builders_and_ids_are_checked() {
        assert!(super::check("edock-build", "abc123").is_ok());
        for (b, id) in [("-x", "a"), ("edock-build", "a/b"), ("", "a"), ("a b", "a"), ("ok", "")] {
            assert!(super::check(b, id).is_err(), "{b} {id}");
        }
    }
}

//! An image on eDock's engine, looked into: its layers (each Dockerfile
//! line and what it added), and its known vulnerabilities, found by Grype
//! from the image in eDock's store (the VM isn't woken), with an SBOM.

use std::collections::HashSet;
use std::sync::Arc;

use elyra::files::ChosenFile;
use elyra::{command, Ctx, EventBus};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::daemon::{engine_request, engine_stream};
use crate::engine::{Engine, EDOCK};

fn on_edock(ctx: &Ctx) -> Result<(), String> {
    if ctx.get::<Engine>().context().0 == EDOCK {
        Ok(())
    } else {
        Err("Layers and scans are eDock's own: switch to its engine in the toolbar".into())
    }
}

/// A query value, percent-encoded (an image reference has `/` and `:`).
fn enc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

fn answer(r: Option<(u16, Value)>) -> Result<Value, String> {
    match r {
        Some((s, v)) if s < 300 => Ok(v),
        Some((s, v)) => Err(v["message"].as_str().map(str::to_owned).unwrap_or_else(|| format!("The engine said {s}"))),
        None => Err("eDock's engine isn't running".into()),
    }
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or("").to_owned()
}

// ── Layers ─────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, specta::Type)]
pub struct Layer {
    /// The Dockerfile line (`RUN apk add …`), or "" when the image doesn't say.
    pub by: String,
    pub size: f64,
    /// Changed no files (ENV, CMD…).
    pub empty: bool,
    pub created: String,
    /// Came with the base image.
    pub base: bool,
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct ImageLayers {
    pub platform: String,
    pub size: f64,
    pub layers: Vec<Layer>,
}

#[command]
pub async fn image_layers(ctx: Ctx, name: String) -> Result<ImageLayers, String> {
    on_edock(&ctx)?;
    let v = answer(engine_request("GET", &format!("/edock/images/layers?name={}", enc(&name))).await)?;
    let img = &v["images"][0];
    Ok(ImageLayers {
        platform: s(&img["platform"]),
        size: img["size"].as_f64().unwrap_or(0.0),
        layers: img["layers"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|l| Layer { by: s(&l["by"]), size: l["size"].as_f64().unwrap_or(0.0), empty: l["empty"] == true, created: s(&l["created"]), base: l["base"] == true })
            .collect(),
    })
}

// ── Vulnerabilities ────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, specta::Type, Default)]
pub struct Counts {
    pub critical: u32,
    pub high: u32,
    pub medium: u32,
    pub low: u32,
    pub other: u32,
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct ScanSummary {
    /// Unix time: the scan's key.
    pub scanned: f64,
    pub counts: Counts,
    pub vulns: u32,
    pub fixable: u32,
    pub packages: u32,
}

fn counts(v: &Value) -> Counts {
    let n = |k: &str| v[k].as_u64().unwrap_or(0) as u32;
    Counts { critical: n("Critical"), high: n("High"), medium: n("Medium"), low: n("Low"), other: n("Negligible") + n("Unknown") }
}

/// The image's scans, newest first.
#[command]
pub async fn image_scans(ctx: Ctx, name: String) -> Result<Vec<ScanSummary>, String> {
    on_edock(&ctx)?;
    let v = answer(engine_request("GET", &format!("/edock/images/scans?name={}", enc(&name))).await)?;
    Ok(v["scans"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|sc| ScanSummary {
            scanned: sc["scanned"].as_f64().unwrap_or(0.0),
            counts: counts(&sc["counts"]),
            vulns: sc["vulns"].as_u64().unwrap_or(0) as u32,
            fixable: sc["fixable"].as_u64().unwrap_or(0) as u32,
            packages: sc["packages"].as_u64().unwrap_or(0) as u32,
        })
        .collect())
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct Vuln {
    pub id: String,
    /// Critical, High, Medium, Low, Negligible or Unknown.
    pub severity: String,
    pub package: String,
    pub version: String,
    pub kind: String,
    /// Versions that fix it; none yet when empty.
    pub fixed: Vec<String>,
    pub url: String,
    pub description: String,
    /// The Dockerfile line that brought the package in, when known.
    pub by: Option<String>,
    /// It came with the base image.
    pub base: Option<bool>,
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct ScanResult {
    pub scanned: f64,
    pub packages: u32,
    /// When the vulnerability database was built.
    pub db: String,
    pub scanner: String,
    pub vulns: Vec<Vuln>,
}

fn result(v: &Value) -> ScanResult {
    ScanResult {
        scanned: v["scanned"].as_f64().unwrap_or(0.0),
        packages: v["packages"].as_u64().unwrap_or(0) as u32,
        db: s(&v["db"]),
        scanner: s(&v["grype"]),
        vulns: v["vulns"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|x| Vuln {
                id: s(&x["id"]),
                severity: s(&x["severity"]),
                package: s(&x["package"]),
                version: s(&x["version"]),
                kind: s(&x["kind"]),
                fixed: x["fixed"].as_array().into_iter().flatten().filter_map(|f| f.as_str().map(str::to_owned)).collect(),
                url: s(&x["url"]),
                description: s(&x["description"]),
                by: x["by"].as_str().map(str::to_owned),
                base: x["base"].as_bool(),
            })
            .collect(),
    }
}

/// One scan's findings (`at`), or the newest when `at` is none.
#[command]
pub async fn image_scan_result(ctx: Ctx, name: String, at: Option<f64>) -> Result<Option<ScanResult>, String> {
    on_edock(&ctx)?;
    let at = at.map(|a| format!("&at={}", a as u64)).unwrap_or_default();
    let v = answer(engine_request("GET", &format!("/edock/images/scan?name={}{at}", enc(&name))).await)?;
    Ok((!v.is_null() && v.get("vulns").is_some()).then(|| result(&v)))
}

/// Images being scanned now.
#[derive(Default)]
pub struct Scanning(Mutex<HashSet<String>>);

#[derive(Serialize, Deserialize, specta::Type, Clone)]
pub struct ScanProgress {
    pub name: String,
    /// "scanner" (downloading Grype), "database", "scanning".
    pub phase: String,
    pub pct: u32,
    pub done: bool,
    pub error: Option<String>,
}

/// Scan an image for known vulnerabilities. Progress on `scan:progress`;
/// the first scan downloads Grype and its database (about 200 MB).
#[command]
pub async fn image_scan(ctx: Ctx, name: String) -> Result<(), String> {
    on_edock(&ctx)?;
    let scanning = ctx.get::<Scanning>();
    if !scanning.0.lock().insert(name.clone()) {
        return Err(format!("{name} is being scanned already"));
    }
    let bus = ctx.get::<EventBus>();
    tokio::spawn(scan(name, bus, scanning));
    Ok(())
}

async fn scan(name: String, bus: Arc<EventBus>, scanning: Arc<Scanning>) {
    let mut error = None;
    let n2 = name.clone();
    let b2 = bus.clone();
    let mut got_result = false;
    let status = engine_stream("POST", &format!("/edock/images/scan?name={}", enc(&name)), |line| {
        if let Some(e) = line["error"].as_str() {
            error = Some(e.to_owned());
        } else if line.get("result").is_some() {
            got_result = true;
        } else if let Some(phase) = line["phase"].as_str() {
            let _ = b2.emit("scan:progress", &ScanProgress { name: n2.clone(), phase: phase.into(), pct: line["pct"].as_u64().unwrap_or(0) as u32, done: false, error: None });
        }
    })
    .await;
    let error = match status {
        Ok(s) if s < 300 && error.is_none() && got_result => None,
        Ok(s) if error.is_none() => Some(format!("The engine said {s}")),
        Ok(_) => error,
        Err(e) => Some(e),
    };
    scanning.0.lock().remove(&name);
    let _ = bus.emit("scan:progress", &ScanProgress { name, phase: "done".into(), pct: 100, done: true, error });
}

/// Save a scan's SBOM (CycloneDX JSON) where the user chose.
#[command]
pub async fn image_sbom_save(ctx: Ctx, name: String, at: f64, file: ChosenFile) -> Result<(), String> {
    on_edock(&ctx)?;
    let v = answer(engine_request("GET", &format!("/edock/images/sbom?name={}&at={}", enc(&name), at as u64)).await)?;
    let text = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    std::fs::write(file.path(), text).map_err(|e| format!("{}: {e}", file.name()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_reference_is_one_query_value() {
        assert_eq!(super::enc("ghcr.io/org/app:1.2"), "ghcr.io%2Forg%2Fapp%3A1.2");
        assert_eq!(super::enc("a&at=1"), "a%26at%3D1");
    }
}

//! USB devices for containers (beta): a device on the Mac handed to eDock's
//! VM (`edock usb attach`), where containers reach it with `--device`, and
//! given back (`edock usb detach`). The first attach installs eDock's USB
//! helper, which asks for an administrator's password once.

use elyra::{command, Ctx};
use serde::{Deserialize, Serialize};
use serde_json::Value;


fn on_edock(ctx: &Ctx) -> Result<(), String> {
    crate::engine::only_on_edock(ctx, "USB devices are passed to eDock's engine: switch to it in the toolbar")
}

/// A device's key: its location on the Mac's bus (`0x02114000`), or a
/// test device's name; never a flag.
fn valid(key: &str) -> Result<&str, String> {
    if !key.is_empty() && key.len() <= 64 && !key.starts_with('-') && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b':') {
        Ok(key)
    } else {
        Err("That isn't a USB device".into())
    }
}

async fn edock(args: &[&str]) -> Result<String, String> {
    let bin = crate::daemon::engine_binary().ok_or("The eDock engine (edock) isn't installed")?;
    let out = tokio::process::Command::new(bin).arg("usb").args(args).output().await.map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let e = String::from_utf8_lossy(&out.stderr);
        Err(e.trim().lines().last().unwrap_or("edock usb failed").trim_start_matches("edock: ").to_owned())
    }
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct UsbPath {
    /// `/dev/ttyACM0`, `/dev/input/event3`…
    pub path: String,
    pub what: String,
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct UsbDevice {
    pub key: String,
    pub name: String,
    /// `vendor:product`.
    pub id: String,
    pub speed: String,
    /// "edock" (in the VM, for containers) or "host" (on the Mac).
    pub state: String,
    /// serial, hid, storage, …: what kind of device it is.
    pub kind: String,
    pub maker: String,
    /// Where containers find it, while it's in eDock.
    pub paths: Vec<UsbPath>,
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct UsbList {
    pub devices: Vec<UsbDevice>,
    pub error: Option<String>,
}

#[command]
pub async fn usb_list(ctx: Ctx) -> Result<UsbList, String> {
    on_edock(&ctx)?;
    let v: Value = serde_json::from_str(&edock(&["list", "--json"]).await?).map_err(|e| e.to_string())?;
    let s = |x: &Value| x.as_str().unwrap_or("").to_owned();
    Ok(UsbList {
        error: v["error"].as_str().map(str::to_owned),
        devices: v["devices"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|d| UsbDevice {
                key: s(&d["key"]),
                name: s(&d["name"]),
                id: s(&d["id"]),
                speed: s(&d["speed"]),
                state: s(&d["state"]),
                kind: s(&d["kind"]),
                maker: s(&d["maker"]),
                paths: d["paths"].as_array().into_iter().flatten().map(|p| UsbPath { path: s(&p["path"]), what: s(&p["what"]) }).collect(),
            })
            .collect(),
    })
}

/// Hand a device to eDock: it leaves the Mac until it's given back.
#[command]
pub async fn usb_attach(ctx: Ctx, key: String) -> Result<(), String> {
    on_edock(&ctx)?;
    edock(&["attach", valid(&key)?]).await.map(|_| ())
}

/// Give a device back to the Mac: the engine closes its pipe, and the
/// helper hands the device back (as `edock usb detach` does, by location,
/// so it works for a device that's in eDock but not on the Mac's bus).
#[command]
pub async fn usb_detach(ctx: Ctx, key: String) -> Result<(), String> {
    on_edock(&ctx)?;
    let key = valid(&key)?;
    match crate::daemon::engine_call("DELETE", &format!("/edock/usb/{key}"), None).await {
        Some((s, _)) if s < 300 => Ok(()),
        Some((_, v)) => Err(v["message"].as_str().unwrap_or("The engine refused").to_owned()),
        None => Err("eDock's engine isn't running".into()),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn keys_are_locations_or_names_never_flags() {
        for ok in ["0x02114000", "1532:00d7", "t-serial"] {
            assert!(super::valid(ok).is_ok(), "{ok}");
        }
        for bad in ["", "--all", "a b", "a/b", "x;rm"] {
            assert!(super::valid(bad).is_err(), "{bad}");
        }
    }
}

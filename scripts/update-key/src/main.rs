//! eDock's update signing key (ed25519), as Elyra's updater checks it.
//!
//!   update-key keygen [KEY]       make the key (refuses to overwrite one)
//!   update-key public [KEY]       the public key, base64, for the app
//!   update-key sign FILE [KEY]    the file's signature, base64
//!   update-key manifest VERSION URL ZIP [NOTES_FILE]
//!                                 latest.json for Elyra's updater: the zip's
//!                                 SHA-256 and signature, for macos-aarch64
//!
//! KEY defaults to `$EDOCK_UPDATE_KEY`, else `~/.config/edock/update-key`.
//! It holds the 32-byte secret as base64, readable by its owner only. Keep
//! a copy somewhere safe: an installed eDock only accepts updates signed
//! with it.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};

fn default_key() -> PathBuf {
    if let Some(p) = std::env::var_os("EDOCK_UPDATE_KEY") {
        return p.into();
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".config/edock/update-key")
}

fn load(path: &PathBuf) -> Result<SigningKey, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e} (make one with `update-key keygen`)", path.display()))?;
    let bytes = B64.decode(text.trim()).map_err(|e| format!("{}: {e}", path.display()))?;
    let seed: [u8; 32] = bytes.as_slice().try_into().map_err(|_| format!("{}: not a 32-byte key", path.display()))?;
    Ok(SigningKey::from_bytes(&seed))
}

fn run(args: &[String]) -> Result<String, String> {
    let key_arg = |i: usize| args.get(i).map(PathBuf::from).unwrap_or_else(default_key);
    match args.first().map(String::as_str) {
        Some("keygen") => {
            let path = key_arg(1);
            if path.exists() {
                return Err(format!("{} exists: not overwriting a key installed apps trust", path.display()));
            }
            let mut seed = [0u8; 32];
            getrandom::fill(&mut seed).map_err(|e| e.to_string())?;
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            let mut f = std::fs::OpenOptions::new();
            f.write(true).create_new(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut f, 0o600);
            let mut file = f.open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            writeln!(file, "{}", B64.encode(seed)).map_err(|e| e.to_string())?;
            let public = B64.encode(SigningKey::from_bytes(&seed).verifying_key().to_bytes());
            Ok(format!("wrote {}\npublic key: {public}", path.display()))
        }
        Some("public") => Ok(B64.encode(load(&key_arg(1))?.verifying_key().to_bytes())),
        Some("sign") => {
            let file = args.get(1).ok_or("usage: update-key sign FILE [KEY]")?;
            let data = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
            Ok(B64.encode(load(&key_arg(2))?.sign(&data).to_bytes()))
        }
        Some("manifest") => {
            let (Some(version), Some(url), Some(zip)) = (args.get(1), args.get(2), args.get(3)) else {
                return Err("usage: update-key manifest VERSION URL ZIP [NOTES_FILE]".into());
            };
            let data = std::fs::read(zip).map_err(|e| format!("{zip}: {e}"))?;
            let notes = match args.get(4) {
                Some(f) => std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?.trim().to_owned(),
                None => format!("eDock {version}"),
            };
            use sha2::Digest;
            let sha: String = sha2::Sha256::digest(&data).iter().map(|b| format!("{b:02x}")).collect();
            let signature = B64.encode(load(&default_key())?.sign(&data).to_bytes());
            let manifest = serde_json::json!({
                "version": version,
                "notes": notes,
                "platforms": { "macos-aarch64": { "url": url, "signature": signature, "sha256": sha } },
            });
            serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())
        }
        _ => Err("usage: update-key keygen [KEY] | public [KEY] | sign FILE [KEY] | manifest VERSION URL ZIP [NOTES_FILE]".into()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(out) => {
            println!("{out}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("update-key: {e}");
            ExitCode::FAILURE
        }
    }
}

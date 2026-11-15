use crate::paths;
use crate::state::Ctl;
use crate::t;
use crate::vehicles::VehicleDb;
use serde_json::Value;
use std::io::Read;
use std::time::Duration;

pub const DEFAULT_URL: &str = "https://github.com/mrvokintos/WT-Vehicles-Parser/releases/download/db-latest";
/// default of earlier builds, migrated on load
pub const OLD_DEFAULT_URL: &str = "https://github.com/mrvokintos/WT-Vehicles-Parser/releases/latest/download";
pub const META_FILE: &str = "vehicles.meta.json";
const MIN_ENTRIES: usize = 1500;

pub fn installed_version() -> Option<String> {
    let text = std::fs::read_to_string(paths::file(META_FILE)).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v.get("game_version").and_then(Value::as_str).map(str::to_string)
}

pub fn spawn_check(ctl: Ctl, force: bool) {
    let url = ctl.with(|s| {
        s.db_status = t!("db.checking");
        s.cfg.db.url.trim_end_matches('/').to_string()
    });
    ctl.repaint();
    std::thread::Builder::new()
        .name("db-update".into())
        .spawn(move || {
            let status = match check(&ctl, &url, force) {
                Ok(s) => s,
                Err(e) => {
                    log::warn!("Обновление базы: {e}");
                    t!("db.failed", error = e)
                }
            };
            ctl.with(|s| s.db_status = status);
            ctl.repaint();
        })
        .ok();
}

fn check(ctl: &Ctl, url: &str, force: bool) -> Result<String, String> {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).build();
    let manifest: Value = agent
        .get(&format!("{url}/manifest.json"))
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(404, _) => t!("db.not_published"),
            e => e.to_string(),
        })?
        .into_json()
        .map_err(|e| format!("manifest.json: {e}"))?;
    let remote = manifest.get("game_version").and_then(Value::as_str).unwrap_or("").to_string();
    let local = installed_version();
    if !force && !remote.is_empty() && local.as_deref() == Some(remote.as_str()) {
        log::info!("vehicle db is up to date (game {remote})");
        return Ok(t!("db.up_to_date"));
    }
    let mut bytes = Vec::new();
    agent
        .get(&format!("{url}/vehicles.json"))
        .call()
        .map_err(|e| e.to_string())?
        .into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| format!("vehicles.json: {e}"))?;
    let text = String::from_utf8(bytes).map_err(|_| "vehicles.json: not UTF-8".to_string())?;
    let db = VehicleDb::from_json(&text).map_err(|e| t!("db.corrupt", error = e))?;
    if db.len() < MIN_ENTRIES {
        return Err(t!("db.too_small", count = db.len()));
    }
    verify(&manifest, text.as_bytes(), db.len())?;
    let n = db.vehicle_count();
    write_atomic(paths::VEHICLES_FILE, &text)?;
    write_atomic(META_FILE, &serde_json::to_string_pretty(&manifest).unwrap_or_default())?;
    ctl.with(|s| {
        s.logic.replace_db(db);
        s.db_version = Some(remote.clone());
    });
    log::info!("vehicle db updated: {n} vehicles, game {remote}");
    Ok(t!("db.updated"))
}

/// The release may be mid-update or the download cut short: never store a vehicles.json that
/// doesn't match its manifest, otherwise the newer version would be recorded with old data.
fn verify(manifest: &Value, bytes: &[u8], count: usize) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    if let Some(want) = manifest.get("sha256").and_then(Value::as_str) {
        let got: String = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
        if !got.eq_ignore_ascii_case(want) {
            return Err(t!("db.mismatch"));
        }
    }
    if let Some(want) = manifest.get("count").and_then(Value::as_u64) {
        if want as usize != count {
            return Err(t!("db.count_mismatch", count = count, want = want));
        }
    }
    Ok(())
}

fn write_atomic(name: &str, text: &str) -> Result<(), String> {
    let path = paths::file(name);
    let tmp = path.with_extension("download");
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn manifest_must_match_download() {
        let data = b"{\"A\": {}}\n";
        let sha = "4c5d3d4b1d6b1e1d0bff1a8c2f7f0d37a4c58d93d2b6e9a6a4c0b2b6c3a6e2f1";
        assert!(verify(&json!({}), data, 1).is_ok(), "old manifests without checks");
        assert!(verify(&json!({"count": 1}), data, 1).is_ok());
        assert!(verify(&json!({"count": 2}), data, 1).is_err());
        assert!(verify(&json!({"sha256": sha}), data, 1).is_err());
        use sha2::{Digest, Sha256};
        let real: String = Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect();
        assert!(verify(&json!({"sha256": real, "count": 1}), data, 1).is_ok());
    }
}

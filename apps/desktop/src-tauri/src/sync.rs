use crate::{setup, store};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::io::Read;
pub fn check_grants(c: &Value) -> Result<(), String> {
    if c["fixture"] == true {
        return Ok(());
    }
    let grants: Vec<Value> =
        serde_json::from_str(include_str!("../../../../resources/catalog-grants.json")).unwrap();
    for w in c["worlds"].as_array().ok_or("missing worlds")? {
        if !grants.iter().any(|g| {
            g["id"] == w["permissionRef"]
                && g["license"] == w["sourceLicense"]
                && g["redistributionAllowed"] == true
        }) {
            return Err(
                "本番カタログの許諾リストは未登録です。架空カタログのみ導入できます。".into(),
            );
        }
    }
    Ok(())
}
pub fn apply_delta(db: &Connection, d: &Value) -> Result<(), String> {
    let mut c = store::load(db)?["catalog"].clone();
    if d["schemaVersion"] != 1 || d["baseVersion"] != c["version"] || !d["version"].is_string() {
        return Err("delta base/schema mismatch; retain old catalog".into());
    }
    let rows = c["worlds"].as_array_mut().unwrap();
    for w in d["upserts"].as_array().ok_or("upserts required")? {
        if let Some(index) = rows.iter().position(|x| x["worldId"] == w["worldId"]) {
            rows[index] = w.clone();
        } else {
            rows.push(w.clone());
        }
    }
    for id in d["deletedIds"].as_array().ok_or("deleted IDs required")? {
        if let Some(w) = rows.iter_mut().find(|w| w["worldId"] == *id) {
            w["deleted"] = json!(true);
        }
    }
    c["version"] = d["version"].clone();
    check_grants(&c)?;
    store::replace_catalog(db, &c)
}
pub fn download_catalog() -> Result<(Value, bool), String> {
    let m: Value =
        serde_json::from_str(include_str!("../../../../resources/catalog-manifest.json")).unwrap();
    let (a, delta) = if !m["delta"].is_null() {
        (&m["delta"], true)
    } else if !m["snapshot"].is_null() {
        (&m["snapshot"], false)
    } else {
        return Err("許諾済み配信元は未設定です。現在のカタログで検索を続けられます。".into());
    };
    let asset: setup::Asset = serde_json::from_value(a.clone()).map_err(|e| e.to_string())?;
    if !asset.url.starts_with("https://") || asset.size > 100 * 1024 * 1024 {
        return Err("invalid trusted catalog manifest".into());
    }
    let response = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?
        .get(&asset.url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("同期失敗（旧カタログを保持）: {e}"))?;
    let mut bytes = Vec::new();
    response
        .take(asset.size + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    use sha2::{Digest, Sha256};
    if bytes.len() as u64 != asset.size || format!("{:x}", Sha256::digest(&bytes)) != asset.sha256 {
        return Err("catalog integrity mismatch".into());
    }
    Ok((
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())?,
        delta,
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delta_deletion_and_failed_sync() {
        let t = tempfile::tempdir().unwrap();
        let db = store::open(t.path()).unwrap();
        let old = store::load(&db).unwrap();
        let d = json!({"schemaVersion":1,"baseVersion":"bad","version":"new","upserts":[],"deletedIds":[]});
        assert!(apply_delta(&db, &d).is_err());
        assert_eq!(store::load(&db).unwrap(), old);
        let d = json!({"schemaVersion":1,"baseVersion":old["catalog"]["version"],"version":"new","upserts":[],"deletedIds":["fixture-rain-room"]});
        apply_delta(&db, &d).unwrap();
        assert!(store::load(&db).unwrap()["catalog"]["worlds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["worldId"] == "fixture-rain-room" && w["deleted"] == true));
        assert!(download_catalog().is_err());
    }
}

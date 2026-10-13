use crate::{preferences, store};
use reqwest::blocking::Client;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::{
    io::Read,
    time::{SystemTime, UNIX_EPOCH},
};
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn world_id(input: &str) -> Result<String, String> {
    let s = input.trim();
    if store::official_url(s).is_some() {
        return Ok(s.to_lowercase());
    }
    let url =
        reqwest::Url::parse(s).map_err(|_| "ワールドURLかwrld_で始まるIDを入力してください")?;
    if url.scheme() != "https"
        || ![Some("vrchat.com"), Some("www.vrchat.com"), Some("vrch.at")].contains(&url.host_str())
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("VRChat公式のワールドURLを入力してください".into());
    }
    let candidate = url
        .query_pairs()
        .find(|(k, _)| k == "worldId")
        .map(|(_, v)| v.into_owned())
        .or_else(|| {
            url.path_segments()?
                .find(|s| s.starts_with("wrld_"))
                .map(String::from)
        })
        .ok_or("URLにワールドIDがありません")?;
    if store::official_url(&candidate).is_none() {
        return Err("ワールドIDの形式が不正です".into());
    }
    Ok(candidate.to_lowercase())
}
pub fn normalize(raw: &Value, timestamp: u64) -> Result<Value, String> {
    let id = raw["id"]
        .as_str()
        .ok_or("APIからワールドIDが返りませんでした")?;
    let url = store::official_url(id).ok_or("APIのIDが不正です")?;
    if raw["releaseStatus"] != "public" {
        return Err("公開ワールドだけを追加できます".into());
    }
    let name = raw["name"]
        .as_str()
        .filter(|s| !s.is_empty() && s.chars().count() <= 200)
        .ok_or("ワールド名が不正です")?;
    let tags: Vec<&str> = raw["tags"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .filter_map(|s| s.strip_prefix("author_tag_"))
                .take(64)
                .collect()
        })
        .unwrap_or_default();
    let mut platforms: Vec<&str> = raw["unityPackages"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|p| match p["platform"].as_str() {
                    Some("standalonewindows") => Some("pc"),
                    Some("android") => Some("quest"),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    platforms.sort();
    platforms.dedup();
    let source = "vrchat-api-public";
    let thumbnail = raw["thumbnailImageUrl"].as_str().filter(|url|image_url(url)).or(raw["imageUrl"].as_str().filter(|url|image_url(url))).map(|url|json!({"url":url,"license":"VRChat公開画像・個人閲覧用キャッシュ（再配布許諾なし）"}));
    let attributes: serde_json::Map<String, Value> =
        ["rain", "night", "chill", "photo", "nature", "game", "light"]
            .iter()
            .map(|k| ((*k).into(), json!({"value":null,"sourceId":source})))
            .collect();
    // Anonymous responses omit package/platform and population information. Never infer compatibility or measured performance.
    Ok(
        json!({"schemaVersion":1,"worldId":id,"name":name,"description":raw["description"].as_str().map(|s|s.chars().take(8000).collect::<String>()),"author":raw["authorName"].as_str(),"tags":tags,"platforms":if platforms.is_empty(){Value::Null}else{json!(platforms)},"capacity":raw["capacity"].as_u64().filter(|n|*n>0),"contentWarnings":null,"thumbnail":thumbnail,"sourceUrl":url,"fetchedAt":timestamp_string(timestamp),"sourceLicense":"VRChat公開API・個人用ローカルキャッシュ（再配布許諾なし）","permissionRef":"personal-local-cache","deleted":false,"attributes":attributes,"sources":[{"id":source,"label":"VRChat公開API（説明・タグは作者の記載。体験と軽さは未確認）","url":format!("https://api.vrchat.cloud/api/1/worlds/{id}")}]}),
    )
}
pub fn image_url(value: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    let parts: Vec<&str> = url.path_segments().map(|p| p.collect()).unwrap_or_default();
    url.scheme() == "https"
        && url.host_str() == Some("api.vrchat.cloud")
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && parts.len() == 6
        && parts[..3] == ["api", "1", "file"]
        && parts[3]
            .strip_prefix("file_")
            .is_some_and(|id| store::official_url(&format!("wrld_{id}")).is_some())
        && parts[4].parse::<u32>().is_ok()
        && parts[5] == "file"
}
pub fn wait_short(state: &ApiState) -> Result<(), String> {
    let remaining = state.next_request.saturating_sub(now());
    if remaining > 2 {
        return Err(format!(
            "APIの待機時間です。{remaining}秒後に再試行してください"
        ));
    }
    if remaining > 0 {
        std::thread::sleep(std::time::Duration::from_secs(remaining));
    }
    Ok(())
}
pub fn picture(url: &str, contact: &str, state: &mut ApiState) -> Result<Vec<u8>, String> {
    preferences::validate_contact(contact)?;
    if !image_url(url) {
        return Err("公開画像URLの形式が不正です".into());
    }
    wait_short(state)?;
    state.next_request = now() + 2;
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent(format!("WorldCompass/0.2 {contact}"))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let u = attempt.url();
            if attempt.previous().len() > 4
                || u.scheme() != "https"
                || ![Some("api.vrchat.cloud"), Some("files.vrchat.cloud")].contains(&u.host_str())
                || u.port_or_known_default() != Some(443)
                || !u.username().is_empty()
                || u.password().is_some()
            {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(url)
        .send()
        .map_err(|_| "画像を取得できません。通信を確認してください")?;
    if response.status().as_u16() == 429 {
        let delay = response
            .headers()
            .get("Retry-After")
            .and_then(|h| h.to_str().ok())
            .and_then(|h| retry_after(h, now()))
            .unwrap_or(60)
            .max(2);
        state.next_request = now().saturating_add(delay);
        return Err(format!(
            "画像APIのアクセス制限です。{delay}秒後に再試行してください"
        ));
    }
    if !response.status().is_success() {
        state.next_request = now() + 30;
        return Err("公開画像を取得できませんでした".into());
    }
    let mut bytes = Vec::new();
    response
        .take(5 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 5 * 1024 * 1024 || !valid_picture(&bytes) {
        return Err("対応していない画像、または画像サイズの上限です".into());
    }
    Ok(bytes)
}
fn valid_picture(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(&[0xff, 0xd8, 0xff])
        || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"))
}
fn timestamp_string(timestamp: u64) -> String {
    // SQLite supplies a UTC ISO timestamp without adding another time dependency.
    let db = Connection::open_in_memory().unwrap();
    db.query_row(
        "SELECT strftime('%Y-%m-%dT%H:%M:%SZ',?1,'unixepoch')",
        params![timestamp as i64],
        |r| r.get(0),
    )
    .unwrap()
}
pub fn cached(db: &Connection, id: &str) -> Option<Value> {
    db.query_row(
        "SELECT record FROM personal.worlds WHERE id=?1",
        params![id],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .and_then(|s| serde_json::from_str(&s).ok())
}
pub fn fresh(db: &Connection, id: &str) -> bool {
    db.query_row(
        "SELECT fetched FROM personal.worlds WHERE id=?1",
        params![id],
        |r| r.get::<_, i64>(0),
    )
    .is_ok_and(|t| now().saturating_sub(t as u64) < 24 * 3600)
}
pub fn save(db: &Connection, w: &Value) -> Result<(), String> {
    store::validate_catalog(
        &json!({"schemaVersion":1,"version":"personal-v1","fixture":false,"worlds":[w]}),
    )?;
    let id = w["worldId"].as_str().unwrap();
    db.execute(
        "INSERT OR REPLACE INTO personal.worlds(id,record,fetched) VALUES (?1,?2,?3)",
        params![id, w.to_string(), now() as i64],
    )
    .map_err(|e| e.to_string())?;
    db.execute("DELETE FROM vectors WHERE id=?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}
pub struct ApiState {
    pub next_request: u64,
}
pub fn retry_after(value: &str, timestamp: u64) -> Option<u64> {
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(seconds);
    }
    let parts: Vec<&str> = value.split_whitespace().collect();
    if parts.len() != 6 || parts[5] != "GMT" {
        return None;
    }
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|m| *m == parts[2])?
        + 1;
    let day = parts[1].parse::<u8>().ok()?;
    let year = parts[3].parse::<u16>().ok()?;
    if !(1..=31).contains(&day) || parts[4].len() != 8 {
        return None;
    }
    let iso = format!("{year:04}-{month:02}-{day:02} {}", parts[4]);
    let db = Connection::open_in_memory().ok()?;
    let target: i64 = db
        .query_row("SELECT unixepoch(?1)", params![iso], |r| r.get(0))
        .ok()?;
    Some((target.max(0) as u64).saturating_sub(timestamp))
}
pub fn fetch(id: &str, contact: &str, state: &mut ApiState) -> Result<Value, String> {
    preferences::validate_contact(contact)?;
    if state.next_request > now() {
        return Err(format!(
            "APIの待機時間です。{}秒後に再試行してください",
            state.next_request - now()
        ));
    }
    state.next_request = now() + 2;
    let response = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(20))
        .user_agent(format!("WorldCompass/0.2 {contact}"))
        .build()
        .map_err(|e| e.to_string())?
        .get(format!("https://api.vrchat.cloud/api/1/worlds/{id}"))
        .send()
        .map_err(|_| {
            state.next_request = now() + 30;
            "通信できませんでした。保存済みのワールドはオフラインでも使えます".to_string()
        })?;
    match response.status().as_u16() {
        200 => {}
        401 | 403 => return Err("この情報は匿名取得できません。認証の回避は行いません".into()),
        404 => return Err("公開ワールドが見つかりません。保存済み情報は保持しています".into()),
        429 => {
            let delay = response
                .headers()
                .get("Retry-After")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| retry_after(s, now()))
                .unwrap_or(60);
            state.next_request = now().saturating_add(delay.max(2));
            return Err(format!(
                "APIのアクセス制限です。{}秒後に再試行してください",
                delay.max(2)
            ));
        }
        _ => {
            state.next_request = now() + 30;
            return Err("APIでエラーが発生しました。しばらくして再試行してください".into());
        }
    }
    let mut body = Vec::new();
    response
        .take(512 * 1024 + 1)
        .read_to_end(&mut body)
        .map_err(|e| e.to_string())?;
    if body.len() > 512 * 1024 {
        return Err("API応答が大きすぎます".into());
    }
    let raw: Value = serde_json::from_slice(&body).map_err(|_| "API応答の形式が不正です")?;
    if raw["id"] != id {
        return Err("API応答のワールドIDが一致しません".into());
    }
    normalize(&raw, now())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn url_boundary() {
        let id = "wrld_4432ea9b-729c-46e3-8eaf-846aa0a37fdd";
        assert_eq!(
            world_id(&format!("https://vrchat.com/home/launch?worldId={id}")).unwrap(),
            id
        );
        assert_eq!(world_id(&format!("https://vrch.at/{id}")).unwrap(), id);
        assert!(world_id(&format!("https://evil.example/{id}")).is_err());
        assert!(world_id("https://vrchat.com@evil.example/home/world/wrld_fake").is_err());
        assert_eq!(retry_after("120", 0), Some(120));
        assert_eq!(
            retry_after("Thu, 08 Oct 2026 00:00:10 GMT", 1791417600),
            Some(10)
        );
        assert_eq!(retry_after("garbage", 0), None);
        let picture =
            "https://api.vrchat.cloud/api/1/file/file_c2b7ad18-5276-4e9b-aef7-8e18555e5030/26/file";
        assert!(image_url(picture));
        assert!(!image_url(
            &picture.replace("api.vrchat.cloud", "evil.example")
        ));
        assert!(!image_url("https://api.vrchat.cloud/api/1/auth/user"));
        assert!(valid_picture(b"\x89PNG\r\n\x1a\n123"));
        assert!(!valid_picture(b"<svg onload='evil'/>"));
    }
    #[test]
    fn anonymous_unknown_and_personal() {
        let w=normalize(&json!({"id":"wrld_4432ea9b-729c-46e3-8eaf-846aa0a37fdd","name":"Test","releaseStatus":"public","description":"Rain chill","tags":["author_tag_rain"],"capacity":16,"unityPackages":[]}),now()).unwrap();
        assert!(w["platforms"].is_null());
        assert!(w["attributes"]["rain"]["value"].is_null());
        let t = tempfile::tempdir().unwrap();
        let db = store::open(t.path()).unwrap();
        save(&db, &w).unwrap();
        assert!(fresh(&db, w["worldId"].as_str().unwrap()));
        assert_eq!(cached(&db, w["worldId"].as_str().unwrap()).unwrap(), w);
        assert!(store::load_for(&db, "demo").unwrap()["catalog"]["worlds"]
            .as_array()
            .unwrap()
            .iter()
            .all(|w| w["worldId"].as_str().unwrap().starts_with("fixture-")));
        assert_eq!(
            store::load_for(&db, "personal").unwrap()["catalog"]["worlds"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let chat = json!({"id":"test","title":"Test","updated":1,"turns":[]});
        store::save_chats(&db, &[chat.clone()]).unwrap();
        assert_eq!(store::load_chats(&db).unwrap(), vec![chat.clone()]);
        assert!(store::save_chats(&db, &[chat.clone(), chat.clone()]).is_err());
        assert_eq!(store::load_chats(&db).unwrap(), vec![chat]);
    }
}

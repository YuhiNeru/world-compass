use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::path::Path;
pub const FIXTURE: &str = include_str!("../../../../fixtures/catalog/worlds.json");
pub fn load_chats(db: &Connection) -> Result<Vec<Value>, String> {
    let mut statement = db
        .prepare("SELECT record FROM personal.chats ORDER BY updated DESC LIMIT 40")
        .map_err(|e| e.to_string())?;
    let result = statement
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .map(|r| serde_json::from_str(&r.map_err(|e| e.to_string())?).map_err(|e| e.to_string()))
        .collect();
    result
}
pub fn save_chats(db: &Connection, chats: &[Value]) -> Result<(), String> {
    if chats.len() > 40
        || serde_json::to_vec(chats).map_err(|e| e.to_string())?.len() > 16 * 1024 * 1024
    {
        return Err("会話履歴の保存上限です".into());
    }
    let mut seen = std::collections::HashSet::new();
    for chat in chats {
        if chat["id"]
            .as_str()
            .is_none_or(|s| s.len() > 64 || s.is_empty() || !seen.insert(s))
            || chat["title"]
                .as_str()
                .is_none_or(|s| s.chars().count() > 120)
            || chat["updated"].as_i64().is_none()
            || chat["turns"].as_array().is_none_or(|t| t.len() > 200)
        {
            return Err("会話履歴の形式が不正です".into());
        }
    }
    db.execute_batch("SAVEPOINT chats")
        .map_err(|e| e.to_string())?;
    let result = (|| {
        db.execute("DELETE FROM personal.chats", [])
            .map_err(|e| e.to_string())?;
        for chat in chats {
            db.execute(
                "INSERT INTO personal.chats(id,record,updated) VALUES (?1,?2,?3)",
                params![
                    chat["id"].as_str(),
                    chat.to_string(),
                    chat["updated"].as_i64()
                ],
            )
            .map_err(|e| e.to_string())?;
        }
        Ok::<(), String>(())
    })();
    if let Err(e) = result {
        let _ = db.execute_batch("ROLLBACK TO chats; RELEASE chats");
        return Err(e);
    }
    db.execute_batch("RELEASE chats").map_err(|e| e.to_string())
}
pub fn open(root: &Path) -> Result<Connection, String> {
    std::fs::create_dir_all(root.join("catalog")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(root.join("userdata")).map_err(|e| e.to_string())?;
    let db = Connection::open(root.join("catalog/catalog.sqlite")).map_err(|e| e.to_string())?;
    let schema: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if schema > 1 {
        return Err("newer catalog database schema; data was left unchanged".into());
    }
    db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY,value TEXT NOT NULL); CREATE TABLE IF NOT EXISTS worlds (id TEXT PRIMARY KEY, record TEXT NOT NULL); CREATE VIRTUAL TABLE IF NOT EXISTS world_fts USING fts5(id UNINDEXED,text,tokenize='trigram'); CREATE TABLE IF NOT EXISTS vectors (id TEXT PRIMARY KEY, model TEXT NOT NULL, vector TEXT NOT NULL); PRAGMA user_version=1;").map_err(|e|e.to_string())?;
    let personal = root.join("userdata/personal.sqlite");
    db.execute(
        "ATTACH DATABASE ?1 AS personal",
        params![personal.to_string_lossy()],
    )
    .map_err(|e| e.to_string())?;
    let personal_schema: i64 = db
        .query_row("PRAGMA personal.user_version", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if personal_schema > 1 {
        return Err("newer personal database schema; data was left unchanged".into());
    }
    db.execute_batch("CREATE TABLE IF NOT EXISTS personal.library(id TEXT PRIMARY KEY,value TEXT NOT NULL); CREATE TABLE IF NOT EXISTS personal.worlds(id TEXT PRIMARY KEY,record TEXT NOT NULL,fetched INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS personal.chats(id TEXT PRIMARY KEY,record TEXT NOT NULL,updated INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS personal.images(id TEXT PRIMARY KEY,url TEXT NOT NULL,bytes BLOB NOT NULL); CREATE TABLE IF NOT EXISTS personal.translations(cache_key TEXT PRIMARY KEY,text TEXT NOT NULL); PRAGMA personal.user_version=1;").map_err(|e|e.to_string())?;
    let count: i64 = db
        .query_row("SELECT COUNT(*) FROM meta WHERE key='version'", [], |r| {
            r.get(0)
        })
        .map_err(|e| e.to_string())?;
    if count == 0 {
        replace_catalog(&db, &serde_json::from_str(FIXTURE).unwrap())?;
    }
    Ok(db)
}
pub fn validate_catalog(c: &Value) -> Result<(), String> {
    if !keys(c, &["schemaVersion", "version", "fixture", "worlds"])
        || c["version"].as_str().is_none_or(|v| {
            v.is_empty()
                || !v
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        })
    {
        return Err("invalid catalog fields".into());
    }
    if c["schemaVersion"] != 1 || !c["version"].is_string() || !c["fixture"].is_boolean() {
        return Err("unsupported catalog schema".into());
    }
    let worlds = c["worlds"].as_array().ok_or("worlds required")?;
    if worlds.len() > 100000 {
        return Err("catalog too large".into());
    }
    let mut ids = std::collections::HashSet::new();
    for w in worlds {
        if !keys(
            w,
            &[
                "schemaVersion",
                "worldId",
                "name",
                "description",
                "author",
                "tags",
                "platforms",
                "capacity",
                "contentWarnings",
                "thumbnail",
                "sourceUrl",
                "fetchedAt",
                "sourceLicense",
                "permissionRef",
                "deleted",
                "attributes",
                "sources",
            ],
        ) || !optional_string(&w["description"])
            || !optional_string(&w["author"])
            || !strings(&w["tags"])
            || !(w["contentWarnings"].is_null() || strings(&w["contentWarnings"]))
            || !url(&w["sourceUrl"])
            || w["fetchedAt"]
                .as_str()
                .is_none_or(|d| d.len() < 20 || !d.ends_with('Z') || !d.contains('T'))
        {
            return Err("invalid world fields".into());
        }
        if !w["thumbnail"].is_null()
            && (!keys(&w["thumbnail"], &["url", "license"])
                || !url(&w["thumbnail"]["url"])
                || w["thumbnail"]["license"]
                    .as_str()
                    .is_none_or(|v| v.is_empty()))
        {
            return Err("invalid thumbnail".into());
        }
        let id = w["worldId"].as_str().ok_or("world ID required")?;
        if !ids.insert(id)
            || w["schemaVersion"] != 1
            || w["name"]
                .as_str()
                .is_none_or(|n| n.is_empty() || n.chars().count() > 200)
            || w["sourceLicense"].as_str().is_none_or(|n| n.is_empty())
            || w["permissionRef"].as_str().is_none_or(|n| n.is_empty())
            || !w["deleted"].is_boolean()
        {
            return Err("invalid or unlicensed world".into());
        }
        if c["fixture"].as_bool().unwrap() != id.starts_with("fixture-") {
            return Err("fixture boundary violation".into());
        }
        if id.starts_with("fixture-")
            && (id.len() <= 8
                || !id[8..]
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'))
        {
            return Err("invalid fixture ID".into());
        }
        if !id.starts_with("fixture-") && official_url(id).is_none() {
            return Err("invalid production ID".into());
        }
        if !w["platforms"].is_null()
            && w["platforms"]
                .as_array()
                .is_none_or(|a| a.iter().any(|p| p != "pc" && p != "quest"))
        {
            return Err("invalid platforms".into());
        }
        if !w["capacity"].is_null() && w["capacity"].as_u64().is_none_or(|n| n == 0) {
            return Err("invalid capacity".into());
        }
        let sources = w["sources"].as_array().ok_or("sources required")?;
        for s in sources {
            if !keys(s, &["id", "label", "url"])
                || !s["id"].is_string()
                || !s["label"].is_string()
                || !url(&s["url"])
            {
                return Err("invalid source".into());
            }
        }
        if !keys(
            &w["attributes"],
            &["rain", "night", "chill", "photo", "nature", "game", "light"],
        ) {
            return Err("invalid attributes".into());
        }
        for a in w["attributes"]
            .as_object()
            .ok_or("attributes required")?
            .values()
        {
            if !keys(a, &["value", "sourceId"])
                || a["sourceId"].as_str().is_none_or(|s| s.is_empty())
                || !(a["value"].is_boolean() || a["value"].is_null())
                || !sources.iter().any(|s| s["id"] == a["sourceId"])
            {
                return Err("invalid attribute evidence".into());
            }
        }
    }
    Ok(())
}
pub fn keys(v: &Value, fields: &[&str]) -> bool {
    v.as_object()
        .is_some_and(|m| m.len() == fields.len() && fields.iter().all(|k| m.contains_key(*k)))
}
fn optional_string(v: &Value) -> bool {
    v.is_null() || v.is_string()
}
fn strings(v: &Value) -> bool {
    v.as_array()
        .is_some_and(|a| a.iter().all(|s| s.is_string()))
}
fn url(v: &Value) -> bool {
    v.as_str().is_some_and(|s| reqwest::Url::parse(s).is_ok())
}
pub fn replace_catalog(db: &Connection, c: &Value) -> Result<(), String> {
    validate_catalog(c)?;
    db.execute_batch("SAVEPOINT catalog_update")
        .map_err(|e| e.to_string())?;
    let result = (|| {
        db.execute_batch("DELETE FROM worlds; DELETE FROM world_fts; DELETE FROM vectors;")?;
        for w in c["worlds"].as_array().unwrap() {
            let id = w["worldId"].as_str().unwrap();
            db.execute(
                "INSERT INTO worlds VALUES (?1,?2)",
                params![id, w.to_string()],
            )?;
            if w["deleted"] != true {
                db.execute(
                    "INSERT INTO world_fts VALUES (?1,?2)",
                    params![
                        id,
                        format!("{} {} {}", w["name"], w["description"], w["tags"])
                    ],
                )?;
            }
        }
        db.execute(
            "INSERT OR REPLACE INTO meta VALUES ('version',?1)",
            params![c["version"].as_str()],
        )?;
        db.execute(
            "INSERT OR REPLACE INTO meta VALUES ('fixture',?1)",
            params![c["fixture"].to_string()],
        )?;
        Ok::<_, rusqlite::Error>(())
    })();
    match result {
        Ok(()) => db.execute_batch("RELEASE catalog_update"),
        Err(e) => {
            let _ = db.execute_batch("ROLLBACK TO catalog_update; RELEASE catalog_update");
            return Err(e.to_string());
        }
    }
    .map_err(|e| e.to_string())
}
pub fn load(db: &Connection) -> Result<Value, String> {
    let mut stmt = db
        .prepare("SELECT record FROM worlds")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    let worlds: Result<Vec<Value>, String> = rows
        .map(|r| {
            r.map_err(|e| e.to_string())
                .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
        })
        .collect();
    let version: String = db
        .query_row("SELECT value FROM meta WHERE key='version'", [], |r| {
            r.get(0)
        })
        .map_err(|e| e.to_string())?;
    let fixture: String = db
        .query_row("SELECT value FROM meta WHERE key='fixture'", [], |r| {
            r.get(0)
        })
        .map_err(|e| e.to_string())?;
    let mut personal = serde_json::Map::new();
    let mut stmt = db
        .prepare("SELECT id,value FROM personal.library")
        .map_err(|e| e.to_string())?;
    for r in stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?
    {
        let (id, v) = r.map_err(|e| e.to_string())?;
        personal.insert(id, serde_json::from_str(&v).map_err(|e| e.to_string())?);
    }
    Ok(
        json!({"catalog":{"schemaVersion":1,"version":version,"fixture":fixture=="true","worlds":worlds?},"personal":personal}),
    )
}
pub fn save(db: &Connection, id: &str, value: &Value) -> Result<(), String> {
    if value.as_object().is_none_or(|m| m.len() != 4)
        || !["favorite", "visited", "excluded"]
            .iter()
            .all(|k| value[k].is_boolean())
        || value["note"]
            .as_str()
            .is_none_or(|s| s.chars().count() > 4000)
    {
        return Err("invalid personal data".into());
    }
    let exists: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM worlds WHERE id=?1 UNION SELECT 1 FROM personal.worlds WHERE id=?1)",
            params![id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !exists {
        return Err("unknown ID".into());
    }
    db.execute(
        "INSERT OR REPLACE INTO personal.library VALUES (?1,?2)",
        params![id, value.to_string()],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}
pub fn load_for(db: &Connection, mode: &str) -> Result<Value, String> {
    let mut data = load(db)?;
    if mode == "personal" {
        let mut stmt = db
            .prepare("SELECT record FROM personal.worlds")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        let worlds = rows
            .map(|r| {
                r.map_err(|e| e.to_string())
                    .and_then(|s| serde_json::from_str::<Value>(&s).map_err(|e| e.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        data["catalog"] =
            json!({"schemaVersion":1,"version":"personal-v1","fixture":false,"worlds":worlds});
    }
    Ok(data)
}
pub fn official_url(id: &str) -> Option<String> {
    let v = id.strip_prefix("wrld_")?;
    if v.len() != 36 {
        return None;
    }
    for (i, b) in v.bytes().enumerate() {
        if [8, 13, 18, 23].contains(&i) {
            if b != b'-' {
                return None;
            }
        } else if !b.is_ascii_hexdigit() {
            return None;
        }
    }
    Some(format!("https://vrchat.com/home/world/{id}"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rollback_and_personal() {
        let t = tempfile::tempdir().unwrap();
        let db = open(t.path()).unwrap();
        save(
            &db,
            "fixture-rain-room",
            &json!({"favorite":true,"visited":false,"excluded":false,"note":"local"}),
        )
        .unwrap();
        let before = load(&db).unwrap();
        let mut bad = before["catalog"].clone();
        bad["worlds"][0]["sourceLicense"] = json!("");
        assert!(replace_catalog(&db, &bad).is_err());
        assert_eq!(load(&db).unwrap(), before);
        let mut removed = before["catalog"].clone();
        removed["worlds"][0]["deleted"] = json!(true);
        replace_catalog(&db, &removed).unwrap();
        assert_eq!(load(&db).unwrap()["personal"], before["personal"]);
    }
    #[test]
    fn urls() {
        assert!(official_url("fixture-rain-room").is_none());
        assert!(official_url("wrld_00000000-0000-0000-0000-000000000000").is_some());
        assert!(official_url("wrld_../../evil").is_none());
    }
}

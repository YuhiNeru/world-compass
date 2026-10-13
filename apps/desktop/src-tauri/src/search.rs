use rusqlite::{params, Connection};
use serde_json::{json, Value};
#[cfg(test)]
pub fn candidates(db: &Connection, query: &str, c: &Value) -> Result<Value, String> {
    candidates_for(db, query, c, "demo")
}
pub fn candidates_for(
    db: &Connection,
    query: &str,
    c: &Value,
    mode: &str,
) -> Result<Value, String> {
    let table = if mode == "personal" {
        "personal.worlds"
    } else {
        "worlds"
    };
    let mut sql = format!("SELECT id FROM {table} WHERE json_extract(record,'$.deleted')=0");
    // Attribute names come exclusively from the validated enum; query values remain bound parameters.
    if let Some(n) = c["people"].as_u64() {
        sql.push_str(&format!(" AND (json_extract(record,'$.capacity') IS NULL OR json_extract(record,'$.capacity')>={n})"));
    }
    if let Some(platform) = c["platform"].as_str() {
        if !["pc", "quest"].contains(&platform) {
            return Err("invalid platform".into());
        }
        sql.push_str(&format!(" AND (json_extract(record,'$.platforms') IS NULL OR EXISTS(SELECT 1 FROM json_each(record,'$.platforms') WHERE value='{platform}'))"));
    }
    for (kind, wrong) in [("required", 0), ("excluded", 1)] {
        for a in c[kind].as_array().ok_or("invalid conditions")? {
            let a = a.as_str().ok_or("invalid attribute")?;
            if !["rain", "night", "chill", "photo", "nature", "game", "light"].contains(&a) {
                return Err("invalid attribute".into());
            }
            sql.push_str(&format!(" AND (json_extract(record,'$.attributes.{a}.value') IS NULL OR json_extract(record,'$.attributes.{a}.value')!={wrong})"));
        }
    }
    let mut stmt = db.prepare(&sql).map_err(|e| e.to_string())?;
    let ids = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let terms: Vec<&str> = query
        .split_whitespace()
        .filter(|s| s.chars().count() >= 3)
        .take(8)
        .collect();
    let mut weights = serde_json::Map::new();
    if mode == "personal" {
        return Ok(json!({"ids":ids,"weights":weights}));
    }
    for term in terms {
        let escaped = format!("\"{}\"", term.replace('"', "\"\""));
        let mut fts = db
            .prepare("SELECT id FROM world_fts WHERE world_fts MATCH ?1 LIMIT 100")
            .map_err(|e| e.to_string())?;
        for id in fts
            .query_map(params![escaped], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
        {
            let id = id.map_err(|e| e.to_string())?;
            weights.insert(id, json!(0.5));
        }
    }
    Ok(json!({"ids":ids,"weights":weights}))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sqlite_conditions_and_fts() {
        let t = tempfile::tempdir().unwrap();
        let db = crate::store::open(t.path()).unwrap();
        let c =
            json!({"people":2,"platform":"quest","required":["rain"],"excluded":[],"preferred":[]});
        let v = candidates(&db, "小さな部屋", &c).unwrap();
        assert!(v["ids"]
            .as_array()
            .unwrap()
            .contains(&json!("fixture-rain-room")));
        assert!(!v["ids"]
            .as_array()
            .unwrap()
            .contains(&json!("fixture-rain-station")));
        assert_eq!(v["weights"]["fixture-rain-room"], 0.5);
    }
}

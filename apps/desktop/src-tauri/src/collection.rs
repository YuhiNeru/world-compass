//! Resumable public-world discovery. Categories are search provenance, not verified world attributes.
use super::*;
use rusqlite::OptionalExtension;

const PLAN: &str = include_str!("../../../../resources/japanese-collection-plan.json");
pub(super) fn plan() -> Value {
    serde_json::from_str(PLAN).expect("bundled collection plan")
}
pub(super) fn schema(db: &Connection) -> Result<(), String> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS personal.collection_queries(key TEXT PRIMARY KEY,category TEXT NOT NULL,rank INTEGER NOT NULL,kind TEXT NOT NULL,value TEXT NOT NULL,offset INTEGER NOT NULL DEFAULT 0,state TEXT NOT NULL DEFAULT 'pending');
    CREATE TABLE IF NOT EXISTS personal.collection_pages(query_key TEXT NOT NULL,offset INTEGER NOT NULL,fingerprint TEXT NOT NULL,PRIMARY KEY(query_key,offset));
    CREATE TABLE IF NOT EXISTS personal.collection_memberships(world_id TEXT NOT NULL,category TEXT NOT NULL,query_key TEXT NOT NULL,PRIMARY KEY(world_id,query_key));
    CREATE TABLE IF NOT EXISTS personal.collection_rejections(world_id TEXT PRIMARY KEY,reason TEXT NOT NULL);").map_err(|e|e.to_string())
}
fn insert_query(
    db: &Connection,
    key: &str,
    category: &str,
    rank: i64,
    kind: &str,
    value: &str,
) -> Result<(), String> {
    db.execute("INSERT OR IGNORE INTO personal.collection_queries(key,category,rank,kind,value) VALUES (?1,?2,?3,?4,?5)",params![key,category,rank,kind,value]).map_err(|e|e.to_string())?;
    Ok(())
}
pub(super) fn seed(db: &Connection) -> Result<(), String> {
    schema(db)?;
    let p = plan();
    for c in p["categories"].as_array().unwrap() {
        let id = c["id"].as_str().unwrap();
        let label = c["label"].as_str().unwrap();
        let rank = c["priority"].as_i64().unwrap();
        for (field, kind) in [("terms", "search"), ("tags", "tag")] {
            for word in c[field].as_array().unwrap() {
                let word = word.as_str().unwrap();
                insert_query(db, &format!("{id}:{kind}:{word}"), label, rank, kind, word)?;
            }
        }
    }
    // Include voiced/small kana, rather than only the basic alphabet.
    if p["kanaSweep"] == true {
        for code in (0x3041..=0x3096).chain(0x30a1..=0x30fa) {
            let word = char::from_u32(code).unwrap().to_string();
            insert_query(
                db,
                &format!("kana:{word}"),
                "日本語文字の補完",
                8,
                "search",
                &word,
            )?;
        }
    }
    Ok(())
}
#[derive(Debug)]
struct Query {
    key: String,
    category: String,
    rank: i64,
    kind: String,
    value: String,
    offset: i64,
}
fn next_query(db: &Connection) -> Result<Option<Query>, String> {
    db.query_row("SELECT key,category,rank,kind,value,offset FROM personal.collection_queries WHERE state='pending' ORDER BY rank,rowid LIMIT 1",[],|r|Ok(Query{key:r.get(0)?,category:r.get(1)?,rank:r.get(2)?,kind:r.get(3)?,value:r.get(4)?,offset:r.get(5)?})).optional().map_err(|e|e.to_string())
}
fn path(q: &Query) -> String {
    let mut url = reqwest::Url::parse("https://api.vrchat.cloud/api/1/worlds").unwrap();
    url.query_pairs_mut()
        .append_pair("n", "100")
        .append_pair("offset", &q.offset.to_string())
        .append_pair("sort", "favorites")
        .append_pair("order", "descending")
        .append_pair("releaseStatus", "public")
        .append_pair("fuzzy", "false")
        .append_pair(
            if q.kind == "creator" {
                "userId"
            } else if q.kind == "tag" {
                "tag"
            } else {
                "search"
            },
            &if q.kind == "tag" {
                format!("author_tag_{}", q.value)
            } else {
                q.value.clone()
            },
        );
    format!("worlds?{}", url.query().unwrap())
}
fn forced(q: &Query) -> bool {
    q.kind == "creator"
        || (q.kind == "tag" && ["jp", "japanese", "japan", "jpn"].contains(&q.value.as_str()))
}
fn valid_author(id: &str) -> bool {
    id.strip_prefix("usr_")
        .is_some_and(|v| store::official_url(&format!("wrld_{v}")).is_some())
}
fn add_creator(db: &Connection, raw: &Value) -> Result<(), String> {
    if plan()["creatorExpansion"] == true && japanese_priority(raw) > 0 {
        if let Some(id) = raw["authorId"].as_str().filter(|id| valid_author(id)) {
            insert_query(
                db,
                &format!("creator:{id}"),
                "関連作者の公開作品",
                9,
                "creator",
                id,
            )?;
        }
    }
    Ok(())
}
fn page(db: &Connection, q: &Query, raw: &Value) -> Result<(), String> {
    let rows = raw.as_array().ok_or("一覧APIの応答が不正です")?;
    let mut ids: Vec<&str> = rows.iter().filter_map(|w| w["id"].as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    let fingerprint = ids.join(",");
    let repeated:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM personal.collection_pages WHERE query_key=?1 AND fingerprint=?2)",params![q.key,fingerprint],|r|r.get(0)).map_err(|e|e.to_string())?;
    let transaction = db.unchecked_transaction().map_err(|e| e.to_string())?;
    if !repeated {
        for w in rows {
            if w["releaseStatus"] != "public" {
                continue;
            }
            if q.kind == "creator" && w["authorId"].as_str() != Some(q.value.as_str()) {
                continue;
            }
            if q.kind == "tag"
                && !w["tags"].as_array().is_some_and(|tags| {
                    tags.iter().any(|tag| {
                        tag.as_str().is_some_and(|tag| {
                            tag.eq_ignore_ascii_case(&format!("author_tag_{}", q.value))
                        })
                    })
                })
            {
                continue;
            }
            let Some(id) = w["id"]
                .as_str()
                .filter(|id| store::official_url(id).is_some())
            else {
                continue;
            };
            let hint = japanese_priority(w);
            // English genre tags are broad: avoid importing all non-Japanese genre results.
            if q.kind == "tag" && !forced(q) && hint == 0 {
                continue;
            }
            transaction
                .execute(
                    "INSERT OR IGNORE INTO personal.collection_memberships VALUES (?1,?2,?3)",
                    params![id, q.category, q.key],
                )
                .map_err(|e| e.to_string())?;
            let priority = hint + if forced(q) { 20 } else { 0 };
            transaction.execute("INSERT OR IGNORE INTO personal.import_queue(id,state,priority) VALUES (?1,'queued',?2)",params![id,priority]).map_err(|e|e.to_string())?;
            transaction.execute("UPDATE personal.import_queue SET priority=MAX(priority,?2),state=CASE WHEN state='filtered' AND ?2>=20 THEN 'queued' ELSE state END WHERE id=?1",params![id,priority]).map_err(|e|e.to_string())?;
            add_creator(&transaction, w)?;
        }
        transaction
            .execute(
                "INSERT INTO personal.collection_pages VALUES (?1,?2,?3)",
                params![q.key, q.offset, fingerprint],
            )
            .map_err(|e| e.to_string())?;
    }
    // A repeated page is recorded as limited, never claimed to be an exhaustive traversal.
    let state = if repeated {
        "limited"
    } else if rows.len() < 100 {
        "done"
    } else {
        "pending"
    };
    transaction
        .execute(
            "UPDATE personal.collection_queries SET offset=?2,state=?3 WHERE key=?1",
            params![q.key, q.offset + rows.len() as i64, state],
        )
        .map_err(|e| e.to_string())?;
    transaction.commit().map_err(|e| e.to_string())
}
pub(super) fn stats(db: &Connection) -> Result<(usize, usize, usize), String> {
    db.query_row("SELECT COUNT(*),COALESCE(SUM(state!='pending'),0),COALESCE(SUM(state='limited'),0) FROM personal.collection_queries",[],|r|Ok((r.get::<_,u32>(0)? as usize,r.get::<_,u32>(1)? as usize,r.get::<_,u32>(2)? as usize))).map_err(|e|e.to_string())
}
fn reject_record(db: &Connection, id: &str, reason: &str) -> Result<(), String> {
    let transaction = db.unchecked_transaction().map_err(|e| e.to_string())?;
    transaction.execute("INSERT OR REPLACE INTO personal.collection_rejections VALUES (?1,?2)", params![id, reason]).map_err(|e| e.to_string())?;
    transaction.execute("UPDATE personal.import_queue SET state='skipped' WHERE id=?1", params![id]).map_err(|e| e.to_string())?;
    transaction.commit().map_err(|e| e.to_string())
}
pub(super) fn run(s: &AppState) -> Result<(), String> {
    seed(&*s.db.lock().map_err(|e| e.to_string())?)?;
    loop {
        cancelled(s)?;
        // Finish and index every saved page before progressing to lower-priority searches.
        loop {
            cancelled(s)?;
            let item:Option<(String,i32)>=s.db.lock().map_err(|e|e.to_string())?.query_row("SELECT id,priority FROM personal.import_queue WHERE state='queued' ORDER BY priority DESC,rowid LIMIT 1",[],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|e.to_string())?;
            let Some((id, priority)) = item else {
                break;
            };
            publish(
                s,
                "fetching",
                "日本語向け候補の説明・タグ・対応機種を追加しています",
            )?;
            match request(s, &format!("worlds/{id}")) {
                Ok(raw) => {
                    if raw["id"] != id {
                        return Err("API応答のワールドIDが一致しません".into());
                    }
                    let db = s.db.lock().map_err(|e| e.to_string())?;
                    if japanese_priority(&raw) == 0 && priority < 20 {
                        db.execute(
                            "UPDATE personal.import_queue SET state='filtered' WHERE id=?1",
                            params![id],
                        )
                        .map_err(|e| e.to_string())?;
                        db.execute("INSERT OR REPLACE INTO personal.collection_rejections VALUES (?1,'Japanese signal not found in public metadata')",params![id]).map_err(|e|e.to_string())?;
                    } else {
                        match vrchat::normalize(&raw, vrchat::now()) {
                            Ok(world) => {
                                add_creator(&db, &raw)?;
                                vrchat::save(&db, &world)?;
                                db.execute(
                                    "UPDATE personal.import_queue SET state='done' WHERE id=?1",
                                    params![id],
                                )
                                .map_err(|e| e.to_string())?;
                            }
                            // Invalid public metadata affects this record only. Transport,
                            // authentication and database failures still stop the worker.
                            Err(e) => reject_record(&db, &id, &e)?,
                        }
                    }
                }
                Err(e) if e.contains("公開ワールドが見つかりません") => skip(s, &id)?,
                Err(e) => return Err(e),
            }
        }
        index_pending(s)?;
        let q = next_query(&*s.db.lock().map_err(|e| e.to_string())?)?;
        let Some(q) = q else {
            return publish(
                s,
                "complete",
                "検索計画の巡回が終了しました。全世界の完全網羅を保証するものではありません",
            );
        };
        s.corpus.lock().map_err(|e| e.to_string())?.category = q.category.clone();
        publish(
            s,
            "discovering",
            &format!(
                "優先{}・{}：{}（{}件目から）を検索しています",
                q.rank, q.category, q.value, q.offset
            ),
        )?;
        let raw = match request(s, &path(&q)) {
            Err(error)
                if q.offset > 0
                    && (error.contains("（400）") || error == crate::auth::PAGINATION_LIMIT) =>
            {
                s.db.lock()
                    .map_err(|e| e.to_string())?
                    .execute(
                        "UPDATE personal.collection_queries SET state='limited' WHERE key=?1",
                        params![q.key],
                    )
                    .map_err(|e| e.to_string())?;
                publish(s,"waiting","APIが次の検索ページを受け付けませんでした。巡回制限として記録し、次の検索へ進みます")?;
                continue;
            }
            other => other?,
        };
        page(&*s.db.lock().map_err(|e| e.to_string())?, &q, &raw)?;
        s.corpus.lock().map_err(|e| e.to_string())?.page += 1;
        publish(
            s,
            "discovering",
            "検索ページを保存しました。新しい候補だけを追加します",
        )?;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_metadata_is_recorded_without_blocking_remaining_queue() {
        let t = tempfile::tempdir().unwrap();
        let db = store::open(t.path()).unwrap();
        super::super::schema(&db).unwrap();
        schema(&db).unwrap();
        db.execute("INSERT INTO personal.import_queue(id,state,priority) VALUES ('invalid','queued',20),('next','queued',1)", []).unwrap();
        let raw = json!({"id":"wrld_4432ea9b-729c-46e3-8eaf-846aa0a37fdd","releaseStatus":"public","name":""});
        let error = vrchat::normalize(&raw, vrchat::now()).unwrap_err();
        reject_record(&db, "invalid", &error).unwrap();
        let next: String = db.query_row("SELECT id FROM personal.import_queue WHERE state='queued' ORDER BY priority DESC,rowid LIMIT 1", [], |r| r.get(0)).unwrap();
        assert_eq!(next, "next");
        let reason: String = db.query_row("SELECT reason FROM personal.collection_rejections WHERE world_id='invalid'", [], |r| r.get(0)).unwrap();
        assert_eq!(reason, error);
        drop(db);
        let db = store::open(t.path()).unwrap();
        let state: String = db.query_row("SELECT state FROM personal.import_queue WHERE id='invalid'", [], |r| r.get(0)).unwrap();
        assert_eq!(state, "skipped");
    }
    #[test]
    fn priority_pagination_restart_and_repeated_pages() {
        let t = tempfile::tempdir().unwrap();
        let db = store::open(t.path()).unwrap();
        super::super::schema(&db).unwrap();
        seed(&db).unwrap();
        let q = next_query(&db).unwrap().unwrap();
        assert_eq!(q.rank, 1);
        assert_eq!(q.value, "チル");
        let url =
            reqwest::Url::parse(&format!("https://api.vrchat.cloud/api/1/{}", path(&q))).unwrap();
        assert!(url.query_pairs().any(|(k, v)| k == "search" && v == "チル"));
        let worlds:Vec<Value>=(1..=100).map(|i|json!({"id":format!("wrld_{}",uuid::Uuid::from_u128(i)),"releaseStatus":"public","name":"チル部屋","authorId":"usr_4432ea9b-729c-46e3-8eaf-846aa0a37fdd"})).collect();
        page(&db, &q, &json!(worlds)).unwrap();
        assert_eq!(counts(&db).unwrap().1, 100);
        let q = next_query(&db).unwrap().unwrap();
        assert_eq!(q.offset, 100);
        page(&db, &q, &json!(worlds)).unwrap();
        assert_eq!(counts(&db).unwrap().1, 100);
        assert_eq!(stats(&db).unwrap().2, 1);
        drop(db);
        let db = store::open(t.path()).unwrap();
        seed(&db).unwrap();
        assert_ne!(next_query(&db).unwrap().unwrap().key, q.key);
        assert_eq!(stats(&db).unwrap().2, 1);
        let creator: u32 = db
            .query_row(
                "SELECT COUNT(*) FROM personal.collection_queries WHERE kind='creator'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(creator, 1);
    }
    #[test]
    fn new_scope_accepts_more_than_one_thousand_and_filters_nonpublic() {
        let t = tempfile::tempdir().unwrap();
        let db = store::open(t.path()).unwrap();
        super::super::schema(&db).unwrap();
        seed(&db).unwrap();
        let mut q = next_query(&db).unwrap().unwrap();
        for batch in 0..12 {
            let rows:Vec<Value>=(1..=100).map(|i|json!({"id":format!("wrld_{}",uuid::Uuid::from_u128(batch*100+i)),"releaseStatus":"public","name":"チル"})).collect();
            page(&db, &q, &json!(rows)).unwrap();
            q.offset += 100;
        }
        assert_eq!(counts(&db).unwrap().1, 1200);
        let q = Query {
            key: "test-tag".into(),
            category: "test".into(),
            rank: 1,
            kind: "tag".into(),
            value: "chill".into(),
            offset: 0,
        };
        insert_query(&db, &q.key, &q.category, q.rank, &q.kind, &q.value).unwrap();
        page(&db,&q,&json!([{"id":format!("wrld_{}",uuid::Uuid::from_u128(9000)),"releaseStatus":"private","name":"チル"},{"id":format!("wrld_{}",uuid::Uuid::from_u128(9001)),"releaseStatus":"public","name":"English Chill"}])).unwrap();
        assert_eq!(counts(&db).unwrap().1, 1200);
        let q = Query {
            key: "creator-test".into(),
            category: "test".into(),
            rank: 9,
            kind: "creator".into(),
            value: "usr_4432ea9b-729c-46e3-8eaf-846aa0a37fdd".into(),
            offset: 0,
        };
        insert_query(&db, &q.key, &q.category, q.rank, &q.kind, &q.value).unwrap();
        page(&db,&q,&json!([
            {"id":format!("wrld_{}",uuid::Uuid::from_u128(9100)),"releaseStatus":"public","name":"別の作者の部屋","authorId":"usr_a729bf90-1220-423a-9630-7e60851e7475"},
            {"id":format!("wrld_{}",uuid::Uuid::from_u128(9101)),"releaseStatus":"public","name":"English Room","authorId":q.value}
        ])).unwrap();
        assert_eq!(counts(&db).unwrap().1, 1201);
        let q = Query {
            key: "jp-tag-test".into(),
            category: "test".into(),
            rank: 7,
            kind: "tag".into(),
            value: "jp".into(),
            offset: 0,
        };
        insert_query(&db, &q.key, &q.category, q.rank, &q.kind, &q.value).unwrap();
        page(&db,&q,&json!([
            {"id":format!("wrld_{}",uuid::Uuid::from_u128(9200)),"releaseStatus":"public","name":"Wrong Tag","tags":["author_tag_english"]},
            {"id":format!("wrld_{}",uuid::Uuid::from_u128(9201)),"releaseStatus":"public","name":"English Name","tags":["author_tag_JP"]}
        ])).unwrap();
        assert_eq!(counts(&db).unwrap().1, 1202);
    }
}

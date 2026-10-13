use crate::{preferences, setup, store, use_server, vrchat, AppState};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use serde_json::json;
use serde_json::Value;
use std::{path::Path, sync::atomic::Ordering, time::Duration};
use tauri::Manager;
const TARGET: usize = 1000;
#[path = "collection.rs"]
mod expanded;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Progress {
    pub running: bool,
    pub stage: String,
    pub completed: usize,
    pub indexed: usize,
    pub queued: usize,
    pub skipped: usize,
    pub page: usize,
    pub target: usize,
    pub message: String,
    pub bytes: u64,
    pub focus: String,
    pub expanded: bool,
    pub remote: bool,
    pub category: String,
    pub queries_done: usize,
    pub queries_total: usize,
    pub limited_queries: usize,
    pub automatic_resume: bool,
}
impl Default for Progress {
    fn default() -> Self {
        Self {
            running: false,
            stage: "ready".into(),
            completed: 0,
            indexed: 0,
            queued: 0,
            skipped: 0,
            page: 0,
            target: TARGET,
            message: "ログイン後、公開ワールド1,000件の取得を開始できます".into(),
            bytes: 0,
            focus: "japanese-v1".into(),
            expanded: false,
            remote: false,
            category: String::new(),
            queries_done: 0,
            queries_total: 0,
            limited_queries: 0,
            automatic_resume: false,
        }
    }
}
impl Progress {
    pub fn load(root: &Path) -> Self {
        let raw: Value = std::fs::read(root.join("userdata/corpus-progress.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or(Value::Null);
        let mut p: Self = serde_json::from_value(raw.clone()).unwrap_or_default();
        if !p.expanded && raw["focus"] != "japanese-v1" && p.stage != "complete" {
            p.page = 0;
        }
        p.focus = if p.expanded {
            "japanese-expanded-v1"
        } else {
            "japanese-v1"
        }
        .into();
        if p.running {
            p.running = false;
            p.stage = "paused".into();
            p.message = "前回の続きから再開できます".into();
        }
        if !p.expanded {
            p.target = TARGET;
        }
        p
    }
}
pub fn schema(db: &Connection) -> Result<(), String> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS personal.import_queue(id TEXT PRIMARY KEY,state TEXT NOT NULL,priority INTEGER NOT NULL DEFAULT 0);").map_err(|e|e.to_string())?;
    let priority_exists: bool = db
        .prepare("PRAGMA personal.table_info(import_queue)")
        .map_err(|e| e.to_string())?
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?
        .iter()
        .any(|name| name == "priority");
    if !priority_exists {
        db.execute_batch(
            "ALTER TABLE personal.import_queue ADD COLUMN priority INTEGER NOT NULL DEFAULT 0;",
        )
        .map_err(|e| e.to_string())?;
    }
    expanded::schema(db)
}
fn counts(db: &Connection) -> Result<(usize, usize, usize), String> {
    db.query_row("SELECT COALESCE(SUM(state='done'),0),COALESCE(SUM(state='queued'),0),COALESCE(SUM(state='skipped'),0) FROM personal.import_queue",[],|r|Ok((r.get::<_,u32>(0)? as usize,r.get::<_,u32>(1)? as usize,r.get::<_,u32>(2)? as usize))).map_err(|e|e.to_string())
}
fn publish(s: &AppState, stage: &str, message: &str) -> Result<(), String> {
    let db = s.db.lock().map_err(|e| e.to_string())?;
    let (done, queued, skipped) = counts(&db)?;
    let version = embedding_version();
    let indexed=db.query_row("SELECT COUNT(*) FROM vectors v JOIN personal.import_queue q ON v.id=q.id WHERE q.state='done' AND v.model=?1",params![version],|r|r.get::<_,u32>(0)).map_err(|e|e.to_string())? as usize;
    let (query_total, query_done, limited) = expanded::stats(&db)?;
    drop(db);
    let bytes = [
        "userdata/personal.sqlite",
        "catalog/catalog.sqlite",
        "userdata/personal.sqlite-wal",
        "catalog/catalog.sqlite-wal",
    ]
    .iter()
    .filter_map(|p| std::fs::metadata(s.root.join(p)).ok().map(|m| m.len()))
    .sum();
    let mut p = s.corpus.lock().map_err(|e| e.to_string())?;
    p.remote = remote_mode(&s.root);
    p.completed = done;
    p.queued = queued;
    p.skipped = skipped;
    p.indexed = indexed;
    if p.expanded {
        p.queries_total = query_total;
        p.queries_done = query_done;
        p.limited_queries = limited;
        p.target = p.completed + p.queued;
    }
    p.stage = stage.into();
    p.message = message.into();
    p.bytes = bytes;
    let temp = s.root.join("userdata/corpus-progress.tmp");
    std::fs::write(&temp, serde_json::to_vec(&*p).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    std::fs::rename(temp, s.root.join("userdata/corpus-progress.json")).map_err(|e| e.to_string())
}
fn embedding_version() -> String {
    setup::manifest()
        .assets
        .into_iter()
        .find(|a| a.id == "embedding")
        .unwrap()
        .version
}
fn cancelled(s: &AppState) -> Result<(), String> {
    if s.corpus_cancel.load(Ordering::SeqCst) {
        Err("取得を中断しました。保存済み情報から再開できます".into())
    } else {
        Ok(())
    }
}
fn request(s: &AppState, path: &str) -> Result<Value, String> {
    let mut failures = 0u32;
    loop {
        cancelled(s)?;
        let result = s
            .auth
            .lock()
            .map_err(|e| e.to_string())?
            .request(s, path, "GET", None, None, true);
        match result {
            Err(e) if e.starts_with("APIのアクセス制限") => {
                publish(s, "waiting", &e)?;
            }
            Err(e)
                if failures < 6
                    && (e.contains("接続できません")
                        || e.contains("読み取り")
                        || ["（500）", "（502）", "（503）", "（504）"]
                            .iter()
                            .any(|code| e.contains(code))) =>
            {
                failures += 1;
                let seconds = (10u64 << failures.min(4)).min(120);
                publish(
                    s,
                    "waiting",
                    &format!(
                        "一時的な通信エラーです。{seconds}秒待って再試行します（{failures}/6）"
                    ),
                )?;
                for _ in 0..seconds * 10 {
                    cancelled(s)?;
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            other => return other,
        }
    }
}
fn list_path(page: usize) -> Option<String> {
    const JAPANESE: &[&str] = &[
        "日本",
        "日本語",
        "Japan",
        "JP",
        "チル",
        "睡眠",
        "和風",
        "桜",
        "カフェ",
        "ホーム",
        "景色",
        "温泉",
        "神社",
        "花火",
        "秋",
        "夏",
        "記憶",
        "自然",
        "ゲーム",
        "chill",
    ];
    let (search, offset, sort) = if page < JAPANESE.len() * 2 {
        (JAPANESE[page / 2], (page % 2) * 100, "favorites")
    } else if page < JAPANESE.len() * 2 + 25 {
        ("", (page - JAPANESE.len() * 2) * 100, "favorites")
    } else {
        return None;
    };
    Some(format!("worlds?n=100&offset={offset}&sort={sort}&order=descending&releaseStatus=public&search={search}"))
}
fn japanese_priority(world: &Value) -> i32 {
    fn kana(text: &str) -> bool {
        text.chars().any(|c| ('\u{3040}'..='\u{30ff}').contains(&c))
    }
    fn kanji(text: &str) -> bool {
        text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
    }
    let text = format!(
        "{} {} {}",
        world["name"].as_str().unwrap_or(""),
        world["description"].as_str().unwrap_or(""),
        world["tags"]
    )
    .to_lowercase();
    let author = world["authorName"]
        .as_str()
        .or(world["author"].as_str())
        .unwrap_or("");
    let indicated = text.contains("日本")
        || text.contains("japan")
        || text.contains("author_tag_jp")
        || text.contains("[jp]")
        || text.contains("jpn");
    (if kana(&text) {
        4
    } else if kanji(&text) {
        1
    } else {
        0
    }) + (if indicated { 3 } else { 0 })
        + (if kana(author) {
            2
        } else if kanji(author) {
            1
        } else {
            0
        })
}
fn enqueue(db: &Connection, raw: &Value, limit: usize) -> Result<usize, String> {
    let items = raw.as_array().ok_or("一覧APIの応答が不正です")?;
    let mut added = 0;
    for world in items {
        if added >= limit {
            break;
        }
        if world["releaseStatus"] != "public" {
            continue;
        }
        let Some(id) = world["id"]
            .as_str()
            .filter(|id| store::official_url(id).is_some())
        else {
            continue;
        };
        let priority = japanese_priority(world);
        added += db
            .execute(
                "INSERT OR IGNORE INTO personal.import_queue(id,state,priority) VALUES (?1,'queued',?2)",
                params![id,priority],
            )
            .map_err(|e| e.to_string())?;
        db.execute(
            "UPDATE personal.import_queue SET priority=MAX(priority,?2) WHERE id=?1",
            params![id, priority],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(added)
}
fn run(s: &AppState) -> Result<(), String> {
    if remote_mode(&s.root) {
        return index_pending(s);
    }
    {
        let mut session = s.auth.lock().map_err(|e| e.to_string())?;
        session.check(s)?;
        if !session.verified {
            return Err("二段階認証を完了してください".into());
        }
    }
    let is_expanded = s.corpus.lock().map_err(|e| e.to_string())?.expanded;
    if is_expanded {
        return expanded::run(s);
    }
    {
        let db = s.db.lock().map_err(|e| e.to_string())?;
        // Include existing public records without touching chats, favourites or visits.
        db.execute("INSERT OR IGNORE INTO personal.import_queue(id,state) SELECT id,'done' FROM personal.worlds WHERE json_extract(record,'$.deleted')=0 LIMIT 1000",[]).map_err(|e|e.to_string())?;
    }
    loop {
        cancelled(s)?;
        let (done, queued, _) = counts(&*s.db.lock().map_err(|e| e.to_string())?)?;
        if done >= TARGET {
            break;
        }
        if done + queued < TARGET {
            let page = s.corpus.lock().map_err(|e| e.to_string())?.page;
            let path = list_path(page)
                .ok_or("取得できる公開ワールド候補が不足しました。取得済みDBは利用できます")?;
            publish(s, "discovering", "公開ワールドの一覧を取得しています")?;
            let raw = request(s, &path)?;
            enqueue(
                &*s.db.lock().map_err(|e| e.to_string())?,
                &raw,
                TARGET - done - queued,
            )?;
            s.corpus.lock().map_err(|e| e.to_string())?.page = page + 1;
            publish(
                s,
                "fetching",
                "ワールドの説明・タグ・対応機種を保存しています",
            )?;
            continue;
        }
        let id: String = s
            .db
            .lock()
            .map_err(|e| e.to_string())?
            .query_row(
                "SELECT id FROM personal.import_queue WHERE state='queued' ORDER BY priority DESC,rowid LIMIT 1",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        publish(
            s,
            "fetching",
            "ワールドの説明・タグ・対応機種を保存しています",
        )?;
        match request(s, &format!("worlds/{id}")) {
            Ok(raw) => {
                if raw["id"] != id {
                    return Err("API応答のワールドIDが一致しません".into());
                }
                match vrchat::normalize(&raw, vrchat::now()) {
                    Ok(world) => {
                        let db = s.db.lock().map_err(|e| e.to_string())?;
                        vrchat::save(&db, &world)?;
                        db.execute(
                            "UPDATE personal.import_queue SET state='done' WHERE id=?1",
                            params![id],
                        )
                        .map_err(|e| e.to_string())?;
                    }
                    Err(e) if e.contains("公開ワールドだけ") => {
                        skip(s, &id)?;
                    }
                    Err(e) => return Err(e),
                }
            }
            Err(e) if e.contains("公開ワールドが見つかりません") => skip(s, &id)?,
            Err(e) => return Err(e),
        }
        publish(
            s,
            "fetching",
            "ワールドの説明・タグ・対応機種を保存しています",
        )?;
    }
    index_pending(s)?;
    publish(
        s,
        "complete",
        "公開ワールド1,000件の検索DBができました。画像と翻訳は表示時に保存します",
    )?;
    Ok(())
}
fn index_pending(s: &AppState) -> Result<(), String> {
    let version = embedding_version();
    loop {
        cancelled(s)?;
        let record:Option<String>=s.db.lock().map_err(|e|e.to_string())?.query_row("SELECT w.record FROM personal.worlds w JOIN personal.import_queue q ON q.id=w.id WHERE q.state='done' AND NOT EXISTS(SELECT 1 FROM vectors v WHERE v.id=w.id AND v.model=?1) LIMIT 1",params![version],|r|r.get(0)).ok();
        let Some(record) = record else {
            break;
        };
        let w: Value = serde_json::from_str(&record).map_err(|e| e.to_string())?;
        publish(
            s,
            "indexing",
            "日本語で検索するための意味検索データを作っています",
        )?;
        let vector = loop {
            cancelled(s)?;
            let result = use_server(s, "embedding", |server| server.embed(&document(&w), false));
            match result {
                Err(e) if e == "推論実行中です" => {
                    std::thread::sleep(Duration::from_millis(500))
                }
                other => break other?,
            }
        };
        s.db.lock()
            .map_err(|e| e.to_string())?
            .execute(
                "INSERT OR REPLACE INTO vectors VALUES (?1,?2,?3)",
                params![
                    w["worldId"].as_str().unwrap(),
                    version,
                    serde_json::to_string(&vector).map_err(|e| e.to_string())?
                ],
            )
            .map_err(|e| e.to_string())?;
        publish(
            s,
            "indexing",
            "日本語で検索するための意味検索データを作っています",
        )?;
    }
    Ok(())
}
pub fn resume_saved(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        if remote_mode(&app.state::<AppState>().root) {
            if let Err(error) = corpus_start(app.clone()) {
                let _ = publish(&app.state::<AppState>(), "paused", &error);
            }
            return;
        }
        let result = {
            let s = app.state::<AppState>();
            s.auth
                .lock()
                .map_err(|e| e.to_string())
                .and_then(|mut session| session.check(&s))
        };
        if let Err(error) = result {
            let s = app.state::<AppState>();
            let _ = publish(&s, "paused", &error);
            return;
        }
        if let Err(error) = corpus_start(app.clone()) {
            let s = app.state::<AppState>();
            let _ = publish(&s, "paused", &error);
        }
    });
}
fn prepare_expansion(s: &AppState) -> Result<(), String> {
    expanded::seed(&*s.db.lock().map_err(|e| e.to_string())?)?;
    {
        let mut progress = s.corpus.lock().map_err(|e| e.to_string())?;
        progress.expanded = true;
        progress.focus = "japanese-expanded-v1".into();
        progress.automatic_resume = true;
    }
    publish(
        s,
        "ready",
        "カテゴリーの優先順で日本語向け公開ワールドを追加します",
    )
}
pub fn expand_saved(app: tauri::AppHandle) {
    let result = prepare_expansion(&app.state::<AppState>());
    if result.is_ok() {
        resume_saved(app);
    } else if let Err(error) = result {
        let _ = publish(&app.state::<AppState>(), "paused", &error);
    }
}
#[tauri::command]
pub fn corpus_expand(app: tauri::AppHandle) -> Result<(), String> {
    let s = app.state::<AppState>();
    if remote_mode(&s.root) {
        return Err("ワールド収集はVPSで実行しています。PCでは同期済みデータを検索します".into());
    }
    if s.corpus_busy.load(Ordering::SeqCst) {
        return Err("既に取得中です".into());
    }
    if !s
        .auth
        .try_lock()
        .map_err(|_| "ログイン処理中です")?
        .verified
    {
        return Err("先にVRChatログインを完了してください".into());
    }
    prepare_expansion(&s)?;
    corpus_start(app)
}
pub fn document(w: &Value) -> String {
    format!(
        "{} {} {}",
        w["name"].as_str().unwrap_or(""),
        w["description"].as_str().unwrap_or(""),
        w["tags"]
    )
    .chars()
    .take(2400)
    .collect()
}
fn skip(s: &AppState, id: &str) -> Result<(), String> {
    s.db.lock()
        .map_err(|e| e.to_string())?
        .execute(
            "UPDATE personal.import_queue SET state='skipped' WHERE id=?1",
            params![id],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}
#[tauri::command]
pub fn corpus_status(s: tauri::State<AppState>) -> Result<Progress, String> {
    Ok(s.corpus.lock().map_err(|e| e.to_string())?.clone())
}
#[tauri::command]
pub fn corpus_start(app: tauri::AppHandle) -> Result<(), String> {
    let s = app.state::<AppState>();
    if !remote_mode(&s.root) && !s
        .auth
        .try_lock()
        .map_err(|_| "ログイン処理中です")?
        .verified
    {
        return Err("先にVRChatログインを完了してください".into());
    }
    if s.corpus_busy.swap(true, Ordering::SeqCst) {
        return Err("既に取得中です".into());
    }
    s.corpus_cancel.store(false, Ordering::SeqCst);
    let mut prefs = preferences::load(&s.root);
    prefs.catalog_mode = "personal".into();
    if let Err(e) = preferences::save(&s.root, &prefs) {
        s.corpus_busy.store(false, Ordering::SeqCst);
        return Err(e);
    }
    {
        let mut progress = s.corpus.lock().map_err(|e| e.to_string())?;
        progress.running = true;
        progress.automatic_resume = true;
    }
    std::thread::spawn(move || {
        let s = app.state::<AppState>();
        let result = run(&s);
        if let Ok(mut p) = s.corpus.lock() {
            p.running = false;
            if result.is_ok() {
                p.automatic_resume = false;
            }
        }
        match result {
            Ok(()) => {
                let message = if remote_mode(&s.root) {
                    "VPSから同期した情報の意味検索データができました。ワールド収集はVPSで継続します"
                } else if s.corpus.lock().map(|p| p.expanded).unwrap_or(false) {
                    "検索計画の巡回が終了しました。全世界の完全網羅を保証するものではありません"
                } else {
                    "公開ワールド1,000件の検索DBができました。画像と翻訳は表示時に保存します"
                };
                let _ = publish(&s, "complete", message);
            }
            Err(e) => {
                let _ = publish(&s, "paused", &e);
            }
        }
        s.corpus_busy.store(false, Ordering::SeqCst);
    });
    Ok(())
}
pub fn remote_mode(root: &Path) -> bool {
    root.join("userdata/remote-collector.json").is_file()
}
#[tauri::command]
pub fn corpus_pause(s: tauri::State<AppState>) {
    if let Ok(mut progress) = s.corpus.lock() {
        progress.automatic_resume = false;
    }
    s.corpus_cancel.store(true, Ordering::SeqCst);
    let _ = publish(
        &s,
        "pausing",
        "中断しています。保存済みデータとページ位置は残ります",
    );
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expanded_progress_preserves_scope_cursor_and_manual_pause() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("userdata")).unwrap();
        let p = Progress {
            expanded: true,
            focus: "japanese-expanded-v1".into(),
            completed: 2500,
            page: 33,
            target: 2600,
            running: true,
            automatic_resume: true,
            ..Default::default()
        };
        let file = t.path().join("userdata/corpus-progress.json");
        std::fs::write(&file, serde_json::to_vec(&p).unwrap()).unwrap();
        let restored = Progress::load(t.path());
        assert!(restored.expanded);
        assert_eq!(restored.page, 33);
        assert_eq!(restored.target, 2600);
        assert!(restored.automatic_resume);
        assert!(!restored.running);
        let paused = Progress {
            running: false,
            automatic_resume: false,
            ..p
        };
        std::fs::write(&file, serde_json::to_vec(&paused).unwrap()).unwrap();
        assert!(!Progress::load(t.path()).automatic_resume);
    }
    #[test]
    fn thousand_item_limit_and_restart_keep_saved_work() {
        let t = tempfile::tempdir().unwrap();
        let db = store::open(t.path()).unwrap();
        schema(&db).unwrap();
        let worlds:Vec<Value>=(1..=1100).map(|i|json!({"id":format!("wrld_{}",uuid::Uuid::from_u128(i)),"releaseStatus":"public"})).collect();
        assert_eq!(enqueue(&db, &json!(worlds), 1000).unwrap(), 1000);
        assert_eq!(counts(&db).unwrap(), (0, 1000, 0));
        db.execute(
            "UPDATE personal.import_queue SET state='done' WHERE rowid<=50",
            [],
        )
        .unwrap();
        drop(db);
        let reopened = store::open(t.path()).unwrap();
        schema(&reopened).unwrap();
        assert_eq!(counts(&reopened).unwrap(), (50, 950, 0));
        let p = Progress {
            running: true,
            completed: 50,
            page: 3,
            ..Default::default()
        };
        std::fs::write(
            t.path().join("userdata/corpus-progress.json"),
            serde_json::to_vec(&p).unwrap(),
        )
        .unwrap();
        let resumed = Progress::load(t.path());
        assert_eq!(resumed.completed, 50);
        assert_eq!(resumed.page, 3);
        assert!(!resumed.running);
        assert_eq!(resumed.stage, "paused");
    }
    #[test]
    fn discovery_is_public_bounded_and_deduplicated() {
        let t = tempfile::tempdir().unwrap();
        let db = store::open(t.path()).unwrap();
        schema(&db).unwrap();
        let id = "wrld_4432ea9b-729c-46e3-8eaf-846aa0a37fdd";
        let w = json!({"id":id,"releaseStatus":"public"});
        assert_eq!(enqueue(&db,&json!([w.clone(),w,{"id":"bad","releaseStatus":"public"},{"id":"wrld_a729bf90-1220-423a-9630-7e60851e7475","releaseStatus":"private"}]),1000).unwrap(),1);
        assert_eq!(counts(&db).unwrap(), (0, 1, 0));
        assert!(list_path(65).is_none());
        assert!(list_path(0).unwrap().contains("search=日本"));
        assert!(list_path(10).unwrap().contains("releaseStatus=public"));
    }
    #[test]
    fn japanese_hints_rank_first_without_assigning_nationality() {
        assert_eq!(
            japanese_priority(&json!({"name":"English Room","authorName":"Alice"})),
            0
        );
        let japanese = japanese_priority(
            &json!({"name":"雨のカフェ","description":"日本語でくつろぐ","authorName":"はな"}),
        );
        assert!(japanese > japanese_priority(&json!({"name":"JP Room","authorName":"Alice"})));
        assert!(japanese_priority(&json!({"name":"Room","tags":["author_tag_japanese"]})) > 0);
        let t = tempfile::tempdir().unwrap();
        let db = store::open(t.path()).unwrap();
        schema(&db).unwrap();
        let raw = json!([{"id":"wrld_4432ea9b-729c-46e3-8eaf-846aa0a37fdd","name":"English Room","releaseStatus":"public"},{"id":"wrld_a729bf90-1220-423a-9630-7e60851e7475","name":"雨のカフェ","releaseStatus":"public"}]);
        enqueue(&db, &raw, 1000).unwrap();
        let first: String = db
            .query_row(
                "SELECT id FROM personal.import_queue ORDER BY priority DESC,rowid LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(first, "wrld_a729bf90-1220-423a-9630-7e60851e7475");
        std::fs::write(
            t.path().join("userdata/corpus-progress.json"),
            br#"{"running":true,"completed":77,"page":3}"#,
        )
        .unwrap();
        let migrated = Progress::load(t.path());
        assert_eq!(migrated.completed, 77);
        assert_eq!(migrated.page, 0);
        assert_eq!(migrated.focus, "japanese-v1");
    }
}

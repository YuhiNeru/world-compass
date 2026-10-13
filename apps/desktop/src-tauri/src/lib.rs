mod answer;
mod auth;
mod chat;
mod corpus;
mod download;
mod inference;
mod preferences;
mod presentation;
mod search;
mod setup;
mod store;
mod sync;
mod vrchat;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Mutex,
    },
    time::Duration,
};
use tauri::{Manager, State};
pub struct AppState {
    root: PathBuf,
    db: Mutex<rusqlite::Connection>,
    server: Mutex<HashMap<String, inference::Server>>,
    setup_cancel: AtomicBool,
    setup_busy: AtomicBool,
    inference_cancel: AtomicBool,
    pid: AtomicU32,
    embedding_pid: AtomicU32,
    api: Mutex<vrchat::ApiState>,
    auth: Mutex<auth::Session>,
    corpus: Mutex<corpus::Progress>,
    corpus_busy: AtomicBool,
    corpus_cancel: AtomicBool,
}
fn state_root() -> PathBuf {
    std::env::var_os("WORLD_COMPASS_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_else(|| ".".into()))
                .join("WorldCompass")
        })
}
#[tauri::command]
fn load_data(s: State<AppState>) -> Result<Value, String> {
    store::load_for(
        &*s.db.lock().map_err(|e| e.to_string())?,
        &preferences::load(&s.root).catalog_mode,
    )
}
#[tauri::command]
fn get_preferences(s: State<AppState>) -> preferences::Preferences {
    preferences::load(&s.root)
}
#[tauri::command]
fn load_chats(s: State<AppState>) -> Result<Vec<Value>, String> {
    store::load_chats(&*s.db.lock().map_err(|e| e.to_string())?)
}
#[tauri::command]
fn save_chats(s: State<AppState>, chats: Vec<Value>) -> Result<(), String> {
    store::save_chats(&*s.db.lock().map_err(|e| e.to_string())?, &chats)
}
#[tauri::command]
fn save_preferences(s: State<AppState>, value: preferences::Preferences) -> Result<(), String> {
    if s.setup_busy.load(Ordering::SeqCst) {
        return Err("セットアップ終了後に設定を変更してください".into());
    }
    let mut servers = s
        .server
        .try_lock()
        .map_err(|_| "応答を停止してから設定を変更してください")?;
    preferences::save(&s.root, &value)?;
    servers.clear();
    s.pid.store(0, Ordering::SeqCst);
    s.embedding_pid.store(0, Ordering::SeqCst);
    Ok(())
}
#[tauri::command]
async fn add_world(app: tauri::AppHandle, input: String, refresh: bool) -> Result<Value, String> {
    let id = vrchat::world_id(&input)?;
    tauri::async_runtime::spawn_blocking(move || {
        let s = app.state::<AppState>();
        {
            let db = s.db.lock().map_err(|e| e.to_string())?;
            if !refresh && vrchat::fresh(&db, &id) {
                return Ok(json!({"world":vrchat::cached(&db,&id),"cached":true}));
            }
        }
        let p = preferences::load(&s.root);
        let w = vrchat::fetch(
            &id,
            &p.api_contact,
            &mut *s.api.lock().map_err(|e| e.to_string())?,
        )?;
        vrchat::save(&*s.db.lock().map_err(|e| e.to_string())?, &w)?;
        Ok(json!({"world":w,"cached":false}))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
fn save_personal(s: State<AppState>, id: String, value: Value) -> Result<(), String> {
    store::save(&*s.db.lock().map_err(|e| e.to_string())?, &id, &value)
}
#[tauri::command]
fn setup_status(s: State<AppState>) -> Value {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let m = setup::selected(&s.root);
    let p = preferences::load(&s.root);
    let ready = setup::runtime_path(&s.root).is_ok()
        && setup::model_path(&s.root, &p.model).is_ok()
        && setup::model_path(&s.root, "embedding").is_ok()
        && (p.backend != "vulkan" || setup::runtime_path_for(&s.root, "runtime-vulkan").is_ok());
    json!({"root":s.root,"manifest":m,"ram":sys.total_memory(),"availableRam":sys.available_memory(),"free":setup::available(&s.root),"gpu":if p.backend=="vulkan"{"GPU（Vulkan・起動失敗時はCPU）"}else{"CPU"},"ready":ready,"busy":s.setup_busy.load(Ordering::SeqCst),"installedModels":setup::manifest().assets.iter().filter(|a|a.id.starts_with("chat")&&setup::model_path(&s.root,&a.id).is_ok()).map(|a|a.id.clone()).collect::<Vec<_>>()})
}
#[tauri::command]
fn runtime_status(s: State<AppState>) -> Value {
    let pids = [
        s.pid.load(Ordering::SeqCst),
        s.embedding_pid.load(Ordering::SeqCst),
    ];
    let mut sys = sysinfo::System::new();
    for pid in pids.iter().filter(|p| **p > 0) {
        sys.refresh_processes(
            sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(*pid)]),
            true,
        );
    }
    let processes = pids
        .iter()
        .filter_map(|p| sys.process(sysinfo::Pid::from_u32(*p)))
        .collect::<Vec<_>>();
    let device = s
        .server
        .try_lock()
        .ok()
        .and_then(|m| m.get("chat").map(|s| s.device.clone()))
        .unwrap_or_else(|| preferences::load(&s.root).backend);
    json!({"running":!processes.is_empty(),"memory":processes.iter().map(|p|p.memory()).sum::<u64>(),"device":device})
}
#[tauri::command]
async fn install_assets(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let s = app.state::<AppState>();
        if s.setup_busy.swap(true, Ordering::SeqCst) {
            return Err("セットアップ実行中です".into());
        }
        s.setup_cancel.store(false, Ordering::SeqCst);
        let result = setup::install(&s.root, &s.setup_cancel, |id, done, total| {
            setup::event(&app, id, done, total)
        });
        s.setup_busy.store(false, Ordering::SeqCst);
        result
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
fn cancel_setup(s: State<AppState>) {
    s.setup_cancel.store(true, Ordering::SeqCst);
}
#[tauri::command]
fn cancel_inference(s: State<AppState>) {
    s.inference_cancel.store(true, Ordering::SeqCst);
    for pid in [
        s.pid.load(Ordering::SeqCst),
        s.embedding_pid.load(Ordering::SeqCst),
    ]
    .into_iter()
    .filter(|p| *p > 0)
    {
        let mut sys = sysinfo::System::new();
        sys.refresh_processes(
            sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(pid)]),
            true,
        );
        if let Some(p) = sys.process(sysinfo::Pid::from_u32(pid)) {
            p.kill();
        }
    }
    if let Ok(mut server) = s.server.try_lock() {
        server.clear();
        s.pid.store(0, Ordering::SeqCst);
        s.embedding_pid.store(0, Ordering::SeqCst);
    }
}
fn use_server<T>(
    s: &AppState,
    kind: &str,
    action: impl FnOnce(&mut inference::Server) -> Result<T, String>,
) -> Result<T, String> {
    let mut holder = s.server.try_lock().map_err(|_| "推論実行中です")?;
    s.inference_cancel.store(false, Ordering::SeqCst);
    let pid = if kind == "embedding" {
        &s.embedding_pid
    } else {
        &s.pid
    };
    if holder
        .get_mut(kind)
        .is_some_and(|p| p.child.try_wait().ok().flatten().is_some())
    {
        holder.remove(kind);
        pid.store(0, Ordering::SeqCst);
    }
    if !holder.contains_key(kind) {
        match inference::Server::start(&s.root, kind, pid, &s.inference_cancel) {
            Ok(server) => {
                holder.insert(kind.into(), server);
            }
            Err(e) => {
                pid.store(0, Ordering::SeqCst);
                return Err(e);
            }
        }
    }
    let result = action(holder.get_mut(kind).unwrap());
    if result.is_err() {
        holder.remove(kind);
        pid.store(0, Ordering::SeqCst);
    }
    result
}
#[tauri::command]
fn search_candidates(
    s: State<AppState>,
    query: String,
    conditions: Value,
) -> Result<Value, String> {
    validate_conditions(&conditions)?;
    if query.chars().count() > 2000 {
        return Err("query too long".into());
    }
    search::candidates_for(
        &*s.db.lock().map_err(|e| e.to_string())?,
        &query,
        &conditions,
        &preferences::load(&s.root).catalog_mode,
    )
}
#[tauri::command]
async fn ai_conditions(app: tauri::AppHandle, query: String) -> Result<Value, String> {
    if query.chars().count() > 2000 {
        return Err("検索文が長すぎます".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let s = app.state::<AppState>();
        let value = use_server(&s, "chat", |server| {
            server.chat(
                include_str!("../../../../packages/prompts/conditions-v1.txt"),
                &json!({"query":query}),
                serde_json::from_str(include_str!(
                    "../../../../packages/contracts/conditions.schema.json"
                ))
                .unwrap(),
            )
        })?;
        validate_conditions(&value)?;
        Ok(value)
    })
    .await
    .map_err(|e| e.to_string())?
}
fn validate_conditions(c: &Value) -> Result<(), String> {
    if !store::keys(
        c,
        &["people", "platform", "required", "preferred", "excluded"],
    ) || !(c["people"].is_null() || c["people"].as_u64().is_some_and(|n| (1..=100).contains(&n)))
        || !(c["platform"].is_null() || c["platform"] == "pc" || c["platform"] == "quest")
    {
        return Err("invalid conditions".into());
    }
    for k in ["required", "preferred", "excluded"] {
        if c[k]
            .as_array()
            .is_none_or(|a| a.len() > 7 || a.iter().any(|v| !is_attr(v.as_str().unwrap_or(""))))
        {
            return Err("invalid conditions attributes".into());
        }
    }
    Ok(())
}
fn is_attr(s: &str) -> bool {
    ["rain", "night", "chill", "photo", "nature", "game", "light"].contains(&s)
}
fn eligible(w: &Value, c: &Value) -> bool {
    if w["deleted"] == true {
        return false;
    }
    if let Some(n) = c["people"].as_u64() {
        if !w["capacity"].as_u64().is_some_and(|m| m >= n) {
            return false;
        }
    }
    if let Some(p) = c["platform"].as_str() {
        if !w["platforms"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v == p))
        {
            return false;
        }
    }
    for k in c["required"].as_array().unwrap() {
        if w["attributes"][k.as_str().unwrap()]["value"] != true {
            return false;
        }
    }
    for k in c["excluded"].as_array().unwrap() {
        if w["attributes"][k.as_str().unwrap()]["value"] != false {
            return false;
        }
    }
    true
}
pub fn validate_reasons(v: &Value, worlds: &[Value], c: &Value) -> Result<(), String> {
    if !store::keys(v, &["items"]) {
        return Err("invalid reasons".into());
    }
    let items = v["items"].as_array().ok_or("missing items")?;
    if items.len() > 6 {
        return Err("too many items".into());
    }
    let mut seen = std::collections::HashSet::new();
    for r in items {
        if !store::keys(r, &["worldId", "evidence", "unknown"]) {
            return Err("unexpected reason fields".into());
        }
        let id = r["worldId"].as_str().ok_or("missing ID")?;
        let w = worlds
            .iter()
            .find(|w| w["worldId"] == id)
            .ok_or("unknown ID")?;
        if !seen.insert(id) || !eligible(w, c) {
            return Err("invalid candidate".into());
        }
        let evidence = r["evidence"].as_array().ok_or("missing evidence")?;
        if evidence.is_empty() || evidence.len() > 10 {
            return Err("invalid evidence count".into());
        }
        for e in evidence {
            if !store::keys(e, &["field", "sourceId"]) {
                return Err("unexpected evidence".into());
            }
            let f = e["field"].as_str().ok_or("missing field")?;
            if ["capacity", "platforms"].contains(&f) {
                if w[f].is_null() || !e["sourceId"].is_null() {
                    return Err("missing evidence".into());
                }
            } else if !is_attr(f)
                || w["attributes"][f]["value"] != true
                || w["attributes"][f]["sourceId"] != e["sourceId"]
            {
                return Err("unverified evidence".into());
            }
        }
        let unknown = r["unknown"].as_array().ok_or("missing unknown")?;
        if unknown.len() > 10 {
            return Err("unknown overflow".into());
        }
        for f in unknown {
            let f = f.as_str().ok_or("invalid unknown")?;
            if ["capacity", "platforms"].contains(&f) {
                if !w[f].is_null() {
                    return Err("known field marked unknown".into());
                }
            } else if !is_attr(f) || !w["attributes"][f]["value"].is_null() {
                return Err("known field marked unknown".into());
            }
        }
    }
    Ok(())
}
#[tauri::command]
async fn ai_reasons(
    app: tauri::AppHandle,
    ids: Vec<String>,
    conditions: Value,
) -> Result<Value, String> {
    validate_conditions(&conditions)?;
    if ids.len() > 3 {
        return Err("too many candidates".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let s = app.state::<AppState>();
        let data = store::load_for(
            &*s.db.lock().map_err(|e| e.to_string())?,
            &preferences::load(&s.root).catalog_mode,
        )?;
        let worlds: Vec<Value> = data["catalog"]["worlds"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|w| ids.iter().any(|id| w["worldId"] == *id) && eligible(w, &conditions))
            .cloned()
            .collect();
        let compact = reason_input(&worlds);
        let value = use_server(&s, "chat", |server| {
            server.chat(
                include_str!("../../../../packages/prompts/reasons-v1.txt"),
                &json!({"conditions":conditions,"candidates":compact}),
                reason_schema(&worlds),
            )
        })?;
        validate_reasons(&value, &worlds, &conditions)?;
        Ok(value)
    })
    .await
    .map_err(|e| e.to_string())?
}
fn allowed_evidence(w: &Value) -> Vec<Value> {
    let mut evidence = Vec::new();
    for k in ["capacity", "platforms"] {
        if !w[k].is_null() {
            evidence.push(json!({"field":k,"sourceId":null}));
        }
    }
    for (k, a) in w["attributes"].as_object().unwrap() {
        if a["value"] == true {
            evidence.push(json!({"field":k,"sourceId":a["sourceId"]}));
        }
    }
    evidence
}
fn allowed_unknown(w: &Value) -> Vec<Value> {
    let mut unknown = Vec::new();
    for k in ["capacity", "platforms"] {
        if w[k].is_null() {
            unknown.push(json!(k));
        }
    }
    for (k, a) in w["attributes"].as_object().unwrap() {
        if a["value"].is_null() {
            unknown.push(json!(k));
        }
    }
    unknown
}
fn reason_input(worlds: &[Value]) -> Vec<Value> {
    worlds.iter().map(|w|json!({"worldId":w["worldId"],"allowedEvidence":allowed_evidence(w),"allowedUnknown":allowed_unknown(w)})).collect()
}
fn reason_schema(worlds: &[Value]) -> Value {
    let variants:Vec<Value>=worlds.iter().map(|w|{let evidence=allowed_evidence(w);let unknown=allowed_unknown(w);json!({"type":"object","additionalProperties":false,"required":["worldId","evidence","unknown"],"properties":{"worldId":{"const":w["worldId"]},"evidence":{"type":"array","minItems":1,"maxItems":3,"items":{"enum":evidence}},"unknown":{"type":"array","maxItems":unknown.len(),"items":if unknown.is_empty(){json!({"type":"string"})}else{json!({"enum":unknown})}}}})}).collect();
    json!({"type":"object","additionalProperties":false,"required":["items"],"properties":{"items":{"type":"array","maxItems":worlds.len(),"items":{"oneOf":variants}}}})
}
#[tauri::command]
async fn semantic_scores(app: tauri::AppHandle, query: String) -> Result<Value, String> {
    if query.chars().count() > 2000 {
        return Err("検索文が長すぎます".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let s = app.state::<AppState>();
        let data = store::load_for(
            &*s.db.lock().map_err(|e| e.to_string())?,
            &preferences::load(&s.root).catalog_mode,
        )?;
        use_server(&s, "embedding", |server| {
            let query = server.embed(&query, true)?;
            let mut scores = serde_json::Map::new();
            let version = setup::manifest()
                .assets
                .into_iter()
                .find(|a| a.id == "embedding")
                .unwrap()
                .version;
            for w in data["catalog"]["worlds"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|w| w["deleted"] != true)
            {
                let id = w["worldId"].as_str().unwrap();
                let db = s.db.lock().map_err(|e| e.to_string())?;
                let cached: Option<String> = db
                    .query_row(
                        "SELECT vector FROM vectors WHERE id=?1 AND model=?2",
                        rusqlite::params![id, version],
                        |r| r.get(0),
                    )
                    .ok();
                drop(db);
                let vector: Vec<f32> = if let Some(v) = cached {
                    serde_json::from_str(&v).map_err(|e| e.to_string())?
                } else {
                    // The importer builds missing vectors in the background. Keep searches usable meanwhile.
                    if s.corpus_busy.load(Ordering::SeqCst) {
                        continue;
                    }
                    let v = server.embed(&corpus::document(w), false)?;
                    s.db.lock()
                        .map_err(|e| e.to_string())?
                        .execute(
                            "INSERT OR REPLACE INTO vectors VALUES (?1,?2,?3)",
                            rusqlite::params![id, version, serde_json::to_string(&v).unwrap()],
                        )
                        .map_err(|e| e.to_string())?;
                    v
                };
                scores.insert(id.into(), json!(inference::cosine(&query, &vector)));
            }
            Ok(Value::Object(scores))
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
fn import_catalog(s: State<AppState>, catalog: Value) -> Result<(), String> {
    sync::check_grants(&catalog)?;
    store::replace_catalog(&*s.db.lock().map_err(|e| e.to_string())?, &catalog)
}
#[tauri::command]
async fn sync_catalog(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (catalog, delta) = sync::download_catalog()?;
        let s = app.state::<AppState>();
        let db = s.db.lock().map_err(|e| e.to_string())?;
        if delta {
            sync::apply_delta(&db, &catalog)
        } else {
            sync::check_grants(&catalog)?;
            store::replace_catalog(&db, &catalog)
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
fn open_world(s: State<AppState>, id: String) -> Result<(), String> {
    let data = store::load_for(
        &*s.db.lock().map_err(|e| e.to_string())?,
        &preferences::load(&s.root).catalog_mode,
    )?;
    if !data["catalog"]["worlds"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w["worldId"] == id && w["deleted"] != true)
    {
        return Err("unknown world".into());
    }
    let url = store::official_url(&id).ok_or("架空ワールドには公式ページがありません")?;
    #[cfg(windows)]
    {
        std::process::Command::new("rundll32.exe")
            .args(["url.dll,FileProtocolHandler", &url])
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let root = state_root();
            for dir in ["models", "runtime", "catalog", "userdata", "logs"] {
                std::fs::create_dir_all(root.join(dir))?;
            }
            let db = store::open(&root).map_err(std::io::Error::other)?;
            corpus::schema(&db).map_err(std::io::Error::other)?;
            let auth = auth::Session::load(&root);
            let corpus = corpus::Progress::load(&root);
            app.manage(AppState {
                root,
                db: Mutex::new(db),
                server: Mutex::new(HashMap::new()),
                setup_cancel: AtomicBool::new(false),
                setup_busy: AtomicBool::new(false),
                inference_cancel: AtomicBool::new(false),
                pid: AtomicU32::new(0),
                embedding_pid: AtomicU32::new(0),
                api: Mutex::new(vrchat::ApiState { next_request: 0 }),
                auth: Mutex::new(auth),
                corpus: Mutex::new(corpus),
                corpus_busy: AtomicBool::new(false),
                corpus_cancel: AtomicBool::new(false),
            });
            if std::env::args().any(|arg| arg == "--collect-japanese") {
                corpus::expand_saved(app.handle().clone());
            } else if std::env::args().any(|arg| arg == "--resume-db")
                || corpus::remote_mode(&app.state::<AppState>().root)
                || app
                    .state::<AppState>()
                    .corpus
                    .lock()
                    .map(|p| p.automatic_resume)
                    .unwrap_or(false)
            {
                corpus::resume_saved(app.handle().clone());
            }
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_secs(10));
                let s = handle.state::<AppState>();
                if let Ok(mut server) = s.server.try_lock() {
                    let idle = preferences::load(&s.root).idle_seconds;
                    server.retain(|_, p| p.last_used.elapsed() <= Duration::from_secs(idle));
                    if !server.contains_key("chat") {
                        s.pid.store(0, Ordering::SeqCst);
                    }
                    if !server.contains_key("embedding") {
                        s.embedding_pid.store(0, Ordering::SeqCst);
                    }
                };
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_data,
            save_personal,
            setup_status,
            runtime_status,
            install_assets,
            cancel_setup,
            cancel_inference,
            ai_conditions,
            search_candidates,
            ai_reasons,
            semantic_scores,
            import_catalog,
            sync_catalog,
            open_world,
            get_preferences,
            presentation::world_image,
            presentation::translate_description,
            auth::vrchat_auth_status,
            auth::vrchat_login,
            auth::vrchat_verify,
            auth::vrchat_restore,
            auth::vrchat_forget,
            corpus::corpus_status,
            corpus::corpus_start,
            corpus::corpus_expand,
            corpus::corpus_pause,
            load_chats,
            save_chats,
            save_preferences,
            add_world,
            chat::ai_plan,
            chat::ai_answer
        ])
        .build(tauri::generate_context!())
        .expect("desktop build")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                let s = app.state::<AppState>();
                s.setup_cancel.store(true, Ordering::SeqCst);
                s.corpus_cancel.store(true, Ordering::SeqCst);
                cancel_inference(s);
            }
        });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn llm_boundary() {
        let data: Value = serde_json::from_str(store::FIXTURE).unwrap();
        let worlds = data["worlds"].as_array().unwrap();
        let c =
            json!({"people":2,"platform":null,"required":["rain"],"preferred":[],"excluded":[]});
        validate_conditions(&c).unwrap();
        let good = json!({"items":[{"worldId":"fixture-rain-room","evidence":[{"field":"rain","sourceId":"fixture:rain-room"}],"unknown":["light"]}]});
        validate_reasons(&good, worlds, &c).unwrap();
        let mut bad = good.clone();
        bad["items"][0]["worldId"] = json!("wrld_fake");
        assert!(validate_reasons(&bad, worlds, &c).is_err());
        let mut bad = good.clone();
        bad["items"][0]["evidence"][0]["field"] = json!("light");
        assert!(validate_reasons(&bad, worlds, &c).is_err());
        assert!(validate_conditions(&json!({"people":-1})).is_err());
    }

    #[test]
    #[ignore = "Downloads pinned assets (~1.3 GB); run explicitly for local AI acceptance"]
    fn real_local_ai() {
        let root = std::env::var_os("WORLD_COMPASS_DATA_DIR")
            .map(PathBuf::from)
            .expect("explicit test data directory");
        let cancel = AtomicBool::new(false);
        let pid = AtomicU32::new(0);
        let mut last = std::time::Instant::now();
        setup::install(&root, &cancel, |id, done, total| {
            if done == total {
                println!("installed {id}: {done}/{total}");
            }
        })
        .unwrap();
        println!("setup_seconds={:.2}", last.elapsed().as_secs_f64());
        last = std::time::Instant::now();
        let mut chat = inference::Server::start(&root, "chat", &pid, &cancel).unwrap();
        let value = chat
            .chat(
                include_str!("../../../../packages/prompts/conditions-v1.txt"),
                &json!({"query":"雨の夜に2人でゆっくりできる場所"}),
                serde_json::from_str(include_str!(
                    "../../../../packages/contracts/conditions.schema.json"
                ))
                .unwrap(),
            )
            .unwrap();
        println!(
            "conditions={value}; chat_seconds={:.2}",
            last.elapsed().as_secs_f64()
        );
        validate_conditions(&value).unwrap();
        let data: Value = serde_json::from_str(store::FIXTURE).unwrap();
        let worlds = vec![data["worlds"][0].clone()];
        let reasons = chat
            .chat(
                include_str!("../../../../packages/prompts/reasons-v1.txt"),
                &json!({"conditions":value,"candidates":reason_input(&worlds)}),
                reason_schema(&worlds),
            )
            .unwrap();
        println!("reasons={reasons}");
        validate_reasons(&reasons, &worlds, &value).unwrap();
        let child_pid = chat.child.id();
        drop(chat);
        let mut system = sysinfo::System::new();
        system.refresh_processes(
            sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(child_pid)]),
            true,
        );
        assert!(system.process(sysinfo::Pid::from_u32(child_pid)).is_none());
        last = std::time::Instant::now();
        let mut embedding = inference::Server::start(&root, "embedding", &pid, &cancel).unwrap();
        let q = embedding.embed("雨の夜にゆっくり過ごす", true).unwrap();
        let a = embedding
            .embed("窓越しの雨音と夜景。落ち着いた小さな部屋。", false)
            .unwrap();
        let b = embedding
            .embed("協力して謎を解くゲーム用の小島。", false)
            .unwrap();
        println!(
            "embedding_dimensions={}; rain_cosine={}; game_cosine={}; embedding_seconds={:.2}",
            q.len(),
            inference::cosine(&q, &a),
            inference::cosine(&q, &b),
            last.elapsed().as_secs_f64()
        );
        assert_eq!(q.len(), a.len());
    }
}

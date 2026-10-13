use crate::{preferences, store, use_server, validate_conditions, AppState};
use serde_json::{json, Value};
use tauri::{Emitter, Manager};
pub fn plan_schema() -> Value {
    json!({"type":"object","additionalProperties":false,"required":["intent","conditions","semanticQuery"],"properties":{"intent":{"enum":["search","question","chat"]},"conditions":serde_json::from_str::<Value>(include_str!("../../../../packages/contracts/conditions.schema.json")).unwrap(),"semanticQuery":{"type":"string","maxLength":160}}})
}
#[tauri::command]
pub async fn ai_plan(
    app: tauri::AppHandle,
    query: String,
    previous: Option<Value>,
) -> Result<Value, String> {
    if query.chars().count() > 2000 {
        return Err("メッセージが長すぎます".into());
    }
    if let Some(ref c) = previous {
        validate_conditions(c)?;
    }
    tauri::async_runtime::spawn_blocking(move || {
        let s = app.state::<AppState>();
        let value = use_server(&s, "chat", |server| {
            server.chat(
                include_str!("../../../../packages/prompts/plan-v1.txt"),
                &json!({"query":query,"previous":previous}),
                plan_schema(),
            )
        })?;
        validate_conditions(&value["conditions"])?;
        if !["search", "question", "chat"].contains(&value["intent"].as_str().unwrap_or(""))
            || value["semanticQuery"]
                .as_str()
                .is_none_or(|s| s.chars().count() > 160)
        {
            return Err("AIの検索計画を検証できませんでした".into());
        }
        Ok(value)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn ai_answer(
    app: tauri::AppHandle,
    request_id: String,
    query: String,
    ids: Vec<String>,
    conditions: Value,
    history: Vec<Value>,
    intent: String,
) -> Result<String, String> {
    validate_conditions(&conditions)?;
    if request_id.len() > 64
        || query.chars().count() > 2000
        || ids.len() > 6
        || history.len() > 8
        || !["search", "question", "chat"].contains(&intent.as_str())
    {
        return Err("会話入力が範囲外です".into());
    }
    for m in &history {
        if !["user", "assistant"].contains(&m["role"].as_str().unwrap_or(""))
            || m["content"]
                .as_str()
                .is_none_or(|s| s.chars().count() > 2000)
        {
            return Err("会話履歴が不正です".into());
        }
    }
    tauri::async_runtime::spawn_blocking(move||{
        let s=app.state::<AppState>();let p=preferences::load(&s.root);
        let data=store::load_for(&*s.db.lock().map_err(|e|e.to_string())?,&p.catalog_mode)?;
        let worlds:Vec<Value>=ids.iter().take(3).filter_map(|id|data["catalog"]["worlds"].as_array().unwrap().iter().find(|w|w["worldId"]==*id && w["deleted"]!=true)).map(|w| {
            let attributes:serde_json::Map<String,Value> = w["attributes"].as_object().unwrap().iter().map(|(k,v)|(k.clone(),v["value"].clone())).collect();
            let sources:Vec<&str> = w["sources"].as_array().unwrap().iter().filter_map(|s|s["label"].as_str()).collect();
            json!({"name":w["name"],"description":w["description"].as_str().map(|s|s.chars().take(350).collect::<String>()),"tags":w["tags"].as_array().map(|t|t.iter().take(12).collect::<Vec<_>>()),"platforms":w["platforms"],"capacity":w["capacity"],"attributes":attributes,"source":sources})
        }).collect();
        let history:Vec<Value> = history.into_iter().rev().take(4).map(|m|json!({"role":m["role"],"content":m["content"].as_str().unwrap().chars().take(350).collect::<String>()})).collect::<Vec<_>>().into_iter().rev().collect();
        let input=json!({"query":query.chars().take(1200).collect::<String>(),"conditions":conditions,"intent":intent,"history":history,"fixture":data["catalog"]["fixture"],"candidates":worlds});
        let messages=json!([{"role":"system","content":include_str!("../../../../packages/prompts/chat-v1.txt")},{"role":"user","content":input.to_string()}]);
        let answer=use_server(&s,"chat",|server|server.generate(messages,&s.inference_cancel,|text|{
            if crate::answer::grounded(text,&worlds) { let _=app.emit("chat-token",json!({"requestId":request_id,"text":text})); }
        }))?;
        let answer=if crate::answer::grounded(&answer,&worlds) {answer} else {crate::answer::fallback(&worlds)};
        let _=app.emit("chat-token",json!({"requestId":request_id,"text":answer}));
        Ok(answer)
    }).await.map_err(|e|e.to_string())?
}

use crate::{preferences, setup, use_server, vrchat, AppState};
use rusqlite::params;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::Manager;
#[tauri::command]
pub async fn world_image(
    app: tauri::AppHandle,
    id: String,
) -> Result<tauri::ipc::Response, String> {
    if crate::store::official_url(&id).is_none() {
        return Err("公開ワールドのIDが不正です".into());
    }
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        let s = app.state::<AppState>();
        let mut world = vrchat::cached(&*s.db.lock().map_err(|e| e.to_string())?, &id)
            .ok_or("未登録のワールドです")?;
        let cached: Option<(String, Vec<u8>)> =
            s.db.lock()
                .map_err(|e| e.to_string())?
                .query_row(
                    "SELECT url,bytes FROM personal.images WHERE id=?1",
                    params![id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .ok();
        if let Some((url, bytes)) = &cached {
            if world["thumbnail"]["url"] == *url || world["thumbnail"].is_null() {
                return Ok(bytes.clone());
            }
        }
        let prefs = preferences::load(&s.root);
        let fetched = (|| {
            let mut api = s.api.lock().map_err(|e| e.to_string())?;
            if world["thumbnail"].is_null() {
                vrchat::wait_short(&api)?;
                world = vrchat::fetch(&id, &prefs.api_contact, &mut api)?;
                vrchat::save(&*s.db.lock().map_err(|e| e.to_string())?, &world)?;
            }
            let url = world["thumbnail"]["url"]
                .as_str()
                .ok_or("公開画像がありません")?;
            let bytes = vrchat::picture(url, &prefs.api_contact, &mut api)?;
            s.db.lock()
                .map_err(|e| e.to_string())?
                .execute(
                    "INSERT OR REPLACE INTO personal.images(id,url,bytes) VALUES (?1,?2,?3)",
                    params![id, url, bytes],
                )
                .map_err(|e| e.to_string())?;
            Ok::<Vec<u8>, String>(bytes)
        })();
        match fetched {
            Ok(bytes) => Ok(bytes),
            Err(error) => cached.map(|(_, bytes)| bytes).ok_or(error),
        }
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(tauri::ipc::Response::new(bytes))
}
fn chunks(text: &str) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut buffer = String::new();
    for sentence in text.split_inclusive(['.', '!', '?', '\n']) {
        for word in sentence.split_inclusive(char::is_whitespace) {
            if buffer.chars().count() + word.chars().count() > 550 && !buffer.is_empty() {
                pieces.push(std::mem::take(&mut buffer));
            }
            for ch in word.chars() {
                buffer.push(ch);
                if buffer.chars().count() >= 550 {
                    pieces.push(std::mem::take(&mut buffer));
                }
            }
        }
    }
    if !buffer.is_empty() {
        pieces.push(buffer);
    }
    pieces
}
fn japanese(text: &str) -> bool {
    text.chars()
        .any(|c| ('\u{3040}'..='\u{30ff}').contains(&c) || ('\u{4e00}'..='\u{9fff}').contains(&c))
}
#[tauri::command]
pub async fn translate_description(
    app: tauri::AppHandle,
    id: String,
    original: String,
) -> Result<Value, String> {
    if original.is_empty() || original.chars().count() > 8000 {
        return Err("説明文の翻訳サイズが範囲外です".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let s=app.state::<AppState>();
        if vrchat::cached(&*s.db.lock().map_err(|e|e.to_string())?,&id).is_none() {return Err("未登録のワールドです".into());}
        let prefs=preferences::load(&s.root); let asset=setup::manifest().assets.into_iter().find(|a|a.id==prefs.model).ok_or("モデル設定が不正です")?;
        let key=format!("{:x}",Sha256::digest(format!("translation-v1\n{}\n{}",asset.sha256,original).as_bytes()));
        if let Ok(text)=s.db.lock().map_err(|e|e.to_string())?.query_row("SELECT text FROM personal.translations WHERE cache_key=?1",params![key],|r|r.get::<_,String>(0)) {return Ok(json!({"text":text,"cached":true}));}
        let pieces=chunks(&original);
        let text=use_server(&s,"chat",|server|{
            let mut output=Vec::new();
            for piece in pieces {
                if s.inference_cancel.load(std::sync::atomic::Ordering::SeqCst) {return Err("翻訳を停止しました".into());}
                let result=server.chat(include_str!("../../../../packages/prompts/translation-v1.txt"),&json!({"original":piece}),json!({"type":"object","additionalProperties":false,"required":["text"],"properties":{"text":{"type":"string"}}}))?;
                let translated=result["text"].as_str().filter(|t|!t.trim().is_empty() && t.chars().count()<=3000).ok_or("翻訳を検証できませんでした")?;
                output.push(translated.trim().to_string());
            }
            let text=output.join("\n");if !japanese(&text) {return Err("日本語訳を生成できませんでした".into());} Ok(text)
        })?;
        s.db.lock().map_err(|e|e.to_string())?.execute("INSERT OR REPLACE INTO personal.translations(cache_key,text) VALUES (?1,?2)",params![key,text]).map_err(|e|e.to_string())?;
        Ok(json!({"text":text,"cached":false}))
    }).await.map_err(|e|e.to_string())?
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chunks_preserve_the_complete_original() {
        let source = "A sentence. Another sentence!\n".repeat(100);
        let parts = chunks(&source);
        assert_eq!(parts.concat(), source);
        assert!(parts.iter().all(|p| p.chars().count() <= 550));
        assert!(japanese("日本語訳"));
        assert!(!japanese("English only"));
    }
}

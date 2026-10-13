use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Preferences {
    pub model: String,
    pub backend: String,
    pub threads: u32,
    pub idle_seconds: u64,
    pub api_contact: String,
    pub catalog_mode: String,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            model: "chat-quality".into(),
            backend: "vulkan".into(),
            threads: 4,
            idle_seconds: 120,
            api_contact: String::new(),
            catalog_mode: "demo".into(),
        }
    }
}
pub fn load(root: &Path) -> Preferences {
    std::fs::read(root.join("userdata/preferences.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}
pub fn save(root: &Path, p: &Preferences) -> Result<(), String> {
    if !["chat", "chat-balanced", "chat-quality"].contains(&p.model.as_str())
        || !["cpu", "vulkan"].contains(&p.backend.as_str())
        || !(1..=8).contains(&p.threads)
        || !(30..=600).contains(&p.idle_seconds)
        || !["demo", "personal"].contains(&p.catalog_mode.as_str())
    {
        return Err("設定値が範囲外です".into());
    }
    if !p.api_contact.is_empty() {
        validate_contact(&p.api_contact)?;
    }
    std::fs::create_dir_all(root.join("userdata")).map_err(|e| e.to_string())?;
    let temp = root.join("userdata/preferences.tmp");
    std::fs::write(&temp, serde_json::to_vec_pretty(p).unwrap()).map_err(|e| e.to_string())?;
    std::fs::rename(temp, root.join("userdata/preferences.json")).map_err(|e| e.to_string())
}
pub fn validate_contact(s: &str) -> Result<(), String> {
    if s.len() > 200
        || s.chars().any(|c| c.is_control() || c.is_whitespace())
        || !(s.contains('@') || s.starts_with("https://"))
    {
        return Err("連絡先メールアドレスかhttpsの連絡先ページを入力してください".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profiles_validate_and_persist() {
        let t = tempfile::tempdir().unwrap();
        let mut p = Preferences::default();
        save(t.path(), &p).unwrap();
        p.model = "chat-balanced".into();
        p.backend = "cpu".into();
        p.threads = 2;
        p.idle_seconds = 30;
        save(t.path(), &p).unwrap();
        assert_eq!(load(t.path()).model, "chat-balanced");
        p.model = "../other".into();
        assert!(save(t.path(), &p).is_err());
        assert_eq!(load(t.path()).model, "chat-balanced");
        assert!(validate_contact("contact@example.com\r\nInjected: yes").is_err());
    }
}

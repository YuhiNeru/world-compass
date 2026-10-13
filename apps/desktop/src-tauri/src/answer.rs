use serde_json::Value;
// A conservative guard for consequential world claims, independent of LLM instructions.
// It does not claim to verify every possible assertion in natural language.
pub fn grounded(text: &str, worlds: &[Value]) -> bool {
    if worlds.is_empty() {
        return true;
    }
    if ["platforms", "sourceId", "worldId", "null"]
        .iter()
        .any(|field| text.contains(field))
    {
        return false;
    }
    for sentence in text.split(['。', '！', '？', '\n', '、']) {
        let uncertain = [
            "未確認",
            "不明",
            "確認でき",
            "情報がありません",
            "情報はありません",
            "記載されていません",
            "記載がありません",
            "保証できません",
        ]
        .iter()
        .any(|s| sentence.contains(s));
        if uncertain {
            continue;
        }
        for (field, terms) in [
            ("rain", vec!["雨音"]),
            ("light", vec!["軽い", "軽量", "FPS", "VRAM"]),
            ("night", vec!["夜景", "夜の"]),
            ("game", vec!["ゲームなし", "ゲームがない"]),
        ] {
            if terms.iter().any(|term| sentence.contains(term))
                && !worlds
                    .iter()
                    .all(|w| w["attributes"][field] == (field != "game"))
            {
                return false;
            }
        }
        for (platform, term) in [("quest", "Quest"), ("pc", "PC")] {
            if sentence.contains(term)
                && !worlds.iter().all(|w| {
                    w["platforms"]
                        .as_array()
                        .is_some_and(|p| p.iter().any(|v| v == platform))
                })
            {
                return false;
            }
        }
    }
    true
}
pub fn fallback(worlds: &[Value]) -> String {
    let names = worlds
        .iter()
        .filter_map(|w| w["name"].as_str())
        .map(|n| format!("「{n}」"))
        .collect::<Vec<_>>()
        .join("、");
    let unknown = [("rain", "雨音"), ("light", "軽さ"), ("night", "夜の雰囲気")]
        .iter()
        .filter(|(field, _)| worlds.iter().any(|w| w["attributes"][*field].is_null()))
        .map(|(_, label)| *label)
        .collect::<Vec<_>>();
    let mut limits = unknown.join("・");
    if worlds.iter().any(|w| w["platforms"].is_null()) {
        if !limits.is_empty() {
            limits.push_str("・");
        }
        limits.push_str("対応機種");
    }
    let note = if limits.is_empty() {
        "詳しい条件はカードの出典をご確認ください。".to_string()
    } else {
        format!("{limits}が未確認の候補を含みます。")
    };
    format!("登録情報から、{names}を候補として表示します。作者の説明・タグと定員はカードで確認できます。{note}気になるワールドについて、確認したいことを教えてください。")
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn unknown_claims_are_not_promoted_to_facts() {
        let worlds = vec![
            json!({"name":"Rainy Daze","platforms":null,"attributes":{"rain":null,"light":null}}),
        ];
        assert!(!grounded("Rainy Dazeは雨音を楽しめる場所です。", &worlds));
        assert!(!grounded("Questで遊べます。", &worlds));
        assert!(!grounded("platformsがnullなので未確認です。", &worlds));
        assert!(grounded("雨音は未確認です。Quest対応も不明です。", &worlds));
        let known = vec![json!({"platforms":["quest"],"attributes":{"rain":true}})];
        assert!(grounded("雨音を楽しめます。Quest対応です。", &known));
        assert!(fallback(&worlds).contains("未確認"));
    }
}

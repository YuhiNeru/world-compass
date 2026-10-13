use crate::{preferences, vrchat, AppState};
use reqwest::blocking::{Client, RequestBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, io::Read, path::Path};
use tauri::Manager;

pub const PAGINATION_LIMIT: &str = "APIのページ巡回上限に達しました";
fn pagination_limit(status: u16, method: &str, path: &str, body: Option<&Value>) -> bool {
    if status != 403 || method != "GET" {
        return false;
    }
    let Ok(url) = reqwest::Url::parse(&format!("https://api.vrchat.cloud/api/1/{path}")) else {
        return false;
    };
    url.path() == "/api/1/worlds"
        && url.query_pairs().any(|(key, value)| {
            key == "offset" && value.parse::<u64>().is_ok_and(|offset| offset > 0)
        })
        && body
            .and_then(|value| value["error"]["message"].as_str())
            .is_some_and(|message| {
                message
                    .trim()
                    .trim_matches('"')
                    .eq_ignore_ascii_case("you've gone too far, pagination stops here.")
            })
}

#[derive(Default, Serialize, Deserialize)]
pub struct Session {
    cookies: BTreeMap<String, String>,
    display: String,
    #[serde(skip)]
    pending: Vec<String>,
    #[serde(skip)]
    pub verified: bool,
}
impl Session {
    pub fn status(&self) -> Value {
        json!({"loggedIn":self.verified,"saved":self.cookies.contains_key("auth"),"displayName":self.display,"twoFactorMethods":self.pending})
    }
    pub fn apply(&self, request: RequestBuilder) -> RequestBuilder {
        if self.cookies.is_empty() {
            request
        } else {
            request.header(
                reqwest::header::COOKIE,
                self.cookies
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        }
    }
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    fn receive(&mut self, headers: &reqwest::header::HeaderMap) {
        for header in headers.get_all(reqwest::header::SET_COOKIE) {
            if let Some((name, value)) = header
                .to_str()
                .ok()
                .and_then(|s| s.split(';').next())
                .and_then(|s| s.split_once('='))
            {
                if ["auth", "twoFactorAuth"].contains(&name) && valid_token(value) {
                    self.cookies.insert(name.into(), value.into());
                }
            }
        }
    }
    pub fn load(root: &Path) -> Self {
        std::fs::read(root.join("userdata/vrchat-session.bin"))
            .ok()
            .and_then(|b| protect(&b, false).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    fn save(&self, root: &Path) -> Result<(), String> {
        if !self.verified {
            return Ok(());
        }
        let bytes = protect(
            &serde_json::to_vec(self).map_err(|_| "セッション保存に失敗しました")?,
            true,
        )?;
        let temp = root.join("userdata/vrchat-session.tmp");
        std::fs::write(&temp, bytes).map_err(|_| "セッション保存に失敗しました")?;
        std::fs::rename(temp, root.join("userdata/vrchat-session.bin"))
            .map_err(|_| "セッション保存に失敗しました".into())
    }
    pub fn request(
        &mut self,
        s: &AppState,
        path: &str,
        method: &str,
        body: Option<Value>,
        basic: Option<(&str, &str)>,
        cancel: bool,
    ) -> Result<Value, String> {
        let contact = preferences::load(&s.root).api_contact;
        preferences::validate_contact(&contact)?;
        let mut api = s.api.lock().map_err(|_| "APIの処理に失敗しました")?;
        while api.next_request > vrchat::now() {
            if cancel && s.corpus_cancel.load(std::sync::atomic::Ordering::SeqCst) {
                return Err("取得を中断しました".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        api.next_request = vrchat::now() + 2;
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(20))
            .user_agent(format!(
                "WorldCompass/{} {contact}",
                env!("CARGO_PKG_VERSION")
            ))
            .build()
            .map_err(|_| "APIの初期化に失敗しました")?;
        let url = format!("https://api.vrchat.cloud/api/1/{path}");
        let mut request = self.apply(match method {
            "POST" => client.post(url),
            "PUT" => client.put(url),
            _ => client.get(url),
        });
        if let Some((user, password)) = basic {
            request = request.basic_auth(encode(user), Some(encode(password)));
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .map_err(|_| "VRChat APIに接続できませんでした。保存済みデータは保持しています")?;
        let status = response.status().as_u16();
        if status == 429 {
            let delay = response
                .headers()
                .get("Retry-After")
                .and_then(|h| h.to_str().ok())
                .and_then(|s| vrchat::retry_after(s, vrchat::now()))
                .unwrap_or(60)
                .max(2);
            api.next_request = vrchat::now().saturating_add(delay);
            return Err(format!(
                "APIのアクセス制限です。{delay}秒待機して再開します"
            ));
        }
        self.receive(response.headers());
        let mut bytes = Vec::new();
        response
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "API応答の読み取りに失敗しました")?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err("API応答のサイズ上限です".into());
        }
        let parsed: Option<Value> = serde_json::from_slice(&bytes).ok();
        match status {
            200..=299 => {}
            401 | 403 => {
                if pagination_limit(status, method, path, parsed.as_ref()) {
                    return Err(PAGINATION_LIMIT.into());
                }
                self.verified = false;
                return Err(api_error(status, parsed.as_ref()));
            }
            404 => return Err("公開ワールドが見つかりませんでした".into()),
            _ => {
                api.next_request = vrchat::now() + 30;
                return Err(format!(
                    "VRChat APIのエラーです（{status}）。取得済み情報は保持しています"
                ));
            }
        }
        parsed.ok_or("API応答の形式が不正です".into())
    }
    pub fn check(&mut self, s: &AppState) -> Result<(), String> {
        let result = self.request(s, "auth/user", "GET", None, None, false)?;
        self.accept_user(&result)?;
        self.save(&s.root)
    }
    fn accept_user(&mut self, result: &Value) -> Result<(), String> {
        self.pending = result["requiresTwoFactorAuth"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        self.verified = result["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("usr_"))
            && self.pending.is_empty();
        if self.verified {
            self.display = result["displayName"]
                .as_str()
                .unwrap_or("VRChatユーザー")
                .into();
        }
        if !self.verified && self.pending.is_empty() {
            return Err("ログイン状態を確認できませんでした".into());
        }
        Ok(())
    }
}
fn valid_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-./=+".contains(&c))
}
fn api_error(status: u16, body: Option<&Value>) -> String {
    let message = body
        .and_then(|b| b["error"]["message"].as_str())
        .unwrap_or("");
    let lower = message.to_lowercase();
    if lower.contains("invalid") || lower.contains("incorrect") {
        return format!("VRChatがユーザー名・パスワード、または認証コードを受け付けませんでした（HTTP {status}）。表示名ではなくログイン用ユーザー名を確認してください。");
    }
    if lower.contains("missing credentials") {
        return format!("VRChatがログインセッションを確認できませんでした（HTTP {status}）。ログイン情報を消去して再ログインしてください。");
    }
    if status == 403 && message.is_empty() {
        return "VRChatが接続を拒否しました（HTTP 403）。公式サイトでログイン状態を確認してから再試行してください。".into();
    }
    // Only the API error message is shown, never the full user response, cookies or request headers.
    let safe = if message.len() <= 300
        && !lower.contains("cookie")
        && !lower.contains("token")
        && !lower.contains("password=")
        && !message.chars().any(|c| c.is_control())
    {
        message
    } else {
        ""
    };
    format!(
        "VRChatの認証エラー（HTTP {status}）{}。ログイン情報または認証コードを確認してください。",
        if safe.is_empty() {
            String::new()
        } else {
            format!("：{safe}")
        }
    )
}
fn diagnostic(root: &Path, phase: &str, result: Result<Value, String>) -> Result<Value, String> {
    let report = match &result {
        Ok(value) => {
            json!({"phase":phase,"status":if value["loggedIn"]==true{"loggedIn"}else{"twoFactorRequired"},"twoFactorMethods":value["twoFactorMethods"],"at":vrchat::now()})
        }
        Err(error) => json!({"phase":phase,"status":"error","message":error,"at":vrchat::now()}),
    };
    // This local diagnostic contains only the public status/error, never credentials or cookies.
    let _ = std::fs::write(
        root.join("logs/vrchat-login-status.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    );
    result
}
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
#[cfg(windows)]
fn protect(bytes: &[u8], encrypt: bool) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes
            .len()
            .try_into()
            .map_err(|_| "セッションサイズが不正です")?,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    unsafe {
        let ok = if encrypt {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            return Err("Windowsのセッション暗号化に失敗しました。再ログインしてください".into());
        }
        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData as *mut _);
        Ok(result)
    }
}
#[cfg(not(windows))]
fn protect(_bytes: &[u8], _encrypt: bool) -> Result<Vec<u8>, String> {
    Err("ログイン保存はWindows版で利用できます".into())
}

#[tauri::command]
pub fn vrchat_auth_status(s: tauri::State<AppState>) -> Result<Value, String> {
    Ok(s.auth
        .lock()
        .map_err(|_| "ログイン状態の取得に失敗しました")?
        .status())
}
#[tauri::command]
pub async fn vrchat_login(
    app: tauri::AppHandle,
    username: String,
    password: String,
) -> Result<Value, String> {
    if username.is_empty() || password.is_empty() || username.len() > 256 || password.len() > 1024 {
        return Err("ユーザー名とパスワードを入力してください".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let s = app.state::<AppState>();
        let result = (|| {
            if s.corpus_busy.load(std::sync::atomic::Ordering::SeqCst) {
                return Err("取得を中断してからログインしてください".into());
            }
            let mut session = s.auth.lock().map_err(|_| "ログイン処理に失敗しました")?;
            // Never store credentials or return the full CurrentUser response to the WebView.
            if session.verified {
                session.check(&s)?;
                return Ok(session.status());
            }
            session.clear();
            let result = session.request(
                &s,
                "auth/user",
                "GET",
                None,
                Some((&username, &password)),
                false,
            )?;
            session.accept_user(&result)?;
            session.save(&s.root)?;
            Ok(session.status())
        })();
        diagnostic(&s.root, "login", result)
    })
    .await
    .map_err(|_| "ログイン処理に失敗しました")?
}
#[tauri::command]
pub async fn vrchat_verify(
    app: tauri::AppHandle,
    method: String,
    code: String,
) -> Result<Value, String> {
    if !["totp", "emailOtp"].contains(&method.as_str())
        || !(6..=8).contains(&code.len())
        || !code.bytes().all(|b| b.is_ascii_digit())
    {
        return Err("認証コードを確認してください".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let s = app.state::<AppState>();
        let result = (|| {
            let mut session = s.auth.lock().map_err(|_| "認証処理に失敗しました")?;
            if !session.pending.contains(&method) {
                return Err("この認証方法は要求されていません".into());
            }
            let result = session.request(
                &s,
                &format!("auth/twofactorauth/{method}/verify"),
                "POST",
                Some(json!({"code":code})),
                None,
                false,
            )?;
            if result["verified"] != true {
                return Err("認証コードが一致しませんでした".into());
            }
            session.check(&s)?;
            Ok(session.status())
        })();
        diagnostic(&s.root, "twoFactor", result)
    })
    .await
    .map_err(|_| "認証処理に失敗しました")?
}
#[tauri::command]
pub async fn vrchat_restore(app: tauri::AppHandle) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let s = app.state::<AppState>();
        let result = (|| {
            let mut session = s.auth.lock().map_err(|_| "認証処理に失敗しました")?;
            session.check(&s)?;
            Ok(session.status())
        })();
        diagnostic(&s.root, "restore", result)
    })
    .await
    .map_err(|_| "認証処理に失敗しました")?
}
#[tauri::command]
pub fn vrchat_forget(s: tauri::State<AppState>) -> Result<(), String> {
    if s.corpus_busy.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("取得を中断してからログイン情報を消去してください".into());
    }
    s.auth
        .lock()
        .map_err(|_| "ログイン情報の消去に失敗しました")?
        .clear();
    let path = s.root.join("userdata/vrchat-session.bin");
    if path.exists() {
        std::fs::remove_file(path).map_err(|_| "保存済みセッションの消去に失敗しました")?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pagination_403_is_distinct_from_credentials_and_other_endpoints() {
        let body = json!({"error":{"message":"\"you've gone too far, pagination stops here.\""}});
        assert!(pagination_limit(
            403,
            "GET",
            "worlds?offset=1000&n=100",
            Some(&body)
        ));
        assert!(!pagination_limit(
            401,
            "GET",
            "worlds?offset=1000",
            Some(&body)
        ));
        assert!(!pagination_limit(
            403,
            "GET",
            "worlds?offset=0",
            Some(&body)
        ));
        assert!(!pagination_limit(
            403,
            "GET",
            "auth/user?offset=1000",
            Some(&body)
        ));
        assert!(!pagination_limit(
            403,
            "POST",
            "worlds?offset=1000",
            Some(&body)
        ));
        assert!(!pagination_limit(
            403,
            "GET",
            "worlds?offset=1000",
            Some(&json!({"error":{"message":"Missing Credentials"}}))
        ));
    }
    #[test]
    fn two_factor_and_api_errors_are_explicit_without_exposing_user_data() {
        let mut session = Session::default();
        session
            .accept_user(&json!({"requiresTwoFactorAuth":["emailOtp"]}))
            .unwrap();
        assert!(!session.verified);
        assert_eq!(session.status()["twoFactorMethods"], json!(["emailOtp"]));
        session.accept_user(&json!({"id":"usr_test","displayName":"Test","authToken":"not-for-webview","email":"not-for-webview@example.com"})).unwrap();
        assert!(session.verified);
        assert!(!session.status().to_string().contains("not-for-webview"));
        let error = api_error(
            401,
            Some(&json!({"error":{"message":"Invalid Username or Password"}})),
        );
        assert!(error.contains("HTTP 401"));
        assert!(error.contains("ログイン用ユーザー名"));
        assert!(
            !api_error(401, Some(&json!({"error":{"message":"authcookie_secret"}})))
                .contains("secret")
        );
    }
    #[test]
    fn credentials_and_cookies_are_bounded() {
        assert_eq!(encode("a:b +日"), "a%3Ab%20%2B%E6%97%A5");
        assert!(!valid_token("x; evil=y"));
        assert!(!valid_token("x\r\ny"));
        let s = Session {
            cookies: BTreeMap::from([("auth".into(), "secret".into())]),
            ..Default::default()
        };
        assert!(!s.status().to_string().contains("secret"));
    }
    #[test]
    #[cfg(windows)]
    fn session_is_encrypted_for_this_windows_user() {
        let secret = b"test-only-cookie";
        let encrypted = protect(secret, true).unwrap();
        assert_ne!(encrypted, secret);
        assert_eq!(protect(&encrypted, false).unwrap(), secret);
        assert!(protect(b"not-encrypted", false).is_err());
    }
}

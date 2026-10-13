use crate::setup;
use reqwest::blocking::Client;
use serde_json::{json, Value};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::{
    io::{BufRead, BufReader},
    net::TcpListener,
    path::Path,
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
    time::{Duration, Instant},
};
pub struct Server {
    pub child: Child,
    pub endpoint: String,
    pub key: String,
    pub last_used: Instant,
    pub device: String,
    #[cfg(windows)]
    job: usize,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.job as _);
        }
    }
}
impl Server {
    pub fn start(
        root: &Path,
        kind: &str,
        pid: &AtomicU32,
        cancel: &AtomicBool,
    ) -> Result<Self, String> {
        let result = Self::start_inner(root, kind, pid, cancel, false);
        if result.is_err()
            && kind == "chat"
            && crate::preferences::load(root).backend == "vulkan"
            && !cancel.load(Ordering::SeqCst)
        {
            return Self::start_inner(root, kind, pid, cancel, true);
        }
        result
    }
    fn start_inner(
        root: &Path,
        kind: &str,
        pid: &AtomicU32,
        cancel: &AtomicBool,
        force_cpu: bool,
    ) -> Result<Self, String> {
        let mut prefs = crate::preferences::load(root);
        if force_cpu {
            prefs.backend = "cpu".into();
        }
        let model_id = if kind == "chat" {
            prefs.model.as_str()
        } else {
            kind
        };
        let runtime_id = if kind == "chat" && prefs.backend == "vulkan" {
            "runtime-vulkan"
        } else {
            "runtime"
        };
        for a in setup::manifest()
            .assets
            .into_iter()
            .filter(|a| a.id == runtime_id || a.id == model_id)
        {
            setup::verify_installed(root, &a)?;
        }
        let exe = setup::runtime_path_for(root, runtime_id)?;
        let model = setup::model_path(root, model_id)?;
        let socket = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        let port = socket.local_addr().map_err(|e| e.to_string())?.port();
        let key = uuid::Uuid::new_v4().to_string();
        let mut command = Command::new(exe);
        command.arg("-m").arg(model).args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--api-key",
            &key,
            "--ctx-size",
            "4096",
            "--threads-batch",
            &prefs.threads.to_string(),
            "--threads",
            &prefs.threads.to_string(),
            "--parallel",
            "1",
            "--n-gpu-layers",
            if runtime_id == "runtime-vulkan" {
                "99"
            } else {
                "0"
            },
            "--no-webui",
        ]);
        if kind == "embedding" {
            command.args(["--embedding", "--pooling", "last"]);
        }
        if runtime_id == "runtime-vulkan" {
            command.args(["--device", "Vulkan0"]);
        }
        command.stdout(Stdio::null()).stderr(Stdio::null());
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        if cancel.load(Ordering::SeqCst) {
            return Err("推論を中断しました".into());
        }
        drop(socket);
        let mut child = command.spawn().map_err(|e| format!("推論起動失敗: {e}"))?;
        pid.store(child.id(), Ordering::SeqCst);
        #[cfg(windows)]
        let job = match attach_job(&child) {
            Ok(job) => job,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
        };
        let mut server = Self {
            child,
            endpoint: format!("http://127.0.0.1:{port}"),
            key,
            last_used: Instant::now(),
            device: if runtime_id == "runtime-vulkan" {
                "vulkan"
            } else {
                "cpu"
            }
            .into(),
            #[cfg(windows)]
            job,
        };
        let client = local_client(2)?;
        for _ in 0..120 {
            if cancel.load(Ordering::SeqCst) {
                return Err("推論を中断しました".into());
            }
            if server
                .child
                .try_wait()
                .map_err(|e| e.to_string())?
                .is_some()
            {
                return Err("推論プロセスが終了しました".into());
            }
            if client
                .get(format!("{}/health", server.endpoint))
                .bearer_auth(&server.key)
                .send()
                .is_ok_and(|r| r.status().is_success())
            {
                return Ok(server);
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        Err("推論起動タイムアウト".into())
    }
    pub fn chat(&mut self, prompt: &str, input: &Value, schema: Value) -> Result<Value, String> {
        self.last_used = Instant::now();
        let result:Value=local_client(60)?.post(format!("{}/v1/chat/completions",self.endpoint)).bearer_auth(&self.key).json(&json!({"messages":[{"role":"system","content":format!("{prompt}\nOutput JSON Schema: {schema}")},{"role":"user","content":input.to_string()}],"temperature":0,"chat_template_kwargs":{"enable_thinking":false},"reasoning_effort":"none","max_tokens":384,"response_format":{"type":"json_schema","json_schema":{"name":"output","strict":true,"schema":schema}}})).send().and_then(|r|r.error_for_status()).map_err(|e|e.to_string())?.json().map_err(|e|e.to_string())?;
        let text = result["choices"][0]["message"]["content"]
            .as_str()
            .ok_or("empty LLM response")?;
        serde_json::from_str(text).map_err(|e| e.to_string())
    }
    pub fn embed(&mut self, text: &str, query: bool) -> Result<Vec<f32>, String> {
        self.last_used = Instant::now();
        let input = if query {
            format!("Instruct: Given a Japanese world search query, retrieve matching virtual world descriptions\nQuery: {text}")
        } else {
            text.into()
        };
        let result: Value = local_client(30)?
            .post(format!("{}/v1/embeddings", self.endpoint))
            .bearer_auth(&self.key)
            .json(&json!({"input":input}))
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|e| e.to_string())?
            .json()
            .map_err(|e| e.to_string())?;
        let v: Vec<f32> = serde_json::from_value(result["data"][0]["embedding"].clone())
            .map_err(|e| e.to_string())?;
        if v.is_empty() || !v.iter().all(|x| x.is_finite()) {
            return Err("invalid embedding".into());
        }
        Ok(v)
    }
    pub fn generate(
        &mut self,
        messages: Value,
        cancel: &AtomicBool,
        mut emit: impl FnMut(&str),
    ) -> Result<String, String> {
        self.last_used = Instant::now();
        let response=local_client(120)?.post(format!("{}/v1/chat/completions",self.endpoint)).bearer_auth(&self.key).json(&json!({"messages":messages,"temperature":0.45,"top_p":0.9,"repeat_penalty":1.1,"chat_template_kwargs":{"enable_thinking":false},"reasoning_effort":"none","max_tokens":384,"stream":true})).send().and_then(|r|r.error_for_status()).map_err(|e|e.to_string())?;
        let mut reader = BufReader::new(response);
        let mut line = String::new();
        let mut answer = String::new();
        let mut completed = false;
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err("応答を停止しました".into());
            }
            line.clear();
            if reader.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
                break;
            }
            if line.len() > 65536 {
                return Err("推論応答が大きすぎます".into());
            }
            if let Some(data) = line.trim().strip_prefix("data: ") {
                if data == "[DONE]" {
                    completed = true;
                    break;
                }
                let v: Value = serde_json::from_str(data).map_err(|e| e.to_string())?;
                if let Some(text) = v["choices"][0]["delta"]["content"].as_str() {
                    answer.push_str(text);
                    if answer.chars().count() > 4000 {
                        return Err("応答が長すぎます".into());
                    }
                    emit(&answer);
                }
            }
        }
        if !completed {
            return Err("AIの応答が途中で切れました".into());
        }
        if answer.trim().is_empty() {
            return Err("AIから返答がありませんでした".into());
        }
        Ok(answer)
    }
}
#[cfg(windows)]
fn attach_job(child: &Child) -> Result<usize, String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{Foundation::CloseHandle, System::JobObjects::*};
    // KILL_ON_JOB_CLOSE prevents an orphan inference process even if the app crashes.
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as _,
            std::mem::size_of_val(&info) as u32,
        ) == 0
            || AssignProcessToJobObject(job, child.as_raw_handle() as _) == 0
        {
            let error = std::io::Error::last_os_error().to_string();
            CloseHandle(job);
            return Err(error);
        }
        Ok(job as usize)
    }
}
fn local_client(seconds: u64) -> Result<Client, String> {
    Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(seconds))
        .build()
        .map_err(|e| e.to_string())
}
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm = (a.iter().map(|x| x * x).sum::<f32>() * b.iter().map(|x| x * x).sum::<f32>()).sqrt();
    if norm > 0. {
        dot / norm
    } else {
        0.
    }
}

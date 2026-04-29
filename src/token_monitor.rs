use crate::config::TokenMonitorConfig;
use crate::model::{AgentSession, UsageEvent};
use chrono::Utc;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const FLUSH_INTERVAL: Duration = Duration::from_secs(30);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const MAX_BATCH_SIZE: usize = 500;
const MAX_QUEUE_SIZE: usize = 10_000;
const MAX_SEEN_IDS: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    Login,
    Register,
}

impl AuthMode {
    pub fn toggle(self) -> Self {
        match self {
            Self::Login => Self::Register,
            Self::Register => Self::Login,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Login => "登录",
            Self::Register => "注册",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthField {
    ServerUrl,
    Email,
    Password,
    Name,
    Department,
    Submit,
    Enable,
    ClearAuth,
}

#[derive(Debug, Clone)]
pub struct AuthForm {
    pub open: bool,
    pub mode: AuthMode,
    pub selected: usize,
    pub server_url: String,
    pub email: String,
    pub password: String,
    pub name: String,
    pub department: String,
    pub message: String,
}

impl AuthForm {
    pub fn from_config(cfg: &TokenMonitorConfig) -> Self {
        Self {
            open: false,
            mode: AuthMode::Login,
            selected: 0,
            server_url: cfg.server_url.clone(),
            email: cfg.user_id.clone(),
            password: String::new(),
            name: cfg.user_name.clone(),
            department: cfg.department.clone(),
            message: String::new(),
        }
    }

    pub fn fields(&self) -> Vec<AuthField> {
        let mut fields = vec![AuthField::ServerUrl, AuthField::Email, AuthField::Password];
        if self.mode == AuthMode::Register {
            fields.push(AuthField::Name);
            fields.push(AuthField::Department);
        }
        fields.push(AuthField::Submit);
        fields.push(AuthField::Enable);
        fields.push(AuthField::ClearAuth);
        fields
    }

    pub fn selected_field(&self) -> AuthField {
        let fields = self.fields();
        let idx = self.selected.min(fields.len().saturating_sub(1));
        fields[idx]
    }

    pub fn next(&mut self) {
        let len = self.fields().len();
        if len > 0 {
            self.selected = (self.selected + 1) % len;
        }
    }

    pub fn prev(&mut self) {
        let len = self.fields().len();
        if len > 0 {
            self.selected = if self.selected == 0 {
                len - 1
            } else {
                self.selected - 1
            };
        }
    }

    pub fn on_mode_changed(&mut self) {
        let len = self.fields().len();
        if self.selected >= len {
            self.selected = len.saturating_sub(1);
        }
    }

    pub fn clear_secret_fields(&mut self) {
        self.password.clear();
    }

    pub fn edit_char(&mut self, c: char) {
        match self.selected_field() {
            AuthField::ServerUrl => self.server_url.push(c),
            AuthField::Email => self.email.push(c),
            AuthField::Password => self.password.push(c),
            AuthField::Name => self.name.push(c),
            AuthField::Department => self.department.push(c),
            AuthField::Submit | AuthField::Enable | AuthField::ClearAuth => {}
        }
    }

    pub fn is_text_field(&self) -> bool {
        matches!(
            self.selected_field(),
            AuthField::ServerUrl
                | AuthField::Email
                | AuthField::Password
                | AuthField::Name
                | AuthField::Department
        )
    }

    pub fn backspace(&mut self) {
        let target = match self.selected_field() {
            AuthField::ServerUrl => &mut self.server_url,
            AuthField::Email => &mut self.email,
            AuthField::Password => &mut self.password,
            AuthField::Name => &mut self.name,
            AuthField::Department => &mut self.department,
            AuthField::Submit | AuthField::Enable | AuthField::ClearAuth => return,
        };
        target.pop();
    }
}

#[derive(Debug, Clone, Default)]
pub struct ReporterStatus {
    pub queue_len: usize,
    pub total_sent: u64,
    pub total_failed: u64,
    pub total_tokens_sent: u64,
    pub last_ok_at: String,
    pub last_error: String,
}

#[derive(Debug, Clone, Serialize)]
struct UsageRecord {
    client_id: String,
    user_name: String,
    user_id: String,
    department: String,
    source: String,
    model: String,
    vendor: String,
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    request_time: String,
    request_id: String,
    source_app: String,
    endpoint: String,
}

#[derive(Debug, Clone, Serialize)]
struct ClientHeartbeat {
    client_id: String,
    user_name: String,
    user_id: String,
    department: String,
    hostname: String,
    version: String,
}

#[derive(Debug, Clone, Serialize)]
struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Debug, Clone, Serialize)]
struct RegisterRequest {
    name: String,
    email: String,
    department: String,
    password: String,
}

#[derive(Debug, Clone, Deserialize)]
struct AuthResponse {
    employee_id: String,
    name: String,
    department: Option<String>,
    auth_token: String,
}

pub struct TokenMonitorClient {
    config: TokenMonitorConfig,
    queue: VecDeque<UsageRecord>,
    seen_ids: HashSet<String>,
    seen_order: VecDeque<String>,
    last_flush_at: Option<Instant>,
    last_heartbeat_at: Option<Instant>,
    status: ReporterStatus,
    hostname: String,
}

impl TokenMonitorClient {
    pub fn new(config: TokenMonitorConfig) -> Self {
        let hostname = std::env::var("HOSTNAME")
            .or_else(|_| std::env::var("COMPUTERNAME"))
            .unwrap_or_else(|_| "unknown-host".to_string());
        Self {
            config,
            queue: VecDeque::new(),
            seen_ids: HashSet::new(),
            seen_order: VecDeque::new(),
            last_flush_at: None,
            last_heartbeat_at: None,
            status: ReporterStatus::default(),
            hostname,
        }
    }

    pub fn config(&self) -> &TokenMonitorConfig {
        &self.config
    }

    pub fn status(&self) -> &ReporterStatus {
        &self.status
    }

    pub fn is_authenticated(&self) -> bool {
        !self.config.auth_token.trim().is_empty()
            && !self.config.user_id.trim().is_empty()
            && !self.config.user_name.trim().is_empty()
    }

    pub fn auth_label(&self) -> String {
        if self.is_authenticated() {
            format!("{} / {}", self.config.user_name, self.config.user_id)
        } else {
            "未登录".to_string()
        }
    }

    pub fn clear_auth(&mut self) {
        self.config.auth_token.clear();
        self.config.user_id.clear();
        self.config.user_name.clear();
        self.config.department.clear();
        self.config.enabled = false;
        self.status.last_error.clear();
        self.refresh_queue_identity();
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.config.enabled = enabled;
    }

    pub fn collect_session_events(&mut self, sessions: &[AgentSession]) {
        for session in sessions {
            for event in &session.usage_events {
                self.push_event(event);
            }
        }
        self.status.queue_len = self.queue.len();
    }

    pub fn tick(&mut self) {
        if !self.config.enabled
            || !self.is_authenticated()
            || self.config.server_url.trim().is_empty()
        {
            return;
        }

        let now = Instant::now();
        if self
            .last_heartbeat_at
            .is_none_or(|last| now.duration_since(last) >= HEARTBEAT_INTERVAL)
        {
            self.last_heartbeat_at = Some(now);
            if let Err(err) = self.send_heartbeat() {
                self.status.last_error = err;
                self.status.total_failed += 1;
            }
        }

        if self.queue.is_empty() {
            return;
        }
        if self
            .last_flush_at
            .is_some_and(|last| now.duration_since(last) < FLUSH_INTERVAL)
        {
            return;
        }

        self.last_flush_at = Some(now);
        if let Err(err) = self.flush_once() {
            self.status.last_error = err;
            self.status.total_failed += 1;
        }
        self.status.queue_len = self.queue.len();
    }

    pub fn authenticate(&mut self, form: &AuthForm) -> Result<(), String> {
        let server_url = normalize_server_url(&form.server_url)?;
        let auth = match form.mode {
            AuthMode::Login => self.do_login(&server_url, form)?,
            AuthMode::Register => self.do_register(&server_url, form)?,
        };

        self.config.server_url = server_url;
        self.config.user_id = auth.employee_id;
        self.config.user_name = auth.name;
        self.config.department = auth.department.unwrap_or_default();
        self.config.auth_token = auth.auth_token;
        self.config.enabled = true;
        self.status.last_error.clear();
        self.refresh_queue_identity();
        Ok(())
    }

    fn do_login(&self, server_url: &str, form: &AuthForm) -> Result<AuthResponse, String> {
        let email = form.email.trim();
        let password = form.password.trim();
        if email.is_empty() || password.is_empty() {
            return Err("邮箱和密码不能为空".to_string());
        }

        self.post_json(
            &format!("{server_url}/api/auth/login"),
            &LoginRequest {
                email: email.to_string(),
                password: password.to_string(),
            },
            None,
        )
    }

    fn do_register(&self, server_url: &str, form: &AuthForm) -> Result<AuthResponse, String> {
        let name = form.name.trim();
        let email = form.email.trim();
        let password = form.password.trim();
        if name.is_empty() || email.is_empty() || password.is_empty() {
            return Err("注册需要姓名、邮箱和密码".to_string());
        }

        self.post_json(
            &format!("{server_url}/api/auth/register"),
            &RegisterRequest {
                name: name.to_string(),
                email: email.to_string(),
                department: form.department.trim().to_string(),
                password: password.to_string(),
            },
            None,
        )
    }

    fn flush_once(&mut self) -> Result<(), String> {
        while !self.queue.is_empty() {
            let batch_len = self.queue.len().min(MAX_BATCH_SIZE);
            let mut batch = Vec::with_capacity(batch_len);
            for _ in 0..batch_len {
                if let Some(item) = self.queue.pop_front() {
                    batch.push(item);
                }
            }

            let url = format!(
                "{}/api/collect",
                normalize_server_url(&self.config.server_url)?
            );
            if let Err(err) = self.post_json::<_, serde_json::Value>(
                &url,
                &batch,
                Some(self.config.auth_token.as_str()),
            ) {
                for item in batch.into_iter().rev() {
                    self.queue.push_front(item);
                }
                return Err(err);
            }

            let tokens: u64 = batch.iter().map(|r| r.total_tokens).sum();
            self.status.total_sent += batch_len as u64;
            self.status.total_tokens_sent += tokens;
            self.status.last_ok_at = Utc::now().to_rfc3339();
            self.status.last_error.clear();
        }
        Ok(())
    }

    fn send_heartbeat(&mut self) -> Result<(), String> {
        let url = format!(
            "{}/api/clients/heartbeat",
            normalize_server_url(&self.config.server_url)?
        );
        let body = ClientHeartbeat {
            client_id: self.client_id(),
            user_name: self.config.user_name.clone(),
            user_id: self.config.user_id.clone(),
            department: self.config.department.clone(),
            hostname: self.hostname.clone(),
            version: format!("abtop {}", env!("CARGO_PKG_VERSION")),
        };
        self.post_json::<_, serde_json::Value>(&url, &body, Some(self.config.auth_token.as_str()))
            .map(|_| {
                self.status.last_ok_at = Utc::now().to_rfc3339();
                self.status.last_error.clear();
            })
    }

    fn post_json<T: Serialize, R: DeserializeOwned>(
        &self,
        url: &str,
        body: &T,
        bearer_token: Option<&str>,
    ) -> Result<R, String> {
        post_json_with_curl(url, body, bearer_token)
    }

    fn push_event(&mut self, event: &UsageEvent) {
        if !self.seen_ids.insert(event.request_id.clone()) {
            return;
        }
        self.seen_order.push_back(event.request_id.clone());
        while self.seen_order.len() > MAX_SEEN_IDS {
            if let Some(old) = self.seen_order.pop_front() {
                self.seen_ids.remove(&old);
            }
        }

        if self.queue.len() >= MAX_QUEUE_SIZE {
            self.queue.pop_front();
            self.status.total_failed += 1;
            self.status.last_error = "上报队列已满，已丢弃最旧记录".to_string();
        }

        let model = if event.model.trim().is_empty() {
            default_model_for_source_app(&event.source_app).to_string()
        } else {
            event.model.clone()
        };

        self.queue.push_back(UsageRecord {
            client_id: self.client_id(),
            user_name: self.config.user_name.clone(),
            user_id: self.config.user_id.clone(),
            department: self.config.department.clone(),
            source: "client".to_string(),
            vendor: infer_provider_from_model(&model, &event.source_app).to_string(),
            model,
            prompt_tokens: event.prompt_tokens,
            completion_tokens: event.completion_tokens,
            total_tokens: event.total_tokens,
            request_time: normalize_request_time(&event.request_time),
            request_id: event.request_id.clone(),
            source_app: event.source_app.clone(),
            endpoint: event.endpoint.clone(),
        });
    }

    fn client_id(&self) -> String {
        format!("{}@{}#abtop", self.config.user_id, self.hostname)
    }

    fn refresh_queue_identity(&mut self) {
        let client_id = self.client_id();
        let user_name = self.config.user_name.clone();
        let user_id = self.config.user_id.clone();
        let department = self.config.department.clone();
        for record in &mut self.queue {
            record.client_id = client_id.clone();
            record.user_name = user_name.clone();
            record.user_id = user_id.clone();
            record.department = department.clone();
        }
    }
}

fn normalize_server_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("Tokens-Monitor server_url 不能为空".to_string());
    }
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return Err("server_url 需要以 http:// 或 https:// 开头".to_string());
    }
    Ok(trimmed.to_string())
}

fn normalize_request_time(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        Utc::now().to_rfc3339()
    } else {
        trimmed.to_string()
    }
}

fn default_model_for_source_app(source_app: &str) -> &'static str {
    match source_app {
        "claude" => "claude",
        "codex" => "gpt",
        _ => "unknown-model",
    }
}

fn infer_provider_from_model<'a>(model: &'a str, source_app: &'a str) -> &'a str {
    let model_lower = model.to_ascii_lowercase();
    if model_lower.contains("claude") {
        return "anthropic";
    }
    if model_lower.starts_with("gpt-")
        || model_lower.starts_with("o1")
        || model_lower.starts_with("o3")
        || model_lower.starts_with("o4")
        || model_lower.starts_with("chatgpt")
    {
        return "openai";
    }
    if model_lower.contains("gemini") || model_lower.contains("gemma") {
        return "google";
    }
    if model_lower.contains("deepseek") {
        return "deepseek";
    }
    if model_lower.contains("qwen") || model_lower.contains("qwq") {
        return "qwen";
    }
    if model_lower.contains("glm") || model_lower.contains("chatglm") {
        return "zhipu";
    }
    if model_lower.contains("moonshot") || model_lower.contains("kimi") {
        return "moonshot";
    }
    if model_lower.contains("doubao") {
        return "doubao";
    }
    if model_lower.contains("yi-") {
        return "yi";
    }
    if model_lower.contains("spark") {
        return "spark";
    }
    if model_lower.contains("mistral")
        || model_lower.contains("mixtral")
        || model_lower.contains("codestral")
    {
        return "mistral";
    }
    match source_app {
        "claude" => "anthropic",
        "codex" => "openai",
        _ => "unknown",
    }
}

fn post_json_with_curl<T: Serialize, R: DeserializeOwned>(
    url: &str,
    body: &T,
    bearer_token: Option<&str>,
) -> Result<R, String> {
    let payload = serde_json::to_vec(body).map_err(|e| e.to_string())?;
    let mut cmd = Command::new("curl");
    cmd.args([
        "-sS",
        "-m",
        "30",
        "-X",
        "POST",
        url,
        "-H",
        "Content-Type: application/json; charset=utf-8",
        "-w",
        "\n%{http_code}",
        "--data-binary",
        "@-",
    ]);
    if let Some(token) = bearer_token.filter(|t| !t.trim().is_empty()) {
        cmd.arg("-H")
            .arg(format!("Authorization: Bearer {}", token.trim()));
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动 curl 失败: {e}"))?;

    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| "无法获取 curl stdin".to_string())?;
        stdin.write_all(&payload).map_err(|e| e.to_string())?;
    }

    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("curl 退出失败: {}", output.status)
        } else {
            stderr
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut parts = stdout.rsplitn(2, '\n');
    let status_line = parts.next().unwrap_or("").trim();
    let body_text = parts.next().unwrap_or("").trim();
    let status: u16 = status_line
        .parse()
        .map_err(|_| format!("无法解析 HTTP 状态码: {status_line}"))?;

    if status >= 400 {
        return Err(if body_text.is_empty() {
            format!("HTTP {status}")
        } else {
            format!("HTTP {status}: {body_text}")
        });
    }

    serde_json::from_str(body_text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_provider_prefers_model_name() {
        assert_eq!(
            infer_provider_from_model("claude-sonnet-4-6", "claude"),
            "anthropic"
        );
        assert_eq!(
            infer_provider_from_model("gpt-5.4-codex", "codex"),
            "openai"
        );
        assert_eq!(infer_provider_from_model("qwen-max", "codex"), "qwen");
    }

    #[test]
    fn auth_form_fields_change_with_mode() {
        let mut form = AuthForm::from_config(&TokenMonitorConfig::default());
        assert_eq!(form.fields().len(), 6);
        form.mode = AuthMode::Register;
        assert_eq!(form.fields().len(), 8);
    }
}

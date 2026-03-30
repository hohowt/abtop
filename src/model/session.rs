use serde::Deserialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Account-level rate limit info (shared across all sessions).
#[derive(Debug, Clone, Default)]
pub struct RateLimitInfo {
    /// "claude" or "codex"
    #[allow(dead_code)]
    pub source: String,
    /// 5-hour window usage percentage (0-100)
    pub five_hour_pct: Option<f64>,
    /// 5-hour window reset timestamp (epoch seconds)
    pub five_hour_resets_at: Option<u64>,
    /// 7-day window usage percentage (0-100)
    pub seven_day_pct: Option<f64>,
    /// 7-day window reset timestamp (epoch seconds)
    pub seven_day_resets_at: Option<u64>,
    /// When this data was last updated
    pub updated_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SessionStatus {
    Working,
    Waiting,
    #[allow(dead_code)]
    Error(String),
    Done,
}

#[derive(Debug, Clone)]
pub struct ChildProcess {
    pub pid: u32,
    pub command: String,
    pub mem_kb: u64,
    pub port: Option<u16>,
}

/// A port left open by a process whose parent session has ended.
#[derive(Debug, Clone)]
pub struct OrphanPort {
    pub port: u16,
    pub pid: u32,
    pub command: String,
    pub project_name: String,
}

#[derive(Debug, Clone)]
pub struct SubAgent {
    pub name: String,
    #[allow(dead_code)]
    pub agent_type: String,
    pub status: String,
    pub tokens: u64,
}

#[derive(Debug, Clone)]
pub struct AgentSession {
    /// Which CLI tool this session belongs to: "claude", "codex", etc.
    pub agent_cli: &'static str,
    pub pid: u32,
    pub session_id: String,
    #[allow(dead_code)]
    pub cwd: String,
    pub project_name: String,
    pub started_at: u64,
    pub status: SessionStatus,
    pub model: String,
    pub context_percent: f64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_read: u64,
    pub total_cache_create: u64,
    pub turn_count: u32,
    pub current_tasks: Vec<String>,
    pub mem_mb: u64,
    pub version: String,
    pub git_branch: String,
    pub git_added: u32,
    pub git_modified: u32,
    pub token_history: Vec<u64>,
    pub subagents: Vec<SubAgent>,
    pub mem_file_count: u32,
    pub mem_line_count: u32,
    pub children: Vec<ChildProcess>,
    #[allow(dead_code)]
    pub transcript_offset: u64,
    /// First user prompt text, truncated — used as session title
    pub initial_prompt: String,
    /// First assistant response text (text blocks only) — used as summary fallback
    pub first_assistant_text: String,
}

impl AgentSession {
    pub fn total_tokens(&self) -> u64 {
        self.total_input_tokens + self.total_output_tokens + self.total_cache_read + self.total_cache_create
    }

    /// Tokens that represent new work (input + output), excluding cache hits.
    /// Used for rate calculation to avoid inflated numbers from cache_read.
    pub fn active_tokens(&self) -> u64 {
        self.total_input_tokens + self.total_output_tokens + self.total_cache_create
    }

    pub fn elapsed(&self) -> Duration {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Duration::from_millis(now.saturating_sub(self.started_at))
    }

    pub fn elapsed_display(&self) -> String {
        let secs = self.elapsed().as_secs();
        if secs < 60 {
            format!("{}s", secs)
        } else if secs < 3600 {
            format!("{}m", secs / 60)
        } else {
            format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SessionFile {
    pub pid: u32,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub cwd: String,
    #[serde(rename = "startedAt")]
    pub started_at: u64,
    #[serde(default)]
    #[allow(dead_code)]
    pub kind: String,
}

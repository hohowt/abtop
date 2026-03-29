use super::process::{self, ProcInfo};
use crate::model::{AgentSession, ChildProcess, SessionStatus};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Collector for OpenAI Codex CLI sessions.
///
/// Discovery strategy (no PID session file like Claude):
/// 1. `ps` to find running codex processes
/// 2. `lsof` to map PID → open rollout-*.jsonl file
/// 3. Parse JSONL for session metadata, tokens, tool usage
///
/// JSONL event types:
/// - `session_meta`: session ID, cwd, cli_version, model_provider, git info
/// - `event_msg` subtypes: task_started, user_message, token_count, agent_message, task_complete
/// - `response_item`: assistant messages (commentary/final), function_call, function_call_output
/// - `turn_context`: model, cwd, effort, context window size
pub struct CodexCollector {
    sessions_dir: PathBuf,
}

impl CodexCollector {
    pub fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        Self {
            sessions_dir: home.join(".codex").join("sessions"),
        }
    }

    pub fn collect(&mut self, shared: &super::SharedProcessData) -> Vec<AgentSession> {
        if !self.sessions_dir.exists() {
            return vec![];
        }

        // Step 1: Find running codex processes and map to JSONL files
        let codex_pids = Self::find_codex_pids();
        let pid_to_jsonl = Self::map_pid_to_jsonl(&codex_pids);

        let mut sessions = Vec::new();
        let mut seen_jsonl = std::collections::HashSet::new();

        // Active sessions: running codex processes with open JSONL files
        for (pid, jsonl_path) in &pid_to_jsonl {
            if let Some(session) = self.load_session(
                Some(*pid),
                jsonl_path,
                &shared.process_info,
                &shared.children_map,
                &shared.ports,
            ) {
                seen_jsonl.insert(jsonl_path.clone());
                sessions.push(session);
            }
        }

        // Recently finished sessions: scan today's JSONL files not owned by any running process.
        // This ensures Codex sessions transition to Done instead of vanishing.
        if let Some(recent_dir) = Self::today_session_dir(&self.sessions_dir) {
            if let Ok(entries) = fs::read_dir(&recent_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                        continue;
                    }
                    if seen_jsonl.contains(&path) {
                        continue;
                    }
                    // Only show recently finished sessions (< 5 min old)
                    if let Ok(meta) = fs::metadata(&path) {
                        if let Ok(modified) = meta.modified() {
                            let age = std::time::SystemTime::now()
                                .duration_since(modified)
                                .unwrap_or_default();
                            if age.as_secs() > 300 {
                                continue;
                            }
                        }
                    }
                    if let Some(session) = self.load_session(
                        None,
                        &path,
                        &shared.process_info,
                        &shared.children_map,
                        &shared.ports,
                    ) {
                        sessions.push(session);
                    }
                }
            }
        }

        sessions.sort_by_key(|s| std::cmp::Reverse(s.started_at));
        sessions
    }

    /// Get today's session directory path: ~/.codex/sessions/YYYY/MM/DD
    fn today_session_dir(sessions_dir: &Path) -> Option<PathBuf> {
        let now = chrono::Local::now();
        let dir = sessions_dir
            .join(now.format("%Y").to_string())
            .join(now.format("%m").to_string())
            .join(now.format("%d").to_string());
        if dir.exists() { Some(dir) } else { None }
    }

    fn load_session(
        &self,
        pid: Option<u32>,
        jsonl_path: &Path,
        process_info: &HashMap<u32, ProcInfo>,
        children_map: &HashMap<u32, Vec<u32>>,
        ports: &HashMap<u32, Vec<u16>>,
    ) -> Option<AgentSession> {
        let result = parse_codex_jsonl(jsonl_path)?;

        let proc = pid.and_then(|p| process_info.get(&p));
        let mem_mb = proc.map(|p| p.rss_kb / 1024).unwrap_or(0);
        let display_pid = pid.unwrap_or(0);

        let project_name = result
            .cwd
            .rsplit('/')
            .next()
            .unwrap_or("?")
            .to_string();

        // Status detection
        let pid_alive = proc.is_some();
        let status = if !pid_alive {
            SessionStatus::Done
        } else if result.task_complete {
            SessionStatus::Done
        } else {
            let since_activity = std::time::SystemTime::now()
                .duration_since(result.last_activity)
                .unwrap_or_default();
            if since_activity.as_secs() < 30 {
                SessionStatus::Working
            } else {
                let cpu_active = proc.map_or(false, |p| p.cpu_pct > 1.0);
                let has_active_child = pid.map_or(false, |p| {
                    process::has_active_descendant(p, children_map, process_info, 5.0)
                });
                if cpu_active || has_active_child {
                    SessionStatus::Working
                } else {
                    SessionStatus::Waiting
                }
            }
        };

        // Current task from last tool use
        let current_tasks = if !result.current_task.is_empty() {
            vec![result.current_task]
        } else if result.task_complete || !pid_alive {
            vec!["finished".to_string()]
        } else if matches!(status, SessionStatus::Waiting) {
            vec!["waiting for input".to_string()]
        } else {
            vec!["thinking...".to_string()]
        };

        // Context window
        let context_percent = 0.0; // TODO: derive from response token usage when available

        // Children
        let mut children = Vec::new();
        if let Some(p) = pid {
            if let Some(child_pids) = children_map.get(&p) {
                for &cpid in child_pids {
                    if let Some(cproc) = process_info.get(&cpid) {
                        let port = ports.get(&cpid).and_then(|v| v.first().copied());
                        children.push(ChildProcess {
                            pid: cpid,
                            command: cproc.command.clone(),
                            mem_kb: cproc.rss_kb,
                            port,
                        });
                    }
                }
            }
        }

        // Git stats
        let (git_added, git_modified) = process::collect_git_stats(&result.cwd);

        Some(AgentSession {
            agent_cli: "codex",
            pid: display_pid,
            session_id: result.session_id,
            cwd: result.cwd,
            project_name,
            started_at: result.started_at,
            status,
            model: result.model,
            context_percent,
            total_input_tokens: 0,  // Codex JSONL doesn't include per-turn usage in token_count
            total_output_tokens: 0,
            total_cache_read: 0,
            total_cache_create: 0,
            turn_count: result.turn_count,
            current_tasks,
            mem_mb,
            version: result.version,
            git_branch: result.git_branch,
            git_added,
            git_modified,
            token_history: vec![],
            subagents: vec![],
            mem_file_count: 0,
            mem_line_count: 0,
            children,
            transcript_offset: 0,
            initial_prompt: result.initial_prompt,
        })
    }

    /// Find PIDs of running codex processes (the native binary, not node wrapper).
    fn find_codex_pids() -> Vec<u32> {
        let output = Command::new("ps")
            .args(["-eo", "pid,command"])
            .output()
            .ok();

        let mut pids = Vec::new();
        if let Some(output) = output {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines().skip(1) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let cmd = parts[1..].join(" ");
                    // Match the native codex binary running exec/interactive, skip app-server
                    if cmd.contains("/codex") && (cmd.contains(" exec") || cmd.contains(" interactive"))
                        && !cmd.contains("app-server")
                        && !cmd.contains("grep")
                    {
                        if let Ok(pid) = parts[0].trim().parse::<u32>() {
                            pids.push(pid);
                        }
                    }
                }
            }
        }
        pids
    }

    /// Map codex PIDs to their open rollout-*.jsonl files via lsof.
    fn map_pid_to_jsonl(pids: &[u32]) -> HashMap<u32, PathBuf> {
        let mut map = HashMap::new();
        if pids.is_empty() {
            return map;
        }

        // Build lsof command for all PIDs at once
        let pid_args: Vec<String> = pids.iter().map(|p| format!("-p{}", p)).collect();
        let mut args = vec!["-F", "pn"];
        for pa in &pid_args {
            args.push(pa);
        }

        let output = Command::new("lsof")
            .args(&args)
            .output()
            .ok();

        if let Some(output) = output {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut current_pid: Option<u32> = None;
            for line in stdout.lines() {
                if let Some(pid_str) = line.strip_prefix('p') {
                    current_pid = pid_str.parse::<u32>().ok();
                } else if let Some(name) = line.strip_prefix('n') {
                    if let Some(pid) = current_pid {
                        if name.contains("rollout-") && name.ends_with(".jsonl") {
                            map.insert(pid, PathBuf::from(name));
                        }
                    }
                }
            }
        }
        map
    }

}

/// Parsed result from a Codex rollout JSONL file.
struct CodexJSONLResult {
    session_id: String,
    cwd: String,
    started_at: u64,
    model: String,
    version: String,
    git_branch: String,
    context_window: u64,
    turn_count: u32,
    current_task: String,
    task_complete: bool,
    last_activity: std::time::SystemTime,
    initial_prompt: String,
}

/// Parse a Codex rollout-*.jsonl file.
///
/// Event types:
/// - session_meta: session ID, cwd, version, git
/// - event_msg.task_started: context window size
/// - event_msg.token_count: rate limits (handled at app level)
/// - event_msg.user_message: user prompt
/// - event_msg.agent_message: turn count
/// - event_msg.task_complete: session done
/// - response_item (function_call): current tool use
/// - turn_context: model, effort
fn parse_codex_jsonl(path: &Path) -> Option<CodexJSONLResult> {
    let file = fs::File::open(path).ok()?;
    let reader = BufReader::new(file);

    let mut result = CodexJSONLResult {
        session_id: String::new(),
        cwd: String::new(),
        started_at: 0,
        model: String::from("?"),
        version: String::new(),
        git_branch: String::new(),
        context_window: 0,
        turn_count: 0,
        current_task: String::new(),
        task_complete: false,
        last_activity: std::time::UNIX_EPOCH,
        initial_prompt: String::new(),
    };

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        if line.is_empty() {
            continue;
        }

        let val: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue, // partial line at EOF
        };

        // Update last_activity from timestamp
        if let Some(ts_str) = val["timestamp"].as_str() {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts_str) {
                let sys_time = std::time::UNIX_EPOCH
                    + std::time::Duration::from_millis(dt.timestamp_millis() as u64);
                if sys_time > result.last_activity {
                    result.last_activity = sys_time;
                }
            }
        }

        match val["type"].as_str() {
            Some("session_meta") => {
                let payload = &val["payload"];
                if let Some(id) = payload["id"].as_str() {
                    result.session_id = id.to_string();
                }
                if let Some(cwd) = payload["cwd"].as_str() {
                    result.cwd = cwd.to_string();
                }
                if let Some(ver) = payload["cli_version"].as_str() {
                    result.version = ver.to_string();
                }
                // started_at from timestamp
                if let Some(ts) = payload["timestamp"].as_str() {
                    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
                        result.started_at = dt.timestamp_millis() as u64;
                    }
                }
                // Git branch
                if let Some(branch) = payload["git"]["branch"].as_str() {
                    result.git_branch = branch.to_string();
                }
            }

            Some("event_msg") => {
                let payload = &val["payload"];
                match payload["type"].as_str() {
                    Some("task_started") => {
                        if let Some(cw) = payload["model_context_window"].as_u64() {
                            result.context_window = cw;
                        }
                    }
                    Some("user_message") => {
                        if result.initial_prompt.is_empty() {
                            if let Some(msg) = payload["message"].as_str() {
                                result.initial_prompt = msg.chars().take(120).collect();
                            }
                        }
                    }
                    Some("agent_message") => {
                        result.turn_count += 1;
                    }
                    Some("task_complete") => {
                        result.task_complete = true;
                    }
                    _ => {}
                }
            }

            Some("response_item") => {
                let payload = &val["payload"];
                // Track current tool use
                if payload["type"].as_str() == Some("function_call") {
                    if let Some(name) = payload["name"].as_str() {
                        // Extract first arg (typically file path or command)
                        let arg = payload["arguments"]
                            .as_str()
                            .and_then(|s| serde_json::from_str::<Value>(s).ok())
                            .and_then(|v| {
                                v["file_path"]
                                    .as_str()
                                    .or_else(|| v["cmd"].as_str())
                                    .map(|s| s.to_string())
                            })
                            .unwrap_or_default();

                        if arg.is_empty() {
                            result.current_task = name.to_string();
                        } else {
                            // Shorten path: just filename
                            let short = arg.rsplit('/').next().unwrap_or(&arg);
                            result.current_task = format!("{} {}", name, short);
                        }
                    }
                }
            }

            Some("turn_context") => {
                let payload = &val["payload"];
                if let Some(m) = payload["model"].as_str() {
                    result.model = m.to_string();
                }
                if let Some(cw) = payload["model_context_window"].as_u64() {
                    result.context_window = cw;
                }
            }

            _ => {}
        }
    }

    if result.session_id.is_empty() {
        return None;
    }

    Some(result)
}

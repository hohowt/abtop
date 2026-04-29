use std::path::PathBuf;

pub struct AppConfig {
    pub theme: String,
    /// Agent CLI names to exclude from the TUI (e.g. ["codex"] to hide Codex).
    /// Matched case-insensitively against each collector's agent_cli identifier.
    pub hidden_agents: Vec<String>,
    pub token_monitor: TokenMonitorConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TokenMonitorConfig {
    pub enabled: bool,
    pub server_url: String,
    pub user_id: String,
    pub user_name: String,
    pub department: String,
    pub auth_token: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme: "btop".to_string(),
            hidden_agents: Vec::new(),
            token_monitor: TokenMonitorConfig::default(),
        }
    }
}

fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("abtop").join("config.toml"))
}

pub fn load_config() -> AppConfig {
    let path = match config_path() {
        Some(p) => p,
        None => return AppConfig::default(),
    };

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return AppConfig::default(),
    };

    let mut config = AppConfig::default();
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some((key, val)) = line.split_once('=') {
            let key = key.trim();
            // Strip quotes (double or single) and inline comments
            let val = val.trim();
            let val = if let Some(comment_pos) = val.find('#') {
                val[..comment_pos].trim()
            } else {
                val
            };
            if key == "hidden_agents" {
                config.hidden_agents = parse_string_array(val);
                continue;
            }
            let val = val.trim_matches('"').trim_matches('\'');
            if key == "theme" {
                config.theme = val.to_string();
            } else if key == "token_monitor_enabled" {
                config.token_monitor.enabled = matches!(val, "true" | "1" | "yes" | "on");
            } else if key == "token_monitor_server_url" {
                config.token_monitor.server_url = val.to_string();
            } else if key == "token_monitor_user_id" {
                config.token_monitor.user_id = val.to_string();
            } else if key == "token_monitor_user_name" {
                config.token_monitor.user_name = val.to_string();
            } else if key == "token_monitor_department" {
                config.token_monitor.department = val.to_string();
            } else if key == "token_monitor_auth_token" {
                config.token_monitor.auth_token = val.to_string();
            }
        }
    }
    config
}

/// Parse a simple one-line TOML string array like `["a", "b"]`.
/// Returns an empty Vec for malformed input to keep config loading infallible.
fn parse_string_array(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    let Some(inner) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) else {
        return Vec::new();
    };
    inner
        .split(',')
        .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn save_theme(name: &str) -> Result<(), String> {
    let path = config_path().ok_or("no config directory")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    // Read existing config, update theme line (NotFound = fresh file, other errors = fail)
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.to_string()),
    };
    let new_content = rewrite_key_line(&content, "theme", &format!("\"{}\"", name));
    std::fs::write(&path, new_content).map_err(|e| e.to_string())
}

pub fn save_token_monitor(cfg: &TokenMonitorConfig) -> Result<(), String> {
    let path = config_path().ok_or("no config directory")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.to_string()),
    };

    let mut updated = content;
    updated = rewrite_key_line(&updated, "token_monitor_enabled", bool_literal(cfg.enabled));
    updated = rewrite_key_line(
        &updated,
        "token_monitor_server_url",
        &quoted(&cfg.server_url),
    );
    updated = rewrite_key_line(&updated, "token_monitor_user_id", &quoted(&cfg.user_id));
    updated = rewrite_key_line(&updated, "token_monitor_user_name", &quoted(&cfg.user_name));
    updated = rewrite_key_line(
        &updated,
        "token_monitor_department",
        &quoted(&cfg.department),
    );
    updated = rewrite_key_line(
        &updated,
        "token_monitor_auth_token",
        &quoted(&cfg.auth_token),
    );

    std::fs::write(&path, updated).map_err(|e| e.to_string())
}

fn bool_literal(v: bool) -> &'static str {
    if v {
        "true"
    } else {
        "false"
    }
}

fn quoted(v: &str) -> String {
    format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Rewrite (or append) a simple `key = value` line in a config file body.
/// Every other line is preserved verbatim, so unknown keys survive future saves.
fn rewrite_key_line(content: &str, target_key: &str, raw_value: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut found = false;
    for line in content.lines() {
        let is_target_key = line
            .split_once('=')
            .map(|(k, _)| k.trim() == target_key)
            .unwrap_or(false);
        if is_target_key {
            lines.push(format!("{target_key} = {raw_value}"));
            found = true;
        } else {
            lines.push(line.to_string());
        }
    }
    if !found {
        lines.push(format!("{target_key} = {raw_value}"));
    }
    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_string_array_basic() {
        assert_eq!(parse_string_array(r#"["codex"]"#), vec!["codex"]);
        assert_eq!(
            parse_string_array(r#"["codex", "claude"]"#),
            vec!["codex", "claude"]
        );
    }

    #[test]
    fn parse_string_array_quote_styles_and_whitespace() {
        assert_eq!(
            parse_string_array(r#"[ 'codex' , "claude" ]"#),
            vec!["codex", "claude"]
        );
    }

    #[test]
    fn parse_string_array_empty_and_malformed() {
        assert!(parse_string_array("[]").is_empty());
        assert!(parse_string_array("not an array").is_empty());
        assert!(parse_string_array(r#"["a",,]"#)
            .iter()
            .all(|s| !s.is_empty()));
    }

    #[test]
    fn rewrite_theme_preserves_hidden_agents_line() {
        let before = "theme = \"btop\"\nhidden_agents = [\"codex\"]\n";
        let after = rewrite_key_line(before, "theme", "\"dracula\"");
        assert!(after.contains("theme = \"dracula\""));
        assert!(
            after.contains("hidden_agents = [\"codex\"]"),
            "hidden_agents line dropped by rewrite_key_line:\n{after}"
        );
    }

    #[test]
    fn rewrite_theme_preserves_arbitrary_unknown_keys() {
        let before = "# user comment\nfuture_key = 42\ntheme = \"btop\"\n";
        let after = rewrite_key_line(before, "theme", "\"nord\"");
        assert!(after.contains("# user comment"));
        assert!(after.contains("future_key = 42"));
        assert!(after.contains("theme = \"nord\""));
    }

    #[test]
    fn rewrite_theme_appends_when_missing() {
        let before = "hidden_agents = [\"codex\"]\n";
        let after = rewrite_key_line(before, "theme", "\"gruvbox\"");
        assert!(after.contains("hidden_agents = [\"codex\"]"));
        assert!(after.contains("theme = \"gruvbox\""));
    }

    #[test]
    fn load_config_reads_token_monitor_fields() {
        let content = r#"
theme = "btop"
token_monitor_enabled = true
token_monitor_server_url = "https://example.com"
token_monitor_user_id = "me@example.com"
token_monitor_user_name = "Wake"
token_monitor_department = "R&D"
token_monitor_auth_token = "token-123"
"#;

        let path = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(path.path(), content).unwrap();

        let mut cfg = AppConfig::default();
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            if let Some((key, val)) = line.split_once('=') {
                let key = key.trim();
                let val = val.trim().trim_matches('"');
                match key {
                    "token_monitor_enabled" => {
                        cfg.token_monitor.enabled = matches!(val, "true" | "1" | "yes" | "on");
                    }
                    "token_monitor_server_url" => cfg.token_monitor.server_url = val.to_string(),
                    "token_monitor_user_id" => cfg.token_monitor.user_id = val.to_string(),
                    "token_monitor_user_name" => cfg.token_monitor.user_name = val.to_string(),
                    "token_monitor_department" => cfg.token_monitor.department = val.to_string(),
                    "token_monitor_auth_token" => cfg.token_monitor.auth_token = val.to_string(),
                    _ => {}
                }
            }
        }

        assert!(cfg.token_monitor.enabled);
        assert_eq!(cfg.token_monitor.server_url, "https://example.com");
        assert_eq!(cfg.token_monitor.user_id, "me@example.com");
        assert_eq!(cfg.token_monitor.user_name, "Wake");
        assert_eq!(cfg.token_monitor.department, "R&D");
        assert_eq!(cfg.token_monitor.auth_token, "token-123");
    }
}

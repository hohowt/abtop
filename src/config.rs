use std::path::PathBuf;

pub struct AppConfig {
    pub theme: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme: "btop".to_string(),
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
            let val = val.trim_matches('"').trim_matches('\'');
            if key == "theme" {
                config.theme = val.to_string();
            }
        }
    }
    config
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
    let mut lines: Vec<String> = Vec::new();
    let mut found = false;
    for line in content.lines() {
        let is_theme_key = line.split_once('=')
            .map(|(k, _)| k.trim() == "theme")
            .unwrap_or(false);
        if is_theme_key {
            lines.push(format!("theme = \"{}\"", name));
            found = true;
        } else {
            lines.push(line.to_string());
        }
    }
    if !found {
        lines.push(format!("theme = \"{}\"", name));
    }
    std::fs::write(&path, lines.join("\n") + "\n").map_err(|e| e.to_string())
}

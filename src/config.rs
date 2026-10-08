use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct LayoutConfig {
    pub sidebar_percent: u16,
    pub chat_percent: u16,
    #[serde(default)]
    pub sidebar_hidden: bool,
}
impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            sidebar_percent: 32,
            chat_percent: 55,
            sidebar_hidden: false,
        }
    }
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct UserConfig {
    #[serde(default)]
    pub projects: BTreeSet<PathBuf>,
    #[serde(default)]
    pub session_projects: HashMap<String, PathBuf>,
    #[serde(default)]
    pub hidden_projects: HashSet<PathBuf>,
    #[serde(default)]
    pub collapsed_projects: HashSet<PathBuf>,
    #[serde(default)]
    pub pinned_sessions: HashSet<String>,
    #[serde(default)]
    pub renamed_sessions: HashMap<String, String>,
    #[serde(default)]
    pub selected_model: Option<String>,
    #[serde(default)]
    pub desktop_notifications: bool,
    #[serde(default)]
    pub sound_notifications: bool,
    #[serde(default)]
    pub layout: LayoutConfig,
}

pub fn load() -> (UserConfig, Option<String>) {
    let Some(path) = config_path() else {
        return (
            UserConfig::default(),
            Some("Не задан HOME для настроек".into()),
        );
    };
    match fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(config) => (config, None),
            Err(error) => (
                UserConfig::default(),
                Some(format!("Настройки повреждены: {error}")),
            ),
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => (UserConfig::default(), None),
        Err(error) => (
            UserConfig::default(),
            Some(format!("Не удалось прочитать настройки: {error}")),
        ),
    }
}

pub fn save(config: &UserConfig) -> io::Result<()> {
    let path = config_path().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "HOME/XDG_CONFIG_HOME is not set")
    })?;
    let parent = path
        .parent()
        .expect("config file always has a parent directory");
    fs::create_dir_all(parent)?;

    let temp_path = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let bytes = serde_json::to_vec_pretty(config).map_err(io::Error::other)?;
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp_path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    fs::rename(temp_path, path)
}

fn config_path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".config")))
        .map(|root| root.join("claude-code-tui").join("config.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_config_keeps_layout_and_opt_in_notifications() {
        let config: UserConfig = serde_json::from_str(r#"{"projects":[]}"#).unwrap();
        assert_eq!(config.layout.sidebar_percent, 32);
        assert_eq!(config.layout.chat_percent, 55);
        assert!(!config.desktop_notifications && !config.sound_notifications);
        let restored: UserConfig =
            serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
        assert_eq!(restored.layout.sidebar_percent, 32);
    }
}

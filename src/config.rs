use serde::{Deserialize, Serialize};
use std::env;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Config {
    #[serde(default = "default_true")]
    pub normalize_jira_prefix: bool,
    #[serde(default)]
    pub keybindings: Keybindings,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Keybindings {
    #[serde(default = "default_confirm")]
    pub confirm: String,
    #[serde(default = "default_cancel")]
    pub cancel: String,
    #[serde(default = "default_toggle_remotes")]
    pub toggle_remotes: String,
}

fn default_true() -> bool {
    true
}

fn default_confirm() -> String {
    "enter".to_string()
}

fn default_cancel() -> String {
    "esc".to_string()
}

fn default_toggle_remotes() -> String {
    "alt+r".to_string()
}

impl Config {
    pub fn load() -> Self {
        let config_dir = Self::config_dir();
        let config_path = config_dir.join("config.toml");
        
        if let Ok(contents) = std::fs::read_to_string(&config_path) {
            if let Ok(config) = toml::from_str(&contents) {
                return config;
            }
        }
        
        Self::default()
    }
    
    fn config_dir() -> PathBuf {
        // Use herdr's plugin config directory
        let plugin_id = env::var("HERDR_PLUGIN_ID").unwrap_or_else(|_| "herdr-worktree".to_string());
        let home = env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(&home)
            .join(".config")
            .join("herdr")
            .join("plugins")
            .join("config")
            .join(&plugin_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(config.normalize_jira_prefix);
        assert_eq!(config.keybindings.confirm, "enter");
        assert_eq!(config.keybindings.cancel, "esc");
        assert_eq!(config.keybindings.toggle_remotes, "alt+r");
    }
}

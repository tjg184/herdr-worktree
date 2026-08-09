use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};
use std::env;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)]
    pub backend: WorktreeBackend,
    #[serde(default = "default_true")]
    pub normalize_jira_prefix: bool,
    #[serde(default)]
    pub keybindings: Keybindings,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WorktreeBackend {
    Worktrunk,
    Native,
}

impl Default for WorktreeBackend {
    fn default() -> Self {
        Self::Worktrunk
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Keybindings {
    #[serde(default = "default_confirm")]
    pub confirm: KeyBinding,
    #[serde(default = "default_cancel")]
    pub cancel: KeyBinding,
    #[serde(default = "default_toggle_remotes")]
    pub toggle_remotes: KeyBinding,
    #[serde(default = "default_refresh")]
    pub refresh: KeyBinding,
}

#[derive(Debug, Clone)]
pub struct KeyBinding {
    value: String,
    code: KeyCode,
    modifiers: KeyModifiers,
}

impl KeyBinding {
    pub fn matches(&self, key: KeyEvent) -> bool {
        self.code == key.code && self.modifiers == key.modifiers
    }

    pub fn display(&self) -> &str {
        &self.value
    }
}

impl<'de> Deserialize<'de> for KeyBinding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        parse_key_binding(&value).map_err(serde::de::Error::custom)
    }
}

impl Serialize for KeyBinding {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.value)
    }
}

fn parse_key_binding(value: &str) -> Result<KeyBinding, String> {
    let mut modifiers = KeyModifiers::empty();
    let mut key = None;

    for part in value.split('+') {
        let part = part.trim();
        let modifier = match part.to_ascii_lowercase().as_str() {
            "alt" => Some(KeyModifiers::ALT),
            "ctrl" | "control" => Some(KeyModifiers::CONTROL),
            "shift" => Some(KeyModifiers::SHIFT),
            _ => None,
        };

        if let Some(modifier) = modifier {
            if modifiers.contains(modifier) {
                return Err(format!("duplicate modifier in keybinding: {value}"));
            }
            modifiers.insert(modifier);
            continue;
        }

        if key.is_some() {
            return Err(format!("keybinding must contain one key: {value}"));
        }

        key = Some(match part.to_ascii_lowercase().as_str() {
            "enter" => KeyCode::Enter,
            "esc" | "escape" => KeyCode::Esc,
            "tab" => KeyCode::Tab,
            "backspace" => KeyCode::Backspace,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            _ if part.chars().count() == 1 => {
                let character = part.chars().next().unwrap();
                KeyCode::Char(if modifiers.contains(KeyModifiers::SHIFT) {
                    character.to_ascii_uppercase()
                } else {
                    character
                })
            }
            _ => return Err(format!("unsupported keybinding: {value}")),
        });
    }

    let code = key.ok_or_else(|| format!("keybinding is missing a key: {value}"))?;
    Ok(KeyBinding {
        value: value.to_string(),
        code,
        modifiers,
    })
}

fn default_true() -> bool {
    true
}

fn default_confirm() -> KeyBinding {
    parse_key_binding("enter").expect("default keybinding is valid")
}

fn default_cancel() -> KeyBinding {
    parse_key_binding("esc").expect("default keybinding is valid")
}

fn default_toggle_remotes() -> KeyBinding {
    parse_key_binding("alt+r").expect("default keybinding is valid")
}

fn default_refresh() -> KeyBinding {
    parse_key_binding("ctrl+r").expect("default keybinding is valid")
}

impl Default for Config {
    fn default() -> Self {
        Self {
            backend: WorktreeBackend::default(),
            normalize_jira_prefix: default_true(),
            keybindings: Keybindings::default(),
        }
    }
}

impl Default for Keybindings {
    fn default() -> Self {
        Self {
            confirm: default_confirm(),
            cancel: default_cancel(),
            toggle_remotes: default_toggle_remotes(),
            refresh: default_refresh(),
        }
    }
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
        let plugin_id =
            env::var("HERDR_PLUGIN_ID").unwrap_or_else(|_| "herdr-worktree".to_string());
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
        assert_eq!(config.backend, WorktreeBackend::Worktrunk);
        assert_eq!(config.keybindings.confirm.display(), "enter");
        assert_eq!(config.keybindings.cancel.display(), "esc");
        assert_eq!(config.keybindings.toggle_remotes.display(), "alt+r");
        assert_eq!(config.keybindings.refresh.display(), "ctrl+r");
    }

    #[test]
    fn native_backend_deserializes() {
        let config: Config = toml::from_str("backend = \"native\"").unwrap();
        assert_eq!(config.backend, WorktreeBackend::Native);
    }

    #[test]
    fn custom_keybindings_deserialize_and_match_events() {
        let config: Config = toml::from_str(
            r#"
            [keybindings]
            confirm = "ctrl+o"
            cancel = "ctrl+c"
            toggle_remotes = "ctrl+r"
            refresh = "alt+f"
            "#,
        )
        .unwrap();

        assert!(config
            .keybindings
            .confirm
            .matches(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL)));
        assert!(config
            .keybindings
            .cancel
            .matches(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
        assert!(config
            .keybindings
            .toggle_remotes
            .matches(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL)));
        assert!(config
            .keybindings
            .refresh
            .matches(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::ALT)));
    }

    #[test]
    fn shifted_character_keybindings_match_shifted_events() {
        let binding = parse_key_binding("shift+g").unwrap();

        assert!(binding.matches(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT)));
    }

    #[test]
    fn invalid_keybindings_are_rejected() {
        let error = toml::from_str::<Config>("[keybindings]\nconfirm = \"ctrl+alt\"")
            .unwrap_err()
            .to_string();

        assert!(error.contains("keybinding is missing a key"));
    }
}

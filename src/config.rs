//! `~/.config/hogs/config.toml`. Every key is optional.

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Theme name: a bundled theme or a file in ~/.config/hogs/themes/.
    pub theme: Option<String>,
    /// Show apparent size (file length) instead of disk usage.
    pub apparent_size: bool,
    /// Show dotfiles. On by default: hidden dirs like ~/.cache are classic hogs.
    pub show_hidden: bool,
    /// Don't cross into other filesystems.
    pub one_filesystem: bool,
    /// Names to skip while scanning, e.g. [".git", "node_modules"].
    pub exclude: Vec<String>,
    /// "auto" (default), "truecolor", "256" or "none".
    pub color: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: None,
            apparent_size: false,
            show_hidden: true,
            one_filesystem: false,
            exclude: Vec::new(),
            color: None,
        }
    }
}

pub fn config_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"));
    base.join("hogs")
}

pub fn load() -> Result<Config, String> {
    let path = config_dir().join("config.toml");
    if !path.exists() {
        return Ok(Config::default());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

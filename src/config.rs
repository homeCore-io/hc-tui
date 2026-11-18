//! TUI configuration file.
//!
//! All fields have defaults so the file is entirely optional. CLI flags
//! override the corresponding config values when both are present.
//!
//! Default locations follow the XDG base directories:
//! - config: `$XDG_CONFIG_HOME/hc-tui/config.toml` (`~/.config/hc-tui`)
//! - cache:  `$XDG_CACHE_HOME/hc-tui` (`~/.cache/hc-tui`)
//!
//! A `config/config.toml` in the working directory is still read when no
//! XDG config exists, so older checkouts keep working.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default)]
    pub session: SessionConfig,
    pub auto_login: Option<AutoLoginConfig>,
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub base_url: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:8080".to_string(),
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct CacheConfig {
    /// Cache directory. Defaults to the XDG cache directory.
    #[serde(default)]
    pub dir: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SessionConfig {
    /// Persist the session to `{cache_dir}/session.json` and restore it
    /// on next startup, skipping the login screen while it is valid.
    #[serde(default = "default_true")]
    pub persist_token: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            persist_token: true,
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize, Clone)]
pub struct AutoLoginConfig {
    pub username: String,
    pub password: String,
}

impl std::fmt::Debug for AutoLoginConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AutoLoginConfig")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

impl Config {
    /// Load config from `path`. Returns `Config::default()` if the file
    /// does not exist; errors if it exists but cannot be read or parsed.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("failed to parse {}", path.display()))
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

fn xdg_dir(var: &str, fallback: &str) -> PathBuf {
    if let Some(v) = std::env::var_os(var).filter(|v| !v.is_empty()) {
        let p = PathBuf::from(v);
        if p.is_absolute() {
            return p;
        }
    }
    home_dir()
        .map(|h| h.join(fallback))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// `$XDG_CONFIG_HOME/hc-tui`, else `~/.config/hc-tui`.
pub fn default_config_dir() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME", ".config").join("hc-tui")
}

/// `$XDG_CACHE_HOME/hc-tui`, else `~/.cache/hc-tui`.
pub fn default_cache_dir() -> PathBuf {
    xdg_dir("XDG_CACHE_HOME", ".cache").join("hc-tui")
}

/// The config file to read when `--config` was not given.
pub fn default_config_path() -> PathBuf {
    let xdg = default_config_dir().join("config.toml");
    if xdg.exists() {
        return xdg;
    }
    let legacy = PathBuf::from("config/config.toml");
    if legacy.exists() {
        return legacy;
    }
    xdg
}

/// Where backups and exports are written: `~/.homecore/exports`.
pub fn exports_dir() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".homecore")
        .join("exports")
}

/// Where imports are read from: `~/.homecore/imports`.
pub fn imports_dir() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".homecore")
        .join("imports")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_login_debug_hides_password() {
        let a = AutoLoginConfig {
            username: "me".into(),
            password: "hunter2".into(),
        };
        assert!(!format!("{a:?}").contains("hunter2"));
    }

    #[test]
    fn cache_dir_is_optional() {
        let c: Config = toml::from_str("[server]\nbase_url = \"http://x\"\n").unwrap();
        assert!(c.cache.dir.is_none());
        let c: Config = toml::from_str("[cache]\ndir = \"./cache\"\n").unwrap();
        assert_eq!(c.cache.dir.as_deref(), Some("./cache"));
    }
}

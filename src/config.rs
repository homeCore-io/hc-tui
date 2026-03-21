//! TUI configuration file (`config/config.toml`).
//!
//! All fields have defaults so the file is entirely optional.  CLI flags
//! override the corresponding config values when both are present.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

// ---------------------------------------------------------------------------
// Top-level
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Sections
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub base_url: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self { base_url: "http://127.0.0.1:8080".to_string() }
    }
}

#[derive(Debug, Deserialize)]
pub struct CacheConfig {
    pub dir: String,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self { dir: "./cache".to_string() }
    }
}

#[derive(Debug, Deserialize)]
pub struct SessionConfig {
    /// Persist the JWT to `{cache_dir}/session.json` and restore it on next
    /// startup, skipping the login screen while the token is still valid.
    #[serde(default = "default_true")]
    pub persist_token: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self { persist_token: true }
    }
}

fn default_true() -> bool { true }

#[derive(Debug, Deserialize, Clone)]
pub struct AutoLoginConfig {
    pub username: String,
    pub password: String,
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

impl Config {
    /// Load config from `path`.  Returns `Config::default()` silently if the
    /// file does not exist (the file is optional).  Returns an error if the
    /// file exists but cannot be read or parsed.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        toml::from_str(&text)
            .with_context(|| format!("failed to parse {}", path.display()))
    }
}

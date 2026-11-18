//! On-disk cache of the last good data, per user, plus the saved session.
//!
//! Each section (devices, rules, ...) is its own file and is written only
//! from data that came from a successful call, so a failed sync can never
//! replace good cached data with an empty or partial list.
//!
//! Directories are created 0700 and files 0600; writes go to a temporary
//! file first and are renamed into place.

use crate::model::{
    Area, DeviceState, EventEntry, ModeRecord, PluginRecord, Rule, Scene, UserInfo,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

// ── Private file helpers ────────────────────────────────────────────────────

/// Create `dir` (and parents) readable only by the owner.
pub fn ensure_private_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        // `mode` only applies to directories it created; tighten an
        // existing one too.
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir)
    }
}

/// Write `bytes` to `path` as a 0600 file, via a temporary file and a
/// rename so readers never see half a file.
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        ensure_private_dir(parent)?;
    }
    // A unique temporary name, so two writers of the same file never
    // share one.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(format!(".{}.{n}.tmp", std::process::id()));
    let tmp = path.with_file_name(tmp_name);
    {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&tmp, path)
}

// ── Session ─────────────────────────────────────────────────────────────────

/// Saved login at `{cache_dir}/session.json`.
#[derive(Clone, Serialize, Deserialize)]
pub struct SavedSession {
    pub username: String,
    pub token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// Unix seconds when `token` expires.
    #[serde(default)]
    pub expires_at: Option<i64>,
}

impl std::fmt::Debug for SavedSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SavedSession")
            .field("username", &self.username)
            .field("token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

// ── Snapshot ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CacheSnapshot {
    pub devices: Option<Vec<DeviceState>>,
    /// Native scenes only; scene devices come from `devices`.
    pub scenes: Option<Vec<Scene>>,
    pub areas: Option<Vec<Area>>,
    pub rules: Option<Vec<Rule>>,
    pub events: Option<Vec<EventEntry>>,
    pub users: Option<Vec<UserInfo>>,
    pub plugins: Option<Vec<PluginRecord>>,
    pub modes: Option<Vec<ModeRecord>>,
}

impl CacheSnapshot {
    pub fn is_empty(&self) -> bool {
        self.devices.as_ref().is_none_or(Vec::is_empty)
            && self.scenes.as_ref().is_none_or(Vec::is_empty)
            && self.areas.as_ref().is_none_or(Vec::is_empty)
            && self.rules.as_ref().is_none_or(Vec::is_empty)
    }
}

#[derive(Clone, Debug)]
pub struct CacheStore {
    root: PathBuf,
}

impl CacheStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    #[cfg(test)]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn user_dir(&self, username: &str) -> PathBuf {
        self.root.join(sanitize_component(username))
    }

    /// Write every section that is `Some`. Sections that are `None` (the
    /// call failed or was not made) keep whatever file is already there.
    pub fn save_sections(&self, username: &str, snap: &CacheSnapshot) -> Result<()> {
        let dir = self.user_dir(username);
        ensure_private_dir(&dir)
            .with_context(|| format!("creating cache directory {}", dir.display()))?;
        fn put<T: Serialize>(dir: &Path, name: &str, v: &Option<T>) -> Result<()> {
            if let Some(v) = v {
                let bytes = serde_json::to_vec_pretty(v)?;
                let path = dir.join(name);
                write_private(&path, &bytes)
                    .with_context(|| format!("writing cache file {}", path.display()))?;
            }
            Ok(())
        }
        put(&dir, "devices.json", &snap.devices)?;
        put(&dir, "native_scenes.json", &snap.scenes)?;
        put(&dir, "areas.json", &snap.areas)?;
        put(&dir, "rules.json", &snap.rules)?;
        put(&dir, "events.json", &snap.events)?;
        put(&dir, "users.json", &snap.users)?;
        put(&dir, "plugins.json", &snap.plugins)?;
        put(&dir, "modes.json", &snap.modes)?;
        Ok(())
    }

    /// Read whatever sections exist. A missing or unreadable file is just
    /// an absent section.
    pub fn load_snapshot(&self, username: &str) -> CacheSnapshot {
        let dir = self.user_dir(username);
        fn get<T: for<'a> Deserialize<'a>>(dir: &Path, name: &str) -> Option<T> {
            let bytes = std::fs::read(dir.join(name)).ok()?;
            serde_json::from_slice(&bytes).ok()
        }
        CacheSnapshot {
            devices: get(&dir, "devices.json"),
            scenes: get(&dir, "native_scenes.json"),
            areas: get(&dir, "areas.json"),
            rules: get(&dir, "rules.json"),
            events: get(&dir, "events.json"),
            users: get(&dir, "users.json"),
            plugins: get(&dir, "plugins.json"),
            modes: get(&dir, "modes.json"),
        }
    }

    pub fn save_session(&self, session: &SavedSession) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(session)?;
        let path = self.root.join("session.json");
        write_private(&path, &bytes).with_context(|| format!("writing {}", path.display()))
    }

    pub fn load_session(&self) -> Option<SavedSession> {
        let bytes = std::fs::read(self.root.join("session.json")).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn clear_session(&self) -> Result<()> {
        let path = self.root.join("session.json");
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }
}

fn sanitize_component(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "hc-tui-test-{tag}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn rule(id: &str) -> Rule {
        Rule {
            id: id.into(),
            name: id.into(),
            enabled: true,
            priority: 0,
            tags: vec![],
            error: None,
            trigger: None,
            triggers: vec![],
        }
    }

    #[test]
    fn absent_sections_keep_the_old_file() {
        let store = CacheStore::new(temp_dir("sections"));
        let first = CacheSnapshot {
            rules: Some(vec![rule("r1"), rule("r2")]),
            areas: Some(vec![]),
            ..Default::default()
        };
        store.save_sections("me", &first).unwrap();

        // A later sync whose rules call failed: rules is None.
        let second = CacheSnapshot {
            rules: None,
            areas: Some(vec![Area {
                id: "a".into(),
                name: "Kitchen".into(),
                slug: "kitchen".into(),
                device_ids: vec![],
            }]),
            ..Default::default()
        };
        store.save_sections("me", &second).unwrap();

        let loaded = store.load_snapshot("me");
        assert_eq!(loaded.rules.unwrap().len(), 2, "rules must survive");
        assert_eq!(loaded.areas.unwrap().len(), 1);
        assert!(loaded.devices.is_none());
        let _ = std::fs::remove_dir_all(store.root());
    }

    #[cfg(unix)]
    #[test]
    fn files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let store = CacheStore::new(temp_dir("perms"));
        store
            .save_session(&SavedSession {
                username: "me".into(),
                token: "t".into(),
                refresh_token: None,
                expires_at: None,
            })
            .unwrap();
        store
            .save_sections(
                "me",
                &CacheSnapshot {
                    rules: Some(vec![]),
                    ..Default::default()
                },
            )
            .unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(store.root()), 0o700);
        assert_eq!(mode(&store.root().join("session.json")), 0o600);
        assert_eq!(mode(&store.root().join("me")), 0o700);
        assert_eq!(mode(&store.root().join("me").join("rules.json")), 0o600);
        let _ = std::fs::remove_dir_all(store.root());
    }

    #[test]
    fn concurrent_writes_never_mix() {
        let dir = temp_dir("race");
        let path = dir.join("f.json");
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let body = vec![b'a' + i as u8; 1000 * (i + 1)];
                    write_private(&path, &body).unwrap();
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let got = std::fs::read(&path).unwrap();
        assert!(got.iter().all(|b| *b == got[0]), "one writer's bytes only");
        assert_eq!(got.len(), 1000 * (got[0] - b'a' + 1) as usize);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn session_debug_hides_tokens() {
        let s = SavedSession {
            username: "me".into(),
            token: "topsecret".into(),
            refresh_token: Some("alsosecret".into()),
            expires_at: Some(1),
        };
        let out = format!("{s:?}");
        assert!(!out.contains("secret"));
    }
}

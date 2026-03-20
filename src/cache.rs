use crate::api::{Area, DeviceState, EventEntry, PluginRecord, Rule, UserInfo};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CacheSnapshot {
    pub devices: Vec<DeviceState>,
    pub scenes: Vec<crate::api::Scene>,
    pub areas: Vec<Area>,
    pub automations: Vec<Rule>,
    pub events: Vec<EventEntry>,
    pub users: Vec<UserInfo>,
    pub plugins: Vec<PluginRecord>,
}

#[derive(Clone)]
pub struct CacheStore {
    root: PathBuf,
}

impl CacheStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub async fn save_snapshot(&self, username: &str, snapshot: &CacheSnapshot) -> Result<()> {
        let dir = self.user_dir(username);
        tokio::fs::create_dir_all(&dir)
            .await
            .with_context(|| format!("failed to create cache directory {}", dir.display()))?;

        self.write_json(dir.join("devices.json"), &snapshot.devices).await?;
        self.write_json(dir.join("scenes.json"), &snapshot.scenes).await?;
        self.write_json(dir.join("areas.json"), &snapshot.areas).await?;
        self.write_json(dir.join("automations.json"), &snapshot.automations).await?;
        self.write_json(dir.join("events.json"), &snapshot.events).await?;
        self.write_json(dir.join("users.json"), &snapshot.users).await?;
        self.write_json(dir.join("plugins.json"), &snapshot.plugins).await?;
        Ok(())
    }

    pub async fn load_snapshot(&self, username: &str) -> Result<CacheSnapshot> {
        let dir = self.user_dir(username);
        if !dir.exists() {
            return Ok(CacheSnapshot::default());
        }

        Ok(CacheSnapshot {
            devices: self.read_json_or_default(dir.join("devices.json")).await?,
            scenes: self.read_json_or_default(dir.join("scenes.json")).await?,
            areas: self.read_json_or_default(dir.join("areas.json")).await?,
            automations: self.read_json_or_default(dir.join("automations.json")).await?,
            events: self.read_json_or_default(dir.join("events.json")).await?,
            users: self.read_json_or_default(dir.join("users.json")).await?,
            plugins: self.read_json_or_default(dir.join("plugins.json")).await?,
        })
    }

    fn user_dir(&self, username: &str) -> PathBuf {
        self.root.join(sanitize_component(username))
    }

    async fn write_json<T: Serialize>(&self, path: PathBuf, value: &T) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(value)
            .with_context(|| format!("failed to serialize cache file {}", path.display()))?;
        tokio::fs::write(&path, bytes)
            .await
            .with_context(|| format!("failed to write cache file {}", path.display()))?;
        Ok(())
    }

    async fn read_json_or_default<T>(&self, path: PathBuf) -> Result<T>
    where
        T: for<'a> Deserialize<'a> + Default,
    {
        if !Path::new(&path).exists() {
            return Ok(T::default());
        }
        let bytes = tokio::fs::read(&path)
            .await
            .with_context(|| format!("failed to read cache file {}", path.display()))?;
        let parsed = serde_json::from_slice::<T>(&bytes)
            .with_context(|| format!("failed to parse cache file {}", path.display()))?;
        Ok(parsed)
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

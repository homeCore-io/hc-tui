//! One sync path for login, refresh (`r`) and reconnects.
//!
//! Every call runs concurrently. Each result is kept separately, so a
//! failing call costs only its own section: the rest still update, the
//! failure is reported by name, and the on-disk cache for that section
//! is left untouched.

use crate::model::{
    Area, DeviceState, ModeRecord, PluginRecord, Rule, Scene, SystemStatus, UserInfo,
};
use crate::net::HomeCoreClient;
use anyhow::Result;
use serde_json::Value;

/// Which optional calls to make, from the user's permissions.
#[derive(Debug, Clone, Copy, Default)]
pub struct SyncPlan {
    pub users: bool,
    pub plugins: bool,
}

pub type Part<T> = std::result::Result<T, String>;

#[derive(Debug)]
pub struct SyncReport {
    pub me: Part<UserInfo>,
    pub devices: Part<Vec<DeviceState>>,
    pub scenes: Part<Vec<Scene>>,
    pub areas: Part<Vec<Area>>,
    pub rules: Part<Vec<Rule>>,
    /// Raw `/events` entries; the app turns them into rows.
    pub events: Part<Vec<Value>>,
    pub modes: Part<Vec<ModeRecord>>,
    pub status: Part<SystemStatus>,
    /// `None` when the plan skipped the call.
    pub users: Option<Part<Vec<UserInfo>>>,
    pub plugins: Option<Part<Vec<PluginRecord>>>,
}

fn part<T>(r: Result<T>) -> Part<T> {
    r.map_err(|e| e.to_string())
}

impl SyncReport {
    /// `(section, message)` for every call that failed.
    pub fn errors(&self) -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        let mut check = |name: &'static str, e: Option<&String>| {
            if let Some(e) = e {
                out.push((name, e.clone()));
            }
        };
        check("me", self.me.as_ref().err());
        check("devices", self.devices.as_ref().err());
        check("scenes", self.scenes.as_ref().err());
        check("areas", self.areas.as_ref().err());
        check("rules", self.rules.as_ref().err());
        check("events", self.events.as_ref().err());
        check("modes", self.modes.as_ref().err());
        check("status", self.status.as_ref().err());
        check("users", self.users.as_ref().and_then(|r| r.as_ref().err()));
        check(
            "plugins",
            self.plugins.as_ref().and_then(|r| r.as_ref().err()),
        );
        out
    }

    /// True when any failure was an expired session.
    pub fn unauthorized(&self) -> bool {
        self.errors()
            .iter()
            .any(|(_, e)| e == &crate::net::Unauthorized.to_string())
    }
}

pub async fn run_sync(client: HomeCoreClient, plan: SyncPlan) -> SyncReport {
    let (me, devices, scenes, areas, rules, events, modes, status, users, plugins) = tokio::join!(
        client.me(),
        client.list_devices(),
        client.list_scenes(),
        client.list_areas(),
        client.list_rules(),
        client.list_events(50),
        client.list_modes(),
        client.get_system_status(),
        async {
            if plan.users {
                Some(client.list_users().await)
            } else {
                None
            }
        },
        async {
            if plan.plugins {
                Some(client.list_plugins().await)
            } else {
                None
            }
        },
    );
    SyncReport {
        me: part(me),
        devices: part(devices),
        scenes: part(scenes),
        areas: part(areas),
        rules: part(rules),
        events: part(events),
        modes: part(modes),
        status: part(status),
        users: users.map(part),
        plugins: plugins.map(part),
    }
}

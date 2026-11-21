//! System: `GET /system/status`, `/health`, `/system/versions`, core's
//! log level, who is connected (admin) and restarting core.

use crate::app::{App, Load};
use crate::model::settings::{
    Health, HouseTemperature, WsConnection, log_directive, parse_versions, read_log_level,
    step_log_level,
};

/// One finding of `GET /system/vocabulary-check`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct VocabFinding {
    #[serde(default)]
    pub severity: String,
    #[serde(default)]
    pub place: serde_json::Value,
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub message: String,
}

impl VocabFinding {
    /// Where it is, in words: an automation or scene by name, or the device.
    pub fn place_text(&self) -> String {
        let kind = self
            .place
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let name = self.place.get("name").and_then(serde_json::Value::as_str);
        match (kind, name) {
            ("automation", Some(n)) => format!("automation {n}"),
            ("scene", Some(n)) => format!("scene {n}"),
            _ => "the device itself".to_string(),
        }
    }
}

#[derive(Debug, Default)]
pub struct StatusPanel {
    pub loading: bool,
    pub last_refresh: Option<String>,
    pub error: Option<String>,
    pub health: Load<Health>,
    pub versions: Load<Vec<(String, String)>>,
    /// Core's log directive as it says it.
    pub log_level: Load<String>,
    pub connections: Load<Vec<WsConnection>>,
    pub house_temp: Option<HouseTemperature>,
    /// Set while a restart runs.
    pub restarting: Option<String>,
    /// The vocabulary check overlay (`c`), when open.
    pub vocab: Option<Load<Vec<VocabFinding>>>,
}

/// Plugins as the System screen counts them: running, meant to run but
/// not, and turned off.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PluginHealth {
    pub running: usize,
    pub stopped: usize,
    pub off: usize,
}

impl App {
    pub(crate) fn refresh_system_status(&mut self) {
        self.manage.status.loading = true;
        let c = self.client.clone();
        self.spawn(async move { c.get_system_status().await }, |app, res| {
            let p = &mut app.manage.status;
            p.loading = false;
            match res {
                Ok(s) => {
                    p.error = None;
                    p.last_refresh = Some(chrono::Local::now().format("%H:%M:%S").to_string());
                    app.set_system_status(s);
                }
                Err(e) => {
                    p.error = Some(e.to_string());
                    app.error = Some(format!("Failed to load system status: {e}"));
                }
            }
        });
    }

    /// Everything the System screen shows.
    pub(crate) fn load_system_screen(&mut self) {
        self.refresh_system_status();
        let c = self.client.clone();
        self.spawn(
            async move {
                let (h, v, l, t) = tokio::join!(
                    c.health(),
                    c.system_versions(),
                    c.get_log_level(),
                    c.house_temperature()
                );
                Ok((h, v, l, t))
            },
            |app, res: anyhow::Result<_>| {
                let Ok((h, v, l, t)) = res else { return };
                let p = &mut app.manage.status;
                p.health = load_of(h);
                p.versions = load_of(v.map(|v| parse_versions(&v)));
                p.log_level = load_of(l);
                p.house_temp = t.ok();
            },
        );
        if self.is_admin() {
            let c = self.client.clone();
            self.spawn(async move { c.ws_connections().await }, |app, res| {
                app.manage.status.connections = load_of(res.map(|c| c.connections));
            });
        }
    }

    pub fn plugin_health(&self) -> PluginHealth {
        let mut h = PluginHealth::default();
        for p in &self.plugins {
            let running = matches!(p.status.as_str(), "running" | "active" | "online");
            if running {
                h.running += 1;
            } else if p.enabled {
                h.stopped += 1;
            } else {
                h.off += 1;
            }
        }
        h
    }

    /// Whether this user may change core's log level.
    /// `c` on System: what automations, scenes and devices say that devices
    /// won't take. A report; it changes nothing.
    pub fn open_vocabulary_check(&mut self) {
        self.manage.status.vocab = Some(Load::Loading);
        let c = self.client.clone();
        self.spawn(
            async move {
                let v: serde_json::Value = c.get_json("/system/vocabulary-check").await?;
                Ok(serde_json::from_value::<Vec<VocabFinding>>(
                    v.get("findings").cloned().unwrap_or_default(),
                )
                .unwrap_or_default())
            },
            |app, res: anyhow::Result<Vec<VocabFinding>>| {
                if app.manage.status.vocab.is_some() {
                    app.manage.status.vocab = Some(match res {
                        Ok(v) => Load::Ready(v),
                        Err(e) => Load::Failed(e.to_string()),
                    });
                }
            },
        );
    }

    pub fn close_vocabulary_check(&mut self) {
        self.manage.status.vocab = None;
    }

    pub fn can_set_log_level(&self) -> bool {
        self.can("plugins:write") || self.is_admin()
    }

    /// Left and Right on System: one level quieter or louder, kept until
    /// core restarts.
    pub fn step_core_log_level(&mut self, delta: isize) {
        if !self.can_set_log_level() {
            return;
        }
        let Some(cur) = self.manage.status.log_level.ready() else {
            return;
        };
        let reading = read_log_level(cur);
        let next = step_log_level(reading.level.as_deref(), delta);
        if reading.level.as_deref() == Some(next) {
            return;
        }
        let directive = log_directive(next, &reading.others);
        let was = cur.clone();
        // Show it at once; put it back if core refuses.
        self.manage.status.log_level = Load::Ready(directive.clone());
        let c = self.client.clone();
        self.spawn(
            async move { c.set_log_level(&directive).await },
            move |app, res| match res {
                Ok(()) => app.status = format!("Core log level: {next}, until homeCore restarts"),
                Err(e) => {
                    app.manage.status.log_level = Load::Ready(was);
                    app.error = Some(format!("log level: {e}"));
                }
            },
        );
    }
}

fn load_of<T>(r: anyhow::Result<T>) -> Load<T> {
    match r {
        Ok(v) => Load::Ready(v),
        Err(e) => Load::Failed(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::*;
    use crate::model::Role;

    #[tokio::test]
    async fn the_log_level_steps_and_keeps_the_broker_quiet() {
        let mut a = signed_in(Role::Admin);
        a.manage.status.log_level = Load::Ready("info,rumqttc::state=info,rumqttd=info".into());
        a.step_core_log_level(1);
        assert_eq!(
            a.manage.status.log_level.ready().map(String::as_str),
            Some("debug,rumqttc::state=info,rumqttd=info")
        );
        assert_eq!(a.pending, 1);
        // At the end it does nothing.
        a.manage.status.log_level = Load::Ready("trace".into());
        a.step_core_log_level(1);
        assert_eq!(a.pending, 1);
    }

    #[test]
    fn a_household_member_cannot_change_it() {
        let mut a = signed_in(Role::HouseholdMember);
        a.manage.status.log_level = Load::Ready("info".into());
        a.step_core_log_level(1);
        assert_eq!(
            a.manage.status.log_level.ready().map(String::as_str),
            Some("info")
        );
    }

    #[test]
    fn plugins_count_running_stopped_and_off() {
        let mut a = signed_in(Role::Admin);
        a.plugins = serde_json::from_value(serde_json::json!([
            {"plugin_id": "a", "status": "running", "enabled": true},
            {"plugin_id": "b", "status": "stopped", "enabled": true},
            {"plugin_id": "c", "status": "stopped", "enabled": false}
        ]))
        .unwrap();
        assert_eq!(
            a.plugin_health(),
            PluginHealth {
                running: 1,
                stopped: 1,
                off: 1
            }
        );
    }
}

//! Helpers (homeCore's own devices: switches, timers, counters...),
//! Alerts, and the one main action every device-like thing has, shared
//! by Home, Devices, Helpers and Modes.

use super::App;
use super::home::{is_helper, is_mode_device};
use crate::model::Alert;
use crate::screens::ScreenId;
use crate::text::matches_query;
use serde_json::{Value, json};

/// What Space does to a helper of `device_type` in `state`, as the
/// `PATCH /devices/{id}/state` body and a word for the status line.
/// `None` for types without a one-key action (text, datetime...).
pub fn helper_action(d: &crate::model::DeviceState) -> Option<(Value, &'static str)> {
    let on = d.attributes.get("on").and_then(Value::as_bool);
    Some(match d.device_type() {
        "switch" => {
            let on = on.unwrap_or(false);
            (json!({ "on": !on }), if on { "off" } else { "on" })
        }
        "timer" => match super::devices::timer_state(d) {
            "running" => (json!({ "action": "pause" }), "paused"),
            "paused" => (json!({ "action": "resume" }), "resumed"),
            _ => (json!({ "action": "start" }), "started"),
        },
        "counter" | "number" => (json!({ "action": "increment" }), "up one"),
        "button" => (json!({ "action": "press" }), "pressed"),
        "select" => (json!({ "action": "next" }), "next option"),
        _ => return None,
    })
}

/// The footer words for a helper's Space.
pub fn helper_action_hint(device_type: &str) -> &'static str {
    match device_type {
        "switch" => "Space toggle",
        "timer" => "Space start or pause",
        "counter" | "number" => "Space up one",
        "button" => "Space press",
        "select" => "Space next option",
        _ => "-",
    }
}

impl App {
    // ── Helpers list ─────────────────────────────────────────────────────

    pub(crate) fn compute_helper_view(&mut self) {
        let q = self.query(ScreenId::Helpers).to_string();
        let mut idx: Vec<usize> = (0..self.devices.len())
            .filter(|&i| {
                let d = &self.devices[i];
                is_helper(d) && matches_query(&q, &[d.display_name(), d.device_type()])
            })
            .collect();
        idx.sort_by_cached_key(|&i| self.devices[i].display_name().to_lowercase());
        self.views.helpers = idx;
    }

    pub(crate) fn helper_view_ids(&self) -> Vec<String> {
        self.views
            .helpers
            .iter()
            .filter_map(|&i| self.devices.get(i))
            .map(|d| d.device_id.clone())
            .collect()
    }

    pub fn visible_helpers(&self) -> impl Iterator<Item = &crate::model::DeviceState> {
        self.views
            .helpers
            .iter()
            .filter_map(|&i| self.devices.get(i))
    }

    pub fn selected_helper(&self) -> Option<&crate::model::DeviceState> {
        let id = self.helpers_sel.id()?;
        self.visible_helpers().find(|d| d.device_id == id)
    }

    pub fn move_helper(&mut self, delta: isize) {
        let ids = self.helper_view_ids();
        self.helpers_sel.move_by(delta, &ids);
    }

    pub fn helper_main_action_selected(&mut self) {
        if let Some(id) = self.selected_helper().map(|d| d.device_id.clone()) {
            self.device_main_action(&id);
        }
    }

    pub fn open_helper_creator(&mut self) {
        self.manage.glue_creator = Some(super::editors::GlueCreator::default());
    }

    // ── One main action ──────────────────────────────────────────────────

    /// Space on a device, wherever it is listed: a helper's own action,
    /// a mode switched, a scene run, a lock or light toggled, media
    /// played or stopped.
    pub fn device_main_action(&mut self, id: &str) {
        let Some(d) = self.device_by_id(id) else {
            return;
        };
        let name = d.display_name().to_string();
        let id = id.to_string();
        if is_helper(d) {
            match helper_action(d) {
                Some((body, word)) => self.command_device(id, body, format!("{name} {word}")),
                None => self.status = format!("{name} has no one-key action"),
            }
            return;
        }
        if is_mode_device(d) {
            self.toggle_mode(&id);
            return;
        }
        if d.is_scene() {
            self.activate_scene_id(&id);
            return;
        }
        let attr = |k: &str| d.attributes.get(k).and_then(Value::as_bool);
        if let Some(locked) = super::devices::device_lock_state(d) {
            let word = if locked { "unlocked" } else { "locked" };
            self.command_device(id, json!({ "locked": !locked }), format!("{name} {word}"));
            return;
        }
        if let Some(action) = super::media::toggle_action(d) {
            self.command_device(id, json!({ "action": action }), format!("{name} {action}"));
            return;
        }
        if let Some(on) = attr("on") {
            let word = if on { "off" } else { "on" };
            self.command_device(id, json!({ "on": !on }), format!("{name} {word}"));
            return;
        }
        self.status = format!("{name} has no on or off");
    }

    /// A manual mode turned on or off. Solar modes follow the sun.
    pub fn toggle_mode(&mut self, id: &str) {
        let Some(m) = self.modes.iter().find(|m| m.config.id == id) else {
            return;
        };
        let name = m.config.name.clone();
        if m.config.kind != "manual" {
            self.status = format!("{name} follows the sun; it switches itself");
            return;
        }
        let on = self.mode_on(id);
        let word = if on { "off" } else { "on" };
        self.command_device(
            id.to_string(),
            json!({ "on": !on }),
            format!("{name} {word}"),
        );
    }

    // ── Alerts ───────────────────────────────────────────────────────────

    /// Open alerts: the sidebar badge.
    pub fn open_alerts(&self) -> usize {
        self.alerts.iter().filter(|a| a.is_open()).count()
    }

    pub(crate) fn compute_alert_view(&mut self) {
        let q = self.query(ScreenId::Alerts).to_string();
        let mut idx: Vec<usize> = (0..self.alerts.len())
            .filter(|&i| matches_query(&q, &[&self.alerts[i].message]))
            .collect();
        // Open first, then newest.
        idx.sort_by(|&a, &b| {
            let (x, y) = (&self.alerts[a], &self.alerts[b]);
            y.is_open()
                .cmp(&x.is_open())
                .then(y.raised_at.cmp(&x.raised_at))
        });
        self.views.alerts = idx;
    }

    pub(crate) fn alert_view_ids(&self) -> Vec<String> {
        self.views
            .alerts
            .iter()
            .filter_map(|&i| self.alerts.get(i))
            .map(|a| a.id.clone())
            .collect()
    }

    pub fn visible_alerts(&self) -> impl Iterator<Item = &Alert> {
        self.views.alerts.iter().filter_map(|&i| self.alerts.get(i))
    }

    pub fn move_alert(&mut self, delta: isize) {
        let ids = self.alert_view_ids();
        self.alerts_sel.move_by(delta, &ids);
    }

    /// A live `alert_raised` or `alert_acknowledged`.
    pub(crate) fn upsert_alert(&mut self, alert: Alert) {
        match self.alerts.iter_mut().find(|a| a.id == alert.id) {
            Some(a) => *a = alert,
            None => self.alerts.push(alert),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::*;

    fn helper(ty: &str, attrs: Value) -> crate::model::DeviceState {
        let mut d = device("h", "H");
        d.plugin_id = "core.glue".into();
        d.device_type = Some(ty.into());
        d.attributes = attrs.as_object().cloned().unwrap();
        d
    }

    #[test]
    fn helper_actions_follow_type_and_state() {
        let a = |ty, attrs| helper_action(&helper(ty, attrs)).map(|(b, _)| b);
        assert_eq!(a("switch", json!({"on": true})), Some(json!({"on": false})));
        assert_eq!(
            a("timer", json!({"state": "running"})),
            Some(json!({"action": "pause"}))
        );
        assert_eq!(
            a("timer", json!({"state": "paused"})),
            Some(json!({"action": "resume"}))
        );
        assert_eq!(
            a("timer", json!({"state": "idle"})),
            Some(json!({"action": "start"}))
        );
        assert_eq!(
            a("counter", json!({})),
            Some(json!({"action": "increment"}))
        );
        assert_eq!(a("button", json!({})), Some(json!({"action": "press"})));
        assert_eq!(a("text", json!({})), None);
    }

    #[test]
    fn open_alerts_skip_acknowledged_ones() {
        let mut a = signed_in(crate::model::Role::Admin);
        a.alerts = vec![
            serde_json::from_value(json!({"id": "1", "message": "Door open"})).unwrap(),
            serde_json::from_value(
                json!({"id": "2", "message": "Low", "acknowledged": {"by": "x"}}),
            )
            .unwrap(),
        ];
        assert_eq!(a.open_alerts(), 1);
    }
}

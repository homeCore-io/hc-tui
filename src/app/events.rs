//! Live events: applying WebSocket frames to the cached lists, and
//! turning events into rows for the Events panel.
//!
//! Core serializes `Event` with `#[serde(tag = "type", rename_all =
//! "snake_case")]` (see `hc-types/src/event.rs`). A payload that lacks a
//! field never clears that field locally.

use super::App;
use crate::model::{Area, DeviceState, EventEntry, Rule, Scene};
use crate::text::truncate;
use serde_json::Value;

const MAX_EVENTS: usize = 200;

fn s<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// The name to show for an event: its `type`, or for `custom` events
/// the inner `event_type`.
pub fn event_kind(event: &Value) -> (String, bool) {
    let ty = s(event, "type").unwrap_or("unknown");
    if ty == "custom" {
        let inner = s(event, "event_type").unwrap_or("custom");
        return (inner.to_string(), true);
    }
    (ty.to_string(), false)
}

/// A short, generic description of an event's payload.
pub fn summarize(event: &Value) -> Option<String> {
    let ty = s(event, "type").unwrap_or("");
    let out = match ty {
        "device_state_changed" => event.get("changed").and_then(Value::as_array).map(|c| {
            let names: Vec<&str> = c.iter().filter_map(Value::as_str).collect();
            format!("changed={}", names.join(","))
        }),
        "device_availability_changed" => event
            .get("available")
            .and_then(Value::as_bool)
            .map(|a| format!("available={a}")),
        "device_name_changed" => s(event, "current_name").map(|n| format!("name={n}")),
        "device_battery_low" | "device_battery_recovered" => event
            .get("battery_pct")
            .and_then(Value::as_f64)
            .map(|b| format!("battery={b:.0}%")),
        "device_command_sent" => event.get("command").map(|c| format!("command={c}")),
        "rule_fired" => s(event, "trigger_type").map(|t| format!("trigger={t}")),
        "rule_evaluation_failed" => s(event, "reason").map(|r| format!("reason={r}")),
        "action_failed" => s(event, "error").map(|e| format!("error={e}")),
        "scene_activated" => s(event, "scene_name").map(|n| format!("scene={n}")),
        "mode_changed" => {
            let name = s(event, "mode_name").or(s(event, "mode_id")).unwrap_or("?");
            let on = event.get("on").and_then(Value::as_bool).unwrap_or(false);
            Some(format!("{name} {}", if on { "on" } else { "off" }))
        }
        "plugin_status_changed" => s(event, "status").map(|st| format!("status={st}")),
        "system_alert" => {
            let sev = s(event, "severity").unwrap_or("info");
            s(event, "message").map(|m| format!("{sev}: {m}"))
        }
        "alert_raised" | "alert_acknowledged" => event.get("alert").and_then(|a| {
            s(a, "message")
                .or(s(a, "title"))
                .or(s(a, "kind"))
                .map(str::to_string)
        }),
        "alert_cleared" => s(event, "alert_id").map(|id| format!("alert={id}")),
        "timer_state_changed" => s(event, "state").map(|st| format!("state={st}")),
        "room_lights_changed" => {
            let on = event.get("on").and_then(Value::as_bool).unwrap_or(false);
            s(event, "room_name").map(|r| format!("{r} {}", if on { "on" } else { "off" }))
        }
        "custom" => event
            .get("payload")
            .filter(|p| !p.is_null())
            .map(|p| format!("payload={p}")),
        _ => None,
    };
    out.map(|d| truncate(&d, 120))
}

/// Build a row from a bare event.
pub fn entry_from_event(event: &Value) -> EventEntry {
    let (event_type, custom) = event_kind(event);
    let rule_name = s(event, "rule_name").map(str::to_string);
    let device_id = s(event, "device_id")
        .or_else(|| event.get("device").and_then(|d| s(d, "device_id")))
        .map(str::to_string);
    EventEntry {
        event_type,
        timestamp: s(event, "timestamp").unwrap_or("").to_string(),
        plugin_id: s(event, "plugin_id").map(str::to_string),
        device_id,
        rule_name,
        custom,
        event_detail: summarize(event),
        seq: 0,
    }
}

/// Rows from `GET /events`: `[{seq, event_type, device_id?, event}]`,
/// newest first.
pub fn entries_from_log(items: &[Value]) -> Vec<EventEntry> {
    items
        .iter()
        .map(|item| {
            let event = item.get("event").unwrap_or(item);
            let mut entry = entry_from_event(event);
            if entry.device_id.is_none() {
                entry.device_id = s(item, "device_id").map(str::to_string);
            }
            if entry.event_type == "unknown"
                && let Some(t) = s(item, "event_type")
            {
                entry.event_type = t.to_string();
            }
            entry
        })
        .collect()
}

fn parse<T: for<'a> serde::Deserialize<'a>>(v: Option<&Value>) -> Option<T> {
    serde_json::from_value(v?.clone()).ok()
}

impl App {
    /// Apply one live event to the cached lists and record it.
    pub fn apply_event(&mut self, event: Value) {
        let ty = s(&event, "type").unwrap_or("").to_string();
        let ts = s(&event, "timestamp").unwrap_or("").to_string();
        let device_id = s(&event, "device_id").map(str::to_string);

        match ty.as_str() {
            "device_state_changed" => {
                if let (Some(id), Some(current)) = (
                    device_id.as_deref(),
                    event.get("current").and_then(Value::as_object),
                ) && let Some(d) = self.devices.iter_mut().find(|d| d.device_id == id)
                {
                    // Merge: a key missing from `current` is left as is.
                    for (k, v) in current {
                        d.attributes.insert(k.clone(), v.clone());
                    }
                    if !ts.is_empty() {
                        d.last_seen = ts.clone();
                    }
                    if d.is_scene() {
                        self.rebuild_scenes();
                    }
                }
            }
            "device_availability_changed" => {
                if let (Some(id), Some(avail)) = (
                    device_id.as_deref(),
                    event.get("available").and_then(Value::as_bool),
                ) && let Some(d) = self.devices.iter_mut().find(|d| d.device_id == id)
                {
                    d.available = avail;
                    if !ts.is_empty() {
                        d.last_seen = ts.clone();
                    }
                }
            }
            "device_name_changed" => {
                if let (Some(id), Some(name)) = (device_id.as_deref(), s(&event, "current_name"))
                    && let Some(d) = self.devices.iter_mut().find(|d| d.device_id == id)
                {
                    d.name = name.to_string();
                    if d.is_scene() {
                        self.rebuild_scenes();
                    }
                }
            }
            "device_added" | "device_metadata_changed" => {
                if let Some(new) = parse::<DeviceState>(event.get("device")) {
                    self.upsert_device(new);
                }
            }
            "device_removed" => {
                if let Some(id) = device_id.as_deref() {
                    self.devices.retain(|d| d.device_id != id);
                    self.rebuild_scenes();
                }
            }
            "room_changed" => {
                if let Some(area) = parse::<Area>(event.get("room")) {
                    super::rooms::upsert_area(&mut self.areas, area);
                }
            }
            "room_removed" => {
                if let Some(id) = s(&event, "room_id") {
                    self.areas.retain(|a| a.id != id);
                }
            }
            "scene_changed" => {
                if let Some(scene) = parse::<Scene>(event.get("scene")) {
                    match self.native_scenes.iter_mut().find(|x| x.id == scene.id) {
                        Some(x) => *x = scene,
                        None => self.native_scenes.push(scene),
                    }
                    self.rebuild_scenes();
                }
            }
            "scene_removed" => {
                if let Some(id) = s(&event, "scene_id") {
                    self.native_scenes.retain(|x| x.id != id);
                    self.rebuild_scenes();
                }
            }
            "rule_changed" => {
                if let Some(rule) = parse::<Rule>(event.get("rule")) {
                    match self.rules.iter_mut().find(|r| r.id == rule.id) {
                        Some(r) => *r = rule,
                        None => self.rules.push(rule),
                    }
                }
                self.on_rule_event("rule_changed", &event);
            }
            "rule_fired" => self.on_rule_event("rule_fired", &event),
            "rule_removed" => {
                if let Some(id) = s(&event, "rule_id") {
                    self.rules.retain(|r| r.id != id);
                }
            }
            "mode_changed" => {
                if let (Some(id), Some(on)) = (
                    s(&event, "mode_id"),
                    event.get("on").and_then(Value::as_bool),
                ) {
                    if let Some(m) = self.modes.iter_mut().find(|m| m.config.id == id)
                        && let Some(st) = m.state.as_mut()
                    {
                        st.attributes.insert("on".into(), Value::Bool(on));
                    }
                    if let Some(d) = self.devices.iter_mut().find(|d| d.device_id == id) {
                        d.attributes.insert("on".into(), Value::Bool(on));
                    }
                }
            }
            "plugin_status_changed" => {
                if let (Some(pid), Some(status)) = (s(&event, "plugin_id"), s(&event, "status"))
                    && let Some(p) = self.plugins.iter_mut().find(|p| p.plugin_id == pid)
                {
                    p.status = status.to_string();
                }
            }
            "alert_raised" | "alert_acknowledged" => {
                if let Some(alert) = parse::<crate::model::Alert>(event.get("alert")) {
                    self.upsert_alert(alert);
                }
            }
            "alert_cleared" => {
                if let Some(id) = s(&event, "alert_id") {
                    self.alerts.retain(|a| a.id != id);
                }
            }
            // Custom events and everything else are recorded only.
            _ => {}
        }

        let mut entry = entry_from_event(&event);
        self.event_seq += 1;
        entry.seq = self.event_seq;
        self.events.insert(0, entry);
        self.events.truncate(MAX_EVENTS);
    }

    /// Replace a device with a full payload, keeping known attributes if
    /// the payload carries none.
    fn upsert_device(&mut self, mut new: DeviceState) {
        match self
            .devices
            .iter_mut()
            .find(|d| d.device_id == new.device_id)
        {
            Some(d) => {
                if new.attributes.is_empty() {
                    new.attributes = std::mem::take(&mut d.attributes);
                }
                *d = new;
            }
            None => self.devices.push(new),
        }
        self.rebuild_scenes();
    }

    // ── Events panel ─────────────────────────────────────────────────────

    pub(crate) fn compute_event_view(&mut self) {
        let filter = self.manage.events.type_filter.clone();
        let q = self.query(crate::screens::ScreenId::Events).to_string();
        self.views.events = (0..self.events.len())
            .filter(|&i| {
                let e = &self.events[i];
                filter.as_deref().is_none_or(|f| e.event_type == f) && {
                    let device = e
                        .device_id
                        .as_deref()
                        .and_then(|d| self.device_by_id(d))
                        .map(|d| d.display_name())
                        .unwrap_or("");
                    crate::text::matches_query(
                        &q,
                        &[
                            &e.event_type,
                            e.device_id.as_deref().unwrap_or(""),
                            device,
                            e.rule_name.as_deref().unwrap_or(""),
                            e.plugin_id.as_deref().unwrap_or(""),
                            e.event_detail.as_deref().unwrap_or(""),
                        ],
                    )
                }
            })
            .collect();
    }

    pub fn visible_events(&self) -> impl Iterator<Item = &EventEntry> {
        self.views.events.iter().filter_map(|&i| self.events.get(i))
    }

    pub fn plugin_events(&self, plugin_id: &str) -> impl Iterator<Item = &EventEntry> {
        self.events
            .iter()
            .filter(move |e| e.plugin_id.as_deref() == Some(plugin_id))
    }

    /// Distinct event types present, sorted; what `f` cycles through.
    pub fn event_types(&self) -> Vec<String> {
        let mut types: Vec<String> = self.events.iter().map(|e| e.event_type.clone()).collect();
        types.sort();
        types.dedup();
        types
    }

    /// `f` on the Events panel: All, then each type present, then All.
    pub(crate) fn cycle_event_filter(&mut self) {
        let types = self.event_types();
        let next = match &self.manage.events.type_filter {
            None => types.first().cloned(),
            Some(cur) => {
                let i = types.iter().position(|t| t == cur);
                i.and_then(|i| types.get(i + 1).cloned())
            }
        };
        self.manage.events.type_filter = next;
        self.manage.events.sel.reset();
        self.status = format!(
            "Events filter: {}",
            self.manage.events.type_filter.as_deref().unwrap_or("all")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::model::Role;
    use serde_json::json;

    fn app_with_lamp() -> App {
        let mut a = signed_in(Role::Admin);
        let mut d = device("lamp", "Lamp");
        d.attributes = json!({"on": false, "brightness_pct": 40.0})
            .as_object()
            .cloned()
            .unwrap();
        a.devices.push(d);
        a
    }

    #[test]
    fn state_changes_merge_and_never_wipe() {
        let mut a = app_with_lamp();
        a.apply_event(json!({
            "type": "device_state_changed", "device_id": "lamp",
            "timestamp": "2026-11-17T00:00:00Z",
            "previous": {}, "current": {"on": true}, "changed": ["on"]
        }));
        let d = &a.devices[0];
        assert_eq!(d.attributes["on"], true);
        assert_eq!(d.attributes["brightness_pct"], 40.0, "untouched key kept");
        assert_eq!(d.last_seen, "2026-11-17T00:00:00Z");

        // No `current` at all: nothing changes.
        a.apply_event(json!({"type": "device_state_changed", "device_id": "lamp"}));
        assert_eq!(a.devices[0].attributes.len(), 2);
    }

    #[test]
    fn availability_needs_the_field() {
        let mut a = app_with_lamp();
        a.apply_event(json!({"type": "device_availability_changed", "device_id": "lamp"}));
        assert!(
            a.devices[0].available,
            "missing field must not mean offline"
        );
        a.apply_event(
            json!({"type": "device_availability_changed", "device_id": "lamp", "available": false}),
        );
        assert!(!a.devices[0].available);
    }

    #[test]
    fn added_metadata_and_removed() {
        let mut a = app_with_lamp();
        a.apply_event(json!({"type": "device_added", "device": {
            "device_id": "fan", "name": "Fan", "plugin_id": "p", "available": true,
            "attributes": {"on": true}, "last_seen": ""
        }}));
        assert_eq!(a.devices.len(), 2);
        a.apply_event(json!({"type": "device_metadata_changed", "device": {
            "device_id": "lamp", "name": "Lamp", "name_override": "Reading lamp",
            "room": "den", "plugin_id": "p", "available": true, "attributes": {},
            "last_seen": ""
        }}));
        let lamp = a.device_by_id("lamp").unwrap();
        assert_eq!(lamp.display_name(), "Reading lamp");
        assert_eq!(lamp.room_name(), Some("den"));
        assert_eq!(
            lamp.attributes["brightness_pct"], 40.0,
            "empty attributes kept"
        );
        a.apply_event(json!({"type": "device_removed", "device_id": "fan"}));
        assert!(a.device_by_id("fan").is_none());
    }

    #[test]
    fn rooms_scenes_rules_and_modes() {
        let mut a = signed_in(Role::Admin);
        a.apply_event(json!({"type": "room_changed", "room": {
            "id": "r1", "name": "Den", "slug": "den", "device_ids": []
        }}));
        assert_eq!(a.areas.len(), 1);
        a.apply_event(json!({"type": "room_removed", "room_id": "r1"}));
        assert!(a.areas.is_empty());

        a.apply_event(json!({"type": "scene_changed", "scene": {
            "id": "s1", "name": "Evening", "states": {}
        }}));
        assert_eq!(a.scenes.len(), 1);
        a.apply_event(json!({"type": "scene_removed", "scene_id": "s1"}));
        assert!(a.scenes.is_empty());

        a.apply_event(json!({"type": "rule_changed", "rule": {
            "id": "x", "name": "Night", "enabled": true, "priority": 1,
            "trigger": "ManualTrigger"
        }}));
        assert_eq!(a.rules[0].trigger_label(), "ManualTrigger");
        a.apply_event(json!({"type": "rule_removed", "rule_id": "x"}));
        assert!(a.rules.is_empty());

        a.modes.push(
            serde_json::from_value(json!({
                "config": {"id": "mode_night", "name": "Night", "kind": "manual"},
                "state": {"device_id": "mode_night", "name": "Night", "plugin_id": "core.mode",
                          "available": true, "attributes": {"on": false}, "last_seen": ""}
            }))
            .unwrap(),
        );
        a.apply_event(json!({"type": "mode_changed", "mode_id": "mode_night", "mode_name": "Night", "on": true}));
        assert_eq!(a.modes[0].state.as_ref().unwrap().attributes["on"], true);
    }

    #[test]
    fn scene_device_state_reaches_the_scenes_tab() {
        let mut a = signed_in(Role::Admin);
        let mut sc = device("sc", "Evening");
        sc.device_type = Some("scene".into());
        a.devices.push(sc);
        a.rebuild_scenes();
        assert_eq!(a.scenes[0].active, None);
        a.apply_event(json!({
            "type": "device_state_changed", "device_id": "sc",
            "current": {"active": true}, "changed": ["active"]
        }));
        assert_eq!(a.scenes[0].active, Some(true));
    }

    #[test]
    fn event_rows_keep_their_ids_as_new_ones_arrive() {
        let mut a = signed_in(Role::Admin);
        for i in 0..250 {
            a.apply_event(json!({"type": "rule_fired", "rule_name": format!("r{i}")}));
        }
        a.refresh_views();
        let before = a.event_row_ids()[5].clone();
        a.apply_event(json!({"type": "rule_fired", "rule_name": "new"}));
        a.refresh_views();
        assert_eq!(a.event_row_ids()[6], before);
    }

    #[test]
    fn custom_events_use_their_event_type() {
        let mut a = signed_in(Role::Admin);
        a.apply_event(json!({
            "type": "custom", "timestamp": "t", "event_type": "doorbell",
            "payload": {"x": 1}
        }));
        assert_eq!(a.events[0].event_type, "doorbell");
        assert!(a.events[0].custom);
        a.apply_event(json!({"type": "alert_raised", "alert": {"message": "Leak"}}));
        assert_eq!(a.events[0].event_detail.as_deref(), Some("Leak"));
    }

    #[test]
    fn log_entries_unwrap_the_event() {
        let items = vec![
            json!({"seq": 2, "event_type": "custom", "event": {
                "type": "custom", "timestamp": "t2", "event_type": "webhook", "payload": null
            }}),
            json!({"seq": 1, "event_type": "device_state_changed", "device_id": "d", "event": {
                "type": "device_state_changed", "timestamp": "t1", "device_id": "d",
                "current": {}, "changed": ["on"]
            }}),
        ];
        let rows = entries_from_log(&items);
        assert_eq!(rows[0].event_type, "webhook");
        assert_eq!(rows[1].device_id.as_deref(), Some("d"));
        assert_eq!(rows[1].event_detail.as_deref(), Some("changed=on"));
    }

    #[test]
    fn long_details_are_cut_safely() {
        let msg = "ü".repeat(500);
        let d = summarize(&json!({"type": "action_failed", "error": msg})).unwrap();
        assert!(d.chars().count() <= 120);
    }

    #[test]
    fn event_type_filter_cycles() {
        let mut a = signed_in(Role::Admin);
        a.apply_event(json!({"type": "rule_fired", "rule_name": "x"}));
        a.apply_event(json!({"type": "device_removed", "device_id": "nope"}));
        a.cycle_event_filter();
        a.refresh_views();
        assert_eq!(
            a.manage.events.type_filter.as_deref(),
            Some("device_removed")
        );
        assert_eq!(a.visible_events().count(), 1);
        a.cycle_event_filter();
        a.cycle_event_filter();
        assert!(a.manage.events.type_filter.is_none());
    }
}

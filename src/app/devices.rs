//! Devices tab: list views, filters, device commands and the editor.

use super::editors::{Confirm, ConfirmAction, DeviceEditor};
use super::{App, ListSel, Section};
use crate::model::DeviceState;
use crate::text::normalize_label;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DeviceViewMode {
    #[default]
    Grouped,
    Flat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DeviceSubPanel {
    #[default]
    All,
    MediaPlayers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DeviceFilterMode {
    #[default]
    All,
    Online,
    Offline,
    LowBattery,
}

impl DeviceFilterMode {
    pub fn title(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Online => "online",
            Self::Offline => "offline",
            Self::LowBattery => "low_battery",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Online,
            Self::Online => Self::Offline,
            Self::Offline => Self::LowBattery,
            Self::LowBattery => Self::All,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DeviceSortMode {
    #[default]
    Name,
    Status,
    LastSeen,
}

impl DeviceSortMode {
    pub fn title(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Status => "status",
            Self::LastSeen => "last_seen",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Name => Self::Status,
            Self::Status => Self::LastSeen,
            Self::LastSeen => Self::Name,
        }
    }
}

#[derive(Debug, Default)]
pub struct DevicesTab {
    pub sub: DeviceSubPanel,
    pub view_mode: DeviceViewMode,
    pub filter: DeviceFilterMode,
    pub sort: DeviceSortMode,
    pub search_query: String,
    pub search_open: bool,
    pub sel: ListSel,
    pub media_sel: ListSel,
    pub editor: Option<DeviceEditor>,
}

// ── Attribute readers ───────────────────────────────────────────────────────

pub fn device_battery(device: &DeviceState) -> Option<u8> {
    ["battery_pct", "battery", "battery_level", "battery_percent"]
        .iter()
        .find_map(|k| device.attributes.get(*k).and_then(Value::as_f64))
        .map(|n| n.clamp(0.0, 100.0) as u8)
}

/// Temperature in °C, as carried on the bus.
pub fn device_temperature(device: &DeviceState) -> Option<f64> {
    device.attributes.get("temperature").and_then(Value::as_f64)
}

pub fn device_humidity(device: &DeviceState) -> Option<f64> {
    device.attributes.get("humidity").and_then(Value::as_f64)
}

/// `brightness_pct`, 0 to 100.
pub fn device_brightness(device: &DeviceState) -> Option<u8> {
    device
        .attributes
        .get("brightness_pct")
        .and_then(Value::as_f64)
        .map(|n| n.clamp(0.0, 100.0).round() as u8)
}

pub fn device_lock_state(device: &DeviceState) -> Option<bool> {
    device.attributes.get("locked").and_then(Value::as_bool)
}

pub fn is_timer(device: &DeviceState) -> bool {
    device.device_type() == "timer"
}

pub fn timer_state(device: &DeviceState) -> &str {
    device
        .attributes
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("idle")
}

/// A running timer's remaining seconds, computed from `started_at` and
/// `duration_secs` so the countdown moves on every tick without a
/// request. Other states use the stored `remaining_secs`.
pub fn timer_remaining_secs(device: &DeviceState) -> u64 {
    use chrono::{DateTime, Utc};
    if timer_state(device) == "running" {
        let duration = device
            .attributes
            .get("duration_secs")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let started = device
            .attributes
            .get("started_at")
            .and_then(Value::as_str)
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));
        if let Some(started) = started {
            let elapsed = (Utc::now() - started).num_seconds().max(0) as u64;
            return duration.saturating_sub(elapsed);
        }
    }
    device
        .attributes
        .get("remaining_secs")
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

/// The Devices list leaves out scene devices; they live on Scenes.
pub fn shown_in_devices(device: &DeviceState) -> bool {
    !device.is_scene()
}

impl App {
    // ── Views ────────────────────────────────────────────────────────────

    /// The room's display name: the area whose slug, id or name matches,
    /// else the raw value.
    pub fn room_label(&self, device: &DeviceState) -> Option<String> {
        let room = device.room_name()?;
        Some(
            self.areas
                .iter()
                .find(|a| a.slug == room || a.id == room || a.name.eq_ignore_ascii_case(room))
                .map(|a| a.name.clone())
                .unwrap_or_else(|| room.to_string()),
        )
    }

    fn device_matches_filter(&self, device: &DeviceState) -> bool {
        match self.devices_tab.filter {
            DeviceFilterMode::All => true,
            DeviceFilterMode::Online => device.available,
            DeviceFilterMode::Offline => !device.available,
            DeviceFilterMode::LowBattery => device_battery(device).is_some_and(|b| b <= 20),
        }
    }

    fn device_matches_search(&self, device: &DeviceState) -> bool {
        let q = self.devices_tab.search_query.trim().to_lowercase();
        if q.is_empty() {
            return true;
        }
        let has = |s: &str| s.to_lowercase().contains(&q);
        has(device.display_name())
            || has(&device.name)
            || has(&device.device_id)
            || device.canonical_name.as_deref().is_some_and(has)
            || has(&device.plugin_id)
            || device.room_name().is_some_and(has)
            || self.room_label(device).is_some_and(|r| has(&r))
            || has(device.device_type())
    }

    pub(super) fn compute_device_views(&mut self) {
        let mut idx: Vec<usize> = (0..self.devices.len())
            .filter(|&i| {
                let d = &self.devices[i];
                shown_in_devices(d)
                    && self.device_matches_filter(d)
                    && self.device_matches_search(d)
            })
            .collect();

        match self.devices_tab.sort {
            DeviceSortMode::Name => {
                idx.sort_by_cached_key(|&i| self.devices[i].display_name().to_lowercase())
            }
            DeviceSortMode::Status => idx.sort_by_cached_key(|&i| {
                let d = &self.devices[i];
                (
                    self.device_status(d).to_lowercase(),
                    d.display_name().to_lowercase(),
                )
            }),
            DeviceSortMode::LastSeen => {
                idx.sort_by(|&a, &b| self.devices[b].last_seen.cmp(&self.devices[a].last_seen))
            }
        }

        self.views.media = idx
            .iter()
            .copied()
            .filter(|&i| super::media::is_media_player(&self.devices[i]))
            .collect();

        if self.devices_tab.view_mode == DeviceViewMode::Grouped {
            let mut groups: std::collections::BTreeMap<String, Vec<usize>> = Default::default();
            let mut unassigned = Vec::new();
            for &i in &idx {
                match self.room_label(&self.devices[i]) {
                    Some(label) => groups.entry(label).or_default().push(i),
                    None => unassigned.push(i),
                }
            }
            let mut order = Vec::with_capacity(idx.len());
            let mut headers = Vec::new();
            for (label, members) in groups {
                headers.push((label, members.len()));
                order.extend(members);
            }
            if !unassigned.is_empty() {
                headers.push(("Unassigned".to_string(), unassigned.len()));
                order.extend(unassigned);
            }
            self.views.devices = order;
            self.views.device_groups = headers;
        } else {
            self.views.devices = idx;
            self.views.device_groups.clear();
        }
    }

    pub(super) fn device_view_ids(&self) -> Vec<String> {
        self.views
            .devices
            .iter()
            .filter_map(|&i| self.devices.get(i))
            .map(|d| d.device_id.clone())
            .collect()
    }

    pub(super) fn media_view_ids(&self) -> Vec<String> {
        self.views
            .media
            .iter()
            .filter_map(|&i| self.devices.get(i))
            .map(|d| d.device_id.clone())
            .collect()
    }

    /// Visible devices in display order.
    pub fn visible_devices(&self) -> impl Iterator<Item = &DeviceState> {
        self.views
            .devices
            .iter()
            .filter_map(|&i| self.devices.get(i))
    }

    pub fn device_by_id(&self, id: &str) -> Option<&DeviceState> {
        self.devices.iter().find(|d| d.device_id == id)
    }

    /// The device under the cursor on the current sub-panel.
    pub fn selected_device(&self) -> Option<&DeviceState> {
        let (sel, view) = match self.devices_tab.sub {
            DeviceSubPanel::All => (&self.devices_tab.sel, &self.views.devices),
            DeviceSubPanel::MediaPlayers => (&self.devices_tab.media_sel, &self.views.media),
        };
        if let Some(id) = sel.id()
            && let Some(d) = self.device_by_id(id)
            && view
                .iter()
                .any(|&i| self.devices.get(i).is_some_and(|x| x.device_id == id))
        {
            return Some(d);
        }
        view.get(sel.index()).and_then(|&i| self.devices.get(i))
    }

    pub fn any_timer_running(&self) -> bool {
        self.devices
            .iter()
            .any(|d| is_timer(d) && timer_state(d) == "running")
    }

    /// Short status word for lists and the detail pane.
    pub fn device_status(&self, device: &DeviceState) -> String {
        let attrs = &device.attributes;
        let b = |k: &str| attrs.get(k).and_then(Value::as_bool);
        if let Some(locked) = b("locked") {
            return if locked { "Locked" } else { "Unlocked" }.into();
        }
        if let Some(on) = b("on") {
            return if on { "On" } else { "Off" }.into();
        }
        if let Some(state) = attrs.get("state").and_then(Value::as_str) {
            return normalize_label(state);
        }
        if let Some(pct) = attrs.get("brightness_pct").and_then(Value::as_f64) {
            return if pct > 0.0 { "On" } else { "Off" }.into();
        }
        if let Some(open) = b("open").or_else(|| b("contact_open")) {
            return if open { "Open" } else { "Closed" }.into();
        }
        if let Some(motion) = b("motion") {
            return if motion { "Motion" } else { "Clear" }.into();
        }
        if let Some(mode) = attrs
            .get("hvac_mode")
            .or_else(|| attrs.get("mode"))
            .and_then(Value::as_str)
        {
            return normalize_label(mode);
        }
        if let Some(pos) = attrs.get("position").and_then(Value::as_f64) {
            return if pos >= 99.0 {
                "Open".into()
            } else if pos <= 1.0 {
                "Closed".into()
            } else {
                format!("{pos:.0}%")
            };
        }
        if let Some(t) = device_temperature(device) {
            return self.temp_unit.format(t);
        }
        if let Some(h) = device_humidity(device) {
            return format!("{h:.0}%rh");
        }
        if let Some(battery) = device_battery(device) {
            return format!("{battery}%");
        }
        if let Some(state) = attrs.get("battery_state").and_then(Value::as_str) {
            return normalize_label(state);
        }
        if let Some(lux) = attrs.get("illuminance").and_then(Value::as_f64) {
            return format!("{lux:.0} lx");
        }
        for key in ["smoke", "co", "water_detected"] {
            if b(key) == Some(true) {
                return normalize_label(key);
            }
        }
        if let Some(online) = b("online") {
            return if online { "Online" } else { "Offline" }.into();
        }
        if let Some(occupied) = b("occupied") {
            return if occupied { "Occupied" } else { "Vacant" }.into();
        }
        "-".into()
    }

    // ── Keys ─────────────────────────────────────────────────────────────

    pub(super) fn on_key_devices(&mut self, key: KeyEvent) {
        let media = self.devices_tab.sub == DeviceSubPanel::MediaPlayers;
        match key.code {
            KeyCode::Left | KeyCode::Right => {
                self.devices_tab.sub = if media {
                    DeviceSubPanel::All
                } else {
                    DeviceSubPanel::MediaPlayers
                };
            }
            KeyCode::Down | KeyCode::Char('j') => self.move_device_cursor(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_device_cursor(-1),
            KeyCode::PageDown => self.move_device_cursor(10),
            KeyCode::PageUp => self.move_device_cursor(-10),
            KeyCode::Enter => self.open_device_editor(),
            KeyCode::Char('d') if !media => self.ask_delete_device(),
            KeyCode::Char(' ') => self.toggle_lock_or_switch(),
            KeyCode::Char('t') => self.toggle_selected_device(),
            KeyCode::Char('v') => {
                self.devices_tab.view_mode = match self.devices_tab.view_mode {
                    DeviceViewMode::Grouped => DeviceViewMode::Flat,
                    DeviceViewMode::Flat => DeviceViewMode::Grouped,
                };
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                if media {
                    self.media_adjust_volume(5);
                } else {
                    self.adjust_brightness(1);
                }
            }
            KeyCode::Char('-') => {
                if media {
                    self.media_adjust_volume(-5);
                } else {
                    self.adjust_brightness(-1);
                }
            }
            KeyCode::Char('l') => self.lock_device(true),
            KeyCode::Char('u') => self.lock_device(false),
            KeyCode::Char('p') if media => self.media_play_pause(),
            KeyCode::Char('n') if media => self.media_simple("next", "Next"),
            KeyCode::Char('b') if media => self.media_simple("previous", "Previous"),
            KeyCode::Char('x') if media => self.media_stop(),
            KeyCode::Char('m') if media => self.media_toggle_mute(),
            KeyCode::Char('f') if !media => {
                self.devices_tab.filter = self.devices_tab.filter.next();
                self.devices_tab.sel.reset();
                self.status = format!("Device filter: {}", self.devices_tab.filter.title());
            }
            KeyCode::Char('s') if !media => {
                self.devices_tab.sort = self.devices_tab.sort.next();
                self.status = format!("Device sort: {}", self.devices_tab.sort.title());
            }
            KeyCode::Char('/') if !media => {
                self.devices_tab.search_open = true;
                self.status = format!("Device search: {}", self.devices_tab.search_query);
            }
            _ => {}
        }
    }

    fn move_device_cursor(&mut self, delta: isize) {
        match self.devices_tab.sub {
            DeviceSubPanel::All => {
                let ids = self.device_view_ids();
                self.devices_tab.sel.move_by(delta, &ids);
            }
            DeviceSubPanel::MediaPlayers => {
                let ids = self.media_view_ids();
                self.devices_tab.media_sel.move_by(delta, &ids);
            }
        }
    }

    pub(super) fn on_key_device_search(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.devices_tab.search_open = false;
                self.status = "Device search canceled".to_string();
            }
            KeyCode::Enter => {
                self.devices_tab.search_open = false;
                self.devices_tab.sel.reset();
                let q = self.devices_tab.search_query.trim();
                self.status = if q.is_empty() {
                    "Device search cleared".to_string()
                } else {
                    format!("Device search: {q}")
                };
            }
            KeyCode::Backspace => {
                self.devices_tab.search_query.pop();
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.devices_tab.search_query.push(ch);
            }
            _ => {}
        }
    }

    // ── Commands ─────────────────────────────────────────────────────────

    /// Send `body` to the device's state endpoint, reporting `done` on
    /// success.
    pub(super) fn command_device(&mut self, device_id: String, body: Value, done: String) {
        let c = self.client.clone();
        self.spawn_ok(
            "command",
            async move { c.patch_device_state(&device_id, body).await },
            move |app, ()| app.status = done,
        );
    }

    fn selected_device_parts(&self) -> Option<(String, String)> {
        self.selected_device()
            .map(|d| (d.device_id.clone(), d.display_name().to_string()))
    }

    fn toggle_selected_device(&mut self) {
        let Some(device) = self.selected_device() else {
            return;
        };
        if super::media::is_media_player(device) {
            self.media_play_stop();
            return;
        }
        let on = device
            .attributes
            .get("on")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let (id, name) = (device.device_id.clone(), device.display_name().to_string());
        let label = if on { "Off" } else { "On" };
        self.command_device(id, json!({ "on": !on }), format!("{name} → {label}"));
    }

    /// Space: play/stop for media, lock toggle for locks, else on/off.
    fn toggle_lock_or_switch(&mut self) {
        let Some(device) = self.selected_device() else {
            return;
        };
        if let Some(locked) = device_lock_state(device) {
            let (id, name) = (device.device_id.clone(), device.display_name().to_string());
            let label = if locked { "Unlocked" } else { "Locked" };
            self.command_device(
                id,
                json!({ "locked": !locked }),
                format!("{name} → {label}"),
            );
        } else {
            self.toggle_selected_device();
        }
    }

    fn adjust_brightness(&mut self, direction: i64) {
        let Some(device) = self.selected_device() else {
            return;
        };
        let Some(raw) = device
            .attributes
            .get("brightness_pct")
            .and_then(Value::as_f64)
        else {
            self.error = Some("this device has no brightness_pct".to_string());
            return;
        };
        let new_val = ((raw + direction as f64 * 10.0).clamp(0.0, 100.0) * 10.0).round() / 10.0;
        let (id, name) = (device.device_id.clone(), device.display_name().to_string());
        self.command_device(
            id,
            json!({ "brightness_pct": new_val }),
            format!("{name} brightness → {new_val:.0}%"),
        );
    }

    fn lock_device(&mut self, locked: bool) {
        let Some((id, name)) = self.selected_device_parts() else {
            return;
        };
        let label = if locked { "Locked" } else { "Unlocked" };
        self.command_device(id, json!({ "locked": locked }), format!("{name} → {label}"));
    }

    // ── Editor ───────────────────────────────────────────────────────────

    fn open_device_editor(&mut self) {
        let Some(d) = self.selected_device() else {
            return;
        };
        self.devices_tab.editor = Some(DeviceEditor::new(
            d.device_id.clone(),
            d.display_name().to_string(),
            self.room_label(d).unwrap_or_default(),
            d.canonical_name.clone().unwrap_or_default(),
        ));
    }

    pub(super) fn save_device_editor(&mut self) {
        let Some(ed) = self.devices_tab.editor.clone() else {
            return;
        };
        let changes = ed.changes();
        if changes.is_empty() {
            self.devices_tab.editor = None;
            self.status = "No changes".to_string();
            return;
        }
        let c = self.client.clone();
        let id = ed.device_id.clone();
        self.status = format!("Saving {id}...");
        self.spawn_ok(
            "update device",
            async move { c.update_device(&id, changes).await },
            move |app, ()| {
                app.devices_tab.editor = None;
                app.status = format!("Updated {}", ed.device_id);
                // Core answers with no body; the device_metadata_changed
                // event brings the new values, but show them now.
                if let Some(d) = app.devices.iter_mut().find(|d| d.device_id == ed.device_id) {
                    let name = ed.name.trim();
                    if name != ed.initial_name.trim() {
                        d.name_override = (!name.is_empty()).then(|| name.to_string());
                    }
                    let cn = ed.canonical_name.trim();
                    if cn != ed.initial_canonical_name.trim() {
                        d.canonical_name = (!cn.is_empty()).then(|| cn.to_string());
                    }
                }
                app.save_cache(&[Section::Devices]);
            },
        );
    }

    // ── Delete ───────────────────────────────────────────────────────────

    fn ask_delete_device(&mut self) {
        let Some((id, name)) = self.selected_device_parts() else {
            return;
        };
        self.ask(
            Confirm::new(
                "Delete device",
                format!("Delete device \"{name}\"?"),
                ConfirmAction::DeleteDevice { id: id.clone() },
            )
            .with_detail(format!("ID: {id}")),
        );
    }

    pub(super) fn delete_device(&mut self, id: String) {
        let c = self.client.clone();
        let target = id.clone();
        self.spawn_ok(
            "delete device",
            async move { c.delete_device(&target).await },
            move |app, ()| {
                app.devices.retain(|d| d.device_id != id);
                app.rebuild_scenes();
                app.status = format!("Deleted device '{id}'");
                app.save_cache(&[Section::Devices]);
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::model::Role;

    fn with_attrs(id: &str, name: &str, attrs: Value) -> DeviceState {
        let mut d = device(id, name);
        d.attributes = attrs.as_object().cloned().unwrap();
        d
    }

    #[test]
    fn selection_survives_a_resort() {
        let mut a = signed_in(Role::Admin);
        a.devices_tab.view_mode = DeviceViewMode::Flat;
        a.devices = vec![
            device("a", "Alpha"),
            device("b", "Bravo"),
            device("c", "Charlie"),
        ];
        a.refresh_views();
        a.on_key(key(KeyCode::Down));
        a.on_key(key(KeyCode::Down));
        assert_eq!(a.selected_device().unwrap().device_id, "c");

        // A live rename moves Charlie to the top of the name sort.
        a.devices[2].name_override = Some("Aardvark".into());
        a.refresh_views();
        assert_eq!(a.devices_tab.sel.index(), 0);
        assert_eq!(a.selected_device().unwrap().device_id, "c");
    }

    #[test]
    fn grouped_view_orders_by_room() {
        let mut a = signed_in(Role::Admin);
        let mut k = device("k", "Kettle");
        k.room = Some("kitchen".into());
        let mut b = device("b", "Bedside");
        b.room = Some("bedroom".into());
        let n = device("n", "Nowhere");
        a.devices = vec![k, n, b];
        a.refresh_views();
        let labels: Vec<&str> = a
            .views
            .device_groups
            .iter()
            .map(|(l, _)| l.as_str())
            .collect();
        assert_eq!(labels, vec!["bedroom", "kitchen", "Unassigned"]);
        assert_eq!(a.device_view_ids(), vec!["b", "k", "n"]);
    }

    #[test]
    fn scene_devices_are_not_listed() {
        let mut a = signed_in(Role::Admin);
        let mut s = device("s", "Evening");
        s.device_type = Some("scene".into());
        a.devices = vec![s, device("l", "Lamp")];
        a.refresh_views();
        assert_eq!(a.device_view_ids(), vec!["l"]);
        a.rebuild_scenes();
        assert_eq!(a.scenes.len(), 1);
    }

    #[test]
    fn temperatures_follow_the_unit() {
        let mut a = signed_in(Role::Admin);
        let d = with_attrs("t", "Temp", json!({"temperature": 20.0}));
        assert_eq!(a.device_status(&d), "20.0°C");
        a.temp_unit = crate::model::TempUnit::Fahrenheit;
        assert_eq!(a.device_status(&d), "68.0°F");
    }

    #[test]
    fn brightness_reads_only_brightness_pct() {
        let d = with_attrs("l", "L", json!({"brightness": 200}));
        assert_eq!(device_brightness(&d), None);
        let d = with_attrs("l", "L", json!({"brightness_pct": 42.4}));
        assert_eq!(device_brightness(&d), Some(42));
    }

    #[test]
    fn delete_asks_first() {
        let mut a = signed_in(Role::Admin);
        a.devices = vec![device("a", "Alpha")];
        a.refresh_views();
        a.on_key(key(KeyCode::Char('d')));
        assert!(matches!(
            a.confirm.as_ref().map(|c| &c.action),
            Some(ConfirmAction::DeleteDevice { id }) if id == "a"
        ));
        a.on_key(key(KeyCode::Char('n')));
        assert!(a.confirm.is_none());
        assert_eq!(a.devices.len(), 1);
    }
}

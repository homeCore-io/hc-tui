//! The device picker (type to search, Space to mark, Enter to add) and
//! room membership: adding devices to a room and taking them out.
//!
//! Membership goes through `PUT /areas/{id}/devices`, which replaces a
//! room's list; core then sets or clears each device's room override,
//! and clears the plugin's own room when it pointed at this one, so a
//! device taken out really leaves.

use super::controls::DeviceConfirm;
use super::editors::{Confirm, ConfirmAction, edit_text};
use super::{App, ListSel, Section};
use crate::model::DeviceState;
use crate::text::matches_query;
use crossterm::event::{KeyCode, KeyEvent};

/// What the picked devices are for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickFor {
    Room { area_id: String },
    Scene,
}

#[derive(Debug, Clone)]
pub struct DevicePicker {
    pub purpose: PickFor,
    pub title: String,
    pub query: String,
    pub sel: ListSel,
    /// Marked with Space, in the order marked.
    pub marked: Vec<String>,
    /// Already there; not offered.
    pub exclude: Vec<String>,
}

impl DevicePicker {
    pub fn new(purpose: PickFor, title: impl Into<String>, exclude: Vec<String>) -> Self {
        Self {
            purpose,
            title: title.into(),
            query: String::new(),
            sel: ListSel::default(),
            marked: Vec::new(),
            exclude,
        }
    }
}

impl App {
    /// The picker's rows: every device but scenes and those already
    /// there, narrowed by the search, by name.
    pub fn picker_devices(&self) -> Vec<&DeviceState> {
        let Some(p) = self.devices_tab.controls.picker.as_ref() else {
            return Vec::new();
        };
        let mut v: Vec<&DeviceState> = self
            .devices
            .iter()
            .filter(|d| !d.is_scene() && !p.exclude.contains(&d.device_id))
            .filter(|d| {
                let room = self.room_label(d).unwrap_or_default();
                matches_query(
                    &p.query,
                    &[d.display_name(), &d.device_id, d.device_type(), &room],
                )
            })
            .collect();
        v.sort_by_cached_key(|d| d.display_name().to_lowercase());
        v
    }

    fn picker_ids(&self) -> Vec<String> {
        self.picker_devices()
            .iter()
            .map(|d| d.device_id.clone())
            .collect()
    }

    pub(crate) fn on_key_device_picker(&mut self, key: KeyEvent) {
        let ids = self.picker_ids();
        let Some(p) = self.devices_tab.controls.picker.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                self.devices_tab.controls.picker = None;
                self.status = "Cancelled".into();
            }
            KeyCode::Down => p.sel.move_by(1, &ids),
            KeyCode::Up => p.sel.move_by(-1, &ids),
            KeyCode::PageDown => p.sel.move_by(10, &ids),
            KeyCode::PageUp => p.sel.move_by(-10, &ids),
            KeyCode::Char(' ') => {
                p.sel.resolve(&ids);
                if let Some(id) = ids.get(p.sel.index()) {
                    match p.marked.iter().position(|m| m == id) {
                        Some(i) => {
                            p.marked.remove(i);
                        }
                        None => p.marked.push(id.clone()),
                    }
                }
            }
            KeyCode::Enter => {
                p.sel.resolve(&ids);
                let picked = if p.marked.is_empty() {
                    ids.get(p.sel.index()).cloned().into_iter().collect()
                } else {
                    p.marked.clone()
                };
                let purpose = p.purpose.clone();
                self.devices_tab.controls.picker = None;
                if picked.is_empty() {
                    return;
                }
                match purpose {
                    PickFor::Room { area_id } => self.change_room(area_id, picked, Vec::new()),
                    PickFor::Scene => self.add_scene_members(picked),
                }
            }
            _ => {
                if edit_text(&mut p.query, key) {
                    p.sel.reset();
                }
            }
        }
    }

    // ── Room membership ──────────────────────────────────────────────────

    /// `a` on a room's devices: pick devices to add.
    pub fn pick_devices_for_room(&mut self) {
        let Some(area) = self
            .devices_tab
            .room
            .as_deref()
            .and_then(|id| self.areas.iter().find(|a| a.id == id))
        else {
            return;
        };
        let inside: Vec<String> = self
            .devices
            .iter()
            .filter(|d| super::rooms::in_room(area, d))
            .map(|d| d.device_id.clone())
            .collect();
        let title = format!("Add to {}", area.name);
        let purpose = PickFor::Room {
            area_id: area.id.clone(),
        };
        self.devices_tab.controls.picker = Some(DevicePicker::new(purpose, title, inside));
    }

    /// `x` on a room's devices: take the selected one out, after asking.
    pub fn ask_remove_from_room(&mut self) {
        let Some(area_id) = self.devices_tab.room.clone() else {
            return;
        };
        let Some(d) = self.selected_device() else {
            return;
        };
        let room = self
            .areas
            .iter()
            .find(|a| a.id == area_id)
            .map(|a| a.name.clone())
            .unwrap_or_default();
        let (id, name) = (d.device_id.clone(), d.display_name().to_string());
        self.ask(Confirm::new(
            "Remove from room?",
            format!("{name} will be taken out of {room}. It stays in homeCore."),
            ConfirmAction::Devices(DeviceConfirm::LeaveRoom {
                area_id,
                device_ids: vec![id],
            }),
        ));
    }

    /// Add `add` to the room and take `remove` out.
    pub(crate) fn change_room(&mut self, area_id: String, add: Vec<String>, remove: Vec<String>) {
        let c = self.client.clone();
        let target = area_id.clone();
        let (a2, r2) = (add.clone(), remove.clone());
        self.status = "Saving the room...".into();
        self.spawn_ok(
            "room devices",
            async move { c.change_room_devices(&target, &a2, &r2).await },
            move |app, area: crate::model::Area| {
                let slug = area.slug.clone();
                for d in app.devices.iter_mut() {
                    if add.contains(&d.device_id) {
                        d.area_override = Some(slug.clone());
                        d.room = Some(slug.clone());
                    } else if remove.contains(&d.device_id) {
                        d.area_override = None;
                        if d.area.as_deref() == Some(slug.as_str()) {
                            d.area = None;
                        }
                        d.room = d.area.clone();
                    }
                }
                let name = area.name.clone();
                super::rooms::upsert_area(&mut app.areas, area);
                app.status = match (add.len(), remove.len()) {
                    (0, n) => format!("Took {n} out of {name}"),
                    (n, 0) => format!("Added {n} to {name}"),
                    (a, r) => format!("Added {a} to {name} and took {r} out"),
                };
                app.save_cache(&[Section::Devices, Section::Areas]);
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::model::{Area, Role};
    use crate::screens::ScreenId;

    fn room() -> Area {
        serde_json::from_value(serde_json::json!({
            "id": "r1", "name": "Kitchen", "slug": "kitchen", "device_ids": ["a"]
        }))
        .unwrap()
    }

    #[test]
    fn the_room_picker_offers_devices_not_in_the_room() {
        let mut a = signed_in(Role::Admin);
        let mut inside = device("a", "Alpha");
        inside.room = Some("kitchen".into());
        a.devices = vec![inside, device("b", "Bravo"), device("c", "Charlie")];
        a.areas = vec![room()];
        a.devices_tab.room = Some("r1".into());
        a.screen = ScreenId::Devices;
        a.pick_devices_for_room();
        let names: Vec<&str> = a
            .picker_devices()
            .iter()
            .map(|d| d.display_name())
            .collect();
        assert_eq!(names, vec!["Bravo", "Charlie"]);
        a.on_key(key(KeyCode::Char('c')));
        a.on_key(key(KeyCode::Char('h')));
        let names: Vec<&str> = a
            .picker_devices()
            .iter()
            .map(|d| d.display_name())
            .collect();
        assert_eq!(names, vec!["Charlie"], "typing searches");
        a.on_key(key(KeyCode::Char(' ')));
        assert_eq!(
            a.devices_tab.controls.picker.as_ref().unwrap().marked,
            vec!["c"]
        );
        a.on_key(key(KeyCode::Esc));
        assert!(a.devices_tab.controls.picker.is_none());
    }

    #[test]
    fn taking_a_device_out_of_a_room_asks_first() {
        let mut a = signed_in(Role::Admin);
        let mut inside = device("a", "Alpha");
        inside.room = Some("kitchen".into());
        a.devices = vec![inside];
        a.areas = vec![room()];
        a.devices_tab.room = Some("r1".into());
        a.screen = ScreenId::Devices;
        a.refresh_views();
        a.on_key(key(KeyCode::Char('x')));
        assert!(matches!(
            a.confirm.as_ref().map(|c| &c.action),
            Some(ConfirmAction::Devices(DeviceConfirm::LeaveRoom { device_ids, .. }))
                if device_ids == &vec!["a".to_string()]
        ));
    }
}

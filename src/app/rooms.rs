//! Rooms: what is on in each room, Room on and Room off, and room
//! create, rename and delete.

use super::editors::{AreaEditor, Confirm, ConfirmAction};
use super::{App, ListSel, Section};
use crate::model::{Area, DeviceState};
use crate::screens::ScreenId;
use crate::text::matches_query;
use serde_json::Value;

#[derive(Debug, Default)]
pub struct RoomsTab {
    pub sel: ListSel,
    pub editor: Option<AreaEditor>,
}

/// What the Rooms table shows for a room.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RoomSummary {
    pub lights_on: usize,
    pub lights_total: usize,
    /// On or off of each light, for the dots.
    pub lights: Vec<bool>,
    /// The mean of the room's temperature readings, °C.
    pub temp_c: Option<f64>,
    pub low_battery: usize,
    pub open: usize,
    pub offline: usize,
}

impl RoomSummary {
    /// `"1 low battery · 2 open"`.
    pub fn notes(&self) -> String {
        let mut parts = Vec::new();
        if self.open > 0 {
            parts.push(format!("{} open", self.open));
        }
        if self.low_battery > 0 {
            parts.push(format!("{} low battery", self.low_battery));
        }
        if self.offline > 0 {
            parts.push(format!("{} offline", self.offline));
        }
        parts.join(" · ")
    }
}

/// core's room slug (`normalize_name_segment`): ASCII letters and
/// digits in lower case, runs of anything else as one `_`.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for ch in s.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            if gap && !out.is_empty() {
                out.push('_');
            }
            gap = false;
            out.push(ch.to_ascii_lowercase());
        } else {
            gap = true;
        }
    }
    out
}

/// Whether `d` is in `room`: listed by the room, or its own room field
/// names the room's slug or one of its aliases.
pub fn in_room(room: &Area, d: &DeviceState) -> bool {
    if room.device_ids.contains(&d.device_id) {
        return true;
    }
    let Some(r) = d.room_name() else {
        return false;
    };
    let s = slug(r);
    s == room.slug || r == room.id || room.aliases.contains(&s)
}

/// A light, as core's Room on counts them: `light`, or anything a person
/// marked as a light; never a group or a scene, nor a camera's own
/// spotlight.
pub fn is_light(d: &DeviceState, all: &[DeviceState]) -> bool {
    let t = d.device_type();
    if matches!(t, "group" | "scene") {
        return false;
    }
    if t != "light" && d.ui_hint.as_deref() != Some("light") {
        return false;
    }
    if let Some(camera) = d.device_id.strip_suffix("_spotlight")
        && all
            .iter()
            .any(|c| c.device_id == camera && matches!(c.device_type(), "camera" | "doorbell"))
    {
        return false;
    }
    true
}

/// On: its `on`, else a brightness above nothing.
pub fn light_on(d: &DeviceState) -> bool {
    match d.attributes.get("on") {
        Some(v) => v.as_bool().unwrap_or(false),
        None => d
            .attributes
            .get("brightness_pct")
            .and_then(Value::as_f64)
            .is_some_and(|b| b > 0.0),
    }
}

impl App {
    pub(crate) fn compute_room_view(&mut self) {
        let q = self.query(ScreenId::Rooms).to_string();
        let mut idx: Vec<usize> = (0..self.areas.len())
            .filter(|&i| matches_query(&q, &[&self.areas[i].name]))
            .collect();
        idx.sort_by_cached_key(|&i| self.areas[i].name.to_lowercase());
        self.views.rooms = idx;
    }

    pub(crate) fn room_view_ids(&self) -> Vec<String> {
        self.views
            .rooms
            .iter()
            .filter_map(|&i| self.areas.get(i))
            .map(|a| a.id.clone())
            .collect()
    }

    pub fn visible_rooms(&self) -> impl Iterator<Item = &Area> {
        self.views.rooms.iter().filter_map(|&i| self.areas.get(i))
    }

    pub fn selected_room(&self) -> Option<&Area> {
        let id = self.rooms.sel.id()?;
        self.visible_rooms().find(|a| a.id == id)
    }

    /// The room's devices, scenes left out, by name.
    pub fn room_devices(&self, room: &Area) -> Vec<&DeviceState> {
        let mut v: Vec<&DeviceState> = self
            .devices
            .iter()
            .filter(|d| !d.is_scene() && in_room(room, d))
            .collect();
        v.sort_by_cached_key(|d| d.display_name().to_lowercase());
        v
    }

    pub fn room_summary(&self, room: &Area) -> RoomSummary {
        let mut s = RoomSummary::default();
        let mut temps = Vec::new();
        for d in self.room_devices(room) {
            if is_light(d, &self.devices) {
                let on = light_on(d);
                s.lights.push(on);
                s.lights_total += 1;
                s.lights_on += usize::from(on);
            }
            if let Some(t) = super::devices::device_temperature(d)
                && d.device_type() != "weather"
            {
                temps.push(t);
            }
            if super::devices::device_battery(d).is_some_and(|b| b <= 20) {
                s.low_battery += 1;
            }
            let b = |k: &str| d.attributes.get(k).and_then(Value::as_bool);
            if b("open").or_else(|| b("contact_open")) == Some(true) {
                s.open += 1;
            }
            if !d.available {
                s.offline += 1;
            }
        }
        if !temps.is_empty() {
            s.temp_c = Some(temps.iter().sum::<f64>() / temps.len() as f64);
        }
        s
    }

    pub fn move_room(&mut self, delta: isize) {
        let ids = self.room_view_ids();
        self.rooms.sel.move_by(delta, &ids);
    }

    /// Space: every light in the room off when any is on, else on.
    pub fn toggle_selected_room(&mut self) {
        let Some(room) = self.selected_room() else {
            return;
        };
        let on = self.room_summary(room).lights_on == 0;
        self.room_switch(room.id.clone(), on);
    }

    /// `POST /areas/{id}/on|off`. The lights' own events bring the new
    /// state.
    pub fn room_switch(&mut self, id: String, on: bool) {
        let name = self
            .areas
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.name.clone())
            .unwrap_or_else(|| id.clone());
        let word = if on { "on" } else { "off" };
        self.status = format!("Turning {name} {word}...");
        let c = self.client.clone();
        self.spawn_ok(
            "room on/off",
            async move { c.room_switch(&id, on).await },
            move |app, ()| app.status = format!("{name} {word}"),
        );
    }

    /// Enter: the room's devices on the Devices screen.
    pub fn open_selected_room(&mut self) {
        let Some(room) = self.selected_room() else {
            return;
        };
        let id = room.id.clone();
        self.devices_tab.sub = super::DeviceSubPanel::All;
        self.devices_tab.room = Some(id);
        self.devices_tab.sel.reset();
        self.queries.remove(&ScreenId::Devices);
        self.switch_screen(ScreenId::Devices);
        self.return_to = Some(ScreenId::Rooms);
    }

    pub fn new_room(&mut self) {
        self.rooms.editor = Some(AreaEditor {
            id: None,
            name: String::new(),
        });
    }

    pub fn rename_selected_room(&mut self) {
        let Some(room) = self.selected_room() else {
            return;
        };
        self.rooms.editor = Some(AreaEditor {
            id: Some(room.id.clone()),
            name: room.name.clone(),
        });
    }

    pub(crate) fn save_area_editor(&mut self) {
        let Some(ed) = self.rooms.editor.clone() else {
            return;
        };
        let name = ed.name.trim().to_string();
        if name.is_empty() {
            self.error = Some("a room needs a name".to_string());
            return;
        }
        let c = self.client.clone();
        match ed.id {
            None => self.spawn_ok(
                "create room",
                async move { c.create_area(&name).await },
                |app, area: Area| {
                    app.rooms.editor = None;
                    app.status = format!("Made room {}", area.name);
                    app.rooms.sel.select_id(area.id.clone());
                    upsert_area(&mut app.areas, area);
                    app.save_cache(&[Section::Areas]);
                },
            ),
            Some(id) => self.spawn_ok(
                "rename room",
                async move { c.rename_area(&id, &name).await },
                |app, area: Area| {
                    app.rooms.editor = None;
                    app.status = format!("Renamed the room to {}", area.name);
                    upsert_area(&mut app.areas, area);
                    app.save_cache(&[Section::Areas]);
                },
            ),
        }
    }

    pub fn ask_delete_room(&mut self) {
        let Some(room) = self.selected_room() else {
            return;
        };
        let (id, name) = (room.id.clone(), room.name.clone());
        let n = self.room_devices(room).len();
        self.ask(
            Confirm::new(
                "Delete room?",
                format!("{name} will be deleted from homeCore."),
                ConfirmAction::DeleteArea { id, name },
            )
            .with_detail(format!("Its {n} device(s) will have no room.")),
        );
    }

    pub(crate) fn delete_area(&mut self, id: String, name: String) {
        let c = self.client.clone();
        let target = id.clone();
        self.spawn_ok(
            "delete room",
            async move { c.delete_area(&target).await },
            move |app, ()| {
                app.areas.retain(|a| a.id != id);
                app.status = format!("Deleted room {name}");
                app.save_cache(&[Section::Areas]);
            },
        );
    }
}

pub fn upsert_area(areas: &mut Vec<Area>, area: Area) {
    match areas.iter_mut().find(|a| a.id == area.id) {
        Some(a) => *a = area,
        None => areas.push(area),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::*;
    use crate::model::Role;
    use serde_json::json;

    fn area(id: &str, name: &str, ids: &[&str]) -> Area {
        Area {
            id: id.into(),
            name: name.into(),
            slug: slug(name),
            device_ids: ids.iter().map(|s| s.to_string()).collect(),
            aliases: vec![],
        }
    }

    fn light(id: &str, on: bool) -> DeviceState {
        let mut d = device(id, id);
        d.device_type = Some("light".into());
        d.attributes = json!({ "on": on }).as_object().cloned().unwrap();
        d
    }

    #[test]
    fn slugs_match_core() {
        assert_eq!(slug("Primary Bedroom"), "primary_bedroom");
        assert_eq!(slug("  Kids' Room!! "), "kids_room");
    }

    #[test]
    fn summary_counts_lights_temps_and_notes() {
        let mut a = signed_in(Role::Admin);
        let mut temp = device("t", "Temp");
        temp.room = Some("kitchen".into());
        temp.attributes = json!({"temperature": 20.0, "battery_pct": 10})
            .as_object()
            .cloned()
            .unwrap();
        let mut door = device("door", "Door");
        door.room = Some("Kitchen".into());
        door.attributes = json!({"open": true}).as_object().cloned().unwrap();
        let mut spot = light("cam_spotlight", true);
        spot.room = Some("kitchen".into());
        let mut cam = device("cam", "Cam");
        cam.device_type = Some("camera".into());
        let mut switch = device("sw", "Switch");
        switch.ui_hint = Some("light".into());
        switch.attributes = json!({"on": false}).as_object().cloned().unwrap();
        a.devices = vec![
            light("l1", true),
            light("l2", false),
            temp,
            door,
            spot,
            cam,
            switch,
        ];
        let k = area("k", "Kitchen", &["l1", "l2", "sw"]);
        let s = a.room_summary(&k);
        assert_eq!((s.lights_on, s.lights_total), (1, 3));
        assert_eq!(s.temp_c, Some(20.0));
        assert_eq!(s.notes(), "1 open · 1 low battery");
    }

    #[test]
    fn delete_asks_first() {
        let mut a = signed_in(Role::Admin);
        a.areas = vec![area("k", "Kitchen", &[])];
        a.screen = ScreenId::Rooms;
        a.refresh_views();
        a.on_key(key(crossterm::event::KeyCode::Char('x')));
        assert!(matches!(
            a.confirm.as_ref().map(|c| &c.action),
            Some(ConfirmAction::DeleteArea { id, .. }) if id == "k"
        ));
        a.on_key(key(crossterm::event::KeyCode::Esc));
        assert_eq!(a.areas.len(), 1);
    }

    #[test]
    fn enter_opens_the_rooms_devices() {
        let mut a = signed_in(Role::Admin);
        let mut l = light("l1", true);
        l.room = Some("kitchen".into());
        a.devices = vec![l, light("other", false)];
        a.areas = vec![area("k", "Kitchen", &[])];
        a.screen = ScreenId::Rooms;
        a.refresh_views();
        a.on_key(key(crossterm::event::KeyCode::Enter));
        assert_eq!(a.screen, ScreenId::Devices);
        a.refresh_views();
        assert_eq!(a.device_view_ids(), vec!["l1"]);
        a.on_key(key(crossterm::event::KeyCode::Esc));
        assert_eq!(a.screen, ScreenId::Rooms);
        assert!(a.devices_tab.room.is_none());
    }
}

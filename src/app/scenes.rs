//! Scenes: making, editing and deleting homeCore's own scenes. Plugin
//! scenes (devices with `device_type: "scene"`) are run from here but
//! belong to their plugin, so they are read only.
//!
//! A homeCore scene is a name and, per device, the command to send
//! (`{"on": true, "brightness_pct": 40}`). Adding a device takes how it
//! is now, the way the app does: its first few everyday settings, or
//! just `on: false` when it is off.

use super::controls::DeviceConfirm;
use super::editors::{Confirm, ConfirmAction, edit_text};
use super::picker::{DevicePicker, PickFor};
use super::{App, Section};
use crate::model::{DeviceState, Scene, SceneSource};
use crate::model_devices::{AttributeKind, DeviceSchema};
use crossterm::event::{KeyCode, KeyEvent};
use serde_json::{Map, Value};

#[derive(Debug, Default)]
pub struct ScenesTab {
    pub editor: Option<SceneEditor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneField {
    Name,
    Devices,
}

#[derive(Debug, Clone)]
pub struct SceneEditor {
    /// `None` makes a new scene.
    pub id: Option<String>,
    pub name: String,
    /// Device id and the command it gets, in the order shown.
    pub members: Vec<(String, Value)>,
    pub field: SceneField,
    pub sel: usize,
}

impl SceneEditor {
    pub fn states(&self) -> Map<String, Value> {
        self.members.iter().cloned().collect()
    }
}

/// The settings a scene takes from a device: writable, everyday (not
/// setup or diagnostics) switches, ranges and choices, `on` first, then
/// the schema's headline order, at most `max`.
pub fn scene_settings(d: &DeviceState, schema: Option<&DeviceSchema>, max: usize) -> Vec<String> {
    let mut keys: Vec<String> = match schema {
        Some(s) => {
            let rank = |k: &str| s.primary.iter().position(|p| p == k).unwrap_or(usize::MAX);
            let mut v: Vec<&String> = s
                .attributes
                .iter()
                .filter(|(_, a)| a.writable && a.category.is_none())
                .filter(|(_, a)| match a.kind {
                    AttributeKind::Bool => true,
                    AttributeKind::Integer | AttributeKind::Float | AttributeKind::ColorTemp => {
                        a.min.is_some() && a.max.is_some()
                    }
                    AttributeKind::Enum => a.options.as_ref().is_some_and(|o| !o.is_empty()),
                    _ => false,
                })
                .map(|(k, _)| k)
                .collect();
            v.sort_by_key(|k| (k.as_str() != "on", rank(k), k.to_string()));
            v.into_iter().cloned().collect()
        }
        None => [
            "on",
            "brightness_pct",
            "color_temp",
            "position",
            "locked",
            "hvac_mode",
            "setpoint",
            "heat_setpoint",
            "cool_setpoint",
        ]
        .iter()
        .map(|k| k.to_string())
        .collect(),
    };
    keys.retain(|k| d.attributes.get(k).is_some_and(|v| !v.is_null()));
    // A schema that names a category for everything leaves nothing;
    // fall back to the vocabulary then.
    if keys.is_empty() && schema.is_some() {
        return scene_settings(d, None, max);
    }
    keys.truncate(max);
    keys
}

/// How `d` is now, as a scene command. Off is just `{"on": false}`.
pub fn capture_state(d: &DeviceState, schema: Option<&DeviceSchema>) -> Value {
    if d.attributes.get("on") == Some(&Value::Bool(false)) {
        return serde_json::json!({ "on": false });
    }
    let mut out = Map::new();
    for k in scene_settings(d, schema, 3) {
        if let Some(v) = d.attributes.get(&k) {
            out.insert(k, v.clone());
        }
    }
    Value::Object(out)
}

/// A scene command in a few words: `on, 40%`, `off`, `locked`, `3 steps`.
pub fn state_summary(v: &Value, house: crate::model::TempUnit) -> String {
    let Some(m) = v.as_object() else {
        return v.to_string();
    };
    if let Some(steps) = m.get("actions").and_then(Value::as_array) {
        return format!("{} steps", steps.len());
    }
    let mut parts = Vec::new();
    let mut keys: Vec<(&String, &Value)> = m.iter().collect();
    keys.sort_by_key(|(k, _)| k.as_str() != "on");
    for (k, v) in keys {
        let p = match (k.as_str(), v) {
            ("on", Value::Bool(b)) => if *b { "on" } else { "off" }.to_string(),
            ("locked", Value::Bool(b)) => if *b { "locked" } else { "unlocked" }.to_string(),
            ("brightness_pct", n) => format!("{}%", n),
            ("position", n) => format!("open {}%", n),
            ("color_temp", n) => format!("{n} K"),
            (k, Value::Number(n)) if k.contains("setpoint") => {
                house.format(n.as_f64().unwrap_or(0.0))
            }
            (k, Value::String(s)) => format!("{} {s}", crate::text::normalize_label(k)),
            (k, other) => format!("{} {other}", crate::text::normalize_label(k)),
        };
        parts.push(p);
    }
    if parts.is_empty() {
        "nothing set".into()
    } else {
        parts.join(", ")
    }
}

impl App {
    /// `n` on Scenes.
    pub fn new_scene(&mut self) {
        self.scenes_tab.editor = Some(SceneEditor {
            id: None,
            name: String::new(),
            members: Vec::new(),
            field: SceneField::Name,
            sel: 0,
        });
    }

    /// Enter on Scenes: edit a homeCore scene; a plugin's is read only.
    pub fn edit_selected_scene(&mut self) {
        let Some(s) = self.selected_scene() else {
            return;
        };
        if s.source == SceneSource::Device {
            self.status = format!("{} belongs to its plugin; it can only be run here", s.name);
            return;
        }
        let mut members: Vec<(String, Value)> = s
            .states
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        members.sort_by_cached_key(|(id, _)| {
            self.device_by_id(id)
                .map(|d| d.display_name().to_lowercase())
                .unwrap_or_else(|| id.clone())
        });
        self.scenes_tab.editor = Some(SceneEditor {
            id: Some(s.id.clone()),
            name: s.name.clone(),
            members,
            field: SceneField::Name,
            sel: 0,
        });
    }

    /// `x` on Scenes.
    pub fn ask_delete_scene(&mut self) {
        let Some(s) = self.selected_scene() else {
            return;
        };
        if s.source == SceneSource::Device {
            self.status = format!(
                "{} belongs to its plugin and cannot be deleted here",
                s.name
            );
            return;
        }
        let (id, name) = (s.id.clone(), s.name.clone());
        self.ask(Confirm::new(
            "Delete scene?",
            format!("{name} will be deleted from homeCore."),
            ConfirmAction::Devices(DeviceConfirm::DeleteScene { id, name }),
        ));
    }

    pub(crate) fn delete_scene(&mut self, id: String, name: String) {
        let c = self.client.clone();
        let target = id.clone();
        self.spawn_ok(
            "delete scene",
            async move { c.delete_scene(&target).await },
            move |app, ()| {
                app.native_scenes.retain(|s| s.id != id);
                app.rebuild_scenes();
                app.status = format!("Deleted {name}");
                app.save_cache(&[Section::Scenes]);
            },
        );
    }

    /// Picked devices join the scene as they are now.
    pub(crate) fn add_scene_members(&mut self, ids: Vec<String>) {
        for id in ids {
            let state = match self.device_by_id(&id) {
                Some(d) => capture_state(d, self.device_schema(&id)),
                None => continue,
            };
            if let Some(ed) = self.scenes_tab.editor.as_mut()
                && !ed.members.iter().any(|(m, _)| *m == id)
            {
                ed.members.push((id.clone(), state));
                ed.sel = ed.members.len() - 1;
            }
            if !self.devices_tab.controls.schemas.contains_key(&id)
                && tokio::runtime::Handle::try_current().is_ok()
            {
                // Take it again with its schema once that is here.
                let again = id.clone();
                self.fetch_schema(id, move |app| app.recapture_scene_member(&again));
            }
        }
    }

    fn recapture_scene_member(&mut self, id: &str) {
        let Some(d) = self.device_by_id(id) else {
            return;
        };
        let state = capture_state(d, self.device_schema(id));
        if let Some(ed) = self.scenes_tab.editor.as_mut()
            && let Some(m) = ed.members.iter_mut().find(|(m, _)| m == id)
        {
            m.1 = state;
        }
    }

    pub(crate) fn on_key_scene_editor(&mut self, key: KeyEvent) {
        let Some(ed) = self.scenes_tab.editor.as_mut() else {
            return;
        };
        match (key.code, ed.field) {
            (KeyCode::Esc, _) => {
                self.scenes_tab.editor = None;
                self.status = "Cancelled".into();
            }
            (KeyCode::Tab | KeyCode::BackTab, _) => {
                ed.field = match ed.field {
                    SceneField::Name => SceneField::Devices,
                    SceneField::Devices => SceneField::Name,
                };
            }
            (KeyCode::Enter, _) => self.save_scene_editor(),
            (KeyCode::Down, SceneField::Devices) => {
                ed.sel = (ed.sel + 1).min(ed.members.len().saturating_sub(1));
            }
            (KeyCode::Up, SceneField::Devices) => ed.sel = ed.sel.saturating_sub(1),
            (KeyCode::Down, SceneField::Name) => ed.field = SceneField::Devices,
            (KeyCode::Char('a'), SceneField::Devices) => {
                let exclude = ed.members.iter().map(|(id, _)| id.clone()).collect();
                let title = format!(
                    "Add to {}",
                    if ed.name.trim().is_empty() {
                        "the scene"
                    } else {
                        ed.name.trim()
                    }
                );
                self.devices_tab.controls.picker =
                    Some(DevicePicker::new(PickFor::Scene, title, exclude));
            }
            (KeyCode::Char('x') | KeyCode::Delete, SceneField::Devices) => {
                if ed.sel < ed.members.len() {
                    ed.members.remove(ed.sel);
                    ed.sel = ed.sel.min(ed.members.len().saturating_sub(1));
                }
            }
            (KeyCode::Char('c'), SceneField::Devices) => {
                if let Some(id) = ed.members.get(ed.sel).map(|(id, _)| id.clone()) {
                    self.recapture_scene_member(&id);
                    self.status = "Took how it is now".into();
                }
            }
            (_, SceneField::Name) => {
                edit_text(&mut ed.name, key);
            }
            _ => {}
        }
    }

    fn save_scene_editor(&mut self) {
        let Some(ed) = self.scenes_tab.editor.clone() else {
            return;
        };
        let name = ed.name.trim().to_string();
        if name.is_empty() {
            self.error = Some("a scene needs a name".into());
            return;
        }
        if ed.members.is_empty() {
            self.error = Some("add a device first (Tab, then a)".into());
            return;
        }
        let states = ed.states();
        let c = self.client.clone();
        self.status = format!("Saving {name}...");
        self.spawn_ok(
            "save scene",
            async move {
                match ed.id.as_deref() {
                    Some(id) => c.update_scene(id, &name, &states).await,
                    None => c.create_scene(&name, &states).await,
                }
            },
            |app, scene: Scene| {
                app.status = format!("Saved {}", scene.name);
                app.scenes_sel.select_id(scene.id.clone());
                match app.native_scenes.iter_mut().find(|s| s.id == scene.id) {
                    Some(s) => *s = scene,
                    None => app.native_scenes.push(scene),
                }
                app.rebuild_scenes();
                app.scenes_tab.editor = None;
                app.save_cache(&[Section::Scenes]);
            },
        );
    }

    /// The modals of Devices, Rooms and Scenes: the picker over the
    /// scene editor. True when one took the key.
    pub(crate) fn on_key_device_modals(&mut self, key: KeyEvent) -> bool {
        if self.devices_tab.controls.picker.is_some() {
            self.on_key_device_picker(key);
        } else if self.scenes_tab.editor.is_some() {
            self.on_key_scene_editor(key);
        } else {
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::model::Role;
    use serde_json::json;

    fn lamp(attrs: Value) -> DeviceState {
        let mut d = device("lamp", "Lamp");
        d.device_type = Some("light".into());
        d.attributes = attrs.as_object().cloned().unwrap();
        d
    }

    #[test]
    fn capturing_takes_the_everyday_settings() {
        let d = lamp(json!({"on": true, "brightness_pct": 40, "color_temp": 2700, "rssi": -50}));
        let s: DeviceSchema = serde_json::from_value(json!({
            "attributes": {
                "on": {"kind": "bool"},
                "brightness_pct": {"kind": "integer", "min": 0.0, "max": 100.0},
                "color_temp": {"kind": "color_temp", "min": 2000.0, "max": 6500.0},
                "rssi": {"kind": "integer", "min": -100.0, "max": 0.0, "category": "diagnostic"},
                "transition_ms": {"kind": "integer", "min": 0.0, "max": 10000.0, "category": "config"}
            },
            "primary": ["brightness_pct", "on", "color_temp"]
        }))
        .unwrap();
        assert_eq!(
            capture_state(&d, Some(&s)),
            json!({"on": true, "brightness_pct": 40, "color_temp": 2700})
        );
        let off = lamp(json!({"on": false, "brightness_pct": 40}));
        assert_eq!(capture_state(&off, Some(&s)), json!({"on": false}));
        // Without a schema, the vocabulary.
        let lock = lamp(json!({"locked": true, "battery_pct": 80}));
        assert_eq!(capture_state(&lock, None), json!({"locked": true}));
    }

    #[test]
    fn summaries_read_short() {
        let t = crate::model::TempUnit::Celsius;
        assert_eq!(
            state_summary(&json!({"on": true, "brightness_pct": 40}), t),
            "on, 40%"
        );
        assert_eq!(state_summary(&json!({"actions": [{}, {}]}), t), "2 steps");
        assert_eq!(state_summary(&json!({"on": false}), t), "off");
    }

    #[test]
    fn a_new_scene_needs_a_name_and_a_device() {
        let mut a = signed_in(Role::Admin);
        a.devices = vec![lamp(json!({"on": true, "brightness_pct": 40}))];
        a.screen = crate::screens::ScreenId::Scenes;
        a.on_key(key(KeyCode::Char('n')));
        assert!(a.scenes_tab.editor.is_some());
        for ch in "Evening".chars() {
            a.on_key(key(KeyCode::Char(ch)));
        }
        a.on_key(key(KeyCode::Enter));
        assert!(a.error.as_deref().unwrap_or("").contains("device"));
        a.on_key(key(KeyCode::Tab));
        a.on_key(key(KeyCode::Char('a')));
        assert!(a.devices_tab.controls.picker.is_some(), "a picks devices");
        a.on_key(key(KeyCode::Enter));
        let ed = a.scenes_tab.editor.as_ref().unwrap();
        assert_eq!(ed.name, "Evening");
        assert_eq!(
            ed.members,
            vec![(
                "lamp".to_string(),
                json!({"on": true, "brightness_pct": 40})
            )]
        );
        assert_eq!(
            crate::net::devices_api::scene_body(&ed.name, &ed.states()),
            json!({"name": "Evening", "states": {"lamp": {"on": true, "brightness_pct": 40}}})
        );
        a.on_key(key(KeyCode::Char('x')));
        assert!(a.scenes_tab.editor.as_ref().unwrap().members.is_empty());
        a.on_key(key(KeyCode::Esc));
        assert!(a.scenes_tab.editor.is_none());
    }

    #[test]
    fn plugin_scenes_are_read_only() {
        let mut a = signed_in(Role::Admin);
        let mut s = device("hue_s", "Relax");
        s.device_type = Some("scene".into());
        a.devices = vec![s];
        a.rebuild_scenes();
        a.screen = crate::screens::ScreenId::Scenes;
        a.refresh_views();
        a.on_key(key(KeyCode::Enter));
        assert!(a.scenes_tab.editor.is_none());
        a.on_key(key(KeyCode::Char('x')));
        assert!(a.confirm.is_none());
        assert!(a.status.contains("plugin"));
    }
}

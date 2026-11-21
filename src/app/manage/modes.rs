//! Modes: the list, making, editing and deleting modes, and switching
//! manual ones.
//!
//! A new mode is `POST /modes {id, name, kind}`; core takes no sun times
//! then, so a solar mode's events and offsets follow with
//! `PATCH /modes/{id}`, which is also how a mode is edited.

use crate::app::editors::{Confirm, ConfirmAction, ModeEditor, ModeKind, sun_event_words};
use crate::app::{App, Section};
use crate::model::{ModeConfig, ModeRecord};
use serde_json::Value;

/// The modes core keeps and will not delete.
const BUILT_IN: &[&str] = &["mode_day", "mode_night"];

/// `sunset -15m`, `sunrise`.
fn event_text(event: Option<&str>, offset: Option<i32>) -> Option<String> {
    let e = sun_event_words(event?);
    Some(match offset.unwrap_or(0) {
        0 => e,
        o if o > 0 => format!("{e} +{o}m"),
        o => format!("{e} {o}m"),
    })
}

/// When a solar mode next changes, from its device's attributes
/// (`effective_on`/`effective_off`, today's times with the offsets in,
/// local `HH:MM`): `until sunset 16:46` while on, `from sunset 16:46`
/// while off. Empty for a manual mode or when core sent no times.
pub fn mode_schedule(
    m: &ModeConfig,
    on: bool,
    attrs: Option<&serde_json::Map<String, Value>>,
) -> String {
    if m.kind != "solar" {
        return String::new();
    }
    let time = |k: &str| {
        attrs
            .and_then(|a| a.get(k))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let (word, event, offset, at) = if on {
        (
            "until",
            m.off_event.as_deref(),
            m.off_offset_minutes,
            time("effective_off"),
        )
    } else {
        (
            "from",
            m.on_event.as_deref(),
            m.on_offset_minutes,
            time("effective_on"),
        )
    };
    match (event_text(event, offset), at) {
        (Some(e), Some(t)) => format!("{word} {e} {t}"),
        (Some(e), None) => format!("{word} {e}"),
        (None, Some(t)) => format!("{word} {t}"),
        (None, None) => String::new(),
    }
}

impl App {
    /// `d` on a criteria mode: what turns it on and off.
    pub fn open_mode_definition(&mut self) {
        if let Some(m) = self.selected_mode().filter(|m| m.definition.is_some()) {
            self.manage.mode_detail = Some(m.config.id.clone());
        }
    }

    pub fn close_mode_definition(&mut self) {
        self.manage.mode_detail = None;
    }

    pub fn selected_mode(&self) -> Option<&ModeRecord> {
        let id = self.manage.modes_sel.id()?;
        self.views
            .modes
            .iter()
            .filter_map(|&i| self.modes.get(i))
            .find(|m| m.config.id == id)
    }

    pub fn move_mode(&mut self, delta: isize) {
        let ids = self.mode_view_ids();
        self.manage.modes_sel.move_by(delta, &ids);
    }

    pub fn toggle_selected_mode(&mut self) {
        if let Some(id) = self.selected_mode().map(|m| m.config.id.clone()) {
            self.toggle_mode(&id);
        }
    }

    /// The device attributes of mode `id`: the live device when there is
    /// one, else what `/modes` sent.
    pub fn mode_attributes(&self, id: &str) -> Option<&serde_json::Map<String, Value>> {
        self.device_by_id(id).map(|d| &d.attributes).or_else(|| {
            self.modes
                .iter()
                .find(|m| m.config.id == id)
                .and_then(|m| m.state.as_ref())
                .map(|s| &s.attributes)
        })
    }

    pub fn new_mode(&mut self) {
        self.manage.mode_editor = Some(ModeEditor::create());
    }

    /// Enter on Modes.
    pub fn edit_selected_mode(&mut self) {
        if let Some(m) = self.selected_mode() {
            self.manage.mode_editor = Some(ModeEditor::edit(&m.config));
        }
    }

    pub fn ask_delete_mode(&mut self) {
        if let Some(m) = self.selected_mode() {
            let id = m.config.id.clone();
            let name = m.config.name.clone();
            if BUILT_IN.contains(&id.as_str()) {
                self.status = format!("{name} is built in and cannot be deleted");
                return;
            }
            self.ask(Confirm::new(
                "Delete mode?",
                format!("{name} ({id}) will be deleted from homeCore."),
                ConfirmAction::DeleteMode { id },
            ));
        }
    }

    pub(crate) fn save_mode_editor(&mut self) {
        let Some(ed) = self.manage.mode_editor.clone() else {
            return;
        };
        let name = ed.name.trim().to_string();
        if name.is_empty() {
            self.error = Some("a mode needs a name".to_string());
            return;
        }
        let patch = ed.patch();
        let c = self.client.clone();
        match ed.editing.clone() {
            Some(before) => {
                if patch.is_empty() {
                    self.manage.mode_editor = None;
                    self.status = "No changes".into();
                    return;
                }
                self.spawn_ok(
                    "save mode",
                    async move { c.update_mode(&before.id, patch).await },
                    |app, cfg: ModeConfig| {
                        app.status = format!("Saved {}", cfg.name);
                        app.put_mode(cfg);
                        app.manage.mode_editor = None;
                    },
                );
            }
            None => {
                let id = ed.new_id();
                if id == "mode_" {
                    self.error = Some("the name needs a letter or a digit".to_string());
                    return;
                }
                let kind = ed.kind.as_str();
                self.spawn_ok(
                    "create mode",
                    async move {
                        let made = c.create_mode(&id, &name, kind).await?;
                        if ed.kind == ModeKind::Solar && !patch.is_empty() {
                            return c.update_mode(&made.id, patch).await;
                        }
                        Ok(made)
                    },
                    |app, cfg: ModeConfig| {
                        app.status = format!("Created {}", cfg.name);
                        app.manage.modes_sel.select_id(cfg.id.clone());
                        app.put_mode(cfg);
                        app.manage.mode_editor = None;
                    },
                );
            }
        }
    }

    fn put_mode(&mut self, cfg: ModeConfig) {
        match self.modes.iter_mut().find(|m| m.config.id == cfg.id) {
            Some(m) => m.config = cfg,
            None => self.modes.push(ModeRecord {
                config: cfg,
                state: None,
                definition: None,
            }),
        }
        self.save_cache(&[Section::Modes]);
    }

    pub(crate) fn delete_mode(&mut self, id: String) {
        let c = self.client.clone();
        let target = id.clone();
        self.spawn_ok(
            "delete mode",
            async move { c.delete_mode(&target).await },
            move |app, ()| {
                app.modes.retain(|m| m.config.id != id);
                app.status = format!("Deleted {id}");
                app.save_cache(&[Section::Modes]);
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::editors::ModeEditField;
    use serde_json::json;

    fn solar() -> ModeConfig {
        serde_json::from_value(json!({
            "id": "mode_day", "name": "Day", "kind": "solar",
            "on_event": "Sunrise", "off_event": "Sunset", "off_offset_minutes": -15
        }))
        .unwrap()
    }

    #[test]
    fn a_solar_mode_says_when_it_changes() {
        let attrs = json!({"effective_on": "07:01", "effective_off": "16:31"});
        let attrs = attrs.as_object();
        assert_eq!(
            mode_schedule(&solar(), true, attrs),
            "until sunset -15m 16:31"
        );
        assert_eq!(mode_schedule(&solar(), false, attrs), "from sunrise 07:01");
        assert_eq!(mode_schedule(&solar(), true, None), "until sunset -15m");
        let mut manual = solar();
        manual.kind = "manual".into();
        assert_eq!(mode_schedule(&manual, true, attrs), "");
    }

    #[test]
    fn a_new_solar_mode_sends_its_sun_times_after() {
        let mut ed = ModeEditor::create();
        ed.name = "Porch time!".into();
        assert_eq!(ed.new_id(), "mode_porch_time");
        assert_eq!(ed.fields(), vec![ModeEditField::Name, ModeEditField::Kind]);
        assert!(ed.patch().is_empty(), "a manual mode has no sun times");
        ed.kind = ModeKind::Solar;
        ed.on_event = "Sunset".into();
        ed.on_offset = -15;
        let p = serde_json::Value::Object(ed.patch());
        assert_eq!(
            p,
            json!({"on_event": "Sunset", "off_event": "Sunset", "on_offset_minutes": -15})
        );
    }

    #[test]
    fn editing_sends_only_what_changed() {
        let mut ed = ModeEditor::edit(&solar());
        assert!(ed.patch().is_empty());
        assert!(!ed.fields().contains(&ModeEditField::Kind), "kind is fixed");
        ed.name = "Daytime".into();
        ed.off_offset = 0;
        assert_eq!(
            serde_json::Value::Object(ed.patch()),
            json!({"name": "Daytime", "off_offset_minutes": 0})
        );
    }
}

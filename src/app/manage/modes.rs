//! Manage › Modes.

use crate::app::editors::{Confirm, ConfirmAction, ModeEditField, ModeEditor, ModeKind};
use crate::app::{App, Section};
use crate::model::{ModeConfig, ModeRecord};
use crossterm::event::{KeyCode, KeyEvent};

impl App {
    pub fn selected_mode(&self) -> Option<&ModeRecord> {
        match self.manage.modes_sel.id() {
            Some(id) => self.modes.iter().find(|m| m.config.id == id),
            None => self.modes.get(self.manage.modes_sel.index()),
        }
    }

    pub(crate) fn on_key_modes(&mut self, key: KeyEvent) {
        let ids: Vec<String> = self.modes.iter().map(|m| m.config.id.clone()).collect();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.manage.modes_sel.move_by(1, &ids),
            KeyCode::Up | KeyCode::Char('k') => self.manage.modes_sel.move_by(-1, &ids),
            KeyCode::Enter | KeyCode::Char('n') => {
                self.manage.mode_editor = Some(ModeEditor {
                    id: String::new(),
                    name: String::new(),
                    kind: ModeKind::Solar,
                    field: ModeEditField::Id,
                });
            }
            KeyCode::Char('d') => {
                if let Some(m) = self.selected_mode() {
                    let id = m.config.id.clone();
                    let name = m.config.name.clone();
                    self.ask(Confirm::new(
                        "Delete mode",
                        format!("Delete mode \"{name}\" ({id})?"),
                        ConfirmAction::DeleteMode { id },
                    ));
                }
            }
            _ => {}
        }
    }

    pub(crate) fn save_mode_editor(&mut self) {
        let Some(ed) = self.manage.mode_editor.clone() else {
            return;
        };
        let id = ed.id.trim().to_string();
        if id.is_empty() {
            self.error = Some("mode id cannot be empty".to_string());
            return;
        }
        if !id.starts_with("mode_") {
            self.error = Some("mode id must start with 'mode_'".to_string());
            return;
        }
        let name = match ed.name.trim() {
            "" => id.clone(),
            n => n.to_string(),
        };
        let c = self.client.clone();
        self.spawn_ok(
            "create mode",
            async move { c.create_mode(&id, &name, ed.kind.as_str()).await },
            |app, cfg: ModeConfig| {
                app.status = format!("Created {}", cfg.id);
                app.modes.push(ModeRecord {
                    config: cfg,
                    state: None,
                });
                app.manage.mode_editor = None;
                app.save_cache(&[Section::Modes]);
            },
        );
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

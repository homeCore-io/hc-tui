//! Modes: the list, creating and deleting, and switching manual modes.

use crate::app::editors::{Confirm, ConfirmAction, ModeEditField, ModeEditor, ModeKind};
use crate::app::{App, Section};
use crate::model::{ModeConfig, ModeRecord};

impl App {
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

    pub fn new_mode(&mut self) {
        self.manage.mode_editor = Some(ModeEditor {
            id: String::new(),
            name: String::new(),
            kind: ModeKind::Solar,
            field: ModeEditField::Id,
        });
    }

    pub fn ask_delete_mode(&mut self) {
        if let Some(m) = self.selected_mode() {
            let id = m.config.id.clone();
            let name = m.config.name.clone();
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

//! Manage › Backup: backup zip, rule and scene export and import.
//!
//! Exports are written 0600 into a 0700 directory. Imports ask first.

use crate::app::editors::{Confirm, ConfirmAction};
use crate::app::{App, ListSel};
use crate::cache::write_private;
use crate::config::{exports_dir, imports_dir};
use anyhow::{Context, Result, anyhow};
use crossterm::event::{KeyCode, KeyEvent};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupAction {
    BackupZip,
    ExportRules,
    ExportScenes,
    ImportRules,
    ImportScenes,
}

impl BackupAction {
    pub const ALL: [BackupAction; 5] = [
        BackupAction::BackupZip,
        BackupAction::ExportRules,
        BackupAction::ExportScenes,
        BackupAction::ImportRules,
        BackupAction::ImportScenes,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::BackupZip => "backup_zip",
            Self::ExportRules => "export_rules",
            Self::ExportScenes => "export_scenes",
            Self::ImportRules => "import_rules",
            Self::ImportScenes => "import_scenes",
        }
    }

    pub fn label(self) -> String {
        match self {
            Self::BackupZip => "Download system backup (.zip)".into(),
            Self::ExportRules => "Export all rules to JSON".into(),
            Self::ExportScenes => "Export all scenes to JSON".into(),
            Self::ImportRules => format!(
                "Import rules from {}",
                imports_dir().join("rules.json").display()
            ),
            Self::ImportScenes => format!(
                "Import scenes from {}",
                imports_dir().join("scenes.json").display()
            ),
        }
    }
}

#[derive(Debug, Default)]
pub struct BackupPanel {
    pub sel: ListSel,
    pub busy: bool,
    pub status: String,
}

fn stamp() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

fn save_json(name: &str, value: &Value) -> Result<std::path::PathBuf> {
    let path = exports_dir().join(name);
    let pretty = serde_json::to_vec_pretty(value).context("serializing export")?;
    write_private(&path, &pretty).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

fn read_array(name: &str) -> Result<(std::path::PathBuf, Value)> {
    let path = imports_dir().join(name);
    let raw =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let value: Value = serde_json::from_str(&raw).context("parsing JSON")?;
    if !value.is_array() {
        return Err(anyhow!("{} must hold a JSON array", path.display()));
    }
    Ok((path, value))
}

impl App {
    pub fn selected_backup_action(&self) -> BackupAction {
        BackupAction::ALL
            .get(self.manage.backup.sel.index())
            .copied()
            .unwrap_or(BackupAction::BackupZip)
    }

    pub(crate) fn on_key_backup(&mut self, key: KeyEvent) {
        let ids: Vec<&str> = BackupAction::ALL.iter().map(|a| a.key()).collect();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.manage.backup.sel.move_by(1, &ids),
            KeyCode::Up | KeyCode::Char('k') => self.manage.backup.sel.move_by(-1, &ids),
            KeyCode::Enter => {
                let action = self.selected_backup_action();
                match action {
                    BackupAction::ImportRules => self.ask(
                        Confirm::new(
                            "Import rules",
                            "Import rules into homeCore?",
                            ConfirmAction::ImportRules,
                        )
                        .with_detail(action.label()),
                    ),
                    BackupAction::ImportScenes => self.ask(
                        Confirm::new(
                            "Import scenes",
                            "Import scenes into homeCore?",
                            ConfirmAction::ImportScenes,
                        )
                        .with_detail(action.label()),
                    ),
                    _ => self.run_backup_action(action),
                }
            }
            _ => {}
        }
    }

    pub(crate) fn run_backup_action(&mut self, action: BackupAction) {
        if self.manage.backup.busy {
            return;
        }
        self.manage.backup.busy = true;
        self.manage.backup.status = format!("Running: {}...", action.key());
        let c = self.client.clone();
        self.spawn(
            async move {
                match action {
                    BackupAction::BackupZip => {
                        let bytes = c.backup_zip().await?;
                        let path = exports_dir().join(format!("homecore-backup-{}.zip", stamp()));
                        write_private(&path, &bytes)
                            .with_context(|| format!("writing {}", path.display()))?;
                        Ok(format!(
                            "Saved {} ({:.1} KB)",
                            path.display(),
                            bytes.len() as f64 / 1024.0
                        ))
                    }
                    BackupAction::ExportRules => {
                        let v = c.export_rules().await?;
                        let n = v.as_array().map(Vec::len).unwrap_or(0);
                        let path = save_json(&format!("rules-{}.json", stamp()), &v)?;
                        Ok(format!("Exported {n} rule(s) to {}", path.display()))
                    }
                    BackupAction::ExportScenes => {
                        let v = c.export_scenes().await?;
                        let n = v.as_array().map(Vec::len).unwrap_or(0);
                        let path = save_json(&format!("scenes-{}.json", stamp()), &v)?;
                        Ok(format!("Exported {n} scene(s) to {}", path.display()))
                    }
                    BackupAction::ImportRules => {
                        let (path, v) = read_array("rules.json")?;
                        let n = c.import_rules(v).await?;
                        Ok(format!("Imported {n} rule(s) from {}", path.display()))
                    }
                    BackupAction::ImportScenes => {
                        let (path, v) = read_array("scenes.json")?;
                        let n = c.import_scenes(v).await?;
                        Ok(format!("Imported {n} scene(s) from {}", path.display()))
                    }
                }
            },
            move |app, res: Result<String>| {
                app.manage.backup.busy = false;
                app.manage.backup.status = match res {
                    Ok(msg) => msg,
                    Err(e) => format!("Error: {e}"),
                };
                if matches!(
                    action,
                    BackupAction::ImportRules | BackupAction::ImportScenes
                ) {
                    app.start_sync();
                }
            },
        );
    }
}

//! Data & backups: backups kept on the house (download, restore), making
//! one now, restoring from a file, automation and scene export and
//! import, and calendars.
//!
//! Downloads and exports are written 0600 into a 0700 directory. Imports,
//! restores and deletes ask first; a restore asks for the backup's date.

use super::settings_modal::{PathForm, PathPurpose, SettingsConfirm, SettingsModal};
use crate::app::editors::{Confirm, ConfirmAction};
use crate::app::{App, ListSel, Load};
use crate::cache::write_private;
use crate::config::{exports_dir, imports_dir};
use crate::model::settings::{Calendar, KeptBackup, KeptBackups, size};
use anyhow::{Context, Result, anyhow};
use serde_json::Value;
use std::path::PathBuf;

/// A path under the home folder written with `~`.
pub fn tilde(p: &std::path::Path) -> String {
    match std::env::var_os("HOME").map(PathBuf::from) {
        Some(h) if p.starts_with(&h) => format!("~/{}", p.strip_prefix(&h).unwrap_or(p).display()),
        _ => p.display().to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupAction {
    BackupZip,
    RestoreFile,
    ExportRules,
    ExportScenes,
    ImportRules,
    ImportScenes,
    AddCalendar,
}

impl BackupAction {
    pub fn key(self) -> &'static str {
        match self {
            Self::BackupZip => "backup_zip",
            Self::RestoreFile => "restore_file",
            Self::ExportRules => "export_rules",
            Self::ExportScenes => "export_scenes",
            Self::ImportRules => "import_rules",
            Self::ImportScenes => "import_scenes",
            Self::AddCalendar => "add_calendar",
        }
    }

    pub fn label(self) -> String {
        match self {
            Self::BackupZip => {
                format!("Make a backup now and save it in {}", tilde(&exports_dir()))
            }
            Self::RestoreFile => "Restore from a backup file on this computer".into(),
            Self::ExportRules => "Export all automations to JSON".into(),
            Self::ExportScenes => "Export all scenes to JSON".into(),
            Self::ImportRules => format!(
                "Import automations from {}",
                tilde(&imports_dir().join("rules.json"))
            ),
            Self::ImportScenes => format!(
                "Import scenes from {}",
                tilde(&imports_dir().join("scenes.json"))
            ),
            Self::AddCalendar => "Add a calendar from a .ics file".into(),
        }
    }

    /// What Enter does to it, for the footer.
    pub fn verb(self) -> &'static str {
        match self {
            Self::BackupZip => "back up now",
            Self::RestoreFile => "choose a file",
            Self::ExportRules | Self::ExportScenes => "export",
            Self::ImportRules | Self::ImportScenes => "import",
            Self::AddCalendar => "choose a file",
        }
    }
}

/// A row of the Data & backups list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataRow {
    Action(BackupAction),
    Kept(String),
    Calendar(String),
}

impl DataRow {
    pub fn key(&self) -> String {
        match self {
            Self::Action(a) => format!("a:{}", a.key()),
            Self::Kept(n) => format!("k:{n}"),
            Self::Calendar(c) => format!("c:{c}"),
        }
    }
}

/// The list's sections, in order, each with its heading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataSection {
    Backups,
    Kept,
    Transfer,
    Calendars,
}

#[derive(Debug, Default)]
pub struct BackupPanel {
    pub sel: ListSel,
    pub busy: bool,
    pub status: String,
    pub kept: Load<KeptBackups>,
    pub calendars: Load<Vec<Calendar>>,
}

fn stamp() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

fn save_json(name: &str, value: &Value) -> Result<PathBuf> {
    let path = exports_dir().join(name);
    let pretty = serde_json::to_vec_pretty(value).context("serializing export")?;
    write_private(&path, &pretty).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

fn read_array(name: &str) -> Result<(PathBuf, Value)> {
    let path = imports_dir().join(name);
    let raw =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let value: Value = serde_json::from_str(&raw).context("parsing JSON")?;
    if !value.is_array() {
        return Err(anyhow!("{} must hold a JSON array", path.display()));
    }
    Ok((path, value))
}

/// A kept backup's file name is core's; never let it leave the exports
/// folder.
fn safe_file_name(name: &str) -> Option<&str> {
    let ok = !name.is_empty()
        && !name.contains(['/', '\\'])
        && !name.contains("..")
        && !name.starts_with('.');
    ok.then_some(name)
}

impl App {
    /// The rows in order with the section each is in.
    pub fn data_rows(&self) -> Vec<(DataSection, DataRow)> {
        let admin = self.is_admin();
        let q = self.query(crate::screens::ScreenId::Data);
        let hit = |s: &str| crate::text::matches_query(q, &[s]);
        let mut out = Vec::new();
        if admin {
            for a in [BackupAction::BackupZip, BackupAction::RestoreFile] {
                out.push((DataSection::Backups, DataRow::Action(a)));
            }
            if let Some(k) = self.manage.backup.kept.ready() {
                for b in k.backups.iter().filter(|b| hit(&b.name)) {
                    out.push((DataSection::Kept, DataRow::Kept(b.name.clone())));
                }
            }
        }
        if self.can("automations:write") || admin {
            for a in [
                BackupAction::ExportRules,
                BackupAction::ExportScenes,
                BackupAction::ImportRules,
                BackupAction::ImportScenes,
            ] {
                out.push((DataSection::Transfer, DataRow::Action(a)));
            }
        }
        if self.can("automations:read") || admin {
            if let Some(cs) = self.manage.backup.calendars.ready() {
                for c in cs.iter().filter(|c| hit(&c.id)) {
                    out.push((DataSection::Calendars, DataRow::Calendar(c.id.clone())));
                }
            }
            if self.can("automations:write") || admin {
                out.push((
                    DataSection::Calendars,
                    DataRow::Action(BackupAction::AddCalendar),
                ));
            }
        }
        out
    }

    pub(crate) fn data_row_ids(&self) -> Vec<String> {
        self.data_rows().iter().map(|(_, r)| r.key()).collect()
    }

    pub fn selected_data_row(&self) -> Option<DataRow> {
        let id = self.manage.backup.sel.id()?;
        self.data_rows()
            .into_iter()
            .map(|(_, r)| r)
            .find(|r| r.key() == id)
    }

    pub fn kept_backup(&self, name: &str) -> Option<&KeptBackup> {
        self.manage
            .backup
            .kept
            .ready()?
            .backups
            .iter()
            .find(|b| b.name == name)
    }

    pub fn move_data(&mut self, delta: isize) {
        let ids = self.data_row_ids();
        self.manage.backup.sel.move_by(delta, &ids);
    }

    /// What the screen needs: the kept backups (admin) and calendars.
    pub(crate) fn load_data_screen(&mut self) {
        if self.is_admin() && !self.manage.backup.kept.is_loading() {
            if self.manage.backup.kept.ready().is_none() {
                self.manage.backup.kept = Load::Loading;
            }
            let c = self.client.clone();
            self.spawn(async move { c.list_kept_backups().await }, |app, res| {
                app.manage.backup.kept = match res {
                    Ok(v) => Load::Ready(v),
                    Err(e) => Load::Failed(e.to_string()),
                };
            });
        }
        self.load_calendars();
    }

    fn load_calendars(&mut self) {
        if !(self.can("automations:read") || self.is_admin())
            || self.manage.backup.calendars.is_loading()
        {
            return;
        }
        if self.manage.backup.calendars.ready().is_none() {
            self.manage.backup.calendars = Load::Loading;
        }
        let c = self.client.clone();
        self.spawn(async move { c.list_calendars().await }, |app, res| {
            app.manage.backup.calendars = match res {
                Ok(v) => Load::Ready(v),
                Err(e) => Load::Failed(e.to_string()),
            };
        });
    }

    /// Enter: download a kept backup, or run the action.
    pub fn data_enter(&mut self) {
        match self.selected_data_row() {
            Some(DataRow::Kept(name)) => self.download_kept(name),
            Some(DataRow::Action(a)) => self.data_action(a),
            Some(DataRow::Calendar(_)) | None => {}
        }
    }

    fn data_action(&mut self, action: BackupAction) {
        match action {
            BackupAction::ImportRules => self.ask(
                Confirm::new(
                    "Import automations?",
                    "Automations from the file will be added to homeCore.",
                    ConfirmAction::ImportRules,
                )
                .with_detail(action.label()),
            ),
            BackupAction::ImportScenes => self.ask(
                Confirm::new(
                    "Import scenes?",
                    "Scenes from the file will be added to homeCore.",
                    ConfirmAction::ImportScenes,
                )
                .with_detail(action.label()),
            ),
            BackupAction::RestoreFile => {
                self.manage.modal =
                    Some(SettingsModal::Path(PathForm::new(PathPurpose::RestoreFile)))
            }
            BackupAction::AddCalendar => {
                self.manage.modal = Some(SettingsModal::Path(PathForm::new(PathPurpose::Calendar)))
            }
            _ => self.run_backup_action(action),
        }
    }

    /// `R` on a kept backup.
    pub fn restore_selected_kept(&mut self) {
        let Some(DataRow::Kept(name)) = self.selected_data_row() else {
            return;
        };
        let date = self
            .kept_backup(&name)
            .map(KeptBackup::date)
            .unwrap_or_default();
        self.start_restore_kept(name, date);
    }

    fn download_kept(&mut self, name: String) {
        let Some(file) = safe_file_name(&name).map(str::to_string) else {
            self.error = Some(format!("Won't save a file named {name:?}"));
            return;
        };
        if self.manage.backup.busy {
            return;
        }
        self.manage.backup.busy = true;
        self.manage.backup.status = format!("Downloading {name}...");
        let c = self.client.clone();
        self.spawn(
            async move {
                let bytes = c.download_kept_backup(&name).await?;
                let path = exports_dir().join(&file);
                write_private(&path, &bytes)
                    .with_context(|| format!("writing {}", path.display()))?;
                Ok(format!(
                    "Saved {} ({}). Keep it somewhere other than this machine.",
                    path.display(),
                    size(bytes.len() as u64)
                ))
            },
            |app, res: Result<String>| {
                app.manage.backup.busy = false;
                app.manage.backup.status = match res {
                    Ok(m) => m,
                    Err(e) => format!("Error: {e}"),
                };
            },
        );
    }

    pub fn ask_delete_selected_calendar(&mut self) {
        let Some(DataRow::Calendar(id)) = self.selected_data_row() else {
            return;
        };
        self.ask(Confirm::new(
            format!("Delete the calendar \"{id}\"?"),
            "Rules that check it find nothing in it from now on.",
            ConfirmAction::Settings(SettingsConfirm::DeleteCalendar { id }),
        ));
    }

    pub(crate) fn delete_calendar(&mut self, id: String) {
        let c = self.client.clone();
        self.spawn_ok(
            "delete calendar",
            async move { c.delete_calendar(&id).await },
            |app, rules| {
                app.manage.backup.status = if rules.is_empty() {
                    "Calendar deleted.".to_string()
                } else {
                    format!(
                        "Calendar deleted. These rules still check it: {}.",
                        rules.join(", ")
                    )
                };
                app.load_calendars();
            },
        );
    }

    pub(crate) fn upload_calendar(&mut self, path: PathBuf, name: String) {
        let c = self.client.clone();
        self.spawn(
            async move {
                let text = tokio::fs::read_to_string(&path)
                    .await
                    .map_err(|e| anyhow!("reading {}: {e}", path.display()))?;
                c.upload_calendar(&name, &text).await
            },
            |app, res| match res {
                Ok((id, n)) => {
                    if matches!(app.manage.modal, Some(SettingsModal::Path(_))) {
                        app.manage.modal = None;
                    }
                    app.manage.backup.status = format!("Added \"{id}\": {n} events.");
                    app.load_calendars();
                }
                Err(e) => match app.manage.modal.as_mut() {
                    Some(SettingsModal::Path(f)) => {
                        f.busy = false;
                        f.error = Some(e.to_string());
                    }
                    _ => app.manage.backup.status = format!("Error: {e}"),
                },
            },
        );
    }

    pub(crate) fn run_backup_action(&mut self, action: BackupAction) {
        if self.manage.backup.busy {
            return;
        }
        self.manage.backup.busy = true;
        self.manage.backup.status = match action {
            BackupAction::BackupZip => {
                "Making the backup. This can take a minute with a lot of history...".to_string()
            }
            _ => format!("{}...", action.verb()),
        };
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
                            "Saved {} ({}). Keep it somewhere other than this machine.",
                            path.display(),
                            size(bytes.len() as u64)
                        ))
                    }
                    BackupAction::ExportRules => {
                        let v = c.export_rules().await?;
                        let n = v.as_array().map(Vec::len).unwrap_or(0);
                        let path = save_json(&format!("rules-{}.json", stamp()), &v)?;
                        Ok(format!("Exported {n} automation(s) to {}", path.display()))
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
                        Ok(format!(
                            "Imported {n} automation(s) from {}",
                            path.display()
                        ))
                    }
                    BackupAction::ImportScenes => {
                        let (path, v) = read_array("scenes.json")?;
                        let n = c.import_scenes(v).await?;
                        Ok(format!("Imported {n} scene(s) from {}", path.display()))
                    }
                    BackupAction::RestoreFile | BackupAction::AddCalendar => {
                        Err(anyhow!("not run in the background"))
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
                if action == BackupAction::BackupZip {
                    app.refresh_system_status();
                }
            },
        );
    }
}

/// For the key-table tests: `n` 1 selects a kept backup, 2 a calendar.
#[cfg(test)]
pub fn test_state(a: &mut App, n: usize) {
    a.manage.backup.kept = Load::Ready(KeptBackups {
        backups: vec![KeptBackup {
            name: "homecore-backup-20261117T000000Z.zip".into(),
            ..Default::default()
        }],
        ..Default::default()
    });
    a.manage.backup.calendars = Load::Ready(vec![Calendar {
        id: "bins".into(),
        ..Default::default()
    }]);
    match n {
        1 => a
            .manage
            .backup
            .sel
            .select_id("k:homecore-backup-20261117T000000Z.zip"),
        2 => a.manage.backup.sel.select_id("c:bins"),
        _ => a.manage.backup.sel.reset(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::*;
    use crate::model::Role;
    use crate::net::http::parse_body;

    fn with_kept(a: &mut App) {
        let k: KeptBackups = parse_body(
            "/system/backups",
            include_str!("../../../tests/fixtures/system_backups.json"),
        )
        .unwrap();
        a.manage.backup.kept = Load::Ready(k);
        a.manage.backup.calendars = Load::Ready(vec![Calendar {
            id: "bins".into(),
            ..Default::default()
        }]);
        a.refresh_views();
    }

    #[test]
    fn admins_see_backups_kept_and_calendars_in_order() {
        let mut a = signed_in(Role::Admin);
        with_kept(&mut a);
        let rows: Vec<String> = a.data_rows().iter().map(|(_, r)| r.key()).collect();
        assert_eq!(rows[0], "a:backup_zip");
        assert_eq!(rows[1], "a:restore_file");
        assert_eq!(rows[2], "k:homecore-backup-20261117T210247Z.zip");
        assert!(rows.contains(&"c:bins".to_string()));
        assert_eq!(rows.last().unwrap(), "a:add_calendar");
    }

    #[test]
    fn authors_get_export_and_calendars_but_no_backups() {
        let mut a = signed_in(Role::User);
        with_kept(&mut a);
        let rows: Vec<String> = a.data_rows().iter().map(|(_, r)| r.key()).collect();
        assert!(
            !rows
                .iter()
                .any(|r| r.starts_with("k:") || r == "a:backup_zip")
        );
        assert!(rows.contains(&"a:export_rules".to_string()));
        assert!(rows.contains(&"c:bins".to_string()));
    }

    #[tokio::test]
    async fn restore_on_a_kept_backup_asks_core_first() {
        let mut a = signed_in(Role::Admin);
        with_kept(&mut a);
        a.move_data(2);
        assert_eq!(
            a.selected_data_row(),
            Some(DataRow::Kept("homecore-backup-20261117T210247Z.zip".into()))
        );
        a.restore_selected_kept();
        // A check goes out; nothing opens until core answers.
        assert_eq!(a.pending, 1);
        assert!(a.manage.modal.is_none());
    }

    #[test]
    fn deleting_a_calendar_asks() {
        let mut a = signed_in(Role::Admin);
        with_kept(&mut a);
        let ids = a.data_row_ids();
        let i = ids.iter().position(|r| r == "c:bins").unwrap();
        a.move_data(i as isize);
        a.ask_delete_selected_calendar();
        assert!(matches!(
            a.confirm.as_ref().map(|c| &c.action),
            Some(ConfirmAction::Settings(
                SettingsConfirm::DeleteCalendar { .. }
            ))
        ));
    }

    #[test]
    fn kept_names_cannot_leave_the_exports_folder() {
        assert!(safe_file_name("homecore-backup-1.zip").is_some());
        for bad in ["../x.zip", "a/b.zip", ".hidden", "", "a\\b"] {
            assert!(safe_file_name(bad).is_none(), "{bad}");
        }
    }
}

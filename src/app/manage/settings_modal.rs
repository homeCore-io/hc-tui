//! The Settings screens' dialogs: a new API key, its secret shown once,
//! restoring a backup (type its date to go ahead), and reading a file
//! from this computer. Also the questions they ask through the shared
//! confirmation box.

use crate::app::editors::{Confirm, ConfirmAction, edit_text};
use crate::app::{App, Load};
use crate::model::settings::{NewApiKey, RestorePlan};
use crossterm::event::{KeyCode, KeyEvent};
use std::path::PathBuf;

/// A Settings question waiting for `y`.
#[derive(Debug, Clone)]
pub enum SettingsConfirm {
    RemovePin { id: String, username: String },
    RotateKey { id: String },
    RevokeKey { id: String, label: String },
    EndSession { id: String },
    EndOtherSessions,
    DeleteCalendar { id: String },
    ReplaceCalendar { path: PathBuf, name: String },
    RestartCore,
    DeleteDisplay { id: String, name: String },
}

/// An open Settings dialog. Only one at a time.
pub enum SettingsModal {
    NewKey(NewKeyForm),
    /// A key's secret, shown once. Dropping it zeroes the secret.
    Secret(NewApiKey),
    Restore(RestoreForm),
    Path(PathForm),
    /// Naming a new display.
    DisplayName(DisplayNameForm),
}

#[derive(Debug, Clone, Default)]
pub struct DisplayNameForm {
    pub name: String,
    pub busy: bool,
    pub error: Option<String>,
}

impl std::fmt::Debug for SettingsModal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NewKey(_) => f.write_str("NewKey"),
            Self::Secret(k) => write!(f, "Secret({k:?})"),
            Self::Restore(r) => write!(f, "Restore({:?})", r.source),
            Self::Path(p) => write!(f, "Path({:?})", p.purpose),
            Self::DisplayName(_) => f.write_str("DisplayName"),
        }
    }
}

// ── New API key ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewKeyField {
    Label,
    Scopes,
}

#[derive(Debug, Clone)]
pub struct NewKeyForm {
    pub label: String,
    /// Every scope I hold, and whether the key gets it.
    pub scopes: Vec<(String, bool)>,
    pub field: NewKeyField,
    pub cursor: usize,
    pub busy: bool,
    pub error: Option<String>,
}

impl NewKeyForm {
    /// Reading is ticked to start with; doing things is a choice.
    pub fn new(scopes: Vec<String>) -> Self {
        Self {
            label: String::new(),
            scopes: scopes
                .into_iter()
                .map(|s| {
                    let read = s.ends_with(":read");
                    (s, read)
                })
                .collect(),
            field: NewKeyField::Label,
            cursor: 0,
            busy: false,
            error: None,
        }
    }

    pub fn chosen(&self) -> Vec<String> {
        self.scopes
            .iter()
            .filter(|(_, on)| *on)
            .map(|(s, _)| s.clone())
            .collect()
    }
}

// ── Restore ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreSource {
    /// One of the backups kept on the house, by name.
    Kept(String),
    /// A zip on this computer.
    File(PathBuf),
}

impl RestoreSource {
    pub fn label(&self) -> String {
        match self {
            Self::Kept(n) => n.clone(),
            Self::File(p) => p.display().to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RestoreForm {
    pub source: RestoreSource,
    pub plan: RestorePlan,
    /// What has to be typed to go ahead: the backup's date.
    pub date: String,
    pub typed: String,
    pub busy: bool,
    pub error: Option<String>,
}

impl RestoreForm {
    pub fn new(source: RestoreSource, plan: RestorePlan, date: String) -> Self {
        Self {
            source,
            plan,
            date,
            typed: String::new(),
            busy: false,
            error: None,
        }
    }

    /// Whether what was typed is the backup's date.
    pub fn confirmed(&self) -> bool {
        !self.date.is_empty() && self.typed.trim() == self.date
    }
}

/// The date to type for a backup file: when it was taken, else the date
/// in its name, else the word `restore`.
pub fn restore_date(plan: &RestorePlan, file_name: &str) -> String {
    plan.taken_at
        .as_deref()
        .map(|t| crate::text::take_chars(t, 10))
        .filter(|d| d.len() == 10)
        .or_else(|| crate::model::settings::backup_date(file_name))
        .unwrap_or_else(|| "restore".to_string())
}

// ── A file on this computer ─────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathPurpose {
    RestoreFile,
    Calendar,
    /// A calendar homeCore downloads, from the address typed as the path.
    CalendarUrl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathField {
    Path,
    Name,
}

#[derive(Debug, Clone)]
pub struct PathForm {
    pub purpose: PathPurpose,
    pub path: String,
    /// The calendar's name; empty takes the file's.
    pub name: String,
    pub field: PathField,
    pub busy: bool,
    pub error: Option<String>,
}

impl PathForm {
    pub fn new(purpose: PathPurpose) -> Self {
        Self {
            purpose,
            path: String::new(),
            name: String::new(),
            field: PathField::Path,
            busy: false,
            error: None,
        }
    }
}

/// `~/x` to the home folder.
pub fn expand_path(p: &str) -> PathBuf {
    let p = p.trim();
    if let Some(rest) = p.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(p)
}

// ── Keys ────────────────────────────────────────────────────────────────────

impl App {
    /// The Settings dialog's keys. Called while one is open.
    pub(crate) fn on_key_settings_modal(&mut self, key: KeyEvent) {
        let Some(modal) = self.manage.modal.as_mut() else {
            return;
        };
        if key.code == KeyCode::Esc {
            let was_secret = matches!(modal, SettingsModal::Secret(_));
            self.manage.modal = None;
            self.status = if was_secret {
                "The secret is gone from here".to_string()
            } else {
                "Cancelled".to_string()
            };
            return;
        }
        match modal {
            SettingsModal::DisplayName(f) => {
                if f.busy {
                    return;
                }
                f.error = None;
                if key.code != KeyCode::Enter {
                    edit_text(&mut f.name, key);
                    return;
                }
                let name = f.name.trim().to_string();
                if name.is_empty() {
                    f.error = Some("Give it a name, such as Hall tablet".into());
                    return;
                }
                f.busy = true;
                let c = self.client.clone();
                self.spawn(
                    async move { c.create_display(&name).await },
                    |app, res| match res {
                        Ok(k) => {
                            app.status =
                                format!("Made the display {}: its key is shown once", k.label);
                            app.manage.modal = Some(SettingsModal::Secret(k));
                            app.manage.people.displays = Load::Idle;
                            app.load_displays();
                        }
                        Err(e) => {
                            if let Some(SettingsModal::DisplayName(f)) = app.manage.modal.as_mut() {
                                f.busy = false;
                                f.error = Some(e.to_string());
                            }
                        }
                    },
                );
            }
            SettingsModal::Secret(_) => {
                if key.code == KeyCode::Enter {
                    self.manage.modal = None;
                    self.status = "The secret is gone from here".to_string();
                }
            }
            SettingsModal::NewKey(f) => {
                if f.busy {
                    return;
                }
                f.error = None;
                match (key.code, f.field) {
                    (KeyCode::Tab | KeyCode::BackTab, _) => {
                        f.field = match f.field {
                            NewKeyField::Label => NewKeyField::Scopes,
                            NewKeyField::Scopes => NewKeyField::Label,
                        }
                    }
                    (KeyCode::Enter, _) => self.save_new_api_key(),
                    (KeyCode::Down, NewKeyField::Scopes) => {
                        f.cursor = (f.cursor + 1).min(f.scopes.len().saturating_sub(1))
                    }
                    (KeyCode::Up, NewKeyField::Scopes) => f.cursor = f.cursor.saturating_sub(1),
                    (KeyCode::Char(' '), NewKeyField::Scopes) => {
                        if let Some(s) = f.scopes.get_mut(f.cursor) {
                            s.1 = !s.1;
                        }
                    }
                    (KeyCode::Down, NewKeyField::Label) => f.field = NewKeyField::Scopes,
                    (_, NewKeyField::Label) => {
                        edit_text(&mut f.label, key);
                    }
                    _ => {}
                }
            }
            SettingsModal::Restore(f) => {
                if f.busy {
                    return;
                }
                f.error = None;
                if key.code == KeyCode::Enter {
                    if f.confirmed() {
                        self.stage_restore();
                    } else {
                        f.error = Some(format!("Type {} to restore", f.date));
                    }
                } else {
                    edit_text(&mut f.typed, key);
                }
            }
            SettingsModal::Path(f) => {
                if f.busy {
                    return;
                }
                f.error = None;
                match key.code {
                    KeyCode::Tab | KeyCode::BackTab
                        if matches!(
                            f.purpose,
                            PathPurpose::Calendar | PathPurpose::CalendarUrl
                        ) =>
                    {
                        f.field = match f.field {
                            PathField::Path => PathField::Name,
                            PathField::Name => PathField::Path,
                        }
                    }
                    KeyCode::Enter => self.submit_path_form(),
                    _ => {
                        let field = match f.field {
                            PathField::Path => &mut f.path,
                            PathField::Name => &mut f.name,
                        };
                        edit_text(field, key);
                    }
                }
            }
        }
    }

    pub(crate) fn run_settings_confirmed(&mut self, action: SettingsConfirm) {
        use SettingsConfirm as S;
        match action {
            S::RemovePin { id, username } => self.remove_pin(id, username),
            S::RotateKey { id } => self.rotate_api_key(id),
            S::RevokeKey { id, label } => self.revoke_api_key(id, label),
            S::EndSession { id } => self.end_sessions(vec![id]),
            S::EndOtherSessions => self.end_other_sessions(),
            S::DeleteCalendar { id } => self.delete_calendar(id),
            S::ReplaceCalendar { path, name } => self.upload_calendar(path, name),
            S::RestartCore => self.restart_core(),
            S::DeleteDisplay { id, name } => self.delete_display(id, name),
        }
    }

    /// Ask to restart homeCore, with what it costs.
    pub fn ask_restart_core(&mut self, why: Option<&str>) {
        let n = self.plugin_health().running;
        let plugins = if n > 0 {
            format!("Its {n} plugins restart; devices come back as they reconnect.")
        } else {
            "Its plugins restart; devices come back as they reconnect.".to_string()
        };
        let mut c = Confirm::new(
            "Restart homeCore?",
            format!(
                "{}Every screen in the house disconnects until it's back, and rules don't run.",
                why.map(|w| format!("{w} ")).unwrap_or_default()
            ),
            ConfirmAction::Settings(SettingsConfirm::RestartCore),
        );
        c = c.with_detail(plugins);
        self.ask(c);
    }

    // ── Restore ──────────────────────────────────────────────────────────

    /// `R` on a kept backup: ask core what it would bring, then ask for
    /// its date.
    pub fn start_restore_kept(&mut self, name: String, date: String) {
        self.status = "Reading the backup...".to_string();
        let c = self.client.clone();
        let n = name.clone();
        self.spawn_ok(
            "restore",
            async move { c.restore_kept_backup(&n, true).await },
            move |app, plan| {
                app.status = format!("Type {date} to restore {name}");
                app.manage.modal = Some(SettingsModal::Restore(RestoreForm::new(
                    RestoreSource::Kept(name),
                    plan,
                    date,
                )));
            },
        );
    }

    fn submit_path_form(&mut self) {
        let Some(SettingsModal::Path(f)) = self.manage.modal.as_mut() else {
            return;
        };
        if f.path.trim().is_empty() {
            f.error = Some(if f.purpose == PathPurpose::CalendarUrl {
                "Type the calendar's address".to_string()
            } else {
                "Type the file's path".to_string()
            });
            return;
        }
        if f.purpose == PathPurpose::CalendarUrl {
            let url = f.path.trim().to_string();
            let name = Some(f.name.trim().to_string()).filter(|n| !n.is_empty());
            f.busy = true;
            let c = self.client.clone();
            self.spawn(
                async move { c.fetch_calendar(&url, name.as_deref()).await },
                |app, res| match res {
                    Ok((id, n)) => {
                        app.manage.modal = None;
                        app.manage.backup.status = format!(
                            "Added \"{id}\": {n} events. homeCore fetches it again on its own."
                        );
                        app.load_calendars();
                    }
                    Err(e) => {
                        if let Some(SettingsModal::Path(f)) = app.manage.modal.as_mut() {
                            f.busy = false;
                            f.error = Some(e.to_string());
                        }
                    }
                },
            );
            return;
        }
        let path = expand_path(&f.path);
        match f.purpose {
            PathPurpose::RestoreFile => {
                f.busy = true;
                let c = self.client.clone();
                let p = path.clone();
                self.spawn(
                    async move {
                        let zip = tokio::fs::read(&p)
                            .await
                            .map_err(|e| anyhow::anyhow!("reading {}: {e}", p.display()))?;
                        c.restore_backup_file(zip, true).await
                    },
                    move |app, res| match res {
                        Ok(plan) => {
                            let file = path
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_default();
                            let date = restore_date(&plan, &file);
                            app.status = format!("Type {date} to restore it");
                            app.manage.modal = Some(SettingsModal::Restore(RestoreForm::new(
                                RestoreSource::File(path),
                                plan,
                                date,
                            )));
                        }
                        Err(e) => {
                            if let Some(SettingsModal::Path(f)) = app.manage.modal.as_mut() {
                                f.busy = false;
                                f.error = Some(e.to_string());
                            }
                        }
                    },
                );
            }
            PathPurpose::Calendar => {
                let name = if f.name.trim().is_empty() {
                    path.file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default()
                } else {
                    f.name.trim().to_string()
                };
                let name = crate::model::settings::calendar_name(&name);
                if name.is_empty() {
                    f.error = Some("Give the calendar a name".to_string());
                    return;
                }
                let exists = self
                    .manage
                    .backup
                    .calendars
                    .ready()
                    .is_some_and(|cs| cs.iter().any(|c| c.id == name));
                if exists {
                    self.manage.modal = None;
                    self.ask(Confirm::new(
                        format!("Replace the calendar \"{name}\"?"),
                        "There's one by that name already. Its events are replaced by these; rules that check it check the new one.",
                        ConfirmAction::Settings(SettingsConfirm::ReplaceCalendar { path, name }),
                    ));
                } else {
                    f.busy = true;
                    self.upload_calendar(path, name);
                }
            }
            // Sent above, before a path is made of it.
            PathPurpose::CalendarUrl => {}
        }
    }

    /// Typed the date: hand the backup to core for the next start, then
    /// offer the restart that puts it in place.
    fn stage_restore(&mut self) {
        let Some(SettingsModal::Restore(f)) = self.manage.modal.as_mut() else {
            return;
        };
        f.busy = true;
        let source = f.source.clone();
        let c = self.client.clone();
        self.spawn(
            async move {
                match source {
                    RestoreSource::Kept(name) => c.restore_kept_backup(&name, false).await,
                    RestoreSource::File(p) => {
                        let zip = tokio::fs::read(&p)
                            .await
                            .map_err(|e| anyhow::anyhow!("reading {}: {e}", p.display()))?;
                        c.restore_backup_file(zip, false).await
                    }
                }
            },
            |app, res| match res {
                Ok(_) => {
                    app.manage.modal = None;
                    app.manage.backup.status =
                        "Staged. homeCore puts the backup in place when it next starts.".into();
                    app.status = "Backup staged".to_string();
                    app.ask_restart_core(Some("The backup goes in place when it starts again."));
                }
                Err(e) => {
                    if let Some(SettingsModal::Restore(f)) = app.manage.modal.as_mut() {
                        f.busy = false;
                        f.error = Some(e.to_string());
                    }
                }
            },
        );
    }

    // ── Restart ──────────────────────────────────────────────────────────

    pub(crate) fn restart_core(&mut self) {
        if self.manage.status.restarting.is_some() {
            return;
        }
        self.manage.status.restarting = Some("Restarting homeCore...".to_string());
        self.status = "Restarting homeCore...".to_string();
        let c = self.client.clone();
        self.spawn(async move { c.restart_and_wait().await }, |app, res| {
            app.manage.status.restarting = None;
            match res {
                Ok(secs) => {
                    app.status = format!("homeCore is back after {secs} seconds");
                    app.manage.status.health = Load::Idle;
                    app.refresh_system_status();
                    app.start_sync();
                }
                Err(e) => app.error = Some(format!("restart: {e}")),
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::*;
    use crate::model::Role;

    fn restore_open(a: &mut App, date: &str) {
        a.manage.modal = Some(SettingsModal::Restore(RestoreForm::new(
            RestoreSource::Kept("homecore-backup-20261117T210247Z.zip".into()),
            RestorePlan::default(),
            date.into(),
        )));
    }

    #[test]
    fn restore_goes_ahead_only_on_the_typed_date() {
        let mut a = signed_in(Role::Admin);
        restore_open(&mut a, "2026-11-17");
        // Enter with nothing typed: refused, still open, nothing sent.
        a.on_key(key(KeyCode::Enter));
        let Some(SettingsModal::Restore(f)) = &a.manage.modal else {
            panic!("closed")
        };
        assert!(!f.busy && f.error.as_deref() == Some("Type 2026-11-17 to restore"));
        // y is just a letter here, not a yes.
        a.on_key(key(KeyCode::Char('y')));
        a.on_key(key(KeyCode::Enter));
        let Some(SettingsModal::Restore(f)) = &a.manage.modal else {
            panic!("closed")
        };
        assert!(!f.busy && a.pending == 0);
        // A near miss is refused too.
        for c in "\u{8}2026-11-18".chars() {
            let code = if c == '\u{8}' {
                KeyCode::Backspace
            } else {
                KeyCode::Char(c)
            };
            a.on_key(key(code));
        }
        let Some(SettingsModal::Restore(f)) = &a.manage.modal else {
            panic!("closed")
        };
        assert_eq!(f.typed, "2026-11-18");
        assert!(!f.confirmed());
        a.on_key(key(KeyCode::Esc));
        assert!(a.manage.modal.is_none());
    }

    #[tokio::test]
    async fn the_right_date_stages_the_restore() {
        let mut a = signed_in(Role::Admin);
        restore_open(&mut a, "2026-11-17");
        for c in "2026-11-17".chars() {
            a.on_key(key(KeyCode::Char(c)));
        }
        a.on_key(key(KeyCode::Enter));
        let Some(SettingsModal::Restore(f)) = &a.manage.modal else {
            panic!("closed")
        };
        assert!(f.busy, "sent to core");
        assert_eq!(a.pending, 1);
    }

    #[test]
    fn the_date_to_type_comes_from_the_backup() {
        let plan = RestorePlan {
            taken_at: Some("2026-11-16T03:00:00Z".into()),
            ..Default::default()
        };
        assert_eq!(restore_date(&plan, "x.zip"), "2026-11-16");
        let none = RestorePlan::default();
        assert_eq!(
            restore_date(&none, "homecore-backup-20261117T210247Z.zip"),
            "2026-11-17"
        );
        assert_eq!(restore_date(&none, "mine.zip"), "restore");
        assert!(
            !RestoreForm::new(RestoreSource::Kept("x".into()), none, String::new()).confirmed()
        );
    }

    #[test]
    fn the_secret_is_shown_once_and_gone_after() {
        let mut a = signed_in(Role::Admin);
        let key_json = serde_json::json!({
            "id": "k1", "label": "script", "token": "hc_sk_topsecret123"
        });
        let k: NewApiKey = serde_json::from_value(key_json).unwrap();
        a.manage.modal = Some(SettingsModal::Secret(k));
        // Nothing but Enter or Esc closes it, and nothing echoes it.
        a.on_key(key(KeyCode::Char('x')));
        assert!(matches!(a.manage.modal, Some(SettingsModal::Secret(_))));
        assert!(!a.status.contains("topsecret"));
        assert!(!format!("{:?}", a.manage.modal).contains("topsecret"));
        a.on_key(key(KeyCode::Enter));
        assert!(a.manage.modal.is_none());
        assert!(!a.status.contains("topsecret"));
    }

    #[test]
    fn new_key_form_types_its_label_and_ticks_scopes() {
        let mut a = signed_in(Role::Admin);
        a.manage.modal = Some(SettingsModal::NewKey(NewKeyForm::new(vec![
            "devices:read".into(),
            "devices:command".into(),
        ])));
        for c in "nas".chars() {
            a.on_key(key(KeyCode::Char(c)));
        }
        a.on_key(key(KeyCode::Tab));
        a.on_key(key(KeyCode::Down));
        a.on_key(key(KeyCode::Char(' ')));
        let Some(SettingsModal::NewKey(f)) = &a.manage.modal else {
            panic!()
        };
        assert_eq!(f.label, "nas");
        assert_eq!(f.chosen(), vec!["devices:read", "devices:command"]);
    }

    #[test]
    fn paths_expand_home() {
        if let Some(home) = std::env::var_os("HOME") {
            assert_eq!(expand_path(" ~/b.zip "), PathBuf::from(home).join("b.zip"));
        }
        assert_eq!(expand_path("/x/y.zip"), PathBuf::from("/x/y.zip"));
    }
}

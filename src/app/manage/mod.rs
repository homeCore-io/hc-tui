//! Manage tab: modes, Matter, system status, users, logs, events, audit
//! and backup sub-panels.

pub mod audit;
pub mod backup;
pub mod logs;
pub mod matter;
pub mod modes;
pub mod status;
pub mod users;

use super::editors::{GlueCreator, ModeEditor, UserEditor};
use super::{App, ListSel};
use crossterm::event::{KeyCode, KeyEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AdminSubPanel {
    #[default]
    Modes,
    Matter,
    Status,
    Users,
    Logs,
    Events,
    Audit,
    Backup,
}

impl AdminSubPanel {
    pub const ALL: [AdminSubPanel; 8] = [
        AdminSubPanel::Modes,
        AdminSubPanel::Matter,
        AdminSubPanel::Status,
        AdminSubPanel::Users,
        AdminSubPanel::Logs,
        AdminSubPanel::Events,
        AdminSubPanel::Audit,
        AdminSubPanel::Backup,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Modes => "Modes",
            Self::Matter => "Matter",
            Self::Status => "Status",
            Self::Users => "Users",
            Self::Logs => "Logs",
            Self::Events => "Events",
            Self::Audit => "Audit",
            Self::Backup => "Backup",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|p| *p == self).unwrap_or(0)
    }

    fn step(self, forward: bool) -> Self {
        let n = Self::ALL.len();
        let i = self.index();
        Self::ALL[if forward {
            (i + 1) % n
        } else {
            (i + n - 1) % n
        }]
    }
}

#[derive(Debug, Default)]
pub struct EventsPanel {
    /// Show only this event type; `None` shows all.
    pub type_filter: Option<String>,
    pub sel: ListSel,
}

#[derive(Debug, Default)]
pub struct ManageTab {
    pub sub: AdminSubPanel,
    pub modes_sel: ListSel,
    pub mode_editor: Option<ModeEditor>,
    pub users_sel: ListSel,
    pub user_editor: Option<UserEditor>,
    pub glue_creator: Option<GlueCreator>,
    pub matter: matter::MatterPanel,
    pub status: status::StatusPanel,
    pub logs: logs::LogsPanel,
    pub events: EventsPanel,
    pub audit: audit::AuditPanel,
    pub backup: backup::BackupPanel,
}

impl App {
    /// Load whatever the newly shown panel needs.
    pub(crate) fn on_enter_manage_panel(&mut self) {
        match self.manage.sub {
            AdminSubPanel::Matter => self.refresh_matter_nodes(),
            AdminSubPanel::Status => self.refresh_system_status(),
            AdminSubPanel::Audit => self.refresh_audit(),
            _ => {}
        }
    }

    /// `r` on the Manage tab.
    pub(crate) fn refresh_manage_panel(&mut self) {
        match self.manage.sub {
            AdminSubPanel::Matter => self.refresh_matter_nodes(),
            AdminSubPanel::Status => self.refresh_system_status(),
            AdminSubPanel::Audit => self.refresh_audit(),
            _ => {
                self.status = "Refreshing...".to_string();
                self.start_sync();
            }
        }
    }

    pub(crate) fn resolve_manage_selections(&mut self) {
        let ids: Vec<String> = self.modes.iter().map(|m| m.config.id.clone()).collect();
        self.manage.modes_sel.resolve(&ids);
        let ids: Vec<String> = self.users.iter().map(|u| u.id.clone()).collect();
        self.manage.users_sel.resolve(&ids);
        let ids: Vec<String> = self
            .matter_nodes
            .iter()
            .map(|n| n.node_id.clone())
            .collect();
        self.manage.matter.sel.resolve(&ids);
        let ids: Vec<String> = self.audit_ids();
        self.manage.audit.sel.resolve(&ids);
        let ids = self.event_row_ids();
        self.manage.events.sel.resolve(&ids);
        let ids: Vec<&str> = backup::BackupAction::ALL.iter().map(|a| a.key()).collect();
        self.manage.backup.sel.resolve(&ids);
        self.resolve_group_selection();
    }

    /// Events have no server id; rows are keyed by the local number each
    /// got when added, so the cursor stays on an event as new ones arrive.
    pub(crate) fn event_row_ids(&self) -> Vec<String> {
        self.views
            .events
            .iter()
            .filter_map(|&i| self.events.get(i).map(|e| e.seq.to_string()))
            .collect()
    }

    pub(crate) fn on_key_manage(&mut self, key: KeyEvent) {
        let sub = self.manage.sub;
        match key.code {
            KeyCode::Left | KeyCode::Right => {
                self.manage.sub = sub.step(key.code == KeyCode::Right);
                self.on_enter_manage_panel();
                return;
            }
            KeyCode::Char('g') | KeyCode::Char('G') => {
                self.manage.glue_creator = Some(GlueCreator::default());
                self.status = "Open helper creator".to_string();
                return;
            }
            _ => {}
        }
        match sub {
            AdminSubPanel::Modes => self.on_key_modes(key),
            AdminSubPanel::Matter => self.on_key_matter(key),
            AdminSubPanel::Status => {}
            AdminSubPanel::Users => self.on_key_users(key),
            AdminSubPanel::Logs => self.on_key_logs(key),
            AdminSubPanel::Events => {
                let ids = self.event_row_ids();
                match key.code {
                    KeyCode::Down | KeyCode::Char('j') => self.manage.events.sel.move_by(1, &ids),
                    KeyCode::Up | KeyCode::Char('k') => self.manage.events.sel.move_by(-1, &ids),
                    KeyCode::Char('f') => self.cycle_event_filter(),
                    _ => {}
                }
            }
            AdminSubPanel::Audit => self.on_key_audit(key),
            AdminSubPanel::Backup => self.on_key_backup(key),
        }
    }

    pub(crate) fn save_glue_creator(&mut self) {
        let Some(creator) = self.manage.glue_creator.as_mut() else {
            return;
        };
        let (id, name, ty, config) = match creator.build() {
            Ok(v) => v,
            Err(e) => {
                creator.error = Some(e.clone());
                self.error = Some(e);
                return;
            }
        };
        let c = self.client.clone();
        self.spawn(
            async move { c.create_glue(&id, &name, ty, config).await },
            move |app, res| match res {
                Ok(dev) => {
                    app.status = format!("Created {ty} {}", dev.device_id);
                    app.devices.push(dev);
                    app.manage.glue_creator = None;
                }
                Err(e) => {
                    if let Some(c) = app.manage.glue_creator.as_mut() {
                        c.error = Some(e.to_string());
                    }
                    app.error = Some(e.to_string());
                }
            },
        );
    }
}

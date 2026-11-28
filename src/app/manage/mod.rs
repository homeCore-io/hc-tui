//! State for the Settings screens (People, Data & backups, Logs,
//! Events, Audit, System) plus Modes and Plugins › Matter, which grew
//! up here.

pub mod audit;
pub mod backup;
pub mod logs;
pub mod matter;
pub mod modes;
pub mod people;
pub mod rotations;
pub mod settings_modal;
pub mod status;
pub mod users;

use super::editors::{ModeEditor, UserEditor};
use super::{App, ListSel};

#[derive(Debug, Default)]
pub struct EventsPanel {
    /// Show only this event type; `None` shows all.
    pub type_filter: Option<String>,
    pub sel: ListSel,
}

#[derive(Debug, Default)]
pub struct ManageTab {
    pub modes_sel: ListSel,
    pub mode_editor: Option<ModeEditor>,
    /// The criteria mode whose definition is open (`d`), by id.
    pub mode_detail: Option<String>,
    pub users_sel: ListSel,
    pub user_editor: Option<UserEditor>,
    pub matter: matter::MatterPanel,
    pub status: status::StatusPanel,
    pub logs: logs::LogsPanel,
    pub events: EventsPanel,
    pub audit: audit::AuditPanel,
    pub backup: backup::BackupPanel,
    pub people: people::PeoplePanel,
    pub rotations: rotations::RotationsPanel,
    /// The open Settings dialog, if any.
    pub modal: Option<settings_modal::SettingsModal>,
}

impl App {
    pub(crate) fn resolve_manage_selections(&mut self) {
        let ids = self.mode_view_ids();
        self.manage.modes_sel.resolve(&ids);
        let ids = self.user_view_ids();
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
        let ids = self.data_row_ids();
        self.manage.backup.sel.resolve(&ids);
        self.resolve_people_selections();
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

    pub fn move_event(&mut self, delta: isize) {
        let ids = self.event_row_ids();
        self.manage.events.sel.move_by(delta, &ids);
    }
}

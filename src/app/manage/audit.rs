//! Manage › Audit: paged, read-only `GET /audit`.

use crate::app::{App, ListSel};
use crate::model::AuditEntry;
use crossterm::event::{KeyCode, KeyEvent};

/// What the Audit list shows, chosen with `f`; homeCore does the filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuditFilter {
    #[default]
    All,
    SignIns,
    Changes,
    Refused,
    Failed,
}

impl AuditFilter {
    pub const ALL: [AuditFilter; 5] = [
        Self::All,
        Self::SignIns,
        Self::Changes,
        Self::Refused,
        Self::Failed,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "everything",
            Self::SignIns => "sign-ins",
            Self::Changes => "changes",
            Self::Refused => "refused",
            Self::Failed => "failed",
        }
    }

    /// The query pairs that ask homeCore for it.
    pub fn query(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::All => &[],
            Self::SignIns => &[("event_type", "auth.*")],
            Self::Changes => &[("event_type", "!auth.*")],
            Self::Refused => &[("result", "denied")],
            Self::Failed => &[("result", "error")],
        }
    }
}

#[derive(Debug)]
pub struct AuditPanel {
    pub entries: Vec<AuditEntry>,
    pub filter: AuditFilter,
    pub loading: bool,
    pub error: Option<String>,
    pub offset: u32,
    pub limit: u32,
    pub sel: ListSel,
    /// Key of the row whose detail is expanded.
    pub expanded: Option<String>,
}

impl Default for AuditPanel {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            loading: false,
            error: None,
            offset: 0,
            limit: 100,
            sel: ListSel::default(),
            expanded: None,
            filter: AuditFilter::All,
        }
    }
}

impl App {
    /// The page's entries that match the `/` search.
    pub fn visible_audit(&self) -> Vec<&AuditEntry> {
        let q = self.query(crate::screens::ScreenId::Audit);
        self.manage
            .audit
            .entries
            .iter()
            .filter(|e| {
                crate::text::matches_query(
                    q,
                    &[
                        &e.actor_label,
                        &e.event_type,
                        e.result.as_str(),
                        e.target_kind.as_deref().unwrap_or(""),
                        e.target_id.as_deref().unwrap_or(""),
                    ],
                )
            })
            .collect()
    }

    pub(crate) fn audit_ids(&self) -> Vec<String> {
        self.visible_audit()
            .into_iter()
            .map(AuditEntry::key)
            .collect()
    }

    pub fn expanded_audit(&self) -> Option<&AuditEntry> {
        let key = self.manage.audit.expanded.as_deref()?;
        self.manage.audit.entries.iter().find(|e| e.key() == key)
    }

    /// `f`: the next filter, from the first page.
    pub fn step_audit_filter(&mut self) {
        let all = AuditFilter::ALL;
        let i = all
            .iter()
            .position(|f| *f == self.manage.audit.filter)
            .unwrap_or(0);
        self.manage.audit.filter = all[(i + 1) % all.len()];
        self.manage.audit.offset = 0;
        self.refresh_audit();
    }

    pub(crate) fn refresh_audit(&mut self) {
        let a = &mut self.manage.audit;
        a.loading = true;
        a.error = None;
        a.expanded = None;
        let (limit, offset, filter) = (a.limit, a.offset, a.filter);
        let c = self.client.clone();
        self.spawn(
            async move { c.list_audit(limit, offset, filter.query()).await },
            move |app, res| {
                let a = &mut app.manage.audit;
                a.loading = false;
                match res {
                    Ok(rows) => {
                        a.entries = rows;
                        a.sel.reset();
                        app.status = format!(
                            "Audit page {} ({} entries, {})",
                            offset / limit.max(1) + 1,
                            app.manage.audit.entries.len(),
                            filter.label()
                        );
                    }
                    Err(e) => {
                        a.error = Some(e.to_string());
                        a.entries.clear();
                    }
                }
            },
        );
    }

    pub(crate) fn on_key_audit(&mut self, key: KeyEvent) {
        let ids = self.audit_ids();
        let a = &mut self.manage.audit;
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => a.sel.move_by(1, &ids),
            KeyCode::Up | KeyCode::Char('k') => a.sel.move_by(-1, &ids),
            KeyCode::Enter => {
                let cur = a.sel.id().map(str::to_string);
                a.expanded = if a.expanded == cur { None } else { cur };
            }
            // Only a full page can have a next page.
            KeyCode::Char(']') if a.entries.len() >= a.limit as usize => {
                a.offset = a.offset.saturating_add(a.limit);
                self.refresh_audit();
            }
            KeyCode::Char('[') if a.offset > 0 => {
                a.offset = a.offset.saturating_sub(a.limit);
                self.refresh_audit();
            }
            _ => {}
        }
    }
}

//! Manage › Audit: paged, read-only `GET /audit`.

use crate::app::{App, ListSel};
use crate::model::AuditEntry;
use crossterm::event::{KeyCode, KeyEvent};

#[derive(Debug)]
pub struct AuditPanel {
    pub entries: Vec<AuditEntry>,
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
        }
    }
}

impl App {
    pub(crate) fn audit_ids(&self) -> Vec<String> {
        self.manage
            .audit
            .entries
            .iter()
            .map(AuditEntry::key)
            .collect()
    }

    pub fn expanded_audit(&self) -> Option<&AuditEntry> {
        let key = self.manage.audit.expanded.as_deref()?;
        self.manage.audit.entries.iter().find(|e| e.key() == key)
    }

    pub(crate) fn refresh_audit(&mut self) {
        let a = &mut self.manage.audit;
        a.loading = true;
        a.error = None;
        a.expanded = None;
        let (limit, offset) = (a.limit, a.offset);
        let c = self.client.clone();
        self.spawn(
            async move { c.list_audit(limit, offset).await },
            move |app, res| {
                let a = &mut app.manage.audit;
                a.loading = false;
                match res {
                    Ok(rows) => {
                        a.entries = rows;
                        a.sel.reset();
                        app.status = format!(
                            "Audit page {} ({} entries)",
                            offset / limit.max(1) + 1,
                            app.manage.audit.entries.len()
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

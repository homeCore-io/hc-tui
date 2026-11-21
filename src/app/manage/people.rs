//! People's other tabs: Roles (read only), API keys and Sessions, plus
//! the admin's actions on a person (a new password, removing a PIN).
//!
//! A new key's secret goes straight from the response into the
//! [`SettingsModal::Secret`] dialog and nowhere else: not the status
//! line, not the cache. Closing the dialog drops it, which zeroes it.

use super::settings_modal::{NewKeyForm, SettingsConfirm, SettingsModal};
use crate::app::editors::{Confirm, ConfirmAction};
use crate::app::{App, ListSel, Load};
use crate::model::settings::{ApiKey, NewApiKey, RoleScopes, SessionInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PeopleTab {
    #[default]
    People,
    Roles,
    ApiKeys,
    Sessions,
    /// Screens for the house: their own keys, no person's.
    Displays,
}

impl PeopleTab {
    pub const ALL: [PeopleTab; 5] = [
        PeopleTab::People,
        PeopleTab::Roles,
        PeopleTab::ApiKeys,
        PeopleTab::Sessions,
        PeopleTab::Displays,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::People => "People",
            Self::Roles => "Roles",
            Self::ApiKeys => "API keys",
            Self::Sessions => "Sessions",
            Self::Displays => "Displays",
        }
    }
}

#[derive(Debug, Default)]
pub struct PeoplePanel {
    pub tab: PeopleTab,
    pub roles: Load<Vec<RoleScopes>>,
    pub roles_sel: ListSel,
    pub keys: Load<Vec<ApiKey>>,
    pub keys_sel: ListSel,
    pub sessions: Load<Vec<SessionInfo>>,
    pub sessions_sel: ListSel,
    pub displays: Load<Vec<crate::model::settings::Display>>,
    pub displays_sel: ListSel,
}

/// Keys in use first, then stopped ones; newest first within each.
pub fn key_order(keys: &[ApiKey]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..keys.len()).collect();
    idx.sort_by(|&a, &b| {
        let (ka, kb) = (&keys[a], &keys[b]);
        ka.is_revoked()
            .cmp(&kb.is_revoked())
            .then_with(|| kb.created_at.cmp(&ka.created_at))
    });
    idx
}

impl App {
    // ── Tabs ─────────────────────────────────────────────────────────────

    pub fn people_step_tab(&mut self, delta: isize) {
        let all = PeopleTab::ALL;
        let n = all.len() as isize;
        let i = all
            .iter()
            .position(|t| *t == self.manage.people.tab)
            .unwrap_or(0) as isize;
        self.manage.people.tab = all[((i + delta) % n + n) as usize % all.len()];
        self.load_people_tab();
    }

    /// Load what the tab on view shows.
    pub(crate) fn load_people_tab(&mut self) {
        match self.manage.people.tab {
            PeopleTab::People => {}
            PeopleTab::Roles => self.load_roles(),
            PeopleTab::ApiKeys => self.load_api_keys(),
            PeopleTab::Sessions => self.load_sessions(),
            PeopleTab::Displays => self.load_displays(),
        }
    }

    pub(crate) fn resolve_people_selections(&mut self) {
        let ids = self.role_ids();
        self.manage.people.roles_sel.resolve(&ids);
        let ids = self.api_key_ids();
        self.manage.people.keys_sel.resolve(&ids);
        let ids = self.session_ids();
        self.manage.people.sessions_sel.resolve(&ids);
    }

    // ── Roles ────────────────────────────────────────────────────────────

    fn load_roles(&mut self) {
        if self.manage.people.roles.is_loading() {
            return;
        }
        self.manage.people.roles = Load::Loading;
        let c = self.client.clone();
        self.spawn(async move { c.list_roles().await }, |app, res| {
            app.manage.people.roles = match res {
                Ok(v) => Load::Ready(v),
                Err(e) => Load::Failed(e.to_string()),
            };
            // A new-key form opened before the role table came fills now.
            let scopes = app.grantable_scopes();
            if let Some(SettingsModal::NewKey(f)) = app.manage.modal.as_mut()
                && f.scopes.is_empty()
            {
                f.scopes = NewKeyForm::new(scopes).scopes;
            }
        });
    }

    fn role_ids(&self) -> Vec<String> {
        self.manage
            .people
            .roles
            .ready()
            .map(|v| v.iter().map(|r| r.role.clone()).collect())
            .unwrap_or_default()
    }

    pub fn move_role(&mut self, delta: isize) {
        let ids = self.role_ids();
        self.manage.people.roles_sel.move_by(delta, &ids);
    }

    // ── API keys ─────────────────────────────────────────────────────────

    pub(crate) fn load_api_keys(&mut self) {
        if self.manage.people.keys.is_loading() {
            return;
        }
        if self.manage.people.keys.ready().is_none() {
            self.manage.people.keys = Load::Loading;
        }
        let c = self.client.clone();
        self.spawn(async move { c.list_api_keys().await }, |app, res| {
            app.manage.people.keys = match res {
                Ok(v) => Load::Ready(v),
                Err(e) => Load::Failed(e.to_string()),
            };
        });
    }

    /// The keys in display order.
    pub fn visible_api_keys(&self) -> Vec<&ApiKey> {
        let Some(keys) = self.manage.people.keys.ready() else {
            return Vec::new();
        };
        let q = self.query(crate::screens::ScreenId::People);
        key_order(keys)
            .into_iter()
            .map(|i| &keys[i])
            .filter(|k| crate::text::matches_query(q, &[&k.label, &k.scopes.join(" ")]))
            .collect()
    }

    fn api_key_ids(&self) -> Vec<String> {
        self.visible_api_keys()
            .iter()
            .map(|k| k.id.clone())
            .collect()
    }

    pub fn selected_api_key(&self) -> Option<&ApiKey> {
        let id = self.manage.people.keys_sel.id()?;
        self.visible_api_keys().into_iter().find(|k| k.id == id)
    }

    pub fn move_api_key(&mut self, delta: isize) {
        let ids = self.api_key_ids();
        self.manage.people.keys_sel.move_by(delta, &ids);
    }

    /// The scopes a new key may be given: what I hold.
    pub fn grantable_scopes(&self) -> Vec<String> {
        let Some(me) = self.current_user.as_ref() else {
            return Vec::new();
        };
        let mut scopes = match &me.scopes {
            Some(s) => s.clone(),
            None => self
                .manage
                .people
                .roles
                .ready()
                .and_then(|r| r.iter().find(|r| r.role == me.role.as_str()))
                .map(|r| r.scopes.clone())
                .unwrap_or_default(),
        };
        scopes.sort();
        scopes.dedup();
        scopes
    }

    pub fn open_new_api_key(&mut self) {
        let scopes = self.grantable_scopes();
        if scopes.is_empty() {
            // `/auth/me` gave no scopes; the role table has them.
            self.load_roles();
        }
        self.manage.modal = Some(SettingsModal::NewKey(NewKeyForm::new(scopes)));
    }

    /// `e` on a key: change its label or what it may do. Its secret stays.
    pub fn open_edit_api_key(&mut self) {
        let Some(k) = self.selected_api_key().cloned() else {
            return;
        };
        if k.is_revoked() {
            self.status = "A revoked key can't be changed".into();
            return;
        }
        let mut held = self.grantable_scopes();
        for s in &k.scopes {
            if !held.contains(s) {
                held.push(s.clone());
            }
        }
        let mut form = NewKeyForm::new(held);
        for (s, on) in form.scopes.iter_mut() {
            *on = k.scopes.contains(s);
        }
        form.label = k.label.clone();
        form.editing = Some(k.id.clone());
        self.manage.modal = Some(SettingsModal::NewKey(form));
    }

    pub(crate) fn save_new_api_key(&mut self) {
        let Some(SettingsModal::NewKey(form)) = self.manage.modal.as_mut() else {
            return;
        };
        let label = form.label.trim().to_string();
        if label.is_empty() {
            form.error = Some("Say what the key is for".to_string());
            return;
        }
        let scopes = form.chosen();
        if scopes.is_empty() {
            form.error = Some("Pick at least one thing it may do".to_string());
            return;
        }
        form.busy = true;
        let c = self.client.clone();
        if let Some(id) = form.editing.clone() {
            let sent = label.clone();
            self.spawn(
                async move { c.update_api_key(&id, &sent, &scopes).await },
                move |app, res| match res {
                    Ok(()) => {
                        app.manage.modal = None;
                        app.status = format!("Saved the key {label}");
                        app.manage.people.keys = Load::Idle;
                        app.load_api_keys();
                    }
                    Err(e) => {
                        if let Some(SettingsModal::NewKey(f)) = app.manage.modal.as_mut() {
                            f.busy = false;
                            f.error = Some(e.to_string());
                        }
                    }
                },
            );
            return;
        }
        self.spawn(
            async move { c.create_api_key(&label, &scopes).await },
            |app, res| match res {
                Ok(key) => app.show_new_secret(key),
                Err(e) => {
                    if let Some(SettingsModal::NewKey(f)) = app.manage.modal.as_mut() {
                        f.busy = false;
                        f.error = Some(e.to_string());
                    }
                }
            },
        );
    }

    /// A key's secret: into the dialog, once.
    fn show_new_secret(&mut self, key: NewApiKey) {
        self.status = format!("Made a key for \"{}\"", key.label);
        self.manage.modal = Some(SettingsModal::Secret(key));
        self.load_api_keys();
    }

    pub fn ask_rotate_api_key(&mut self) {
        let Some(k) = self.selected_api_key() else {
            return;
        };
        if k.is_revoked() {
            self.error = Some("This key is revoked; make a new key instead".to_string());
            return;
        }
        let (id, label) = (k.id.clone(), k.label.clone());
        self.ask(
            Confirm::new(
                format!("A new secret for \"{label}\"?"),
                "The old one stops working now. Put the new one where the program reads it.",
                ConfirmAction::Settings(SettingsConfirm::RotateKey { id }),
            )
            .with_detail("The new secret is shown once."),
        );
    }

    pub(crate) fn rotate_api_key(&mut self, id: String) {
        let c = self.client.clone();
        self.spawn_ok(
            "new secret",
            async move { c.rotate_api_key(&id).await },
            |app, key| app.show_new_secret(key),
        );
    }

    pub fn ask_revoke_api_key(&mut self) {
        let Some(k) = self.selected_api_key() else {
            return;
        };
        if k.is_revoked() {
            self.status = "Already revoked".to_string();
            return;
        }
        let (id, label) = (k.id.clone(), k.label.clone());
        self.ask(Confirm::new(
            format!("Revoke \"{label}\"?"),
            "It stops working at once, and can't be turned back on.",
            ConfirmAction::Settings(SettingsConfirm::RevokeKey { id, label }),
        ));
    }

    pub(crate) fn revoke_api_key(&mut self, id: String, label: String) {
        let c = self.client.clone();
        self.spawn_ok(
            "revoke key",
            async move { c.revoke_api_key(&id).await },
            move |app, ()| {
                app.status = format!("Revoked \"{label}\"");
                app.load_api_keys();
            },
        );
    }

    // ── Sessions ─────────────────────────────────────────────────────────

    pub(crate) fn load_displays(&mut self) {
        if self.manage.people.displays.ready().is_none() {
            self.manage.people.displays = Load::Loading;
        }
        let c = self.client.clone();
        self.spawn(async move { c.list_displays().await }, |app, res| {
            app.manage.people.displays = match res {
                Ok(mut v) => {
                    v.retain(|d| !d.revoked);
                    v.sort_by_key(|d| d.name.to_lowercase());
                    Load::Ready(v)
                }
                Err(e) => Load::Failed(e.to_string()),
            };
            let ids = app.display_ids();
            app.manage.people.displays_sel.resolve(&ids);
        });
    }

    fn display_ids(&self) -> Vec<String> {
        self.manage
            .people
            .displays
            .ready()
            .map(|v| v.iter().map(|d| d.id.clone()).collect())
            .unwrap_or_default()
    }

    pub fn move_display(&mut self, delta: isize) {
        let ids = self.display_ids();
        self.manage.people.displays_sel.move_by(delta, &ids);
    }

    fn selected_display(&self) -> Option<&crate::model::settings::Display> {
        let id = self.manage.people.displays_sel.id()?;
        self.manage
            .people
            .displays
            .ready()?
            .iter()
            .find(|d| d.id == id)
    }

    /// `n`: name a new display; its key is shown once.
    pub fn open_new_display(&mut self) {
        self.manage.modal = Some(super::settings_modal::SettingsModal::DisplayName(
            super::settings_modal::DisplayNameForm::default(),
        ));
    }

    pub fn ask_delete_display(&mut self) {
        let Some(d) = self.selected_display() else {
            return;
        };
        let (id, name) = (d.id.clone(), d.name.clone());
        self.ask(Confirm::new(
            format!("Delete the display {name}?"),
            "It is signed out on its next request, and its key stops working.",
            ConfirmAction::Settings(SettingsConfirm::DeleteDisplay { id, name }),
        ));
    }

    pub(crate) fn delete_display(&mut self, id: String, name: String) {
        let c = self.client.clone();
        self.spawn_ok(
            "delete display",
            async move { c.delete_display(&id).await },
            move |app, ()| {
                app.status = format!("Deleted the display {name}");
                app.manage.people.displays = Load::Idle;
                app.load_displays();
            },
        );
    }

    pub(crate) fn load_sessions(&mut self) {
        if self.manage.people.sessions.is_loading() {
            return;
        }
        if self.manage.people.sessions.ready().is_none() {
            self.manage.people.sessions = Load::Loading;
        }
        let c = self.client.clone();
        self.spawn(async move { c.list_sessions().await }, |app, res| {
            app.manage.people.sessions = match res {
                Ok(v) => Load::Ready(v),
                Err(e) => Load::Failed(e.to_string()),
            };
        });
    }

    fn session_ids(&self) -> Vec<String> {
        self.manage
            .people
            .sessions
            .ready()
            .map(|v| v.iter().map(|s| s.id.clone()).collect())
            .unwrap_or_default()
    }

    pub fn selected_session(&self) -> Option<&SessionInfo> {
        let id = self.manage.people.sessions_sel.id()?;
        self.manage
            .people
            .sessions
            .ready()?
            .iter()
            .find(|s| s.id == id)
    }

    pub fn move_session(&mut self, delta: isize) {
        let ids = self.session_ids();
        self.manage.people.sessions_sel.move_by(delta, &ids);
    }

    pub fn ask_end_session(&mut self) {
        let Some(s) = self.selected_session() else {
            return;
        };
        if s.current {
            self.error = Some("This is the session you are using; L signs out".to_string());
            return;
        }
        let id = s.id.clone();
        let what = crate::model::settings::client_of(s.user_agent.as_deref());
        self.ask(Confirm::new(
            "End this session?",
            format!("{what} is signed out at once."),
            ConfirmAction::Settings(SettingsConfirm::EndSession { id }),
        ));
    }

    pub fn ask_end_other_sessions(&mut self) {
        let others = self.other_session_ids();
        if others.is_empty() {
            self.status = "No other sessions".to_string();
            return;
        }
        self.ask(Confirm::new(
            "End every other session?",
            format!(
                "{} other session(s) of yours are signed out at once. This one stays.",
                others.len()
            ),
            ConfirmAction::Settings(SettingsConfirm::EndOtherSessions),
        ));
    }

    fn other_session_ids(&self) -> Vec<String> {
        self.manage
            .people
            .sessions
            .ready()
            .map(|v| {
                v.iter()
                    .filter(|s| !s.current)
                    .map(|s| s.id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// End `ids` one by one (core's end-all would end this one too).
    pub(crate) fn end_sessions(&mut self, ids: Vec<String>) {
        let c = self.client.clone();
        let n = ids.len();
        self.spawn_ok(
            "end session",
            async move {
                for id in &ids {
                    c.end_session(id).await?;
                }
                Ok(())
            },
            move |app, ()| {
                app.status = if n == 1 {
                    "Session ended".to_string()
                } else {
                    format!("Ended {n} sessions")
                };
                app.load_sessions();
            },
        );
    }

    pub(crate) fn end_other_sessions(&mut self) {
        let ids = self.other_session_ids();
        self.end_sessions(ids);
    }

    // ── A person ─────────────────────────────────────────────────────────

    pub fn ask_remove_pin(&mut self) {
        let Some(u) = self.selected_user() else {
            return;
        };
        if !u.has_pin {
            self.status = format!("{} has no PIN", u.username);
            return;
        }
        let (id, username) = (u.id.clone(), u.username.clone());
        self.ask(Confirm::new(
            format!("Remove {username}'s PIN?"),
            "They can't use a display that asks for it until they set a new one.",
            ConfirmAction::Settings(SettingsConfirm::RemovePin { id, username }),
        ));
    }

    pub(crate) fn remove_pin(&mut self, id: String, username: String) {
        let c = self.client.clone();
        let target = id.clone();
        self.spawn_ok(
            "remove PIN",
            async move { c.clear_user_pin(&target).await },
            move |app, ()| {
                if let Some(u) = app.users.iter_mut().find(|u| u.id == id) {
                    u.has_pin = false;
                }
                app.status = format!("Removed {username}'s PIN");
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::*;
    use crate::model::Role;
    use crate::net::http::parse_body;

    fn with_keys(a: &mut App) {
        let keys: Vec<ApiKey> = parse_body(
            "/auth/api-keys",
            include_str!("../../../tests/fixtures/api_keys.json"),
        )
        .unwrap();
        a.manage.people.keys = Load::Ready(keys);
        a.manage.people.tab = PeopleTab::ApiKeys;
        a.refresh_views();
    }

    #[test]
    fn keys_in_use_come_first_and_revoked_last() {
        let mut a = signed_in(Role::Admin);
        with_keys(&mut a);
        let labels: Vec<&str> = a
            .visible_api_keys()
            .iter()
            .map(|k| k.label.as_str())
            .collect();
        assert_eq!(labels, vec!["hc-mcp", "Kitchen panel", "backup script"]);
    }

    #[test]
    fn a_revoked_key_is_not_rotated_or_revoked_again() {
        let mut a = signed_in(Role::Admin);
        with_keys(&mut a);
        a.move_api_key(2);
        assert_eq!(a.selected_api_key().unwrap().label, "backup script");
        a.ask_rotate_api_key();
        assert!(a.confirm.is_none() && a.error.is_some());
        a.ask_revoke_api_key();
        assert!(a.confirm.is_none());
        a.move_api_key(-2);
        a.ask_revoke_api_key();
        assert!(matches!(
            a.confirm.as_ref().map(|c| &c.action),
            Some(ConfirmAction::Settings(SettingsConfirm::RevokeKey { .. }))
        ));
    }

    #[test]
    fn the_current_session_is_never_ended_from_the_list() {
        let mut a = signed_in(Role::Admin);
        let s: Vec<SessionInfo> = parse_body(
            "/auth/sessions",
            include_str!("../../../tests/fixtures/sessions.json"),
        )
        .unwrap();
        a.manage.people.sessions = Load::Ready(s);
        a.refresh_views();
        assert!(a.selected_session().unwrap().current);
        a.ask_end_session();
        assert!(a.confirm.is_none());
        assert_eq!(a.other_session_ids().len(), 2);
        a.ask_end_other_sessions();
        assert!(a.confirm.is_some());
    }

    /// A made key's secret reaches the dialog only: not the status line,
    /// not the key list, not any file. (`NewApiKey` has no `Serialize`,
    /// so it cannot be cached by mistake either.)
    #[tokio::test]
    async fn a_new_secret_is_shown_once_and_never_kept() {
        let mut a = signed_in(Role::Admin);
        let token = "hc_sk_neverkeepthis0123";
        let k: NewApiKey = serde_json::from_value(serde_json::json!({
            "id": "k9", "label": "nas", "token": token
        }))
        .unwrap();
        a.show_new_secret(k);
        assert!(matches!(a.manage.modal, Some(SettingsModal::Secret(_))));
        assert!(!a.status.contains(token));
        assert!(
            a.manage
                .people
                .keys
                .ready()
                .is_none_or(|ks| ks.iter().all(|k| !k.prefix.contains("neverkeep")))
        );
        a.save_cache(&[crate::app::Section::Users]);
        a.on_key(key(crossterm::event::KeyCode::Esc));
        assert!(a.manage.modal.is_none());
        fn holds(dir: &std::path::Path, needle: &str) -> bool {
            std::fs::read_dir(dir)
                .into_iter()
                .flatten()
                .flatten()
                .any(|e| {
                    let p = e.path();
                    if p.is_dir() {
                        holds(&p, needle)
                    } else {
                        std::fs::read_to_string(&p).is_ok_and(|s| s.contains(needle))
                    }
                })
        }
        assert!(!holds(a.cache.root(), token));
    }

    #[tokio::test]
    async fn tabs_step_both_ways() {
        let mut a = signed_in(Role::Admin);
        a.people_step_tab(1);
        assert_eq!(a.manage.people.tab, PeopleTab::Roles);
        a.people_step_tab(-2);
        assert_eq!(
            a.manage.people.tab,
            PeopleTab::Displays,
            "wraps to the last"
        );
    }

    #[test]
    fn a_new_key_offers_the_scopes_i_hold() {
        let mut a = signed_in(Role::Admin);
        a.current_user.as_mut().unwrap().scopes =
            Some(vec!["devices:read".into(), "areas:read".into()]);
        assert_eq!(a.grantable_scopes(), vec!["areas:read", "devices:read"]);
    }
}

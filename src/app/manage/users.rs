//! People: who can sign in, their roles, and changing your own
//! password.

use crate::app::editors::{Confirm, ConfirmAction, UserEditField, UserEditMode, UserEditor};
use crate::app::{App, FocusField, Section};
use crate::model::UserInfo;
use crate::text::char_len;
use crossterm::event::{KeyCode, KeyEvent};

const MIN_PASSWORD_CHARS: usize = 8;

impl App {
    pub fn selected_user(&self) -> Option<&UserInfo> {
        let id = self.manage.users_sel.id()?;
        self.views
            .users
            .iter()
            .filter_map(|&i| self.users.get(i))
            .find(|u| u.id == id)
    }

    pub(crate) fn on_key_users(&mut self, key: KeyEvent) {
        let ids = self.user_view_ids();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.manage.users_sel.move_by(1, &ids),
            KeyCode::Up | KeyCode::Char('k') => self.manage.users_sel.move_by(-1, &ids),
            KeyCode::Char('n') if self.is_admin() => self.open_user_create(),
            KeyCode::Enter => {
                if self.is_admin() {
                    self.open_user_role()
                }
            }
            KeyCode::Char('p') => self.open_password(),
            KeyCode::Char('x') | KeyCode::Delete => self.ask_delete_user(),
            _ => {}
        }
    }

    fn open_user_create(&mut self) {
        self.manage.user_editor = Some(UserEditor::new(
            UserEditMode::Create,
            UserEditField::Username,
        ));
    }

    fn open_user_role(&mut self) {
        let Some(u) = self.selected_user() else {
            return;
        };
        let mut ed = UserEditor::new(UserEditMode::EditRole, UserEditField::Role);
        ed.id = Some(u.id.clone());
        ed.username = u.username.clone();
        ed.role = u.role;
        self.manage.user_editor = Some(ed);
    }

    /// `p`: an admin on someone else sets their password; otherwise it is
    /// your own, which needs your current one.
    fn open_password(&mut self) {
        let me = self.current_user.as_ref().map(|u| u.id.clone());
        match self.selected_user() {
            Some(u) if self.is_admin() && Some(&u.id) != me.as_ref() => {
                let mut ed = UserEditor::new(UserEditMode::SetPassword, UserEditField::Password);
                ed.id = Some(u.id.clone());
                ed.username = u.username.clone();
                self.manage.user_editor = Some(ed);
            }
            _ => self.open_change_password(),
        }
    }

    /// Change-password always acts on the signed-in account.
    fn open_change_password(&mut self) {
        let Some(u) = self.current_user.clone() else {
            return;
        };
        let mut ed = UserEditor::new(UserEditMode::ChangePassword, UserEditField::CurrentPassword);
        ed.id = Some(u.id);
        ed.username = u.username;
        self.manage.user_editor = Some(ed);
    }

    pub(crate) fn save_user_editor(&mut self) {
        let Some(ed) = self.manage.user_editor.clone() else {
            return;
        };
        let c = self.client.clone();
        match ed.mode {
            UserEditMode::Create => {
                let username = ed.username.trim().to_string();
                if username.is_empty() {
                    self.error = Some("username cannot be empty".to_string());
                    return;
                }
                if char_len(&ed.password) < MIN_PASSWORD_CHARS {
                    self.error = Some("password must be at least 8 characters".to_string());
                    return;
                }
                if ed.password != ed.confirm_password {
                    self.error = Some("passwords do not match".to_string());
                    return;
                }
                let (password, role) = (ed.password.clone(), ed.role);
                self.spawn_ok(
                    "create user",
                    async move { c.create_user(&username, &password, role).await },
                    |app, user: UserInfo| {
                        app.status = format!("Created user '{}'", user.username);
                        app.users.push(user);
                        app.manage.user_editor = None;
                        app.save_cache(&[Section::Users]);
                    },
                );
            }
            UserEditMode::EditRole => {
                let Some(id) = ed.id.clone() else { return };
                let role = ed.role;
                self.spawn_ok(
                    "change role",
                    async move { c.set_user_role(&id, role).await },
                    move |app, updated: UserInfo| {
                        if let Some(u) = app.users.iter_mut().find(|u| u.id == updated.id) {
                            u.role = updated.role;
                        }
                        app.manage.user_editor = None;
                        app.status = format!("Updated role for '{}'", ed.username);
                        app.save_cache(&[Section::Users]);
                    },
                );
            }
            UserEditMode::SetPassword => {
                if char_len(&ed.password) < MIN_PASSWORD_CHARS {
                    self.error = Some("password must be at least 8 characters".to_string());
                    return;
                }
                if ed.password != ed.confirm_password {
                    self.error = Some("passwords do not match".to_string());
                    return;
                }
                let Some(id) = ed.id.clone() else { return };
                let new = ed.password.clone();
                self.spawn_ok(
                    "set password",
                    async move { c.set_user_password(&id, &new).await },
                    move |app, ()| {
                        app.manage.user_editor = None;
                        app.status = format!(
                            "Set. {} is signed out everywhere; give them the new one in person.",
                            ed.username
                        );
                    },
                );
            }
            UserEditMode::ChangePassword => {
                if char_len(&ed.password) < MIN_PASSWORD_CHARS {
                    self.error = Some("new password must be at least 8 characters".to_string());
                    return;
                }
                if ed.password != ed.confirm_password {
                    self.error = Some("passwords do not match".to_string());
                    return;
                }
                let (cur, new) = (ed.current_password.clone(), ed.password.clone());
                self.spawn_ok(
                    "change password",
                    async move { c.change_password(&cur, &new).await },
                    move |app, ()| {
                        // Core revokes every session on a password change,
                        // this one included: sign in again.
                        app.manage.user_editor = None;
                        app.on_session_expired();
                        app.login.username = ed.username.clone();
                        app.login.focus = Some(FocusField::Password);
                        app.error = None;
                        app.status = "Password changed; sign in with the new password".to_string();
                    },
                );
            }
        }
    }

    fn ask_delete_user(&mut self) {
        if !self.is_admin() {
            return;
        }
        let Some(u) = self.selected_user() else {
            return;
        };
        if self.current_user.as_ref().is_some_and(|me| me.id == u.id) {
            self.error = Some("cannot delete your own account".to_string());
            return;
        }
        let (id, username) = (u.id.clone(), u.username.clone());
        self.ask(Confirm::new(
            "Delete user",
            format!("Delete user \"{username}\"?"),
            ConfirmAction::DeleteUser { id, username },
        ));
    }

    pub(crate) fn delete_user(&mut self, id: String, username: String) {
        let c = self.client.clone();
        let target = id.clone();
        self.spawn_ok(
            "delete user",
            async move { c.delete_user(&target).await },
            move |app, ()| {
                app.users.retain(|u| u.id != id);
                app.status = format!("Deleted user '{username}'");
                app.save_cache(&[Section::Users]);
            },
        );
    }
}

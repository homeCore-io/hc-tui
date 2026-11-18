//! Modal editors, the confirmation dialog, and their key handling.
//!
//! Each editor is a plain struct held by its tab. Key handling here only
//! edits fields; saving is done by the owning module, which spawns the
//! request.

use super::App;
use crate::model::Role;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Apply a text-editing key to `field`: Backspace deletes, printable
/// characters append. Returns true when the key was consumed.
pub fn edit_text(field: &mut String, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Backspace => {
            field.pop();
            true
        }
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            field.push(ch);
            true
        }
        _ => false,
    }
}

/// Like [`edit_text`] but only accepts ASCII digits.
pub fn edit_digits(field: &mut String, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char(ch) if !ch.is_ascii_digit() => true,
        _ => edit_text(field, key),
    }
}

// ── Confirmation ────────────────────────────────────────────────────────────

/// Something destructive, waiting for `y`.
#[derive(Debug, Clone)]
pub enum ConfirmAction {
    DeleteDevice {
        id: String,
    },
    DeleteArea {
        id: String,
        name: String,
    },
    DeleteRule {
        id: String,
        name: String,
    },
    DeleteRuleGroup {
        id: String,
        name: String,
    },
    DeleteUser {
        id: String,
        username: String,
    },
    DeleteMode {
        id: String,
    },
    RemoveMatterNode {
        node_id: String,
    },
    UninstallPlugin {
        plugin_id: String,
    },
    ImportRules,
    ImportScenes,
    /// People, Data & backups and System: see `manage::settings_modal`.
    Settings(super::manage::settings_modal::SettingsConfirm),
    Helpers(super::helpers_tab::HelperConfirm),
    Automations(super::automations::AutomationConfirm),
    Alerts(super::alerts::AlertConfirm),
    /// The Plugins screen's own questions (install, reset, an action).
    Plugins(super::plugin_manage::PluginConfirm),
    /// Asked by Devices, Rooms or Scenes.
    Devices(super::controls::DeviceConfirm),
}

#[derive(Debug, Clone)]
pub struct Confirm {
    pub title: String,
    pub message: String,
    pub detail: Option<String>,
    pub action: ConfirmAction,
}

impl Confirm {
    pub fn new(
        title: impl Into<String>,
        message: impl Into<String>,
        action: ConfirmAction,
    ) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            detail: None,
            action,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

// ── Device editor ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceEditField {
    Name,
    Area,
    Security,
    CanonicalName,
}

/// Rename / move / canonical-name editor. The `initial_*` fields hold
/// what was shown when it opened, so only changed fields are sent.
#[derive(Debug, Clone)]
pub struct DeviceEditor {
    pub device_id: String,
    pub name: String,
    pub area: String,
    pub canonical_name: String,
    pub initial_name: String,
    pub initial_area: String,
    pub initial_canonical_name: String,
    /// Security: in (true), out (false), or as its type says (None).
    pub security: Option<bool>,
    pub initial_security: Option<bool>,
    pub field: DeviceEditField,
}

impl DeviceEditor {
    pub fn new(device_id: String, name: String, area: String, canonical_name: String) -> Self {
        Self {
            device_id,
            initial_name: name.clone(),
            initial_area: area.clone(),
            initial_canonical_name: canonical_name.clone(),
            name,
            area,
            canonical_name,
            security: None,
            initial_security: None,
            field: DeviceEditField::Name,
        }
    }

    pub fn with_security(mut self, security: Option<bool>) -> Self {
        self.security = security;
        self.initial_security = security;
        self
    }

    /// The next Security choice: as its type says, in, out.
    pub fn cycle_security(&mut self, forward: bool) {
        let order = [None, Some(true), Some(false)];
        let i = order.iter().position(|s| *s == self.security).unwrap_or(0);
        self.security = order[if forward { (i + 1) % 3 } else { (i + 2) % 3 }];
    }

    /// The Security choice in words.
    pub fn security_label(s: Option<bool>) -> &'static str {
        match s {
            None => "as its type says",
            Some(true) => "yes, watch it",
            Some(false) => "no, leave it out",
        }
    }

    /// The `PATCH /devices/{id}` body: only fields that changed. An
    /// emptied field is sent as null, which clears the override.
    pub fn changes(&self) -> serde_json::Map<String, serde_json::Value> {
        use serde_json::Value;
        let mut body = serde_json::Map::new();
        let mut put = |key: &str, now: &str, before: &str| {
            let now = now.trim();
            if now != before.trim() {
                let v = if now.is_empty() {
                    Value::Null
                } else {
                    Value::String(now.to_string())
                };
                body.insert(key.to_string(), v);
            }
        };
        put("name", &self.name, &self.initial_name);
        put("area", &self.area, &self.initial_area);
        put(
            "canonical_name",
            &self.canonical_name,
            &self.initial_canonical_name,
        );
        if self.security != self.initial_security {
            body.insert(
                "security".to_string(),
                self.security.map(Value::Bool).unwrap_or(Value::Null),
            );
        }
        body
    }
}

// ── Area editor ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AreaEditor {
    /// `None` = create, `Some(id)` = rename.
    pub id: Option<String>,
    pub name: String,
}

// ── User editor ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserEditMode {
    Create,
    EditRole,
    ChangePassword,
    /// An admin gives someone else a new password.
    SetPassword,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserEditField {
    Username,
    Password,
    ConfirmPassword,
    CurrentPassword,
    Role,
}

#[derive(Clone)]
pub struct UserEditor {
    pub mode: UserEditMode,
    pub id: Option<String>,
    pub field: UserEditField,
    pub username: String,
    pub current_password: String,
    pub password: String,
    pub confirm_password: String,
    pub role: Role,
}

impl std::fmt::Debug for UserEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UserEditor")
            .field("mode", &self.mode)
            .field("username", &self.username)
            .field("role", &self.role)
            .finish_non_exhaustive()
    }
}

impl UserEditor {
    pub fn new(mode: UserEditMode, field: UserEditField) -> Self {
        Self {
            mode,
            id: None,
            field,
            username: String::new(),
            current_password: String::new(),
            password: String::new(),
            confirm_password: String::new(),
            role: Role::User,
        }
    }

    fn fields(&self) -> &'static [UserEditField] {
        use UserEditField as F;
        match self.mode {
            UserEditMode::Create => &[F::Username, F::Password, F::ConfirmPassword, F::Role],
            UserEditMode::EditRole => &[F::Role],
            UserEditMode::ChangePassword => &[F::CurrentPassword, F::Password, F::ConfirmPassword],
            UserEditMode::SetPassword => &[F::Password, F::ConfirmPassword],
        }
    }

    fn step(&mut self, forward: bool) {
        let fields = self.fields();
        let i = fields.iter().position(|f| *f == self.field).unwrap_or(0);
        let n = fields.len();
        self.field = fields[if forward {
            (i + 1) % n
        } else {
            (i + n - 1) % n
        }];
    }

    fn field_mut(&mut self) -> Option<&mut String> {
        match self.field {
            UserEditField::Username => Some(&mut self.username),
            UserEditField::Password => Some(&mut self.password),
            UserEditField::ConfirmPassword => Some(&mut self.confirm_password),
            UserEditField::CurrentPassword => Some(&mut self.current_password),
            UserEditField::Role => None,
        }
    }
}

// ── Mode editor ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeEditField {
    Id,
    Name,
    Kind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeKind {
    Solar,
    Manual,
}

impl ModeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Solar => "solar",
            Self::Manual => "manual",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Solar => Self::Manual,
            Self::Manual => Self::Solar,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModeEditor {
    pub id: String,
    pub name: String,
    pub kind: ModeKind,
    pub field: ModeEditField,
}

// ── Matter commission editor ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatterCommissionField {
    PairingCode,
    Name,
    Room,
    Discriminator,
    Passcode,
}

#[derive(Debug, Clone)]
pub struct MatterCommissionEditor {
    pub pairing_code: String,
    pub name: String,
    pub room: String,
    pub discriminator: String,
    pub passcode: String,
    pub field: MatterCommissionField,
}

impl Default for MatterCommissionEditor {
    fn default() -> Self {
        Self {
            pairing_code: String::new(),
            name: String::new(),
            room: String::new(),
            discriminator: String::new(),
            passcode: String::new(),
            field: MatterCommissionField::PairingCode,
        }
    }
}

// ── Rule filter bar ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleFilterField {
    Tag,
    Trigger,
}

#[derive(Debug, Clone)]
pub struct RuleFilterBar {
    pub tag: String,
    pub trigger: String,
    pub stale: bool,
    pub active_field: RuleFilterField,
}

// ── Key handling ────────────────────────────────────────────────────────────

impl App {
    /// Route a key to whichever editor is open. Returns false when none
    /// is.
    pub(crate) fn on_key_editors(&mut self, key: KeyEvent) -> bool {
        if self.devices_tab.editor.is_some() {
            self.on_key_device_editor(key);
        } else if self.rooms.editor.is_some() {
            self.on_key_area_editor(key);
        } else if self.manage.user_editor.is_some() {
            self.on_key_user_editor(key);
        } else if self.manage.mode_editor.is_some() {
            self.on_key_mode_editor(key);
        } else if self.manage.matter.editor.is_some() {
            self.on_key_matter_editor(key);
        } else {
            return false;
        }
        true
    }

    fn on_key_device_editor(&mut self, key: KeyEvent) {
        let Some(ed) = self.devices_tab.editor.as_mut() else {
            return;
        };
        use DeviceEditField as F;
        match key.code {
            KeyCode::Esc => {
                self.devices_tab.editor = None;
                self.status = "Device edit canceled".to_string();
            }
            KeyCode::Tab | KeyCode::Down => {
                ed.field = match ed.field {
                    F::Name => F::Area,
                    F::Area => F::Security,
                    F::Security => F::CanonicalName,
                    F::CanonicalName => F::Name,
                };
            }
            KeyCode::BackTab | KeyCode::Up => {
                ed.field = match ed.field {
                    F::Name => F::CanonicalName,
                    F::Area => F::Name,
                    F::Security => F::Area,
                    F::CanonicalName => F::Security,
                };
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if ed.field == F::Security => {
                ed.cycle_security(key.code != KeyCode::Left);
            }
            KeyCode::Left | KeyCode::Right if ed.field == F::Area => {
                // Step through the rooms there are, and no room.
                let mut names: Vec<String> = self.areas.iter().map(|a| a.name.clone()).collect();
                names.sort_by_key(|n| n.to_lowercase());
                names.insert(0, String::new());
                let i = names
                    .iter()
                    .position(|n| n.eq_ignore_ascii_case(ed.area.trim()));
                let n = names.len();
                let next = match (i, key.code) {
                    (Some(i), KeyCode::Left) => (i + n - 1) % n,
                    (Some(i), _) => (i + 1) % n,
                    (None, _) => 0,
                };
                ed.area = names[next].clone();
            }
            KeyCode::Enter => self.save_device_editor(),
            _ => {
                let field = match ed.field {
                    F::Name => &mut ed.name,
                    F::Area => &mut ed.area,
                    F::CanonicalName => &mut ed.canonical_name,
                    F::Security => return,
                };
                edit_text(field, key);
            }
        }
    }

    fn on_key_area_editor(&mut self, key: KeyEvent) {
        let Some(ed) = self.rooms.editor.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                self.rooms.editor = None;
                self.status = "Area edit canceled".to_string();
            }
            KeyCode::Enter => self.save_area_editor(),
            _ => {
                edit_text(&mut ed.name, key);
            }
        }
    }

    fn on_key_user_editor(&mut self, key: KeyEvent) {
        let Some(ed) = self.manage.user_editor.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                self.manage.user_editor = None;
                self.status = "User edit canceled".to_string();
            }
            KeyCode::Tab | KeyCode::Down => ed.step(true),
            KeyCode::BackTab | KeyCode::Up => ed.step(false),
            KeyCode::Char(' ') if ed.field == UserEditField::Role => ed.role = ed.role.next(),
            KeyCode::Enter => self.save_user_editor(),
            _ => {
                if let Some(f) = ed.field_mut() {
                    edit_text(f, key);
                }
            }
        }
    }

    fn on_key_mode_editor(&mut self, key: KeyEvent) {
        let Some(ed) = self.manage.mode_editor.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                self.manage.mode_editor = None;
                self.status = "Cancelled".to_string();
            }
            KeyCode::Tab | KeyCode::BackTab => {
                ed.field = match ed.field {
                    ModeEditField::Id => ModeEditField::Name,
                    ModeEditField::Name => ModeEditField::Kind,
                    ModeEditField::Kind => ModeEditField::Id,
                };
            }
            KeyCode::Char(' ') if ed.field == ModeEditField::Kind => ed.kind = ed.kind.next(),
            KeyCode::Enter => self.save_mode_editor(),
            _ => match ed.field {
                ModeEditField::Id => {
                    edit_text(&mut ed.id, key);
                }
                ModeEditField::Name => {
                    edit_text(&mut ed.name, key);
                }
                ModeEditField::Kind => {}
            },
        }
    }

    fn on_key_matter_editor(&mut self, key: KeyEvent) {
        let Some(ed) = self.manage.matter.editor.as_mut() else {
            return;
        };
        use MatterCommissionField as F;
        match key.code {
            KeyCode::Esc => {
                self.manage.matter.editor = None;
                self.status = "Matter commission canceled".to_string();
            }
            KeyCode::Tab | KeyCode::BackTab => {
                ed.field = match ed.field {
                    F::PairingCode => F::Name,
                    F::Name => F::Room,
                    F::Room => F::Discriminator,
                    F::Discriminator => F::Passcode,
                    F::Passcode => F::PairingCode,
                };
            }
            KeyCode::Enter => self.save_matter_editor(),
            _ => {
                match ed.field {
                    F::PairingCode => edit_text(&mut ed.pairing_code, key),
                    F::Name => edit_text(&mut ed.name, key),
                    F::Room => edit_text(&mut ed.room, key),
                    F::Discriminator => edit_digits(&mut ed.discriminator, key),
                    F::Passcode => edit_digits(&mut ed.passcode, key),
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_editor_sends_only_changes() {
        let mut ed =
            DeviceEditor::new("d1".into(), "Porch".into(), "garage".into(), "porch".into());
        assert!(ed.changes().is_empty());
        ed.name = "Front Porch".into();
        let c = ed.changes();
        assert_eq!(c.len(), 1);
        assert_eq!(c["name"], "Front Porch");
        ed.area.clear();
        let c = ed.changes();
        assert!(c["area"].is_null(), "emptied room clears the override");
        assert!(!c.contains_key("canonical_name"));
    }

    #[test]
    fn security_is_sent_as_true_false_or_null_only_when_changed() {
        let mut ed = DeviceEditor::new("d1".into(), "Door".into(), String::new(), String::new())
            .with_security(Some(false));
        assert!(ed.changes().is_empty());
        ed.cycle_security(true);
        assert!(ed.changes()["security"].is_null(), "as its type says");
        ed.cycle_security(true);
        assert_eq!(ed.changes()["security"], true);
        ed.cycle_security(false);
        ed.cycle_security(false);
        assert!(ed.changes().is_empty(), "back where it was");
    }

    #[test]
    fn digits_only() {
        let mut s = String::new();
        edit_digits(
            &mut s,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
        );
        edit_digits(
            &mut s,
            KeyEvent::new(KeyCode::Char('7'), KeyModifiers::NONE),
        );
        assert_eq!(s, "7");
    }
}

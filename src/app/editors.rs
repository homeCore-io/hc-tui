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
    /// The Plugins screen's own questions (install, reset, an action).
    Plugins(super::plugin_manage::PluginConfirm),
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
            field: DeviceEditField::Name,
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

// ── Glue (helper) creator ───────────────────────────────────────────────────

/// The helper types served by `POST /glue`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlueType {
    Switch,
    Timer,
    Counter,
    Number,
    Select,
    Text,
    Button,
    Datetime,
    Group,
    Threshold,
    Schedule,
}

impl GlueType {
    pub const ALL: [GlueType; 11] = [
        GlueType::Switch,
        GlueType::Timer,
        GlueType::Counter,
        GlueType::Number,
        GlueType::Select,
        GlueType::Text,
        GlueType::Button,
        GlueType::Datetime,
        GlueType::Group,
        GlueType::Threshold,
        GlueType::Schedule,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            GlueType::Switch => "switch",
            GlueType::Timer => "timer",
            GlueType::Counter => "counter",
            GlueType::Number => "number",
            GlueType::Select => "select",
            GlueType::Text => "text",
            GlueType::Button => "button",
            GlueType::Datetime => "datetime",
            GlueType::Group => "group",
            GlueType::Threshold => "threshold",
            GlueType::Schedule => "schedule",
        }
    }

    pub fn next(&self) -> Self {
        let i = Self::ALL.iter().position(|t| t == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlueEditField {
    Type,
    Id,
    Name,
    Options,
    Members,
    SourceDeviceId,
    SourceAttribute,
    Threshold,
}

#[derive(Debug, Clone)]
pub struct GlueCreator {
    pub glue_type: GlueType,
    pub id: String,
    pub name: String,
    pub options: String,
    pub members: String,
    pub source_device_id: String,
    pub source_attribute: String,
    pub threshold: String,
    pub field: GlueEditField,
    pub error: Option<String>,
}

impl Default for GlueCreator {
    fn default() -> Self {
        Self {
            glue_type: GlueType::Counter,
            id: String::new(),
            name: String::new(),
            options: String::new(),
            members: String::new(),
            source_device_id: String::new(),
            source_attribute: "value".to_string(),
            threshold: String::new(),
            field: GlueEditField::Type,
            error: None,
        }
    }
}

impl GlueCreator {
    /// Fields in order, skipping ones that do not apply to the type.
    pub fn fields_for_type(t: GlueType) -> Vec<GlueEditField> {
        let mut fields = vec![GlueEditField::Type, GlueEditField::Id, GlueEditField::Name];
        match t {
            GlueType::Select => fields.push(GlueEditField::Options),
            GlueType::Group => fields.push(GlueEditField::Members),
            GlueType::Threshold => {
                fields.push(GlueEditField::SourceDeviceId);
                fields.push(GlueEditField::SourceAttribute);
                fields.push(GlueEditField::Threshold);
            }
            _ => {}
        }
        fields
    }

    fn field_mut(&mut self) -> Option<&mut String> {
        match self.field {
            GlueEditField::Type => None,
            GlueEditField::Id => Some(&mut self.id),
            GlueEditField::Name => Some(&mut self.name),
            GlueEditField::Options => Some(&mut self.options),
            GlueEditField::Members => Some(&mut self.members),
            GlueEditField::SourceDeviceId => Some(&mut self.source_device_id),
            GlueEditField::SourceAttribute => Some(&mut self.source_attribute),
            GlueEditField::Threshold => Some(&mut self.threshold),
        }
    }

    /// Build `(id, name, type, config)` for `POST /glue`, or a message
    /// saying what is missing.
    pub fn build(&self) -> Result<(String, String, &'static str, serde_json::Value), String> {
        use serde_json::{Value, json};
        let id = self.id.trim().to_string();
        if id.is_empty() {
            return Err("id cannot be empty".to_string());
        }
        let name = if self.name.trim().is_empty() {
            id.clone()
        } else {
            self.name.trim().to_string()
        };
        let list = |s: &str| -> Vec<Value> {
            s.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| Value::String(s.to_string()))
                .collect()
        };
        let mut config = serde_json::Map::new();
        match self.glue_type {
            GlueType::Select => {
                let opts = list(&self.options);
                if opts.is_empty() {
                    return Err("select needs at least one option (comma-separated)".into());
                }
                config.insert("options".into(), Value::Array(opts));
            }
            GlueType::Group => {
                config.insert("members".into(), Value::Array(list(&self.members)));
            }
            GlueType::Threshold => {
                let src = self.source_device_id.trim();
                if src.is_empty() {
                    return Err("threshold requires source_device_id".into());
                }
                config.insert("source_device_id".into(), json!(src));
                let attr = match self.source_attribute.trim() {
                    "" => "value",
                    a => a,
                };
                config.insert("source_attribute".into(), json!(attr));
                let t: f64 = self
                    .threshold
                    .trim()
                    .parse()
                    .map_err(|_| "threshold must be a number".to_string())?;
                config.insert("threshold".into(), json!(t));
            }
            _ => {}
        }
        Ok((id, name, self.glue_type.as_str(), Value::Object(config)))
    }
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
        } else if self.manage.glue_creator.is_some() {
            self.on_key_glue_creator(key);
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
            KeyCode::Tab | KeyCode::Right | KeyCode::Down => {
                ed.field = match ed.field {
                    F::Name => F::Area,
                    F::Area => F::CanonicalName,
                    F::CanonicalName => F::Name,
                };
            }
            KeyCode::BackTab | KeyCode::Left | KeyCode::Up => {
                ed.field = match ed.field {
                    F::Name => F::CanonicalName,
                    F::Area => F::Name,
                    F::CanonicalName => F::Area,
                };
            }
            KeyCode::Enter => self.save_device_editor(),
            _ => {
                let field = match ed.field {
                    F::Name => &mut ed.name,
                    F::Area => &mut ed.area,
                    F::CanonicalName => &mut ed.canonical_name,
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

    fn on_key_glue_creator(&mut self, key: KeyEvent) {
        let Some(c) = self.manage.glue_creator.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                self.manage.glue_creator = None;
                self.status = "Cancelled".to_string();
            }
            KeyCode::Tab | KeyCode::BackTab => {
                let fields = GlueCreator::fields_for_type(c.glue_type);
                let i = fields.iter().position(|f| *f == c.field).unwrap_or(0);
                let n = fields.len();
                c.field = if key.code == KeyCode::Tab {
                    fields[(i + 1) % n]
                } else {
                    fields[(i + n - 1) % n]
                };
            }
            KeyCode::Char(' ') if c.field == GlueEditField::Type => {
                c.glue_type = c.glue_type.next();
                if !GlueCreator::fields_for_type(c.glue_type).contains(&c.field) {
                    c.field = GlueEditField::Type;
                }
            }
            KeyCode::Enter => self.save_glue_creator(),
            _ => {
                if let Some(f) = c.field_mut() {
                    edit_text(f, key);
                }
            }
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
    fn glue_build_checks_type_fields() {
        let mut g = GlueCreator {
            id: "x".into(),
            glue_type: GlueType::Select,
            ..Default::default()
        };
        assert!(g.build().is_err());
        g.options = "a, b".into();
        let (id, name, t, cfg) = g.build().unwrap();
        assert_eq!((id.as_str(), name.as_str(), t), ("x", "x", "select"));
        assert_eq!(cfg["options"], serde_json::json!(["a", "b"]));
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

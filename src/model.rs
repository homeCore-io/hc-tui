//! Data shapes the client reads from homeCore.
//!
//! These mirror core's JSON (see `core/crates/hc-types`, `hc-api-types`
//! and `hc-api`). Every optional field defaults, so a newer core that
//! adds fields, or an older one that lacks some, still parses.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

// ── Users and roles ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Admin,
    User,
    #[serde(alias = "readonly")]
    ReadOnly,
    Observer,
    DeviceOperator,
    RuleEditor,
    ServiceOperator,
    HouseholdMember,
}

impl Role {
    /// All roles in a stable cycle order. Drives the Space-to-cycle user
    /// editor.
    pub const ALL: [Role; 8] = [
        Role::Admin,
        Role::User,
        Role::HouseholdMember,
        Role::ReadOnly,
        Role::Observer,
        Role::DeviceOperator,
        Role::RuleEditor,
        Role::ServiceOperator,
    ];

    pub fn is_admin(&self) -> bool {
        matches!(self, Self::Admin)
    }

    pub fn next(&self) -> Role {
        let i = Self::ALL.iter().position(|r| r == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::User => "user",
            Role::ReadOnly => "read_only",
            Role::Observer => "observer",
            Role::DeviceOperator => "device_operator",
            Role::RuleEditor => "rule_editor",
            Role::ServiceOperator => "service_operator",
            Role::HouseholdMember => "household_member",
        }
    }

    /// The role in words, as the app names it: `"Household member"`.
    pub fn label(&self) -> &'static str {
        match self {
            Role::Admin => "Admin",
            Role::User => "User",
            Role::ReadOnly => "Read only",
            Role::Observer => "Observer",
            Role::DeviceOperator => "Device operator",
            Role::RuleEditor => "Rule editor",
            Role::ServiceOperator => "Service operator",
            Role::HouseholdMember => "Household member",
        }
    }

    /// Whether the role carries `scope`, per core's role table in
    /// `hc-auth/src/user.rs`. Used when the server did not send the
    /// user's scopes (login responses do not; `/auth/me` does).
    pub fn has_scope(&self, scope: &str) -> bool {
        const READ_ALL: [&str; 5] = [
            "devices:read",
            "automations:read",
            "scenes:read",
            "areas:read",
            "content:read",
        ];
        const WRITE_AUTHORING: [&str; 4] = [
            "automations:write",
            "scenes:write",
            "areas:write",
            "content:write",
        ];
        if READ_ALL.contains(&scope) {
            return true;
        }
        let authoring = WRITE_AUTHORING.contains(&scope);
        match self {
            Role::Admin => true,
            Role::User => {
                authoring || matches!(scope, "devices:write" | "devices:command" | "scenes:run")
            }
            Role::HouseholdMember => matches!(scope, "devices:command" | "scenes:run"),
            Role::ReadOnly => false,
            Role::Observer => matches!(scope, "plugins:read" | "audit:read"),
            Role::DeviceOperator => matches!(
                scope,
                "devices:write" | "devices:command" | "plugins:read" | "audit:read"
            ),
            Role::RuleEditor => {
                authoring || matches!(scope, "scenes:run" | "plugins:read" | "audit:read")
            }
            Role::ServiceOperator => {
                authoring
                    || matches!(
                        scope,
                        "devices:write" | "devices:command" | "scenes:run" | "audit:read"
                    )
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: String,
    pub username: String,
    pub role: Role,
    #[serde(default)]
    pub created_at: String,
    /// Present on `/auth/me`; absent on login and the users list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
}

impl UserInfo {
    /// Whether this user may use `scope`. Prefers the scopes the server
    /// sent, falling back to the role table.
    pub fn can(&self, scope: &str) -> bool {
        match &self.scopes {
            Some(scopes) => scopes.iter().any(|s| s == scope),
            None => self.role.has_scope(scope),
        }
    }
}

/// `POST /auth/login` and `POST /auth/refresh` responses. Refresh always
/// carries every field; login leaves the refresh pair out when minting
/// the refresh token failed.
#[derive(Clone, Serialize, Deserialize)]
pub struct LoginResponse {
    pub token: String,
    #[serde(default)]
    pub expires_in: Option<u64>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub refresh_expires_in: Option<u64>,
    pub user: UserInfo,
}

impl std::fmt::Debug for LoginResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginResponse")
            .field("token", &"<redacted>")
            .field("expires_in", &self.expires_in)
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "<redacted>"),
            )
            .field("user", &self.user)
            .finish()
    }
}

// ── Devices ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceState {
    pub device_id: String,
    /// The name the plugin gave the device.
    #[serde(default)]
    pub name: String,
    /// A person's rename. Wins over `name` for display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_override: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_name: Option<String>,
    #[serde(default)]
    pub plugin_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_type: Option<String>,
    /// Effective room slug (`area_override` or `area`), derived by core.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
    /// The room the plugin reported, before any person's override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area_override: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_device_id: Option<String>,
    /// A person's hint for how to show the device (`"light"` for a
    /// switch that runs a light).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_hint: Option<String>,
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub attributes: Map<String, Value>,
    #[serde(default)]
    pub last_seen: String,
}

impl DeviceState {
    /// The name to show: a person's rename when set, else the plugin's.
    pub fn display_name(&self) -> &str {
        match self.name_override.as_deref() {
            Some(n) if !n.trim().is_empty() => n,
            _ => &self.name,
        }
    }

    /// The effective room: `room` when core derived one, else the
    /// override, else the plugin's area.
    pub fn room_name(&self) -> Option<&str> {
        self.room
            .as_deref()
            .or(self.area_override.as_deref())
            .or(self.area.as_deref())
            .filter(|r| !r.is_empty())
    }

    pub fn device_type(&self) -> &str {
        self.device_type.as_deref().unwrap_or("")
    }

    pub fn is_scene(&self) -> bool {
        self.device_type() == "scene"
    }
}

// ── Scenes ──────────────────────────────────────────────────────────────────

/// Where a scene lives, which decides how it is activated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SceneSource {
    /// A homeCore scene: `POST /scenes/{id}/activate`.
    #[default]
    Native,
    /// A device with `device_type: "scene"`: `{"action":"activate"}`.
    Device,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub source: SceneSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
}

impl Scene {
    pub fn from_device(d: &DeviceState) -> Self {
        Scene {
            id: d.device_id.clone(),
            name: d.display_name().to_string(),
            source: SceneSource::Device,
            plugin_id: Some(d.plugin_id.clone()),
            room: d.room_name().map(str::to_string),
            active: d.attributes.get("active").and_then(Value::as_bool),
        }
    }
}

/// Native scenes plus every scene device, in that order.
pub fn merge_scenes(native: Vec<Scene>, devices: &[DeviceState]) -> Vec<Scene> {
    let mut out: Vec<Scene> = native
        .into_iter()
        .map(|mut s| {
            s.source = SceneSource::Native;
            s
        })
        .collect();
    out.extend(
        devices
            .iter()
            .filter(|d| d.is_scene())
            .map(Scene::from_device),
    );
    out
}

// ── Areas ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Area {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub device_ids: Vec<String>,
    /// Other slugs the room answers to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
}

// ── Alerts ──────────────────────────────────────────────────────────────────

/// One active alert from `GET /alerts` (core's `hc_types::alert::Alert`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub id: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub severity: String,
    #[serde(default)]
    pub raised_at: String,
    /// Present once someone has seen it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acknowledged: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    /// A time rule's alert: which alert rule raised it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
    /// A plugin's alert: which of its kinds, and what it is about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// The alert's one button (`POST /alerts/{id}/press`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<Value>,
}

impl Alert {
    /// Open: nobody has acknowledged it yet.
    pub fn is_open(&self) -> bool {
        self.acknowledged.as_ref().is_none_or(Value::is_null)
    }
}

// ── Rules ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub error: Option<String>,
    /// Externally tagged trigger enum: `{"DeviceStateChanged":{...}}` or a
    /// bare string such as `"ManualTrigger"`.
    #[serde(default)]
    pub trigger: Option<Value>,
    /// Extra triggers, OR'd with `trigger`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triggers: Vec<Value>,
    /// Its checks and steps as core sends them, for the summary.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<Value>,
}

/// The variant name of an externally tagged enum value: the single key of
/// an object, or the string itself for a unit variant.
pub fn variant_name(value: &Value) -> Option<&str> {
    match value {
        Value::String(s) => Some(s.as_str()),
        Value::Object(map) if map.len() == 1 => map.keys().next().map(String::as_str),
        // Older shapes carried `{"type": "..."}`.
        Value::Object(map) => map.get("type").and_then(Value::as_str),
        _ => None,
    }
}

impl Rule {
    /// Every trigger's variant name, primary first.
    pub fn trigger_names(&self) -> Vec<&str> {
        self.trigger
            .iter()
            .chain(self.triggers.iter())
            .filter_map(variant_name)
            .collect()
    }

    /// Short label for the list: the primary trigger, plus a count of
    /// the extra ones. The screens say it in words (`app::rule_words`).
    #[cfg(test)]
    pub fn trigger_label(&self) -> String {
        let names = self.trigger_names();
        match names.as_slice() {
            [] => "-".to_string(),
            [one] => one.to_string(),
            [first, rest @ ..] => format!("{first} +{}", rest.len()),
        }
    }
}

/// One entry of `GET /automations/{id}/history` (core's
/// `hc_core::engine::RuleFiring`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleFiring {
    pub timestamp: String,
    #[serde(default)]
    pub trigger_type: String,
    #[serde(default)]
    pub trigger_context: Value,
    #[serde(default)]
    pub outcome: FireOutcome,
    #[serde(default)]
    pub conditions: Vec<ConditionTrace>,
    #[serde(default)]
    pub actions: Vec<ActionTrace>,
    #[serde(default)]
    pub eval_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FireOutcome {
    /// `fired`, `condition_failed`, `cooldown`, `paused`,
    /// `required_expression_failed`, `trigger_gate_failed` or `skipped`.
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining_secs: Option<u64>,
}

impl FireOutcome {
    pub fn fired(&self) -> bool {
        self.kind == "fired"
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConditionTrace {
    #[serde(default)]
    pub condition_type: String,
    #[serde(default)]
    pub passed: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActionTrace {
    #[serde(default)]
    pub index: usize,
    #[serde(default)]
    pub action_type: String,
    #[serde(default)]
    pub description: String,
    /// `{"status":"ok"|"error"|"skipped", "message"?}`.
    #[serde(default)]
    pub outcome: Value,
    #[serde(default)]
    pub duration_ms: u64,
}

impl ActionTrace {
    pub fn status(&self) -> &str {
        self.outcome
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("")
    }
}

impl RuleFiring {
    /// Actions that ran without error.
    pub fn actions_ok(&self) -> usize {
        self.actions.iter().filter(|a| a.status() == "ok").count()
    }

    pub fn actions_failed(&self) -> usize {
        self.actions
            .iter()
            .filter(|a| a.status() == "error")
            .count()
    }

    /// One-word summary of the outcome for list rows.
    pub fn outcome_label(&self) -> String {
        match self.outcome.kind.as_str() {
            "" => "unknown".to_string(),
            "condition_failed" => match self.outcome.at_index {
                Some(i) => format!("condition {} failed", i + 1),
                None => "condition failed".to_string(),
            },
            other => other.replace('_', " "),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleGroup {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub rule_ids: Vec<String>,
}

// ── System ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SystemStatus {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub uptime_seconds: u64,
    #[serde(default)]
    pub started_at: String,
    #[serde(default)]
    pub rules_total: usize,
    #[serde(default)]
    pub rules_enabled: usize,
    #[serde(default)]
    pub devices_total: usize,
    #[serde(default)]
    pub plugins_active: usize,
    #[serde(default)]
    pub state_db_bytes: u64,
    #[serde(default)]
    pub history_db_bytes: u64,
    #[serde(default)]
    pub timezone: Option<String>,
    /// `"°C"` or `"°F"`.
    #[serde(default)]
    pub temperature_unit: Option<String>,
    #[serde(default)]
    pub last_backup_at: Option<String>,
}

/// How to show temperatures. Values on the bus are always °C.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TempUnit {
    #[default]
    Celsius,
    Fahrenheit,
}

impl TempUnit {
    pub fn parse(s: &str) -> Self {
        let t = s.trim().trim_start_matches('°').to_ascii_lowercase();
        if t == "f" || t == "fahrenheit" {
            TempUnit::Fahrenheit
        } else {
            TempUnit::Celsius
        }
    }

    /// Convert a °C reading to this unit.
    pub fn convert_celsius(self, c: f64) -> f64 {
        match self {
            TempUnit::Celsius => c,
            TempUnit::Fahrenheit => c * 9.0 / 5.0 + 32.0,
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            TempUnit::Celsius => "°C",
            TempUnit::Fahrenheit => "°F",
        }
    }

    /// Format a °C value, e.g. `21.5°C` or `70.7°F`.
    pub fn format(self, c: f64) -> String {
        format!("{:.1}{}", self.convert_celsius(c), self.symbol())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogLine {
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub level: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub fields: Value,
}

// ── Plugins ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginNotice {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub level: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub remedy: Option<String>,
}

/// One entry of `GET /plugins` (core's `hc_api::PluginRecord`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginRecord {
    pub plugin_id: String,
    #[serde(default)]
    pub registered_at: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub managed: bool,
    #[serde(default)]
    pub restart_count: u64,
    #[serde(default)]
    pub device_count: u64,
    #[serde(default)]
    pub log_level: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub installed_version: Option<String>,
    #[serde(default)]
    pub last_heartbeat: Option<String>,
    #[serde(default)]
    pub supports_management: bool,
    #[serde(default)]
    pub notices: Vec<PluginNotice>,
}

// Plugin capabilities (mirror of hc-types::plugin_capabilities).

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Capabilities {
    #[serde(default)]
    pub spec: String,
    #[serde(default)]
    pub plugin_id: String,
    #[serde(default)]
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub params: Option<Value>,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub cancelable: bool,
    #[serde(default)]
    pub concurrency: Concurrency,
    #[serde(default)]
    pub item_key: Option<String>,
    #[serde(default)]
    pub requires_role: RequiresRole,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// The action does something hard to undo; ask before running it.
    #[serde(default)]
    pub danger: bool,
}

impl Action {
    pub fn display_label(&self) -> &str {
        if self.label.is_empty() {
            &self.id
        } else {
            &self.label
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Concurrency {
    #[default]
    Multi,
    Single,
}

impl Concurrency {
    pub fn as_str(&self) -> &'static str {
        match self {
            Concurrency::Multi => "multi",
            Concurrency::Single => "single",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RequiresRole {
    Admin,
    #[default]
    User,
    ReadOnly,
    SignedIn,
}

impl RequiresRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            RequiresRole::Admin => "admin",
            RequiresRole::User => "user",
            RequiresRole::ReadOnly => "read_only",
            RequiresRole::SignedIn => "signed_in",
        }
    }
}

// ── Events ──────────────────────────────────────────────────────────────────

/// One row of the event list, built from `GET /events` or a live frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEntry {
    /// The event's `type`, or for `custom` events its `event_type`.
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub plugin_id: Option<String>,
    #[serde(default)]
    pub device_id: Option<String>,
    #[serde(default)]
    pub rule_name: Option<String>,
    /// True when the event came in as `{"type":"custom"}`.
    #[serde(default)]
    pub custom: bool,
    #[serde(default)]
    pub event_detail: Option<String>,
    /// Local, increasing number given when the row is added; the stable
    /// id the Events panel selects by.
    #[serde(skip)]
    pub seq: u64,
}

// ── Audit ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditActorType {
    User,
    ApiKey,
    LocalAdmin,
    IpWhitelist,
    System,
    Anonymous,
    #[serde(other)]
    Other,
}

impl AuditActorType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuditActorType::User => "user",
            AuditActorType::ApiKey => "api_key",
            AuditActorType::LocalAdmin => "local_admin",
            AuditActorType::IpWhitelist => "ip_whitelist",
            AuditActorType::System => "system",
            AuditActorType::Anonymous => "anonymous",
            AuditActorType::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditResult {
    Success,
    Denied,
    Error,
    #[serde(other)]
    Other,
}

impl AuditResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuditResult::Success => "success",
            AuditResult::Denied => "denied",
            AuditResult::Error => "error",
            AuditResult::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    #[serde(default)]
    pub id: Option<i64>,
    pub ts: String,
    pub actor_type: AuditActorType,
    #[serde(default)]
    pub actor_id: Option<String>,
    #[serde(default)]
    pub actor_label: String,
    #[serde(default)]
    pub event_type: String,
    #[serde(default)]
    pub scope_used: Option<String>,
    #[serde(default)]
    pub target_kind: Option<String>,
    #[serde(default)]
    pub target_id: Option<String>,
    #[serde(default)]
    pub correlation_id: Option<String>,
    #[serde(default)]
    pub ip: Option<String>,
    #[serde(default)]
    pub user_agent: Option<String>,
    pub result: AuditResult,
    #[serde(default)]
    pub detail: Value,
}

impl AuditEntry {
    /// Stable id for selection; falls back to the timestamp.
    pub fn key(&self) -> String {
        match self.id {
            Some(id) => id.to_string(),
            None => format!("{}|{}", self.ts, self.event_type),
        }
    }
}

// ── Modes ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeConfig {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub on_event: Option<String>,
    #[serde(default)]
    pub off_event: Option<String>,
    #[serde(default)]
    pub on_offset_minutes: Option<i32>,
    #[serde(default)]
    pub off_offset_minutes: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeRecord {
    pub config: ModeConfig,
    #[serde(default)]
    pub state: Option<DeviceState>,
}

// ── Matter ──────────────────────────────────────────────────────────────────

/// One commissioned node from `GET /plugins/matter/nodes`. The plugin
/// publishes `{node_id, device_count}`; anything else it adds is kept.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatterNode {
    #[serde(deserialize_with = "string_or_number")]
    pub node_id: String,
    #[serde(default)]
    pub device_count: u64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MatterNodesResponse {
    #[serde(default)]
    pub nodes: Vec<MatterNode>,
}

fn string_or_number<'de, D>(d: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = Value::deserialize(d)?;
    Ok(match v {
        Value::String(s) => s,
        other => other.to_string(),
    })
}

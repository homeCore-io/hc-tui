use crate::api::{
    Area, AuditEntry, Capabilities, DeviceState, EventEntry, HomeCoreClient, LogLine,
    LoginResponse, MatterNode, ModeRecord, PluginRecord, Role, Rule, RuleFiring, RuleGroup, Scene,
    SystemStatus, UserInfo,
};
use crate::cache::{CacheSnapshot, CacheStore};
use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Local, Utc};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{Value, json};
use std::cmp::min;
use std::collections::{HashSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusField {
    Username,
    Password,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceEditField {
    Name,
    Area,
    CanonicalName,
}

#[derive(Debug, Clone)]
pub struct DeviceEditor {
    pub device_id: String,
    pub name: String,
    pub area: String,
    pub canonical_name: String,
    pub field: DeviceEditField,
}

/// Area create/rename editor (modal).
#[derive(Debug, Clone)]
pub struct AreaEditor {
    /// `None` = create mode, `Some(id)` = rename mode.
    pub id: Option<String>,
    pub name: String,
}

/// Which operation the user editor is performing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserEditMode {
    Create,
    EditRole,
    ChangePassword,
}

/// Active field in the user editor modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserEditField {
    Username,
    Password,
    ConfirmPassword,
    CurrentPassword,
    Role,
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceViewMode {
    Grouped,
    Flat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AreasPane {
    AreasList,
    DeviceList,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventsFilterMode {
    All,
    HueInputs,
    Entertainment,
    PluginMetrics,
}

impl EventsFilterMode {
    pub fn title(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::HueInputs => "hue_inputs",
            Self::Entertainment => "entertainment",
            Self::PluginMetrics => "plugin_metrics",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginDetailPanel {
    Overview,
    Actions,
    Diagnostics,
    Metrics,
}

impl PluginDetailPanel {
    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Actions => "Actions",
            Self::Diagnostics => "Diagnostics",
            Self::Metrics => "Metrics",
        }
    }
}

/// Per-stage status pill for the streaming-action modal. Mirrors the web
/// client's pill set; the modal renders this directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamingStage {
    Starting,
    Running,
    AwaitingUser,
    Complete,
    Error,
    Canceled,
    Timeout,
}

impl StreamingStage {
    pub fn label(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::AwaitingUser => "awaiting input",
            Self::Complete => "complete",
            Self::Error => "error",
            Self::Canceled => "canceled",
            Self::Timeout => "timeout",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Complete | Self::Error | Self::Canceled | Self::Timeout)
    }
}

/// State for one in-flight (or just-finished) streaming plugin-action
/// invocation. Held in `App::streaming_action` while the modal is open.
#[derive(Debug, Clone)]
pub struct StreamingAction {
    pub plugin_id: String,
    /// Retained for debug/audit even though the modal renders `label`;
    /// useful when surfacing failed runs in `status` later.
    #[allow(dead_code)]
    pub action_id: String,
    pub label: String,
    /// Server-minted request_id once the start POST returns. `None` while
    /// the start request is still in flight.
    pub request_id: Option<String>,
    pub stage: StreamingStage,
    /// Latest `progress` event payload (if any) — usually carries `pct`
    /// and a `message` field.
    pub last_progress: Option<serde_json::Value>,
    /// Live item list aggregated from `item` events using the action's
    /// optional `item_key` for dedup. We just push everything for the
    /// MVP; if it overflows, we trim the head.
    pub items: Vec<serde_json::Value>,
    pub warnings: Vec<serde_json::Value>,
    /// Currently-open `awaiting_user` prompt event (if any). Modal shows
    /// the `prompt`/`schema` from the payload and lets the user fill in
    /// a free-form text response.
    pub pending_prompt: Option<serde_json::Value>,
    /// Terminal payload — `Some` once a `complete`/`error`/`canceled`/
    /// `timeout` event arrives. Drives the post-run summary.
    pub terminal: Option<serde_json::Value>,
    /// User-typed response buffer for `awaiting_user` prompts.
    pub response_input: String,
    /// Free-form footer message ("starting…", "cancel sent…", etc.)
    pub footer: String,
}

impl StreamingAction {
    pub fn new(plugin_id: String, action_id: String, label: String) -> Self {
        Self {
            plugin_id,
            action_id,
            label,
            request_id: None,
            stage: StreamingStage::Starting,
            last_progress: None,
            items: Vec::new(),
            warnings: Vec::new(),
            pending_prompt: None,
            terminal: None,
            response_input: String::new(),
            footer: "starting…".into(),
        }
    }

    /// Apply one SSE event payload to the modal state.
    pub fn apply_event(&mut self, ev: serde_json::Value) {
        let stage = ev
            .get("stage")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        match stage.as_str() {
            "progress" => {
                self.last_progress = Some(ev);
                self.stage = StreamingStage::Running;
            }
            "item" => {
                if let Some(data) = ev.get("data").cloned() {
                    self.items.push(data);
                    // Cap list to keep the modal renderable.
                    if self.items.len() > 200 {
                        let drop = self.items.len() - 200;
                        self.items.drain(..drop);
                    }
                }
                self.stage = StreamingStage::Running;
            }
            "warning" => {
                self.warnings.push(ev);
                self.stage = StreamingStage::Running;
            }
            "awaiting_user" => {
                self.pending_prompt = Some(ev);
                self.response_input.clear();
                self.stage = StreamingStage::AwaitingUser;
                self.footer = "awaiting your response — type then press R to send".into();
            }
            "complete" => {
                self.terminal = Some(ev);
                self.stage = StreamingStage::Complete;
                self.footer = "complete — press Esc to close".into();
            }
            "error" => {
                self.terminal = Some(ev);
                self.stage = StreamingStage::Error;
                self.footer = "error — press Esc to close".into();
            }
            "canceled" => {
                self.terminal = Some(ev);
                self.stage = StreamingStage::Canceled;
                self.footer = "canceled — press Esc to close".into();
            }
            "timeout" => {
                self.terminal = Some(ev);
                self.stage = StreamingStage::Timeout;
                self.footer = "timed out — press Esc to close".into();
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceSubPanel {
    All,
    MediaPlayers,
    Switches,
    Timers,
}

impl DeviceSubPanel {}

#[derive(Debug, Clone, Default)]
pub struct MediaPlayerCapabilities {
    pub can_play: bool,
    pub can_pause: bool,
    pub can_stop: bool,
    pub can_next: bool,
    pub can_previous: bool,
    pub can_set_volume: bool,
    pub can_mute: bool,
}

#[derive(Debug, Clone)]
pub struct MediaPlayerDetail {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct MediaPlayerModel {
    pub device_id: String,
    pub display_name: String,
    pub canonical_name: Option<String>,
    pub plugin_id: String,
    pub playback_state: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub source: Option<String>,
    pub volume: Option<u8>,
    pub muted: Option<bool>,
    pub position_secs: Option<u64>,
    pub duration_secs: Option<u64>,
    pub capabilities: MediaPlayerCapabilities,
    pub extra_details: Vec<MediaPlayerDetail>,
}

struct MediaPlayerHook {
    matches: fn(&DeviceState) -> bool,
    enrich: fn(&DeviceState, &mut MediaPlayerModel),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceFilterMode {
    All,
    Online,
    Offline,
    LowBattery,
}

impl DeviceFilterMode {
    pub fn title(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Online => "online",
            Self::Offline => "offline",
            Self::LowBattery => "low_battery",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Online,
            Self::Online => Self::Offline,
            Self::Offline => Self::LowBattery,
            Self::LowBattery => Self::All,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceSortMode {
    Name,
    Status,
    LastSeen,
}

impl DeviceSortMode {
    pub fn title(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Status => "status",
            Self::LastSeen => "last_seen",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Name => Self::Status,
            Self::Status => Self::LastSeen,
            Self::LastSeen => Self::Name,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminSubPanel {
    Modes,
    Matter,
    Status,
    Users,
    Logs,
    Events,
    Audit,
    Backup,
}

/// Actions on the Backup admin sub-panel. Exposed to the UI as a fixed
/// list; the user navigates with Up/Down and triggers with Enter.
pub const BACKUP_ACTIONS: &[(&str, &str)] = &[
    ("backup_zip",     "Download system backup (.zip)"),
    ("export_rules",   "Export all rules to JSON"),
    ("export_scenes",  "Export all scenes to JSON"),
    ("import_rules",   "Import rules from ~/.homecore/imports/rules.json"),
    ("import_scenes",  "Import scenes from ~/.homecore/imports/scenes.json"),
];

fn home_dir() -> std::path::PathBuf {
    std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
}

/// Directory where the Backup panel writes exports.
/// Created on first export if it doesn't exist.
pub fn backup_exports_dir() -> std::path::PathBuf {
    home_dir().join(".homecore").join("exports")
}

/// Directory the Backup panel reads imports from.
pub fn backup_imports_dir() -> std::path::PathBuf {
    home_dir().join(".homecore").join("imports")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchEditField {
    Id,
    Label,
}

#[derive(Debug, Clone)]
pub struct SwitchEditor {
    pub id: String,
    pub label: String,
    pub field: SwitchEditField,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerEditField {
    Id,
    Label,
}

#[derive(Debug, Clone)]
pub struct TimerEditor {
    pub id: String,
    pub label: String,
    pub field: TimerEditField,
}

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

/// Glue device type — the 11 unified primitives served by `POST /glue`.
/// Switch + timer overlap with the existing dedicated editors but are
/// included here so the unified creator can produce them too.
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
    // Type-specific fields (only some apply per type — `next_field` skips
    // irrelevant ones based on the chosen GlueType)
    Options,         // select
    Members,         // group
    SourceDeviceId,  // threshold
    SourceAttribute, // threshold
    Threshold,       // threshold
}

#[derive(Debug, Clone)]
pub struct GlueCreator {
    pub glue_type: GlueType,
    pub id: String,
    pub name: String,
    pub options: String,          // comma-separated
    pub members: String,          // comma-separated device IDs
    pub source_device_id: String, // threshold
    pub source_attribute: String, // threshold (default "value")
    pub threshold: String,        // numeric, parsed at save
    pub field: GlueEditField,
    pub error: Option<String>,
}

impl GlueCreator {
    pub fn new() -> Self {
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

    /// Visit fields in canonical order, skipping ones not relevant to
    /// the chosen glue type. Used for Tab/BackTab navigation.
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
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevelFilter {
    Error,
    Warn,
    Info,
    Debug,
}

impl LogLevelFilter {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Warn => "WARN",
            Self::Info => "INFO",
            Self::Debug => "DEBUG",
        }
    }

    pub fn passes(&self, level: &str) -> bool {
        let level_upper = level.to_uppercase();
        match self {
            Self::Error => level_upper == "ERROR",
            Self::Warn => matches!(level_upper.as_str(), "ERROR" | "WARN"),
            Self::Info => matches!(level_upper.as_str(), "ERROR" | "WARN" | "INFO"),
            Self::Debug => true,
        }
    }
}

/// Delete confirmation dialog state.
#[derive(Debug, Clone)]
pub struct DeleteConfirm {
    pub rule_id: String,
    pub rule_name: String,
}

/// Rule filter bar state.
#[derive(Debug, Clone)]
pub struct RuleFilterBar {
    pub tag: String,
    pub trigger: String,
    pub stale: bool,
    pub active_field: RuleFilterField,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleFilterField {
    Tag,
    Trigger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Devices,
    Scenes,
    Areas,
    Rules,
    Plugins,
    Manage,
}

impl Tab {
    pub fn title(self) -> &'static str {
        match self {
            Self::Devices => "Devices",
            Self::Scenes => "Scenes",
            Self::Areas => "Areas",
            Self::Rules => "Rules",
            Self::Plugins => "Plugins",
            Self::Manage => "Manage",
        }
    }
}

pub struct App {
    pub client: HomeCoreClient,
    pub cache: CacheStore,
    pub username: String,
    pub password: String,
    pub focus: FocusField,
    pub authenticated: bool,
    pub current_user: Option<UserInfo>,
    pub status: String,
    pub error: Option<String>,
    pub should_quit: bool,
    pub tab: usize,
    pub selected: usize,
    pub view_mode: DeviceViewMode,
    pub events_filter_mode: EventsFilterMode,
    pub plugin_detail_open: bool,
    pub plugin_detail_plugin_id: Option<String>,
    pub plugin_detail_panel: PluginDetailPanel,
    /// Cached capabilities for the currently-open plugin's Actions panel.
    /// Cleared when the plugin detail screen closes or the plugin changes.
    pub plugin_capabilities: Option<Capabilities>,
    pub plugin_capabilities_loading: bool,
    pub plugin_capabilities_error: Option<String>,
    /// True while a non-streaming plugin action POST is in flight.
    pub action_busy: bool,
    /// Last status message from the Actions panel ("Action X completed",
    /// "Error: …"). Surfaced in the panel footer.
    pub action_status: String,
    /// Active streaming-action modal — `Some` while the user is driving
    /// a streaming plugin action. `None` when no modal is open.
    pub streaming_action: Option<StreamingAction>,
    /// Background-message sender shared with the SSE consumer. Wired from
    /// `main.rs` after the channel is created. `None` only on the login
    /// screen before the WS plumbing is set up.
    pub ws_sender: Option<tokio::sync::mpsc::UnboundedSender<crate::ws::WsAppMsg>>,
    pub devices: Vec<DeviceState>,
    pub scenes: Vec<Scene>,
    pub areas: Vec<Area>,
    pub rules: Vec<Rule>,
    pub events: Vec<EventEntry>,
    pub users: Vec<UserInfo>,
    pub plugins: Vec<PluginRecord>,
    pub matter_nodes: Vec<MatterNode>,
    pub matter_last_action: String,
    pub matter_last_metric: Option<String>,
    pub matter_pending: bool,
    pub matter_last_node_count: usize,
    pub matter_activity: VecDeque<String>,
    pub matter_blocked_reason: Option<String>,
    pub matter_blocked_suggestions: Vec<String>,
    pub ws_connected: bool,
    pub login_in_progress: bool,
    pub login_animation_step: u16,
    pub login_phase: LoginPhase,
    pub device_editor: Option<DeviceEditor>,
    pub area_editor: Option<AreaEditor>,
    pub areas_pane_focus: AreasPane, // left=areas list, right=device list
    pub areas_selected_area_id: Option<String>,
    pub areas_selected_devices: HashSet<String>,
    pub areas_list_selected: usize, // selection index for areas list pane
    pub areas_devices_selected: usize, // selection index for devices list pane
    pub user_editor: Option<UserEditor>,
    pub device_sub: DeviceSubPanel,
    pub device_filter_mode: DeviceFilterMode,
    pub device_sort_mode: DeviceSortMode,
    pub device_search_query: String,
    pub device_search_input_open: bool,
    pub admin_sub: AdminSubPanel,
    /// Last status message for the Backup panel ("backup saved to …", error, etc.).
    pub backup_status: String,
    /// True while a backup/export/import action is in flight; gates re-fire.
    pub backup_busy: bool,
    /// Audit entries for the current page. Newest-first per the server.
    pub audit_entries: Vec<AuditEntry>,
    /// True while a /audit query is in flight.
    pub audit_loading: bool,
    /// Last error from the audit query, surfaced in the panel footer.
    pub audit_error: Option<String>,
    /// Pagination offset (rows skipped). 0 = first page.
    pub audit_offset: u32,
    /// Page size; fixed at 100 for now.
    pub audit_limit: u32,
    /// Selected row's index within audit_entries that has its detail
    /// JSON expanded. None = no row expanded.
    pub audit_expanded_idx: Option<usize>,
    pub switches: Vec<DeviceState>,
    pub timers: Vec<DeviceState>,
    pub modes: Vec<ModeRecord>,
    pub switch_editor: Option<SwitchEditor>,
    pub timer_editor: Option<TimerEditor>,
    pub mode_editor: Option<ModeEditor>,
    /// Unified creator for the 11 glue device types. Open via `g` on
    /// the Manage tab; submit creates via `POST /glue`.
    pub glue_creator: Option<GlueCreator>,
    pub matter_commission_editor: Option<MatterCommissionEditor>,

    // Rules tab features
    pub rule_filter_tag: String,
    pub rule_filter_trigger: String,
    pub rule_filter_stale: bool,
    pub rule_filter_bar: Option<RuleFilterBar>,
    pub rule_selected_ids: HashSet<String>,
    pub rule_bulk_select_mode: bool,
    pub fire_history_open: bool,
    pub fire_history_rule_id: Option<String>,
    pub fire_history: Vec<RuleFiring>,
    pub rule_delete_confirm: Option<DeleteConfirm>,
    /// Read-only rule detail screen — opens on `Enter` from the rule
    /// list, full-screen replacement of the list view. RON pane shows
    /// the on-disk file verbatim; fire history pane shows the last N
    /// firings inline.
    pub rule_detail_open: bool,
    pub rule_detail_id: Option<String>,
    pub rule_detail_ron: Option<String>,
    pub rule_detail_history: Option<Vec<RuleFiring>>,
    /// Vertical scroll offset (in lines) for the RON pane.
    pub rule_detail_scroll: u16,
    pub rule_detail_loading: bool,
    pub rule_detail_error: Option<String>,
    pub groups_open: bool,
    pub groups: Vec<RuleGroup>,
    pub groups_selected: usize,

    // Logs tab
    pub log_lines: VecDeque<LogLine>,
    pub log_level_filter: LogLevelFilter,
    pub log_module_filter: String,
    pub log_paused: bool,
    pub log_scroll_offset: usize,
    pub log_ws_connected: bool,
    pub log_module_input_open: bool,
    pub log_module_input: String,

    // System Status tab
    pub system_status: Option<SystemStatus>,
    pub system_status_last_refresh: Option<String>,

    // Time display toggle
    pub time_utc: bool,
}

pub struct LoginWorkflowResult {
    pub auth: LoginResponse,
    pub snapshot: CacheSnapshot,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginPhase {
    Authenticating,
    Synthesizing,
}

impl App {
    fn media_player_hooks() -> &'static [MediaPlayerHook] {
        &[MediaPlayerHook {
            matches: |device| device.plugin_id.contains("sonos"),
            enrich: |device, model| {
                model.capabilities.can_stop = true;
                model.capabilities.can_next = true;
                model.capabilities.can_previous = true;
                model.capabilities.can_set_volume = true;
                model.capabilities.can_mute = true;

                if let Some(favorites) = device
                    .attributes
                    .get("available_favorites")
                    .and_then(Value::as_array)
                    .map(|items| items.len())
                {
                    model.extra_details.push(MediaPlayerDetail {
                        label: "Favorites".to_string(),
                        value: favorites.to_string(),
                    });
                }

                if let Some(playlists) = device
                    .attributes
                    .get("available_playlists")
                    .and_then(Value::as_array)
                    .map(|items| items.len())
                {
                    model.extra_details.push(MediaPlayerDetail {
                        label: "Playlists".to_string(),
                        value: playlists.to_string(),
                    });
                }
            },
        }]
    }

    fn supported_media_actions(device: &DeviceState) -> Vec<String> {
        device
            .attributes
            .get("supported_actions")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|value| value.to_ascii_lowercase())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    }

    fn media_string_attr(device: &DeviceState, keys: &[&str]) -> Option<String> {
        keys.iter()
            .find_map(|key| device.attributes.get(*key).and_then(Value::as_str))
            .map(ToString::to_string)
    }

    fn media_u64_attr(device: &DeviceState, keys: &[&str]) -> Option<u64> {
        keys.iter().find_map(|key| {
            let value = device.attributes.get(*key)?;
            if let Some(raw) = value.as_u64() {
                return Some(raw);
            }
            value
                .as_f64()
                .and_then(|raw| if raw >= 0.0 { Some(raw as u64) } else { None })
        })
    }

    fn media_volume_attr(device: &DeviceState) -> Option<u8> {
        device
            .attributes
            .get("volume")
            .and_then(Value::as_f64)
            .map(|raw| raw.clamp(0.0, 100.0) as u8)
    }

    fn is_media_player(device: &DeviceState) -> bool {
        device.device_type.as_deref() == Some("media_player")
            || device.attributes.get("kind").and_then(Value::as_str) == Some("media_player")
    }

    pub fn media_player_model(device: &DeviceState) -> Option<MediaPlayerModel> {
        if !Self::is_media_player(device) {
            return None;
        }

        let supported_actions = Self::supported_media_actions(device);
        let supports = |name: &str| supported_actions.iter().any(|value| value == name);
        let playback_state = device
            .attributes
            .get("state")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_ascii_lowercase();
        let mut model = MediaPlayerModel {
            device_id: device.device_id.clone(),
            display_name: device.name.clone(),
            canonical_name: device.canonical_name.clone(),
            plugin_id: device.plugin_id.clone(),
            playback_state: playback_state.clone(),
            title: Self::media_string_attr(device, &["title", "track_title", "media_title"]),
            artist: Self::media_string_attr(device, &["artist", "track_artist", "media_artist"]),
            album: Self::media_string_attr(device, &["album", "track_album", "media_album"]),
            source: Self::media_string_attr(device, &["source", "input_source", "media_source"]),
            volume: Self::media_volume_attr(device),
            muted: device.attributes.get("muted").and_then(Value::as_bool),
            position_secs: Self::media_u64_attr(device, &["position_secs", "position"]),
            duration_secs: Self::media_u64_attr(device, &["duration_secs", "duration"]),
            capabilities: MediaPlayerCapabilities {
                can_play: supported_actions.is_empty() || supports("play"),
                can_pause: supported_actions.is_empty() || supports("pause"),
                can_stop: supports("stop"),
                can_next: supports("next"),
                can_previous: supports("previous"),
                can_set_volume: supports("set_volume")
                    || supports("volume")
                    || device.attributes.contains_key("volume"),
                can_mute: supports("set_mute")
                    || supports("mute")
                    || device.attributes.contains_key("muted"),
            },
            extra_details: Vec::new(),
        };

        for hook in Self::media_player_hooks() {
            if (hook.matches)(device) {
                (hook.enrich)(device, &mut model);
            }
        }

        Some(model)
    }

    fn media_player_toggle_action(device: &DeviceState) -> Option<&'static str> {
        if !Self::is_media_player(device) {
            return None;
        }

        let state = device
            .attributes
            .get("state")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase();

        if matches!(state.as_str(), "playing" | "buffering") {
            Some("stop")
        } else {
            Some("play")
        }
    }

    pub fn visible_media_players(&self) -> Vec<&DeviceState> {
        self.visible_devices()
            .into_iter()
            .filter(|device| Self::is_media_player(device))
            .collect()
    }

    pub fn selected_media_player(&self) -> Option<&DeviceState> {
        self.visible_media_players().get(self.selected).copied()
    }

    pub fn selected_media_player_model(&self) -> Option<MediaPlayerModel> {
        self.selected_media_player()
            .and_then(Self::media_player_model)
    }

    pub fn new(base_url: String, cache: CacheStore) -> Self {
        Self {
            client: HomeCoreClient::new(base_url),
            cache,
            username: String::new(),
            password: String::new(),
            focus: FocusField::Username,
            authenticated: false,
            current_user: None,
            status: "Enter credentials and press Enter".to_string(),
            error: None,
            should_quit: false,
            tab: 0,
            selected: 0,
            view_mode: DeviceViewMode::Grouped,
            events_filter_mode: EventsFilterMode::All,
            plugin_detail_open: false,
            plugin_detail_plugin_id: None,
            plugin_detail_panel: PluginDetailPanel::Overview,
            plugin_capabilities: None,
            plugin_capabilities_loading: false,
            plugin_capabilities_error: None,
            action_busy: false,
            action_status: String::new(),
            streaming_action: None,
            ws_sender: None,
            devices: Vec::new(),
            scenes: Vec::new(),
            areas: Vec::new(),
            rules: Vec::new(),
            events: Vec::new(),
            users: Vec::new(),
            plugins: Vec::new(),
            matter_nodes: Vec::new(),
            matter_last_action: "No Matter operation started".to_string(),
            matter_last_metric: None,
            matter_pending: false,
            matter_last_node_count: 0,
            matter_activity: VecDeque::new(),
            matter_blocked_reason: None,
            matter_blocked_suggestions: Vec::new(),
            ws_connected: false,
            login_in_progress: false,
            login_animation_step: 0,
            login_phase: LoginPhase::Authenticating,
            device_editor: None,
            area_editor: None,
            areas_pane_focus: AreasPane::AreasList,
            areas_selected_area_id: None,
            areas_selected_devices: HashSet::new(),
            areas_list_selected: 0,
            areas_devices_selected: 0,
            user_editor: None,
            device_sub: DeviceSubPanel::All,
            device_filter_mode: DeviceFilterMode::All,
            device_sort_mode: DeviceSortMode::Name,
            device_search_query: String::new(),
            device_search_input_open: false,
            admin_sub: AdminSubPanel::Modes,
            backup_status: String::new(),
            backup_busy: false,
            audit_entries: Vec::new(),
            audit_loading: false,
            audit_error: None,
            audit_offset: 0,
            audit_limit: 100,
            audit_expanded_idx: None,
            switches: Vec::new(),
            timers: Vec::new(),
            modes: Vec::new(),
            switch_editor: None,
            timer_editor: None,
            mode_editor: None,
            glue_creator: None,
            matter_commission_editor: None,

            rule_filter_tag: String::new(),
            rule_filter_trigger: String::new(),
            rule_filter_stale: false,
            rule_filter_bar: None,
            rule_selected_ids: HashSet::new(),
            rule_bulk_select_mode: false,
            fire_history_open: false,
            fire_history_rule_id: None,
            fire_history: Vec::new(),
            rule_delete_confirm: None,
            rule_detail_open: false,
            rule_detail_id: None,
            rule_detail_ron: None,
            rule_detail_history: None,
            rule_detail_scroll: 0,
            rule_detail_loading: false,
            rule_detail_error: None,
            groups_open: false,
            groups: Vec::new(),
            groups_selected: 0,

            log_lines: VecDeque::new(),
            log_level_filter: LogLevelFilter::Info,
            log_module_filter: String::new(),
            log_paused: false,
            log_scroll_offset: 0,
            log_ws_connected: false,
            log_module_input_open: false,
            log_module_input: String::new(),

            system_status: None,
            system_status_last_refresh: None,

            time_utc: false,
        }
    }

    pub fn tabs(&self) -> Vec<Tab> {
        let mut tabs = vec![
            Tab::Devices,
            Tab::Scenes,
            Tab::Areas,
            Tab::Rules,
        ];
        if self.is_admin() {
            tabs.push(Tab::Plugins);
        }
        tabs.push(Tab::Manage);
        tabs
    }

    pub fn active_tab(&self) -> Tab {
        let tabs = self.tabs();
        tabs[self.tab.min(tabs.len().saturating_sub(1))]
    }

    pub fn wants_log_stream(&self) -> bool {
        self.authenticated
            && matches!(self.active_tab(), Tab::Manage)
            && matches!(self.admin_sub, AdminSubPanel::Logs)
    }

    pub fn is_admin(&self) -> bool {
        self.current_user
            .as_ref()
            .map(|u| u.role.is_admin())
            .unwrap_or(false)
    }

    pub fn begin_login(&mut self) -> Option<(String, String)> {
        self.error = None;
        if self.username.trim().is_empty() || self.password.is_empty() {
            self.error = Some("username and password are required".to_string());
            return None;
        }
        self.login_in_progress = true;
        self.login_animation_step = 0;
        self.login_phase = LoginPhase::Authenticating;
        self.status = "Authenticating and syncing state...".to_string();
        Some((self.username.clone(), self.password.clone()))
    }

    pub fn set_login_phase_synthesizing(&mut self) {
        if self.login_in_progress {
            self.login_phase = LoginPhase::Synthesizing;
            self.status = "Synthesizing homeCore...".to_string();
        }
    }

    pub fn tick_login_animation(&mut self) {
        if self.login_in_progress {
            self.login_animation_step = (self.login_animation_step + 1) % 100;
        }
    }

    pub fn login_spinner(&self) -> &'static str {
        const SPINNER: [&str; 8] = ["|", "/", "-", "\\", "|", "/", "-", "\\"];
        SPINNER[(self.login_animation_step as usize) % SPINNER.len()]
    }

    pub fn login_progress_ratio(&self) -> f64 {
        ((self.login_animation_step % 100) as f64) / 100.0
    }

    pub fn apply_login_success(&mut self, result: LoginWorkflowResult) {
        self.client.set_token(result.auth.token);
        self.current_user = Some(result.auth.user);
        self.authenticated = true;
        self.login_in_progress = false;
        self.apply_snapshot(result.snapshot);
        if let Some(warn) = result.warning {
            self.status = format!("Logged in with cached data fallback: {warn}");
        } else {
            self.status = "Login successful and state synchronized".to_string();
        }
    }

    pub fn apply_login_failure(&mut self, error: String) {
        self.login_in_progress = false;
        self.error = Some(error);
        self.status = "Authentication failed".to_string();
    }

    /// Called before the event loop when auto-login is firing in the background.
    pub fn begin_auto_login(&mut self, username: String) {
        self.login_in_progress = true;
        self.login_animation_step = 0;
        self.login_phase = LoginPhase::Authenticating;
        self.status = format!("Auto-logging in as {}…", username);
    }

    /// Pre-fill the username field on the login screen (focus moves to password).
    #[allow(dead_code)]
    pub fn pre_fill_username(&mut self, username: String) {
        self.username = username;
        self.focus = FocusField::Password;
        self.status = "Enter password and press Enter".to_string();
    }

    /// Validate a saved JWT token.  Returns a `LoginWorkflowResult` when the
    /// token is still valid; returns `None` if the server rejects it.
    pub async fn try_restore_session(
        client: HomeCoreClient,
        cache: CacheStore,
        token: String,
    ) -> Option<LoginWorkflowResult> {
        let mut c = client.clone();
        c.set_token(token.clone());
        let user = c.me().await.ok()?;
        let auth = LoginResponse { token, user };
        login_workflow_from_auth(c, cache, auth).await.ok()
    }

    pub async fn refresh_all(&mut self) -> Result<()> {
        self.status = "Refreshing...".to_string();
        self.devices = self.client.list_devices().await?;
        let mut scenes = self.client.list_scenes().await?;
        scenes.extend(hue_scenes_from_devices(&self.devices));
        self.scenes = scenes;
        self.areas = self.client.list_areas().await?;
        self.rules = self.client.list_rules().await?;
        self.events = self.client.list_events(50).await?;
        self.switches = self.client.list_switches().await.unwrap_or_default();
        self.timers = self.client.list_timers().await.unwrap_or_default();
        self.modes = self.client.list_modes().await.unwrap_or_default();
        if self.is_admin() {
            self.users = self.client.list_users().await?;
            self.plugins = self.client.list_plugins().await?;
            self.matter_nodes = self.client.list_matter_nodes().await.unwrap_or_default();
        }
        if self.current_user.is_none() {
            self.current_user = Some(self.client.me().await?);
        }
        self.save_to_cache().await?;
        self.clamp_selection();
        self.status = "Data refreshed and cached".to_string();
        Ok(())
    }

    async fn save_to_cache(&self) -> Result<()> {
        let Some(user) = self.current_user.as_ref() else {
            return Ok(());
        };
        self.cache
            .save_snapshot(&user.username, &self.snapshot())
            .await?;
        Ok(())
    }

    fn snapshot(&self) -> CacheSnapshot {
        CacheSnapshot {
            devices: self.devices.clone(),
            scenes: self.scenes.clone(),
            areas: self.areas.clone(),
            rules: self.rules.clone(),
            events: self.events.clone(),
            users: self.users.clone(),
            plugins: self.plugins.clone(),
            switches: self.switches.clone(),
            timers: self.timers.clone(),
            modes: self.modes.clone(),
        }
    }

    fn apply_snapshot(&mut self, snapshot: CacheSnapshot) {
        self.devices = snapshot.devices;
        self.scenes = snapshot.scenes;
        self.areas = snapshot.areas;
        self.rules = snapshot.rules;
        self.events = snapshot.events;
        self.users = snapshot.users;
        self.plugins = snapshot.plugins;
        self.switches = snapshot.switches;
        self.timers = snapshot.timers;
        self.modes = snapshot.modes;
        self.clamp_selection();
    }

    pub fn on_key_login(&mut self, key: KeyEvent) -> bool {
        if self.login_in_progress {
            if key.code == KeyCode::Esc {
                self.should_quit = true;
            }
            return false;
        }
        match key.code {
            KeyCode::Esc => {
                self.should_quit = true;
                true
            }
            KeyCode::Tab => {
                self.focus = match self.focus {
                    FocusField::Username => FocusField::Password,
                    FocusField::Password => FocusField::Username,
                };
                false
            }
            KeyCode::BackTab => {
                self.focus = match self.focus {
                    FocusField::Username => FocusField::Password,
                    FocusField::Password => FocusField::Username,
                };
                false
            }
            KeyCode::Backspace => {
                match self.focus {
                    FocusField::Username => {
                        self.username.pop();
                    }
                    FocusField::Password => {
                        self.password.pop();
                    }
                }
                false
            }
            KeyCode::Enter => true,
            KeyCode::Char(ch) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    return false;
                }
                match self.focus {
                    FocusField::Username => self.username.push(ch),
                    FocusField::Password => self.password.push(ch),
                }
                false
            }
            _ => false,
        }
    }

    pub fn ws_endpoint(&self) -> String {
        self.client.ws_events_url()
    }

    pub fn ws_logs_endpoint(&self) -> String {
        self.client.ws_logs_url()
    }

    pub fn ws_token(&self) -> Option<String> {
        self.client.token().map(ToString::to_string)
    }

    pub fn on_ws_connected(&mut self) {
        self.ws_connected = true;
        self.status = "Live event stream connected".to_string();
    }

    pub fn on_ws_disconnected(&mut self, reason: String) {
        self.ws_connected = false;
        self.status = format!("Live stream disconnected ({reason})");
    }

    pub fn on_log_ws_connected(&mut self) {
        self.log_ws_connected = true;
    }

    pub fn on_log_ws_disconnected(&mut self, _reason: String) {
        self.log_ws_connected = false;
    }

    pub fn on_log_line(&mut self, line: LogLine) {
        // Apply level filter
        if !self.log_level_filter.passes(&line.level) {
            return;
        }
        // Apply module filter
        if !self.log_module_filter.is_empty()
            && !line.target.contains(&self.log_module_filter)
            && !line.message.contains(&self.log_module_filter)
        {
            return;
        }
        self.log_lines.push_back(line);
        if self.log_lines.len() > 500 {
            self.log_lines.pop_front();
        }
        // Auto-scroll: if not paused, keep offset at end
        if !self.log_paused {
            self.log_scroll_offset = self.log_lines.len().saturating_sub(1);
        }
    }

    pub fn on_ws_event(&mut self, event: Value) {
        let event_type = event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let timestamp = event
            .get("timestamp")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        match event_type.as_str() {
            "device_state_changed" => {
                if let Some(device_id) = event.get("device_id").and_then(Value::as_str) {
                    let current = event
                        .get("current")
                        .and_then(Value::as_object)
                        .cloned()
                        .unwrap_or_default();
                    if let Some(device) = self.devices.iter_mut().find(|d| d.device_id == device_id)
                    {
                        device.attributes = current;
                        if !timestamp.is_empty() {
                            device.last_seen = timestamp.clone();
                        }
                    }
                }
            }
            "device_availability_changed" => {
                if let Some(device_id) = event.get("device_id").and_then(Value::as_str) {
                    let available = event
                        .get("available")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    if let Some(device) = self.devices.iter_mut().find(|d| d.device_id == device_id)
                    {
                        device.available = available;
                        if !timestamp.is_empty() {
                            device.last_seen = timestamp.clone();
                        }
                    }
                }
            }
            "device_name_changed" => {
                if let Some(device_id) = event.get("device_id").and_then(Value::as_str) {
                    if let Some(name) = event.get("current_name").and_then(Value::as_str) {
                        if let Some(device) =
                            self.devices.iter_mut().find(|d| d.device_id == device_id)
                        {
                            device.name = name.to_string();
                        }
                    }
                }
            }
            _ => {}
        }

        let plugin_id = event
            .get("plugin_id")
            .and_then(Value::as_str)
            .map(ToString::to_string);

        let detail = summarize_live_event_detail(&event);

        if event_type == "plugin_metrics" && plugin_id.as_deref() == Some("plugin.matter") {
            self.matter_last_metric = summarize_matter_plugin_metric(&event).or(detail.clone());
            self.update_matter_commission_feedback_from_metric(&event);
        }

        let entry = EventEntry {
            event_type,
            timestamp,
            plugin_id,
            device_id: event
                .get("device_id")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            rule_name: event
                .get("rule_name")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            event_type_custom: event
                .get("event_type")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            event_detail: detail,
        };
        self.events.insert(0, entry);
        self.events.truncate(200);
    }

    /// Returns the currently visible rules (after applying filters).
    pub fn visible_rules(&self) -> Vec<&Rule> {
        self.rules
            .iter()
            .filter(|r| self.rule_matches_filter(r))
            .collect()
    }

    fn rule_matches_filter(&self, rule: &Rule) -> bool {
        if self.rule_filter_stale && rule.error.is_none() {
            return false;
        }
        if !self.rule_filter_tag.is_empty() {
            let tag = &self.rule_filter_tag;
            if !rule.tags.iter().any(|t| t.contains(tag.as_str())) {
                return false;
            }
        }
        if !self.rule_filter_trigger.is_empty() && self.rule_filter_trigger != "all" {
            let trigger_type = rule
                .trigger
                .as_ref()
                .and_then(|t| t.get("type").and_then(Value::as_str))
                .unwrap_or("");
            if !trigger_type.contains(self.rule_filter_trigger.as_str()) {
                return false;
            }
        }
        true
    }

    pub fn selected_rule(&self) -> Option<&Rule> {
        let visible = self.visible_rules();
        visible.get(self.selected).copied()
    }

    pub async fn on_key_authenticated(&mut self, key: KeyEvent) {
        self.error = None;

        // Streaming-action modal swallows all keystrokes while open.
        // Esc closes the modal at any stage; the action keeps running
        // server-side mid-run unless the user explicitly cancels.
        if self.streaming_action.is_some() {
            self.handle_streaming_action_key(key).await;
            return;
        }

        // Read-only rule detail view takes over the Rules tab
        // when open. Esc returns to the list; j/k or arrows scroll the
        // RON pane; r refreshes both fetches.
        if self.rule_detail_open {
            match key.code {
                KeyCode::Esc => self.close_rule_detail(),
                KeyCode::Char('j') | KeyCode::Down => self.scroll_rule_detail(1),
                KeyCode::Char('k') | KeyCode::Up => self.scroll_rule_detail(-1),
                KeyCode::PageDown => self.scroll_rule_detail(10),
                KeyCode::PageUp => self.scroll_rule_detail(-10),
                KeyCode::Home => self.rule_detail_scroll = 0,
                KeyCode::Char('r') => self.refresh_rule_detail().await,
                KeyCode::Char('q') => self.should_quit = true,
                _ => {}
            }
            return;
        }

        // Handle delete confirmation dialog first
        if self.rule_delete_confirm.is_some() {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.confirm_delete_rule().await;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.rule_delete_confirm = None;
                    self.status = "Delete cancelled".to_string();
                }
                _ => {}
            }
            return;
        }

        if self.plugin_detail_open {
            match key.code {
                KeyCode::Esc => {
                    self.plugin_detail_open = false;
                    self.plugin_detail_plugin_id = None;
                    self.plugin_capabilities = None;
                    self.plugin_capabilities_error = None;
                    self.action_status.clear();
                    self.status = "Closed plugin detail".to_string();
                }
                KeyCode::Char('1') => {
                    self.plugin_detail_panel = PluginDetailPanel::Overview;
                }
                KeyCode::Char('2') => {
                    self.plugin_detail_panel = PluginDetailPanel::Actions;
                    self.refresh_plugin_capabilities().await;
                }
                KeyCode::Char('3') => {
                    self.plugin_detail_panel = PluginDetailPanel::Diagnostics;
                }
                KeyCode::Char('4') => {
                    self.plugin_detail_panel = PluginDetailPanel::Metrics;
                }
                KeyCode::Left | KeyCode::BackTab => {
                    self.cycle_plugin_detail_panel(false);
                    if matches!(self.plugin_detail_panel, PluginDetailPanel::Actions) {
                        self.refresh_plugin_capabilities().await;
                    }
                }
                KeyCode::Right | KeyCode::Tab => {
                    self.cycle_plugin_detail_panel(true);
                    if matches!(self.plugin_detail_panel, PluginDetailPanel::Actions) {
                        self.refresh_plugin_capabilities().await;
                    }
                }
                KeyCode::Char('r') => {
                    if let Err(err) = self.refresh_all().await {
                        self.error = Some(err.to_string());
                    }
                }
                KeyCode::Char('b') => {
                    self.discover_bridges_for_selected_plugin().await;
                }
                KeyCode::Char('p') => {
                    self.pair_bridges_for_selected_plugin().await;
                }
                KeyCode::Up if matches!(self.plugin_detail_panel, PluginDetailPanel::Actions) => {
                    if self.selected > 0 {
                        self.selected -= 1;
                    }
                }
                KeyCode::Down if matches!(self.plugin_detail_panel, PluginDetailPanel::Actions) => {
                    let len = self
                        .plugin_capabilities
                        .as_ref()
                        .map(|c| c.actions.len())
                        .unwrap_or(0);
                    if self.selected + 1 < len {
                        self.selected += 1;
                    }
                }
                KeyCode::Enter if matches!(self.plugin_detail_panel, PluginDetailPanel::Actions) => {
                    self.run_selected_plugin_action().await;
                }
                _ => {}
            }
            return;
        }

        if self.device_editor.is_some() {
            self.on_key_device_editor(key).await;
            return;
        }
        if self.area_editor.is_some() {
            self.on_key_area_editor(key).await;
            return;
        }
        if self.user_editor.is_some() {
            self.on_key_user_editor(key).await;
            return;
        }
        if self.switch_editor.is_some() {
            self.on_key_switch_editor(key).await;
            return;
        }
        if self.timer_editor.is_some() {
            self.on_key_timer_editor(key).await;
            return;
        }
        if self.mode_editor.is_some() {
            self.on_key_mode_editor(key).await;
            return;
        }
        if self.glue_creator.is_some() {
            self.on_key_glue_creator(key).await;
            return;
        }
        if self.matter_commission_editor.is_some() {
            self.on_key_matter_commission_editor(key).await;
            return;
        }

        if self.device_search_input_open {
            self.on_key_device_search_input(key);
            return;
        }

        // Rule filter bar
        if self.rule_filter_bar.is_some() {
            self.on_key_rule_filter_bar(key).await;
            return;
        }

        // Log module filter input
        if self.log_module_input_open {
            self.on_key_log_module_input(key);
            return;
        }

        // Groups overlay
        if self.groups_open {
            self.on_key_groups_panel(key).await;
            return;
        }

        // Areas tab two-pane navigation (but allow tab and other global keys)
        if matches!(self.active_tab(), Tab::Areas)
            && !matches!(
                key.code,
                KeyCode::Tab
                    | KeyCode::BackTab
                    | KeyCode::Char('1')
                    | KeyCode::Char('2')
                    | KeyCode::Char('3')
                    | KeyCode::Char('4')
                    | KeyCode::Char('5')
                    | KeyCode::Char('6')
                    | KeyCode::Char('7')
                    | KeyCode::Char('8')
                    | KeyCode::Char('9')
                    | KeyCode::Char('q')
                    | KeyCode::Char('r')
                    | KeyCode::Char('T')
            )
        {
            self.on_key_areas_pane(key).await;
            return;
        }

        // Global T key: toggle time display
        if key.code == KeyCode::Char('T') {
            self.time_utc = !self.time_utc;
            self.status = if self.time_utc {
                "Timestamps: UTC".to_string()
            } else {
                "Timestamps: Local".to_string()
            };
            return;
        }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('r') => match self.active_tab() {
                Tab::Manage if matches!(self.admin_sub, AdminSubPanel::Matter) => {
                    self.refresh_matter_nodes().await;
                }
                Tab::Manage if matches!(self.admin_sub, AdminSubPanel::Status) => {
                    self.refresh_system_status().await;
                }
                Tab::Manage if matches!(self.admin_sub, AdminSubPanel::Audit) => {
                    self.refresh_audit().await;
                }
                _ => {
                    if let Err(err) = self.refresh_all().await {
                        self.error = Some(err.to_string());
                    }
                }
            },
            KeyCode::Left if matches!(self.active_tab(), Tab::Devices) => {
                self.device_sub = match self.device_sub {
                    DeviceSubPanel::All => DeviceSubPanel::Timers,
                    DeviceSubPanel::MediaPlayers => DeviceSubPanel::All,
                    DeviceSubPanel::Switches => DeviceSubPanel::MediaPlayers,
                    DeviceSubPanel::Timers => DeviceSubPanel::Switches,
                };
                self.selected = 0;
                self.error = None;
            }
            KeyCode::Right if matches!(self.active_tab(), Tab::Devices) => {
                self.device_sub = match self.device_sub {
                    DeviceSubPanel::All => DeviceSubPanel::MediaPlayers,
                    DeviceSubPanel::MediaPlayers => DeviceSubPanel::Switches,
                    DeviceSubPanel::Switches => DeviceSubPanel::Timers,
                    DeviceSubPanel::Timers => DeviceSubPanel::All,
                };
                self.selected = 0;
                self.error = None;
            }
            KeyCode::Left if matches!(self.active_tab(), Tab::Manage) => {
                self.admin_sub = match self.admin_sub {
                    AdminSubPanel::Modes => AdminSubPanel::Backup,
                    AdminSubPanel::Matter => AdminSubPanel::Modes,
                    AdminSubPanel::Status => AdminSubPanel::Matter,
                    AdminSubPanel::Users => AdminSubPanel::Status,
                    AdminSubPanel::Logs => AdminSubPanel::Users,
                    AdminSubPanel::Events => AdminSubPanel::Logs,
                    AdminSubPanel::Audit => AdminSubPanel::Events,
                    AdminSubPanel::Backup => AdminSubPanel::Audit,
                };
                self.selected = 0;
                self.error = None;
                if matches!(self.admin_sub, AdminSubPanel::Matter) {
                    self.refresh_matter_nodes().await;
                }
                if matches!(self.admin_sub, AdminSubPanel::Status) {
                    self.refresh_system_status().await;
                }
                if matches!(self.admin_sub, AdminSubPanel::Audit) {
                    self.refresh_audit().await;
                }
            }
            KeyCode::Right if matches!(self.active_tab(), Tab::Manage) => {
                self.admin_sub = match self.admin_sub {
                    AdminSubPanel::Modes => AdminSubPanel::Matter,
                    AdminSubPanel::Matter => AdminSubPanel::Status,
                    AdminSubPanel::Status => AdminSubPanel::Users,
                    AdminSubPanel::Users => AdminSubPanel::Logs,
                    AdminSubPanel::Logs => AdminSubPanel::Events,
                    AdminSubPanel::Events => AdminSubPanel::Audit,
                    AdminSubPanel::Audit => AdminSubPanel::Backup,
                    AdminSubPanel::Backup => AdminSubPanel::Modes,
                };
                self.selected = 0;
                self.error = None;
                if matches!(self.admin_sub, AdminSubPanel::Matter) {
                    self.refresh_matter_nodes().await;
                }
                if matches!(self.admin_sub, AdminSubPanel::Status) {
                    self.refresh_system_status().await;
                }
                if matches!(self.admin_sub, AdminSubPanel::Audit) {
                    self.refresh_audit().await;
                }
            }
            KeyCode::BackTab => {
                let tab_count = self.tabs().len();
                self.tab = (self.tab + tab_count - 1) % tab_count;
                self.selected = 0;
                self.clamp_selection();
                // Reset areas pane when leaving
                self.areas_pane_focus = AreasPane::AreasList;
                self.areas_selected_area_id = None;
                self.areas_selected_devices.clear();
                self.areas_list_selected = 0;
                self.areas_devices_selected = 0;
                // When entering Manage/Status sub-tab, refresh
                if matches!(self.active_tab(), Tab::Manage)
                    && matches!(self.admin_sub, AdminSubPanel::Status)
                {
                    self.refresh_system_status().await;
                }
            }
            KeyCode::Tab => {
                let tab_count = self.tabs().len();
                self.tab = (self.tab + 1) % tab_count;
                self.selected = 0;
                self.clamp_selection();
                // Reset areas pane when leaving
                self.areas_pane_focus = AreasPane::AreasList;
                self.areas_selected_area_id = None;
                self.areas_selected_devices.clear();
                self.areas_list_selected = 0;
                self.areas_devices_selected = 0;
                if matches!(self.active_tab(), Tab::Manage)
                    && matches!(self.admin_sub, AdminSubPanel::Status)
                {
                    self.refresh_system_status().await;
                }
            }
            // Number keys 1-9 for quick tab selection
            KeyCode::Char(c) if c >= '1' && c <= '9' => {
                let tab_count = self.tabs().len();
                let tab_idx = c as usize - '1' as usize;
                if tab_idx < tab_count {
                    self.tab = tab_idx;
                    self.selected = 0;
                    self.clamp_selection();
                    // Reset areas pane when jumping to new tab
                    self.areas_pane_focus = AreasPane::AreasList;
                    self.areas_selected_area_id = None;
                    self.areas_selected_devices.clear();
                    self.areas_list_selected = 0;
                    self.areas_devices_selected = 0;
                    if matches!(self.active_tab(), Tab::Manage)
                        && matches!(self.admin_sub, AdminSubPanel::Status)
                    {
                        self.refresh_system_status().await;
                    }
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                // Exclude Logs sub-panel in Manage
                if matches!(self.active_tab(), Tab::Manage)
                    && matches!(self.admin_sub, AdminSubPanel::Logs)
                {
                    if self.log_paused {
                        let max = self.log_lines.len().saturating_sub(1);
                        self.log_scroll_offset = min(self.log_scroll_offset + 1, max);
                    }
                    return;
                }
                let len = self.active_items_len();
                if len > 0 {
                    self.selected = min(self.selected + 1, len - 1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                // Exclude Logs sub-panel in Manage
                if matches!(self.active_tab(), Tab::Manage)
                    && matches!(self.admin_sub, AdminSubPanel::Logs)
                {
                    if self.log_paused {
                        self.log_scroll_offset = self.log_scroll_offset.saturating_sub(1);
                    }
                    return;
                }
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Enter => match self.active_tab() {
                Tab::Devices => match self.device_sub {
                    DeviceSubPanel::MediaPlayers => self.open_selected_device_editor(),
                    DeviceSubPanel::Switches => {
                        if let Some(sw) = self.switches.get(self.selected) {
                            self.switch_editor = Some(SwitchEditor {
                                id: sw.device_id.clone(),
                                label: sw.name.clone(),
                                field: SwitchEditField::Id,
                            });
                        }
                    }
                    DeviceSubPanel::Timers => {
                        if let Some(t) = self.timers.get(self.selected) {
                            self.timer_editor = Some(TimerEditor {
                                id: t.device_id.clone(),
                                label: t.name.clone(),
                                field: TimerEditField::Id,
                            });
                        }
                    }
                    DeviceSubPanel::All => self.open_selected_device_editor(),
                },
                Tab::Areas => self.open_area_editor_edit(),
                Tab::Rules => self.open_selected_rule_detail().await,
                Tab::Plugins => self.open_plugin_detail(),
                Tab::Manage => {
                    if matches!(self.admin_sub, AdminSubPanel::Matter) {
                        self.reinterview_selected_matter_node().await;
                    } else if matches!(self.admin_sub, AdminSubPanel::Users) {
                        if self.is_admin() {
                            self.open_user_editor_create();
                        } else {
                            self.open_user_editor_role();
                        }
                    } else if matches!(self.admin_sub, AdminSubPanel::Audit) {
                        self.audit_toggle_expanded();
                    } else if matches!(self.admin_sub, AdminSubPanel::Backup) {
                        self.run_selected_backup_action().await;
                    } else {
                        self.open_manage_editor();
                    }
                }
                _ => {}
            },
            KeyCode::Char('n') => match self.active_tab() {
                Tab::Devices => match self.device_sub {
                    DeviceSubPanel::MediaPlayers => self.media_player_next().await,
                    DeviceSubPanel::Switches => {
                        self.switch_editor = Some(SwitchEditor {
                            id: String::new(),
                            label: String::new(),
                            field: SwitchEditField::Id,
                        });
                    }
                    DeviceSubPanel::Timers => {
                        self.timer_editor = Some(TimerEditor {
                            id: String::new(),
                            label: String::new(),
                            field: TimerEditField::Id,
                        });
                    }
                    DeviceSubPanel::All => {}
                },
                Tab::Areas => self.open_area_editor_create(),
                Tab::Manage => {
                    if matches!(self.admin_sub, AdminSubPanel::Matter) {
                        self.open_matter_commission_editor();
                    } else if matches!(self.admin_sub, AdminSubPanel::Users) {
                        if self.is_admin() {
                            self.open_user_editor_create();
                        }
                    } else if matches!(self.admin_sub, AdminSubPanel::Audit) {
                        self.audit_next_page().await;
                    } else {
                        self.open_manage_editor();
                    }
                }
                _ => {}
            },
            KeyCode::Char('d') => match self.active_tab() {
                Tab::Devices => match self.device_sub {
                    DeviceSubPanel::MediaPlayers => {}
                    DeviceSubPanel::Switches => self.delete_selected_device_switch().await,
                    DeviceSubPanel::Timers => self.delete_selected_device_timer().await,
                    DeviceSubPanel::All => self.delete_selected_device().await,
                },
                Tab::Areas => self.delete_selected_area().await,
                Tab::Plugins => self.deregister_selected_plugin().await,
                Tab::Manage if matches!(self.admin_sub, AdminSubPanel::Logs) => {
                    self.log_lines.clear();
                    self.log_scroll_offset = 0;
                    self.status = "Log buffer cleared".to_string();
                }
                Tab::Manage => {
                    if matches!(self.admin_sub, AdminSubPanel::Users) {
                        self.delete_selected_user().await;
                    } else if matches!(self.admin_sub, AdminSubPanel::Matter) {
                        self.remove_selected_matter_node().await;
                    } else {
                        self.delete_selected_manage_item().await;
                    }
                }
                Tab::Rules => self.disable_selected_rule().await,
                _ => {}
            },
            KeyCode::Char('D') => match self.active_tab() {
                Tab::Rules => {
                    if self.rule_bulk_select_mode && !self.rule_selected_ids.is_empty()
                    {
                        self.bulk_disable_rules().await;
                    } else {
                        self.disable_selected_rule().await;
                    }
                }
                _ => {}
            },
            KeyCode::Char('p') => match self.active_tab() {
                Tab::Devices if matches!(self.device_sub, DeviceSubPanel::MediaPlayers) => {
                    self.media_player_play_pause().await
                }
                Tab::Manage if matches!(self.admin_sub, AdminSubPanel::Users) => {
                    self.open_user_editor_password()
                }
                Tab::Manage if matches!(self.admin_sub, AdminSubPanel::Audit) => {
                    self.audit_prev_page().await;
                }
                Tab::Manage if matches!(self.admin_sub, AdminSubPanel::Logs) => {
                    self.log_paused = !self.log_paused;
                    if !self.log_paused {
                        self.log_scroll_offset = self.log_lines.len().saturating_sub(1);
                    }
                    self.status = if self.log_paused {
                        "Log stream paused".to_string()
                    } else {
                        "Log stream resumed".to_string()
                    };
                }
                _ => {}
            },
            KeyCode::Char(' ') => match self.active_tab() {
                Tab::Devices => self.toggle_lock_or_switch().await,
                Tab::Rules => self.toggle_rule_selection(),
                Tab::Manage if matches!(self.admin_sub, AdminSubPanel::Logs) => {
                    self.log_paused = !self.log_paused;
                    if !self.log_paused {
                        self.log_scroll_offset = self.log_lines.len().saturating_sub(1);
                    }
                }
                _ => {}
            },
            KeyCode::Char('t') => match self.active_tab() {
                Tab::Devices => self.toggle_selected_device().await,
                _ => {}
            },
            KeyCode::Char('m')
                if matches!(self.active_tab(), Tab::Devices)
                    && matches!(self.device_sub, DeviceSubPanel::MediaPlayers) =>
            {
                self.media_player_toggle_mute().await;
            }
            KeyCode::Char('x')
                if matches!(self.active_tab(), Tab::Devices)
                    && matches!(self.device_sub, DeviceSubPanel::MediaPlayers) =>
            {
                self.media_player_stop().await;
            }
            KeyCode::Char('b')
                if matches!(self.active_tab(), Tab::Devices)
                    && matches!(self.device_sub, DeviceSubPanel::MediaPlayers) =>
            {
                self.media_player_previous().await;
            }
            KeyCode::Char('v') => {
                if matches!(self.active_tab(), Tab::Devices) {
                    self.view_mode = match self.view_mode {
                        DeviceViewMode::Grouped => DeviceViewMode::Flat,
                        DeviceViewMode::Flat => DeviceViewMode::Grouped,
                    };
                    self.selected = 0;
                }
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                if matches!(self.active_tab(), Tab::Devices) {
                    if matches!(self.device_sub, DeviceSubPanel::MediaPlayers) {
                        self.media_player_adjust_volume(5).await;
                    } else {
                        self.adjust_brightness(1).await;
                    }
                }
            }
            KeyCode::Char('-') => {
                if matches!(self.active_tab(), Tab::Devices) {
                    if matches!(self.device_sub, DeviceSubPanel::MediaPlayers) {
                        self.media_player_adjust_volume(-5).await;
                    } else {
                        self.adjust_brightness(-1).await;
                    }
                }
            }
            KeyCode::Char('l') => {
                if matches!(self.active_tab(), Tab::Devices) {
                    self.lock_device(true).await;
                }
            }
            KeyCode::Char('u') => {
                if matches!(self.active_tab(), Tab::Devices) {
                    self.lock_device(false).await;
                }
            }
            KeyCode::Char('a') => {
                if matches!(self.active_tab(), Tab::Scenes) {
                    self.activate_selected_scene().await;
                }
            }
            KeyCode::Char('f') => {
                match self.active_tab() {
                    Tab::Devices if matches!(self.device_sub, DeviceSubPanel::All) => {
                        self.device_filter_mode = self.device_filter_mode.next();
                        self.selected = 0;
                        self.clamp_selection();
                        self.status = format!("Device filter: {}", self.device_filter_mode.title());
                    }
                    Tab::Manage if matches!(self.admin_sub, AdminSubPanel::Events) => {
                        self.events_filter_mode = match self.events_filter_mode {
                            EventsFilterMode::All => EventsFilterMode::HueInputs,
                            EventsFilterMode::HueInputs => EventsFilterMode::Entertainment,
                            EventsFilterMode::Entertainment => EventsFilterMode::PluginMetrics,
                            EventsFilterMode::PluginMetrics => EventsFilterMode::All,
                        };
                        self.selected = 0;
                        self.clamp_selection();
                        self.status = format!("Events filter: {}", self.events_filter_mode.title());
                    }
                    Tab::Rules => {
                        // Toggle filter bar
                        if self.rule_filter_bar.is_none() {
                            self.rule_filter_bar = Some(RuleFilterBar {
                                tag: self.rule_filter_tag.clone(),
                                trigger: self.rule_filter_trigger.clone(),
                                stale: self.rule_filter_stale,
                                active_field: RuleFilterField::Tag,
                            });
                        } else {
                            self.rule_filter_bar = None;
                        }
                    }
                    _ => {}
                }
            }
            KeyCode::Char('h') | KeyCode::Char('H') => {
                if matches!(self.active_tab(), Tab::Rules) {
                    self.open_fire_history().await;
                }
            }
            KeyCode::Char('c') | KeyCode::Char('C') => {
                if matches!(self.active_tab(), Tab::Rules) {
                    self.clone_selected_rule().await;
                } else if matches!(self.active_tab(), Tab::Manage)
                    && matches!(self.admin_sub, AdminSubPanel::Matter)
                {
                    self.open_matter_commission_editor();
                } else if matches!(self.active_tab(), Tab::Manage)
                    && matches!(self.admin_sub, AdminSubPanel::Logs)
                {
                    self.log_lines.clear();
                    self.log_scroll_offset = 0;
                    self.status = "Log buffer cleared".to_string();
                }
            }
            KeyCode::Char('e') => match self.active_tab() {
                Tab::Rules => {
                    if self.rule_bulk_select_mode && !self.rule_selected_ids.is_empty()
                    {
                        self.bulk_enable_rules().await;
                    } else {
                        self.enable_selected_rule().await;
                    }
                }
                Tab::Manage if matches!(self.admin_sub, AdminSubPanel::Logs) => {
                    self.log_level_filter = LogLevelFilter::Error;
                    self.status = "Log level: ERROR".to_string();
                }
                _ => {}
            },
            KeyCode::Char('E') => match self.active_tab() {
                Tab::Rules => {
                    if self.rule_bulk_select_mode && !self.rule_selected_ids.is_empty()
                    {
                        self.bulk_enable_rules().await;
                    } else {
                        self.enable_selected_rule().await;
                    }
                }
                _ => {}
            },
            KeyCode::Char('w') => {
                if matches!(self.active_tab(), Tab::Manage)
                    && matches!(self.admin_sub, AdminSubPanel::Logs)
                {
                    self.log_level_filter = LogLevelFilter::Warn;
                    self.status = "Log level: WARN".to_string();
                }
            }
            KeyCode::Char('i') => {
                if matches!(self.active_tab(), Tab::Manage)
                    && matches!(self.admin_sub, AdminSubPanel::Matter)
                {
                    self.reinterview_selected_matter_node().await;
                } else if matches!(self.active_tab(), Tab::Manage)
                    && matches!(self.admin_sub, AdminSubPanel::Logs)
                {
                    self.log_level_filter = LogLevelFilter::Info;
                    self.status = "Log level: INFO".to_string();
                }
            }
            KeyCode::Char('/') => {
                if matches!(self.active_tab(), Tab::Manage)
                    && matches!(self.admin_sub, AdminSubPanel::Logs)
                {
                    self.log_module_input_open = true;
                    self.log_module_input = self.log_module_filter.clone();
                } else if matches!(self.active_tab(), Tab::Devices)
                    && matches!(self.device_sub, DeviceSubPanel::All)
                {
                    self.device_search_input_open = true;
                    self.status = format!("Device search: {}", self.device_search_query);
                }
            }
            KeyCode::Char('g') | KeyCode::Char('G') => {
                if matches!(self.active_tab(), Tab::Rules) {
                    self.open_groups_panel().await;
                } else if matches!(self.active_tab(), Tab::Manage) {
                    self.glue_creator = Some(GlueCreator::new());
                    self.status = "Open glue creator".to_string();
                }
            }
            KeyCode::Char('s') => {
                if matches!(self.active_tab(), Tab::Rules) {
                    self.rule_filter_stale = !self.rule_filter_stale;
                    self.selected = 0;
                    self.clamp_selection();
                    self.status = if self.rule_filter_stale {
                        "Filter: showing stale rules only".to_string()
                    } else {
                        "Filter: showing all rules".to_string()
                    };
                } else if matches!(self.active_tab(), Tab::Devices)
                    && matches!(self.device_sub, DeviceSubPanel::All)
                {
                    self.device_sort_mode = self.device_sort_mode.next();
                    self.selected = 0;
                    self.clamp_selection();
                    self.status = format!("Device sort: {}", self.device_sort_mode.title());
                }
            }
            KeyCode::Delete | KeyCode::Char('x') => {
                if matches!(self.active_tab(), Tab::Rules) {
                    self.initiate_delete_rule();
                }
            }
            KeyCode::Esc => match self.active_tab() {
                Tab::Rules => {
                    if self.fire_history_open {
                        self.fire_history_open = false;
                        self.fire_history_rule_id = None;
                        self.fire_history.clear();
                    } else if self.rule_bulk_select_mode {
                        self.rule_bulk_select_mode = false;
                        self.rule_selected_ids.clear();
                        self.status = "Selection cleared".to_string();
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn on_key_device_search_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.device_search_input_open = false;
                self.status = "Device search canceled".to_string();
            }
            KeyCode::Enter => {
                self.device_search_input_open = false;
                self.selected = 0;
                self.clamp_selection();
                self.status = if self.device_search_query.trim().is_empty() {
                    "Device search cleared".to_string()
                } else {
                    format!("Device search: {}", self.device_search_query)
                };
            }
            KeyCode::Backspace => {
                self.device_search_query.pop();
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.device_search_query.push(ch);
            }
            _ => {}
        }
    }

    // ── Rule filter bar ─────────────────────────────────────────────────

    async fn on_key_rule_filter_bar(&mut self, key: KeyEvent) {
        let Some(bar) = self.rule_filter_bar.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                self.rule_filter_bar = None;
            }
            KeyCode::Tab | KeyCode::Right => {
                bar.active_field = match bar.active_field {
                    RuleFilterField::Tag => RuleFilterField::Trigger,
                    RuleFilterField::Trigger => RuleFilterField::Tag,
                };
            }
            KeyCode::BackTab | KeyCode::Left => {
                bar.active_field = match bar.active_field {
                    RuleFilterField::Tag => RuleFilterField::Trigger,
                    RuleFilterField::Trigger => RuleFilterField::Tag,
                };
            }
            KeyCode::Backspace => match bar.active_field {
                RuleFilterField::Tag => {
                    bar.tag.pop();
                }
                RuleFilterField::Trigger => {
                    bar.trigger.pop();
                }
            },
            KeyCode::Enter => {
                let tag = bar.tag.clone();
                let trigger = bar.trigger.clone();
                let stale = bar.stale;
                self.rule_filter_tag = tag;
                self.rule_filter_trigger = trigger;
                self.rule_filter_stale = stale;
                self.rule_filter_bar = None;
                self.selected = 0;
                self.clamp_selection();
                self.status = "Rule filter applied".to_string();
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                let bar = self.rule_filter_bar.as_mut().unwrap();
                match bar.active_field {
                    RuleFilterField::Tag => bar.tag.push(ch),
                    RuleFilterField::Trigger => bar.trigger.push(ch),
                }
            }
            _ => {}
        }
    }

    // ── Log module filter input ───────────────────────────────────────────────

    fn on_key_log_module_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.log_module_input_open = false;
            }
            KeyCode::Enter => {
                self.log_module_filter = self.log_module_input.trim().to_string();
                self.log_module_input_open = false;
                self.status = if self.log_module_filter.is_empty() {
                    "Log module filter cleared".to_string()
                } else {
                    format!("Log module filter: {}", self.log_module_filter)
                };
            }
            KeyCode::Backspace => {
                self.log_module_input.pop();
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.log_module_input.push(ch);
            }
            _ => {}
        }
    }

    // ── Groups panel ──────────────────────────────────────────────────────────

    async fn open_groups_panel(&mut self) {
        match self.client.list_rule_groups().await {
            Ok(groups) => {
                self.groups = groups;
                self.groups_open = true;
                self.groups_selected = 0;
            }
            Err(e) => {
                self.error = Some(format!("Failed to load groups: {e}"));
            }
        }
    }

    async fn on_key_groups_panel(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.groups_open = false;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.groups.is_empty() {
                    self.groups_selected = min(self.groups_selected + 1, self.groups.len() - 1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.groups_selected = self.groups_selected.saturating_sub(1);
            }
            KeyCode::Char('e') | KeyCode::Char('E') => {
                if let Some(g) = self.groups.get(self.groups_selected).cloned() {
                    match self.client.enable_rule_group(&g.id).await {
                        Ok(_) => self.status = format!("Group '{}' enabled", g.name),
                        Err(e) => self.error = Some(e.to_string()),
                    }
                }
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                if let Some(g) = self.groups.get(self.groups_selected).cloned() {
                    match self.client.disable_rule_group(&g.id).await {
                        Ok(_) => self.status = format!("Group '{}' disabled", g.name),
                        Err(e) => self.error = Some(e.to_string()),
                    }
                }
            }
            KeyCode::Delete | KeyCode::Char('x') => {
                if let Some(g) = self.groups.get(self.groups_selected).cloned() {
                    match self.client.delete_rule_group(&g.id).await {
                        Ok(_) => {
                            self.groups.retain(|gr| gr.id != g.id);
                            self.groups_selected = self
                                .groups_selected
                                .min(self.groups.len().saturating_sub(1));
                            self.status = format!("Deleted group '{}'", g.name);
                        }
                        Err(e) => self.error = Some(e.to_string()),
                    }
                }
            }
            _ => {}
        }
    }

    // ── Fire history ──────────────────────────────────────────────────────────

    async fn open_fire_history(&mut self) {
        let Some(rule) = self.selected_rule().cloned() else {
            return;
        };
        match self.client.get_rule_history(&rule.id).await {
            Ok(history) => {
                self.fire_history = history;
                self.fire_history_rule_id = Some(rule.id);
                self.fire_history_open = true;
            }
            Err(e) => {
                self.error = Some(format!("Failed to load history: {e}"));
            }
        }
    }

    // ── Rule read-only detail view ──────────────────────────────────────

    /// Open the read-only rule detail view for the currently-selected
    /// rule. Fetches the on-disk RON and recent fire history in
    /// parallel; renders a "loading…" placeholder until both land.
    pub async fn open_selected_rule_detail(&mut self) {
        let Some(rule) = self.selected_rule().cloned() else {
            return;
        };
        self.rule_detail_open = true;
        self.rule_detail_id = Some(rule.id.clone());
        self.rule_detail_ron = None;
        self.rule_detail_history = None;
        self.rule_detail_scroll = 0;
        self.rule_detail_error = None;
        self.rule_detail_loading = true;

        let (ron, history) = tokio::join!(
            self.client.get_rule_ron(&rule.id),
            self.client.get_rule_history(&rule.id),
        );

        // Only apply if the user hasn't already navigated away.
        if self.rule_detail_id.as_deref() != Some(rule.id.as_str()) {
            return;
        }

        match ron {
            Ok(text) => self.rule_detail_ron = Some(text),
            Err(e) => {
                self.rule_detail_error = Some(format!("RON: {e}"));
            }
        }
        match history {
            Ok(h) => self.rule_detail_history = Some(h),
            Err(e) => {
                // History failure is non-fatal — show RON anyway.
                if self.rule_detail_error.is_none() {
                    self.rule_detail_error = Some(format!("history: {e}"));
                }
                self.rule_detail_history = Some(Vec::new());
            }
        }
        self.rule_detail_loading = false;
    }

    /// Re-fetch RON + history for the open detail view. Bound to `r`.
    pub async fn refresh_rule_detail(&mut self) {
        let Some(id) = self.rule_detail_id.clone() else {
            return;
        };
        self.rule_detail_loading = true;
        self.rule_detail_error = None;
        let (ron, history) = tokio::join!(
            self.client.get_rule_ron(&id),
            self.client.get_rule_history(&id),
        );
        if self.rule_detail_id.as_deref() != Some(id.as_str()) {
            return;
        }
        match ron {
            Ok(text) => self.rule_detail_ron = Some(text),
            Err(e) => self.rule_detail_error = Some(format!("RON: {e}")),
        }
        match history {
            Ok(h) => self.rule_detail_history = Some(h),
            Err(e) => {
                if self.rule_detail_error.is_none() {
                    self.rule_detail_error = Some(format!("history: {e}"));
                }
            }
        }
        self.rule_detail_loading = false;
    }

    pub fn close_rule_detail(&mut self) {
        self.rule_detail_open = false;
        self.rule_detail_id = None;
        self.rule_detail_ron = None;
        self.rule_detail_history = None;
        self.rule_detail_scroll = 0;
        self.rule_detail_error = None;
        self.rule_detail_loading = false;
    }

    pub fn scroll_rule_detail(&mut self, delta: i32) {
        let cur = self.rule_detail_scroll as i32;
        let next = (cur + delta).max(0);
        self.rule_detail_scroll = u16::try_from(next).unwrap_or(u16::MAX);
    }

    // ── Rule enable/disable/clone/delete ────────────────────────────────

    async fn enable_selected_rule(&mut self) {
        let Some(rule) = self.selected_rule().cloned() else {
            return;
        };
        match self.client.toggle_rule(&rule.id, true).await {
            Ok(_) => {
                if let Some(r) = self.rules.iter_mut().find(|r| r.id == rule.id) {
                    r.enabled = true;
                }
                self.status = format!("Enabled rule '{}'", rule.name);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    async fn disable_selected_rule(&mut self) {
        let Some(rule) = self.selected_rule().cloned() else {
            return;
        };
        match self.client.toggle_rule(&rule.id, false).await {
            Ok(_) => {
                if let Some(r) = self.rules.iter_mut().find(|r| r.id == rule.id) {
                    r.enabled = false;
                }
                self.status = format!("Disabled rule '{}'", rule.name);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    async fn clone_selected_rule(&mut self) {
        let Some(rule) = self.selected_rule().cloned() else {
            return;
        };
        match self.client.clone_rule(&rule.id).await {
            Ok(cloned) => {
                let name = cloned.name.clone();
                self.rules.push(cloned);
                self.status = format!("Cloned -> \"{}\"", name);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn initiate_delete_rule(&mut self) {
        let Some(rule) = self.selected_rule().cloned() else {
            return;
        };
        self.rule_delete_confirm = Some(DeleteConfirm {
            rule_id: rule.id,
            rule_name: rule.name,
        });
    }

    async fn confirm_delete_rule(&mut self) {
        let Some(confirm) = self.rule_delete_confirm.take() else {
            return;
        };
        match self.client.delete_rule(&confirm.rule_id).await {
            Ok(_) => {
                self.rules.retain(|r| r.id != confirm.rule_id);
                self.clamp_selection();
                self.status = format!("Deleted rule '{}'", confirm.rule_name);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn toggle_rule_selection(&mut self) {
        let Some(rule) = self.selected_rule().cloned() else {
            return;
        };
        if self.rule_selected_ids.contains(&rule.id) {
            self.rule_selected_ids.remove(&rule.id);
        } else {
            self.rule_selected_ids.insert(rule.id);
            self.rule_bulk_select_mode = true;
        }
        if self.rule_selected_ids.is_empty() {
            self.rule_bulk_select_mode = false;
        }
        let count = self.rule_selected_ids.len();
        self.status = format!("{count} rule(s) selected");
    }

    async fn bulk_enable_rules(&mut self) {
        let ids: Vec<String> = self.rule_selected_ids.iter().cloned().collect();
        match self.client.bulk_toggle_rules(&ids, true).await {
            Ok(_) => {
                for r in self.rules.iter_mut() {
                    if ids.contains(&r.id) {
                        r.enabled = true;
                    }
                }
                self.rule_selected_ids.clear();
                self.rule_bulk_select_mode = false;
                self.status = format!("Enabled {} rule(s)", ids.len());
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    async fn bulk_disable_rules(&mut self) {
        let ids: Vec<String> = self.rule_selected_ids.iter().cloned().collect();
        match self.client.bulk_toggle_rules(&ids, false).await {
            Ok(_) => {
                for r in self.rules.iter_mut() {
                    if ids.contains(&r.id) {
                        r.enabled = false;
                    }
                }
                self.rule_selected_ids.clear();
                self.rule_bulk_select_mode = false;
                self.status = format!("Disabled {} rule(s)", ids.len());
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    // ── Backup / export / import ──────────────────────────────────────────────
    //
    // Five fixed actions on the Backup admin sub-panel. Selection is the
    // shared `self.selected` (clamped to BACKUP_ACTIONS.len()); Enter
    // dispatches via this method.

    async fn run_selected_backup_action(&mut self) {
        if self.backup_busy {
            return;
        }
        let Some((key, _)) = BACKUP_ACTIONS.get(self.selected) else {
            return;
        };
        self.backup_busy = true;
        self.backup_status = format!("Running: {key}…");

        let result: Result<String> = match *key {
            "backup_zip" => self.action_backup_zip().await,
            "export_rules" => self.action_export_rules().await,
            "export_scenes" => self.action_export_scenes().await,
            "import_rules" => self.action_import_rules().await,
            "import_scenes" => self.action_import_scenes().await,
            _ => Err(anyhow!("unknown backup action")),
        };
        self.backup_status = match result {
            Ok(msg) => msg,
            Err(e) => format!("Error: {e}"),
        };
        self.backup_busy = false;
    }

    async fn action_backup_zip(&mut self) -> Result<String> {
        let bytes = self.client.backup_zip().await?;
        let dir = backup_exports_dir();
        std::fs::create_dir_all(&dir).context("creating exports dir")?;
        let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let path = dir.join(format!("homecore-backup-{ts}.zip"));
        std::fs::write(&path, &bytes).context("writing backup zip")?;
        Ok(format!(
            "Saved {} ({:.1} KB)",
            path.display(),
            bytes.len() as f64 / 1024.0
        ))
    }

    async fn action_export_rules(&mut self) -> Result<String> {
        let value = self.client.export_rules().await?;
        let dir = backup_exports_dir();
        std::fs::create_dir_all(&dir).context("creating exports dir")?;
        let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let path = dir.join(format!("rules-{ts}.json"));
        let pretty = serde_json::to_string_pretty(&value).context("serializing rules")?;
        std::fs::write(&path, &pretty).context("writing rules export")?;
        let count = value.as_array().map(|a| a.len()).unwrap_or(0);
        Ok(format!("Exported {} rule(s) to {}", count, path.display()))
    }

    async fn action_export_scenes(&mut self) -> Result<String> {
        let value = self.client.export_scenes().await?;
        let dir = backup_exports_dir();
        std::fs::create_dir_all(&dir).context("creating exports dir")?;
        let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let path = dir.join(format!("scenes-{ts}.json"));
        let pretty = serde_json::to_string_pretty(&value).context("serializing scenes")?;
        std::fs::write(&path, &pretty).context("writing scenes export")?;
        let count = value.as_array().map(|a| a.len()).unwrap_or(0);
        Ok(format!("Exported {} scene(s) to {}", count, path.display()))
    }

    async fn action_import_rules(&mut self) -> Result<String> {
        let path = backup_imports_dir().join("rules.json");
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let value: Value = serde_json::from_str(&raw).context("parsing rules JSON")?;
        if !value.is_array() {
            return Err(anyhow!("rules import expects a JSON array"));
        }
        let count = self.client.import_rules(value).await?;
        Ok(format!("Imported {count} rule(s) from {}", path.display()))
    }

    async fn action_import_scenes(&mut self) -> Result<String> {
        let path = backup_imports_dir().join("scenes.json");
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let value: Value = serde_json::from_str(&raw).context("parsing scenes JSON")?;
        if !value.is_array() {
            return Err(anyhow!("scenes import expects a JSON array"));
        }
        let count = self.client.import_scenes(value).await?;
        Ok(format!("Imported {count} scene(s) from {}", path.display()))
    }

    // ── Audit log ─────────────────────────────────────────────────────────────

    /// Pull the current audit page from the server. Resets selection and
    /// closes any expanded detail panel so the user lands on a coherent view.
    async fn refresh_audit(&mut self) {
        self.audit_loading = true;
        self.audit_error = None;
        self.audit_expanded_idx = None;
        match self
            .client
            .list_audit(self.audit_limit, self.audit_offset)
            .await
        {
            Ok(rows) => {
                self.audit_entries = rows;
                self.selected = 0;
                self.clamp_selection();
                self.status = format!(
                    "Audit page {} ({} entries)",
                    self.audit_offset / self.audit_limit + 1,
                    self.audit_entries.len()
                );
            }
            Err(e) => {
                self.audit_error = Some(e.to_string());
                self.audit_entries.clear();
            }
        }
        self.audit_loading = false;
    }

    /// Advance to the next audit page (offset += limit) and refetch.
    async fn audit_next_page(&mut self) {
        // Only advance if the current page is full — otherwise there's
        // nothing on the next page.
        if self.audit_entries.len() < self.audit_limit as usize {
            return;
        }
        self.audit_offset = self.audit_offset.saturating_add(self.audit_limit);
        self.refresh_audit().await;
    }

    /// Step back one audit page; saturates at 0.
    async fn audit_prev_page(&mut self) {
        if self.audit_offset == 0 {
            return;
        }
        self.audit_offset = self.audit_offset.saturating_sub(self.audit_limit);
        self.refresh_audit().await;
    }

    /// Toggle the expanded detail panel for the currently selected row.
    fn audit_toggle_expanded(&mut self) {
        let idx = self.selected;
        if idx >= self.audit_entries.len() {
            return;
        }
        self.audit_expanded_idx = match self.audit_expanded_idx {
            Some(prev) if prev == idx => None,
            _ => Some(idx),
        };
    }

    // ── System Status ─────────────────────────────────────────────────────────

    async fn refresh_system_status(&mut self) {
        match self.client.get_system_status().await {
            Ok(status) => {
                self.system_status_last_refresh = Some(Local::now().format("%H:%M:%S").to_string());
                self.system_status = Some(status);
                self.status = "System status refreshed".to_string();
            }
            Err(e) => {
                self.error = Some(format!("Failed to load system status: {e}"));
            }
        }
    }

    async fn refresh_matter_nodes(&mut self) {
        match self.client.list_matter_nodes().await {
            Ok(nodes) => {
                let prev = self.matter_last_node_count;
                self.matter_nodes = nodes;
                self.matter_last_node_count = self.matter_nodes.len();
                self.clamp_selection();
                self.status = format!("Matter nodes refreshed ({})", self.matter_nodes.len());
                if self.matter_pending && self.matter_nodes.len() > prev {
                    self.matter_pending = false;
                    self.matter_last_action = format!(
                        "Commission completed: inventory {} -> {}",
                        prev,
                        self.matter_nodes.len()
                    );
                }
                self.push_matter_activity(self.status.clone());
                self.error = None;
            }
            Err(err) => {
                let message = format!("Matter list failed: {err}");
                self.matter_last_action = message.clone();
                self.push_matter_activity(message.clone());
                self.error = Some(message);
            }
        }
    }

    async fn commission_matter(
        &mut self,
        pairing_code: Option<String>,
        name: Option<String>,
        room: Option<String>,
        discriminator: Option<u16>,
        passcode: Option<u32>,
    ) {
        self.matter_blocked_reason = None;
        self.matter_blocked_suggestions.clear();

        let mut payload = serde_json::Map::new();
        if let Some(code) = pairing_code {
            payload.insert("pairing_code".to_string(), Value::String(code));
        }
        if let Some(device_name) = name {
            payload.insert("name".to_string(), Value::String(device_name));
        }
        if let Some(area) = room {
            payload.insert("area".to_string(), Value::String(area));
        }
        if let Some(disc) = discriminator {
            payload.insert(
                "discriminator".to_string(),
                Value::Number((disc as u64).into()),
            );
        }
        if let Some(pin) = passcode {
            payload.insert("passcode".to_string(), Value::Number((pin as u64).into()));
        }

        let before = self.matter_nodes.len();

        match self.client.matter_commission(Value::Object(payload)).await {
            Ok(_) => {
                self.error = None;
                self.matter_pending = true;
                self.matter_last_action =
                    "Commission request accepted; waiting for device response".to_string();
                self.push_matter_activity(self.matter_last_action.clone());
                self.refresh_matter_nodes().await;
                if self.error.is_none() {
                    let after = self.matter_nodes.len();
                    if after > before {
                        self.status = format!(
                            "Matter commission accepted; inventory {} -> {}",
                            before, after
                        );
                        self.matter_pending = false;
                        self.matter_last_action = self.status.clone();
                    } else {
                        self.status = format!(
                            "Matter commission accepted; waiting for device response (inventory {})",
                            after
                        );
                        self.matter_last_action = self.status.clone();
                    }
                    self.push_matter_activity(self.status.clone());
                }
            }
            Err(err) => {
                self.matter_pending = false;
                let message = format!("Matter commission failed: {err}");
                self.matter_last_action = message.clone();
                self.push_matter_activity(message.clone());
                self.error = Some(message);
            }
        }
    }

    fn open_matter_commission_editor(&mut self) {
        self.matter_commission_editor = Some(MatterCommissionEditor {
            pairing_code: String::new(),
            name: String::new(),
            room: String::new(),
            discriminator: String::new(),
            passcode: String::new(),
            field: MatterCommissionField::PairingCode,
        });
        self.error = None;
    }

    async fn on_key_matter_commission_editor(&mut self, key: KeyEvent) {
        let Some(editor) = self.matter_commission_editor.as_mut() else {
            return;
        };

        match key.code {
            KeyCode::Esc => {
                self.matter_commission_editor = None;
                self.status = "Matter commission canceled".to_string();
            }
            KeyCode::Tab | KeyCode::BackTab => {
                editor.field = match editor.field {
                    MatterCommissionField::PairingCode => MatterCommissionField::Name,
                    MatterCommissionField::Name => MatterCommissionField::Room,
                    MatterCommissionField::Room => MatterCommissionField::Discriminator,
                    MatterCommissionField::Discriminator => MatterCommissionField::Passcode,
                    MatterCommissionField::Passcode => MatterCommissionField::PairingCode,
                };
            }
            KeyCode::Backspace => match editor.field {
                MatterCommissionField::PairingCode => {
                    editor.pairing_code.pop();
                }
                MatterCommissionField::Name => {
                    editor.name.pop();
                }
                MatterCommissionField::Room => {
                    editor.room.pop();
                }
                MatterCommissionField::Discriminator => {
                    editor.discriminator.pop();
                }
                MatterCommissionField::Passcode => {
                    editor.passcode.pop();
                }
            },
            KeyCode::Enter => {
                self.save_matter_commission_editor().await;
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                match editor.field {
                    MatterCommissionField::PairingCode => editor.pairing_code.push(ch),
                    MatterCommissionField::Name => editor.name.push(ch),
                    MatterCommissionField::Room => editor.room.push(ch),
                    MatterCommissionField::Discriminator => {
                        if ch.is_ascii_digit() {
                            editor.discriminator.push(ch);
                        }
                    }
                    MatterCommissionField::Passcode => {
                        if ch.is_ascii_digit() {
                            editor.passcode.push(ch);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    async fn save_matter_commission_editor(&mut self) {
        let Some(editor) = self.matter_commission_editor.clone() else {
            return;
        };

        let pairing_code = if editor.pairing_code.trim().is_empty() {
            None
        } else {
            Some(editor.pairing_code.trim().to_string())
        };

        let name = if editor.name.trim().is_empty() {
            None
        } else {
            Some(editor.name.trim().to_string())
        };

        let room = if editor.room.trim().is_empty() {
            None
        } else {
            Some(editor.room.trim().to_string())
        };

        let discriminator = if editor.discriminator.trim().is_empty() {
            None
        } else {
            match editor.discriminator.trim().parse::<u16>() {
                Ok(v) => Some(v),
                Err(_) => {
                    self.error =
                        Some("Matter discriminator must be a number (0-65535)".to_string());
                    return;
                }
            }
        };

        let passcode = if editor.passcode.trim().is_empty() {
            None
        } else {
            match editor.passcode.trim().parse::<u32>() {
                Ok(v) => Some(v),
                Err(_) => {
                    self.error = Some("Matter passcode must be a number".to_string());
                    return;
                }
            }
        };

        self.matter_commission_editor = None;
        self.commission_matter(pairing_code, name, room, discriminator, passcode)
            .await;
    }

    async fn reinterview_selected_matter_node(&mut self) {
        let Some(node) = self.matter_nodes.get(self.selected).cloned() else {
            self.error = Some("No Matter node selected for reinterview".to_string());
            return;
        };

        match self.client.matter_reinterview(&node.node_id).await {
            Ok(_) => {
                self.status = format!("Matter reinterview requested for {}", node.node_id);
                self.matter_last_action = self.status.clone();
                self.push_matter_activity(self.status.clone());
                self.error = None;
                self.refresh_matter_nodes().await;
            }
            Err(err) => {
                let message = format!("Matter reinterview failed for {}: {}", node.node_id, err);
                self.matter_last_action = message.clone();
                self.push_matter_activity(message.clone());
                self.error = Some(message);
            }
        }
    }

    async fn remove_selected_matter_node(&mut self) {
        let Some(node) = self.matter_nodes.get(self.selected).cloned() else {
            self.error = Some("No Matter node selected for removal".to_string());
            return;
        };

        match self.client.matter_remove_node(&node.node_id).await {
            Ok(_) => {
                self.status = format!("Matter node removal requested for {}", node.node_id);
                self.matter_last_action = self.status.clone();
                self.push_matter_activity(self.status.clone());
                self.error = None;
                self.refresh_matter_nodes().await;
            }
            Err(err) => {
                let message = format!("Matter remove failed for {}: {}", node.node_id, err);
                self.matter_last_action = message.clone();
                self.push_matter_activity(message.clone());
                self.error = Some(message);
            }
        }
    }

    fn push_matter_activity(&mut self, line: String) {
        let ts = Local::now().format("%H:%M:%S");
        self.matter_activity.push_front(format!("[{ts}] {line}"));
        while self.matter_activity.len() > 8 {
            self.matter_activity.pop_back();
        }
    }

    fn update_matter_commission_feedback_from_metric(&mut self, event: &Value) {
        let phase = event.get("phase").and_then(Value::as_str).unwrap_or("");
        let result = event.get("result").and_then(Value::as_str).unwrap_or("");

        if phase == "commission_blocked" || result == "blocked" {
            self.matter_pending = false;

            let reason_code = event
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let reason_text = humanize_matter_block_reason(reason_code);
            let timeout_suffix = event
                .get("timeout_ms")
                .and_then(Value::as_u64)
                .map(|v| format!(" (scan {}ms)", v))
                .unwrap_or_default();

            let message = format!("Matter commission blocked: {reason_text}{timeout_suffix}");
            self.status = message.clone();
            self.matter_last_action = message.clone();
            self.matter_blocked_reason = Some(reason_text.to_string());
            self.matter_blocked_suggestions = event
                .get("suggestions")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .take(3)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            if self
                .matter_activity
                .front()
                .map(|line| !line.contains(&message))
                .unwrap_or(true)
            {
                self.push_matter_activity(message);
            }
            return;
        }

        if phase == "commission" && result == "ok" {
            self.matter_blocked_reason = None;
            self.matter_blocked_suggestions.clear();
        }
    }

    // ── Devices ───────────────────────────────────────────────────────────────

    /// Returns devices grouped by area, sorted alphabetically. Unassigned devices last.
    /// Devices that should appear in the Devices tab (scene devices are shown in Scenes tab).
    pub fn visible_devices(&self) -> Vec<&DeviceState> {
        let mut visible = self
            .devices
            .iter()
            .filter(|d| !is_hidden_in_devices_view_with_context(d, &self.devices))
            .filter(|d| self.device_matches_filter(d))
            .filter(|d| self.device_matches_search(d))
            .collect::<Vec<_>>();

        match self.device_sort_mode {
            DeviceSortMode::Name => {
                visible.sort_by_key(|d| d.name.to_lowercase());
            }
            DeviceSortMode::Status => {
                visible.sort_by(|a, b| {
                    let sa = self.device_status(a).to_lowercase();
                    let sb = self.device_status(b).to_lowercase();
                    sa.cmp(&sb)
                        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                });
            }
            DeviceSortMode::LastSeen => {
                visible.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
            }
        }

        visible
    }

    fn device_matches_filter(&self, device: &DeviceState) -> bool {
        match self.device_filter_mode {
            DeviceFilterMode::All => true,
            DeviceFilterMode::Online => device.available,
            DeviceFilterMode::Offline => !device.available,
            DeviceFilterMode::LowBattery => Self::device_battery(device)
                .map(|b| b <= 20)
                .unwrap_or(false),
        }
    }

    fn device_matches_search(&self, device: &DeviceState) -> bool {
        let q = self.device_search_query.trim().to_lowercase();
        if q.is_empty() {
            return true;
        }

        device.name.to_lowercase().contains(&q)
            || device.device_id.to_lowercase().contains(&q)
            || device
                .canonical_name
                .as_deref()
                .map(|name| name.to_lowercase().contains(&q))
                .unwrap_or(false)
            || device.plugin_id.to_lowercase().contains(&q)
            || device
                .area
                .as_deref()
                .map(|a| a.to_lowercase().contains(&q))
                .unwrap_or(false)
            || device
                .attributes
                .get("kind")
                .and_then(Value::as_str)
                .map(|k| k.to_lowercase().contains(&q))
                .unwrap_or(false)
    }

    pub fn grouped_devices(&self) -> Vec<(String, Vec<usize>)> {
        let mut map: std::collections::BTreeMap<String, Vec<usize>> =
            std::collections::BTreeMap::new();
        let mut unassigned: Vec<usize> = Vec::new();
        for (i, device) in self.visible_devices().iter().enumerate() {
            match &device.area {
                Some(area) if !area.is_empty() => {
                    map.entry(area.clone()).or_default().push(i);
                }
                _ => unassigned.push(i),
            }
        }
        let mut groups: Vec<(String, Vec<usize>)> = map.into_iter().collect();
        if !unassigned.is_empty() {
            groups.push(("Unassigned".to_string(), unassigned));
        }
        groups
    }

    /// Resolves `self.selected` to a device, accounting for view mode.
    pub fn selected_device(&self) -> Option<&DeviceState> {
        if matches!(self.device_sub, DeviceSubPanel::MediaPlayers) {
            return self.selected_media_player();
        }
        if !matches!(self.device_sub, DeviceSubPanel::All) {
            return None;
        }
        let visible = self.visible_devices();
        if self.view_mode == DeviceViewMode::Grouped {
            let groups = self.grouped_devices();
            let mut flat = 0usize;
            for (_, indices) in &groups {
                for &idx in indices {
                    if flat == self.selected {
                        return visible.get(idx).copied();
                    }
                    flat += 1;
                }
            }
            None
        } else {
            visible.get(self.selected).copied()
        }
    }

    pub fn device_battery(device: &DeviceState) -> Option<u8> {
        for key in &["battery", "battery_level", "battery_percent", "battery_pct"] {
            if let Some(n) = device.attributes.get(*key).and_then(|v| v.as_f64()) {
                return Some(n.clamp(0.0, 100.0) as u8);
            }
        }
        None
    }

    pub fn device_temperature(device: &DeviceState) -> Option<f64> {
        for key in &["temperature", "temp"] {
            if let Some(n) = device.attributes.get(*key).and_then(|v| v.as_f64()) {
                return Some(n);
            }
        }
        None
    }

    pub fn device_humidity(device: &DeviceState) -> Option<f64> {
        device.attributes.get("humidity").and_then(|v| v.as_f64())
    }

    pub fn device_brightness(device: &DeviceState) -> Option<u8> {
        device
            .attributes
            .get("brightness")
            .and_then(|v| v.as_f64())
            .map(|n| {
                if n <= 1.0 {
                    (n * 100.0) as u8
                } else if n <= 100.0 {
                    n as u8
                } else {
                    (n / 255.0 * 100.0) as u8
                }
            })
    }

    pub fn device_lock_state(device: &DeviceState) -> Option<bool> {
        device.attributes.get("locked").and_then(|v| v.as_bool())
    }

    fn open_selected_device_editor(&mut self) {
        let Some(device) = self.selected_device() else {
            return;
        };
        let device_id = device.device_id.clone();
        let name = device.name.clone();
        let area = device.area.clone().unwrap_or_default();
        let canonical_name = device.canonical_name.clone().unwrap_or_default();

        self.device_editor = Some(DeviceEditor {
            device_id,
            name,
            area,
            canonical_name,
            field: DeviceEditField::Name,
        });
    }

    async fn on_key_device_editor(&mut self, key: KeyEvent) {
        let Some(editor) = self.device_editor.as_mut() else {
            return;
        };

        match key.code {
            KeyCode::Esc => {
                self.device_editor = None;
                self.status = "Device edit canceled".to_string();
            }
            KeyCode::Tab | KeyCode::Right => {
                editor.field = match editor.field {
                    DeviceEditField::Name => DeviceEditField::Area,
                    DeviceEditField::Area => DeviceEditField::CanonicalName,
                    DeviceEditField::CanonicalName => DeviceEditField::Name,
                };
            }
            KeyCode::BackTab | KeyCode::Left => {
                editor.field = match editor.field {
                    DeviceEditField::Name => DeviceEditField::CanonicalName,
                    DeviceEditField::Area => DeviceEditField::Name,
                    DeviceEditField::CanonicalName => DeviceEditField::Area,
                };
            }
            KeyCode::Backspace => match editor.field {
                DeviceEditField::Name => {
                    editor.name.pop();
                }
                DeviceEditField::Area => {
                    editor.area.pop();
                }
                DeviceEditField::CanonicalName => {
                    editor.canonical_name.pop();
                }
            },
            KeyCode::Enter => {
                self.save_device_editor().await;
            }
            KeyCode::Char(ch) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    return;
                }
                match editor.field {
                    DeviceEditField::Name => editor.name.push(ch),
                    DeviceEditField::Area => editor.area.push(ch),
                    DeviceEditField::CanonicalName => editor.canonical_name.push(ch),
                }
            }
            _ => {}
        }
    }

    async fn save_device_editor(&mut self) {
        let Some(editor) = self.device_editor.clone() else {
            return;
        };

        let name = editor.name.trim().to_string();
        if name.is_empty() {
            self.error = Some("device name cannot be empty".to_string());
            return;
        }

        let area_value = editor.area.trim().to_string();
        let area = if area_value.is_empty() {
            None
        } else {
            Some(area_value.clone())
        };
        let canonical_value = editor.canonical_name.trim().to_string();
        let canonical_name = if canonical_value.is_empty() {
            None
        } else {
            Some(canonical_value.clone())
        };

        match self
            .client
            .update_device_metadata(
                &editor.device_id,
                &name,
                area.as_deref(),
                canonical_name.as_deref(),
            )
            .await
        {
            Ok(_) => {
                if let Some(device) = self
                    .devices
                    .iter_mut()
                    .find(|device| device.device_id == editor.device_id)
                {
                    device.name = name.clone();
                    device.area = area.clone();
                    device.canonical_name = canonical_name.clone();
                }
                self.device_editor = None;
                self.status = format!("Updated {}", editor.device_id);
                if let Err(err) = self.save_to_cache().await {
                    self.error = Some(err.to_string());
                }
            }
            Err(err) => {
                self.error = Some(err.to_string());
            }
        }
    }

    pub fn device_status(&self, device: &DeviceState) -> String {
        let attrs = &device.attributes;

        // Lock state (ZWave CC 98, door locks)
        if let Some(locked) = attrs.get("locked").and_then(|v| v.as_bool()) {
            return if locked {
                "Locked".to_string()
            } else {
                "Unlocked".to_string()
            };
        }
        // Explicit on/off (binary switch, most smart plugs)
        if let Some(on) = attrs.get("on").and_then(|v| v.as_bool()) {
            return if on {
                "On".to_string()
            } else {
                "Off".to_string()
            };
        }
        // Generic state string
        if let Some(state) = attrs.get("state").and_then(|v| v.as_str()) {
            return normalize_label(state);
        }
        // Dimmer — derive on/off from brightness_pct (Hue) or brightness (ZWave CC 38)
        if let Some(b) = attrs
            .get("brightness_pct")
            .or_else(|| attrs.get("brightness"))
            .and_then(|v| v.as_f64())
        {
            return if b > 0.0 {
                "On".to_string()
            } else {
                "Off".to_string()
            };
        }
        // Contact sensor (open/closed bool)
        if let Some(open) = attrs
            .get("open")
            .or_else(|| attrs.get("contact_open"))
            .and_then(|v| v.as_bool())
        {
            return if open {
                "Open".to_string()
            } else {
                "Closed".to_string()
            };
        }
        // Motion sensor
        if let Some(motion) = attrs.get("motion").and_then(|v| v.as_bool()) {
            return if motion {
                "Motion".to_string()
            } else {
                "Clear".to_string()
            };
        }
        // Thermostat mode
        if let Some(mode) = attrs.get("mode").and_then(|v| v.as_str()) {
            return normalize_label(mode);
        }
        // Window covering position
        if let Some(pos) = attrs.get("position").and_then(|v| v.as_f64()) {
            return if pos >= 99.0 {
                "Open".to_string()
            } else if pos <= 1.0 {
                "Closed".to_string()
            } else {
                format!("{pos:.0}%")
            };
        }
        // Sensor-only devices — show primary reading as status
        if let Some(temp) = attrs
            .get("temperature")
            .or_else(|| attrs.get("temp"))
            .and_then(|v| v.as_f64())
        {
            return format!("{temp:.1}°");
        }
        if let Some(hum) = attrs.get("humidity").and_then(|v| v.as_f64()) {
            return format!("{hum:.0}%rh");
        }
        // Battery-only sensor/status devices (e.g. Hue device_power)
        if let Some(battery) = Self::device_battery(device) {
            return format!("{battery}%");
        }
        if let Some(state) = attrs.get("battery_state").and_then(|v| v.as_str()) {
            return normalize_label(state);
        }
        // Illuminance sensors (e.g. Hue light_level)
        if let Some(illuminance) = attrs
            .get("illuminance")
            .or_else(|| attrs.get("illuminance_lux"))
            .or_else(|| attrs.get("illuminance_raw"))
            .and_then(|v| v.as_f64())
        {
            let unit = attrs
                .get("illuminance_unit")
                .and_then(|v| v.as_str())
                .unwrap_or("lux");
            return if unit.eq_ignore_ascii_case("raw") {
                format!("{illuminance:.0} raw")
            } else {
                format!("{illuminance:.0} lx")
            };
        }
        // Smoke / CO / water alarms
        for key in &["smoke", "co", "water_detected"] {
            if let Some(true) = attrs.get(*key).and_then(|v| v.as_bool()) {
                return normalize_label(key);
            }
        }
        // Online/offline from a plugin status field
        if let Some(online) = attrs.get("online").and_then(|v| v.as_bool()) {
            return if online {
                "Online".to_string()
            } else {
                "Offline".to_string()
            };
        }
        // Occupancy sensor (Lutron occupancy groups)
        if let Some(occupied) = attrs.get("occupied").and_then(|v| v.as_bool()) {
            return if occupied {
                "Occupied".to_string()
            } else {
                "Vacant".to_string()
            };
        }
        // No recognisable state — device is read-only or state not yet received
        "—".to_string()
    }

    pub fn filtered_events(&self) -> Vec<&EventEntry> {
        self.events
            .iter()
            .filter(|e| self.event_matches_filter(e))
            .collect()
    }

    pub fn plugin_events(&self, plugin_id: &str) -> Vec<&EventEntry> {
        self.events
            .iter()
            .filter(|e| e.plugin_id.as_deref() == Some(plugin_id))
            .collect()
    }

    pub fn selected_plugin(&self) -> Option<&PluginRecord> {
        self.plugins.get(self.selected)
    }

    fn cycle_plugin_detail_panel(&mut self, forward: bool) {
        self.plugin_detail_panel = match (self.plugin_detail_panel, forward) {
            (PluginDetailPanel::Overview, true) => PluginDetailPanel::Actions,
            (PluginDetailPanel::Actions, true) => PluginDetailPanel::Diagnostics,
            (PluginDetailPanel::Diagnostics, true) => PluginDetailPanel::Metrics,
            (PluginDetailPanel::Metrics, true) => PluginDetailPanel::Overview,
            (PluginDetailPanel::Overview, false) => PluginDetailPanel::Metrics,
            (PluginDetailPanel::Actions, false) => PluginDetailPanel::Overview,
            (PluginDetailPanel::Diagnostics, false) => PluginDetailPanel::Actions,
            (PluginDetailPanel::Metrics, false) => PluginDetailPanel::Diagnostics,
        };
        self.selected = 0;
    }

    // ── Plugin capabilities + actions ────────────────────────────────────────

    /// Fetch and cache the manifest for the currently-open plugin's
    /// Actions panel. Populates `plugin_capabilities` or
    /// `plugin_capabilities_error` and resets the row cursor.
    async fn refresh_plugin_capabilities(&mut self) {
        let Some(plugin_id) = self.plugin_detail_plugin_id.clone() else {
            return;
        };
        self.plugin_capabilities_loading = true;
        self.plugin_capabilities_error = None;
        match self.client.get_plugin_capabilities(&plugin_id).await {
            Ok(caps) => {
                let n = caps.actions.len();
                self.plugin_capabilities = Some(caps);
                self.selected = 0;
                self.status = format!("Capabilities loaded ({n} actions)");
            }
            Err(e) => {
                self.plugin_capabilities = None;
                self.plugin_capabilities_error = Some(e.to_string());
            }
        }
        self.plugin_capabilities_loading = false;
    }

    /// Run the currently-selected action. Non-streaming actions POST and
    /// surface the response in `action_status`. Streaming actions show
    /// a "use web client" hint — full streaming UI is Phase 2.
    async fn run_selected_plugin_action(&mut self) {
        if self.action_busy {
            return;
        }
        let Some(plugin_id) = self.plugin_detail_plugin_id.clone() else {
            return;
        };
        let Some(caps) = self.plugin_capabilities.clone() else {
            return;
        };
        let Some(action) = caps.actions.get(self.selected).cloned() else {
            return;
        };

        if action.stream {
            // Streaming actions take the SSE path: open the modal,
            // POST to start, and on success spawn the SSE consumer.
            // The modal renders progress/items/warnings/prompts/terminal
            // and offers Cancel + Respond.
            self.start_streaming_action(action).await;
            return;
        }

        // Role gate: server enforces, but surface a friendly error early
        // if the user clearly lacks the required role.
        let role_required = action.requires_role.as_str();
        let user_role = self
            .current_user
            .as_ref()
            .map(|u| match u.role {
                Role::Admin => "admin",
                Role::User => "user",
                Role::ReadOnly => "read_only",
                Role::Observer => "observer",
                Role::DeviceOperator => "device_operator",
                Role::RuleEditor => "rule_editor",
                Role::ServiceOperator => "service_operator",
            })
            .unwrap_or("user");
        if role_required == "admin" && user_role != "admin" {
            self.action_status = format!(
                "Action `{}` requires admin role; you are `{}`.",
                action.id, user_role
            );
            return;
        }

        self.action_busy = true;
        self.action_status = format!("Running `{}`…", action.id);

        // Phase 1 sends empty params. Param input UI is a follow-up.
        let result = self
            .client
            .post_plugin_command(&plugin_id, &action.id, serde_json::json!({}))
            .await;

        self.action_busy = false;
        match result {
            Ok(resp) => {
                // Compact summary: show top-level keys and any "status" or
                // "request_id" so the operator gets feedback without a
                // full JSON dump on the panel footer.
                let summary = match &resp {
                    Value::Object(map) => {
                        if let Some(s) = map.get("status").and_then(Value::as_str) {
                            format!("status={s}")
                        } else if let Some(rid) = map.get("request_id").and_then(Value::as_str) {
                            format!("request_id={rid}")
                        } else {
                            // Generic — list top-level keys
                            let keys: Vec<&str> = map.keys().map(String::as_str).collect();
                            format!("keys=[{}]", keys.join(","))
                        }
                    }
                    _ => resp.to_string(),
                };
                self.action_status = format!("`{}` ok — {summary}", action.id);
            }
            Err(e) => {
                self.action_status = format!("`{}` failed: {e}", action.id);
            }
        }
    }

    /// Open the streaming-action modal, POST the start command, and
    /// (on success) spawn the SSE consumer that pumps progress events
    /// into `streaming_action.apply_event` via the shared WS channel.
    async fn start_streaming_action(&mut self, action: crate::api::Action) {
        let Some(plugin_id) = self.plugin_detail_plugin_id.clone() else {
            return;
        };

        let label = if action.label.is_empty() {
            action.id.clone()
        } else {
            action.label.clone()
        };
        let mut state = StreamingAction::new(plugin_id.clone(), action.id.clone(), label);
        state.footer = "starting…".into();
        self.streaming_action = Some(state);

        // Phase 2 sends empty params for the start (typed param form
        // for streaming actions lives behind the same TODO as Phase 1's
        // non-streaming form). Plugins that require parameters surface
        // a validation `error` stage that the modal renders normally.
        let result = self
            .client
            .start_streaming_action(&plugin_id, &action.id, serde_json::json!({}))
            .await;

        match result {
            Ok(resp) => {
                if resp.get("status").and_then(serde_json::Value::as_str) == Some("busy") {
                    let active = resp
                        .get("active_request_id")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    if let Some(ref mut s) = self.streaming_action {
                        s.stage = StreamingStage::Error;
                        s.terminal = Some(serde_json::json!({
                            "stage": "error",
                            "error": format!(
                                "another invocation is in flight (request_id={active})"
                            ),
                        }));
                        s.footer = "busy — press Esc to close".into();
                    }
                    return;
                }
                let Some(rid) = resp
                    .get("request_id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
                else {
                    if let Some(ref mut s) = self.streaming_action {
                        s.stage = StreamingStage::Error;
                        s.terminal = Some(serde_json::json!({
                            "stage": "error",
                            "error": "plugin did not return a request_id",
                        }));
                        s.footer = "error — press Esc to close".into();
                    }
                    return;
                };

                let Some(ref tx) = self.ws_sender.clone() else {
                    if let Some(ref mut s) = self.streaming_action {
                        s.stage = StreamingStage::Error;
                        s.terminal = Some(serde_json::json!({
                            "stage": "error",
                            "error": "internal: no ws sender available",
                        }));
                        s.footer = "error — press Esc to close".into();
                    }
                    return;
                };
                let Some(token) = self.ws_token() else {
                    if let Some(ref mut s) = self.streaming_action {
                        s.stage = StreamingStage::Error;
                        s.terminal = Some(serde_json::json!({
                            "stage": "error",
                            "error": "not authenticated",
                        }));
                        s.footer = "error — press Esc to close".into();
                    }
                    return;
                };

                if let Some(ref mut s) = self.streaming_action {
                    s.request_id = Some(rid.clone());
                    s.footer = "connecting to event stream…".into();
                }
                crate::sse::spawn_streaming_action(
                    self.client.base_url().to_string(),
                    plugin_id,
                    rid,
                    token,
                    tx.clone(),
                );
            }
            Err(e) => {
                if let Some(ref mut s) = self.streaming_action {
                    s.stage = StreamingStage::Error;
                    s.terminal = Some(serde_json::json!({
                        "stage": "error",
                        "error": format!("start failed: {e}"),
                    }));
                    s.footer = "error — press Esc to close".into();
                }
            }
        }
    }

    /// Send `cancel` for the in-flight streaming action. The SSE stream
    /// will close itself on the resulting terminal `canceled` event.
    pub async fn cancel_streaming_action(&mut self) {
        let Some(state) = &self.streaming_action else {
            return;
        };
        if state.stage.is_terminal() {
            return;
        }
        let Some(rid) = state.request_id.clone() else {
            return;
        };
        let plugin_id = state.plugin_id.clone();
        if let Some(ref mut s) = self.streaming_action {
            s.footer = "cancel sent — waiting for terminal…".into();
        }
        if let Err(e) = self
            .client
            .cancel_streaming_action(&plugin_id, &rid)
            .await
        {
            if let Some(ref mut s) = self.streaming_action {
                s.footer = format!("cancel failed: {e}");
            }
        }
    }

    /// Send `respond` to satisfy a current `awaiting_user` prompt.
    pub async fn respond_streaming_action(&mut self) {
        let Some(state) = &self.streaming_action else {
            return;
        };
        if state.pending_prompt.is_none() {
            return;
        }
        let Some(rid) = state.request_id.clone() else {
            return;
        };
        let plugin_id = state.plugin_id.clone();
        // For the MVP the response is a single string; the plugin can
        // still parse JSON if the user types a JSON literal.
        let raw = state.response_input.clone();
        let response = serde_json::from_str::<serde_json::Value>(&raw)
            .unwrap_or(serde_json::Value::String(raw));

        if let Some(ref mut s) = self.streaming_action {
            s.footer = "response sent — waiting for plugin…".into();
            s.pending_prompt = None;
            s.response_input.clear();
        }
        if let Err(e) = self
            .client
            .respond_streaming_action(&plugin_id, &rid, response)
            .await
        {
            if let Some(ref mut s) = self.streaming_action {
                s.footer = format!("respond failed: {e}");
            }
        }
    }

    /// Close the streaming-action modal. No-op when no modal is open.
    /// Cancellation is handled separately — closing the modal mid-run
    /// just hides the UI; the action keeps running server-side until
    /// the user explicitly cancels.
    pub fn close_streaming_action(&mut self) {
        self.streaming_action = None;
    }

    /// Key dispatch for the streaming-action modal. Branches on the
    /// current stage (terminal vs. running vs. awaiting_user) so the
    /// same key can mean different things depending on context.
    pub async fn handle_streaming_action_key(&mut self, key: KeyEvent) {
        let Some(state) = &self.streaming_action else {
            return;
        };
        let stage = state.stage;
        let awaiting = state.pending_prompt.is_some();

        // Esc always closes the modal.
        if matches!(key.code, KeyCode::Esc) {
            self.close_streaming_action();
            return;
        }

        // After terminal stage, only Esc/q close (no Cancel/Respond).
        if stage.is_terminal() {
            if matches!(key.code, KeyCode::Char('q')) {
                self.close_streaming_action();
            }
            return;
        }

        if awaiting {
            // Type into the response buffer; Enter sends.
            match key.code {
                KeyCode::Enter => {
                    self.respond_streaming_action().await;
                }
                KeyCode::Backspace => {
                    if let Some(ref mut s) = self.streaming_action {
                        s.response_input.pop();
                    }
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.cancel_streaming_action().await;
                }
                KeyCode::Char(ch) => {
                    if let Some(ref mut s) = self.streaming_action {
                        s.response_input.push(ch);
                    }
                }
                _ => {}
            }
            return;
        }

        // Running with no prompt — `c` cancels.
        if matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C')) {
            self.cancel_streaming_action().await;
        }
    }

    pub fn on_stream_connected(&mut self) {
        if let Some(ref mut s) = self.streaming_action {
            s.footer = "streaming…".into();
        }
    }

    pub fn on_stream_event(&mut self, ev: serde_json::Value) {
        if let Some(ref mut s) = self.streaming_action {
            s.apply_event(ev);
        }
    }

    pub fn on_stream_closed(&mut self) {
        if let Some(ref mut s) = self.streaming_action {
            if !s.stage.is_terminal() {
                s.footer = "stream closed without terminal — press Esc".into();
            }
        }
    }

    pub fn on_stream_error(&mut self, reason: String) {
        if let Some(ref mut s) = self.streaming_action {
            if !s.stage.is_terminal() {
                s.stage = StreamingStage::Error;
                s.terminal = Some(serde_json::json!({
                    "stage": "error",
                    "error": reason.clone(),
                }));
                s.footer = format!("stream error: {reason}");
            }
        }
    }

    async fn discover_bridges_for_selected_plugin(&mut self) {
        let Some(plugin_id) = self.plugin_detail_plugin_id.clone() else {
            return;
        };

        match self.client.discover_plugin_bridges(&plugin_id).await {
            Ok(_) => {
                self.status = format!("Requested bridge discovery for {}", plugin_id);
                if let Err(err) = self.refresh_all().await {
                    self.error = Some(err.to_string());
                }
            }
            Err(err) => {
                self.error = Some(format!("Bridge discovery failed: {}", err));
            }
        }
    }

    async fn pair_bridges_for_selected_plugin(&mut self) {
        let Some(plugin_id) = self.plugin_detail_plugin_id.clone() else {
            return;
        };

        let bridge_ids = self.selected_plugin_hue_bridge_ids(&plugin_id);

        if bridge_ids.is_empty() {
            self.error = Some("No Hue bridges found for selected plugin".to_string());
            return;
        }

        let mut ok = 0usize;
        let mut failed = Vec::new();
        for device_id in bridge_ids {
            match self
                .client
                .send_device_action(&device_id, "pair_bridge")
                .await
            {
                Ok(_) => ok += 1,
                Err(err) => failed.push(format!("{device_id}: {err}")),
            }
        }

        if failed.is_empty() {
            let pairing_status =
                format!("Pairing requested for {ok} bridge(s). Press Hue link button if needed.");
            self.status = pairing_status.clone();

            if let Err(err) = self.refresh_all().await {
                if self.error.is_none() {
                    self.error = Some(err.to_string());
                }
                return;
            }

            // Preserve explicit pairing feedback instead of generic refresh status.
            self.status = pairing_status;
        } else {
            self.error = Some(format!("Pairing request errors: {}", failed.join(" | ")));

            if let Err(err) = self.refresh_all().await {
                if self.error.is_none() {
                    self.error = Some(err.to_string());
                }
            }
        }
    }

    fn open_plugin_detail(&mut self) {
        let Some(plugin_id) = self.selected_plugin().map(|p| p.plugin_id.clone()) else {
            return;
        };
        self.plugin_detail_open = true;
        self.plugin_detail_plugin_id = Some(plugin_id.clone());
        self.plugin_detail_panel = PluginDetailPanel::Overview;
        self.status = format!("Opened plugin detail: {}", plugin_id);
    }

    fn selected_plugin_hue_bridge_ids(&self, plugin_id: &str) -> Vec<String> {
        self.devices
            .iter()
            .filter(|d| {
                d.plugin_id == plugin_id
                    && d.attributes.get("kind").and_then(|v| v.as_str()) == Some("hue_bridge")
            })
            .map(|d| d.device_id.clone())
            .collect::<Vec<_>>()
    }

    fn event_matches_filter(&self, entry: &EventEntry) -> bool {
        let ty = entry.event_type.as_str();
        let custom = entry.event_type_custom.as_deref().unwrap_or("");
        match self.events_filter_mode {
            EventsFilterMode::All => true,
            EventsFilterMode::HueInputs => {
                matches!(
                    ty,
                    "device_button"
                        | "device_rotary"
                        | "entertainment_action_applied"
                        | "entertainment_status_changed"
                        | "plugin_command_result"
                        | "bridge_pairing_status"
                ) || matches!(
                    custom,
                    "device_button"
                        | "device_rotary"
                        | "entertainment_action_applied"
                        | "entertainment_status_changed"
                        | "plugin_command_result"
                        | "bridge_pairing_status"
                )
            }
            EventsFilterMode::Entertainment => {
                matches!(
                    ty,
                    "entertainment_action_applied" | "entertainment_status_changed"
                ) || matches!(
                    custom,
                    "entertainment_action_applied" | "entertainment_status_changed"
                )
            }
            EventsFilterMode::PluginMetrics => ty == "plugin_metrics" || custom == "plugin_metrics",
        }
    }

    fn active_items_len(&self) -> usize {
        match self.active_tab() {
            Tab::Devices => match self.device_sub {
                DeviceSubPanel::All => self.visible_devices().len(),
                DeviceSubPanel::MediaPlayers => self.visible_media_players().len(),
                DeviceSubPanel::Switches => self.switches.len(),
                DeviceSubPanel::Timers => self.timers.len(),
            },
            Tab::Scenes => self.scenes.len(),
            Tab::Areas => self.areas.len(),
            Tab::Rules => self.visible_rules().len(),
            Tab::Plugins => self.plugins.len(),
            Tab::Manage => match self.admin_sub {
                AdminSubPanel::Modes => self.modes.len(),
                AdminSubPanel::Matter => self.matter_nodes.len(),
                AdminSubPanel::Status => 0,
                AdminSubPanel::Users => self.users.len(),
                AdminSubPanel::Logs => 0,
                AdminSubPanel::Events => self.filtered_events().len(),
                AdminSubPanel::Audit => self.audit_entries.len(),
                AdminSubPanel::Backup => BACKUP_ACTIONS.len(),
            },
        }
    }

    fn clamp_selection(&mut self) {
        let len = self.active_items_len();
        if len == 0 {
            self.selected = 0;
        } else if self.selected >= len {
            self.selected = len - 1;
        }
    }


    async fn toggle_selected_device(&mut self) {
        let (device_id, device_name, current_on, media_action) = {
            let Some(device) = self.selected_device() else {
                return;
            };
            let on = device
                .attributes
                .get("on")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            (
                device.device_id.clone(),
                device.name.clone(),
                on,
                Self::media_player_toggle_action(device),
            )
        };

        if let Some(action) = media_action {
            match self.client.send_device_action(&device_id, action).await {
                Ok(_) => {
                    self.status = format!(
                        "{} → {}",
                        device_name,
                        if action == "play" { "Play" } else { "Stop" }
                    );
                }
                Err(stop_err) if action == "stop" => {
                    match self.client.send_device_action(&device_id, "pause").await {
                        Ok(_) => {
                            self.status = format!("{} → Pause", device_name);
                        }
                        Err(pause_err) => {
                            self.error = Some(format!(
                                "stop failed: {}; pause fallback failed: {}",
                                stop_err, pause_err
                            ));
                        }
                    }
                }
                Err(err) => self.error = Some(err.to_string()),
            }
            return;
        }

        match self.client.set_device_on(&device_id, !current_on).await {
            Ok(_) => {
                self.status = format!(
                    "{} → {}",
                    device_name,
                    if !current_on { "On" } else { "Off" }
                )
            }
            Err(err) => self.error = Some(err.to_string()),
        }
    }

    async fn send_media_player_action(
        &mut self,
        action: &str,
        success_message: impl FnOnce(&str) -> String,
    ) -> bool {
        let Some(device) = self.selected_media_player() else {
            return false;
        };
        let device_id = device.device_id.clone();
        let device_name = device.name.clone();

        match self.client.send_device_action(&device_id, action).await {
            Ok(_) => {
                self.status = success_message(&device_name);
                true
            }
            Err(err) => {
                self.error = Some(err.to_string());
                false
            }
        }
    }

    async fn media_player_play_pause(&mut self) {
        let Some(model) = self.selected_media_player_model() else {
            return;
        };

        if matches!(model.playback_state.as_str(), "playing" | "buffering")
            && model.capabilities.can_pause
        {
            self.send_media_player_action("pause", |name| format!("{name} → Pause"))
                .await;
        } else if model.capabilities.can_play {
            self.send_media_player_action("play", |name| format!("{name} → Play"))
                .await;
        }
    }

    async fn media_player_stop(&mut self) {
        let Some(model) = self.selected_media_player_model() else {
            return;
        };
        let Some(device) = self.selected_media_player() else {
            return;
        };
        let device_id = device.device_id.clone();
        let device_name = device.name.clone();

        if model.capabilities.can_stop {
            match self.client.send_device_action(&device_id, "stop").await {
                Ok(_) => {
                    self.status = format!("{device_name} → Stop");
                }
                Err(stop_err) => {
                    if model.capabilities.can_pause {
                        match self.client.send_device_action(&device_id, "pause").await {
                            Ok(_) => {
                                self.status = format!("{device_name} → Pause");
                            }
                            Err(pause_err) => {
                                self.error = Some(format!(
                                    "stop failed: {}; pause fallback failed: {}",
                                    stop_err, pause_err
                                ));
                            }
                        }
                    } else {
                        self.error = Some(stop_err.to_string());
                    }
                }
            }
        } else if model.capabilities.can_pause {
            self.send_media_player_action("pause", |name| format!("{name} → Pause"))
                .await;
        }
    }

    async fn media_player_next(&mut self) {
        let Some(model) = self.selected_media_player_model() else {
            return;
        };
        if model.capabilities.can_next {
            self.send_media_player_action("next", |name| format!("{name} → Next"))
                .await;
        }
    }

    async fn media_player_previous(&mut self) {
        let Some(model) = self.selected_media_player_model() else {
            return;
        };
        if model.capabilities.can_previous {
            self.send_media_player_action("previous", |name| format!("{name} → Previous"))
                .await;
        }
    }

    async fn media_player_toggle_mute(&mut self) {
        let Some(model) = self.selected_media_player_model() else {
            return;
        };
        if !model.capabilities.can_mute {
            return;
        }
        let Some(device) = self.selected_media_player() else {
            return;
        };
        let device_id = device.device_id.clone();
        let device_name = device.name.clone();
        let muted = !model.muted.unwrap_or(false);

        match self
            .client
            .patch_device_state(&device_id, json!({ "action": "set_mute", "muted": muted }))
            .await
        {
            Ok(_) => {
                self.status = format!(
                    "{} → {}",
                    device_name,
                    if muted { "Muted" } else { "Unmuted" }
                );
            }
            Err(err) => self.error = Some(err.to_string()),
        }
    }

    async fn media_player_adjust_volume(&mut self, delta: i64) {
        let Some(model) = self.selected_media_player_model() else {
            return;
        };
        if !model.capabilities.can_set_volume {
            return;
        }
        let Some(device) = self.selected_media_player() else {
            return;
        };
        let device_id = device.device_id.clone();
        let device_name = device.name.clone();
        let next = (i64::from(model.volume.unwrap_or(0)) + delta).clamp(0, 100);

        match self
            .client
            .patch_device_state(
                &device_id,
                json!({ "action": "set_volume", "volume": next }),
            )
            .await
        {
            Ok(_) => {
                self.status = format!("{device_name} volume → {next}%");
            }
            Err(err) => self.error = Some(err.to_string()),
        }
    }

    async fn adjust_brightness(&mut self, direction: i64) {
        let (device_id, device_name, raw_pct, raw_abs) = {
            let Some(device) = self.selected_device() else {
                return;
            };
            let pct = device
                .attributes
                .get("brightness_pct")
                .and_then(|v| v.as_f64());
            let abs = device.attributes.get("brightness").and_then(|v| v.as_f64());
            (device.device_id.clone(), device.name.clone(), pct, abs)
        };

        if let Some(raw) = raw_pct {
            // Hue-style 0–100% brightness
            let new_val = ((raw + direction as f64 * 10.0).clamp(0.0, 100.0) * 10.0).round() / 10.0;
            match self
                .client
                .set_device_brightness_pct(&device_id, new_val)
                .await
            {
                Ok(_) => self.status = format!("{device_name} brightness → {new_val:.0}%"),
                Err(err) => self.error = Some(err.to_string()),
            }
        } else {
            // ZWave / generic 0–255 or 0.0–1.0 brightness
            let raw = raw_abs.unwrap_or(0.0);
            let (max, step) = if raw <= 1.0 {
                (1.0_f64, 0.1)
            } else if raw <= 100.0 {
                (100.0_f64, 10.0)
            } else {
                (255.0_f64, 25.0)
            };
            let new_val = ((raw + direction as f64 * step).clamp(0.0, max) * 10.0).round() / 10.0;
            let new_val_i = new_val as i64;
            match self
                .client
                .set_device_brightness(&device_id, new_val_i)
                .await
            {
                Ok(_) => self.status = format!("{device_name} brightness → {new_val_i}"),
                Err(err) => self.error = Some(err.to_string()),
            }
        }
    }

    async fn lock_device(&mut self, locked: bool) {
        let (device_id, device_name) = {
            let Some(device) = self.selected_device() else {
                return;
            };
            (device.device_id.clone(), device.name.clone())
        };
        match self.client.set_device_locked(&device_id, locked).await {
            Ok(_) => {
                self.status = format!(
                    "{} → {}",
                    device_name,
                    if locked { "Locked" } else { "Unlocked" }
                );
            }
            Err(err) => self.error = Some(err.to_string()),
        }
    }

    /// Space bar: toggle lock state for lock devices, or on/off for switches.
    async fn toggle_lock_or_switch(&mut self) {
        let Some(device) = self.selected_device() else {
            return;
        };
        let device_id = device.device_id.clone();
        let device_name = device.name.clone();

        if let Some(action) = Self::media_player_toggle_action(device) {
            match self.client.send_device_action(&device_id, action).await {
                Ok(_) => {
                    self.status = format!(
                        "{} → {}",
                        device_name,
                        if action == "play" { "Play" } else { "Stop" }
                    );
                }
                Err(stop_err) if action == "stop" => {
                    match self.client.send_device_action(&device_id, "pause").await {
                        Ok(_) => {
                            self.status = format!("{} → Pause", device_name);
                        }
                        Err(pause_err) => {
                            self.error = Some(format!(
                                "stop failed: {}; pause fallback failed: {}",
                                stop_err, pause_err
                            ));
                        }
                    }
                }
                Err(err) => self.error = Some(err.to_string()),
            }
            return;
        }

        if let Some(locked) = Self::device_lock_state(device) {
            let new_locked = !locked;
            match self.client.set_device_locked(&device_id, new_locked).await {
                Ok(_) => {
                    self.status = format!(
                        "{} → {}",
                        device_name,
                        if new_locked { "Locked" } else { "Unlocked" }
                    )
                }
                Err(err) => self.error = Some(err.to_string()),
            }
        } else {
            let on = device
                .attributes
                .get("on")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            match self.client.set_device_on(&device_id, !on).await {
                Ok(_) => {
                    self.status = format!("{} → {}", device_name, if !on { "On" } else { "Off" })
                }
                Err(err) => self.error = Some(err.to_string()),
            }
        }
    }

    // ── Area CRUD ─────────────────────────────────────────────────────────────

    fn open_area_editor_create(&mut self) {
        self.area_editor = Some(AreaEditor {
            id: None,
            name: String::new(),
        });
    }

    fn open_area_editor_edit(&mut self) {
        let Some(area) = self.areas.get(self.selected) else {
            return;
        };
        self.area_editor = Some(AreaEditor {
            id: Some(area.id.clone()),
            name: area.name.clone(),
        });
    }

    async fn on_key_area_editor(&mut self, key: KeyEvent) {
        let Some(editor) = self.area_editor.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                self.area_editor = None;
                self.status = "Area edit canceled".to_string();
            }
            KeyCode::Backspace => {
                editor.name.pop();
            }
            KeyCode::Enter => {
                self.save_area_editor().await;
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                editor.name.push(ch);
            }
            _ => {}
        }
    }

    async fn save_area_editor(&mut self) {
        let Some(editor) = self.area_editor.clone() else {
            return;
        };
        let name = editor.name.trim().to_string();
        if name.is_empty() {
            self.error = Some("area name cannot be empty".to_string());
            return;
        }
        match editor.id {
            None => match self.client.create_area(&name).await {
                Ok(area) => {
                    self.areas.push(area);
                    self.area_editor = None;
                    self.status = format!("Created area '{name}'");
                    let _ = self.save_to_cache().await;
                }
                Err(e) => self.error = Some(e.to_string()),
            },
            Some(ref id) => match self.client.rename_area(id, &name).await {
                Ok(updated) => {
                    if let Some(a) = self.areas.iter_mut().find(|a| a.id == updated.id) {
                        a.name = updated.name.clone();
                    }
                    self.area_editor = None;
                    self.status = format!("Renamed area to '{}'", updated.name);
                    let _ = self.save_to_cache().await;
                }
                Err(e) => self.error = Some(e.to_string()),
            },
        }
    }

    async fn delete_selected_area(&mut self) {
        let Some(area) = self.areas.get(self.selected) else {
            return;
        };
        let id = area.id.clone();
        let name = area.name.clone();
        match self.client.delete_area(&id).await {
            Ok(_) => {
                self.areas.retain(|a| a.id != id);
                self.clamp_selection();
                self.status = format!("Deleted area '{name}'");
                let _ = self.save_to_cache().await;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    async fn delete_selected_area_from_pane(&mut self) {
        let Some(area) = self.areas.get(self.areas_list_selected) else {
            return;
        };
        let id = area.id.clone();
        let name = area.name.clone();
        match self.client.delete_area(&id).await {
            Ok(_) => {
                self.areas.retain(|a| a.id != id);
                if self.areas_list_selected > 0 && self.areas_list_selected >= self.areas.len() {
                    self.areas_list_selected = self.areas_list_selected.saturating_sub(1);
                }
                self.areas_selected_area_id = None;
                self.areas_devices_selected = 0;
                self.status = format!("Deleted area '{name}'");
                let _ = self.save_to_cache().await;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    // ── Areas Pane Navigation and Device Management ────────────────────────────

    async fn on_key_areas_pane(&mut self, key: KeyEvent) {
        use crate::app::AreasPane;

        match key.code {
            // Pane switching (h/l keys and arrow keys)
            KeyCode::Char('h') | KeyCode::Left => {
                self.areas_pane_focus = AreasPane::AreasList;
                self.areas_selected_devices.clear();
                // Auto-select the area at current selection
                if let Some(area) = self.areas.get(self.areas_list_selected) {
                    self.areas_selected_area_id = Some(area.id.clone());
                }
            }
            KeyCode::Char('l') | KeyCode::Right => {
                if self.areas_selected_area_id.is_some() {
                    self.areas_pane_focus = AreasPane::DeviceList;
                }
            }

            // Navigation within current pane
            KeyCode::Up | KeyCode::Char('k') => {
                match self.areas_pane_focus {
                    AreasPane::AreasList => {
                        self.areas_list_selected = self.areas_list_selected.saturating_sub(1);
                        // Auto-update selected area and reset device selection
                        if let Some(area) = self.areas.get(self.areas_list_selected) {
                            self.areas_selected_area_id = Some(area.id.clone());
                            self.areas_devices_selected = 0;
                            self.areas_selected_devices.clear();
                        }
                    }
                    AreasPane::DeviceList => {
                        self.areas_devices_selected = self.areas_devices_selected.saturating_sub(1);
                    }
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let len = self.get_areas_pane_len();
                if len > 0 {
                    match self.areas_pane_focus {
                        AreasPane::AreasList => {
                            self.areas_list_selected = min(self.areas_list_selected + 1, len - 1);
                            // Auto-update selected area and reset device selection
                            if let Some(area) = self.areas.get(self.areas_list_selected) {
                                self.areas_selected_area_id = Some(area.id.clone());
                                self.areas_devices_selected = 0;
                                self.areas_selected_devices.clear();
                            }
                        }
                        AreasPane::DeviceList => {
                            self.areas_devices_selected =
                                min(self.areas_devices_selected + 1, len - 1);
                        }
                    }
                }
            }

            // Enter key behavior based on pane focus
            KeyCode::Enter => match self.areas_pane_focus {
                AreasPane::AreasList => {
                    if let Some(area) = self.areas.get(self.areas_list_selected) {
                        self.areas_selected_area_id = Some(area.id.clone());
                        self.areas_pane_focus = AreasPane::DeviceList;
                        self.areas_devices_selected = 0;
                        self.areas_selected_devices.clear();
                    }
                }
                AreasPane::DeviceList => {
                    if self.areas_selected_area_id.is_some() {
                        self.open_area_editor_edit();
                    }
                }
            },

            // Create new area
            KeyCode::Char('n') => {
                self.open_area_editor_create();
            }

            // Rename or delete based on pane focus
            KeyCode::Char('d') => {
                match self.areas_pane_focus {
                    AreasPane::AreasList => {
                        self.delete_selected_area_from_pane().await;
                    }
                    AreasPane::DeviceList => {
                        // Remove selected devices from area
                        self.remove_selected_devices_from_area().await;
                    }
                }
            }

            // Space: toggle device selection in device list pane
            KeyCode::Char(' ') => {
                if matches!(self.areas_pane_focus, AreasPane::DeviceList) {
                    if let Some(area_id) = &self.areas_selected_area_id {
                        let device_ids = self
                            .areas
                            .iter()
                            .find(|a| &a.id == area_id)
                            .map(|a| a.device_ids.clone())
                            .unwrap_or_default();

                        let visible_devices: Vec<_> = self
                            .devices
                            .iter()
                            .filter(|d| device_ids.contains(&d.device_id))
                            .collect();

                        if let Some(device) = visible_devices.get(self.areas_devices_selected) {
                            if self.areas_selected_devices.contains(&device.device_id) {
                                self.areas_selected_devices.remove(&device.device_id);
                            } else {
                                self.areas_selected_devices.insert(device.device_id.clone());
                            }
                        }
                    }
                }
            }

            // Plus/Minus: add/remove devices from area
            KeyCode::Char('+') | KeyCode::Char('=') => {
                if !self.areas_selected_devices.is_empty() {
                    self.add_selected_devices_to_area().await;
                }
            }
            KeyCode::Char('-') => {
                if !self.areas_selected_devices.is_empty() {
                    self.remove_selected_devices_from_area().await;
                }
            }

            _ => {}
        }
    }

    fn get_areas_pane_len(&self) -> usize {
        use crate::app::AreasPane;

        match self.areas_pane_focus {
            AreasPane::AreasList => self.areas.len(),
            AreasPane::DeviceList => {
                if let Some(area_id) = &self.areas_selected_area_id {
                    let device_ids = self
                        .areas
                        .iter()
                        .find(|a| &a.id == area_id)
                        .map(|a| a.device_ids.clone())
                        .unwrap_or_default();
                    self.devices
                        .iter()
                        .filter(|d| device_ids.contains(&d.device_id))
                        .count()
                } else {
                    0
                }
            }
        }
    }

    async fn add_selected_devices_to_area(&mut self) {
        if self.areas_selected_devices.is_empty() {
            return;
        }

        if let Some(area_id) = &self.areas_selected_area_id {
            if let Some(area) = self.areas.iter().find(|a| &a.id == area_id) {
                let mut new_device_ids = area.device_ids.clone();

                // Add selected devices that aren't already in the area
                for device_id in &self.areas_selected_devices {
                    if !new_device_ids.contains(device_id) {
                        new_device_ids.push(device_id.clone());
                    }
                }

                // Call API to set area devices
                match self.client.set_area_devices(area_id, &new_device_ids).await {
                    Ok(_) => {
                        self.status = format!(
                            "Added {} device(s) to area",
                            self.areas_selected_devices.len()
                        );
                        self.areas_selected_devices.clear();
                        // Refresh to get updated area
                        if let Err(e) = self.refresh_all().await {
                            self.error = Some(e.to_string());
                        }
                    }
                    Err(e) => {
                        self.error = Some(format!("Failed to add devices: {}", e));
                    }
                }
            }
        }
    }

    async fn remove_selected_devices_from_area(&mut self) {
        if self.areas_selected_devices.is_empty()
            && matches!(self.areas_pane_focus, AreasPane::AreasList)
        {
            self.delete_selected_area().await;
            return;
        }

        if let Some(area_id) = &self.areas_selected_area_id {
            if let Some(area) = self.areas.iter().find(|a| &a.id == area_id) {
                let new_device_ids: Vec<String> = area
                    .device_ids
                    .iter()
                    .filter(|d| !self.areas_selected_devices.contains(*d))
                    .cloned()
                    .collect();

                // Call API to set area devices (now with removed devices)
                match self.client.set_area_devices(area_id, &new_device_ids).await {
                    Ok(_) => {
                        let count = self.areas_selected_devices.len();
                        self.status = format!("Removed {} device(s) from area", count);
                        self.areas_selected_devices.clear();
                        // Refresh to get updated area
                        if let Err(e) = self.refresh_all().await {
                            self.error = Some(e.to_string());
                        }
                    }
                    Err(e) => {
                        self.error = Some(format!("Failed to remove devices: {}", e));
                    }
                }
            }
        }
    }

    // ── Device delete ─────────────────────────────────────────────────────────

    async fn delete_selected_device(&mut self) {
        let device_id = {
            let Some(device) = self.selected_device() else {
                return;
            };
            device.device_id.clone()
        };
        match self.client.delete_device(&device_id).await {
            Ok(_) => {
                self.devices.retain(|d| d.device_id != device_id);
                self.clamp_selection();
                self.status = format!("Deleted device '{device_id}'");
                let _ = self.save_to_cache().await;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    async fn delete_selected_device_switch(&mut self) {
        let Some(sw) = self.switches.get(self.selected).cloned() else {
            return;
        };
        let id = sw.device_id.clone();
        match self.client.delete_device(&id).await {
            Ok(_) => {
                self.switches.retain(|s| s.device_id != id);
                self.devices.retain(|d| d.device_id != id);
                self.clamp_selection();
                self.status = format!("Deleted switch '{id}'");
                let _ = self.save_to_cache().await;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    async fn delete_selected_device_timer(&mut self) {
        let Some(t) = self.timers.get(self.selected).cloned() else {
            return;
        };
        let id = t.device_id.clone();
        match self.client.delete_device(&id).await {
            Ok(_) => {
                self.timers.retain(|ti| ti.device_id != id);
                self.devices.retain(|d| d.device_id != id);
                self.clamp_selection();
                self.status = format!("Deleted timer '{id}'");
                let _ = self.save_to_cache().await;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    // ── Plugin deregister ─────────────────────────────────────────────────────

    async fn deregister_selected_plugin(&mut self) {
        let Some(plugin) = self.plugins.get(self.selected) else {
            return;
        };
        let id = plugin.plugin_id.clone();
        match self.client.deregister_plugin(&id).await {
            Ok(_) => {
                self.plugins.retain(|p| p.plugin_id != id);
                self.clamp_selection();
                self.status = format!("Deregistered plugin '{id}'");
                let _ = self.save_to_cache().await;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    // ── User CRUD ─────────────────────────────────────────────────────────────

    fn open_user_editor_create(&mut self) {
        self.user_editor = Some(UserEditor {
            mode: UserEditMode::Create,
            id: None,
            field: UserEditField::Username,
            username: String::new(),
            current_password: String::new(),
            password: String::new(),
            confirm_password: String::new(),
            role: Role::User,
        });
    }

    fn open_user_editor_role(&mut self) {
        let Some(user) = self.users.get(self.selected) else {
            return;
        };
        self.user_editor = Some(UserEditor {
            mode: UserEditMode::EditRole,
            id: Some(user.id.clone()),
            field: UserEditField::Role,
            username: user.username.clone(),
            current_password: String::new(),
            password: String::new(),
            confirm_password: String::new(),
            role: user.role.clone(),
        });
    }

    fn open_user_editor_password(&mut self) {
        // The backend change-password endpoint always operates on the JWT user
        // (the currently logged-in account). Always change your own password here.
        let Some(u) = self.current_user.clone() else {
            return;
        };
        self.user_editor = Some(UserEditor {
            mode: UserEditMode::ChangePassword,
            id: Some(u.id.clone()),
            field: UserEditField::CurrentPassword,
            username: u.username.clone(),
            current_password: String::new(),
            password: String::new(),
            confirm_password: String::new(),
            role: Role::User,
        });
    }

    pub async fn on_key_user_editor(&mut self, key: KeyEvent) {
        let Some(editor) = self.user_editor.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                self.user_editor = None;
                self.status = "User edit canceled".to_string();
            }
            KeyCode::Tab | KeyCode::Down => {
                editor.field = match (&editor.mode, &editor.field) {
                    (UserEditMode::Create, UserEditField::Username) => UserEditField::Password,
                    (UserEditMode::Create, UserEditField::Password) => {
                        UserEditField::ConfirmPassword
                    }
                    (UserEditMode::Create, UserEditField::ConfirmPassword) => UserEditField::Role,
                    (UserEditMode::Create, UserEditField::Role) => UserEditField::Username,
                    (UserEditMode::ChangePassword, UserEditField::CurrentPassword) => {
                        UserEditField::Password
                    }
                    (UserEditMode::ChangePassword, UserEditField::Password) => {
                        UserEditField::ConfirmPassword
                    }
                    (UserEditMode::ChangePassword, UserEditField::ConfirmPassword) => {
                        UserEditField::CurrentPassword
                    }
                    _ => editor.field,
                };
            }
            KeyCode::BackTab | KeyCode::Up => {
                editor.field = match (&editor.mode, &editor.field) {
                    (UserEditMode::Create, UserEditField::Username) => UserEditField::Role,
                    (UserEditMode::Create, UserEditField::Password) => UserEditField::Username,
                    (UserEditMode::Create, UserEditField::ConfirmPassword) => {
                        UserEditField::Password
                    }
                    (UserEditMode::Create, UserEditField::Role) => UserEditField::ConfirmPassword,
                    (UserEditMode::ChangePassword, UserEditField::CurrentPassword) => {
                        UserEditField::ConfirmPassword
                    }
                    (UserEditMode::ChangePassword, UserEditField::Password) => {
                        UserEditField::CurrentPassword
                    }
                    (UserEditMode::ChangePassword, UserEditField::ConfirmPassword) => {
                        UserEditField::Password
                    }
                    _ => editor.field,
                };
            }
            KeyCode::Backspace => match editor.field {
                UserEditField::Username => {
                    editor.username.pop();
                }
                UserEditField::CurrentPassword => {
                    editor.current_password.pop();
                }
                UserEditField::Password => {
                    editor.password.pop();
                }
                UserEditField::ConfirmPassword => {
                    editor.confirm_password.pop();
                }
                UserEditField::Role => {}
            },
            KeyCode::Char(' ') if editor.field == UserEditField::Role => {
                editor.role = editor.role.next();
            }
            KeyCode::Enter => {
                self.save_user_editor().await;
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                let editor = self.user_editor.as_mut().unwrap();
                match editor.field {
                    UserEditField::Username => editor.username.push(ch),
                    UserEditField::CurrentPassword => editor.current_password.push(ch),
                    UserEditField::Password => editor.password.push(ch),
                    UserEditField::ConfirmPassword => editor.confirm_password.push(ch),
                    UserEditField::Role => {}
                }
            }
            _ => {}
        }
    }

    async fn save_user_editor(&mut self) {
        let Some(editor) = self.user_editor.clone() else {
            return;
        };
        match editor.mode {
            UserEditMode::Create => {
                let username = editor.username.trim().to_string();
                if username.is_empty() {
                    self.error = Some("username cannot be empty".to_string());
                    return;
                }
                if editor.password.len() < 8 {
                    self.error = Some("password must be at least 8 characters".to_string());
                    return;
                }
                if editor.password != editor.confirm_password {
                    self.error = Some("passwords do not match".to_string());
                    return;
                }
                match self
                    .client
                    .create_user(&username, &editor.password, &editor.role)
                    .await
                {
                    Ok(user) => {
                        self.users.push(user);
                        self.user_editor = None;
                        self.status = format!("Created user '{username}'");
                        let _ = self.save_to_cache().await;
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            UserEditMode::EditRole => {
                let Some(id) = editor.id else { return };
                match self.client.set_user_role(&id, &editor.role).await {
                    Ok(updated) => {
                        if let Some(u) = self.users.iter_mut().find(|u| u.id == updated.id) {
                            u.role = updated.role;
                        }
                        self.user_editor = None;
                        self.status = format!("Updated role for '{}'", editor.username);
                        let _ = self.save_to_cache().await;
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            UserEditMode::ChangePassword => {
                if editor.password.len() < 8 {
                    self.error = Some("new password must be at least 8 characters".to_string());
                    return;
                }
                if editor.password != editor.confirm_password {
                    self.error = Some("passwords do not match".to_string());
                    return;
                }
                match self
                    .client
                    .change_password(&editor.current_password, &editor.password)
                    .await
                {
                    Ok(_) => {
                        self.user_editor = None;
                        self.status = format!("Password changed for '{}'", editor.username);
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
        }
    }

    async fn delete_selected_user(&mut self) {
        let Some(user) = self.users.get(self.selected) else {
            return;
        };
        // Guard: cannot delete yourself
        if self
            .current_user
            .as_ref()
            .map(|u| u.id == user.id)
            .unwrap_or(false)
        {
            self.error = Some("cannot delete your own account".to_string());
            return;
        }
        let id = user.id.clone();
        let username = user.username.clone();
        match self.client.delete_user(&id).await {
            Ok(_) => {
                self.users.retain(|u| u.id != id);
                self.clamp_selection();
                self.status = format!("Deleted user '{username}'");
                let _ = self.save_to_cache().await;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    async fn activate_selected_scene(&mut self) {
        let Some(scene) = self.scenes.get(self.selected) else {
            return;
        };
        let scene_id = scene.id.clone();
        let scene_name = scene.name.clone();
        let is_hue_scene = self.devices.iter().any(|d| {
            d.device_id == scene_id
                && d.attributes.get("kind").and_then(Value::as_str) == Some("hue_scene")
        });
        let is_lutron_scene = scene_id.starts_with("lutron_scene_");
        let result = if is_hue_scene {
            self.client.activate_device_scene(&scene_id).await
        } else if is_lutron_scene {
            self.client.activate_lutron_device_scene(&scene_id).await
        } else {
            self.client.activate_scene(&scene_id).await
        };
        match result {
            Ok(_) => {
                self.status = format!("Activated scene '{scene_name}'");
                if let Err(err) = self.refresh_all().await {
                    self.error = Some(err.to_string());
                }
            }
            Err(err) => {
                self.error = Some(err.to_string());
            }
        }
    }

    // ── Manage tab ────────────────────────────────────────────────────────────

    fn open_manage_editor(&mut self) {
        self.error = None;
        match self.admin_sub {
            AdminSubPanel::Modes => {
                self.mode_editor = Some(ModeEditor {
                    id: String::new(),
                    name: String::new(),
                    kind: ModeKind::Solar,
                    field: ModeEditField::Id,
                });
            }
            AdminSubPanel::Matter => {
                // Commissioning is action-driven; no modal editor.
            }
            AdminSubPanel::Status => {
                // No create action for system status panel.
            }
            AdminSubPanel::Users => {
                self.user_editor = Some(UserEditor {
                    mode: UserEditMode::Create,
                    id: None,
                    username: String::new(),
                    current_password: String::new(),
                    password: String::new(),
                    confirm_password: String::new(),
                    role: crate::api::Role::User,
                    field: UserEditField::Username,
                });
            }
            AdminSubPanel::Logs => {
                // No create action for logs panel.
            }
            AdminSubPanel::Events => {
                // No create action for events panel.
            }
            AdminSubPanel::Audit => {
                // No create action — read-only viewer.
            }
            AdminSubPanel::Backup => {
                // No create action — Enter dispatches to action list directly.
            }
        }
    }

    async fn delete_selected_manage_item(&mut self) {
        match self.admin_sub {
            AdminSubPanel::Modes => {
                let Some(m) = self.modes.get(self.selected).cloned() else {
                    return;
                };
                let id = m.config.id.clone();
                match self.client.delete_mode(&id).await {
                    Ok(_) => {
                        self.modes.retain(|mo| mo.config.id != id);
                        self.clamp_selection();
                        self.status = format!("Deleted {id}");
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            AdminSubPanel::Matter => {
                self.remove_selected_matter_node().await;
            }
            AdminSubPanel::Status => {
                // No delete action for system status panel.
            }
            AdminSubPanel::Users => {
                let Some(u) = self.users.get(self.selected).cloned() else {
                    return;
                };
                let id = u.id.clone();
                match self.client.delete_user(&id).await {
                    Ok(_) => {
                        self.users.retain(|us| us.id != id);
                        self.clamp_selection();
                        self.status = format!("Deleted user {}", u.username);
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            AdminSubPanel::Logs => {
                // No delete action for logs panel.
            }
            AdminSubPanel::Events => {
                // No delete action for events panel.
            }
            AdminSubPanel::Audit => {
                // No delete action — audit log is append-only.
            }
            AdminSubPanel::Backup => {
                // No delete action for backup panel.
            }
        }
    }

    // ── Glue creator ─────────────────────────────────────────────────────────

    /// Modal for creating any of the 11 glue device types via the
    /// unified `POST /glue` endpoint. Type is cycled with Space when
    /// the cursor is on the Type field; Tab/BackTab navigates between
    /// fields, skipping ones not relevant to the chosen type.
    async fn on_key_glue_creator(&mut self, key: KeyEvent) {
        let Some(creator) = self.glue_creator.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                self.glue_creator = None;
                self.status = "Cancelled".to_string();
            }
            KeyCode::Tab => {
                let fields = GlueCreator::fields_for_type(creator.glue_type);
                let idx = fields.iter().position(|f| *f == creator.field).unwrap_or(0);
                creator.field = fields[(idx + 1) % fields.len()];
            }
            KeyCode::BackTab => {
                let fields = GlueCreator::fields_for_type(creator.glue_type);
                let idx = fields.iter().position(|f| *f == creator.field).unwrap_or(0);
                creator.field = fields[(idx + fields.len() - 1) % fields.len()];
            }
            KeyCode::Char(' ') if creator.field == GlueEditField::Type => {
                creator.glue_type = creator.glue_type.next();
                // If the new type makes the current field irrelevant, snap
                // back to the type cursor so the user can keep cycling.
                let fields = GlueCreator::fields_for_type(creator.glue_type);
                if !fields.contains(&creator.field) {
                    creator.field = GlueEditField::Type;
                }
            }
            KeyCode::Backspace => match creator.field {
                GlueEditField::Type => {}
                GlueEditField::Id => {
                    creator.id.pop();
                }
                GlueEditField::Name => {
                    creator.name.pop();
                }
                GlueEditField::Options => {
                    creator.options.pop();
                }
                GlueEditField::Members => {
                    creator.members.pop();
                }
                GlueEditField::SourceDeviceId => {
                    creator.source_device_id.pop();
                }
                GlueEditField::SourceAttribute => {
                    creator.source_attribute.pop();
                }
                GlueEditField::Threshold => {
                    creator.threshold.pop();
                }
            },
            KeyCode::Enter => {
                self.save_glue_creator().await;
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                match creator.field {
                    GlueEditField::Type => {} // Space cycles; other chars ignored
                    GlueEditField::Id => creator.id.push(ch),
                    GlueEditField::Name => creator.name.push(ch),
                    GlueEditField::Options => creator.options.push(ch),
                    GlueEditField::Members => creator.members.push(ch),
                    GlueEditField::SourceDeviceId => creator.source_device_id.push(ch),
                    GlueEditField::SourceAttribute => creator.source_attribute.push(ch),
                    GlueEditField::Threshold => creator.threshold.push(ch),
                }
            }
            _ => {}
        }
    }

    /// Build the type-specific config map and POST `/glue`. On success
    /// the new device is pushed onto `self.devices` (and into the right
    /// per-type list when applicable) and the modal closes.
    async fn save_glue_creator(&mut self) {
        let Some(creator) = self.glue_creator.clone() else {
            return;
        };
        let id = creator.id.trim().to_string();
        if id.is_empty() {
            self.error = Some("id cannot be empty".to_string());
            return;
        }
        let name = if creator.name.trim().is_empty() {
            id.clone()
        } else {
            creator.name.trim().to_string()
        };

        // Build type-specific config.
        let mut config = serde_json::Map::new();
        match creator.glue_type {
            GlueType::Select => {
                let opts: Vec<Value> = creator
                    .options
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| Value::String(s.to_string()))
                    .collect();
                if opts.is_empty() {
                    self.error = Some(
                        "select needs at least one option (comma-separated)".to_string(),
                    );
                    return;
                }
                config.insert("options".into(), Value::Array(opts));
            }
            GlueType::Group => {
                let members: Vec<Value> = creator
                    .members
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| Value::String(s.to_string()))
                    .collect();
                config.insert("members".into(), Value::Array(members));
            }
            GlueType::Threshold => {
                if creator.source_device_id.trim().is_empty() {
                    self.error = Some("threshold requires source_device_id".to_string());
                    return;
                }
                config.insert(
                    "source_device_id".into(),
                    Value::String(creator.source_device_id.trim().to_string()),
                );
                let attr = if creator.source_attribute.trim().is_empty() {
                    "value".to_string()
                } else {
                    creator.source_attribute.trim().to_string()
                };
                config.insert("source_attribute".into(), Value::String(attr));
                let threshold_val: f64 = match creator.threshold.trim().parse() {
                    Ok(n) => n,
                    Err(_) => {
                        self.error = Some("threshold must be a number".to_string());
                        return;
                    }
                };
                config.insert("threshold".into(), json!(threshold_val));
            }
            _ => {}
        }

        match self
            .client
            .create_glue(&id, &name, creator.glue_type.as_str(), Value::Object(config))
            .await
        {
            Ok(dev) => {
                let device_id = dev.device_id.clone();
                let glue_type_str = creator.glue_type.as_str();
                // Push to the right per-type vec for any types that have one.
                match creator.glue_type {
                    GlueType::Switch => self.switches.push(dev.clone()),
                    GlueType::Timer => self.timers.push(dev.clone()),
                    _ => {}
                }
                self.devices.push(dev);
                self.glue_creator = None;
                self.error = None;
                self.status = format!("Created {glue_type_str} {device_id}");
            }
            Err(e) => {
                if let Some(c) = self.glue_creator.as_mut() {
                    c.error = Some(e.to_string());
                }
                self.error = Some(e.to_string());
            }
        }
    }

    async fn on_key_switch_editor(&mut self, key: KeyEvent) {
        let Some(editor) = self.switch_editor.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                self.switch_editor = None;
                self.status = "Cancelled".to_string();
            }
            KeyCode::Tab | KeyCode::BackTab => {
                editor.field = match editor.field {
                    SwitchEditField::Id => SwitchEditField::Label,
                    SwitchEditField::Label => SwitchEditField::Id,
                };
            }
            KeyCode::Backspace => match editor.field {
                SwitchEditField::Id => {
                    editor.id.pop();
                }
                SwitchEditField::Label => {
                    editor.label.pop();
                }
            },
            KeyCode::Enter => {
                self.save_switch_editor().await;
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                match editor.field {
                    SwitchEditField::Id => editor.id.push(ch),
                    SwitchEditField::Label => editor.label.push(ch),
                }
            }
            _ => {}
        }
    }

    async fn save_switch_editor(&mut self) {
        let Some(editor) = self.switch_editor.clone() else {
            return;
        };
        let id = editor.id.trim().to_string();
        if id.is_empty() {
            self.error = Some("switch id cannot be empty".to_string());
            return;
        }
        let label = if editor.label.trim().is_empty() {
            editor.id.trim()
        } else {
            editor.label.trim()
        };
        match self.client.create_switch(&id, label).await {
            Ok(dev) => {
                let device_id = dev.device_id.clone();
                self.switches.push(dev.clone());
                self.devices.push(dev);
                self.switch_editor = None;
                self.error = None;
                self.status = format!("Created {device_id}");
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    async fn on_key_timer_editor(&mut self, key: KeyEvent) {
        let Some(editor) = self.timer_editor.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                self.timer_editor = None;
                self.status = "Cancelled".to_string();
            }
            KeyCode::Tab | KeyCode::BackTab => {
                editor.field = match editor.field {
                    TimerEditField::Id => TimerEditField::Label,
                    TimerEditField::Label => TimerEditField::Id,
                };
            }
            KeyCode::Backspace => match editor.field {
                TimerEditField::Id => {
                    editor.id.pop();
                }
                TimerEditField::Label => {
                    editor.label.pop();
                }
            },
            KeyCode::Enter => {
                self.save_timer_editor().await;
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                match editor.field {
                    TimerEditField::Id => editor.id.push(ch),
                    TimerEditField::Label => editor.label.push(ch),
                }
            }
            _ => {}
        }
    }

    async fn save_timer_editor(&mut self) {
        let Some(editor) = self.timer_editor.clone() else {
            return;
        };
        let id = editor.id.trim().to_string();
        if id.is_empty() {
            self.error = Some("timer id cannot be empty".to_string());
            return;
        }
        let label = if editor.label.trim().is_empty() {
            editor.id.trim()
        } else {
            editor.label.trim()
        };
        match self.client.create_timer(&id, label).await {
            Ok(dev) => {
                let device_id = dev.device_id.clone();
                self.timers.push(dev.clone());
                self.devices.push(dev);
                self.timer_editor = None;
                self.error = None;
                self.status = format!("Created {device_id}");
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    async fn on_key_mode_editor(&mut self, key: KeyEvent) {
        let Some(editor) = self.mode_editor.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                self.mode_editor = None;
                self.status = "Cancelled".to_string();
            }
            KeyCode::Tab | KeyCode::BackTab => {
                editor.field = match editor.field {
                    ModeEditField::Id => ModeEditField::Name,
                    ModeEditField::Name => ModeEditField::Kind,
                    ModeEditField::Kind => ModeEditField::Id,
                };
            }
            KeyCode::Char(' ') if editor.field == ModeEditField::Kind => {
                editor.kind = editor.kind.next();
            }
            KeyCode::Backspace => match editor.field {
                ModeEditField::Id => {
                    editor.id.pop();
                }
                ModeEditField::Name => {
                    editor.name.pop();
                }
                ModeEditField::Kind => {}
            },
            KeyCode::Enter => {
                self.save_mode_editor().await;
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                match editor.field {
                    ModeEditField::Id => editor.id.push(ch),
                    ModeEditField::Name => editor.name.push(ch),
                    ModeEditField::Kind => {}
                }
            }
            _ => {}
        }
    }

    async fn save_mode_editor(&mut self) {
        let Some(editor) = self.mode_editor.clone() else {
            return;
        };
        let id = editor.id.trim().to_string();
        if id.is_empty() {
            self.error = Some("mode id cannot be empty".to_string());
            return;
        }
        if !id.starts_with("mode_") {
            self.error = Some("mode id must start with 'mode_'".to_string());
            return;
        }
        let name = if editor.name.trim().is_empty() {
            editor.id.trim()
        } else {
            editor.name.trim()
        };
        match self
            .client
            .create_mode(&id, name, editor.kind.as_str())
            .await
        {
            Ok(cfg) => {
                let cfg_id = cfg.id.clone();
                self.modes.push(ModeRecord {
                    config: cfg,
                    state: None,
                });
                self.mode_editor = None;
                self.error = None;
                self.status = format!("Created {cfg_id}");
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }
}

/// Returns true if this device is a scene and should be excluded from the device list.
/// Scenes are shown in the Scenes tab instead.
pub fn is_scene_device(device: &DeviceState) -> bool {
    // Hue scenes have a kind attribute set by hc-hue
    if device.attributes.get("kind").and_then(Value::as_str) == Some("hue_scene") {
        return true;
    }
    // Lutron scene devices: phantom buttons, keypad phantom buttons, etc.
    device.device_id.starts_with("lutron_scene_")
}

fn is_hidden_in_devices_view(device: &DeviceState) -> bool {
    if is_scene_device(device) {
        return true;
    }

    // Hue zigbee_connectivity resources are internal connectivity diagnostics and
    // should not appear in the main interactive Devices view.
    if device.attributes.get("kind").and_then(Value::as_str) == Some("hue_zigbee_connectivity") {
        return true;
    }

    false
}

fn is_hidden_in_devices_view_with_context(
    device: &DeviceState,
    all_devices: &[DeviceState],
) -> bool {
    if is_hidden_in_devices_view(device) {
        return true;
    }

    // Compact Hue motion facets in the TUI when the corresponding motion device
    // exists: show one motion row that carries motion/temp/lux/battery values.
    let Some(kind) = device.attributes.get("kind").and_then(Value::as_str) else {
        return false;
    };
    if !matches!(
        kind,
        "hue_temperature" | "hue_light_level" | "hue_device_power"
    ) {
        return false;
    }

    all_devices.iter().any(|other| {
        if other.plugin_id != device.plugin_id || other.name != device.name {
            return false;
        }
        other.attributes.get("kind").and_then(Value::as_str) == Some("hue_motion")
    })
}

/// Extract hue scene devices from the device list and convert them to Scene entries.
fn hue_scenes_from_devices(devices: &[DeviceState]) -> Vec<Scene> {
    devices
        .iter()
        .filter(|d| is_scene_device(d))
        .map(|d| {
            let scene_name = d
                .attributes
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(&d.name)
                .to_string();
            let area = d.area.clone().or_else(|| {
                d.attributes
                    .get("group_name")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            });
            let active = d.attributes.get("active").and_then(Value::as_bool);
            Scene {
                id: d.device_id.clone(),
                name: scene_name,
                plugin_id: Some(d.plugin_id.clone()),
                area,
                active,
            }
        })
        .collect()
}

fn summarize_live_event_detail(event: &Value) -> Option<String> {
    let event_type = event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    match event_type {
        "device_button" => event
            .get("event")
            .and_then(Value::as_str)
            .map(|v| format!("button_event={v}")),
        "device_rotary" => {
            let action = event.get("action").and_then(Value::as_str);
            let direction = event.get("direction").and_then(Value::as_str);
            let steps = event.get("steps").and_then(Value::as_i64);
            let mut parts = Vec::new();
            if let Some(v) = action {
                parts.push(format!("action={v}"));
            }
            if let Some(v) = direction {
                parts.push(format!("direction={v}"));
            }
            if let Some(v) = steps {
                parts.push(format!("steps={v}"));
            }
            if parts.is_empty() {
                None
            } else {
                Some(parts.join(" "))
            }
        }
        "plugin_command_result" => {
            let operation = event
                .get("operation")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let success = event
                .get("success")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let error_code = event.get("error_code").and_then(Value::as_str);
            let latency_ms = event.get("latency_ms").and_then(Value::as_u64);
            let error = event.get("error").and_then(Value::as_str);

            let mut parts = Vec::new();
            parts.push(format!("op={operation}"));

            if success {
                parts.push("success".to_string());
            } else {
                parts.push("failed".to_string());
                if let Some(code) = error_code {
                    parts.push(format!("err_code={code}"));
                }
                if let Some(msg) = error {
                    // Truncate long error messages
                    let msg_short = if msg.len() > 30 {
                        format!("{}...", &msg[..27])
                    } else {
                        msg.to_string()
                    };
                    parts.push(format!("msg={msg_short}"));
                }
            }

            if let Some(ms) = latency_ms {
                parts.push(format!("{ms}ms"));
            }

            if parts.is_empty() {
                None
            } else {
                Some(parts.join(" "))
            }
        }
        "bridge_pairing_status" => {
            let phase = event
                .get("phase")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let success = event.get("success").and_then(Value::as_bool);
            let error = event.get("error").and_then(Value::as_str);

            let mut parts = vec![format!("phase={phase}")];
            if let Some(v) = success {
                parts.push(if v {
                    "success".to_string()
                } else {
                    "failed".to_string()
                });
            }
            if let Some(msg) = error {
                let msg_short = if msg.len() > 30 {
                    format!("{}...", &msg[..27])
                } else {
                    msg.to_string()
                };
                parts.push(format!("msg={msg_short}"));
            }
            Some(parts.join(" "))
        }
        "plugin_metrics" => {
            let fallback = event
                .get("eventstream_fallback_refresh_total")
                .and_then(Value::as_u64);
            let applied = event
                .get("eventstream_incremental_applied_total")
                .and_then(Value::as_u64);
            let ratio = event
                .get("eventstream_fallback_ratio_pct")
                .and_then(Value::as_f64);
            let recent_fallback = event
                .get("eventstream_fallback_refresh_recent")
                .and_then(Value::as_u64);
            let recent_applied = event
                .get("eventstream_incremental_applied_recent")
                .and_then(Value::as_u64);
            let recent_ratio = event
                .get("eventstream_fallback_ratio_recent_pct")
                .and_then(Value::as_f64);

            let mut parts = Vec::new();
            if let (Some(f), Some(a), Some(r)) = (fallback, applied, ratio) {
                parts.push(format!(
                    "fallback={f} incremental={a} fallback_ratio={r:.2}%"
                ));
            }
            if let (Some(f), Some(a), Some(r)) = (recent_fallback, recent_applied, recent_ratio) {
                parts.push(format!(
                    "recent_fallback={f} recent_incremental={a} recent_ratio={r:.2}%"
                ));
            }

            if parts.is_empty() {
                None
            } else {
                Some(parts.join(" | "))
            }
        }
        "entertainment_action_applied" => {
            let action = event.get("action").and_then(Value::as_str);
            let config_id = event.get("config_id").and_then(Value::as_str);
            let active = event.get("active").and_then(Value::as_bool);

            let mut parts = Vec::new();
            if let Some(v) = action {
                parts.push(format!("action={v}"));
            }
            if let Some(v) = config_id {
                parts.push(format!("config_id={v}"));
            }
            if let Some(v) = active {
                parts.push(format!("active={v}"));
            }
            if parts.is_empty() {
                None
            } else {
                Some(parts.join(" "))
            }
        }
        "entertainment_status_changed" => {
            let config_id = event.get("config_id").and_then(Value::as_str);
            let active = event.get("active").and_then(Value::as_bool);
            let status = event.get("status").and_then(Value::as_str);
            let etype = event.get("entertainment_type").and_then(Value::as_str);

            let mut parts = Vec::new();
            if let Some(v) = config_id {
                parts.push(format!("config_id={v}"));
            }
            if let Some(v) = active {
                parts.push(format!("active={v}"));
            }
            if let Some(v) = status {
                parts.push(format!("status={v}"));
            }
            if let Some(v) = etype {
                parts.push(format!("type={v}"));
            }
            if parts.is_empty() {
                None
            } else {
                Some(parts.join(" "))
            }
        }
        _ => None,
    }
}

fn summarize_matter_plugin_metric(event: &Value) -> Option<String> {
    let phase = event.get("phase").and_then(Value::as_str);
    let result = event.get("result").and_then(Value::as_str);
    let reason = event.get("reason").and_then(Value::as_str);
    let timeout_ms = event.get("timeout_ms").and_then(Value::as_u64);
    let nodes = event.get("commissioned_nodes").and_then(Value::as_u64);
    let bridged = event.get("bridged_endpoints").and_then(Value::as_u64);
    let failed = event.get("failed_commands").and_then(Value::as_u64);
    let latency = event.get("command_latency_ms").and_then(Value::as_u64);
    let loop_prevented = event.get("loop_prevented_writes").and_then(Value::as_u64);

    let mut parts = Vec::new();
    if let Some(v) = phase {
        parts.push(format!("phase={v}"));
    }
    if let Some(v) = result {
        parts.push(format!("result={v}"));
    }
    if let Some(v) = reason {
        parts.push(format!("reason={v}"));
    }
    if let Some(v) = timeout_ms {
        parts.push(format!("timeout={v}ms"));
    }
    if let Some(v) = nodes {
        parts.push(format!("nodes={v}"));
    }
    if let Some(v) = bridged {
        parts.push(format!("bridged={v}"));
    }
    if let Some(v) = failed {
        parts.push(format!("failed={v}"));
    }
    if let Some(v) = latency {
        parts.push(format!("latency={v}ms"));
    }
    if let Some(v) = loop_prevented {
        parts.push(format!("loop_prevented={v}"));
    }

    if parts.is_empty() {
        return None;
    }

    Some(parts.join(" "))
}

fn humanize_matter_block_reason(reason: &str) -> &'static str {
    match reason {
        "no_commissionable_device_discovered" => "no commissionable device discovered",
        _ => "commissioning blocked by plugin",
    }
}

fn normalize_label(value: &str) -> String {
    let mut spaced = String::with_capacity(value.len() + 4);
    let mut prev_lower = false;
    for ch in value.chars() {
        if ch.is_uppercase() && prev_lower {
            spaced.push(' ');
        }
        prev_lower = ch.is_lowercase();
        spaced.push(ch);
    }
    spaced
        .replace('_', " ")
        .split_whitespace()
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!(
                    "{}{}",
                    first.to_ascii_uppercase(),
                    chars.as_str().to_ascii_lowercase()
                ),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub async fn login_workflow_from_auth(
    mut client: HomeCoreClient,
    cache: CacheStore,
    auth: LoginResponse,
) -> Result<LoginWorkflowResult> {
    client.set_token(auth.token.clone());

    let cached = cache
        .load_snapshot(&auth.user.username)
        .await
        .unwrap_or_default();

    let fetched = fetch_remote_snapshot(&client, auth.user.role.clone()).await;
    match fetched {
        Ok((snapshot, warning)) => {
            cache.save_snapshot(&auth.user.username, &snapshot).await?;
            Ok(LoginWorkflowResult {
                auth,
                snapshot,
                warning,
            })
        }
        Err(err) => {
            if snapshot_is_empty(&cached) {
                Err(err)
            } else {
                Ok(LoginWorkflowResult {
                    auth,
                    snapshot: cached,
                    warning: Some(err.to_string()),
                })
            }
        }
    }
}

async fn fetch_remote_snapshot(
    client: &HomeCoreClient,
    role: Role,
) -> Result<(CacheSnapshot, Option<String>)> {
    let devices = client.list_devices().await.unwrap_or_default();
    let mut scenes = client.list_scenes().await.unwrap_or_default();
    scenes.extend(hue_scenes_from_devices(&devices));
    let areas = client.list_areas().await.unwrap_or_default();
    let rules = client.list_rules().await.unwrap_or_default();
    let events = client.list_events(50).await.unwrap_or_default();
    let switches = client.list_switches().await.unwrap_or_default();
    let timers = client.list_timers().await.unwrap_or_default();
    let modes = client.list_modes().await.unwrap_or_default();
    let (users, plugins) = if role.is_admin() {
        (
            client.list_users().await.unwrap_or_default(),
            client.list_plugins().await.unwrap_or_default(),
        )
    } else {
        (Vec::new(), Vec::new())
    };

    Ok((
        CacheSnapshot {
            devices,
            scenes,
            areas,
            rules,
            events,
            users,
            plugins,
            switches,
            timers,
            modes,
        },
        None,
    ))
}

fn snapshot_is_empty(snapshot: &CacheSnapshot) -> bool {
    snapshot.devices.is_empty()
        && snapshot.scenes.is_empty()
        && snapshot.areas.is_empty()
        && snapshot.rules.is_empty()
        && snapshot.events.is_empty()
        && snapshot.users.is_empty()
        && snapshot.plugins.is_empty()
}

/// Format a timestamp string for display. Respects the `utc` flag.
pub fn format_timestamp_utc(ts: &str, utc: bool) -> String {
    if utc {
        // Show as UTC with date+time
        if let Ok(dt) = DateTime::parse_from_rfc3339(ts) {
            return dt
                .with_timezone(&Utc)
                .format("%Y-%m-%d %H:%M:%S UTC")
                .to_string();
        }
    } else {
        // Show as local time (time only for brevity)
        if let Ok(dt) = DateTime::parse_from_rfc3339(ts) {
            return dt.with_timezone(&Local).format("%H:%M:%S").to_string();
        }
    }
    // Fallback: trim to first 19 chars
    ts.chars().take(19).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_app() -> App {
        App::new(
            "http://127.0.0.1:8080".to_string(),
            CacheStore::new(PathBuf::from("/tmp/hc-tui-tests")),
        )
    }

    fn mk_event(event_type: &str) -> EventEntry {
        EventEntry {
            event_type: event_type.to_string(),
            timestamp: "2026-03-21T00:00:00Z".to_string(),
            plugin_id: None,
            device_id: None,
            rule_name: None,
            event_type_custom: None,
            event_detail: None,
        }
    }

    fn make_admin(app: &mut App) {
        app.current_user = Some(UserInfo {
            id: "u1".to_string(),
            username: "admin".to_string(),
            role: Role::Admin,
            created_at: "2026-03-21T00:00:00Z".to_string(),
        });
        app.authenticated = true;
    }

    fn make_user(app: &mut App) {
        app.current_user = Some(UserInfo {
            id: "u2".to_string(),
            username: "user".to_string(),
            role: Role::User,
            created_at: "2026-03-21T00:00:00Z".to_string(),
        });
        app.authenticated = true;
    }

    #[test]
    fn test_admin_has_more_tabs() {
        let mut app = test_app();
        make_user(&mut app);
        let user_tabs = app.tabs().len();
        make_admin(&mut app);
        let admin_tabs = app.tabs().len();
        assert!(admin_tabs > user_tabs);
    }

    #[test]
    fn test_events_filter_all_passes() {
        let mut app = test_app();
        app.events_filter_mode = EventsFilterMode::All;
        let e = mk_event("device_state_changed");
        assert!(app.event_matches_filter(&e));
    }

    #[test]
    fn test_log_level_filter_error_only() {
        let filter = LogLevelFilter::Error;
        assert!(filter.passes("ERROR"));
        assert!(!filter.passes("WARN"));
        assert!(!filter.passes("INFO"));
    }

    #[test]
    fn test_log_level_filter_info_includes_warn_error() {
        let filter = LogLevelFilter::Info;
        assert!(filter.passes("ERROR"));
        assert!(filter.passes("WARN"));
        assert!(filter.passes("INFO"));
        assert!(!filter.passes("DEBUG"));
    }

    #[test]
    fn test_rule_stale_filter() {
        let mut app = test_app();
        app.rule_filter_stale = true;
        let rule_ok = Rule {
            id: "r1".to_string(),
            name: "ok".to_string(),
            enabled: true,
            priority: 0,
            tags: vec![],
            error: None,
            trigger: None,
        };
        let rule_stale = Rule {
            id: "r2".to_string(),
            name: "stale".to_string(),
            enabled: true,
            priority: 0,
            tags: vec![],
            error: Some("parse error".to_string()),
            trigger: None,
        };
        assert!(!app.rule_matches_filter(&rule_ok));
        assert!(app.rule_matches_filter(&rule_stale));
    }

    #[test]
    fn test_device_search_matches_canonical_name() {
        let mut app = test_app();
        app.device_search_query = "living_room.floor_lamp".to_string();
        app.devices.push(DeviceState {
            device_id: "light_living".to_string(),
            canonical_name: Some("living_room.floor_lamp".to_string()),
            name: "Living Floor Lamp".to_string(),
            plugin_id: "plugin.hue".to_string(),
            device_type: Some("light".to_string()),
            area: Some("Living Room".to_string()),
            available: true,
            attributes: serde_json::Map::new(),
            last_seen: "2026-03-21T00:00:00Z".to_string(),
        });

        assert_eq!(app.visible_devices().len(), 1);
    }

    #[test]
    fn test_media_player_toggle_action_prefers_stop_when_playing() {
        let mut attributes = serde_json::Map::new();
        attributes.insert("state".to_string(), Value::String("playing".to_string()));

        let device = DeviceState {
            device_id: "sonos_living".to_string(),
            canonical_name: None,
            name: "Living Room".to_string(),
            plugin_id: "plugin.sonos".to_string(),
            device_type: Some("media_player".to_string()),
            area: None,
            available: true,
            attributes,
            last_seen: String::new(),
        };

        assert_eq!(App::media_player_toggle_action(&device), Some("stop"));
    }

    #[test]
    fn test_media_player_toggle_action_prefers_play_when_idle() {
        let mut attributes = serde_json::Map::new();
        attributes.insert("state".to_string(), Value::String("paused".to_string()));

        let device = DeviceState {
            device_id: "sonos_living".to_string(),
            canonical_name: None,
            name: "Living Room".to_string(),
            plugin_id: "plugin.sonos".to_string(),
            device_type: Some("media_player".to_string()),
            area: None,
            available: true,
            attributes,
            last_seen: String::new(),
        };

        assert_eq!(App::media_player_toggle_action(&device), Some("play"));
    }

    #[test]
    fn test_media_player_model_uses_supported_actions() {
        let mut attributes = serde_json::Map::new();
        attributes.insert("state".to_string(), Value::String("playing".to_string()));
        attributes.insert(
            "supported_actions".to_string(),
            Value::Array(vec![
                Value::String("play".to_string()),
                Value::String("pause".to_string()),
                Value::String("next".to_string()),
                Value::String("set_volume".to_string()),
            ]),
        );
        attributes.insert("volume".to_string(), Value::from(42));

        let device = DeviceState {
            device_id: "media_1".to_string(),
            canonical_name: Some("living.media_1".to_string()),
            name: "Player".to_string(),
            plugin_id: "plugin.generic".to_string(),
            device_type: Some("media_player".to_string()),
            area: None,
            available: true,
            attributes,
            last_seen: String::new(),
        };

        let model = App::media_player_model(&device).expect("media player model");
        assert!(model.capabilities.can_play);
        assert!(model.capabilities.can_pause);
        assert!(model.capabilities.can_next);
        assert!(model.capabilities.can_set_volume);
        assert!(!model.capabilities.can_previous);
        assert!(!model.capabilities.can_stop);
        assert_eq!(model.volume, Some(42));
        assert_eq!(model.canonical_name.as_deref(), Some("living.media_1"));
    }

    #[test]
    fn test_media_player_model_applies_plugin_hook_enrichment() {
        let mut attributes = serde_json::Map::new();
        attributes.insert("state".to_string(), Value::String("playing".to_string()));
        attributes.insert(
            "available_favorites".to_string(),
            Value::Array(vec![Value::String("one".to_string())]),
        );
        attributes.insert(
            "available_playlists".to_string(),
            Value::Array(vec![
                Value::String("alpha".to_string()),
                Value::String("beta".to_string()),
            ]),
        );

        let device = DeviceState {
            device_id: "sonos_living".to_string(),
            canonical_name: None,
            name: "Living Room".to_string(),
            plugin_id: "plugin.sonos".to_string(),
            device_type: Some("media_player".to_string()),
            area: None,
            available: true,
            attributes,
            last_seen: String::new(),
        };

        let model = App::media_player_model(&device).expect("media player model");
        assert!(model.capabilities.can_stop);
        assert!(model.capabilities.can_next);
        assert!(model.capabilities.can_previous);
        assert!(model.capabilities.can_set_volume);
        assert!(model.capabilities.can_mute);
        assert_eq!(model.extra_details.len(), 2);
    }
}

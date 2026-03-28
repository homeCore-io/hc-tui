use crate::api::{
    Area, DeviceState, EventEntry, HomeCoreClient, LogLine, LoginResponse, ModeRecord,
    MatterNode, PluginRecord, Role, Rule, RuleFiring, RuleGroup, Scene, SystemStatus, UserInfo,
};
use crate::cache::{CacheSnapshot, CacheStore};
use anyhow::Result;
use chrono::{DateTime, Local, Utc};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::Value;
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
}

#[derive(Debug, Clone)]
pub struct DeviceEditor {
    pub device_id: String,
    pub name: String,
    pub area: String,
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
    Diagnostics,
    Metrics,
}

impl PluginDetailPanel {
    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Diagnostics => "Diagnostics",
            Self::Metrics => "Metrics",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceSubPanel {
    All,
    Switches,
    Timers,
}

impl DeviceSubPanel {
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

/// Automation filter bar state.
#[derive(Debug, Clone)]
pub struct AutomationFilterBar {
    pub tag: String,
    pub trigger: String,
    pub stale: bool,
    pub active_field: AutomationFilterField,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationFilterField {
    Tag,
    Trigger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Devices,
    Scenes,
    Areas,
    Automations,
    Plugins,
    Manage,
}

impl Tab {
    pub fn title(self) -> &'static str {
        match self {
            Self::Devices => "Devices",
            Self::Scenes => "Scenes",
            Self::Areas => "Areas",
            Self::Automations => "Automations",
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
    pub devices: Vec<DeviceState>,
    pub scenes: Vec<Scene>,
    pub areas: Vec<Area>,
    pub automations: Vec<Rule>,
    pub events: Vec<EventEntry>,
    pub users: Vec<UserInfo>,
    pub plugins: Vec<PluginRecord>,
    pub matter_nodes: Vec<MatterNode>,
    pub matter_last_action: String,
    pub matter_pending: bool,
    pub matter_last_node_count: usize,
    pub matter_activity: VecDeque<String>,
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
    pub switches: Vec<DeviceState>,
    pub timers: Vec<DeviceState>,
    pub modes: Vec<ModeRecord>,
    pub switch_editor: Option<SwitchEditor>,
    pub timer_editor: Option<TimerEditor>,
    pub mode_editor: Option<ModeEditor>,
    pub matter_commission_editor: Option<MatterCommissionEditor>,

    // Automations tab features
    pub automation_filter_tag: String,
    pub automation_filter_trigger: String,
    pub automation_filter_stale: bool,
    pub automation_filter_bar: Option<AutomationFilterBar>,
    pub automation_selected_ids: HashSet<String>,
    pub automation_bulk_select_mode: bool,
    pub fire_history_open: bool,
    pub fire_history_rule_id: Option<String>,
    pub fire_history: Vec<RuleFiring>,
    pub automation_delete_confirm: Option<DeleteConfirm>,
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
            devices: Vec::new(),
            scenes: Vec::new(),
            areas: Vec::new(),
            automations: Vec::new(),
            events: Vec::new(),
            users: Vec::new(),
            plugins: Vec::new(),
            matter_nodes: Vec::new(),
            matter_last_action: "No Matter operation started".to_string(),
            matter_pending: false,
            matter_last_node_count: 0,
            matter_activity: VecDeque::new(),
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
            switches: Vec::new(),
            timers: Vec::new(),
            modes: Vec::new(),
            switch_editor: None,
            timer_editor: None,
            mode_editor: None,
            matter_commission_editor: None,

            automation_filter_tag: String::new(),
            automation_filter_trigger: String::new(),
            automation_filter_stale: false,
            automation_filter_bar: None,
            automation_selected_ids: HashSet::new(),
            automation_bulk_select_mode: false,
            fire_history_open: false,
            fire_history_rule_id: None,
            fire_history: Vec::new(),
            automation_delete_confirm: None,
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
            Tab::Automations,
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
        self.automations = self.client.list_automations().await?;
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
            automations: self.automations.clone(),
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
        self.automations = snapshot.automations;
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
                    if let Some(device) = self.devices.iter_mut().find(|d| d.device_id == device_id) {
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
                    if let Some(device) = self.devices.iter_mut().find(|d| d.device_id == device_id) {
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
                        if let Some(device) = self.devices.iter_mut().find(|d| d.device_id == device_id) {
                            device.name = name.to_string();
                        }
                    }
                }
            }
            _ => {}
        }

        let entry = EventEntry {
            event_type,
            timestamp,
            plugin_id: event
                .get("plugin_id")
                .and_then(Value::as_str)
                .map(ToString::to_string),
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
            event_detail: summarize_live_event_detail(&event),
        };
        self.events.insert(0, entry);
        self.events.truncate(200);
    }

    /// Returns the currently visible automations (after applying filters).
    pub fn visible_automations(&self) -> Vec<&Rule> {
        self.automations
            .iter()
            .filter(|r| self.automation_matches_filter(r))
            .collect()
    }

    fn automation_matches_filter(&self, rule: &Rule) -> bool {
        if self.automation_filter_stale && rule.error.is_none() {
            return false;
        }
        if !self.automation_filter_tag.is_empty() {
            let tag = &self.automation_filter_tag;
            if !rule.tags.iter().any(|t| t.contains(tag.as_str())) {
                return false;
            }
        }
        if !self.automation_filter_trigger.is_empty() && self.automation_filter_trigger != "all" {
            let trigger_type = rule
                .trigger
                .as_ref()
                .and_then(|t| t.get("type").and_then(Value::as_str))
                .unwrap_or("");
            if !trigger_type.contains(self.automation_filter_trigger.as_str()) {
                return false;
            }
        }
        true
    }

    pub fn selected_automation(&self) -> Option<&Rule> {
        let visible = self.visible_automations();
        visible.get(self.selected).copied()
    }

    pub async fn on_key_authenticated(&mut self, key: KeyEvent) {
        self.error = None;

        // Handle delete confirmation dialog first
        if self.automation_delete_confirm.is_some() {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.confirm_delete_automation().await;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.automation_delete_confirm = None;
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
                    self.status = "Closed plugin detail".to_string();
                }
                KeyCode::Char('1') => {
                    self.plugin_detail_panel = PluginDetailPanel::Overview;
                }
                KeyCode::Char('2') => {
                    self.plugin_detail_panel = PluginDetailPanel::Diagnostics;
                }
                KeyCode::Char('3') => {
                    self.plugin_detail_panel = PluginDetailPanel::Metrics;
                }
                KeyCode::Left | KeyCode::BackTab => {
                    self.cycle_plugin_detail_panel(false);
                }
                KeyCode::Right | KeyCode::Tab => {
                    self.cycle_plugin_detail_panel(true);
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
        if self.matter_commission_editor.is_some() {
            self.on_key_matter_commission_editor(key).await;
            return;
        }

        if self.device_search_input_open {
            self.on_key_device_search_input(key);
            return;
        }

        // Automation filter bar
        if self.automation_filter_bar.is_some() {
            self.on_key_automation_filter_bar(key).await;
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
            && !matches!(key.code, 
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
                         | KeyCode::Char('T')) {
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
            KeyCode::Char('r') => {
                match self.active_tab() {
                    Tab::Manage if matches!(self.admin_sub, AdminSubPanel::Matter) => {
                        self.refresh_matter_nodes().await;
                    }
                    Tab::Manage if matches!(self.admin_sub, AdminSubPanel::Status) => {
                        self.refresh_system_status().await;
                    }
                    _ => {
                        if let Err(err) = self.refresh_all().await {
                            self.error = Some(err.to_string());
                        }
                    }
                }
            }
            KeyCode::Left if matches!(self.active_tab(), Tab::Devices) => {
                self.device_sub = match self.device_sub {
                    DeviceSubPanel::All => DeviceSubPanel::Timers,
                    DeviceSubPanel::Switches => DeviceSubPanel::All,
                    DeviceSubPanel::Timers => DeviceSubPanel::Switches,
                };
                self.selected = 0;
                self.error = None;
            }
            KeyCode::Right if matches!(self.active_tab(), Tab::Devices) => {
                self.device_sub = match self.device_sub {
                    DeviceSubPanel::All => DeviceSubPanel::Switches,
                    DeviceSubPanel::Switches => DeviceSubPanel::Timers,
                    DeviceSubPanel::Timers => DeviceSubPanel::All,
                };
                self.selected = 0;
                self.error = None;
            }
            KeyCode::Left if matches!(self.active_tab(), Tab::Manage) => {
                self.admin_sub = match self.admin_sub {
                    AdminSubPanel::Modes => AdminSubPanel::Events,
                    AdminSubPanel::Matter => AdminSubPanel::Modes,
                    AdminSubPanel::Status => AdminSubPanel::Matter,
                    AdminSubPanel::Users => AdminSubPanel::Status,
                    AdminSubPanel::Logs => AdminSubPanel::Users,
                    AdminSubPanel::Events => AdminSubPanel::Logs,
                };
                self.selected = 0;
                self.error = None;
                if matches!(self.admin_sub, AdminSubPanel::Matter) {
                    self.refresh_matter_nodes().await;
                }
                if matches!(self.admin_sub, AdminSubPanel::Status) {
                    self.refresh_system_status().await;
                }
            }
            KeyCode::Right if matches!(self.active_tab(), Tab::Manage) => {
                self.admin_sub = match self.admin_sub {
                    AdminSubPanel::Modes => AdminSubPanel::Matter,
                    AdminSubPanel::Matter => AdminSubPanel::Status,
                    AdminSubPanel::Status => AdminSubPanel::Users,
                    AdminSubPanel::Users => AdminSubPanel::Logs,
                    AdminSubPanel::Logs => AdminSubPanel::Events,
                    AdminSubPanel::Events => AdminSubPanel::Modes,
                };
                self.selected = 0;
                self.error = None;
                if matches!(self.admin_sub, AdminSubPanel::Matter) {
                    self.refresh_matter_nodes().await;
                }
                if matches!(self.admin_sub, AdminSubPanel::Status) {
                    self.refresh_system_status().await;
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
                if matches!(self.active_tab(), Tab::Manage) && matches!(self.admin_sub, AdminSubPanel::Status) {
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
                if matches!(self.active_tab(), Tab::Manage) && matches!(self.admin_sub, AdminSubPanel::Status) {
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
                    if matches!(self.active_tab(), Tab::Manage) && matches!(self.admin_sub, AdminSubPanel::Status) {
                        self.refresh_system_status().await;
                    }
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                                // Exclude Logs sub-panel in Manage
                                if matches!(self.active_tab(), Tab::Manage) && matches!(self.admin_sub, AdminSubPanel::Logs) {
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
                                if matches!(self.active_tab(), Tab::Manage) && matches!(self.admin_sub, AdminSubPanel::Logs) {
                                    if self.log_paused {
                                        self.log_scroll_offset = self.log_scroll_offset.saturating_sub(1);
                                    }
                                    return;
                                }
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Enter => {
                match self.active_tab() {
                    Tab::Devices => {
                        match self.device_sub {
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
                        }
                    }
                    Tab::Areas   => self.open_area_editor_edit(),
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
                        } else {
                            self.open_manage_editor();
                        }
                    }
                    _ => {}
                }
            }
            KeyCode::Char('n') => {
                match self.active_tab() {
                    Tab::Devices => {
                        match self.device_sub {
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
                        }
                    }
                    Tab::Areas => self.open_area_editor_create(),
                    Tab::Manage => {
                        if matches!(self.admin_sub, AdminSubPanel::Matter) {
                            self.open_matter_commission_editor();
                        } else if matches!(self.admin_sub, AdminSubPanel::Users) {
                            if self.is_admin() {
                                self.open_user_editor_create();
                            }
                        } else {
                            self.open_manage_editor();
                        }
                    }
                    _ => {}
                }
            }
            KeyCode::Char('d') => {
                match self.active_tab() {
                    Tab::Devices => {
                        match self.device_sub {
                            DeviceSubPanel::Switches => self.delete_selected_device_switch().await,
                            DeviceSubPanel::Timers => self.delete_selected_device_timer().await,
                            DeviceSubPanel::All => self.delete_selected_device().await,
                        }
                    }
                    Tab::Areas   => self.delete_selected_area().await,
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
                    Tab::Automations => self.disable_selected_automation().await,
                    _ => {}
                }
            }
            KeyCode::Char('D') => {
                match self.active_tab() {
                    Tab::Automations => {
                        if self.automation_bulk_select_mode && !self.automation_selected_ids.is_empty() {
                            self.bulk_disable_automations().await;
                        } else {
                            self.disable_selected_automation().await;
                        }
                    }
                    _ => {}
                }
            }
            KeyCode::Char('p') => {
                match self.active_tab() {
                    Tab::Manage if matches!(self.admin_sub, AdminSubPanel::Users) => self.open_user_editor_password(),
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
                }
            }
            KeyCode::Char(' ') => {
                match self.active_tab() {
                    Tab::Devices => self.toggle_lock_or_switch().await,
                    Tab::Automations => self.toggle_automation_selection(),
                    Tab::Manage if matches!(self.admin_sub, AdminSubPanel::Logs) => {
                        self.log_paused = !self.log_paused;
                        if !self.log_paused {
                            self.log_scroll_offset = self.log_lines.len().saturating_sub(1);
                        }
                    }
                    _ => {}
                }
            }
            KeyCode::Char('t') => {
                match self.active_tab() {
                    Tab::Devices => self.toggle_selected_device().await,
                    _ => {}
                }
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
                    self.adjust_brightness(1).await;
                }
            }
            KeyCode::Char('-') => {
                if matches!(self.active_tab(), Tab::Devices) {
                    self.adjust_brightness(-1).await;
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
                    Tab::Automations => {
                        // Toggle filter bar
                        if self.automation_filter_bar.is_none() {
                            self.automation_filter_bar = Some(AutomationFilterBar {
                                tag: self.automation_filter_tag.clone(),
                                trigger: self.automation_filter_trigger.clone(),
                                stale: self.automation_filter_stale,
                                active_field: AutomationFilterField::Tag,
                            });
                        } else {
                            self.automation_filter_bar = None;
                        }
                    }
                    _ => {}
                }
            }
            KeyCode::Char('h') | KeyCode::Char('H') => {
                if matches!(self.active_tab(), Tab::Automations) {
                    self.open_fire_history().await;
                }
            }
            KeyCode::Char('c') | KeyCode::Char('C') => {
                if matches!(self.active_tab(), Tab::Automations) {
                    self.clone_selected_automation().await;
                } else if matches!(self.active_tab(), Tab::Manage)
                    && matches!(self.admin_sub, AdminSubPanel::Matter)
                {
                    self.open_matter_commission_editor();
                } else if matches!(self.active_tab(), Tab::Manage) && matches!(self.admin_sub, AdminSubPanel::Logs) {
                    self.log_lines.clear();
                    self.log_scroll_offset = 0;
                    self.status = "Log buffer cleared".to_string();
                }
            }
            KeyCode::Char('e') => {
                match self.active_tab() {
                    Tab::Automations => {
                        if self.automation_bulk_select_mode && !self.automation_selected_ids.is_empty() {
                            self.bulk_enable_automations().await;
                        } else {
                            self.enable_selected_automation().await;
                        }
                    }
                    Tab::Manage if matches!(self.admin_sub, AdminSubPanel::Logs) => {
                        self.log_level_filter = LogLevelFilter::Error;
                        self.status = "Log level: ERROR".to_string();
                    }
                    _ => {}
                }
            }
            KeyCode::Char('E') => {
                match self.active_tab() {
                    Tab::Automations => {
                        if self.automation_bulk_select_mode && !self.automation_selected_ids.is_empty() {
                            self.bulk_enable_automations().await;
                        } else {
                            self.enable_selected_automation().await;
                        }
                    }
                    _ => {}
                }
            }
            KeyCode::Char('w') => {
                if matches!(self.active_tab(), Tab::Manage) && matches!(self.admin_sub, AdminSubPanel::Logs) {
                    self.log_level_filter = LogLevelFilter::Warn;
                    self.status = "Log level: WARN".to_string();
                }
            }
            KeyCode::Char('i') => {
                if matches!(self.active_tab(), Tab::Manage)
                    && matches!(self.admin_sub, AdminSubPanel::Matter)
                {
                    self.reinterview_selected_matter_node().await;
                } else if matches!(self.active_tab(), Tab::Manage) && matches!(self.admin_sub, AdminSubPanel::Logs) {
                    self.log_level_filter = LogLevelFilter::Info;
                    self.status = "Log level: INFO".to_string();
                }
            }
            KeyCode::Char('/') => {
                if matches!(self.active_tab(), Tab::Manage) && matches!(self.admin_sub, AdminSubPanel::Logs) {
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
                if matches!(self.active_tab(), Tab::Automations) {
                    self.open_groups_panel().await;
                }
            }
            KeyCode::Char('s') => {
                if matches!(self.active_tab(), Tab::Automations) {
                    self.automation_filter_stale = !self.automation_filter_stale;
                    self.selected = 0;
                    self.clamp_selection();
                    self.status = if self.automation_filter_stale {
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
                if matches!(self.active_tab(), Tab::Automations) {
                    self.initiate_delete_automation();
                }
            }
            KeyCode::Esc => {
                match self.active_tab() {
                    Tab::Automations => {
                        if self.fire_history_open {
                            self.fire_history_open = false;
                            self.fire_history_rule_id = None;
                            self.fire_history.clear();
                        } else if self.automation_bulk_select_mode {
                            self.automation_bulk_select_mode = false;
                            self.automation_selected_ids.clear();
                            self.status = "Selection cleared".to_string();
                        }
                    }
                    _ => {}
                }
            }
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

    // ── Automation filter bar ─────────────────────────────────────────────────

    async fn on_key_automation_filter_bar(&mut self, key: KeyEvent) {
        let Some(bar) = self.automation_filter_bar.as_mut() else { return };
        match key.code {
            KeyCode::Esc => {
                self.automation_filter_bar = None;
            }
            KeyCode::Tab | KeyCode::Right => {
                bar.active_field = match bar.active_field {
                    AutomationFilterField::Tag => AutomationFilterField::Trigger,
                    AutomationFilterField::Trigger => AutomationFilterField::Tag,
                };
            }
            KeyCode::BackTab | KeyCode::Left => {
                bar.active_field = match bar.active_field {
                    AutomationFilterField::Tag => AutomationFilterField::Trigger,
                    AutomationFilterField::Trigger => AutomationFilterField::Tag,
                };
            }
            KeyCode::Backspace => match bar.active_field {
                AutomationFilterField::Tag => { bar.tag.pop(); }
                AutomationFilterField::Trigger => { bar.trigger.pop(); }
            },
            KeyCode::Enter => {
                let tag = bar.tag.clone();
                let trigger = bar.trigger.clone();
                let stale = bar.stale;
                self.automation_filter_tag = tag;
                self.automation_filter_trigger = trigger;
                self.automation_filter_stale = stale;
                self.automation_filter_bar = None;
                self.selected = 0;
                self.clamp_selection();
                self.status = "Automation filter applied".to_string();
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                let bar = self.automation_filter_bar.as_mut().unwrap();
                match bar.active_field {
                    AutomationFilterField::Tag => bar.tag.push(ch),
                    AutomationFilterField::Trigger => bar.trigger.push(ch),
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
        match self.client.list_automation_groups().await {
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
                    match self.client.enable_automation_group(&g.id).await {
                        Ok(_) => self.status = format!("Group '{}' enabled", g.name),
                        Err(e) => self.error = Some(e.to_string()),
                    }
                }
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                if let Some(g) = self.groups.get(self.groups_selected).cloned() {
                    match self.client.disable_automation_group(&g.id).await {
                        Ok(_) => self.status = format!("Group '{}' disabled", g.name),
                        Err(e) => self.error = Some(e.to_string()),
                    }
                }
            }
            KeyCode::Delete | KeyCode::Char('x') => {
                if let Some(g) = self.groups.get(self.groups_selected).cloned() {
                    match self.client.delete_automation_group(&g.id).await {
                        Ok(_) => {
                            self.groups.retain(|gr| gr.id != g.id);
                            self.groups_selected = self.groups_selected.min(
                                self.groups.len().saturating_sub(1)
                            );
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
        let Some(rule) = self.selected_automation().cloned() else { return };
        match self.client.get_automation_history(&rule.id).await {
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

    // ── Automation enable/disable/clone/delete ────────────────────────────────

    async fn enable_selected_automation(&mut self) {
        let Some(rule) = self.selected_automation().cloned() else { return };
        match self.client.toggle_automation(&rule.id, true).await {
            Ok(_) => {
                if let Some(r) = self.automations.iter_mut().find(|r| r.id == rule.id) {
                    r.enabled = true;
                }
                self.status = format!("Enabled rule '{}'", rule.name);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    async fn disable_selected_automation(&mut self) {
        let Some(rule) = self.selected_automation().cloned() else { return };
        match self.client.toggle_automation(&rule.id, false).await {
            Ok(_) => {
                if let Some(r) = self.automations.iter_mut().find(|r| r.id == rule.id) {
                    r.enabled = false;
                }
                self.status = format!("Disabled rule '{}'", rule.name);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    async fn clone_selected_automation(&mut self) {
        let Some(rule) = self.selected_automation().cloned() else { return };
        match self.client.clone_automation(&rule.id).await {
            Ok(cloned) => {
                let name = cloned.name.clone();
                self.automations.push(cloned);
                self.status = format!("Cloned -> \"{}\"", name);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn initiate_delete_automation(&mut self) {
        let Some(rule) = self.selected_automation().cloned() else { return };
        self.automation_delete_confirm = Some(DeleteConfirm {
            rule_id: rule.id,
            rule_name: rule.name,
        });
    }

    async fn confirm_delete_automation(&mut self) {
        let Some(confirm) = self.automation_delete_confirm.take() else { return };
        match self.client.delete_automation(&confirm.rule_id).await {
            Ok(_) => {
                self.automations.retain(|r| r.id != confirm.rule_id);
                self.clamp_selection();
                self.status = format!("Deleted rule '{}'", confirm.rule_name);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn toggle_automation_selection(&mut self) {
        let Some(rule) = self.selected_automation().cloned() else { return };
        if self.automation_selected_ids.contains(&rule.id) {
            self.automation_selected_ids.remove(&rule.id);
        } else {
            self.automation_selected_ids.insert(rule.id);
            self.automation_bulk_select_mode = true;
        }
        if self.automation_selected_ids.is_empty() {
            self.automation_bulk_select_mode = false;
        }
        let count = self.automation_selected_ids.len();
        self.status = format!("{count} rule(s) selected");
    }

    async fn bulk_enable_automations(&mut self) {
        let ids: Vec<String> = self.automation_selected_ids.iter().cloned().collect();
        match self.client.bulk_toggle_automations(&ids, true).await {
            Ok(_) => {
                for r in self.automations.iter_mut() {
                    if ids.contains(&r.id) {
                        r.enabled = true;
                    }
                }
                self.automation_selected_ids.clear();
                self.automation_bulk_select_mode = false;
                self.status = format!("Enabled {} rule(s)", ids.len());
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    async fn bulk_disable_automations(&mut self) {
        let ids: Vec<String> = self.automation_selected_ids.iter().cloned().collect();
        match self.client.bulk_toggle_automations(&ids, false).await {
            Ok(_) => {
                for r in self.automations.iter_mut() {
                    if ids.contains(&r.id) {
                        r.enabled = false;
                    }
                }
                self.automation_selected_ids.clear();
                self.automation_bulk_select_mode = false;
                self.status = format!("Disabled {} rule(s)", ids.len());
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    // ── System Status ─────────────────────────────────────────────────────────

    async fn refresh_system_status(&mut self) {
        match self.client.get_system_status().await {
            Ok(status) => {
                self.system_status_last_refresh = Some(
                    Local::now().format("%H:%M:%S").to_string()
                );
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
            payload.insert("discriminator".to_string(), Value::Number((disc as u64).into()));
        }
        if let Some(pin) = passcode {
            payload.insert("passcode".to_string(), Value::Number((pin as u64).into()));
        }

        let before = self.matter_nodes.len();

        match self.client.matter_commission(Value::Object(payload)).await {
            Ok(_) => {
                self.error = None;
                self.matter_pending = true;
                self.matter_last_action = "Commission request accepted; waiting for device response".to_string();
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
                    self.error = Some("Matter discriminator must be a number (0-65535)".to_string());
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
                let message = format!(
                    "Matter reinterview failed for {}: {}",
                    node.node_id, err
                );
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
                let message = format!(
                    "Matter remove failed for {}: {}",
                    node.node_id, err
                );
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
                    sa.cmp(&sb).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
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
            DeviceFilterMode::LowBattery => Self::device_battery(device).map(|b| b <= 20).unwrap_or(false),
        }
    }

    fn device_matches_search(&self, device: &DeviceState) -> bool {
        let q = self.device_search_query.trim().to_lowercase();
        if q.is_empty() {
            return true;
        }

        device.name.to_lowercase().contains(&q)
            || device.device_id.to_lowercase().contains(&q)
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
        device.attributes.get("brightness").and_then(|v| v.as_f64()).map(|n| {
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

        self.device_editor = Some(DeviceEditor {
            device_id,
            name,
            area,
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
                    DeviceEditField::Area => DeviceEditField::Name,
                };
            }
            KeyCode::BackTab | KeyCode::Left => {
                editor.field = match editor.field {
                    DeviceEditField::Name => DeviceEditField::Area,
                    DeviceEditField::Area => DeviceEditField::Name,
                };
            }
            KeyCode::Backspace => match editor.field {
                DeviceEditField::Name => {
                    editor.name.pop();
                }
                DeviceEditField::Area => {
                    editor.area.pop();
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

        match self
            .client
            .update_device_metadata(&editor.device_id, &name, area.as_deref())
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
            return if locked { "Locked".to_string() } else { "Unlocked".to_string() };
        }
        // Explicit on/off (binary switch, most smart plugs)
        if let Some(on) = attrs.get("on").and_then(|v| v.as_bool()) {
            return if on { "On".to_string() } else { "Off".to_string() };
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
            return if b > 0.0 { "On".to_string() } else { "Off".to_string() };
        }
        // Contact sensor (open/closed bool)
        if let Some(open) = attrs.get("open").or_else(|| attrs.get("contact_open")).and_then(|v| v.as_bool()) {
            return if open { "Open".to_string() } else { "Closed".to_string() };
        }
        // Motion sensor
        if let Some(motion) = attrs.get("motion").and_then(|v| v.as_bool()) {
            return if motion { "Motion".to_string() } else { "Clear".to_string() };
        }
        // Thermostat mode
        if let Some(mode) = attrs.get("mode").and_then(|v| v.as_str()) {
            return normalize_label(mode);
        }
        // Window covering position
        if let Some(pos) = attrs.get("position").and_then(|v| v.as_f64()) {
            return if pos >= 99.0 { "Open".to_string() } else if pos <= 1.0 { "Closed".to_string() } else { format!("{pos:.0}%") };
        }
        // Sensor-only devices — show primary reading as status
        if let Some(temp) = attrs.get("temperature").or_else(|| attrs.get("temp")).and_then(|v| v.as_f64()) {
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
            return if online { "Online".to_string() } else { "Offline".to_string() };
        }
        // Occupancy sensor (Lutron occupancy groups)
        if let Some(occupied) = attrs.get("occupied").and_then(|v| v.as_bool()) {
            return if occupied { "Occupied".to_string() } else { "Vacant".to_string() };
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
            (PluginDetailPanel::Overview, true) => PluginDetailPanel::Diagnostics,
            (PluginDetailPanel::Diagnostics, true) => PluginDetailPanel::Metrics,
            (PluginDetailPanel::Metrics, true) => PluginDetailPanel::Overview,
            (PluginDetailPanel::Overview, false) => PluginDetailPanel::Metrics,
            (PluginDetailPanel::Diagnostics, false) => PluginDetailPanel::Overview,
            (PluginDetailPanel::Metrics, false) => PluginDetailPanel::Diagnostics,
        };
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
            match self.client.send_device_action(&device_id, "pair_bridge").await {
                Ok(_) => ok += 1,
                Err(err) => failed.push(format!("{device_id}: {err}")),
            }
        }

        if failed.is_empty() {
            let pairing_status = format!(
                "Pairing requested for {ok} bridge(s). Press Hue link button if needed."
            );
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
                    && d
                        .attributes
                        .get("kind")
                        .and_then(|v| v.as_str())
                        == Some("hue_bridge")
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
                    "device_button" | "device_rotary" | "entertainment_action_applied" | "entertainment_status_changed" | "plugin_command_result" | "bridge_pairing_status"
                ) || matches!(
                    custom,
                    "device_button" | "device_rotary" | "entertainment_action_applied" | "entertainment_status_changed" | "plugin_command_result" | "bridge_pairing_status"
                )
            }
            EventsFilterMode::Entertainment => {
                matches!(ty, "entertainment_action_applied" | "entertainment_status_changed")
                    || matches!(custom, "entertainment_action_applied" | "entertainment_status_changed")
            }
            EventsFilterMode::PluginMetrics => {
                ty == "plugin_metrics" || custom == "plugin_metrics"
            }
        }
    }

    fn active_items_len(&self) -> usize {
        match self.active_tab() {
            Tab::Devices => match self.device_sub {
                DeviceSubPanel::All => self.visible_devices().len(),
                DeviceSubPanel::Switches => self.switches.len(),
                DeviceSubPanel::Timers => self.timers.len(),
            },
            Tab::Scenes => self.scenes.len(),
            Tab::Areas => self.areas.len(),
            Tab::Automations => self.visible_automations().len(),
            Tab::Plugins => self.plugins.len(),
            Tab::Manage => match self.admin_sub {
                AdminSubPanel::Modes => self.modes.len(),
                AdminSubPanel::Matter => self.matter_nodes.len(),
                AdminSubPanel::Status => 0,
                AdminSubPanel::Users => self.users.len(),
                        AdminSubPanel::Logs => 0,
                        AdminSubPanel::Events => self.filtered_events().len(),
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
        let (device_id, device_name, current_on) = {
            let Some(device) = self.selected_device() else { return };
            let on = device.attributes.get("on").and_then(|v| v.as_bool()).unwrap_or(false);
            (device.device_id.clone(), device.name.clone(), on)
        };
        match self.client.set_device_on(&device_id, !current_on).await {
            Ok(_) => self.status = format!("{} → {}", device_name, if !current_on { "On" } else { "Off" }),
            Err(err) => self.error = Some(err.to_string()),
        }
    }

    async fn adjust_brightness(&mut self, direction: i64) {
        let (device_id, device_name, raw_pct, raw_abs) = {
            let Some(device) = self.selected_device() else { return };
            let pct = device.attributes.get("brightness_pct").and_then(|v| v.as_f64());
            let abs = device.attributes.get("brightness").and_then(|v| v.as_f64());
            (device.device_id.clone(), device.name.clone(), pct, abs)
        };

        if let Some(raw) = raw_pct {
            // Hue-style 0–100% brightness
            let new_val = ((raw + direction as f64 * 10.0).clamp(0.0, 100.0) * 10.0).round() / 10.0;
            match self.client.set_device_brightness_pct(&device_id, new_val).await {
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
            match self.client.set_device_brightness(&device_id, new_val_i).await {
                Ok(_) => self.status = format!("{device_name} brightness → {new_val_i}"),
                Err(err) => self.error = Some(err.to_string()),
            }
        }
    }

    async fn lock_device(&mut self, locked: bool) {
        let (device_id, device_name) = {
            let Some(device) = self.selected_device() else { return };
            (device.device_id.clone(), device.name.clone())
        };
        match self.client.set_device_locked(&device_id, locked).await {
            Ok(_) => {
                self.status = format!("{} → {}", device_name, if locked { "Locked" } else { "Unlocked" });
            }
            Err(err) => self.error = Some(err.to_string()),
        }
    }

    /// Space bar: toggle lock state for lock devices, or on/off for switches.
    async fn toggle_lock_or_switch(&mut self) {
        let Some(device) = self.selected_device() else { return };
        let device_id   = device.device_id.clone();
        let device_name = device.name.clone();

        if let Some(locked) = Self::device_lock_state(device) {
            let new_locked = !locked;
            match self.client.set_device_locked(&device_id, new_locked).await {
                Ok(_) => self.status = format!(
                    "{} → {}",
                    device_name,
                    if new_locked { "Locked" } else { "Unlocked" }
                ),
                Err(err) => self.error = Some(err.to_string()),
            }
        } else {
            let on = device.attributes.get("on").and_then(|v| v.as_bool()).unwrap_or(false);
            match self.client.set_device_on(&device_id, !on).await {
                Ok(_) => self.status = format!("{} → {}", device_name, if !on { "On" } else { "Off" }),
                Err(err) => self.error = Some(err.to_string()),
            }
        }
    }

    // ── Area CRUD ─────────────────────────────────────────────────────────────

    fn open_area_editor_create(&mut self) {
        self.area_editor = Some(AreaEditor { id: None, name: String::new() });
    }

    fn open_area_editor_edit(&mut self) {
        let Some(area) = self.areas.get(self.selected) else { return };
        self.area_editor = Some(AreaEditor {
            id:   Some(area.id.clone()),
            name: area.name.clone(),
        });
    }

    async fn on_key_area_editor(&mut self, key: KeyEvent) {
        let Some(editor) = self.area_editor.as_mut() else { return };
        match key.code {
            KeyCode::Esc => {
                self.area_editor = None;
                self.status = "Area edit canceled".to_string();
            }
            KeyCode::Backspace => { editor.name.pop(); }
            KeyCode::Enter => { self.save_area_editor().await; }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                editor.name.push(ch);
            }
            _ => {}
        }
    }

    async fn save_area_editor(&mut self) {
        let Some(editor) = self.area_editor.clone() else { return };
        let name = editor.name.trim().to_string();
        if name.is_empty() {
            self.error = Some("area name cannot be empty".to_string());
            return;
        }
        match editor.id {
            None => {
                match self.client.create_area(&name).await {
                    Ok(area) => {
                        self.areas.push(area);
                        self.area_editor = None;
                        self.status = format!("Created area '{name}'");
                        let _ = self.save_to_cache().await;
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            Some(ref id) => {
                match self.client.rename_area(id, &name).await {
                    Ok(updated) => {
                        if let Some(a) = self.areas.iter_mut().find(|a| a.id == updated.id) {
                            a.name = updated.name.clone();
                        }
                        self.area_editor = None;
                        self.status = format!("Renamed area to '{}'", updated.name);
                        let _ = self.save_to_cache().await;
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
        }
    }

    async fn delete_selected_area(&mut self) {
        let Some(area) = self.areas.get(self.selected) else { return };
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
        let Some(area) = self.areas.get(self.areas_list_selected) else { return };
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
                            self.areas_devices_selected = min(self.areas_devices_selected + 1, len - 1);
                        }
                    }
                }
            }
            
            // Enter key behavior based on pane focus
            KeyCode::Enter => {
                match self.areas_pane_focus {
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
                }
            }
            
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
                        let device_ids = self.areas
                            .iter()
                            .find(|a| &a.id == area_id)
                            .map(|a| a.device_ids.clone())
                            .unwrap_or_default();
                        
                        let visible_devices: Vec<_> = self.devices
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
                    let device_ids = self.areas
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
                        self.status = format!("Added {} device(s) to area", self.areas_selected_devices.len());
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
        if self.areas_selected_devices.is_empty() && matches!(self.areas_pane_focus, AreasPane::AreasList) {
            self.delete_selected_area().await;
            return;
        }
        
        if let Some(area_id) = &self.areas_selected_area_id {
            if let Some(area) = self.areas.iter().find(|a| &a.id == area_id) {
                let new_device_ids: Vec<String> = area.device_ids
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
            let Some(device) = self.selected_device() else { return };
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
        let Some(sw) = self.switches.get(self.selected).cloned() else { return };
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
        let Some(t) = self.timers.get(self.selected).cloned() else { return };
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
        let Some(plugin) = self.plugins.get(self.selected) else { return };
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
            mode:             UserEditMode::Create,
            id:               None,
            field:            UserEditField::Username,
            username:         String::new(),
            current_password: String::new(),
            password:         String::new(),
            confirm_password: String::new(),
            role:             Role::User,
        });
    }

    fn open_user_editor_role(&mut self) {
        let Some(user) = self.users.get(self.selected) else { return };
        self.user_editor = Some(UserEditor {
            mode:             UserEditMode::EditRole,
            id:               Some(user.id.clone()),
            field:            UserEditField::Role,
            username:         user.username.clone(),
            current_password: String::new(),
            password:         String::new(),
            confirm_password: String::new(),
            role:             user.role.clone(),
        });
    }

    fn open_user_editor_password(&mut self) {
        // The backend change-password endpoint always operates on the JWT user
        // (the currently logged-in account). Always change your own password here.
        let Some(u) = self.current_user.clone() else { return };
        self.user_editor = Some(UserEditor {
            mode:             UserEditMode::ChangePassword,
            id:               Some(u.id.clone()),
            field:            UserEditField::CurrentPassword,
            username:         u.username.clone(),
            current_password: String::new(),
            password:         String::new(),
            confirm_password: String::new(),
            role:             Role::User,
        });
    }

    pub async fn on_key_user_editor(&mut self, key: KeyEvent) {
        let Some(editor) = self.user_editor.as_mut() else { return };
        match key.code {
            KeyCode::Esc => {
                self.user_editor = None;
                self.status = "User edit canceled".to_string();
            }
            KeyCode::Tab | KeyCode::Down => {
                editor.field = match (&editor.mode, &editor.field) {
                    (UserEditMode::Create, UserEditField::Username) => UserEditField::Password,
                    (UserEditMode::Create, UserEditField::Password) => UserEditField::ConfirmPassword,
                    (UserEditMode::Create, UserEditField::ConfirmPassword) => UserEditField::Role,
                    (UserEditMode::Create, UserEditField::Role) => UserEditField::Username,
                    (UserEditMode::ChangePassword, UserEditField::CurrentPassword) => UserEditField::Password,
                    (UserEditMode::ChangePassword, UserEditField::Password) => UserEditField::ConfirmPassword,
                    (UserEditMode::ChangePassword, UserEditField::ConfirmPassword) => UserEditField::CurrentPassword,
                    _ => editor.field,
                };
            }
            KeyCode::BackTab | KeyCode::Up => {
                editor.field = match (&editor.mode, &editor.field) {
                    (UserEditMode::Create, UserEditField::Username) => UserEditField::Role,
                    (UserEditMode::Create, UserEditField::Password) => UserEditField::Username,
                    (UserEditMode::Create, UserEditField::ConfirmPassword) => UserEditField::Password,
                    (UserEditMode::Create, UserEditField::Role) => UserEditField::ConfirmPassword,
                    (UserEditMode::ChangePassword, UserEditField::CurrentPassword) => UserEditField::ConfirmPassword,
                    (UserEditMode::ChangePassword, UserEditField::Password) => UserEditField::CurrentPassword,
                    (UserEditMode::ChangePassword, UserEditField::ConfirmPassword) => UserEditField::Password,
                    _ => editor.field,
                };
            }
            KeyCode::Backspace => {
                match editor.field {
                    UserEditField::Username        => { editor.username.pop(); }
                    UserEditField::CurrentPassword => { editor.current_password.pop(); }
                    UserEditField::Password        => { editor.password.pop(); }
                    UserEditField::ConfirmPassword => { editor.confirm_password.pop(); }
                    UserEditField::Role            => {}
                }
            }
            KeyCode::Char(' ') if editor.field == UserEditField::Role => {
                // Cycle role
                editor.role = match editor.role {
                    Role::Admin    => Role::User,
                    Role::User     => Role::ReadOnly,
                    Role::ReadOnly => Role::Admin,
                };
            }
            KeyCode::Enter => { self.save_user_editor().await; }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                let editor = self.user_editor.as_mut().unwrap();
                match editor.field {
                    UserEditField::Username        => editor.username.push(ch),
                    UserEditField::CurrentPassword => editor.current_password.push(ch),
                    UserEditField::Password        => editor.password.push(ch),
                    UserEditField::ConfirmPassword => editor.confirm_password.push(ch),
                    UserEditField::Role            => {}
                }
            }
            _ => {}
        }
    }

    async fn save_user_editor(&mut self) {
        let Some(editor) = self.user_editor.clone() else { return };
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
                match self.client.create_user(&username, &editor.password, &editor.role).await {
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
                match self.client.change_password(&editor.current_password, &editor.password).await {
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
        let Some(user) = self.users.get(self.selected) else { return };
        // Guard: cannot delete yourself
        if self.current_user.as_ref().map(|u| u.id == user.id).unwrap_or(false) {
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
        let scene_id   = scene.id.clone();
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
        }
    }

    async fn delete_selected_manage_item(&mut self) {
        match self.admin_sub {
            AdminSubPanel::Modes => {
                let Some(m) = self.modes.get(self.selected).cloned() else { return };
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
                let Some(u) = self.users.get(self.selected).cloned() else { return };
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
        }
    }

    async fn on_key_switch_editor(&mut self, key: KeyEvent) {
        let Some(editor) = self.switch_editor.as_mut() else { return };
        match key.code {
            KeyCode::Esc => {
                self.switch_editor = None;
                self.status = "Cancelled".to_string();
            }
            KeyCode::Tab | KeyCode::BackTab => {
                editor.field = match editor.field {
                    SwitchEditField::Id    => SwitchEditField::Label,
                    SwitchEditField::Label => SwitchEditField::Id,
                };
            }
            KeyCode::Backspace => match editor.field {
                SwitchEditField::Id    => { editor.id.pop(); }
                SwitchEditField::Label => { editor.label.pop(); }
            },
            KeyCode::Enter => { self.save_switch_editor().await; }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                match editor.field {
                    SwitchEditField::Id    => editor.id.push(ch),
                    SwitchEditField::Label => editor.label.push(ch),
                }
            }
            _ => {}
        }
    }

    async fn save_switch_editor(&mut self) {
        let Some(editor) = self.switch_editor.clone() else { return };
        let id = editor.id.trim().to_string();
        if id.is_empty() {
            self.error = Some("switch id cannot be empty".to_string());
            return;
        }
        let label = if editor.label.trim().is_empty() { editor.id.trim() } else { editor.label.trim() };
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
        let Some(editor) = self.timer_editor.as_mut() else { return };
        match key.code {
            KeyCode::Esc => {
                self.timer_editor = None;
                self.status = "Cancelled".to_string();
            }
            KeyCode::Tab | KeyCode::BackTab => {
                editor.field = match editor.field {
                    TimerEditField::Id    => TimerEditField::Label,
                    TimerEditField::Label => TimerEditField::Id,
                };
            }
            KeyCode::Backspace => match editor.field {
                TimerEditField::Id    => { editor.id.pop(); }
                TimerEditField::Label => { editor.label.pop(); }
            },
            KeyCode::Enter => { self.save_timer_editor().await; }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                match editor.field {
                    TimerEditField::Id    => editor.id.push(ch),
                    TimerEditField::Label => editor.label.push(ch),
                }
            }
            _ => {}
        }
    }

    async fn save_timer_editor(&mut self) {
        let Some(editor) = self.timer_editor.clone() else { return };
        let id = editor.id.trim().to_string();
        if id.is_empty() {
            self.error = Some("timer id cannot be empty".to_string());
            return;
        }
        let label = if editor.label.trim().is_empty() { editor.id.trim() } else { editor.label.trim() };
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
        let Some(editor) = self.mode_editor.as_mut() else { return };
        match key.code {
            KeyCode::Esc => {
                self.mode_editor = None;
                self.status = "Cancelled".to_string();
            }
            KeyCode::Tab | KeyCode::BackTab => {
                editor.field = match editor.field {
                    ModeEditField::Id   => ModeEditField::Name,
                    ModeEditField::Name => ModeEditField::Kind,
                    ModeEditField::Kind => ModeEditField::Id,
                };
            }
            KeyCode::Char(' ') if editor.field == ModeEditField::Kind => {
                editor.kind = editor.kind.next();
            }
            KeyCode::Backspace => match editor.field {
                ModeEditField::Id   => { editor.id.pop(); }
                ModeEditField::Name => { editor.name.pop(); }
                ModeEditField::Kind => {}
            },
            KeyCode::Enter => { self.save_mode_editor().await; }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                match editor.field {
                    ModeEditField::Id   => editor.id.push(ch),
                    ModeEditField::Name => editor.name.push(ch),
                    ModeEditField::Kind => {}
                }
            }
            _ => {}
        }
    }

    async fn save_mode_editor(&mut self) {
        let Some(editor) = self.mode_editor.clone() else { return };
        let id = editor.id.trim().to_string();
        if id.is_empty() {
            self.error = Some("mode id cannot be empty".to_string());
            return;
        }
        if !id.starts_with("mode_") {
            self.error = Some("mode id must start with 'mode_'".to_string());
            return;
        }
        let name = if editor.name.trim().is_empty() { editor.id.trim() } else { editor.name.trim() };
        match self.client.create_mode(&id, name, editor.kind.as_str()).await {
            Ok(cfg) => {
                let cfg_id = cfg.id.clone();
                self.modes.push(ModeRecord { config: cfg, state: None });
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

fn is_hidden_in_devices_view_with_context(device: &DeviceState, all_devices: &[DeviceState]) -> bool {
    if is_hidden_in_devices_view(device) {
        return true;
    }

    // Compact Hue motion facets in the TUI when the corresponding motion device
    // exists: show one motion row that carries motion/temp/lux/battery values.
    let Some(kind) = device.attributes.get("kind").and_then(Value::as_str) else {
        return false;
    };
    if !matches!(kind, "hue_temperature" | "hue_light_level" | "hue_device_power") {
        return false;
    }

    all_devices.iter().any(|other| {
        if other.plugin_id != device.plugin_id || other.name != device.name {
            return false;
        }
        other
            .attributes
            .get("kind")
            .and_then(Value::as_str)
            == Some("hue_motion")
    })
}

/// Extract hue scene devices from the device list and convert them to Scene entries.
fn hue_scenes_from_devices(devices: &[DeviceState]) -> Vec<Scene> {
    devices
        .iter()
        .filter(|d| is_scene_device(d))
        .map(|d| {
            let scene_name = d.attributes.get("name")
                .and_then(Value::as_str)
                .unwrap_or(&d.name)
                .to_string();
            let area = d.area.clone()
                .or_else(|| d.attributes.get("group_name").and_then(Value::as_str).map(str::to_string));
            let active = d.attributes.get("active").and_then(Value::as_bool);
            Scene {
                id:        d.device_id.clone(),
                name:      scene_name,
                plugin_id: Some(d.plugin_id.clone()),
                area,
                active,
            }
        })
        .collect()
}

fn summarize_live_event_detail(event: &Value) -> Option<String> {
    let event_type = event.get("type").and_then(Value::as_str).unwrap_or("unknown");
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
            let operation = event.get("operation").and_then(Value::as_str).unwrap_or("unknown");
            let success = event.get("success").and_then(Value::as_bool).unwrap_or(false);
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
            let phase = event.get("phase").and_then(Value::as_str).unwrap_or("unknown");
            let success = event.get("success").and_then(Value::as_bool);
            let error = event.get("error").and_then(Value::as_str);

            let mut parts = vec![format!("phase={phase}")];
            if let Some(v) = success {
                parts.push(if v { "success".to_string() } else { "failed".to_string() });
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
                parts.push(format!("fallback={f} incremental={a} fallback_ratio={r:.2}%"));
            }
            if let (Some(f), Some(a), Some(r)) = (recent_fallback, recent_applied, recent_ratio) {
                parts.push(format!("recent_fallback={f} recent_incremental={a} recent_ratio={r:.2}%"));
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
        Ok(snapshot) => {
            cache.save_snapshot(&auth.user.username, &snapshot).await?;
            Ok(LoginWorkflowResult {
                auth,
                snapshot,
                warning: None,
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

async fn fetch_remote_snapshot(client: &HomeCoreClient, role: Role) -> Result<CacheSnapshot> {
    let devices = client.list_devices().await.unwrap_or_default();
    let mut scenes = client.list_scenes().await.unwrap_or_default();
    scenes.extend(hue_scenes_from_devices(&devices));
    let areas = client.list_areas().await.unwrap_or_default();
    let automations = client.list_automations().await.unwrap_or_default();
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

    Ok(CacheSnapshot {
        devices,
        scenes,
        areas,
        automations,
        events,
        users,
        plugins,
        switches,
        timers,
        modes,
    })
}

fn snapshot_is_empty(snapshot: &CacheSnapshot) -> bool {
    snapshot.devices.is_empty()
        && snapshot.scenes.is_empty()
        && snapshot.areas.is_empty()
        && snapshot.automations.is_empty()
        && snapshot.events.is_empty()
        && snapshot.users.is_empty()
        && snapshot.plugins.is_empty()
}

/// Format a timestamp string for display. Respects the `utc` flag.
pub fn format_timestamp_utc(ts: &str, utc: bool) -> String {
    if utc {
        // Show as UTC with date+time
        if let Ok(dt) = DateTime::parse_from_rfc3339(ts) {
            return dt.with_timezone(&Utc).format("%Y-%m-%d %H:%M:%S UTC").to_string();
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
    fn test_automation_stale_filter() {
        let mut app = test_app();
        app.automation_filter_stale = true;
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
        assert!(!app.automation_matches_filter(&rule_ok));
        assert!(app.automation_matches_filter(&rule_stale));
    }
}

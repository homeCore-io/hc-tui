use crate::api::{
    Area, DeviceState, EventEntry, HomeCoreClient, LoginResponse, PluginRecord, Role, Rule, Scene,
    UserInfo,
};
use crate::cache::{CacheSnapshot, CacheStore};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::Value;
use std::cmp::min;

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
pub enum Tab {
    Devices,
    Scenes,
    Areas,
    Automations,
    Events,
    Users,
    Plugins,
}

impl Tab {
    pub fn title(self) -> &'static str {
        match self {
            Self::Devices => "Devices",
            Self::Scenes => "Scenes",
            Self::Areas => "Areas",
            Self::Automations => "Automations",
            Self::Events => "Events",
            Self::Users => "Users",
            Self::Plugins => "Plugins",
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
    pub ws_connected: bool,
    pub login_in_progress: bool,
    pub login_animation_step: u16,
    pub login_phase: LoginPhase,
    pub device_editor: Option<DeviceEditor>,
    pub area_editor: Option<AreaEditor>,
    pub user_editor: Option<UserEditor>,
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
            ws_connected: false,
            login_in_progress: false,
            login_animation_step: 0,
            login_phase: LoginPhase::Authenticating,
            device_editor: None,
            area_editor: None,
            user_editor: None,
        }
    }

    pub fn tabs(&self) -> Vec<Tab> {
        let mut tabs = vec![
            Tab::Devices,
            Tab::Scenes,
            Tab::Areas,
            Tab::Automations,
            Tab::Events,
        ];
        if self.is_admin() {
            tabs.push(Tab::Users);
            tabs.push(Tab::Plugins);
        }
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
        if self.is_admin() {
            self.users = self.client.list_users().await?;
            self.plugins = self.client.list_plugins().await?;
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


    pub async fn on_key_authenticated(&mut self, key: KeyEvent) {
        self.error = None;

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

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('r') => {
                if let Err(err) = self.refresh_all().await {
                    self.error = Some(err.to_string());
                }
            }
            KeyCode::Left | KeyCode::BackTab => {
                let tab_count = self.tabs().len();
                self.tab = (self.tab + tab_count - 1) % tab_count;
                self.selected = 0;
                self.clamp_selection();
            }
            KeyCode::Right | KeyCode::Tab => {
                let tab_count = self.tabs().len();
                self.tab = (self.tab + 1) % tab_count;
                self.selected = 0;
                self.clamp_selection();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let len = self.active_items_len();
                if len > 0 {
                    self.selected = min(self.selected + 1, len - 1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Enter => {
                match self.active_tab() {
                    Tab::Devices => self.open_selected_device_editor(),
                    Tab::Areas   => self.open_area_editor_edit(),
                    Tab::Users   => self.open_user_editor_role(),
                    Tab::Plugins => self.open_plugin_detail(),
                    _ => {}
                }
            }
            KeyCode::Char('n') => {
                match self.active_tab() {
                    Tab::Areas => self.open_area_editor_create(),
                    Tab::Users if self.is_admin() => self.open_user_editor_create(),
                    _ => {}
                }
            }
            KeyCode::Char('d') => {
                match self.active_tab() {
                    Tab::Devices => self.delete_selected_device().await,
                    Tab::Areas   => self.delete_selected_area().await,
                    Tab::Users   => self.delete_selected_user().await,
                    Tab::Plugins => self.deregister_selected_plugin().await,
                    _ => {}
                }
            }
            KeyCode::Char('p') => {
                if matches!(self.active_tab(), Tab::Users) {
                    self.open_user_editor_password();
                }
            }
            KeyCode::Char('t') => {
                if matches!(self.active_tab(), Tab::Devices) {
                    self.toggle_selected_device().await;
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
            KeyCode::Char(' ') => {
                if matches!(self.active_tab(), Tab::Devices) {
                    self.toggle_lock_or_switch().await;
                }
            }
            KeyCode::Char('a') => {
                if matches!(self.active_tab(), Tab::Scenes) {
                    self.activate_selected_scene().await;
                }
            }
            KeyCode::Char('f') => {
                if matches!(self.active_tab(), Tab::Events) {
                    self.events_filter_mode = match self.events_filter_mode {
                        EventsFilterMode::All => EventsFilterMode::HueInputs,
                        EventsFilterMode::HueInputs => EventsFilterMode::Entertainment,
                        EventsFilterMode::Entertainment => EventsFilterMode::PluginMetrics,
                        EventsFilterMode::PluginMetrics => EventsFilterMode::All,
                    };
                    self.selected = 0;
                    self.clamp_selection();
                    self.status = format!(
                        "Events filter: {}",
                        self.events_filter_mode.title()
                    );
                }
            }
            _ => {}
        }
    }

    /// Returns devices grouped by area, sorted alphabetically. Unassigned devices last.
    /// Devices that should appear in the Devices tab (scene devices are shown in Scenes tab).
    pub fn visible_devices(&self) -> Vec<&DeviceState> {
        self.devices.iter().filter(|d| !is_scene_device(d)).collect()
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
        for key in &["battery", "battery_level", "battery_percent"] {
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
        "Unknown".to_string()
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
            Tab::Devices => self.visible_devices().len(),
            Tab::Scenes => self.scenes.len(),
            Tab::Areas => self.areas.len(),
            Tab::Automations => self.automations.len(),
            Tab::Events => self.filtered_events().len(),
            Tab::Users => self.users.len(),
            Tab::Plugins => self.plugins.len(),
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
        // Admins can change any user's password; non-admins change their own.
        let (id, username) = if self.is_admin() {
            if let Some(user) = self.users.get(self.selected) {
                (Some(user.id.clone()), user.username.clone())
            } else {
                return;
            }
        } else {
            let u = self.current_user.clone().unwrap();
            (Some(u.id.clone()), u.username.clone())
        };
        self.user_editor = Some(UserEditor {
            mode:             UserEditMode::ChangePassword,
            id,
            field:            UserEditField::CurrentPassword,
            username,
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
        let is_device_scene = self.devices.iter().any(|d| {
            d.device_id == scene_id
                && d.attributes.get("kind").and_then(Value::as_str) == Some("hue_scene")
        });
        let result = if is_device_scene {
            self.client.activate_device_scene(&scene_id).await
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
}

/// Returns true if this device is a hue_scene and should be excluded from the device list.
pub fn is_scene_device(device: &DeviceState) -> bool {
    device.attributes.get("kind").and_then(Value::as_str) == Some("hue_scene")
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;
    use serde_json::Value;
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

    fn mk_device(device_id: &str, plugin_id: &str, kind: &str) -> DeviceState {
        let mut attributes = Map::new();
        attributes.insert("kind".to_string(), Value::String(kind.to_string()));

        DeviceState {
            device_id: device_id.to_string(),
            name: device_id.to_string(),
            plugin_id: plugin_id.to_string(),
            area: None,
            available: true,
            attributes,
            last_seen: "2026-03-21T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn filtered_events_respects_selected_mode() {
        let mut app = test_app();
        app.events = vec![
            mk_event("device_button"),
            mk_event("device_rotary"),
            mk_event("entertainment_action_applied"),
            mk_event("entertainment_status_changed"),
            mk_event("plugin_metrics"),
            mk_event("device_state_changed"),
        ];

        app.events_filter_mode = EventsFilterMode::All;
        assert_eq!(app.filtered_events().len(), 6);

        app.events_filter_mode = EventsFilterMode::HueInputs;
        assert_eq!(app.filtered_events().len(), 4);

        app.events_filter_mode = EventsFilterMode::Entertainment;
        assert_eq!(app.filtered_events().len(), 2);

        app.events_filter_mode = EventsFilterMode::PluginMetrics;
        assert_eq!(app.filtered_events().len(), 1);
    }

    #[tokio::test]
    async fn events_filter_key_cycles_modes() {
        let mut app = test_app();
        app.tab = app
            .tabs()
            .iter()
            .position(|t| matches!(t, Tab::Events))
            .unwrap_or(4);
        app.events_filter_mode = EventsFilterMode::All;

        app.on_key_authenticated(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE))
            .await;
        assert_eq!(app.events_filter_mode, EventsFilterMode::HueInputs);

        app.on_key_authenticated(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE))
            .await;
        assert_eq!(app.events_filter_mode, EventsFilterMode::Entertainment);

        app.on_key_authenticated(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE))
            .await;
        assert_eq!(app.events_filter_mode, EventsFilterMode::PluginMetrics);

        app.on_key_authenticated(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE))
            .await;
        assert_eq!(app.events_filter_mode, EventsFilterMode::All);
    }

    #[tokio::test]
    async fn plugin_detail_key_flow_open_switch_close() {
        let mut app = test_app();
        make_admin(&mut app);
        app.plugins.push(PluginRecord {
            plugin_id: "plugin.hue".to_string(),
            registered_at: "2026-03-21T00:00:00Z".to_string(),
            status: "active".to_string(),
        });
        app.selected = 0;
        app.tab = app
            .tabs()
            .iter()
            .position(|t| matches!(t, Tab::Plugins))
            .unwrap_or(0);

        app.on_key_authenticated(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .await;
        assert!(app.plugin_detail_open);
        assert_eq!(app.plugin_detail_plugin_id.as_deref(), Some("plugin.hue"));
        assert_eq!(app.plugin_detail_panel, PluginDetailPanel::Overview);

        app.on_key_authenticated(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE))
            .await;
        assert_eq!(app.plugin_detail_panel, PluginDetailPanel::Diagnostics);

        app.on_key_authenticated(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE))
            .await;
        assert_eq!(app.plugin_detail_panel, PluginDetailPanel::Metrics);

        app.on_key_authenticated(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
            .await;
        assert_eq!(app.plugin_detail_panel, PluginDetailPanel::Overview);

        app.on_key_authenticated(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
            .await;
        assert_eq!(app.plugin_detail_panel, PluginDetailPanel::Metrics);

        app.on_key_authenticated(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .await;
        assert!(!app.plugin_detail_open);
        assert!(app.plugin_detail_plugin_id.is_none());
    }

    #[test]
    fn selected_plugin_hue_bridge_ids_filters_by_plugin_and_kind() {
        let mut app = test_app();
        app.devices = vec![
            mk_device("bridge-1", "plugin.hue", "hue_bridge"),
            mk_device("bridge-2", "plugin.hue", "hue_bridge"),
            mk_device("light-1", "plugin.hue", "light"),
            mk_device("bridge-other", "plugin.other", "hue_bridge"),
        ];

        let ids = app.selected_plugin_hue_bridge_ids("plugin.hue");
        assert_eq!(ids, vec!["bridge-1".to_string(), "bridge-2".to_string()]);
    }

    #[tokio::test]
    async fn pairing_key_shows_no_bridge_error_when_none_found() {
        let mut app = test_app();
        make_user(&mut app);
        app.plugin_detail_open = true;
        app.plugin_detail_plugin_id = Some("plugin.hue".to_string());
        app.devices = vec![mk_device("light-1", "plugin.hue", "light")];

        app.on_key_authenticated(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE))
            .await;

        assert_eq!(app.error.as_deref(), Some("No Hue bridges found for selected plugin"));
    }

    #[tokio::test]
    async fn pairing_key_preserves_pairing_error_when_refresh_fails() {
        let mut app = test_app();
        make_user(&mut app);
        app.plugin_detail_open = true;
        app.plugin_detail_plugin_id = Some("plugin.hue".to_string());
        app.devices = vec![mk_device("bridge-1", "plugin.hue", "hue_bridge")];

        app.on_key_authenticated(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE))
            .await;

        let error = app.error.unwrap_or_default();
        assert!(error.starts_with("Pairing request errors:"));
        assert!(error.contains("bridge-1: not authenticated"));
    }
}

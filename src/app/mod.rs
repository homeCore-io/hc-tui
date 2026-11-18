//! Application state and input handling.
//!
//! Nothing here awaits the network. A key handler that needs the server
//! calls [`App::spawn`], which runs the request on a tokio task and sends
//! the result back on the [`AsyncMsg`] channel as a closure that is
//! applied to the app on the main loop. The event loop in `main.rs`
//! selects over terminal input, that channel and a tick timer.

pub mod areas;
pub mod devices;
pub mod editors;
pub mod events;
pub mod manage;
pub mod media;
pub mod plugins;
pub mod rules;
pub mod selection;

use crate::cache::{CacheSnapshot, CacheStore, SavedSession};
use crate::config::AutoLoginConfig;
use crate::model::{
    Area, DeviceState, EventEntry, MatterNode, ModeRecord, PluginRecord, Rule, Scene, SystemStatus,
    TempUnit, UserInfo, merge_scenes,
};
use crate::net::sse::StreamMsg;
use crate::net::ws::{LogMsg, WsMsg};
use crate::net::{self, HomeCoreClient};
use crate::sync::{SyncPlan, SyncReport};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashSet;
use std::future::Future;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;
use zeroize::Zeroize;

pub use areas::{AreasPane, AreasTab};
pub use devices::{DeviceSubPanel, DeviceViewMode, DevicesTab};
pub use editors::Confirm;
pub use manage::{AdminSubPanel, ManageTab};
pub use plugins::{PluginDetailPanel, PluginsTab, StreamingAction, StreamingStage};
pub use rules::RulesTab;
pub use selection::ListSel;

/// A result applied to the app on the main loop.
pub type ApplyFn = Box<dyn FnOnce(&mut App) + Send>;

/// Everything that reaches the main loop besides terminal input.
pub enum AsyncMsg {
    Apply(ApplyFn),
    Ws(WsMsg),
    Log(LogMsg),
    Stream(StreamMsg),
}

impl From<WsMsg> for AsyncMsg {
    fn from(m: WsMsg) -> Self {
        AsyncMsg::Ws(m)
    }
}

impl From<LogMsg> for AsyncMsg {
    fn from(m: LogMsg) -> Self {
        AsyncMsg::Log(m)
    }
}

impl From<StreamMsg> for AsyncMsg {
    fn from(m: StreamMsg) -> Self {
        AsyncMsg::Stream(m)
    }
}

/// A value that loads in the background.
#[derive(Debug, Clone, Default)]
pub enum Load<T> {
    #[default]
    Idle,
    Loading,
    Ready(T),
    Failed(String),
}

impl<T> Load<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Load::Ready(v) => Some(v),
            _ => None,
        }
    }

    pub fn is_loading(&self) -> bool {
        matches!(self, Load::Loading)
    }

    pub fn error(&self) -> Option<&str> {
        match self {
            Load::Failed(e) => Some(e),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusField {
    Username,
    Password,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginPhase {
    Restoring,
    Authenticating,
}

#[derive(Default)]
pub struct LoginState {
    pub username: String,
    /// Cleared (and zeroed) as soon as a login starts.
    pub password: String,
    pub focus: Option<FocusField>,
    pub in_progress: bool,
    pub phase: Option<LoginPhase>,
}

impl std::fmt::Debug for LoginState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginState")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("in_progress", &self.in_progress)
            .finish()
    }
}

impl LoginState {
    pub fn focus(&self) -> FocusField {
        self.focus.unwrap_or(FocusField::Username)
    }
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

/// Cached data sections, for deciding what may be written to disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Devices,
    Scenes,
    Areas,
    Rules,
    Events,
    Users,
    Plugins,
    Modes,
}

/// Derived lists, rebuilt once per frame by [`App::refresh_views`].
/// Each holds indexes into the matching data vector, in display order.
#[derive(Debug, Clone, Default)]
pub struct Views {
    pub devices: Vec<usize>,
    /// Group headers for the grouped device view: label and how many of
    /// the following `devices` entries belong to it.
    pub device_groups: Vec<(String, usize)>,
    pub media: Vec<usize>,
    pub rules: Vec<usize>,
    pub events: Vec<usize>,
    pub area_devices: Vec<usize>,
}

pub struct App {
    pub client: HomeCoreClient,
    pub cache: CacheStore,
    pub persist_session: bool,
    tx: UnboundedSender<AsyncMsg>,

    pub login: LoginState,
    pub authenticated: bool,
    pub current_user: Option<UserInfo>,
    pub status: String,
    pub error: Option<String>,
    pub should_quit: bool,
    pub tab: usize,
    pub time_utc: bool,

    /// Requests in flight. Drives the spinner.
    pub pending: usize,
    /// Advanced by the tick timer.
    pub tick: u64,
    pub syncing: bool,
    sync_seq: u64,
    /// Calls that failed in the last sync, by section.
    pub sync_errors: Vec<(String, String)>,
    /// Sections that came from a successful call this session. Only
    /// these are ever written back to the cache.
    fresh: HashSet<Section>,
    saved_session_version: u64,
    /// Bumped on sign-out; results of requests made before that are
    /// dropped instead of applied.
    epoch: u64,

    pub ws_connected: bool,
    ws_task: Option<JoinHandle<()>>,

    // Data.
    pub devices: Vec<DeviceState>,
    /// Native scenes as the server sent them.
    pub native_scenes: Vec<Scene>,
    /// Native scenes plus scene devices; what the Scenes tab shows.
    pub scenes: Vec<Scene>,
    pub areas: Vec<Area>,
    pub rules: Vec<Rule>,
    pub events: Vec<EventEntry>,
    pub users: Vec<UserInfo>,
    pub plugins: Vec<PluginRecord>,
    pub modes: Vec<ModeRecord>,
    pub matter_nodes: Vec<MatterNode>,
    pub system_status: Option<SystemStatus>,
    pub temp_unit: TempUnit,

    pub views: Views,

    // Per-tab state.
    pub devices_tab: DevicesTab,
    pub scenes_sel: ListSel,
    pub areas_tab: AreasTab,
    pub rules_tab: RulesTab,
    pub plugins_tab: PluginsTab,
    pub manage: ManageTab,

    /// Yes/no question over everything else.
    pub confirm: Option<Confirm>,
}

impl App {
    pub fn new(base_url: String, cache: CacheStore) -> (Self, UnboundedReceiver<AsyncMsg>) {
        let (tx, rx) = unbounded_channel();
        let app = Self {
            client: HomeCoreClient::new(base_url),
            cache,
            persist_session: true,
            tx,
            login: LoginState::default(),
            authenticated: false,
            current_user: None,
            status: "Enter credentials and press Enter".to_string(),
            error: None,
            should_quit: false,
            tab: 0,
            time_utc: false,
            pending: 0,
            tick: 0,
            syncing: false,
            sync_seq: 0,
            sync_errors: Vec::new(),
            fresh: HashSet::new(),
            saved_session_version: 0,
            epoch: 0,
            ws_connected: false,
            ws_task: None,
            devices: Vec::new(),
            native_scenes: Vec::new(),
            scenes: Vec::new(),
            areas: Vec::new(),
            rules: Vec::new(),
            events: Vec::new(),
            users: Vec::new(),
            plugins: Vec::new(),
            modes: Vec::new(),
            matter_nodes: Vec::new(),
            system_status: None,
            temp_unit: TempUnit::default(),
            views: Views::default(),
            devices_tab: DevicesTab::default(),
            scenes_sel: ListSel::default(),
            areas_tab: AreasTab::default(),
            rules_tab: RulesTab::default(),
            plugins_tab: PluginsTab::default(),
            manage: ManageTab::default(),
            confirm: None,
        };
        (app, rx)
    }

    /// A sender for the main loop's channel, for stream tasks.
    pub fn sender(&self) -> UnboundedSender<AsyncMsg> {
        self.tx.clone()
    }

    // ── Background work ──────────────────────────────────────────────────

    /// Run `fut` on a tokio task and apply its result on the main loop.
    /// An expired session (after a failed refresh) sends the user back
    /// to the login screen instead of calling `apply`.
    pub fn spawn<T, F, A>(&mut self, fut: F, apply: A)
    where
        T: Send + 'static,
        F: Future<Output = Result<T>> + Send + 'static,
        A: FnOnce(&mut App, Result<T>) + Send + 'static,
    {
        self.pending += 1;
        let tx = self.tx.clone();
        let epoch = self.epoch;
        tokio::spawn(async move {
            let res = fut.await;
            let msg: ApplyFn = Box::new(move |app: &mut App| {
                app.pending = app.pending.saturating_sub(1);
                if app.epoch != epoch {
                    return; // made under a session that has since ended
                }
                if let Err(e) = &res
                    && net::is_unauthorized(e)
                {
                    app.on_session_expired();
                    return;
                }
                apply(app, res);
            });
            let _ = tx.send(AsyncMsg::Apply(msg));
        });
    }

    /// Like [`spawn`](Self::spawn), but an error just lands in the status
    /// line as `"{what}: {error}"`.
    pub fn spawn_ok<T, F, A>(&mut self, what: &'static str, fut: F, on_ok: A)
    where
        T: Send + 'static,
        F: Future<Output = Result<T>> + Send + 'static,
        A: FnOnce(&mut App, T) + Send + 'static,
    {
        self.spawn(fut, move |app, res| match res {
            Ok(v) => on_ok(app, v),
            Err(e) => app.error = Some(format!("{what}: {e}")),
        });
    }

    pub fn on_msg(&mut self, msg: AsyncMsg) {
        match msg {
            AsyncMsg::Apply(f) => f(self),
            AsyncMsg::Ws(m) => self.on_ws(m),
            AsyncMsg::Log(m) => self.on_log_msg(m),
            AsyncMsg::Stream(m) => self.on_stream_msg(m),
        }
    }

    /// Called every tick (250ms). Returns true when the screen should be
    /// redrawn: something is animating or counting down.
    pub fn on_tick(&mut self) -> bool {
        self.tick = self.tick.wrapping_add(1);
        if self.authenticated {
            if self.client.needs_refresh() && self.pending == 0 {
                let c = self.client.clone();
                self.spawn(async move { c.refresh_session().await }, |app, res| {
                    if let Err(e) = res {
                        app.error = Some(format!("session refresh: {e}"));
                    }
                });
            }
            self.persist_session_if_changed();
        }
        self.pending > 0
            || self.login.in_progress
            || self.syncing
            || self.plugins_tab.streaming.is_some()
            || (self.authenticated && self.active_tab() == Tab::Devices && self.any_timer_running())
    }

    pub fn spinner(&self) -> &'static str {
        const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        SPINNER[(self.tick as usize) % SPINNER.len()]
    }

    pub fn busy(&self) -> bool {
        self.pending > 0 || self.syncing
    }

    // ── Permissions and tabs ─────────────────────────────────────────────

    pub fn is_admin(&self) -> bool {
        self.current_user
            .as_ref()
            .map(|u| u.role.is_admin())
            .unwrap_or(false)
    }

    pub fn can(&self, scope: &str) -> bool {
        self.current_user
            .as_ref()
            .map(|u| u.can(scope))
            .unwrap_or(false)
    }

    pub fn tabs(&self) -> Vec<Tab> {
        let mut tabs = vec![Tab::Devices, Tab::Scenes, Tab::Areas, Tab::Rules];
        if self.can("plugins:read") {
            tabs.push(Tab::Plugins);
        }
        tabs.push(Tab::Manage);
        tabs
    }

    pub fn active_tab(&self) -> Tab {
        let tabs = self.tabs();
        tabs[self.tab.min(tabs.len().saturating_sub(1))]
    }

    fn sync_plan(&self) -> SyncPlan {
        SyncPlan {
            users: self.is_admin(),
            plugins: self.can("plugins:read"),
        }
    }

    // ── Login and session ────────────────────────────────────────────────

    /// Validate the form and start a login. The password is moved out of
    /// the form (and zeroed after use).
    pub fn begin_login(&mut self) {
        self.error = None;
        if self.login.username.trim().is_empty() || self.login.password.is_empty() {
            self.error = Some("username and password are required".to_string());
            return;
        }
        let username = self.login.username.trim().to_string();
        let password = std::mem::take(&mut self.login.password);
        self.start_login(username, password);
    }

    fn start_login(&mut self, username: String, mut password: String) {
        self.login.in_progress = true;
        self.login.phase = Some(LoginPhase::Authenticating);
        self.status = "Signing in...".to_string();
        let c = self.client.clone();
        self.spawn(
            async move {
                let res = c.login(&username, &password).await;
                password.zeroize();
                res
            },
            |app, res| match res {
                Ok(resp) => {
                    app.client.set_session_from(&resp);
                    app.on_authenticated(resp.user);
                }
                Err(e) => app.on_login_failed(e.to_string()),
            },
        );
    }

    /// Sign in with the `[auto_login]` credentials from the config file.
    pub fn begin_auto_login(&mut self, cfg: &AutoLoginConfig) {
        self.login.username = cfg.username.clone();
        self.status = format!("Signing in as {}...", cfg.username);
        self.start_login(cfg.username.clone(), cfg.password.clone());
    }

    /// Try a session saved on disk: install its tokens and ask `/auth/me`.
    /// An expired access token is refreshed on the way.
    pub fn begin_restore(&mut self, saved: SavedSession) {
        self.login.username = saved.username.clone();
        self.login.focus = Some(FocusField::Password);
        self.login.in_progress = true;
        self.login.phase = Some(LoginPhase::Restoring);
        self.status = "Restoring session...".to_string();
        self.client
            .set_tokens(saved.token, saved.refresh_token, saved.expires_at);
        let c = self.client.clone();
        self.spawn(async move { c.me().await }, |app, res| match res {
            Ok(user) => app.on_authenticated(user),
            Err(e) => {
                app.client.clear_session();
                app.on_login_failed(format!("saved session not usable: {e}"));
                app.error = None;
                app.status = "Enter password and press Enter".to_string();
            }
        });
    }

    fn on_login_failed(&mut self, error: String) {
        self.login.in_progress = false;
        self.login.phase = None;
        self.error = Some(error);
        self.status = "Authentication failed".to_string();
    }

    /// Signed in: show cached data at once, then sync and go live.
    pub fn on_authenticated(&mut self, user: UserInfo) {
        self.login.in_progress = false;
        self.login.phase = None;
        self.login.password.zeroize();
        self.authenticated = true;
        self.error = None;
        let username = user.username.clone();
        self.current_user = Some(user);
        self.saved_session_version = 0;
        self.persist_session_if_changed();

        let cached = self.cache.load_snapshot(&username);
        let had_cache = !cached.is_empty();
        self.apply_cached(cached);
        self.status = if had_cache {
            "Signed in; showing cached data while syncing".to_string()
        } else {
            "Signed in; syncing".to_string()
        };
        self.start_event_stream();
        self.start_sync();
    }

    /// The server rejected the session and it could not be refreshed.
    pub fn on_session_expired(&mut self) {
        if !self.authenticated && !self.login.in_progress {
            return;
        }
        let username = self
            .current_user
            .as_ref()
            .map(|u| u.username.clone())
            .unwrap_or_else(|| self.login.username.clone());
        self.sign_out_locally();
        self.login.username = username;
        self.login.focus = Some(FocusField::Password);
        self.error = Some("Session expired; sign in again".to_string());
    }

    /// Drop the session and every stream and return to the login screen.
    fn sign_out_locally(&mut self) {
        self.epoch += 1;
        self.client.clear_session();
        let _ = self.cache.clear_session();
        self.stop_event_stream();
        self.stop_log_stream();
        self.close_streaming_action();
        self.authenticated = false;
        self.current_user = None;
        self.login = LoginState::default();
        self.syncing = false;
        self.sync_errors.clear();
        self.fresh.clear();
        self.confirm = None;
        // Nothing of this user's data may show for the next one.
        self.devices.clear();
        self.native_scenes.clear();
        self.scenes.clear();
        self.areas.clear();
        self.rules.clear();
        self.events.clear();
        self.users.clear();
        self.plugins.clear();
        self.modes.clear();
        self.matter_nodes.clear();
        self.system_status = None;
        self.views = Views::default();
        self.devices_tab = DevicesTab::default();
        self.scenes_sel = ListSel::default();
        self.areas_tab = AreasTab::default();
        self.rules_tab = RulesTab::default();
        self.plugins_tab = PluginsTab::default();
        self.manage = ManageTab::default();
        self.tab = 0;
        self.status = "Enter credentials and press Enter".to_string();
    }

    /// `L`: revoke the refresh token and sign out.
    pub fn logout(&mut self) {
        let username = self
            .current_user
            .as_ref()
            .map(|u| u.username.clone())
            .unwrap_or_default();
        // Take the refresh token before the local session is dropped.
        if let Some(rt) = self.client.tokens().refresh {
            let c = self.client.clone();
            tokio::spawn(async move {
                let _ = c.revoke(rt).await;
            });
        }
        self.sign_out_locally();
        self.login.username = username;
        self.login.focus = Some(FocusField::Password);
        self.status = "Signed out".to_string();
    }

    /// Save the session file whenever the tokens changed (login, refresh).
    fn persist_session_if_changed(&mut self) {
        if !self.persist_session || !self.authenticated {
            return;
        }
        let version = self.client.session_version();
        if version == self.saved_session_version {
            return;
        }
        self.saved_session_version = version;
        let tokens = self.client.tokens();
        let (Some(token), Some(user)) = (tokens.access, self.current_user.as_ref()) else {
            return;
        };
        let session = SavedSession {
            username: user.username.clone(),
            token,
            refresh_token: tokens.refresh,
            expires_at: tokens.expires_at,
        };
        let cache = self.cache.clone();
        tokio::task::spawn_blocking(move || {
            let _ = cache.save_session(&session);
        });
    }

    // ── Sync ─────────────────────────────────────────────────────────────

    /// Fetch everything concurrently; see `sync.rs`.
    pub fn start_sync(&mut self) {
        if !self.authenticated {
            return;
        }
        self.sync_seq += 1;
        let seq = self.sync_seq;
        self.syncing = true;
        let c = self.client.clone();
        let plan = self.sync_plan();
        self.spawn(
            async move { Ok(crate::sync::run_sync(c, plan).await) },
            move |app, res: Result<SyncReport>| {
                if seq != app.sync_seq {
                    return; // a newer sync superseded this one
                }
                app.syncing = false;
                if let Ok(report) = res {
                    app.apply_sync(report);
                }
            },
        );
    }

    fn apply_cached(&mut self, snap: CacheSnapshot) {
        if let Some(v) = snap.devices {
            self.devices = v;
        }
        if let Some(v) = snap.scenes {
            self.native_scenes = v;
        }
        if let Some(v) = snap.areas {
            self.areas = v;
        }
        if let Some(v) = snap.rules {
            self.rules = v;
        }
        if let Some(v) = snap.events {
            self.events = v;
        }
        if let Some(v) = snap.users {
            self.users = v;
        }
        if let Some(v) = snap.plugins {
            self.plugins = v;
        }
        if let Some(v) = snap.modes {
            self.modes = v;
        }
        self.rebuild_scenes();
    }

    /// Apply a sync report: every successful part replaces its section;
    /// every failure is named in the status line and leaves both the
    /// in-memory data and the cache file alone.
    pub fn apply_sync(&mut self, report: SyncReport) {
        if report.unauthorized() {
            self.on_session_expired();
            return;
        }
        let errors = report.errors();
        let mut fresh = CacheSnapshot::default();

        if let Ok(me) = report.me {
            self.current_user = Some(me);
        }
        if let Ok(v) = report.devices {
            self.devices = v;
            self.fresh.insert(Section::Devices);
            fresh.devices = Some(self.devices.clone());
        }
        if let Ok(v) = report.scenes {
            self.native_scenes = v;
            self.fresh.insert(Section::Scenes);
            fresh.scenes = Some(self.native_scenes.clone());
        }
        self.rebuild_scenes();
        if let Ok(v) = report.areas {
            self.areas = v;
            self.fresh.insert(Section::Areas);
            fresh.areas = Some(self.areas.clone());
        }
        if let Ok(v) = report.rules {
            self.rules = v;
            self.fresh.insert(Section::Rules);
            fresh.rules = Some(self.rules.clone());
        }
        if let Ok(v) = report.events {
            self.events = events::entries_from_log(&v);
            self.fresh.insert(Section::Events);
            fresh.events = Some(self.events.clone());
        }
        if let Ok(v) = report.modes {
            self.modes = v;
            self.fresh.insert(Section::Modes);
            fresh.modes = Some(self.modes.clone());
        }
        if let Ok(s) = report.status {
            self.set_system_status(s);
        }
        if let Some(Ok(v)) = report.users {
            self.users = v;
            self.fresh.insert(Section::Users);
            fresh.users = Some(self.users.clone());
        }
        if let Some(Ok(v)) = report.plugins {
            self.plugins = v;
            self.fresh.insert(Section::Plugins);
            fresh.plugins = Some(self.plugins.clone());
        }

        self.write_cache(fresh);

        self.sync_errors = errors
            .iter()
            .map(|(s, e)| (s.to_string(), e.clone()))
            .collect();
        if errors.is_empty() {
            self.status = format!(
                "Synced {} devices, {} rules, {} scenes",
                self.devices.len(),
                self.rules.len(),
                self.scenes.len()
            );
        } else {
            let names: Vec<&str> = errors.iter().map(|(s, _)| *s).collect();
            self.status = format!("Sync incomplete: {} failed", names.join(", "));
        }
    }

    pub fn set_system_status(&mut self, s: SystemStatus) {
        if let Some(u) = s.temperature_unit.as_deref() {
            self.temp_unit = TempUnit::parse(u);
        }
        self.system_status = Some(s);
    }

    pub fn rebuild_scenes(&mut self) {
        self.scenes = merge_scenes(self.native_scenes.clone(), &self.devices);
    }

    /// Write `snap`'s sections to the cache in the background.
    fn write_cache(&self, snap: CacheSnapshot) {
        let Some(user) = self.current_user.as_ref() else {
            return;
        };
        let username = user.username.clone();
        let cache = self.cache.clone();
        tokio::task::spawn_blocking(move || {
            let _ = cache.save_sections(&username, &snap);
        });
    }

    /// After a local edit, save the sections named, but only those that
    /// came from the server this session: data still from an old cache
    /// (because its sync failed) is never written back.
    pub fn save_cache(&self, sections: &[Section]) {
        let mut snap = CacheSnapshot::default();
        for s in sections {
            if !self.fresh.contains(s) {
                continue;
            }
            match s {
                Section::Devices => snap.devices = Some(self.devices.clone()),
                Section::Scenes => snap.scenes = Some(self.native_scenes.clone()),
                Section::Areas => snap.areas = Some(self.areas.clone()),
                Section::Rules => snap.rules = Some(self.rules.clone()),
                Section::Events => snap.events = Some(self.events.clone()),
                Section::Users => snap.users = Some(self.users.clone()),
                Section::Plugins => snap.plugins = Some(self.plugins.clone()),
                Section::Modes => snap.modes = Some(self.modes.clone()),
            }
        }
        self.write_cache(snap);
    }

    // ── Live event stream ────────────────────────────────────────────────

    fn start_event_stream(&mut self) {
        self.stop_event_stream();
        self.ws_task = Some(net::ws::spawn_events_stream(
            self.client.clone(),
            self.tx.clone(),
        ));
    }

    fn stop_event_stream(&mut self) {
        if let Some(t) = self.ws_task.take() {
            t.abort();
        }
        self.ws_connected = false;
    }

    fn on_ws(&mut self, msg: WsMsg) {
        if !self.authenticated {
            return;
        }
        match msg {
            WsMsg::Connected { attempt } => {
                self.ws_connected = true;
                if attempt > 0 {
                    // Events may have been missed while disconnected.
                    self.status = "Live stream reconnected; resyncing".to_string();
                    self.start_sync();
                }
            }
            WsMsg::Disconnected(reason) => {
                if self.ws_connected {
                    self.status = format!("Live stream disconnected ({reason})");
                }
                self.ws_connected = false;
            }
            WsMsg::Lagged { missed } => {
                self.status = format!("Live stream missed {missed} events; resyncing");
                self.start_sync();
            }
            WsMsg::Event(value) => self.apply_event(value),
        }
    }

    // ── Derived views ────────────────────────────────────────────────────

    /// Rebuild every derived list and re-find each selection by id.
    /// Called once per frame, before drawing.
    pub fn refresh_views(&mut self) {
        self.compute_device_views();
        self.compute_rule_view();
        self.compute_event_view();
        self.compute_area_views();
        self.resolve_selections();
    }

    /// Open or close streams that depend on what is on screen (the log
    /// stream follows the Logs panel). Called once per frame.
    pub fn update_streams(&mut self) {
        self.update_log_stream();
    }

    fn resolve_selections(&mut self) {
        let ids = self.device_view_ids();
        self.devices_tab.sel.resolve(&ids);
        let ids = self.media_view_ids();
        self.devices_tab.media_sel.resolve(&ids);
        let ids: Vec<&str> = self.scenes.iter().map(|s| s.id.as_str()).collect();
        self.scenes_sel.resolve(&ids);
        let ids: Vec<&str> = self.areas.iter().map(|a| a.id.as_str()).collect();
        self.areas_tab.list_sel.resolve(&ids);
        let ids = self.area_device_ids();
        self.areas_tab.devices_sel.resolve(&ids);
        let ids = self.rule_view_ids();
        self.rules_tab.sel.resolve(&ids);
        let ids: Vec<&str> = self.plugins.iter().map(|p| p.plugin_id.as_str()).collect();
        self.plugins_tab.sel.resolve(&ids);
        self.resolve_plugin_detail_selection();
        self.resolve_manage_selections();
    }

    // ── Keys ─────────────────────────────────────────────────────────────

    pub fn on_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return;
        }
        if !self.authenticated {
            self.on_key_login(key);
            return;
        }
        self.error = None;
        if self.confirm.is_some() {
            self.on_key_confirm(key);
            return;
        }
        if self.plugins_tab.streaming.is_some() {
            self.on_key_streaming(key);
            return;
        }
        if self.rules_tab.detail.is_some() {
            self.on_key_rule_detail(key);
            return;
        }
        if self.plugins_tab.detail.is_some() && self.active_tab() == Tab::Plugins {
            self.on_key_plugin_detail(key);
            return;
        }
        if self.on_key_editors(key) {
            return;
        }
        if self.devices_tab.search_open {
            self.on_key_device_search(key);
            return;
        }
        if self.rules_tab.filter_bar.is_some() {
            self.on_key_rule_filter_bar(key);
            return;
        }
        if self.manage.logs.input_open {
            self.on_key_log_filter_input(key);
            return;
        }
        if self.rules_tab.groups.is_some() {
            self.on_key_groups(key);
            return;
        }
        if self.active_tab() == Tab::Areas && !is_global_key(key) {
            self.on_key_areas(key);
            return;
        }
        self.on_key_global(key);
    }

    fn on_key_login(&mut self, key: KeyEvent) {
        if self.login.in_progress {
            if key.code == KeyCode::Esc {
                self.should_quit = true;
            }
            return;
        }
        match key.code {
            KeyCode::Esc => self.should_quit = true,
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Down | KeyCode::Up => {
                self.login.focus = Some(match self.login.focus() {
                    FocusField::Username => FocusField::Password,
                    FocusField::Password => FocusField::Username,
                });
            }
            KeyCode::Backspace => {
                match self.login.focus() {
                    FocusField::Username => self.login.username.pop(),
                    FocusField::Password => self.login.password.pop(),
                };
            }
            KeyCode::Enter => self.begin_login(),
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                match self.login.focus() {
                    FocusField::Username => self.login.username.push(ch),
                    FocusField::Password => self.login.password.push(ch),
                }
            }
            _ => {}
        }
    }

    fn switch_tab(&mut self, idx: usize) {
        let count = self.tabs().len();
        if count == 0 {
            return;
        }
        self.tab = idx % count;
        self.areas_tab.pane = AreasPane::AreasList;
        self.areas_tab.marked.clear();
        if self.active_tab() == Tab::Manage {
            self.on_enter_manage_panel();
        }
    }

    fn on_key_global(&mut self, key: KeyEvent) {
        let tab = self.active_tab();
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('T') => {
                self.time_utc = !self.time_utc;
                self.status = if self.time_utc {
                    "Timestamps: UTC".to_string()
                } else {
                    "Timestamps: Local".to_string()
                };
            }
            KeyCode::Char('L') => self.logout(),
            KeyCode::Tab => self.switch_tab(self.tab + 1),
            KeyCode::BackTab => {
                let count = self.tabs().len();
                self.switch_tab(self.tab + count - 1);
            }
            KeyCode::Char(c @ '1'..='9') => {
                let idx = c as usize - '1' as usize;
                if idx < self.tabs().len() {
                    self.switch_tab(idx);
                }
            }
            KeyCode::Char('r') if tab == Tab::Manage => self.refresh_manage_panel(),
            KeyCode::Char('r') => {
                self.status = "Refreshing...".to_string();
                self.start_sync();
            }
            _ => match tab {
                Tab::Devices => self.on_key_devices(key),
                Tab::Scenes => self.on_key_scenes(key),
                Tab::Areas => {}
                Tab::Rules => self.on_key_rules(key),
                Tab::Plugins => self.on_key_plugins(key),
                Tab::Manage => self.on_key_manage(key),
            },
        }
    }

    fn on_key_scenes(&mut self, key: KeyEvent) {
        let ids: Vec<String> = self.scenes.iter().map(|s| s.id.clone()).collect();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.scenes_sel.move_by(1, &ids),
            KeyCode::Up | KeyCode::Char('k') => self.scenes_sel.move_by(-1, &ids),
            KeyCode::Char('a') | KeyCode::Enter => self.activate_selected_scene(),
            _ => {}
        }
    }

    pub fn selected_scene(&self) -> Option<&Scene> {
        match self.scenes_sel.id() {
            Some(id) => self.scenes.iter().find(|s| s.id == id),
            None => self.scenes.get(self.scenes_sel.index()),
        }
    }

    /// Activate a native scene through `/scenes/{id}/activate`, a scene
    /// device with `{"action":"activate"}`.
    fn activate_selected_scene(&mut self) {
        let Some(scene) = self.selected_scene().cloned() else {
            return;
        };
        let c = self.client.clone();
        let name = scene.name.clone();
        self.status = format!("Activating '{name}'...");
        self.spawn_ok(
            "activate scene",
            async move {
                match scene.source {
                    crate::model::SceneSource::Native => c.activate_scene(&scene.id).await,
                    crate::model::SceneSource::Device => {
                        c.send_device_action(&scene.id, "activate").await
                    }
                }
            },
            move |app, ()| app.status = format!("Activated scene '{name}'"),
        );
    }

    // ── Confirmation ─────────────────────────────────────────────────────

    pub fn ask(&mut self, confirm: Confirm) {
        self.confirm = Some(confirm);
    }

    fn on_key_confirm(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if let Some(c) = self.confirm.take() {
                    self.run_confirmed(c.action);
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                self.confirm = None;
                self.status = "Cancelled".to_string();
            }
            _ => {}
        }
    }

    fn run_confirmed(&mut self, action: editors::ConfirmAction) {
        use editors::ConfirmAction as C;
        match action {
            C::DeleteDevice { id } => self.delete_device(id),
            C::DeleteArea { id, name } => self.delete_area(id, name),
            C::DeleteRule { id, name } => self.delete_rule(id, name),
            C::DeleteRuleGroup { id, name } => self.delete_rule_group(id, name),
            C::DeleteUser { id, username } => self.delete_user(id, username),
            C::DeleteMode { id } => self.delete_mode(id),
            C::RemoveMatterNode { node_id } => self.remove_matter_node(node_id),
            C::UninstallPlugin { plugin_id } => self.uninstall_plugin(plugin_id),
            C::ImportRules => self.run_backup_action(manage::backup::BackupAction::ImportRules),
            C::ImportScenes => self.run_backup_action(manage::backup::BackupAction::ImportScenes),
            C::RunPluginAction { plugin_id, action } => self.run_plugin_action(plugin_id, *action),
        }
    }
}

/// Keys that work the same on every tab, even the Areas two-pane view.
fn is_global_key(key: KeyEvent) -> bool {
    matches!(
        key.code,
        KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Char('1'..='9')
            | KeyCode::Char('q')
            | KeyCode::Char('r')
            | KeyCode::Char('T')
            | KeyCode::Char('L')
    )
}

/// Format a timestamp string for display. Respects the `utc` flag.
pub fn format_timestamp_utc(ts: &str, utc: bool) -> String {
    use chrono::{DateTime, Local, Utc};
    if let Ok(dt) = DateTime::parse_from_rfc3339(ts) {
        return if utc {
            dt.with_timezone(&Utc)
                .format("%Y-%m-%d %H:%M:%S UTC")
                .to_string()
        } else {
            dt.with_timezone(&Local).format("%H:%M:%S").to_string()
        };
    }
    crate::text::take_chars(ts, 19)
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::model::Role;

    pub fn app_with_rx() -> (App, UnboundedReceiver<AsyncMsg>) {
        let dir = std::env::temp_dir().join(format!("hc-tui-app-test-{}", std::process::id()));
        App::new("http://127.0.0.1:9".to_string(), CacheStore::new(dir))
    }

    pub fn app() -> App {
        app_with_rx().0
    }

    pub fn user(role: Role) -> UserInfo {
        UserInfo {
            id: format!("id-{}", role.as_str()),
            username: role.as_str().to_string(),
            role,
            created_at: String::new(),
            scopes: None,
        }
    }

    pub fn signed_in(role: Role) -> App {
        let mut a = app();
        a.current_user = Some(user(role));
        a.authenticated = true;
        a
    }

    pub fn device(id: &str, name: &str) -> DeviceState {
        serde_json::from_value(serde_json::json!({
            "device_id": id, "name": name, "plugin_id": "plugin.test",
            "available": true, "attributes": {}, "last_seen": ""
        }))
        .unwrap()
    }

    pub fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use crate::model::Role;

    #[test]
    fn plugins_tab_follows_plugins_read() {
        // `user` has no plugins:read in core's role table; observer does.
        let a = signed_in(Role::User);
        assert!(!a.tabs().contains(&Tab::Plugins));
        let a = signed_in(Role::Observer);
        assert!(a.tabs().contains(&Tab::Plugins));
        let a = signed_in(Role::Admin);
        assert!(a.tabs().contains(&Tab::Plugins));
    }

    #[test]
    fn server_scopes_win_over_the_role_table() {
        let mut a = signed_in(Role::User);
        a.current_user.as_mut().unwrap().scopes = Some(vec!["plugins:read".into()]);
        assert!(a.tabs().contains(&Tab::Plugins));
    }

    #[tokio::test]
    async fn results_from_an_ended_session_are_dropped() {
        let (mut a, mut rx) = app_with_rx();
        a.current_user = Some(user(Role::Admin));
        a.authenticated = true;
        a.devices = vec![device("d", "Lamp")];
        a.spawn(async { Ok(()) }, |app, _| app.status = "applied".into());
        a.logout();
        assert!(a.devices.is_empty(), "sign-out clears the old user's data");
        let msg = rx.recv().await.unwrap();
        a.on_msg(msg);
        assert_ne!(a.status, "applied");
        assert_eq!(a.pending, 0);
    }

    #[tokio::test]
    async fn spawned_results_come_back_through_the_channel() {
        let (mut a, mut rx) = app_with_rx();
        a.current_user = Some(user(Role::Admin));
        a.authenticated = true;
        a.spawn(async { Ok(7) }, |app, res: Result<i32>| {
            app.status = format!("got {}", res.unwrap())
        });
        assert_eq!(a.pending, 1, "the key handler returns before the result");
        let msg = rx.recv().await.unwrap();
        a.on_msg(msg);
        assert_eq!(a.status, "got 7");
        assert_eq!(a.pending, 0);
    }

    #[tokio::test]
    async fn an_expired_session_goes_back_to_login() {
        let (mut a, mut rx) = app_with_rx();
        a.current_user = Some(user(Role::Admin));
        a.authenticated = true;
        a.spawn(
            async { Err::<(), _>(anyhow::Error::from(net::Unauthorized)) },
            |app, _| app.status = "should not run".into(),
        );
        let msg = rx.recv().await.unwrap();
        a.on_msg(msg);
        assert!(!a.authenticated);
        assert_eq!(a.login.username, "admin");
        assert!(a.error.as_deref().unwrap_or("").contains("expired"));
    }

    #[test]
    fn ctrl_c_quits_from_anywhere() {
        let mut a = app();
        a.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(a.should_quit);
    }

    #[test]
    fn begin_login_moves_the_password_out() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _g = rt.enter();
        let mut a = app();
        a.login.username = "me".into();
        a.login.password = "secret".into();
        a.begin_login();
        assert!(a.login.password.is_empty());
        assert!(a.login.in_progress);
        assert!(!format!("{:?}", a.login).contains("secret"));
    }

    #[test]
    fn failed_sync_parts_keep_their_data() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _g = rt.enter();
        let mut a = signed_in(Role::Admin);
        a.devices = vec![device("d1", "Lamp")];
        let report = SyncReport {
            me: Err("x".into()),
            devices: Err("500: boom".into()),
            scenes: Ok(vec![]),
            areas: Ok(vec![]),
            rules: Ok(vec![]),
            events: Ok(vec![]),
            modes: Ok(vec![]),
            status: Err("x".into()),
            users: None,
            plugins: None,
        };
        a.apply_sync(report);
        assert_eq!(
            a.devices.len(),
            1,
            "failed devices call must not wipe the list"
        );
        assert!(a.sync_errors.iter().any(|(s, _)| s == "devices"));
        assert!(a.status.contains("devices"));
        assert!(!a.fresh.contains(&Section::Devices));
        assert!(a.fresh.contains(&Section::Rules));
    }
}

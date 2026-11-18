//! Plugins tab: the Installed list, the per-plugin detail screen
//! (overview, settings, actions, notices, logs) and the streaming-action
//! modal.
//!
//! Lifecycle, the Catalog, install and runtimes live in `plugin_manage`,
//! the Settings form and an action's parameters in `plugin_settings`, the
//! log stream in `plugin_logs`.

use super::editors::{Confirm, ConfirmAction};
use super::plugin_logs::PluginLogs;
use super::plugin_manage::PluginManage;
use super::plugin_settings::{ParamForm, SettingsPane};
use super::{App, ListSel, Load};
use crate::model::{Action, Capabilities, PluginRecord, RequiresRole};
use crate::net::sse::{StreamKind, StreamMsg};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{Value, json};
use tokio::task::JoinHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginDetailPanel {
    Overview,
    Settings,
    Actions,
    Notices,
    Logs,
}

impl PluginDetailPanel {
    pub const ALL: [PluginDetailPanel; 5] = [
        PluginDetailPanel::Overview,
        PluginDetailPanel::Settings,
        PluginDetailPanel::Actions,
        PluginDetailPanel::Notices,
        PluginDetailPanel::Logs,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Settings => "Settings",
            Self::Actions => "Actions",
            Self::Notices => "Notices",
            Self::Logs => "Logs",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|p| *p == self).unwrap_or(0)
    }

    fn step(self, forward: bool) -> Self {
        let n = Self::ALL.len();
        let i = self.index();
        Self::ALL[if forward {
            (i + 1) % n
        } else {
            (i + n - 1) % n
        }]
    }
}

/// The Plugins screen's tabs. Runtimes shows to those who can manage
/// plugins, Matter only when the Matter plugin is installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PluginsSub {
    #[default]
    Installed,
    Catalog,
    Runtimes,
    Matter,
}

impl PluginsSub {
    pub fn title(self) -> &'static str {
        match self {
            Self::Installed => "Installed",
            Self::Catalog => "Catalog",
            Self::Runtimes => "Runtimes",
            Self::Matter => "Matter",
        }
    }
}

#[derive(Debug)]
pub struct PluginDetail {
    pub plugin_id: String,
    pub panel: PluginDetailPanel,
    pub caps: Load<Capabilities>,
    /// The detail screen's own cursor over the actions list.
    pub action_sel: ListSel,
    pub action_busy: bool,
    pub action_status: String,
    pub settings: SettingsPane,
    pub logs: PluginLogs,
    /// First notice shown on the Notices tab.
    pub notices_scroll: usize,
}

impl PluginDetail {
    pub fn new(plugin_id: String) -> Self {
        Self {
            plugin_id,
            panel: PluginDetailPanel::Overview,
            caps: Load::Idle,
            action_sel: ListSel::default(),
            action_busy: false,
            action_status: String::new(),
            settings: SettingsPane::default(),
            logs: PluginLogs::default(),
            notices_scroll: 0,
        }
    }
}

#[derive(Debug, Default)]
pub struct PluginsTab {
    pub sub: PluginsSub,
    pub sel: ListSel,
    pub detail: Option<PluginDetail>,
    pub streaming: Option<StreamingAction>,
    /// The registry, runtimes and what is being installed.
    pub manage: PluginManage,
    /// An action's parameters, asked before it runs.
    pub params: Option<ParamForm>,
    stream_task: Option<JoinHandle<()>>,
    /// Bumped for every streaming run, so a late start reply from a run
    /// that was closed is not attached to the next one.
    run_seq: u64,
}

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
        matches!(
            self,
            Self::Complete | Self::Error | Self::Canceled | Self::Timeout
        )
    }
}

/// One streaming plugin-action run, shown in the modal.
#[derive(Debug, Clone)]
pub struct StreamingAction {
    pub plugin_id: String,
    pub label: String,
    /// Set once the start POST returns.
    pub request_id: Option<String>,
    pub stage: StreamingStage,
    pub last_progress: Option<Value>,
    pub items: Vec<Value>,
    pub warnings: Vec<Value>,
    pub pending_prompt: Option<Value>,
    pub terminal: Option<Value>,
    pub response_input: String,
    pub footer: String,
}

impl StreamingAction {
    pub fn new(plugin_id: String, label: String) -> Self {
        Self {
            plugin_id,
            label,
            request_id: None,
            stage: StreamingStage::Starting,
            last_progress: None,
            items: Vec::new(),
            warnings: Vec::new(),
            pending_prompt: None,
            terminal: None,
            response_input: String::new(),
            footer: "starting...".into(),
        }
    }

    pub fn fail(&mut self, error: impl Into<String>) {
        let error = error.into();
        self.stage = StreamingStage::Error;
        self.terminal = Some(json!({ "stage": "error", "message": error }));
        self.footer = "error; press Esc to close".into();
    }

    /// Apply one SSE payload.
    pub fn apply_event(&mut self, ev: Value) {
        let stage = ev
            .get("stage")
            .and_then(Value::as_str)
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
                self.footer = "awaiting your response; type then press Enter".into();
            }
            "complete" | "error" | "canceled" | "timeout" => {
                self.stage = match stage.as_str() {
                    "complete" => StreamingStage::Complete,
                    "error" => StreamingStage::Error,
                    "canceled" => StreamingStage::Canceled,
                    _ => StreamingStage::Timeout,
                };
                self.footer = format!("{}; press Esc to close", self.stage.label());
                self.terminal = Some(ev);
            }
            _ => {}
        }
    }
}

/// Progress percentage from a `progress` event (`percent`, or the older
/// `pct`).
pub fn progress_percent(ev: &Value) -> Option<f64> {
    ev.get("percent")
        .or_else(|| ev.get("pct"))
        .and_then(Value::as_f64)
        .map(|p| p.clamp(0.0, 100.0))
}

impl App {
    pub fn selected_plugin(&self) -> Option<&PluginRecord> {
        match self.plugins_tab.sel.id() {
            Some(id) => self.plugins.iter().find(|p| p.plugin_id == id),
            None => self.plugins.get(self.plugins_tab.sel.index()),
        }
    }

    pub fn detail_plugin(&self) -> Option<&PluginRecord> {
        let d = self.plugins_tab.detail.as_ref()?;
        self.plugins.iter().find(|p| p.plugin_id == d.plugin_id)
    }

    pub fn selected_action(&self) -> Option<&Action> {
        let d = self.plugins_tab.detail.as_ref()?;
        let actions = &d.caps.ready()?.actions;
        match d.action_sel.id() {
            Some(id) => actions.iter().find(|a| a.id == id),
            None => actions.get(d.action_sel.index()),
        }
    }

    pub(crate) fn resolve_plugin_detail_selection(&mut self) {
        if let Some(d) = self.plugins_tab.detail.as_mut() {
            let ids: Vec<String> = d
                .caps
                .ready()
                .map(|c| c.actions.iter().map(|a| a.id.clone()).collect())
                .unwrap_or_default();
            d.action_sel.resolve(&ids);
        }
    }

    /// Whether the Matter plugin is installed, which brings its tab.
    pub fn has_matter(&self) -> bool {
        // Core proxies Matter through the plugin's controller device.
        self.device_by_id("matter_controller").is_some()
            || self
                .plugins
                .iter()
                .any(|p| p.plugin_id.trim_start_matches("plugin.") == "matter")
    }

    pub fn move_plugin(&mut self, delta: isize) {
        let ids = self.plugin_view_ids();
        self.plugins_tab.sel.move_by(delta, &ids);
    }

    /// The tabs this person sees, in order.
    pub fn plugin_subs(&self) -> Vec<PluginsSub> {
        let mut v = vec![PluginsSub::Installed, PluginsSub::Catalog];
        if self.can("plugins:write") {
            v.push(PluginsSub::Runtimes);
        }
        if self.has_matter() {
            v.push(PluginsSub::Matter);
        }
        v
    }

    /// The tab on screen: the chosen one, or Installed when it is gone.
    pub fn plugin_sub(&self) -> PluginsSub {
        let sub = self.plugins_tab.sub;
        if self.plugin_subs().contains(&sub) {
            sub
        } else {
            PluginsSub::Installed
        }
    }

    /// ←/→ between the tabs.
    pub fn step_plugins_sub(&mut self, forward: bool) {
        let subs = self.plugin_subs();
        let n = subs.len();
        let i = subs
            .iter()
            .position(|s| *s == self.plugin_sub())
            .unwrap_or(0);
        let next = subs[if forward {
            (i + 1) % n
        } else {
            (i + n - 1) % n
        }];
        self.plugins_tab.sub = next;
        self.on_plugin_sub_shown();
    }

    /// Load what the tab on screen needs.
    pub fn on_plugin_sub_shown(&mut self) {
        match self.plugin_sub() {
            PluginsSub::Matter => self.refresh_matter_nodes(),
            PluginsSub::Runtimes => {
                if self.plugins_tab.manage.runtimes.ready().is_none() {
                    self.load_runtimes();
                }
            }
            PluginsSub::Installed | PluginsSub::Catalog => {
                if matches!(self.plugins_tab.manage.registry, Load::Idle) {
                    self.load_registry();
                }
            }
        }
    }

    pub fn open_plugin_detail(&mut self) {
        let Some(id) = self.selected_plugin().map(|p| p.plugin_id.clone()) else {
            return;
        };
        self.plugins_tab.detail = Some(PluginDetail::new(id));
    }

    fn set_detail_panel(&mut self, panel: PluginDetailPanel) {
        let Some(d) = self.plugins_tab.detail.as_mut() else {
            return;
        };
        if d.panel == PluginDetailPanel::Settings && panel != PluginDetailPanel::Settings {
            // Leaving the form keeps its changes; an open input is closed.
            d.settings.input = None;
        }
        d.panel = panel;
        match panel {
            PluginDetailPanel::Actions if !d.caps.is_loading() => self.load_capabilities(),
            PluginDetailPanel::Settings if matches!(d.settings.form, Load::Idle) => {
                self.load_plugin_settings()
            }
            _ => {}
        }
    }

    pub fn close_plugin_detail(&mut self) {
        self.plugins_tab.detail = None;
    }

    /// The plugin the lifecycle keys act on: the open one, else the
    /// selected row.
    pub fn target_plugin(&self) -> Option<&PluginRecord> {
        if self.plugins_tab.detail.is_some() {
            self.detail_plugin()
        } else {
            self.selected_plugin()
        }
    }

    /// `j`/`k` on the Notices tab.
    pub fn scroll_notices(&mut self, delta: isize) {
        let n = self.detail_plugin().map(|p| p.notices.len()).unwrap_or(0);
        if let Some(d) = self.plugins_tab.detail.as_mut() {
            d.notices_scroll =
                (d.notices_scroll as isize + delta).clamp(0, n.saturating_sub(1) as isize) as usize;
        }
    }

    /// ←/→ in the detail view.
    pub fn step_detail_panel(&mut self, forward: bool) {
        if let Some(panel) = self.plugins_tab.detail.as_ref().map(|d| d.panel) {
            self.set_detail_panel(panel.step(forward));
        }
    }

    pub fn refresh_plugin_detail(&mut self) {
        self.start_sync();
        let Some(d) = self.plugins_tab.detail.as_ref() else {
            return;
        };
        match d.panel {
            PluginDetailPanel::Actions => self.load_capabilities(),
            // Reloading would drop changes not yet saved.
            PluginDetailPanel::Settings => {
                if d.settings.form.ready().is_some_and(|f| f.dirty()) {
                    self.status = "Save or discard your changes first".into();
                } else {
                    self.load_plugin_settings();
                }
            }
            _ => {}
        }
    }

    pub fn move_plugin_action(&mut self, delta: isize) {
        let Some(d) = self.plugins_tab.detail.as_mut() else {
            return;
        };
        let ids: Vec<String> = d
            .caps
            .ready()
            .map(|c| c.actions.iter().map(|a| a.id.clone()).collect())
            .unwrap_or_default();
        d.action_sel.move_by(delta, &ids);
    }

    fn load_capabilities(&mut self) {
        let Some(d) = self.plugins_tab.detail.as_mut() else {
            return;
        };
        d.caps = Load::Loading;
        let id = d.plugin_id.clone();
        let c = self.client.clone();
        let want = id.clone();
        self.spawn(
            async move { c.get_plugin_capabilities(&want).await },
            move |app, res| {
                if let Some(d) = app.plugins_tab.detail.as_mut()
                    && d.plugin_id == id
                {
                    d.caps = match res {
                        Ok(caps) => {
                            app.status =
                                format!("Capabilities loaded ({} actions)", caps.actions.len());
                            Load::Ready(caps)
                        }
                        Err(e) => Load::Failed(e.to_string()),
                    };
                }
            },
        );
    }

    fn set_action_status(&mut self, s: String) {
        if let Some(d) = self.plugins_tab.detail.as_mut() {
            d.action_status = s;
        }
    }

    /// Enter on an action: check the role, ask first when it is marked
    /// `danger`, then run it.
    pub fn run_selected_action(&mut self) {
        let Some(d) = self.plugins_tab.detail.as_ref() else {
            return;
        };
        if d.action_busy {
            return;
        }
        let plugin_id = d.plugin_id.clone();
        let Some(action) = self.selected_action().cloned() else {
            return;
        };
        if action.requires_role == RequiresRole::Admin && !self.is_admin() {
            let role = self
                .current_user
                .as_ref()
                .map(|u| u.role.as_str())
                .unwrap_or("unknown");
            self.set_action_status(format!(
                "Action `{}` requires admin role; you are `{role}`.",
                action.id
            ));
            return;
        }
        // An action that takes parameters asks for them first; its form
        // runs it (asking first when it is dangerous).
        if let Some(form) = ParamForm::for_action(&plugin_id, &action) {
            self.plugins_tab.params = Some(form);
            return;
        }
        if action.danger {
            self.ask(danger_confirm(plugin_id, action, json!({})));
            return;
        }
        self.run_plugin_action(plugin_id, action);
    }

    pub(crate) fn run_plugin_action(&mut self, plugin_id: String, action: Action) {
        self.run_plugin_action_with(plugin_id, action, json!({}));
    }

    pub(crate) fn run_plugin_action_with(
        &mut self,
        plugin_id: String,
        action: Action,
        params: Value,
    ) {
        if action.stream {
            self.start_streaming_action(plugin_id, action, params);
            return;
        }
        if let Some(d) = self.plugins_tab.detail.as_mut() {
            d.action_busy = true;
            d.action_status = format!("Running `{}`...", action.id);
        }
        let c = self.client.clone();
        let id = action.id.clone();
        self.spawn(
            async move { c.post_plugin_command(&plugin_id, &action.id, params).await },
            move |app, res| {
                let msg = match res {
                    // A reply that says it failed comes back as success.
                    Ok(resp) if resp.get("status").and_then(Value::as_str) == Some("error") => {
                        let why = resp
                            .get("error")
                            .and_then(Value::as_str)
                            .unwrap_or("the plugin said no");
                        format!("`{id}` failed: {why}")
                    }
                    Ok(resp) => format!("`{id}` ok: {}", summarize_response(&resp)),
                    Err(e) => format!("`{id}` failed: {e}"),
                };
                if let Some(d) = app.plugins_tab.detail.as_mut() {
                    d.action_busy = false;
                    d.action_status = msg;
                }
            },
        );
    }

    fn start_streaming_action(&mut self, plugin_id: String, action: Action, params: Value) {
        let label = action.display_label().to_string();
        self.close_streaming_action();
        self.plugins_tab.streaming = Some(StreamingAction::new(plugin_id.clone(), label));
        self.plugins_tab.run_seq += 1;
        let run = self.plugins_tab.run_seq;
        let c = self.client.clone();
        let pid = plugin_id.clone();
        self.spawn(
            async move { c.post_plugin_command(&pid, &action.id, params).await },
            move |app, res| {
                let Some(s) = app.plugins_tab.streaming.as_mut() else {
                    return; // closed while starting
                };
                if app.plugins_tab.run_seq != run || s.request_id.is_some() {
                    return;
                }
                let resp = match res {
                    Ok(v) => v,
                    Err(e) => return s.fail(format!("start failed: {e}")),
                };
                if resp.get("status").and_then(Value::as_str) == Some("busy") {
                    let active = resp
                        .get("active_request_id")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    return s.fail(format!("another run is in flight (request_id={active})"));
                }
                let Some(rid) = resp.get("request_id").and_then(Value::as_str) else {
                    return s.fail("plugin did not return a request_id");
                };
                s.request_id = Some(rid.to_string());
                s.footer = "connecting to event stream...".into();
                let task = crate::net::sse::spawn_streaming_action(
                    app.client.clone(),
                    plugin_id,
                    rid.to_string(),
                    app.sender(),
                );
                app.plugins_tab.stream_task = Some(task);
            },
        );
    }

    /// Close the modal and stop reading its stream. The action itself
    /// keeps running server side unless it was cancelled.
    pub fn close_streaming_action(&mut self) {
        if let Some(t) = self.plugins_tab.stream_task.take() {
            t.abort();
        }
        self.plugins_tab.streaming = None;
    }

    pub(crate) fn on_stream_msg(&mut self, msg: StreamMsg) {
        let Some(s) = self.plugins_tab.streaming.as_mut() else {
            return;
        };
        if s.request_id.as_deref() != Some(msg.request_id.as_str()) {
            return; // from an earlier run
        }
        match msg.kind {
            StreamKind::Connected => s.footer = "streaming...".into(),
            StreamKind::Event(ev) => s.apply_event(ev),
            StreamKind::Closed => {
                if !s.stage.is_terminal() {
                    s.footer = "stream closed before the run finished; press Esc".into();
                }
            }
            StreamKind::Error(reason) => {
                if !s.stage.is_terminal() {
                    s.fail(format!("stream error: {reason}"));
                }
            }
        }
    }

    pub(crate) fn on_key_streaming(&mut self, key: KeyEvent) {
        let Some(s) = self.plugins_tab.streaming.as_mut() else {
            return;
        };
        if key.code == KeyCode::Esc {
            self.close_streaming_action();
            return;
        }
        if s.stage.is_terminal() {
            if key.code == KeyCode::Char('q') {
                self.close_streaming_action();
            }
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if s.pending_prompt.is_some() {
            match key.code {
                KeyCode::Enter => self.respond_streaming(),
                KeyCode::Backspace => {
                    s.response_input.pop();
                }
                KeyCode::Char('x') if ctrl => self.cancel_streaming(),
                KeyCode::Char(ch) if !ctrl => s.response_input.push(ch),
                _ => {}
            }
            return;
        }
        if matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C')) {
            self.cancel_streaming();
        }
    }

    fn cancel_streaming(&mut self) {
        let Some(s) = self.plugins_tab.streaming.as_mut() else {
            return;
        };
        let Some(rid) = s.request_id.clone() else {
            return;
        };
        s.footer = "cancel sent; waiting for the run to stop...".into();
        let pid = s.plugin_id.clone();
        let c = self.client.clone();
        self.spawn(
            async move { c.cancel_streaming_action(&pid, &rid).await },
            |app, res| {
                if let (Err(e), Some(s)) = (res, app.plugins_tab.streaming.as_mut()) {
                    s.footer = format!("cancel failed: {e}");
                }
            },
        );
    }

    fn respond_streaming(&mut self) {
        let Some(s) = self.plugins_tab.streaming.as_mut() else {
            return;
        };
        let Some(rid) = s.request_id.clone() else {
            return;
        };
        let raw = std::mem::take(&mut s.response_input);
        let response = serde_json::from_str::<Value>(&raw).unwrap_or(Value::String(raw));
        s.pending_prompt = None;
        s.footer = "response sent; waiting for the plugin...".into();
        let pid = s.plugin_id.clone();
        let c = self.client.clone();
        self.spawn(
            async move { c.respond_streaming_action(&pid, &rid, response).await },
            |app, res| {
                if let (Err(e), Some(s)) = (res, app.plugins_tab.streaming.as_mut()) {
                    s.footer = format!("respond failed: {e}");
                }
            },
        );
    }

    pub fn ask_uninstall_plugin(&mut self) {
        if !self.is_admin() {
            self.error = Some("uninstalling a plugin needs the admin role".to_string());
            return;
        }
        let Some(p) = self.target_plugin() else {
            return;
        };
        let plugin_id = p.plugin_id.clone();
        let n = p.device_count;
        self.ask(
            Confirm::new(
                "Uninstall plugin?",
                format!("Uninstall {plugin_id}?"),
                ConfirmAction::UninstallPlugin { plugin_id },
            )
            .with_detail(format!(
                "Stops it and removes its registration and its {n} device(s)."
            )),
        );
    }

    pub(crate) fn uninstall_plugin(&mut self, plugin_id: String) {
        let c = self.client.clone();
        let target = plugin_id.clone();
        self.spawn_ok(
            "uninstall plugin",
            async move { c.uninstall_plugin(&target).await },
            move |app, summary: Value| {
                let removed = summary
                    .get("devices_removed")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                app.plugins.retain(|p| p.plugin_id != plugin_id);
                if app
                    .plugins_tab
                    .detail
                    .as_ref()
                    .is_some_and(|d| d.plugin_id == plugin_id)
                {
                    app.plugins_tab.detail = None;
                }
                app.status = format!("Uninstalled {plugin_id} ({removed} device(s) removed)");
                app.start_sync();
            },
        );
    }
}

/// The question before a dangerous action runs.
pub(crate) fn danger_confirm(plugin_id: String, action: Action, params: Value) -> Confirm {
    let label = action.display_label().to_string();
    let detail = action.description.clone().unwrap_or_default();
    Confirm::new(
        "Run action?",
        format!("Run \"{label}\" on {plugin_id}?"),
        ConfirmAction::Plugins(super::plugin_manage::PluginConfirm::RunAction {
            plugin_id,
            action: Box::new(action),
            params,
        }),
    )
    .with_detail(detail)
}

/// Short summary of a plugin command reply for the panel footer.
fn summarize_response(resp: &Value) -> String {
    match resp {
        Value::Object(map) => {
            if let Some(s) = map.get("status").and_then(Value::as_str) {
                format!("status={s}")
            } else if let Some(r) = map.get("request_id").and_then(Value::as_str) {
                format!("request_id={r}")
            } else {
                let keys: Vec<&str> = map.keys().map(String::as_str).collect();
                format!("keys=[{}]", keys.join(","))
            }
        }
        other => crate::text::truncate(&other.to_string(), 80),
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::model::Role;

    #[test]
    fn stream_messages_for_other_runs_are_ignored() {
        let mut a = signed_in(Role::Admin);
        let mut s = StreamingAction::new("p".into(), "Scan".into());
        s.request_id = Some("r2".into());
        a.plugins_tab.streaming = Some(s);
        a.on_stream_msg(StreamMsg {
            request_id: "r1".into(),
            kind: StreamKind::Event(json!({"stage": "complete"})),
        });
        assert_eq!(
            a.plugins_tab.streaming.as_ref().unwrap().stage,
            StreamingStage::Starting
        );
        a.on_stream_msg(StreamMsg {
            request_id: "r2".into(),
            kind: StreamKind::Event(json!({"stage": "complete"})),
        });
        assert_eq!(
            a.plugins_tab.streaming.as_ref().unwrap().stage,
            StreamingStage::Complete
        );
    }

    #[test]
    fn danger_actions_ask_first() {
        let mut a = signed_in(Role::Admin);
        let action: Action = serde_json::from_value(json!({
            "id": "factory_reset", "label": "Factory reset", "danger": true
        }))
        .unwrap();
        let mut d = PluginDetail::new("plugin.x".into());
        d.panel = PluginDetailPanel::Actions;
        d.caps = Load::Ready(Capabilities {
            spec: "1".into(),
            plugin_id: "plugin.x".into(),
            actions: vec![action],
        });
        a.plugins_tab.detail = Some(d);
        a.refresh_views();
        a.run_selected_action();
        assert!(matches!(
            a.confirm.as_ref().map(|c| &c.action),
            Some(ConfirmAction::Plugins(
                super::super::plugin_manage::PluginConfirm::RunAction { .. }
            ))
        ));
    }

    #[test]
    fn progress_reads_percent() {
        assert_eq!(progress_percent(&json!({"percent": 40})), Some(40.0));
        assert_eq!(progress_percent(&json!({"pct": 140})), Some(100.0));
        assert_eq!(progress_percent(&json!({})), None);
    }
}

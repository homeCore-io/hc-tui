//! Plugins tab: the list, the per-plugin detail screen (overview,
//! actions, diagnostics) and the streaming-action modal.

use super::editors::{Confirm, ConfirmAction};
use super::{App, ListSel, Load};
use crate::model::{Action, Capabilities, PluginRecord, RequiresRole};
use crate::net::sse::{StreamKind, StreamMsg};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{Value, json};
use tokio::task::JoinHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginDetailPanel {
    Overview,
    Actions,
    Diagnostics,
}

impl PluginDetailPanel {
    pub const ALL: [PluginDetailPanel; 3] = [
        PluginDetailPanel::Overview,
        PluginDetailPanel::Actions,
        PluginDetailPanel::Diagnostics,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Actions => "Actions",
            Self::Diagnostics => "Diagnostics",
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

#[derive(Debug)]
pub struct PluginDetail {
    pub plugin_id: String,
    pub panel: PluginDetailPanel,
    pub caps: Load<Capabilities>,
    /// The detail screen's own cursor over the actions list.
    pub action_sel: ListSel,
    pub action_busy: bool,
    pub action_status: String,
}

#[derive(Debug, Default)]
pub struct PluginsTab {
    pub sel: ListSel,
    pub detail: Option<PluginDetail>,
    pub streaming: Option<StreamingAction>,
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

    pub(super) fn resolve_plugin_detail_selection(&mut self) {
        if let Some(d) = self.plugins_tab.detail.as_mut() {
            let ids: Vec<String> = d
                .caps
                .ready()
                .map(|c| c.actions.iter().map(|a| a.id.clone()).collect())
                .unwrap_or_default();
            d.action_sel.resolve(&ids);
        }
    }

    pub(super) fn on_key_plugins(&mut self, key: KeyEvent) {
        let ids: Vec<String> = self.plugins.iter().map(|p| p.plugin_id.clone()).collect();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.plugins_tab.sel.move_by(1, &ids),
            KeyCode::Up | KeyCode::Char('k') => self.plugins_tab.sel.move_by(-1, &ids),
            KeyCode::Enter => self.open_plugin_detail(),
            KeyCode::Char('d') => self.ask_uninstall_plugin(),
            _ => {}
        }
    }

    fn open_plugin_detail(&mut self) {
        let Some(id) = self.selected_plugin().map(|p| p.plugin_id.clone()) else {
            return;
        };
        self.status = format!("Opened plugin detail: {id}");
        self.plugins_tab.detail = Some(PluginDetail {
            plugin_id: id,
            panel: PluginDetailPanel::Overview,
            caps: Load::Idle,
            action_sel: ListSel::default(),
            action_busy: false,
            action_status: String::new(),
        });
    }

    fn set_detail_panel(&mut self, panel: PluginDetailPanel) {
        let Some(d) = self.plugins_tab.detail.as_mut() else {
            return;
        };
        d.panel = panel;
        if panel == PluginDetailPanel::Actions && !d.caps.is_loading() {
            self.load_capabilities();
        }
    }

    pub(super) fn on_key_plugin_detail(&mut self, key: KeyEvent) {
        let Some(d) = self.plugins_tab.detail.as_mut() else {
            return;
        };
        let panel = d.panel;
        let ids: Vec<String> = d
            .caps
            .ready()
            .map(|c| c.actions.iter().map(|a| a.id.clone()).collect())
            .unwrap_or_default();
        match key.code {
            KeyCode::Esc => {
                self.plugins_tab.detail = None;
                self.status = "Closed plugin detail".to_string();
            }
            KeyCode::Char('1') => self.set_detail_panel(PluginDetailPanel::Overview),
            KeyCode::Char('2') => self.set_detail_panel(PluginDetailPanel::Actions),
            KeyCode::Char('3') => self.set_detail_panel(PluginDetailPanel::Diagnostics),
            KeyCode::Left | KeyCode::BackTab => self.set_detail_panel(panel.step(false)),
            KeyCode::Right | KeyCode::Tab => self.set_detail_panel(panel.step(true)),
            KeyCode::Char('r') => {
                self.start_sync();
                if panel == PluginDetailPanel::Actions {
                    self.load_capabilities();
                }
            }
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Up | KeyCode::Char('k') if panel == PluginDetailPanel::Actions => {
                d.action_sel.move_by(-1, &ids)
            }
            KeyCode::Down | KeyCode::Char('j') if panel == PluginDetailPanel::Actions => {
                d.action_sel.move_by(1, &ids)
            }
            KeyCode::Enter if panel == PluginDetailPanel::Actions => self.run_selected_action(),
            _ => {}
        }
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
    fn run_selected_action(&mut self) {
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
        if action.danger {
            let label = action.display_label().to_string();
            let detail = action.description.clone().unwrap_or_default();
            self.ask(
                Confirm::new(
                    "Run action",
                    format!("Run \"{label}\" on {plugin_id}?"),
                    ConfirmAction::RunPluginAction {
                        plugin_id,
                        action: Box::new(action),
                    },
                )
                .with_detail(detail),
            );
            return;
        }
        self.run_plugin_action(plugin_id, action);
    }

    pub(super) fn run_plugin_action(&mut self, plugin_id: String, action: Action) {
        if action.stream {
            self.start_streaming_action(plugin_id, action);
            return;
        }
        if let Some(d) = self.plugins_tab.detail.as_mut() {
            d.action_busy = true;
            d.action_status = format!("Running `{}`...", action.id);
        }
        let c = self.client.clone();
        let id = action.id.clone();
        self.spawn(
            async move {
                c.post_plugin_command(&plugin_id, &action.id, json!({}))
                    .await
            },
            move |app, res| {
                let msg = match res {
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

    fn start_streaming_action(&mut self, plugin_id: String, action: Action) {
        let label = action.display_label().to_string();
        self.close_streaming_action();
        self.plugins_tab.streaming = Some(StreamingAction::new(plugin_id.clone(), label));
        self.plugins_tab.run_seq += 1;
        let run = self.plugins_tab.run_seq;
        let c = self.client.clone();
        let pid = plugin_id.clone();
        self.spawn(
            async move { c.post_plugin_command(&pid, &action.id, json!({})).await },
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

    pub(super) fn on_stream_msg(&mut self, msg: StreamMsg) {
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

    pub(super) fn on_key_streaming(&mut self, key: KeyEvent) {
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

    fn ask_uninstall_plugin(&mut self) {
        if !self.is_admin() {
            self.error = Some("uninstalling a plugin needs the admin role".to_string());
            return;
        }
        let Some(p) = self.selected_plugin() else {
            return;
        };
        let plugin_id = p.plugin_id.clone();
        let n = p.device_count;
        self.ask(
            Confirm::new(
                "Uninstall plugin",
                format!("Uninstall {plugin_id}?"),
                ConfirmAction::UninstallPlugin { plugin_id },
            )
            .with_detail(format!(
                "Stops it and removes its registration and its {n} device(s)."
            )),
        );
    }

    pub(super) fn uninstall_plugin(&mut self, plugin_id: String) {
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
                app.status = format!("Uninstalled {plugin_id} ({removed} device(s) removed)");
                app.start_sync();
            },
        );
    }
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
        a.plugins_tab.detail = Some(PluginDetail {
            plugin_id: "plugin.x".into(),
            panel: PluginDetailPanel::Actions,
            caps: Load::Ready(Capabilities {
                spec: "1".into(),
                plugin_id: "plugin.x".into(),
                actions: vec![action],
            }),
            action_sel: ListSel::default(),
            action_busy: false,
            action_status: String::new(),
        });
        a.refresh_views();
        a.run_selected_action();
        assert!(matches!(
            a.confirm.as_ref().map(|c| &c.action),
            Some(ConfirmAction::RunPluginAction { .. })
        ));
    }

    #[test]
    fn progress_reads_percent() {
        assert_eq!(progress_percent(&json!({"percent": 40})), Some(40.0));
        assert_eq!(progress_percent(&json!({"pct": 140})), Some(100.0));
        assert_eq!(progress_percent(&json!({})), None);
    }
}

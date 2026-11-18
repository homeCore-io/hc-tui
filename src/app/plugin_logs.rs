//! Plugins › a plugin's Logs tab: core's log stream for that plugin
//! (`/logs/stream?plugin_id=`), open only while the tab is on screen.
//!
//! The stream task sends into its own channel; a forwarder hands each
//! line to the app tagged with the run it belongs to, so a line from a
//! stream already closed is dropped.

use super::manage::logs::LogLevelFilter;
use super::plugins::PluginDetailPanel;
use super::{App, AsyncMsg};
use crate::model::LogLine;
use crate::net::ws::{LogMsg, LogParams};
use crate::screens::ScreenId;
use std::collections::VecDeque;
use tokio::task::JoinHandle;

const MAX_LINES: usize = 500;

#[derive(Debug, Default)]
pub struct PluginLogs {
    pub lines: VecDeque<LogLine>,
    pub level: LogLevelFilter,
    pub paused: bool,
    pub scroll: usize,
    pub connected: bool,
    pub last_error: Option<String>,
    task: Option<LogTask>,
}

#[derive(Debug)]
struct LogTask {
    stream: JoinHandle<()>,
    forward: JoinHandle<()>,
    params: LogParams,
    run: u64,
}

impl Drop for LogTask {
    fn drop(&mut self) {
        self.stream.abort();
        self.forward.abort();
    }
}

impl PluginLogs {
    #[cfg(test)]
    pub fn is_streaming(&self) -> bool {
        self.task.is_some()
    }
}

/// Runs are numbered across every plugin's Logs tab.
static RUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl App {
    fn wants_plugin_logs(&self) -> Option<LogParams> {
        if !self.authenticated || !self.is_admin() || self.screen != ScreenId::Plugins {
            return None;
        }
        let d = self.plugins_tab.detail.as_ref()?;
        (d.panel == PluginDetailPanel::Logs).then(|| LogParams {
            level: d.logs.level.param().to_string(),
            target: String::new(),
            plugin_id: d.plugin_id.clone(),
        })
    }

    /// Open, reopen or close the plugin's log stream to match the screen.
    /// Closing the detail drops its stream with it.
    pub(crate) fn update_plugin_log_stream(&mut self) {
        let want = self.wants_plugin_logs();
        let Some(d) = self.plugins_tab.detail.as_mut() else {
            return;
        };
        let Some(params) = want else {
            if d.logs.task.take().is_some() {
                d.logs.connected = false;
            }
            return;
        };
        if d.logs.task.as_ref().is_some_and(|t| t.params == params) {
            return;
        }
        d.logs.task = None;
        // The server replays recent history on connect; start clean.
        d.logs.lines.clear();
        d.logs.scroll = 0;
        let run = RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<LogMsg>();
        let stream = crate::net::ws::spawn_log_stream(self.client.clone(), params.clone(), tx);
        let out = self.sender();
        let forward = tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let apply: super::ApplyFn =
                    Box::new(move |app: &mut App| app.on_plugin_log(run, msg));
                if out.send(AsyncMsg::Apply(apply)).is_err() {
                    return;
                }
            }
        });
        let d = self.plugins_tab.detail.as_mut().expect("checked above");
        d.logs.task = Some(LogTask {
            stream,
            forward,
            params,
            run,
        });
    }

    fn on_plugin_log(&mut self, run: u64, msg: LogMsg) {
        let Some(d) = self.plugins_tab.detail.as_mut() else {
            return;
        };
        let logs = &mut d.logs;
        if logs.task.as_ref().map(|t| t.run) != Some(run) {
            return; // from a stream already closed
        }
        match msg {
            LogMsg::Connected => {
                logs.connected = true;
                logs.last_error = None;
            }
            LogMsg::Disconnected(reason) => {
                logs.connected = false;
                logs.last_error = Some(reason);
            }
            LogMsg::Line(line) => {
                if !logs.level.passes(&line.level) {
                    return;
                }
                logs.lines.push_back(line);
                if logs.lines.len() > MAX_LINES {
                    logs.lines.pop_front();
                }
                if !logs.paused {
                    logs.scroll = logs.lines.len().saturating_sub(1);
                }
            }
        }
    }

    fn plugin_logs(&mut self) -> Option<&mut PluginLogs> {
        self.plugins_tab.detail.as_mut().map(|d| &mut d.logs)
    }

    pub fn plugin_logs_pause(&mut self) {
        let Some(l) = self.plugin_logs() else {
            return;
        };
        l.paused = !l.paused;
        if !l.paused {
            l.scroll = l.lines.len().saturating_sub(1);
        }
        let paused = l.paused;
        self.status = if paused { "Log paused" } else { "Log resumed" }.into();
    }

    pub fn plugin_logs_scroll(&mut self, delta: isize) {
        let Some(l) = self.plugin_logs() else {
            return;
        };
        if !l.paused {
            l.paused = true;
        }
        let max = l.lines.len().saturating_sub(1) as isize;
        l.scroll = (l.scroll as isize + delta).clamp(0, max) as usize;
    }

    pub fn plugin_logs_level(&mut self, level: LogLevelFilter) {
        if let Some(l) = self.plugin_logs() {
            l.level = level;
        }
        self.status = format!("Log level: {}", level.as_str());
        // The next frame's update_plugin_log_stream reconnects.
    }

    pub fn plugin_logs_clear(&mut self) {
        if let Some(l) = self.plugin_logs() {
            l.lines.clear();
            l.scroll = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::app::plugins::PluginDetail;
    use crate::model::Role;

    #[tokio::test]
    async fn the_stream_is_open_only_on_the_logs_tab() {
        let mut a = signed_in(Role::Admin);
        a.screen = ScreenId::Plugins;
        a.plugins_tab.detail = Some(PluginDetail::new("plugin.x".into()));
        a.update_plugin_log_stream();
        let logs = |a: &App| a.plugins_tab.detail.as_ref().unwrap().logs.is_streaming();
        assert!(!logs(&a), "Overview has no stream");
        a.plugins_tab.detail.as_mut().unwrap().panel = PluginDetailPanel::Logs;
        a.update_plugin_log_stream();
        assert!(logs(&a));
        let params = |a: &App| {
            a.plugins_tab
                .detail
                .as_ref()
                .unwrap()
                .logs
                .task
                .as_ref()
                .unwrap()
                .params
                .clone()
        };
        assert_eq!(params(&a).plugin_id, "plugin.x");
        a.plugin_logs_level(LogLevelFilter::Debug);
        a.update_plugin_log_stream();
        assert_eq!(params(&a).level, "debug", "a new level reconnects");
        a.screen = ScreenId::Devices;
        a.update_plugin_log_stream();
        assert!(!logs(&a), "leaving the screen closes it");
    }

    #[tokio::test]
    async fn lines_from_a_closed_stream_are_dropped() {
        let mut a = signed_in(Role::Admin);
        a.screen = ScreenId::Plugins;
        let mut d = PluginDetail::new("plugin.x".into());
        d.panel = PluginDetailPanel::Logs;
        a.plugins_tab.detail = Some(d);
        a.update_plugin_log_stream();
        let run = a
            .plugins_tab
            .detail
            .as_ref()
            .unwrap()
            .logs
            .task
            .as_ref()
            .unwrap()
            .run;
        let line = |m: &str| LogLine {
            timestamp: String::new(),
            level: "INFO".into(),
            target: "t".into(),
            message: m.into(),
            fields: serde_json::Value::Null,
        };
        a.on_plugin_log(run, LogMsg::Line(line("now")));
        a.on_plugin_log(run + 1000, LogMsg::Line(line("old")));
        let lines = &a.plugins_tab.detail.as_ref().unwrap().logs.lines;
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].message, "now");
    }

    #[test]
    fn a_non_admin_gets_no_stream() {
        let mut a = signed_in(Role::Observer);
        a.screen = ScreenId::Plugins;
        let mut d = PluginDetail::new("plugin.x".into());
        d.panel = PluginDetailPanel::Logs;
        a.plugins_tab.detail = Some(d);
        a.update_plugin_log_stream();
        assert!(!a.plugins_tab.detail.as_ref().unwrap().logs.is_streaming());
    }
}

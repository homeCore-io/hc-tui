//! Manage › Logs: the live log stream.
//!
//! The WebSocket is open only while this panel is on screen; leaving it
//! closes the stream, and changing the level reconnects with the new
//! level.

use crate::app::App;
use crate::app::editors::edit_text;
use crate::model::LogLine;
use crate::net::ws::{LogMsg, LogParams};
use crossterm::event::{KeyCode, KeyEvent};
use std::collections::VecDeque;
use tokio::task::JoinHandle;

const MAX_LINES: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogLevelFilter {
    Error,
    Warn,
    #[default]
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

    /// The `level` query parameter core expects.
    pub fn param(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
        }
    }

    pub fn passes(&self, level: &str) -> bool {
        let l = level.to_uppercase();
        match self {
            Self::Error => l == "ERROR",
            Self::Warn => matches!(l.as_str(), "ERROR" | "WARN"),
            Self::Info => matches!(l.as_str(), "ERROR" | "WARN" | "INFO"),
            Self::Debug => true,
        }
    }
}

#[derive(Debug, Default)]
pub struct LogsPanel {
    pub lines: VecDeque<LogLine>,
    pub level: LogLevelFilter,
    /// Local substring filter on target and message.
    pub module_filter: String,
    pub paused: bool,
    pub scroll: usize,
    pub connected: bool,
    /// Why the stream last dropped, shown while it is down.
    pub last_error: Option<String>,
    pub input_open: bool,
    pub input: String,
    task: Option<(JoinHandle<()>, LogParams)>,
}

impl LogsPanel {
    #[cfg(test)]
    pub fn is_streaming(&self) -> bool {
        self.task.is_some()
    }
}

impl App {
    fn wants_log_stream(&self) -> bool {
        self.authenticated && self.is_admin() && self.screen == crate::screens::ScreenId::Logs
    }

    /// Open, reopen or close the log stream to match what is on screen.
    pub(crate) fn update_log_stream(&mut self) {
        if !self.wants_log_stream() {
            self.stop_log_stream();
            return;
        }
        let params = LogParams {
            level: self.manage.logs.level.param().to_string(),
            target: String::new(),
            plugin_id: String::new(),
        };
        if let Some((_, current)) = &self.manage.logs.task
            && *current == params
        {
            return;
        }
        self.stop_log_stream();
        // The server replays recent history on connect; start clean.
        self.manage.logs.lines.clear();
        self.manage.logs.scroll = 0;
        let task =
            crate::net::ws::spawn_log_stream(self.client.clone(), params.clone(), self.sender());
        self.manage.logs.task = Some((task, params));
    }

    pub(crate) fn stop_log_stream(&mut self) {
        if let Some((t, _)) = self.manage.logs.task.take() {
            t.abort();
        }
        self.manage.logs.connected = false;
    }

    pub(crate) fn on_log_msg(&mut self, msg: LogMsg) {
        if self.manage.logs.task.is_none() {
            return; // from a stream we already closed
        }
        let logs = &mut self.manage.logs;
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
                let f = &logs.module_filter;
                if !f.is_empty()
                    && !line.target.contains(f.as_str())
                    && !line.message.contains(f.as_str())
                {
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

    fn set_log_level(&mut self, level: LogLevelFilter) {
        self.manage.logs.level = level;
        self.status = format!("Log level: {}", level.as_str());
        // The next frame's update_log_stream reconnects with the new level.
    }

    pub(crate) fn on_key_logs(&mut self, key: KeyEvent) {
        let logs = &mut self.manage.logs;
        match key.code {
            KeyCode::Char('p') | KeyCode::Char(' ') => {
                logs.paused = !logs.paused;
                if !logs.paused {
                    logs.scroll = logs.lines.len().saturating_sub(1);
                }
                self.status = if logs.paused {
                    "Log stream paused".to_string()
                } else {
                    "Log stream resumed".to_string()
                };
            }
            KeyCode::Down | KeyCode::Char('j') if logs.paused => {
                logs.scroll = (logs.scroll + 1).min(logs.lines.len().saturating_sub(1));
            }
            KeyCode::Up | KeyCode::Char('k') if logs.paused => {
                logs.scroll = logs.scroll.saturating_sub(1);
            }
            KeyCode::Char('e') => self.set_log_level(LogLevelFilter::Error),
            KeyCode::Char('w') => self.set_log_level(LogLevelFilter::Warn),
            KeyCode::Char('i') => self.set_log_level(LogLevelFilter::Info),
            KeyCode::Char('d') => self.set_log_level(LogLevelFilter::Debug),
            KeyCode::Char('c') => {
                logs.lines.clear();
                logs.scroll = 0;
                self.status = "Log buffer cleared".to_string();
            }
            KeyCode::Char('/') => {
                logs.input_open = true;
                logs.input = logs.module_filter.clone();
            }
            _ => {}
        }
    }

    pub(crate) fn on_key_log_filter_input(&mut self, key: KeyEvent) {
        let logs = &mut self.manage.logs;
        match key.code {
            KeyCode::Esc => logs.input_open = false,
            KeyCode::Enter => {
                logs.module_filter = logs.input.trim().to_string();
                logs.input_open = false;
                self.status = if logs.module_filter.is_empty() {
                    "Log filter cleared".to_string()
                } else {
                    format!("Log filter: {}", logs.module_filter)
                };
            }
            _ => {
                edit_text(&mut logs.input, key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::*;
    use crate::model::Role;

    #[test]
    fn level_filters() {
        assert!(LogLevelFilter::Error.passes("ERROR"));
        assert!(!LogLevelFilter::Error.passes("WARN"));
        assert!(LogLevelFilter::Info.passes("warn"));
        assert!(!LogLevelFilter::Info.passes("DEBUG"));
        assert_eq!(LogLevelFilter::Warn.param(), "warn");
    }

    #[tokio::test]
    async fn stream_opens_only_on_the_logs_panel() {
        let mut a = signed_in(Role::Admin);
        a.update_log_stream();
        assert!(!a.manage.logs.is_streaming());
        a.screen = crate::screens::ScreenId::Logs;
        a.update_log_stream();
        assert!(a.manage.logs.is_streaming());
        // Changing level reconnects with the new parameter.
        a.on_key(key(KeyCode::Char('w')));
        a.update_log_stream();
        assert_eq!(
            a.manage.logs.task.as_ref().map(|(_, p)| p.level.as_str()),
            Some("warn")
        );
        // Leaving the panel closes it.
        a.screen = crate::screens::ScreenId::People;
        a.update_log_stream();
        assert!(!a.manage.logs.is_streaming());
    }
}

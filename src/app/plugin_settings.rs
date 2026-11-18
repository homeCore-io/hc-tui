//! Plugins › a plugin's Settings tab, and the parameter form an action
//! asks with before it runs.
//!
//! The form comes from the plugin's config descriptor, else its schema
//! (`crate::plugin_form`). Ctrl+S writes only what changed; Esc discards.
//! Secrets are never shown.

use super::{App, Load};
use crate::model::Action;
use crate::plugin_form::{
    ConfigForm, ConfigFormat, Descriptor, Field, Section, descriptor_from_schema, params_descriptor,
};
use crate::screens::ScreenId;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::Value;

use super::plugins::PluginDetailPanel;

#[derive(Debug, Default)]
pub struct SettingsPane {
    pub form: Load<ConfigForm>,
    /// Position among the rows that can be selected.
    pub sel: usize,
    /// The text being typed into the selected setting.
    pub input: Option<String>,
    pub input_error: Option<String>,
    pub saving: bool,
    /// What the last save did.
    pub note: Option<String>,
}

impl SettingsPane {
    /// The selected row's index in the form.
    pub fn row(&self) -> Option<usize> {
        let f = self.form.ready()?;
        let s = f.selectable();
        s.get(self.sel.min(s.len().saturating_sub(1))).copied()
    }
}

/// An action's parameters, asked before it runs.
#[derive(Debug, Clone)]
pub struct ParamForm {
    pub plugin_id: String,
    pub action: Action,
    pub form: ConfigForm,
    pub sel: usize,
    /// What is typed into the selected parameter.
    pub buf: String,
    pub error: Option<String>,
}

impl ParamForm {
    /// The form for an action that takes parameters.
    pub fn for_action(plugin_id: &str, action: &Action) -> Option<Self> {
        let d = params_descriptor(action.params.as_ref()?)?;
        let form = ConfigForm::for_params(&d);
        let mut p = Self {
            plugin_id: plugin_id.to_string(),
            action: action.clone(),
            form,
            sel: 0,
            buf: String::new(),
            error: None,
        };
        p.load_buf();
        Some(p)
    }

    pub fn row(&self) -> Option<usize> {
        self.form.selectable().get(self.sel).copied()
    }

    fn load_buf(&mut self) {
        self.buf = self
            .row()
            .map(|i| self.form.input_text(i))
            .unwrap_or_default();
    }

    /// Put the typed text into the selected parameter.
    fn commit(&mut self) -> bool {
        let Some(i) = self.row() else {
            return true;
        };
        if !self.form.typed(i) {
            return true;
        }
        match self.form.commit(i, &self.buf.clone()) {
            Ok(()) => {
                self.error = None;
                true
            }
            Err(e) => {
                self.error = Some(e);
                false
            }
        }
    }

    fn step_row(&mut self, delta: isize) {
        if !self.commit() {
            return;
        }
        let n = self.form.selectable().len();
        if n == 0 {
            return;
        }
        self.sel = (self.sel as isize + delta).rem_euclid(n as isize) as usize;
        self.load_buf();
    }
}

/// What a plugin says it is, for the Settings tab's words.
fn running(app: &App, plugin_id: &str) -> (bool, String) {
    app.plugins
        .iter()
        .find(|p| p.plugin_id == plugin_id)
        .map(|p| {
            (
                p.enabled && p.status == "active",
                app.plugin_display_name(p),
            )
        })
        .unwrap_or((false, plugin_id.to_string()))
}

impl App {
    fn settings_pane(&mut self) -> Option<&mut SettingsPane> {
        self.plugins_tab.detail.as_mut().map(|d| &mut d.settings)
    }

    /// Whether the Settings tab is on screen.
    pub fn settings_shown(&self) -> bool {
        self.screen == ScreenId::Plugins
            && self
                .plugins_tab
                .detail
                .as_ref()
                .is_some_and(|d| d.panel == PluginDetailPanel::Settings)
    }

    /// Read the plugin's settings and build the form.
    pub fn load_plugin_settings(&mut self) {
        let admin = self.can("plugins:write");
        let Some(d) = self.plugins_tab.detail.as_mut() else {
            return;
        };
        let plugin_id = d.plugin_id.clone();
        if !admin {
            d.settings.form = Load::Failed("Settings need the admin role.".into());
            return;
        }
        d.settings.form = Load::Loading;
        d.settings.input = None;
        let c = self.client.clone();
        let id = plugin_id.clone();
        self.spawn(
            async move {
                let config = c.get_plugin_config(&id).await?;
                let mut desc = c.get_plugin_config_descriptor(&id).await?;
                let mut from_schema = false;
                let has_sections = desc
                    .as_ref()
                    .and_then(|d| d.get("sections"))
                    .and_then(Value::as_array)
                    .is_some_and(|s| !s.is_empty());
                if !has_sections {
                    desc = None;
                    from_schema = true;
                }
                let schema = if from_schema {
                    c.get_plugin_config_schema(&id).await?
                } else {
                    None
                };
                Ok((config, desc, schema))
            },
            move |app, res| {
                let (on, name) = running(app, &plugin_id);
                let level = app
                    .plugins
                    .iter()
                    .find(|p| p.plugin_id == plugin_id)
                    .and_then(|p| p.log_level.clone());
                let Some(d) = app
                    .plugins_tab
                    .detail
                    .as_mut()
                    .filter(|d| d.plugin_id == plugin_id)
                else {
                    return;
                };
                d.settings.form = match res {
                    Err(e) => Load::Failed(e.to_string()),
                    Ok((config, desc, schema)) => {
                        build_form(config, desc, schema, on, &name, level.as_deref())
                    }
                };
            },
        );
    }

    pub fn settings_move(&mut self, delta: isize) {
        let Some(s) = self.settings_pane() else {
            return;
        };
        let n = s.form.ready().map(|f| f.selectable().len()).unwrap_or(0);
        if n == 0 {
            return;
        }
        s.sel = (s.sel as isize + delta).clamp(0, n as isize - 1) as usize;
    }

    /// Enter: type into a setting, or flip one that is not typed.
    pub fn settings_enter(&mut self) {
        let Some(s) = self.settings_pane() else {
            return;
        };
        let Some(i) = s.row() else {
            return;
        };
        let Some(f) = s.form.ready() else {
            return;
        };
        if f.typed(i) {
            s.input = Some(f.input_text(i));
            s.input_error = None;
            return;
        }
        self.settings_step();
    }

    /// Space: flip a toggle, step a choice.
    pub fn settings_step(&mut self) {
        let Some(s) = self.settings_pane() else {
            return;
        };
        let Some(i) = s.row() else {
            return;
        };
        let read_only = s
            .form
            .ready()
            .and_then(|f| f.field_at(i))
            .is_some_and(|f| f.read_only);
        if let Load::Ready(f) = &mut s.form
            && !f.step(i)
        {
            let msg = if read_only {
                "This one is changed in the plugin's file"
            } else {
                "Enter to type a value"
            };
            self.status = msg.into();
        }
    }

    /// `u`: back to what the file holds.
    pub fn settings_undo(&mut self) {
        let Some(s) = self.settings_pane() else {
            return;
        };
        if let (Some(i), Load::Ready(f)) = (s.row(), &mut s.form) {
            f.undo(i);
        }
    }

    /// Esc: discard changes first; with none, close the detail.
    pub fn settings_escape(&mut self) {
        let dirty = self
            .plugins_tab
            .detail
            .as_ref()
            .and_then(|d| d.settings.form.ready())
            .is_some_and(ConfigForm::dirty);
        if !dirty {
            self.close_plugin_detail();
            return;
        }
        if let Some(Load::Ready(f)) = self.settings_pane().map(|s| &mut s.form) {
            f.discard();
        }
        self.status = "Changes discarded".into();
    }

    /// Ctrl+S: write what changed.
    pub fn save_plugin_settings(&mut self) {
        let Some(d) = self.plugins_tab.detail.as_ref() else {
            return;
        };
        let plugin_id = d.plugin_id.clone();
        let Some(form) = d.settings.form.ready() else {
            return;
        };
        if d.settings.saving {
            return;
        }
        if !form.dirty() {
            self.status = "Nothing to save".into();
            return;
        }
        let problems = form.problems();
        if let Some(p) = problems.first() {
            self.error = Some(p.clone());
            return;
        }
        let body = match form.write_body() {
            Ok(b) => b,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let level = form.log_level_change();
        let remote = form.format == ConfigFormat::Remote;
        let (on, name) = running(self, &plugin_id);
        if let Some(s) = self.settings_pane() {
            s.saving = true;
            s.note = None;
        }
        self.status = format!("Saving {name}'s settings...");
        let c = self.client.clone();
        let id = plugin_id.clone();
        let wrote_file = body.is_some();
        self.spawn(
            async move {
                if let Some(b) = body {
                    c.put_plugin_config(&id, b).await?;
                }
                if let Some(l) = level {
                    c.set_plugin_log_level(&id, &l).await?;
                }
                Ok(())
            },
            move |app, res| {
                let Some(d) = app
                    .plugins_tab
                    .detail
                    .as_mut()
                    .filter(|d| d.plugin_id == plugin_id)
                else {
                    return;
                };
                d.settings.saving = false;
                match res {
                    Err(e) => app.error = Some(format!("saving settings: {e}")),
                    Ok(()) => {
                        let note = if !wrote_file {
                            "Saved.".to_string()
                        } else if remote {
                            format!("Saved. Sent to {name}.")
                        } else if on {
                            format!("Saved. homeCore restarts {name} to use them.")
                        } else {
                            format!("Saved. {name} uses them when it is turned on.")
                        };
                        d.settings.note = Some(note.clone());
                        app.status = note;
                        app.start_sync();
                        app.load_plugin_settings();
                    }
                }
            },
        );
    }

    /// Keys for the Plugins screen's own boxes: a new key, the runtime
    /// chooser, an action's parameters, a setting being typed, and
    /// Ctrl+S on the Settings tab. True when one took the key.
    pub(crate) fn on_key_plugins_modal(&mut self, key: KeyEvent) -> bool {
        if self.on_key_issued_key(key) || self.on_key_runtime_chooser(key) {
            return true;
        }
        if self.plugins_tab.params.is_some() {
            self.on_key_param_form(key);
            return true;
        }
        if !self.settings_shown() {
            return false;
        }
        let ctrl_s = key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('s') | KeyCode::Char('S'));
        let typing = self
            .plugins_tab
            .detail
            .as_ref()
            .is_some_and(|d| d.settings.input.is_some());
        if !typing {
            if ctrl_s {
                self.save_plugin_settings();
                return true;
            }
            return false;
        }
        match key.code {
            KeyCode::Esc => {
                if let Some(s) = self.settings_pane() {
                    s.input = None;
                    s.input_error = None;
                }
            }
            KeyCode::Enter => {
                self.commit_setting_input();
            }
            _ if ctrl_s => {
                if self.commit_setting_input() {
                    self.save_plugin_settings();
                }
            }
            _ => {
                if let Some(text) = self.settings_pane().and_then(|s| s.input.as_mut()) {
                    super::editors::edit_text(text, key);
                }
            }
        }
        true
    }

    /// Put the typed text into its setting. False when it was refused.
    fn commit_setting_input(&mut self) -> bool {
        let Some(s) = self.settings_pane() else {
            return false;
        };
        let (Some(i), Some(text)) = (s.row(), s.input.clone()) else {
            return false;
        };
        let Load::Ready(f) = &mut s.form else {
            return false;
        };
        match f.commit(i, &text) {
            Ok(()) => {
                s.input = None;
                s.input_error = None;
                true
            }
            Err(e) => {
                s.input_error = Some(e);
                false
            }
        }
    }

    fn on_key_param_form(&mut self, key: KeyEvent) {
        let Some(p) = self.plugins_tab.params.as_mut() else {
            return;
        };
        let typed = p.row().is_some_and(|i| p.form.typed(i));
        match key.code {
            KeyCode::Esc => {
                self.plugins_tab.params = None;
                self.status = "Cancelled".into();
            }
            KeyCode::Tab | KeyCode::Down => p.step_row(1),
            KeyCode::BackTab | KeyCode::Up => p.step_row(-1),
            KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right if !typed => {
                if let Some(i) = p.row() {
                    p.form.step(i);
                }
            }
            KeyCode::Enter => {
                if !p.commit() {
                    return;
                }
                if let Some(problem) = p.form.problems().into_iter().next() {
                    p.error = Some(problem);
                    return;
                }
                let Some(p) = self.plugins_tab.params.take() else {
                    return;
                };
                let params = Value::Object(p.form.values());
                if p.action.danger {
                    self.ask(super::plugins::danger_confirm(
                        p.plugin_id,
                        p.action,
                        params,
                    ));
                } else {
                    self.run_plugin_action_with(p.plugin_id, p.action, params);
                }
            }
            _ if typed => {
                super::editors::edit_text(&mut p.buf, key);
            }
            _ => {}
        }
    }
}

/// The Settings form from what core sent, or why there is none.
fn build_form(
    config: crate::net::plugins_api::Answer,
    desc: Option<Value>,
    schema: Option<Value>,
    on: bool,
    name: &str,
    level: Option<&str>,
) -> Load<ConfigForm> {
    if !config.ok() {
        // A plugin that keeps its settings to itself can only say them
        // while it runs.
        return Load::Failed(if !on && matches!(config.status, 404 | 503 | 504) {
            format!("{name} is turned off. Turn it on to see its settings.")
        } else {
            config.error()
        });
    }
    let body = config.body;
    if let Some(e) = body.get("config_error").and_then(Value::as_str) {
        return Load::Failed(format!("The file does not read as TOML: {e}"));
    }
    let format = match body.get("format").and_then(Value::as_str) {
        Some("remote") => ConfigFormat::Remote,
        _ => ConfigFormat::Toml,
    };
    let cfg = body.get("config").cloned().filter(Value::is_object);
    let (descriptor, from_schema): (Option<Descriptor>, bool) = match (desc, schema) {
        _ if cfg.is_none() => (None, false),
        (Some(d), _) => (serde_json::from_value(d).ok(), false),
        (None, Some(s)) => (descriptor_from_schema(&s), true),
        (None, None) => (None, false),
    };
    // With no form there is still the log level, and a word on where the
    // settings are.
    let descriptor = descriptor
        .filter(|d| !d.sections.is_empty())
        .unwrap_or_else(|| {
            let text = if format == ConfigFormat::Remote {
                format!(
                    "{name} keeps its own settings and describes none for a form. Its actions may change them."
                )
            } else {
                format!("{name} describes no settings for a form. They are in its file.")
            };
            Descriptor {
                sections: vec![Section {
                    id: "none".into(),
                    title: "Settings".into(),
                    fields: vec![Field {
                        kind: "note".into(),
                        text: Some(text),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
            }
        });
    Load::Ready(ConfigForm::new(
        &descriptor,
        cfg.unwrap_or_else(|| serde_json::json!({})),
        format,
        level,
        from_schema,
    ))
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::app::plugins::PluginDetail;
    use crate::model::Role;
    use crate::net::plugins_api::Answer;
    use serde_json::json;

    fn settings_app() -> App {
        let mut a = signed_in(Role::Admin);
        a.screen = ScreenId::Plugins;
        a.plugins = vec![
            serde_json::from_value(json!({"plugin_id": "plugin.x", "enabled": true, "status": "active", "log_level": "info"})).unwrap(),
        ];
        let mut d = PluginDetail::new("plugin.x".into());
        d.panel = PluginDetailPanel::Settings;
        let config = Answer {
            status: 200,
            body: json!({"format": "toml", "config": {"x": {"port": 80, "token": "__redacted__"}}}),
        };
        let desc = json!({"sections": [{"id": "x", "title": "X", "fields": [
            {"kind": "int", "key": "x.port", "label": "Port"},
            {"kind": "secret", "key": "x.token", "label": "Token"}
        ]}]});
        d.settings.form = build_form(config, Some(desc), None, true, "X", Some("info"));
        a.plugins_tab.detail = Some(d);
        a
    }

    fn form(a: &App) -> &ConfigForm {
        a.plugins_tab
            .detail
            .as_ref()
            .unwrap()
            .settings
            .form
            .ready()
            .unwrap()
    }

    #[test]
    fn typing_a_setting_then_esc_discards() {
        let mut a = settings_app();
        // Row 0 is the log level, 1 the port.
        a.settings_move(1);
        a.settings_enter();
        assert_eq!(
            a.plugins_tab
                .detail
                .as_ref()
                .unwrap()
                .settings
                .input
                .as_deref(),
            Some("80")
        );
        a.on_key(key(KeyCode::Backspace));
        a.on_key(key(KeyCode::Char('1')));
        a.on_key(key(KeyCode::Enter));
        assert!(form(&a).dirty());
        assert_eq!(
            form(&a).write_body().unwrap().unwrap(),
            json!({"patch": {"x": {"port": 81}}})
        );
        a.settings_escape();
        assert!(!form(&a).dirty(), "Esc discards");
        assert!(a.plugins_tab.detail.is_some(), "the detail stays open");
        a.settings_escape();
        assert!(a.plugins_tab.detail.is_none(), "a second Esc goes back");
    }

    #[test]
    fn a_bad_number_keeps_the_input_open() {
        let mut a = settings_app();
        a.settings_move(1);
        a.settings_enter();
        a.on_key(key(KeyCode::Char('x')));
        a.on_key(key(KeyCode::Enter));
        let s = &a.plugins_tab.detail.as_ref().unwrap().settings;
        assert!(s.input.is_some());
        assert!(s.input_error.is_some());
    }

    #[test]
    fn a_secret_input_starts_empty() {
        let mut a = settings_app();
        a.settings_move(2);
        a.settings_enter();
        assert_eq!(
            a.plugins_tab
                .detail
                .as_ref()
                .unwrap()
                .settings
                .input
                .as_deref(),
            Some("")
        );
    }

    #[test]
    fn a_stopped_plugin_says_to_turn_it_on() {
        let off = Answer {
            status: 404,
            body: json!({"error": "no config path for this plugin"}),
        };
        let f = build_form(off, None, None, false, "Hue", None);
        assert_eq!(
            f.error(),
            Some("Hue is turned off. Turn it on to see its settings.")
        );
    }

    #[test]
    fn settings_need_the_admin_role() {
        let mut a = signed_in(Role::Observer);
        a.plugins_tab.detail = Some(PluginDetail::new("plugin.x".into()));
        a.load_plugin_settings();
        let d = a.plugins_tab.detail.as_ref().unwrap();
        assert_eq!(
            d.settings.form.error(),
            Some("Settings need the admin role.")
        );
        assert_eq!(a.pending, 0);
    }

    #[test]
    fn the_parameter_form_runs_with_its_values() {
        let mut a = signed_in(Role::Admin);
        let action: Action = serde_json::from_value(json!({
            "id": "say", "label": "Say", "danger": true,
            "params": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}
        }))
        .unwrap();
        a.plugins_tab.params = ParamForm::for_action("plugin.x", &action);
        a.on_key(key(KeyCode::Enter));
        assert!(
            a.plugins_tab.params.as_ref().unwrap().error.is_some(),
            "text is required"
        );
        a.on_key(key(KeyCode::Char('h')));
        a.on_key(key(KeyCode::Char('i')));
        a.on_key(key(KeyCode::Enter));
        assert!(a.plugins_tab.params.is_none());
        match a.confirm.as_ref().map(|c| &c.action) {
            Some(super::super::editors::ConfirmAction::Plugins(
                super::super::plugin_manage::PluginConfirm::RunAction { params, .. },
            )) => assert_eq!(params, &json!({"text": "hi"})),
            other => panic!("expected a danger question, got {other:?}"),
        }
    }
}

//! Plugins › managing them: turning on and off, start, stop, restart,
//! update, the Catalog and install, and plugin runtimes.
//!
//! Every change needs `plugins:write` (the admin role). Install, update
//! and a runtime's reset ask first; the answer from core is said in the
//! status line, and a 409 that lists runtimes asks which one.

use super::editors::{Confirm, ConfirmAction};
use super::{App, ListSel, Load};
use crate::model::Action;
use crate::net::plugins_api::Lifecycle;
use crate::plugin_admin::{
    self as pa, InstallOutcome, IssuedKey, Placement, RegistryPlugin, Runtime,
};
use crossterm::event::{KeyCode, KeyEvent};
use serde_json::Value;

/// The Plugins screen's questions, answered by `y`.
#[derive(Debug, Clone)]
pub enum PluginConfirm {
    Install {
        plugin_id: String,
        name: String,
        version: Option<String>,
        runtime_id: Option<String>,
        update: bool,
    },
    ResetRuntime {
        runtime_id: String,
        name: String,
    },
    RemoveRuntime {
        runtime_id: String,
        name: String,
    },
    UpdatePython {
        runtime_id: String,
        kind: String,
        series: String,
        to: String,
    },
    RunAction {
        plugin_id: String,
        action: Box<Action>,
        params: Value,
    },
}

/// More than one runtime can run a plugin: which one.
#[derive(Debug, Clone)]
pub struct RuntimeChooser {
    pub plugin_id: String,
    pub name: String,
    pub version: Option<String>,
    pub update: bool,
    pub options: Vec<String>,
    pub sel: usize,
}

#[derive(Debug, Default)]
pub struct PluginManage {
    pub registry: Load<Vec<RegistryPlugin>>,
    pub runtimes: Load<Vec<Runtime>>,
    pub placements: Vec<Placement>,
    /// Each runtime's interpreters and any update waiting.
    pub interpreters: Vec<pa::Interpreter>,
    pub catalog_sel: ListSel,
    pub runtime_sel: ListSel,
    /// The runtime whose detail is open.
    pub runtime_detail: Option<String>,
    pub chooser: Option<RuntimeChooser>,
    /// A key just made by a reset: shown once, then dropped.
    pub issued: Option<IssuedKey>,
    /// What is being installed, for the Catalog's status line.
    pub working: Option<String>,
}

impl App {
    fn can_manage_plugins(&self) -> bool {
        self.can("plugins:write")
    }

    fn needs_admin(&mut self, what: &str) -> bool {
        if self.can_manage_plugins() {
            return false;
        }
        self.error = Some(format!("{what} needs the admin role"));
        true
    }

    /// The core version, for which registry versions run here.
    fn core_version(&self) -> Option<String> {
        self.system_status
            .as_ref()
            .map(|s| s.version.clone())
            .filter(|v| !v.is_empty())
    }

    pub fn registry_list(&self) -> &[RegistryPlugin] {
        self.plugins_tab
            .manage
            .registry
            .ready()
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// A newer version of `p` in the registry, if there is one.
    pub fn plugin_update(&self, p: &crate::model::PluginRecord) -> Option<String> {
        pa::update_of(p, self.registry_list(), self.core_version().as_deref())
    }

    pub fn plugin_display_name(&self, p: &crate::model::PluginRecord) -> String {
        pa::plugin_name(p, self.registry_list())
    }

    // ── Loading ──────────────────────────────────────────────────────────

    pub fn load_registry(&mut self) {
        self.plugins_tab.manage.registry = Load::Loading;
        let c = self.client.clone();
        self.spawn(async move { c.list_registry().await }, |app, res| {
            app.plugins_tab.manage.registry = match res {
                Ok(v) => Load::Ready(v),
                Err(e) => Load::Failed(e.to_string()),
            };
        });
    }

    pub fn load_runtimes(&mut self) {
        if !self.can("plugins:write") {
            return;
        }
        self.plugins_tab.manage.runtimes = Load::Loading;
        let c = self.client.clone();
        self.spawn(
            async move {
                let r = c.list_runtimes().await;
                let p = c.list_placements().await;
                let i = c.list_interpreters().await;
                Ok::<_, anyhow::Error>((r, p, i))
            },
            |app, res| {
                let Ok((r, p, i)) = res else {
                    return;
                };
                if let Ok(i) = i {
                    app.plugins_tab.manage.interpreters = i;
                }
                app.plugins_tab.manage.runtimes = match r {
                    Ok(v) => Load::Ready(v),
                    Err(e) => Load::Failed(e.to_string()),
                };
                if let Ok(p) = p {
                    app.plugins_tab.manage.placements = p;
                }
            },
        );
    }

    /// The runtime a plugin is placed on, if any: an update stays there.
    pub fn placement_of(&self, plugin_id: &str) -> Option<&Placement> {
        self.plugins_tab
            .manage
            .placements
            .iter()
            .find(|p| p.plugin_id == plugin_id)
    }

    // ── Lifecycle ────────────────────────────────────────────────────────

    /// Space: turn the plugin on or off for good.
    pub fn toggle_plugin_enabled(&mut self) {
        if self.needs_admin("turning a plugin on or off") {
            return;
        }
        let Some(p) = self.target_plugin() else {
            return;
        };
        let (id, name, on) = (p.plugin_id.clone(), self.plugin_display_name(p), !p.enabled);
        self.status = format!("Turning {name} {}...", if on { "on" } else { "off" });
        let c = self.client.clone();
        let target = id.clone();
        self.spawn_ok(
            "turn plugin on or off",
            async move { c.set_plugin_enabled(&target, on).await },
            move |app, ()| {
                if let Some(p) = app.plugins.iter_mut().find(|p| p.plugin_id == id) {
                    p.enabled = on;
                }
                app.status = format!("{name} turned {}", if on { "on" } else { "off" });
                app.start_sync();
            },
        );
    }

    /// `s`: start a stopped plugin, stop a running one.
    pub fn start_stop_plugin(&mut self) {
        let Some(p) = self.target_plugin() else {
            return;
        };
        let verb = if pa::is_running(p) {
            Lifecycle::Stop
        } else {
            Lifecycle::Start
        };
        self.plugin_lifecycle(verb);
    }

    /// `R`: restart it.
    pub fn restart_plugin(&mut self) {
        self.plugin_lifecycle(Lifecycle::Restart);
    }

    fn plugin_lifecycle(&mut self, verb: Lifecycle) {
        let what = match verb {
            Lifecycle::Start => "starting",
            Lifecycle::Stop => "stopping",
            Lifecycle::Restart => "restarting",
        };
        if self.needs_admin(&format!("{what} a plugin")) {
            return;
        }
        let Some(p) = self.target_plugin() else {
            return;
        };
        let (id, name) = (p.plugin_id.clone(), self.plugin_display_name(p));
        let mut word = what.to_string();
        if let Some(c) = word.get_mut(0..1) {
            c.make_ascii_uppercase();
        }
        self.status = format!("{word} {name}...");
        let c = self.client.clone();
        self.spawn_ok(
            "plugin start, stop or restart",
            async move { c.plugin_lifecycle(&id, verb).await },
            move |app, ()| {
                app.status = match verb {
                    Lifecycle::Start => format!("Started {name}"),
                    Lifecycle::Stop => format!("Stopped {name}"),
                    Lifecycle::Restart => format!("Restarted {name}"),
                };
                app.start_sync();
            },
        );
    }

    // ── Update and install ───────────────────────────────────────────────

    /// `U`: ask, then update the plugin to the registry's newest.
    pub fn ask_update_plugin(&mut self) {
        if self.needs_admin("updating a plugin") {
            return;
        }
        let Some(p) = self.target_plugin() else {
            return;
        };
        let name = self.plugin_display_name(p);
        let Some(version) = self.plugin_update(p) else {
            self.status = format!("{name} is up to date");
            return;
        };
        let from = pa::version_of(p).unwrap_or("?").to_string();
        let plugin_id = p.plugin_id.clone();
        let runtime_id = self.placement_of(&plugin_id).map(|x| x.runtime_id.clone());
        let where_ = match &runtime_id {
            Some(r) => format!(" Its runtime {r} installs it."),
            None => String::new(),
        };
        self.ask(
            Confirm::new(
                format!("Update {name}?"),
                format!("Update {name} from {from} to {version}?"),
                ConfirmAction::Plugins(PluginConfirm::Install {
                    plugin_id,
                    name,
                    version: Some(version),
                    runtime_id,
                    update: true,
                }),
            )
            .with_detail(format!(
                "Downloaded from the registry, its signature checked, then restarted. Its settings stay.{where_}"
            )),
        );
    }

    /// `i` in the Catalog: ask, then install the selected plugin.
    pub fn ask_install_plugin(&mut self) {
        if self.needs_admin("installing a plugin") {
            return;
        }
        let Some(r) = self.selected_catalog().cloned() else {
            return;
        };
        let core = self.core_version();
        let Some(version) = pa::latest_of(&r, core.as_deref()).map(str::to_string) else {
            self.error = Some(format!(
                "No version of {} runs on this homeCore",
                r.display_name()
            ));
            return;
        };
        let name = r.display_name();
        let size = r
            .size_of(&version)
            .map(|s| format!(", {s}"))
            .unwrap_or_default();
        self.ask(
            Confirm::new(
                format!("Install {name}?"),
                format!("Install {name} {version}?"),
                ConfirmAction::Plugins(PluginConfirm::Install {
                    plugin_id: r.id.clone(),
                    name,
                    version: Some(version),
                    runtime_id: None,
                    update: false,
                }),
            )
            .with_detail(format!(
                "Downloaded from the registry ({}{size}), its signature checked, then started.",
                r.kind(None)
            )),
        );
    }

    pub(crate) fn run_plugin_confirm(&mut self, c: PluginConfirm) {
        match c {
            PluginConfirm::Install {
                plugin_id,
                name,
                version,
                runtime_id,
                update,
            } => self.install_plugin(plugin_id, name, version, runtime_id, update),
            PluginConfirm::ResetRuntime { runtime_id, name } => {
                self.reset_runtime(runtime_id, name)
            }
            PluginConfirm::RemoveRuntime { runtime_id, name } => {
                self.remove_runtime(runtime_id, name)
            }
            PluginConfirm::UpdatePython {
                runtime_id,
                kind,
                series,
                to,
            } => self.update_python(runtime_id, kind, series, to),
            PluginConfirm::RunAction {
                plugin_id,
                action,
                params,
            } => self.run_plugin_action_with(plugin_id, *action, params),
        }
    }

    fn install_plugin(
        &mut self,
        plugin_id: String,
        name: String,
        version: Option<String>,
        runtime_id: Option<String>,
        update: bool,
    ) {
        let doing = if update { "Updating" } else { "Installing" };
        let msg = format!("{doing} {name}...");
        self.status = msg.clone();
        self.plugins_tab.manage.working = Some(msg);
        let c = self.client.clone();
        let (id, v, rt) = (plugin_id.clone(), version.clone(), runtime_id.clone());
        self.spawn(
            async move { c.install_plugin(&id, v.as_deref(), rt.as_deref()).await },
            move |app, res| {
                app.plugins_tab.manage.working = None;
                let answer = match res {
                    Ok(a) => a,
                    Err(e) => {
                        app.error = Some(format!("{doing} {name}: {e}"));
                        return;
                    }
                };
                match pa::install_outcome(answer.status, &answer.body) {
                    InstallOutcome::Installed { version, activated } => {
                        let done = if update { "Updated" } else { "Installed" };
                        let on = if activated { "" } else { " It is turned off." };
                        app.status = format!("{done} {name} {version}.{on}");
                        app.start_sync();
                    }
                    InstallOutcome::Placed {
                        runtime_id,
                        version,
                    } => {
                        app.status = format!(
                            "{name} {version} is going to runtime {runtime_id}, which installs it within a minute or so."
                        );
                        app.start_sync();
                        app.load_runtimes();
                    }
                    InstallOutcome::ChooseRuntime(options) => {
                        app.plugins_tab.manage.chooser = Some(RuntimeChooser {
                            plugin_id,
                            name,
                            version,
                            update,
                            options,
                            sel: 0,
                        });
                    }
                    InstallOutcome::Failed(why) => {
                        app.error = Some(format!("{name} did not {}: {why}", if update { "update" } else { "install" }));
                    }
                }
            },
        );
    }

    /// Keys of the runtime chooser. True when it took the key.
    pub(crate) fn on_key_runtime_chooser(&mut self, key: KeyEvent) -> bool {
        let Some(ch) = self.plugins_tab.manage.chooser.as_mut() else {
            return false;
        };
        let n = ch.options.len();
        match key.code {
            KeyCode::Esc => {
                self.plugins_tab.manage.chooser = None;
                self.status = "Cancelled".into();
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab if n > 0 => ch.sel = (ch.sel + 1) % n,
            KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab if n > 0 => {
                ch.sel = (ch.sel + n - 1) % n
            }
            KeyCode::Enter => {
                if let Some(ch) = self.plugins_tab.manage.chooser.take()
                    && let Some(rt) = ch.options.get(ch.sel).cloned()
                {
                    self.install_plugin(ch.plugin_id, ch.name, ch.version, Some(rt), ch.update);
                }
            }
            _ => {}
        }
        true
    }

    // ── Catalog ──────────────────────────────────────────────────────────

    /// The Catalog as shown: registry plugins not installed, searched.
    pub fn catalog_view(&self) -> Vec<&RegistryPlugin> {
        let q = self.query(crate::screens::ScreenId::Plugins);
        pa::catalog(self.registry_list(), &self.plugins)
            .into_iter()
            .filter(|r| {
                crate::text::matches_query(
                    q,
                    &[
                        &r.id,
                        &r.name,
                        &r.description,
                        r.category.as_deref().unwrap_or(""),
                    ],
                )
            })
            .collect()
    }

    fn catalog_ids(&self) -> Vec<String> {
        self.catalog_view().iter().map(|r| r.id.clone()).collect()
    }

    pub fn selected_catalog(&self) -> Option<&RegistryPlugin> {
        let view = self.catalog_view();
        let sel = &self.plugins_tab.manage.catalog_sel;
        match sel.id() {
            Some(id) => view.into_iter().find(|r| r.id == id),
            None => view.into_iter().nth(sel.index()),
        }
    }

    pub fn move_catalog(&mut self, delta: isize) {
        let ids = self.catalog_ids();
        self.plugins_tab.manage.catalog_sel.move_by(delta, &ids);
    }

    // ── Runtimes ─────────────────────────────────────────────────────────

    pub fn runtime_list(&self) -> &[Runtime] {
        self.plugins_tab
            .manage
            .runtimes
            .ready()
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn runtime_ids(&self) -> Vec<String> {
        self.runtime_list()
            .iter()
            .map(|r| r.runtime_id.clone())
            .collect()
    }

    pub fn selected_runtime(&self) -> Option<&Runtime> {
        let sel = &self.plugins_tab.manage.runtime_sel;
        let list = self.runtime_list();
        match sel.id() {
            Some(id) => list.iter().find(|r| r.runtime_id == id),
            None => list.get(sel.index()),
        }
    }

    /// The runtime whose detail is open.
    pub fn detail_runtime(&self) -> Option<&Runtime> {
        let id = self.plugins_tab.manage.runtime_detail.as_deref()?;
        self.runtime_list().iter().find(|r| r.runtime_id == id)
    }

    pub fn move_runtime(&mut self, delta: isize) {
        let ids = self.runtime_ids();
        self.plugins_tab.manage.runtime_sel.move_by(delta, &ids);
    }

    pub fn open_runtime_detail(&mut self) {
        self.plugins_tab.manage.runtime_detail =
            self.selected_runtime().map(|r| r.runtime_id.clone());
    }

    pub fn close_runtime_detail(&mut self) {
        self.plugins_tab.manage.runtime_detail = None;
    }

    /// `R` on a runtime: ask, then make it a new key.
    /// `U` on a runtime: move its plugins to the newest patch of their Python,
    /// after naming what restarts.
    pub fn ask_update_python(&mut self) {
        if self.needs_admin("updating a runtime's Python") {
            return;
        }
        let Some(r) = self
            .detail_runtime()
            .or_else(|| self.selected_runtime())
            .cloned()
        else {
            return;
        };
        let Some(i) = pa::interpreters_of(&self.plugins_tab.manage.interpreters, &r.runtime_id)
            .into_iter()
            .find(|i| i.update.is_some())
            .cloned()
        else {
            self.status = "No Python update is waiting for this runtime".into();
            return;
        };
        let to = i.update.clone().unwrap_or_default();
        let name = if r.name.is_empty() {
            r.runtime_id.clone()
        } else {
            r.name.clone()
        };
        self.ask(
            Confirm::new(
                format!("Update {} to {to}?", i.name()),
                format!("On {name}, these restart on it: {}.", i.plugins.join(", ")),
                ConfirmAction::Plugins(PluginConfirm::UpdatePython {
                    runtime_id: r.runtime_id,
                    kind: i.kind,
                    series: i.series,
                    to,
                }),
            )
            .with_detail("Their versions and settings stay; the runtime builds their new environments first."),
        );
    }

    fn update_python(&mut self, runtime_id: String, kind: String, series: String, to: String) {
        self.status = format!("Updating to Python {to}...");
        let c = self.client.clone();
        self.spawn_ok(
            "update Python",
            async move { c.update_interpreter(&runtime_id, &kind, &series).await },
            move |app, v| {
                let now = v
                    .get("version")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(&to)
                    .to_string();
                let n = v
                    .get("plugins")
                    .and_then(serde_json::Value::as_array)
                    .map_or(0, Vec::len);
                app.status = format!("{n} plugin(s) now on Python {now}");
                app.load_runtimes();
            },
        );
    }

    /// `n` on Runtimes: name a new runtime; its key is shown once.
    pub fn open_add_runtime(&mut self) {
        if self.needs_admin("adding a runtime") {
            return;
        }
        let mut f = super::manage::settings_modal::PathForm::new(
            super::manage::settings_modal::PathPurpose::NewRuntime,
        );
        // Where hc-tui reaches homeCore is usually where a runtime does too.
        f.path = self.client.base_url().to_string();
        f.field = super::manage::settings_modal::PathField::Name;
        self.manage.modal = Some(super::manage::settings_modal::SettingsModal::Path(f));
    }

    /// `x` on a runtime: say what removing it uninstalls, then ask.
    pub fn ask_remove_runtime(&mut self) {
        if self.needs_admin("removing a runtime") {
            return;
        }
        let Some(r) = self
            .detail_runtime()
            .or_else(|| self.selected_runtime())
            .cloned()
        else {
            return;
        };
        let name = if r.name.is_empty() {
            r.runtime_id.clone()
        } else {
            r.name.clone()
        };
        let placed: Vec<String> =
            crate::plugin_admin::placed_on(&self.plugins_tab.manage.placements, &r.runtime_id)
                .iter()
                .map(|p| p.plugin_id.clone())
                .collect();
        let what = if placed.is_empty() {
            "Nothing is placed on it.".to_string()
        } else {
            format!(
                "Its plugins are uninstalled, with their devices: {}.",
                placed.join(", ")
            )
        };
        self.ask(
            Confirm::new(
                format!("Remove {name}?"),
                format!("{name} is removed and its key stops working. {what}"),
                ConfirmAction::Plugins(PluginConfirm::RemoveRuntime {
                    runtime_id: r.runtime_id,
                    name,
                }),
            )
            .with_detail("The container itself is yours to stop."),
        );
    }

    fn remove_runtime(&mut self, runtime_id: String, name: String) {
        let c = self.client.clone();
        self.spawn_ok(
            "remove runtime",
            async move { c.remove_runtime(&runtime_id).await },
            move |app, ()| {
                app.status = format!("Removed {name}");
                app.load_runtimes();
            },
        );
    }

    pub fn ask_reset_runtime(&mut self) {
        if self.needs_admin("resetting a runtime's login") {
            return;
        }
        let r = self
            .detail_runtime()
            .or_else(|| self.selected_runtime())
            .cloned();
        let Some(r) = r else {
            return;
        };
        let name = if r.name.is_empty() {
            r.runtime_id.clone()
        } else {
            r.name.clone()
        };
        self.ask(
            Confirm::new(
                format!("A new key for {name}?"),
                format!("Reset the login of {name}?"),
                ConfirmAction::Plugins(PluginConfirm::ResetRuntime {
                    runtime_id: r.runtime_id,
                    name,
                }),
            )
            .with_detail(
                "The old key stops working now. The runtime disconnects until the new key is pasted in. Its plugins come back when it does.",
            ),
        );
    }

    fn reset_runtime(&mut self, runtime_id: String, name: String) {
        self.status = format!("Resetting the login of {name}...");
        let c = self.client.clone();
        self.spawn_ok(
            "reset runtime login",
            async move { c.reset_runtime(&runtime_id).await },
            move |app, key| {
                app.status = format!("{name} has a new key");
                app.plugins_tab.manage.issued = Some(key);
                app.load_runtimes();
            },
        );
    }

    /// Keys of the new-key box: any closing key drops the key.
    pub(crate) fn on_key_issued_key(&mut self, key: KeyEvent) -> bool {
        if self.plugins_tab.manage.issued.is_none() {
            return false;
        }
        if matches!(key.code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q')) {
            self.plugins_tab.manage.issued = None;
        }
        true
    }

    pub(crate) fn resolve_plugin_manage_selection(&mut self) {
        let ids = self.catalog_ids();
        self.plugins_tab.manage.catalog_sel.resolve(&ids);
        let ids = self.runtime_ids();
        self.plugins_tab.manage.runtime_sel.resolve(&ids);
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::model::Role;
    use serde_json::json;

    fn with_registry(a: &mut App) {
        a.plugins = vec![
            serde_json::from_value(json!({"plugin_id": "plugin.sonos", "version": "0.1.9", "enabled": true, "status": "active"})).unwrap(),
        ];
        a.plugins_tab.manage.registry = Load::Ready(
            serde_json::from_value(json!([
                {"id": "plugin.sonos", "name": "Sonos", "versions": [{"version": "0.1.10"}]},
                {"id": "plugin.hue", "name": "Hue", "description": "Lights", "versions": [{"version": "0.2.9"}]}
            ]))
            .unwrap(),
        );
        a.refresh_views();
    }

    #[test]
    fn update_asks_first_and_names_both_versions() {
        let mut a = signed_in(Role::Admin);
        with_registry(&mut a);
        a.ask_update_plugin();
        let c = a.confirm.as_ref().expect("asks");
        assert_eq!(c.message, "Update Sonos from 0.1.9 to 0.1.10?");
        assert!(matches!(
            &c.action,
            ConfirmAction::Plugins(PluginConfirm::Install { update: true, version: Some(v), .. }) if v == "0.1.10"
        ));
    }

    #[test]
    fn install_asks_first_for_a_catalog_plugin() {
        let mut a = signed_in(Role::Admin);
        with_registry(&mut a);
        let view: Vec<&str> = a.catalog_view().iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            view,
            vec!["plugin.hue"],
            "installed plugins are not in the Catalog"
        );
        a.ask_install_plugin();
        assert_eq!(a.confirm.as_ref().unwrap().message, "Install Hue 0.2.9?");
    }

    #[test]
    fn read_only_roles_change_nothing() {
        let mut a = signed_in(Role::Observer);
        with_registry(&mut a);
        a.ask_install_plugin();
        a.ask_update_plugin();
        a.toggle_plugin_enabled();
        assert!(a.confirm.is_none());
        assert!(a.error.as_deref().unwrap_or("").contains("admin"));
        assert_eq!(a.pending, 0, "nothing was sent");
    }

    #[test]
    fn the_chooser_picks_a_runtime_and_esc_cancels() {
        let mut a = signed_in(Role::Admin);
        a.plugins_tab.manage.chooser = Some(RuntimeChooser {
            plugin_id: "plugin.mcp".into(),
            name: "Assistant".into(),
            version: None,
            update: false,
            options: vec!["rt-1".into(), "rt-2".into()],
            sel: 0,
        });
        a.on_key(key(KeyCode::Char('j')));
        assert_eq!(a.plugins_tab.manage.chooser.as_ref().unwrap().sel, 1);
        a.on_key(key(KeyCode::Esc));
        assert!(a.plugins_tab.manage.chooser.is_none());
    }

    #[test]
    fn a_new_key_closes_and_is_dropped() {
        let mut a = signed_in(Role::Admin);
        a.plugins_tab.manage.issued = Some(
            serde_json::from_value(json!({"runtime": {"runtime_id": "rt"}, "key": "k"})).unwrap(),
        );
        a.on_key(key(KeyCode::Char('x')));
        assert!(
            a.plugins_tab.manage.issued.is_some(),
            "other keys do nothing"
        );
        a.on_key(key(KeyCode::Enter));
        assert!(a.plugins_tab.manage.issued.is_none());
    }
}

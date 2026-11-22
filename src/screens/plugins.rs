//! Plugins: Installed (with each plugin's detail), Catalog, Runtimes,
//! and Matter when the Matter plugin is installed.
//!
//! Changes need `plugins:write` (the admin role); everyone else who can
//! see plugins gets the lists and details without the keys that change
//! anything.

use super::{Binding, DELETE, DOWN, ENTER, ESC, SPACE, Screen, ScreenId, UP, moves, press};
use crate::app::manage::logs::LogLevelFilter;
use crate::app::{App, Load, PluginDetailPanel, PluginsSub};
use crossterm::event::KeyCode;
use ratatui::{Frame, layout::Rect};

pub struct PluginsScreen;

const RIGHT: &[KeyCode] = &[KeyCode::Right];
const LEFT: &[KeyCode] = &[KeyCode::Left];

fn tabs(v: &mut Vec<Binding>) {
    v.push(Binding::new(RIGHT, "←/→", "tabs", |a| {
        a.step_plugins_sub(true)
    }));
    v.push(Binding::new(LEFT, "←", "previous tab", |a| a.step_plugins_sub(false)).more());
}

/// `s`, `R`, `U` and `x` on the plugin under the cursor or open.
fn lifecycle(app: &App, v: &mut Vec<Binding>) {
    v.extend([
        Binding::new(
            &[KeyCode::Char('s')],
            "s",
            "start/stop",
            App::start_stop_plugin,
        ),
        Binding::new(&[KeyCode::Char('R')], "R", "restart", App::restart_plugin),
    ]);
    if app
        .target_plugin()
        .is_some_and(|p| app.plugin_update(p).is_some())
    {
        v.push(Binding::new(
            &[KeyCode::Char('U')],
            "U",
            "update",
            App::ask_update_plugin,
        ));
    }
    v.push(Binding::new(
        DELETE,
        "x",
        "uninstall",
        App::ask_uninstall_plugin,
    ));
}

fn detail_bindings(app: &App, admin: bool) -> Vec<Binding> {
    let Some(d) = app.plugins_tab.detail.as_ref() else {
        return Vec::new();
    };
    let mut v = vec![
        Binding::new(RIGHT, "←/→", "tab", |a| a.step_detail_panel(true)),
        Binding::new(LEFT, "←", "previous tab", |a| a.step_detail_panel(false)).more(),
    ];
    let mut esc = Binding::new(ESC, "Esc", "back", App::close_plugin_detail);
    match d.panel {
        PluginDetailPanel::Overview => {
            if admin {
                v.push(Binding::new(
                    SPACE,
                    "Space",
                    "on/off",
                    App::toggle_plugin_enabled,
                ));
            }
        }
        PluginDetailPanel::Settings => {
            v.extend(moves(|a| a.settings_move(-1), |a| a.settings_move(1)));
            if d.settings.form.ready().is_some() {
                v.extend([
                    Binding::new(ENTER, "Enter", "edit", App::settings_enter),
                    Binding::new(SPACE, "Space", "change a choice", App::settings_step).more(),
                    Binding::new(
                        &[KeyCode::Char('u')],
                        "u",
                        "undo this change",
                        App::settings_undo,
                    )
                    .more(),
                    Binding::new(&[], "Ctrl+S", "save", |_| {}),
                ]);
                let dirty = d.settings.form.ready().is_some_and(|f| f.dirty());
                let help = if dirty { "discard" } else { "back" };
                esc = Binding::new(ESC, "Esc", help, App::settings_escape);
            }
        }
        PluginDetailPanel::Actions => {
            v.extend([
                Binding::new(DOWN, "j", "move down", |a| a.move_plugin_action(1)).more(),
                Binding::new(UP, "k", "move up", |a| a.move_plugin_action(-1)).more(),
                Binding::new(ENTER, "Enter", "run", App::run_selected_action),
                Binding::new(
                    &[KeyCode::Char('f')],
                    "f",
                    "run with a file",
                    App::run_selected_action_with_file,
                )
                .more(),
            ]);
        }
        PluginDetailPanel::Notices => {
            v.extend(moves(|a| a.scroll_notices(-1), |a| a.scroll_notices(1)));
        }
        PluginDetailPanel::Logs => {
            if app.is_admin() {
                v.extend([
                    Binding::new(SPACE, "Space", "pause", App::plugin_logs_pause),
                    Binding::new(DOWN, "j", "scroll down", |a| a.plugin_logs_scroll(1)).more(),
                    Binding::new(UP, "k", "scroll up", |a| a.plugin_logs_scroll(-1)).more(),
                    Binding::new(&[KeyCode::Char('e')], "e", "errors only", |a| {
                        a.plugin_logs_level(LogLevelFilter::Error)
                    })
                    .more(),
                    Binding::new(&[KeyCode::Char('w')], "w", "warnings and up", |a| {
                        a.plugin_logs_level(LogLevelFilter::Warn)
                    })
                    .more(),
                    Binding::new(&[KeyCode::Char('i')], "i", "info and up", |a| {
                        a.plugin_logs_level(LogLevelFilter::Info)
                    })
                    .more(),
                    Binding::new(&[KeyCode::Char('d')], "d", "debug level", |a| {
                        a.plugin_logs_level(LogLevelFilter::Debug)
                    })
                    .more(),
                    Binding::new(
                        &[KeyCode::Char('c')],
                        "c",
                        "clear the log",
                        App::plugin_logs_clear,
                    )
                    .more(),
                ]);
            }
        }
    }
    if admin {
        lifecycle(app, &mut v);
    }
    v.extend([
        Binding::new(
            &[KeyCode::Char('r')],
            "r",
            "refresh",
            App::refresh_plugin_detail,
        )
        .more(),
        esc,
    ]);
    v
}

fn installed_bindings(app: &App, admin: bool) -> Vec<Binding> {
    let mut v = moves(|a| a.move_plugin(-1), |a| a.move_plugin(1)).to_vec();
    tabs(&mut v);
    v.push(Binding::new(
        ENTER,
        "Enter",
        "detail",
        App::open_plugin_detail,
    ));
    if admin {
        v.push(Binding::new(
            SPACE,
            "Space",
            "on/off",
            App::toggle_plugin_enabled,
        ));
        lifecycle(app, &mut v);
    }
    v
}

fn catalog_bindings(admin: bool) -> Vec<Binding> {
    let mut v = moves(|a| a.move_catalog(-1), |a| a.move_catalog(1)).to_vec();
    tabs(&mut v);
    if admin {
        v.push(Binding::new(
            &[KeyCode::Char('i')],
            "i",
            "install",
            App::ask_install_plugin,
        ));
    }
    v
}

fn runtime_bindings(app: &App, admin: bool) -> Vec<Binding> {
    if app.plugins_tab.manage.runtime_detail.is_some() {
        let mut v = Vec::new();
        if admin {
            v.push(Binding::new(
                &[KeyCode::Char('R')],
                "R",
                "reset login",
                App::ask_reset_runtime,
            ));
            v.push(Binding::new(
                &[KeyCode::Char('U')],
                "U",
                "update Python",
                App::ask_update_python,
            ));
        }
        v.push(Binding::new(ESC, "Esc", "back", App::close_runtime_detail));
        return v;
    }
    let mut v = moves(|a| a.move_runtime(-1), |a| a.move_runtime(1)).to_vec();
    tabs(&mut v);
    v.push(Binding::new(
        ENTER,
        "Enter",
        "detail",
        App::open_runtime_detail,
    ));
    if admin {
        v.push(Binding::new(
            &[KeyCode::Char('R')],
            "R",
            "reset login",
            App::ask_reset_runtime,
        ));
        v.push(
            Binding::new(
                &[KeyCode::Char('U')],
                "U",
                "update Python",
                App::ask_update_python,
            )
            .more(),
        );
        v.extend([
            Binding::new(
                &[KeyCode::Char('n')],
                "n",
                "add a runtime",
                App::open_add_runtime,
            ),
            Binding::new(
                &[KeyCode::Char('x')],
                "x",
                "remove it",
                App::ask_remove_runtime,
            )
            .more(),
        ]);
    }
    v
}

fn matter_bindings(admin: bool) -> Vec<Binding> {
    let mut v = vec![
        Binding::new(DOWN, "j", "move down", |a| {
            a.on_key_matter(press(KeyCode::Down))
        })
        .more(),
        Binding::new(UP, "k", "move up", |a| a.on_key_matter(press(KeyCode::Up))).more(),
    ];
    tabs(&mut v);
    if admin {
        v.extend([
            Binding::new(&[KeyCode::Char('c')], "c", "commission", |a| {
                a.on_key_matter(press(KeyCode::Char('c')))
            }),
            Binding::new(&[KeyCode::Char('i')], "i", "reinterview", |a| {
                a.on_key_matter(press(KeyCode::Char('i')))
            }),
            Binding::new(DELETE, "x", "remove node", |a| {
                a.on_key_matter(press(KeyCode::Char('x')))
            }),
        ]);
    }
    v
}

impl Screen for PluginsScreen {
    fn id(&self) -> ScreenId {
        ScreenId::Plugins
    }

    fn bindings(&self, app: &App) -> Vec<Binding> {
        let admin = app.can("plugins:write");
        if app.plugins_tab.detail.is_some() {
            return detail_bindings(app, admin);
        }
        match app.plugin_sub() {
            PluginsSub::Installed => installed_bindings(app, admin),
            PluginsSub::Catalog => catalog_bindings(admin),
            PluginsSub::Runtimes => runtime_bindings(app, admin),
            // Matter keeps the admin role check it always had.
            PluginsSub::Matter => matter_bindings(app.is_admin()),
        }
    }

    fn draw(&self, frame: &mut Frame<'_>, app: &App, area: Rect) {
        crate::ui::plugins::draw_plugins_tab(frame, app, area);
    }

    fn on_enter(&self, app: &mut App) {
        if matches!(app.plugins_tab.manage.registry, Load::Idle) {
            app.load_registry();
        }
        if app.can("plugins:write") && matches!(app.plugins_tab.manage.runtimes, Load::Idle) {
            app.load_runtimes();
        }
        if app.plugin_sub() == PluginsSub::Matter {
            app.refresh_matter_nodes();
        }
    }

    fn refresh(&self, app: &mut App) {
        app.status = "Refreshing...".to_string();
        app.start_sync();
        app.load_registry();
        app.load_runtimes();
        if app.plugin_sub() == PluginsSub::Matter {
            app.refresh_matter_nodes();
        }
    }

    fn searchable(&self, app: &App) -> bool {
        app.plugins_tab.detail.is_none()
            && matches!(
                app.plugin_sub(),
                PluginsSub::Installed | PluginsSub::Catalog
            )
    }

    fn back(&self, app: &mut App) -> bool {
        if app.plugins_tab.detail.is_some() {
            app.close_plugin_detail();
            return true;
        }
        if app.plugins_tab.manage.runtime_detail.is_some() {
            app.close_runtime_detail();
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::plugins::PluginDetail;
    use crate::app::test_support::*;
    use crate::model::Role;
    use crate::screens::{footer_hints, global_bindings, screen};
    use serde_json::json;

    /// Every Plugins state the shared table walk does not reach.
    fn states(a: &mut App) -> Vec<String> {
        let mut seen = Vec::new();
        a.screen = ScreenId::Plugins;
        a.plugins = vec![
            serde_json::from_value(json!({"plugin_id": "plugin.sonos", "version": "0.1.9", "enabled": true, "status": "active"})).unwrap(),
        ];
        a.plugins_tab.manage.registry = Load::Ready(
            serde_json::from_value(json!([{"id": "plugin.sonos", "name": "Sonos", "versions": [{"version": "0.2.0"}]}])).unwrap(),
        );
        a.plugins_tab.manage.runtimes = Load::Ready(
            serde_json::from_value(json!([{"runtime_id": "rt-1", "name": "Py"}])).unwrap(),
        );
        a.refresh_views();
        let mut check = |a: &App, what: String| {
            let table = screen(ScreenId::Plugins).bindings(a);
            let mut all = table.clone();
            all.extend(global_bindings(a));
            for f in footer_hints(a) {
                assert!(
                    all.iter().any(|b| b.short() == f),
                    "{what}: footer {f} not in the table"
                );
            }
            let mut keys: Vec<KeyCode> = Vec::new();
            for b in &table {
                for k in b.keys {
                    assert!(!keys.contains(k), "{what}: {k:?} bound twice");
                    keys.push(*k);
                }
                if b.keys.contains(&KeyCode::Char('x')) {
                    assert!(
                        b.help.contains("uninstall") || b.help.contains("remove"),
                        "{what}"
                    );
                }
                assert!(!b.help.contains('\u{2014}') && !b.help.contains('\u{2013}'));
                assert!(!b.help.contains("Store"), "the Catalog, never the Store");
            }
            seen.push(what);
        };
        for sub in [
            PluginsSub::Installed,
            PluginsSub::Catalog,
            PluginsSub::Runtimes,
        ] {
            a.plugins_tab.detail = None;
            a.plugins_tab.sub = sub;
            check(a, format!("{sub:?}"));
        }
        a.plugins_tab.manage.runtime_detail = Some("rt-1".into());
        check(a, "runtime detail".into());
        a.plugins_tab.manage.runtime_detail = None;
        a.plugins_tab.sub = PluginsSub::Installed;
        for panel in PluginDetailPanel::ALL {
            let mut d = PluginDetail::new("plugin.sonos".into());
            d.panel = panel;
            a.plugins_tab.detail = Some(d);
            check(a, format!("{panel:?}"));
        }
        seen
    }

    #[test]
    fn every_plugins_state_has_one_key_table() {
        let mut a = signed_in(Role::Admin);
        assert_eq!(states(&mut a).len(), 9);
        let mut a = signed_in(Role::Observer);
        states(&mut a);
    }

    #[test]
    fn readers_get_no_keys_that_change_anything() {
        let mut a = signed_in(Role::Observer);
        states(&mut a);
        for sub in [PluginsSub::Installed, PluginsSub::Catalog] {
            a.plugins_tab.detail = None;
            a.plugins_tab.sub = sub;
            let keys: Vec<&str> = screen(ScreenId::Plugins)
                .bindings(&a)
                .iter()
                .map(|b| b.label)
                .collect();
            for k in ["Space", "s", "R", "U", "x", "i"] {
                assert!(!keys.contains(&k), "{sub:?}: {k} for a reader");
            }
        }
        assert!(
            !a.plugin_subs().contains(&PluginsSub::Runtimes),
            "runtimes need plugins:write"
        );
    }

    #[test]
    fn update_shows_only_when_there_is_one() {
        let mut a = signed_in(Role::Admin);
        states(&mut a);
        a.plugins_tab.detail = None;
        a.plugins_tab.sub = PluginsSub::Installed;
        let has_u = |a: &App| {
            screen(ScreenId::Plugins)
                .bindings(a)
                .iter()
                .any(|b| b.label == "U")
        };
        assert!(has_u(&a));
        a.plugins[0].version = Some("0.2.0".into());
        assert!(!has_u(&a));
    }

    #[test]
    fn tabs_step_and_skip_what_is_hidden() {
        let mut a = signed_in(Role::Admin);
        states(&mut a);
        a.plugins_tab.detail = None;
        a.plugins_tab.sub = PluginsSub::Installed;
        a.plugins_tab.manage.registry = Load::Ready(Vec::new());
        a.plugins_tab.manage.runtimes = Load::Ready(Vec::new());
        a.step_plugins_sub(true);
        assert_eq!(a.plugin_sub(), PluginsSub::Catalog);
        a.step_plugins_sub(true);
        assert_eq!(a.plugin_sub(), PluginsSub::Runtimes);
        a.step_plugins_sub(true);
        assert_eq!(
            a.plugin_sub(),
            PluginsSub::Installed,
            "no Matter without the plugin"
        );
        a.step_plugins_sub(false);
        assert_eq!(a.plugin_sub(), PluginsSub::Runtimes);
    }
}

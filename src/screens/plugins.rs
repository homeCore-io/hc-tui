//! Plugins: the installed list with its detail view, and the Matter tab
//! when the Matter plugin is installed.

use super::{Binding, DELETE, DOWN, ENTER, ESC, SIDE, Screen, ScreenId, UP, moves, press};
use crate::app::{App, PluginDetailPanel, PluginsSub};
use crossterm::event::KeyCode;
use ratatui::{Frame, layout::Rect};

pub struct PluginsScreen;

impl Screen for PluginsScreen {
    fn id(&self) -> ScreenId {
        ScreenId::Plugins
    }

    fn bindings(&self, app: &App) -> Vec<Binding> {
        let admin = app.is_admin();
        if let Some(d) = &app.plugins_tab.detail {
            let mut v = vec![
                Binding::new(&[KeyCode::Right], "←/→", "panel", |a| {
                    a.step_detail_panel(true)
                }),
                Binding::new(&[KeyCode::Left], "←", "panel back", |a| {
                    a.step_detail_panel(false)
                })
                .more(),
            ];
            if d.panel == PluginDetailPanel::Actions {
                v.extend([
                    Binding::new(DOWN, "j", "move down", |a| a.move_plugin_action(1)).more(),
                    Binding::new(UP, "k", "move up", |a| a.move_plugin_action(-1)).more(),
                    Binding::new(ENTER, "Enter", "run", App::run_selected_action),
                ]);
            }
            v.extend([
                Binding::new(
                    &[KeyCode::Char('r')],
                    "r",
                    "refresh",
                    App::refresh_plugin_detail,
                )
                .more(),
                Binding::new(ESC, "Esc", "back", App::close_plugin_detail),
            ]);
            return v;
        }
        let matter = app.has_matter();
        if matter && app.plugins_tab.sub == PluginsSub::Matter {
            let mut v = vec![
                Binding::new(SIDE, "←/→", "installed", App::plugins_switch_sub),
                Binding::new(DOWN, "j", "move down", |a| {
                    a.on_key_matter(press(KeyCode::Down))
                })
                .more(),
                Binding::new(UP, "k", "move up", |a| a.on_key_matter(press(KeyCode::Up))).more(),
            ];
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
            return v;
        }
        let mut v = moves(|a| a.move_plugin(-1), |a| a.move_plugin(1)).to_vec();
        if matter {
            v.push(Binding::new(SIDE, "←/→", "Matter", App::plugins_switch_sub));
        }
        v.push(Binding::new(
            ENTER,
            "Enter",
            "detail",
            App::open_plugin_detail,
        ));
        if admin {
            v.push(Binding::new(
                DELETE,
                "x",
                "uninstall",
                App::ask_uninstall_plugin,
            ));
        }
        v
    }

    fn draw(&self, frame: &mut Frame<'_>, app: &App, area: Rect) {
        crate::ui::plugins::draw_plugins_tab(frame, app, area);
    }

    fn on_enter(&self, app: &mut App) {
        if app.plugins_tab.sub == PluginsSub::Matter && app.has_matter() {
            app.refresh_matter_nodes();
        }
    }

    fn refresh(&self, app: &mut App) {
        if app.plugins_tab.sub == PluginsSub::Matter && app.has_matter() {
            app.refresh_matter_nodes();
        } else {
            app.status = "Refreshing...".to_string();
            app.start_sync();
        }
    }

    fn searchable(&self, app: &App) -> bool {
        app.plugins_tab.detail.is_none() && app.plugins_tab.sub == PluginsSub::Installed
    }
}

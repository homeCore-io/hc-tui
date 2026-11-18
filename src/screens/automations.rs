//! Automations: the list, the read-only detail with Recent runs, and
//! groups.

use super::{
    Binding, DELETE, DOWN, ENTER, ESC, PAGE_DOWN, PAGE_UP, SPACE, Screen, ScreenId, UP, moves,
};
use crate::app::App;
use crate::app::home::{HomeItem, ItemKind};
use crossterm::event::KeyCode;
use ratatui::{Frame, layout::Rect};

pub struct AutomationsScreen;

impl Screen for AutomationsScreen {
    fn id(&self) -> ScreenId {
        ScreenId::Automations
    }

    fn bindings(&self, app: &App) -> Vec<Binding> {
        let write = app.can("automations:write");
        if app.rules_tab.detail.is_some() {
            return vec![
                Binding::new(DOWN, "j/k", "scroll", |a| a.scroll_rule_detail(1)),
                Binding::new(UP, "k", "scroll up", |a| a.scroll_rule_detail(-1)).more(),
                Binding::new(PAGE_DOWN, "PgDn", "page down", |a| a.scroll_rule_detail(10)).more(),
                Binding::new(PAGE_UP, "PgUp", "page up", |a| a.scroll_rule_detail(-10)).more(),
                Binding::new(&[KeyCode::Char('r')], "r", "refresh", App::load_rule_detail).more(),
                Binding::new(ESC, "Esc", "back", App::close_rule_detail),
            ];
        }
        if app.rules_tab.groups.is_some() {
            let mut v = moves(|a| a.move_group(-1), |a| a.move_group(1)).to_vec();
            if write {
                v.extend([
                    Binding::new(&[KeyCode::Char('+')], "+", "group on", |a| {
                        a.set_group_enabled(true)
                    }),
                    Binding::new(&[KeyCode::Char('-')], "-", "group off", |a| {
                        a.set_group_enabled(false)
                    }),
                    Binding::new(DELETE, "x", "delete group", App::ask_delete_group),
                ]);
            }
            v.push(Binding::new(ESC, "Esc", "close", App::close_groups));
            return v;
        }
        let mut v = moves(|a| a.move_rule(-1), |a| a.move_rule(1)).to_vec();
        v.extend([
            Binding::new(PAGE_DOWN, "PgDn", "page down", |a| a.move_rule(10)).more(),
            Binding::new(PAGE_UP, "PgUp", "page up", |a| a.move_rule(-10)).more(),
        ]);
        if write {
            v.push(Binding::new(SPACE, "Space", "on/off", App::toggle_rules));
        }
        v.push(Binding::new(
            ENTER,
            "Enter",
            "detail",
            App::open_rule_detail,
        ));
        v.push(Binding::new(
            &[KeyCode::Char('h')],
            "h",
            "recent runs",
            App::open_recent_runs,
        ));
        if write {
            v.extend([
                Binding::new(&[KeyCode::Char('m')], "m", "mark", App::toggle_rule_mark).more(),
                Binding::new(&[KeyCode::Char('c')], "c", "copy", App::clone_selected_rule),
                Binding::new(DELETE, "x", "delete", App::ask_delete_rule),
            ]);
        }
        v.extend([
            Binding::new(&[KeyCode::Char('g')], "g", "groups", App::open_groups),
            Binding::new(
                &[KeyCode::Char('f')],
                "f",
                "filter",
                App::open_rule_filter_bar,
            ),
            Binding::new(
                &[KeyCode::Char('s')],
                "s",
                "needs fixing only",
                App::toggle_needs_fixing_filter,
            )
            .more(),
        ]);
        v
    }

    fn draw(&self, frame: &mut Frame<'_>, app: &App, area: Rect) {
        crate::ui::rules::draw_rules_tab(frame, app, area);
    }

    fn searchable(&self, app: &App) -> bool {
        app.rules_tab.detail.is_none() && app.rules_tab.groups.is_none()
    }

    fn back(&self, app: &mut App) -> bool {
        app.rules_back()
    }

    fn pin_target(&self, app: &App) -> Option<HomeItem> {
        if app.rules_tab.groups.is_some() {
            return None;
        }
        let id = match &app.rules_tab.detail {
            Some(d) => d.rule_id.clone(),
            None => app.selected_rule()?.id.clone(),
        };
        Some(HomeItem::new(ItemKind::Automation, id))
    }
}

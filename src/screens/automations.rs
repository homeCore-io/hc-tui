//! Automations: the list with what starts each, its last run and group,
//! the selected one's When / If / Then and Recent runs, the read-only
//! RON, run now, a dry run at a time, resuming a paused one, sending a
//! rule's missing device to another, and groups.

use super::{
    Binding, DELETE, DOWN, ENTER, ESC, PAGE_DOWN, PAGE_UP, SPACE, Screen, ScreenId, UP, moves,
};
use crate::app::App;
use crate::app::home::{HomeItem, ItemKind};
use crossterm::event::KeyCode;
use ratatui::{Frame, layout::Rect};

pub struct AutomationsScreen;

const RUN: &[KeyCode] = &[KeyCode::Char('R')];
const TEST: &[KeyCode] = &[KeyCode::Char('t')];
const RESUME: &[KeyCode] = &[KeyCode::Char('u')];
const FIX: &[KeyCode] = &[KeyCode::Char('F')];

/// R, t, u and F, for the list and the detail alike.
fn rule_keys(app: &App, v: &mut Vec<Binding>) {
    let write = app.can("automations:write");
    let rule = match &app.rules_tab.detail {
        Some(d) => app.rules.iter().find(|r| r.id == d.rule_id),
        None => app.selected_rule(),
    };
    let Some(rule) = rule else {
        return;
    };
    if write {
        v.push(Binding::new(RUN, "R", "run now", App::ask_run_rule));
    }
    v.push(Binding::new(TEST, "t", "test at...", App::open_rule_test));
    if write && app.rule_paused(&rule.id) {
        v.push(Binding::new(RESUME, "u", "resume", App::resume_target_rule));
    }
    if write
        && app
            .rules_tab
            .extra
            .missing
            .get(&rule.id)
            .is_some_and(|m| !m.is_empty())
    {
        v.push(Binding::new(
            FIX,
            "F",
            "fix missing device",
            App::open_rule_fix,
        ));
    }
}

impl Screen for AutomationsScreen {
    fn id(&self) -> ScreenId {
        ScreenId::Automations
    }

    fn bindings(&self, app: &App) -> Vec<Binding> {
        let write = app.can("automations:write");
        if app.rules_tab.detail.is_some() {
            let mut v = vec![
                Binding::new(DOWN, "j/k", "scroll", |a| a.scroll_rule_detail(1)),
                Binding::new(UP, "k", "scroll up", |a| a.scroll_rule_detail(-1)).more(),
                Binding::new(PAGE_DOWN, "PgDn", "page down", |a| a.scroll_rule_detail(10)).more(),
                Binding::new(PAGE_UP, "PgUp", "page up", |a| a.scroll_rule_detail(-10)).more(),
                Binding::new(&[KeyCode::Char('r')], "r", "refresh", App::load_rule_detail).more(),
            ];
            rule_keys(app, &mut v);
            v.push(Binding::new(ESC, "Esc", "back", App::close_rule_detail));
            return v;
        }
        if app.rules_tab.variables.is_some() {
            return vec![
                Binding::new(&[KeyCode::Char('r')], "r", "refresh", App::reload_variables),
                Binding::new(ESC, "Esc", "close", App::close_variables),
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
        rule_keys(app, &mut v);
        v.push(Binding::new(ENTER, "Enter", "RON", App::open_rule_detail).more());
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
                &[KeyCode::Char('v')],
                "v",
                "hub variables",
                App::open_variables,
            ),
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

    fn on_enter(&self, app: &mut App) {
        app.load_automation_extras();
    }

    fn refresh(&self, app: &mut App) {
        app.status = "Refreshing...".to_string();
        app.start_sync();
        app.load_automation_extras();
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

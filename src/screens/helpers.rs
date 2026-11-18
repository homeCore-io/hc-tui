//! Helpers: homeCore's own switches, timers, counters and the rest.
//! Space is each kind's main action; the selected kind adds its own keys.

use super::{Binding, DELETE, ENTER, NEW, SPACE, Screen, ScreenId, moves};
use crate::app::App;
use crate::app::home::{HomeItem, ItemKind};
use crossterm::event::KeyCode;
use ratatui::{Frame, layout::Rect};
use serde_json::json;

pub struct HelpersScreen;

const PLUS: &[KeyCode] = &[KeyCode::Char('+'), KeyCode::Char('=')];
const MINUS: &[KeyCode] = &[KeyCode::Char('-')];
const ZERO: &[KeyCode] = &[KeyCode::Char('0')];
const LEFT: &[KeyCode] = &[KeyCode::Left];
const RIGHT: &[KeyCode] = &[KeyCode::Right];
const SET: &[KeyCode] = &[KeyCode::Char('s')];

impl Screen for HelpersScreen {
    fn id(&self) -> ScreenId {
        ScreenId::Helpers
    }

    fn bindings(&self, app: &App) -> Vec<Binding> {
        let mut v = moves(|a| a.move_helper(-1), |a| a.move_helper(1)).to_vec();
        if app.can("devices:write") {
            v.push(Binding::new(
                NEW,
                "n",
                "new helper",
                App::open_helper_creator,
            ));
        }
        let selected = app.selected_helper().map(|d| d.device_type().to_string());
        if selected.is_some() && app.can("devices:write") {
            v.push(Binding::new(
                ENTER,
                "Enter",
                "edit",
                App::open_helper_editor,
            ));
            v.push(Binding::new(DELETE, "x", "delete", App::ask_delete_helper));
        }
        if !app.can("devices:command") {
            return v;
        }
        let Some(t) = selected else {
            return v;
        };
        let space = match t.as_str() {
            "switch" => Some("toggle"),
            "timer" => Some("start or pause"),
            "counter" | "number" => Some("up one"),
            "button" => Some("press"),
            "select" => Some("next choice"),
            _ => None,
        };
        if let Some(word) = space {
            v.push(Binding::new(SPACE, "Space", word, App::helper_space));
        }
        match t.as_str() {
            "timer" => v.push(Binding::new(ZERO, "0", "cancel", |a| {
                a.helper_command("cancel", json!({}), "cancelled")
            })),
            "counter" => v.extend([
                Binding::new(PLUS, "+", "up", |a| {
                    a.helper_command("increment", json!({}), "up one")
                }),
                Binding::new(MINUS, "-", "down", |a| {
                    a.helper_command("decrement", json!({}), "down one")
                }),
                Binding::new(ZERO, "0", "reset", |a| {
                    a.helper_command("reset", json!({}), "reset")
                }),
            ]),
            "number" => v.extend([
                Binding::new(LEFT, "←", "down a step", |a| {
                    a.helper_command("decrement", json!({}), "down a step")
                }),
                Binding::new(RIGHT, "→", "up a step", |a| {
                    a.helper_command("increment", json!({}), "up a step")
                }),
                Binding::new(SET, "s", "set to", App::open_helper_set_value),
            ]),
            "select" => v.extend([
                Binding::new(LEFT, "←", "previous choice", |a| {
                    a.helper_select_step(false)
                })
                .more(),
                Binding::new(RIGHT, "→", "next choice", |a| a.helper_select_step(true)).more(),
            ]),
            "text" | "datetime" => {
                v.push(Binding::new(SET, "s", "set", App::open_helper_set_value))
            }
            _ => {}
        }
        v
    }

    fn draw(&self, frame: &mut Frame<'_>, app: &App, area: Rect) {
        crate::ui::helpers::draw_helpers(frame, app, area);
    }

    fn searchable(&self, _app: &App) -> bool {
        true
    }

    fn pin_target(&self, app: &App) -> Option<HomeItem> {
        app.selected_helper()
            .map(|d| HomeItem::new(ItemKind::Helper, &d.device_id))
    }
}

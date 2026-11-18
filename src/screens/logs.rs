//! Logs: core's live log stream (admin).

use super::{Binding, DOWN, Screen, ScreenId, UP, press};
use crate::app::App;
use crossterm::event::KeyCode;
use ratatui::{Frame, layout::Rect};

pub struct LogsScreen;

fn logs(a: &mut App, c: char) {
    a.on_key_logs(press(KeyCode::Char(c)));
}

impl Screen for LogsScreen {
    fn id(&self) -> ScreenId {
        ScreenId::Logs
    }

    fn bindings(&self, _app: &App) -> Vec<Binding> {
        vec![
            Binding::new(
                &[KeyCode::Char(' '), KeyCode::Char('p')],
                "Space",
                "pause",
                |a| logs(a, 'p'),
            ),
            Binding::new(DOWN, "j/k", "scroll while paused", |a| {
                a.on_key_logs(press(KeyCode::Down))
            })
            .more(),
            Binding::new(UP, "k", "scroll up", |a| a.on_key_logs(press(KeyCode::Up))).more(),
            Binding::new(&[KeyCode::Char('e')], "e/w/i/d", "level", |a| logs(a, 'e')),
            Binding::new(&[KeyCode::Char('w')], "w", "warnings and worse", |a| {
                logs(a, 'w')
            })
            .more(),
            Binding::new(&[KeyCode::Char('i')], "i", "info and worse", |a| {
                logs(a, 'i')
            })
            .more(),
            Binding::new(&[KeyCode::Char('d')], "d", "debug and worse", |a| {
                logs(a, 'd')
            })
            .more(),
            Binding::new(&[KeyCode::Char('/')], "/", "filter", |a| logs(a, '/')),
            Binding::new(&[KeyCode::Char('c')], "c", "clear", |a| logs(a, 'c')),
        ]
    }

    fn draw(&self, frame: &mut Frame<'_>, app: &App, area: Rect) {
        crate::ui::manage::draw_logs(frame, app, area);
    }
}

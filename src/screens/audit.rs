//! Audit: who did what, a page at a time.

use super::{Binding, DOWN, ENTER, Screen, ScreenId, UP, press};
use crate::app::App;
use crossterm::event::KeyCode;
use ratatui::{Frame, layout::Rect};

pub struct AuditScreen;

impl Screen for AuditScreen {
    fn id(&self) -> ScreenId {
        ScreenId::Audit
    }

    fn bindings(&self, _app: &App) -> Vec<Binding> {
        vec![
            Binding::new(DOWN, "j", "move down", |a| {
                a.on_key_audit(press(KeyCode::Down))
            })
            .more(),
            Binding::new(UP, "k", "move up", |a| a.on_key_audit(press(KeyCode::Up))).more(),
            Binding::new(ENTER, "Enter", "detail", |a| {
                a.on_key_audit(press(KeyCode::Enter))
            }),
            Binding::new(&[KeyCode::Char(']')], "]", "next page", |a| {
                a.on_key_audit(press(KeyCode::Char(']')))
            }),
            Binding::new(&[KeyCode::Char('[')], "[", "previous page", |a| {
                a.on_key_audit(press(KeyCode::Char('[')))
            }),
        ]
    }

    fn draw(&self, frame: &mut Frame<'_>, app: &App, area: Rect) {
        crate::ui::manage::draw_audit(frame, app, area);
    }

    fn on_enter(&self, app: &mut App) {
        app.refresh_audit();
    }

    fn refresh(&self, app: &mut App) {
        app.refresh_audit();
    }
}

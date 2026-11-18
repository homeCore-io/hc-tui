//! People: who can sign in and with what role.

use super::{Binding, DELETE, DOWN, ENTER, NEW, Screen, ScreenId, UP, press};
use crate::app::App;
use crossterm::event::KeyCode;
use ratatui::{Frame, layout::Rect};

pub struct PeopleScreen;

impl Screen for PeopleScreen {
    fn id(&self) -> ScreenId {
        ScreenId::People
    }

    fn bindings(&self, app: &App) -> Vec<Binding> {
        let mut v = vec![
            Binding::new(DOWN, "j", "move down", |a| {
                a.on_key_users(press(KeyCode::Down))
            })
            .more(),
            Binding::new(UP, "k", "move up", |a| a.on_key_users(press(KeyCode::Up))).more(),
        ];
        if app.is_admin() {
            v.extend([
                Binding::new(NEW, "n", "new person", |a| {
                    a.on_key_users(press(KeyCode::Char('n')))
                }),
                Binding::new(ENTER, "Enter", "change role", |a| {
                    a.on_key_users(press(KeyCode::Enter))
                }),
                Binding::new(DELETE, "x", "delete", |a| {
                    a.on_key_users(press(KeyCode::Char('x')))
                }),
            ]);
        }
        v.push(Binding::new(
            &[KeyCode::Char('p')],
            "p",
            "change your password",
            |a| a.on_key_users(press(KeyCode::Char('p'))),
        ));
        v
    }

    fn draw(&self, frame: &mut Frame<'_>, app: &App, area: Rect) {
        crate::ui::manage::draw_users(frame, app, area);
    }

    fn searchable(&self, _app: &App) -> bool {
        true
    }
}

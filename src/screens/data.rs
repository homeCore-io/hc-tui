//! Data & backups: the backup zip, and automation and scene export and
//! import.

use super::{Binding, DOWN, ENTER, Screen, ScreenId, UP, press};
use crate::app::App;
use crossterm::event::KeyCode;
use ratatui::{Frame, layout::Rect};

pub struct DataScreen;

impl Screen for DataScreen {
    fn id(&self) -> ScreenId {
        ScreenId::Data
    }

    fn bindings(&self, _app: &App) -> Vec<Binding> {
        vec![
            Binding::new(DOWN, "j", "move down", |a| {
                a.on_key_backup(press(KeyCode::Down))
            })
            .more(),
            Binding::new(UP, "k", "move up", |a| a.on_key_backup(press(KeyCode::Up))).more(),
            Binding::new(ENTER, "Enter", "run", |a| {
                a.on_key_backup(press(KeyCode::Enter))
            }),
        ]
    }

    fn draw(&self, frame: &mut Frame<'_>, app: &App, area: Rect) {
        crate::ui::manage::draw_backup(frame, app, area);
    }
}

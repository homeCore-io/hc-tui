//! Rotations: what each plays now and next; play one, or skip to the next.

use super::{Binding, SPACE, Screen, ScreenId, moves};
use crate::app::App;
use crossterm::event::KeyCode;
use ratatui::{Frame, layout::Rect};

pub struct RotationsScreen;

impl Screen for RotationsScreen {
    fn id(&self) -> ScreenId {
        ScreenId::Rotations
    }

    fn bindings(&self, app: &App) -> Vec<Binding> {
        let mut v = moves(|a| a.move_rotation(-1), |a| a.move_rotation(1)).to_vec();
        if app.can("devices:command") {
            v.extend([
                Binding::new(
                    &[KeyCode::Char('p')],
                    "p",
                    "play",
                    App::play_selected_rotation,
                ),
                Binding::new(SPACE, "Space", "play", App::play_selected_rotation).more(),
                Binding::new(
                    &[KeyCode::Char('s')],
                    "s",
                    "skip to the next",
                    App::skip_selected_rotation,
                ),
            ]);
        }
        v
    }

    fn draw(&self, frame: &mut Frame<'_>, app: &App, area: Rect) {
        crate::ui::manage::draw_rotations(frame, app, area);
    }

    fn searchable(&self, _app: &App) -> bool {
        true
    }

    fn on_enter(&self, app: &mut App) {
        app.refresh_rotations();
    }

    fn refresh(&self, app: &mut App) {
        app.refresh_rotations();
    }
}

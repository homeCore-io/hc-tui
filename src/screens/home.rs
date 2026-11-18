//! Home: the things this person picked, grouped under their headings.

use super::{Binding, DELETE, ENTER, SPACE, Screen, ScreenId, moves};
use crate::app::App;
use crate::app::home::HomeItem;
use crossterm::event::KeyCode;
use ratatui::{Frame, layout::Rect};

pub struct HomeScreen;

impl Screen for HomeScreen {
    fn id(&self) -> ScreenId {
        ScreenId::Home
    }

    fn bindings(&self, _app: &App) -> Vec<Binding> {
        let mut v = moves(|a| move_home(a, -1), |a| move_home(a, 1)).to_vec();
        v.extend([
            Binding::new(SPACE, "Space", "main action", App::home_main_action),
            Binding::new(ENTER, "Enter", "open", App::home_open),
            Binding::new(&[KeyCode::Char('a')], "a", "add", App::open_home_picker),
            Binding::new(DELETE, "x", "remove", App::home_remove),
            Binding::new(&[KeyCode::Char('J')], "J/K", "move", |a| a.home_move(1)),
            Binding::new(&[KeyCode::Char('K')], "K", "move up", |a| a.home_move(-1)).more(),
            Binding::new(
                &[KeyCode::Char('h')],
                "h",
                "heading",
                App::open_heading_editor,
            ),
        ]);
        v
    }

    fn draw(&self, frame: &mut Frame<'_>, app: &App, area: Rect) {
        crate::ui::home::draw_home(frame, app, area);
    }

    /// `*` on Home takes the item off.
    fn pin_target(&self, app: &App) -> Option<HomeItem> {
        app.selected_home_item()
    }
}

fn move_home(a: &mut App, delta: isize) {
    let keys = a.home.layout.keys();
    a.home.sel.move_by(delta, &keys);
}

//! Modes: the house's states. Space switches a manual mode.

use super::{Binding, DELETE, ENTER, NEW, SPACE, Screen, ScreenId, moves};
use crate::app::App;
use crate::app::home::{HomeItem, ItemKind};
use crossterm::event::KeyCode;
use ratatui::{Frame, layout::Rect};

pub struct ModesScreen;

impl Screen for ModesScreen {
    fn id(&self) -> ScreenId {
        ScreenId::Modes
    }

    fn bindings(&self, app: &App) -> Vec<Binding> {
        if app.manage.mode_detail.is_some() {
            return vec![Binding::new(
                &[KeyCode::Esc],
                "Esc",
                "close",
                App::close_mode_definition,
            )];
        }
        let mut v = moves(|a| a.move_mode(-1), |a| a.move_mode(1)).to_vec();
        if app.selected_mode().is_some_and(|m| m.definition.is_some()) {
            v.push(Binding::new(
                &[KeyCode::Char('d')],
                "d",
                "what turns it on and off",
                App::open_mode_definition,
            ));
        }
        if app.can("devices:command") {
            v.push(Binding::new(
                SPACE,
                "Space",
                "mode on/off",
                App::toggle_selected_mode,
            ));
        }
        if app.can("devices:write") {
            v.extend([
                Binding::new(NEW, "n", "new mode", App::new_mode),
                Binding::new(ENTER, "Enter", "edit", App::edit_selected_mode),
                Binding::new(DELETE, "x", "delete", App::ask_delete_mode).more(),
            ]);
        }
        v
    }

    fn draw(&self, frame: &mut Frame<'_>, app: &App, area: Rect) {
        crate::ui::scenes::draw_modes(frame, app, area);
    }

    fn searchable(&self, _app: &App) -> bool {
        true
    }

    fn pin_target(&self, app: &App) -> Option<HomeItem> {
        app.selected_mode()
            .map(|m| HomeItem::new(ItemKind::Mode, &m.config.id))
    }
}

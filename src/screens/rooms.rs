//! Rooms: what is on in each room; Space switches the whole room.

use super::{Binding, DELETE, ENTER, NEW, SPACE, Screen, ScreenId, moves};
use crate::app::App;
use crate::app::home::{HomeItem, ItemKind};
use crossterm::event::KeyCode;
use ratatui::{Frame, layout::Rect};

pub struct RoomsScreen;

impl Screen for RoomsScreen {
    fn id(&self) -> ScreenId {
        ScreenId::Rooms
    }

    fn bindings(&self, app: &App) -> Vec<Binding> {
        let mut v = moves(|a| a.move_room(-1), |a| a.move_room(1)).to_vec();
        if app.can("devices:command") {
            v.push(Binding::new(
                SPACE,
                "Space",
                "room on/off",
                App::toggle_selected_room,
            ));
        }
        v.push(Binding::new(
            ENTER,
            "Enter",
            "open",
            App::open_selected_room,
        ));
        if app.can("areas:write") {
            v.extend([
                Binding::new(NEW, "n", "new room", App::new_room).more(),
                Binding::new(
                    &[KeyCode::Char('e')],
                    "e",
                    "rename",
                    App::rename_selected_room,
                )
                .more(),
                Binding::new(DELETE, "x", "delete", App::ask_delete_room).more(),
            ]);
        }
        v
    }

    fn draw(&self, frame: &mut Frame<'_>, app: &App, area: Rect) {
        crate::ui::rooms::draw_rooms(frame, app, area);
    }

    fn searchable(&self, _app: &App) -> bool {
        true
    }

    fn pin_target(&self, app: &App) -> Option<HomeItem> {
        app.selected_room()
            .map(|r| HomeItem::new(ItemKind::Room, &r.id))
    }
}

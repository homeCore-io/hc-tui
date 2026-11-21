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
        use crate::app::rooms::RoomOverlay;
        match &app.rooms.overlay {
            Some(RoomOverlay::Saved { .. }) => {
                return vec![Binding::new(
                    &[KeyCode::Esc],
                    "Esc",
                    "close",
                    App::close_room_overlay,
                )];
            }
            Some(RoomOverlay::Merge { .. }) => {
                let mut v = moves(|a| a.move_merge_target(-1), |a| a.move_merge_target(1)).to_vec();
                v.extend([
                    Binding::new(
                        ENTER,
                        "Enter",
                        "merge into this room",
                        App::choose_merge_target,
                    ),
                    Binding::new(&[KeyCode::Esc], "Esc", "cancel", App::close_room_overlay),
                ]);
                return v;
            }
            None => {}
        }
        let mut v = moves(|a| a.move_room(-1), |a| a.move_room(1)).to_vec();
        if app.can("areas:read") {
            v.push(
                Binding::new(
                    &[KeyCode::Char('s')],
                    "s",
                    "what Room on brings back",
                    App::show_saved_lights,
                )
                .more(),
            );
        }
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
                Binding::new(
                    &[KeyCode::Char('M')],
                    "M",
                    "merge into another room",
                    App::start_merge_room,
                )
                .more(),
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

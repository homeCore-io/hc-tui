//! Devices: every device (scenes aside), and the media players tab.

use super::{Binding, DELETE, ENTER, PAGE_DOWN, PAGE_UP, SIDE, SPACE, Screen, ScreenId, moves};
use crate::app::home::{HomeItem, ItemKind, is_helper, is_mode_device};
use crate::app::{App, DeviceSubPanel};
use crossterm::event::KeyCode;
use ratatui::{Frame, layout::Rect};

pub struct DevicesScreen;

impl Screen for DevicesScreen {
    fn id(&self) -> ScreenId {
        ScreenId::Devices
    }

    fn bindings(&self, app: &App) -> Vec<Binding> {
        let mut v = moves(|a| a.move_device_cursor(-1), |a| a.move_device_cursor(1)).to_vec();
        v.extend([
            Binding::new(PAGE_DOWN, "PgDn", "page down", |a| a.move_device_cursor(10)).more(),
            Binding::new(PAGE_UP, "PgUp", "page up", |a| a.move_device_cursor(-10)).more(),
        ]);
        let command = app.can("devices:command");
        let write = app.can("devices:write");
        match app.devices_tab.sub {
            DeviceSubPanel::All => {
                v.push(Binding::new(SIDE, "←/→", "media players", App::devices_switch_sub).more());
                if command {
                    v.extend([
                        Binding::new(SPACE, "Space", "on/off", App::selected_device_main_action),
                        Binding::new(
                            &[KeyCode::Char('t')],
                            "t",
                            "on/off",
                            App::toggle_selected_device,
                        )
                        .more(),
                        Binding::new(
                            &[KeyCode::Char('+'), KeyCode::Char('=')],
                            "+",
                            "brighter",
                            |a| a.devices_adjust(true),
                        )
                        .more(),
                        Binding::new(&[KeyCode::Char('-')], "-", "dimmer", |a| {
                            a.devices_adjust(false)
                        })
                        .more(),
                        Binding::new(&[KeyCode::Char('l')], "l", "lock", |a| a.lock_device(true))
                            .more(),
                        Binding::new(&[KeyCode::Char('u')], "u", "unlock", |a| {
                            a.lock_device(false)
                        })
                        .more(),
                    ]);
                }
                v.extend([
                    Binding::new(
                        &[KeyCode::Char('f')],
                        "f",
                        "filter",
                        App::devices_next_filter,
                    ),
                    Binding::new(&[KeyCode::Char('s')], "s", "sort", App::devices_next_sort),
                    Binding::new(
                        &[KeyCode::Char('v')],
                        "v",
                        "grouped or flat",
                        App::devices_toggle_view,
                    )
                    .more(),
                ]);
            }
            DeviceSubPanel::MediaPlayers => {
                v.push(Binding::new(SIDE, "←/→", "all devices", App::devices_switch_sub).more());
                if command {
                    v.extend([
                        Binding::new(SPACE, "Space", "play or stop", App::media_play_stop),
                        Binding::new(
                            &[KeyCode::Char('p')],
                            "p",
                            "play or pause",
                            App::media_play_pause,
                        ),
                        Binding::new(&[KeyCode::Char('s')], "s", "stop", App::media_stop).more(),
                        Binding::new(&[KeyCode::Char('n')], "n", "next track", |a| {
                            a.media_simple("next", "Next")
                        })
                        .more(),
                        Binding::new(&[KeyCode::Char('b')], "b", "previous track", |a| {
                            a.media_simple("previous", "Previous")
                        })
                        .more(),
                        Binding::new(
                            &[KeyCode::Char('+'), KeyCode::Char('=')],
                            "+",
                            "louder",
                            |a| a.devices_adjust(true),
                        ),
                        Binding::new(&[KeyCode::Char('-')], "-", "quieter", |a| {
                            a.devices_adjust(false)
                        })
                        .more(),
                        Binding::new(&[KeyCode::Char('m')], "m", "mute", App::media_toggle_mute)
                            .more(),
                    ]);
                }
            }
        }
        if write {
            v.push(Binding::new(
                ENTER,
                "Enter",
                "edit",
                App::open_device_editor,
            ));
            v.push(Binding::new(DELETE, "x", "remove", App::ask_delete_device).more());
        }
        v
    }

    fn draw(&self, frame: &mut Frame<'_>, app: &App, area: Rect) {
        crate::ui::devices::draw_devices_tab(frame, app, area);
    }

    fn searchable(&self, app: &App) -> bool {
        app.devices_tab.sub == DeviceSubPanel::All
    }

    /// Esc drops the room filter when Devices was not opened from Rooms
    /// (from Rooms, Esc goes back there).
    fn back(&self, app: &mut App) -> bool {
        if app.devices_tab.room.is_some() && app.return_to.is_none() {
            app.devices_tab.room = None;
            return true;
        }
        false
    }

    fn pin_target(&self, app: &App) -> Option<HomeItem> {
        let d = app.selected_device()?;
        let kind = if is_helper(d) {
            ItemKind::Helper
        } else if is_mode_device(d) {
            ItemKind::Mode
        } else {
            ItemKind::Device
        };
        Some(HomeItem::new(kind, &d.device_id))
    }
}

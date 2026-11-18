//! Devices: every device (scenes aside) under All and its category
//! tabs, the media players, and the removed devices.
//!
//! On a device list, Tab moves into the detail pane's controls and
//! ←/→ change the one with focus; Esc leaves them. With no control in
//! focus, ←/→ change the tab.

use super::{Binding, DELETE, ENTER, PAGE_DOWN, PAGE_UP, SPACE, Screen, ScreenId, moves};
use crate::app::home::{HomeItem, ItemKind, is_helper, is_mode_device};
use crate::app::{App, DeviceSubPanel};
use crossterm::event::KeyCode;
use ratatui::{Frame, layout::Rect};

pub struct DevicesScreen;

const LEFT: &[KeyCode] = &[KeyCode::Left];
const RIGHT: &[KeyCode] = &[KeyCode::Right];
const TAB: &[KeyCode] = &[KeyCode::Tab];
const BACKTAB: &[KeyCode] = &[KeyCode::BackTab];

fn tab_keys() -> [Binding; 2] {
    [
        Binding::new(LEFT, "←", "previous tab", |a| a.devices_switch_sub(-1)).more(),
        Binding::new(RIGHT, "→", "next tab", |a| a.devices_switch_sub(1)).more(),
    ]
}

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
        let sub = app.devices_tab.sub;
        match sub {
            DeviceSubPanel::Removed => {
                v.extend(tab_keys());
                if write {
                    v.extend([
                        Binding::new(SPACE, "Space", "bring back", App::restore_selected_device),
                        Binding::new(ENTER, "Enter", "bring back", App::restore_selected_device)
                            .more(),
                    ]);
                }
                return v;
            }
            DeviceSubPanel::MediaPlayers => {
                v.extend(tab_keys());
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
                        Binding::new(&[KeyCode::Char('>')], ">", "next track", |a| {
                            a.media_simple("next", "Next")
                        })
                        .more(),
                        Binding::new(&[KeyCode::Char('<')], "<", "previous track", |a| {
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
            _ => {
                let focused = app.devices_tab.controls.focus.is_some();
                if command {
                    v.push(Binding::new(TAB, "Tab", "control", |a| a.step_control(1)));
                    v.push(
                        Binding::new(BACKTAB, "Shift+Tab", "previous control", |a| {
                            a.step_control(-1)
                        })
                        .more(),
                    );
                }
                if focused && command {
                    v.extend([
                        Binding::new(LEFT, "←", "less or previous", |a| a.change_control(-1)),
                        Binding::new(RIGHT, "→", "more or next", |a| a.change_control(1)),
                        Binding::new(SPACE, "Space", "switch or press", App::press_control),
                    ]);
                } else {
                    v.extend(tab_keys());
                    if command {
                        v.push(Binding::new(
                            SPACE,
                            "Space",
                            "on/off",
                            App::selected_device_main_action,
                        ));
                    }
                }
                if command {
                    v.extend([
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
                    )
                    .more(),
                    Binding::new(&[KeyCode::Char('s')], "s", "sort", App::devices_next_sort).more(),
                    Binding::new(
                        &[KeyCode::Char('v')],
                        "v",
                        "grouped or flat",
                        App::devices_toggle_view,
                    )
                    .more(),
                ]);
            }
        }
        if write {
            v.push(Binding::new(
                ENTER,
                "Enter",
                "edit",
                App::open_device_editor,
            ));
            v.push(
                Binding::new(
                    &[KeyCode::Char('e')],
                    "e",
                    "edit name, room, Security",
                    App::open_device_editor,
                )
                .more(),
            );
        }
        // A room's devices, opened from Rooms: add to it, take out of it.
        if app.devices_tab.room.is_some() && sub != DeviceSubPanel::MediaPlayers {
            if app.can("areas:write") || write {
                v.extend([
                    Binding::new(
                        &[KeyCode::Char('a')],
                        "a",
                        "add devices",
                        App::pick_devices_for_room,
                    ),
                    Binding::new(DELETE, "x", "remove from room", App::ask_remove_from_room),
                ]);
            }
        } else if write {
            v.push(Binding::new(DELETE, "x", "remove", App::ask_delete_device).more());
        }
        v
    }

    fn draw(&self, frame: &mut Frame<'_>, app: &App, area: Rect) {
        crate::ui::devices::draw_devices_tab(frame, app, area);
    }

    fn searchable(&self, app: &App) -> bool {
        app.devices_tab.sub.is_list()
    }

    /// Esc leaves the controls; then drops the room filter when Devices
    /// was not opened from Rooms (from Rooms, Esc goes back there).
    fn back(&self, app: &mut App) -> bool {
        if app.unfocus_control() {
            return true;
        }
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

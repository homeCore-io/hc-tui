//! Drawing. Every function here reads `&App` and the views the app
//! rebuilt for this frame; nothing here mutates state or touches the
//! network.

mod devices;
mod layout;
mod login;
mod manage;
mod modals;
mod plugins;
mod rules;
mod widgets;

use crate::app::App;
use ratatui::Frame;

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    if !app.authenticated {
        login::draw_login(frame, app);
        return;
    }
    layout::draw_main(frame, app);
    modals::draw_overlays(frame, app);
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::app::test_support::*;
    use crate::app::{AdminSubPanel, Tab};
    use crate::model::Role;
    use ratatui::{Terminal, backend::TestBackend};
    use serde_json::json;

    fn render(app: &mut App, w: u16, h: u16) -> String {
        app.refresh_views();
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn populated(role: Role) -> App {
        let mut a = signed_in(role);
        let mut lamp = device("lamp", "Lamp");
        lamp.room = Some("kitchen".into());
        lamp.attributes = json!({"on": true, "brightness_pct": 50.0, "temperature": 21.0})
            .as_object()
            .cloned()
            .unwrap();
        let mut thermo = device("t", "Thermostat with a very long name indeed ünïcödé");
        thermo.attributes =
            json!({"temperature": 19.5, "heat_setpoint": 20.0, "hvac_mode": "heat"})
                .as_object()
                .cloned()
                .unwrap();
        a.devices = vec![lamp, thermo];
        a.rules = vec![
            serde_json::from_value(json!({
                "id": "r1", "name": "Night", "enabled": true, "priority": 1,
                "trigger": {"DeviceStateChanged": {"device_id": "lamp"}}
            }))
            .unwrap(),
        ];
        a
    }

    #[test]
    fn the_login_screen_draws_when_unauthenticated() {
        let mut a = app();
        let screen = render(&mut a, 80, 24);
        assert!(screen.to_lowercase().contains("password"), "{screen}");
    }

    #[test]
    fn the_main_ui_draws_its_tabs_when_authenticated() {
        let mut a = signed_in(Role::User);
        let screen = render(&mut a, 120, 40);
        for tab in [Tab::Devices, Tab::Scenes, Tab::Areas, Tab::Rules] {
            assert!(screen.contains(tab.title()), "{screen}");
        }
    }

    #[test]
    fn every_tab_and_panel_renders_at_odd_sizes() {
        for role in [Role::Admin, Role::User, Role::Observer] {
            let mut a = populated(role);
            let tabs = a.tabs();
            for (i, _) in tabs.iter().enumerate() {
                a.tab = i;
                for sub in AdminSubPanel::ALL {
                    a.manage.sub = sub;
                    for (w, h) in [(20u16, 5u16), (40, 10), (200, 60), (80, 3), (120, 40)] {
                        let _ = render(&mut a, w, h);
                    }
                }
            }
        }
    }

    #[test]
    fn devices_show_display_name_and_converted_temperature() {
        let mut a = populated(Role::Admin);
        a.devices[0].name_override = Some("Kitchen Lamp".into());
        a.temp_unit = crate::model::TempUnit::Fahrenheit;
        let screen = render(&mut a, 160, 40);
        assert!(screen.contains("Kitchen Lamp"), "{screen}");
        assert!(screen.contains("°F"), "{screen}");
    }

    #[test]
    fn the_frame_fills_the_backend_it_was_given() {
        let mut a = signed_in(Role::Admin);
        let screen = render(&mut a, 100, 30);
        assert_eq!(screen.lines().count(), 30);
        assert!(screen.lines().all(|l| l.chars().count() == 100));
    }

    #[test]
    fn a_confirm_dialog_renders() {
        let mut a = populated(Role::Admin);
        a.confirm = Some(crate::app::Confirm::new(
            "Delete device",
            "Delete device \"Lamp\"?",
            crate::app::editors::ConfirmAction::DeleteDevice { id: "lamp".into() },
        ));
        let screen = render(&mut a, 100, 30);
        assert!(screen.contains("Delete device"), "{screen}");
    }
}

//! Alerts: Open, Acknowledged, Rules and Ignored. Alert rules are made
//! in the app; here they are turned on or off and deleted.

use super::{Binding, DELETE, SPACE, Screen, ScreenId, moves};
use crate::app::App;
use crate::app::alerts::AlertsSub;
use crossterm::event::KeyCode;
use ratatui::{Frame, layout::Rect};

pub struct AlertsScreen;

impl Screen for AlertsScreen {
    fn id(&self) -> ScreenId {
        ScreenId::Alerts
    }

    fn bindings(&self, app: &App) -> Vec<Binding> {
        let mut v = moves(|a| a.move_alert(-1), |a| a.move_alert(1)).to_vec();
        let write = app.can("automations:write");
        match app.alerts_tab.sub {
            AlertsSub::Open | AlertsSub::Acknowledged => {
                let sel = app.selected_alert();
                if app.can("devices:command") {
                    if sel.is_some_and(|a| a.is_open()) {
                        v.push(Binding::new(
                            &[KeyCode::Char('a')],
                            "a",
                            "acknowledge",
                            App::acknowledge_selected_alert,
                        ));
                    }
                    if sel.is_some_and(|a| a.action.is_some()) {
                        v.push(Binding::new(
                            &[KeyCode::Char('p')],
                            "p",
                            "run its action",
                            App::press_selected_alert,
                        ));
                    }
                }
                if write && sel.is_some() {
                    v.push(Binding::new(
                        &[KeyCode::Char('i')],
                        "i",
                        "ignore this device",
                        App::ignore_selected_alert_device,
                    ));
                }
            }
            AlertsSub::Rules => {
                if write {
                    v.extend([
                        Binding::new(SPACE, "Space", "on/off", App::toggle_selected_alert_rule),
                        Binding::new(DELETE, "x", "delete", App::ask_delete_alert_rule),
                    ]);
                }
                v.push(Binding::doc(
                    "new rule",
                    "made in the app's Settings, Alerts",
                ));
            }
            AlertsSub::Ignored => {
                if write {
                    v.push(Binding::new(
                        DELETE,
                        "x",
                        "remove the ignore",
                        App::ask_remove_ignore,
                    ));
                }
            }
        }
        let mut tabs = Binding::doc("←→", "tabs");
        tabs.footer = true;
        v.extend([
            tabs,
            Binding::new(&[KeyCode::Left], "←", "previous tab", |a| {
                a.step_alerts_sub(-1)
            })
            .more(),
            Binding::new(&[KeyCode::Right], "→", "next tab", |a| {
                a.step_alerts_sub(1)
            })
            .more(),
        ]);
        v
    }

    fn draw(&self, frame: &mut Frame<'_>, app: &App, area: Rect) {
        crate::ui::alerts::draw_alerts(frame, app, area);
    }

    fn on_enter(&self, app: &mut App) {
        app.load_alert_lists();
    }

    fn refresh(&self, app: &mut App) {
        app.status = "Refreshing...".to_string();
        app.start_sync();
        app.load_alert_lists();
    }

    fn searchable(&self, _app: &App) -> bool {
        true
    }

    fn badge(&self, app: &App) -> Option<usize> {
        Some(app.open_alerts())
    }
}

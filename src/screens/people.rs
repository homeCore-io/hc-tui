//! People: who can sign in and with what role, what each role may do,
//! API keys for programs, and your sessions. Left and Right change tab.

use super::{Binding, DELETE, ENTER, NEW, Screen, ScreenId, moves, press};
use crate::app::App;
use crate::app::manage::people::PeopleTab;
use crossterm::event::KeyCode;
use ratatui::{Frame, layout::Rect};

pub struct PeopleScreen;

fn tabs() -> [Binding; 2] {
    [
        Binding::new(&[KeyCode::Right], "←/→", "tab", |a| {
            a.people_step_tab(1)
        }),
        Binding::new(&[KeyCode::Left], "←", "previous tab", |a| {
            a.people_step_tab(-1)
        })
        .more(),
    ]
}

impl Screen for PeopleScreen {
    fn id(&self) -> ScreenId {
        ScreenId::People
    }

    fn bindings(&self, app: &App) -> Vec<Binding> {
        let mut v = tabs().to_vec();
        match app.manage.people.tab {
            PeopleTab::People => {
                v.extend(moves(
                    |a| a.on_key_users(press(KeyCode::Up)),
                    |a| a.on_key_users(press(KeyCode::Down)),
                ));
                if app.is_admin() {
                    v.extend([
                        Binding::new(NEW, "n", "new person", |a| {
                            a.on_key_users(press(KeyCode::Char('n')))
                        }),
                        Binding::new(ENTER, "Enter", "change role", |a| {
                            a.on_key_users(press(KeyCode::Enter))
                        }),
                        Binding::new(DELETE, "x", "delete", |a| {
                            a.on_key_users(press(KeyCode::Char('x')))
                        }),
                        Binding::new(
                            &[KeyCode::Char('c')],
                            "c",
                            "remove PIN",
                            App::ask_remove_pin,
                        )
                        .more(),
                    ]);
                }
                let me = app
                    .selected_user()
                    .zip(app.current_user.as_ref())
                    .is_none_or(|(sel, me)| sel.id == me.id);
                v.push(Binding::new(
                    &[KeyCode::Char('p')],
                    "p",
                    if me || !app.is_admin() {
                        "change your password"
                    } else {
                        "set their password"
                    },
                    |a| a.on_key_users(press(KeyCode::Char('p'))),
                ));
            }
            PeopleTab::Roles => {
                v.extend(moves(|a| a.move_role(-1), |a| a.move_role(1)));
            }
            PeopleTab::ApiKeys => {
                v.extend(moves(|a| a.move_api_key(-1), |a| a.move_api_key(1)));
                v.extend([
                    Binding::new(NEW, "n", "new key", App::open_new_api_key),
                    Binding::new(&[KeyCode::Char('s')], "s", "new secret", |a| {
                        a.ask_rotate_api_key()
                    }),
                    Binding::new(DELETE, "x", "revoke", App::ask_revoke_api_key),
                ]);
            }
            PeopleTab::Sessions => {
                v.extend(moves(|a| a.move_session(-1), |a| a.move_session(1)));
                v.extend([
                    Binding::new(DELETE, "x", "end session", App::ask_end_session),
                    Binding::new(
                        &[KeyCode::Char('X')],
                        "X",
                        "end all others",
                        App::ask_end_other_sessions,
                    ),
                ]);
            }
        }
        v
    }

    fn draw(&self, frame: &mut Frame<'_>, app: &App, area: Rect) {
        crate::ui::settings::draw_people(frame, app, area);
    }

    fn on_enter(&self, app: &mut App) {
        app.load_people_tab();
    }

    fn refresh(&self, app: &mut App) {
        app.status = "Refreshing...".to_string();
        match app.manage.people.tab {
            PeopleTab::People => app.start_sync(),
            _ => app.load_people_tab(),
        }
    }

    fn searchable(&self, app: &App) -> bool {
        matches!(
            app.manage.people.tab,
            PeopleTab::People | PeopleTab::ApiKeys
        )
    }
}

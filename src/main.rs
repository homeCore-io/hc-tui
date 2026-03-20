mod api;
mod app;
mod cache;
mod ui;
mod ws;

use anyhow::Result;
use app::{login_workflow_from_auth, App, LoginWorkflowResult};
use clap::Parser;
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{io, time::Duration};
use tokio::sync::mpsc;
use ws::{spawn_events_stream, WsAppMsg};

enum AsyncMsg {
    LoginFinished(Result<LoginWorkflowResult, String>),
    LoginPhaseSynthesizing,
}

#[derive(Debug, Parser)]
#[command(name = "hc-tui", about = "Terminal UI client for HomeCore")]
struct Args {
    /// HomeCore API base URL (without /api/v1)
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    base_url: String,
    /// Local cache directory for HomeCore state/config snapshots
    #[arg(long, default_value = "./cache")]
    cache_dir: std::path::PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let mut terminal = setup_terminal()?;
    let mut app = App::new(args.base_url, args.cache_dir);

    let run_result = run_app(&mut terminal, &mut app).await;
    restore_terminal(&mut terminal)?;

    if let Err(err) = run_result {
        eprintln!("hc-tui error: {err}");
    }
    Ok(())
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    let (ws_tx, mut ws_rx) = mpsc::unbounded_channel::<WsAppMsg>();
    let (async_tx, mut async_rx) = mpsc::unbounded_channel::<AsyncMsg>();
    let mut ws_started = false;

    loop {
        app.tick_login_animation();
        terminal.draw(|frame| ui::draw(frame, app))?;

        if app.should_quit {
            break;
        }

        while let Ok(msg) = ws_rx.try_recv() {
            match msg {
                WsAppMsg::Connected => app.on_ws_connected(),
                WsAppMsg::Disconnected(reason) => app.on_ws_disconnected(reason),
                WsAppMsg::Event(event) => app.on_ws_event(event),
            }
        }

        while let Ok(msg) = async_rx.try_recv() {
            match msg {
                AsyncMsg::LoginFinished(Ok(result)) => {
                    app.apply_login_success(result);
                    if app.authenticated && !ws_started {
                        if let Some(token) = app.ws_token() {
                            spawn_events_stream(app.ws_endpoint(), token, ws_tx.clone());
                            ws_started = true;
                        }
                    }
                }
                AsyncMsg::LoginFinished(Err(error)) => app.apply_login_failure(error),
                AsyncMsg::LoginPhaseSynthesizing => app.set_login_phase_synthesizing(),
            }
        }

        if event::poll(Duration::from_millis(100))? {
            let evt = event::read()?;
            if let Event::Key(key) = evt {
                if !app.authenticated {
                    let submit = app.on_key_login(key);
                    if submit {
                        if let Some((username, password)) = app.begin_login() {
                            let tx = async_tx.clone();
                            let client = app.client.clone();
                            let cache = app.cache.clone();
                            tokio::spawn(async move {
                                let result = match client.login(&username, &password).await {
                                    Ok(auth) => {
                                        let _ = tx.send(AsyncMsg::LoginPhaseSynthesizing);
                                        login_workflow_from_auth(client, cache, auth)
                                            .await
                                            .map_err(|e| e.to_string())
                                    }
                                    Err(e) => Err(e.to_string()),
                                };
                                let _ = tx.send(AsyncMsg::LoginFinished(result));
                            });
                        }
                    }
                } else {
                    app.on_key_authenticated(key).await;
                }
            }
        }
    }
    Ok(())
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

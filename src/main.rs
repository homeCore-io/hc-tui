mod api;
mod app;
mod cache;
mod config;
mod sse;
mod ui;
mod ws;

use anyhow::Result;
use app::{App, LoginWorkflowResult, login_workflow_from_auth};
use cache::CacheStore;
use clap::Parser;
use config::Config;
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{
    io,
    path::PathBuf,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use ws::{WsAppMsg, spawn_events_stream, spawn_log_stream};

enum AsyncMsg {
    // Result is significantly larger than the unit variant — box it to keep
    // the enum compact (clippy::large_enum_variant).
    LoginFinished(Box<Result<LoginWorkflowResult, String>>),
    LoginPhaseSynthesizing,
}

#[derive(Debug, Parser)]
#[command(name = "hc-tui", about = "Terminal UI client for HomeCore")]
struct Args {
    /// Path to config file
    #[arg(long, default_value = "config/config.toml")]
    config: PathBuf,
    /// HomeCore API base URL — overrides config file value
    #[arg(long)]
    base_url: Option<String>,
    /// Local cache directory — overrides config file value
    #[arg(long)]
    cache_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Load config file (optional; defaults apply if missing)
    let cfg = Config::load(&args.config)?;

    // CLI overrides take priority over config file
    let base_url = args.base_url.unwrap_or(cfg.server.base_url);
    let cache_dir = args
        .cache_dir
        .unwrap_or_else(|| PathBuf::from(&cfg.cache.dir));
    let persist_token = cfg.session.persist_token;
    let auto_login = cfg.auto_login;

    let cache = CacheStore::new(cache_dir);

    // Try to restore a previously saved session token, and also
    // remember the cached username regardless — we'll use it to
    // pre-fill the login form when restoration fails (expired token,
    // server unreachable, etc.) so users don't retype it.
    let cached_session = if persist_token {
        cache.load_session().await.ok().flatten()
    } else {
        None
    };
    let restored: Option<LoginWorkflowResult> = if let Some(ref saved) = cached_session {
        let client = api::HomeCoreClient::new(base_url.clone());
        App::try_restore_session(client, cache.clone(), saved.token.clone()).await
    } else {
        None
    };

    let mut terminal = setup_terminal()?;
    let mut app = App::new(base_url, cache);

    if let Some(result) = restored {
        // Token is valid — skip login screen entirely
        app.apply_login_success(result);
    } else if let Some(ref al) = auto_login {
        // Pre-fill username; spawn auto-login in the background
        app.begin_auto_login(al.username.clone());
    } else if let Some(saved) = cached_session {
        // Token failed to restore but we know who logged in last;
        // pre-fill the username and jump focus to the password.
        app.username = saved.username;
        app.focus = app::FocusField::Password;
    }

    let run_result = run_app(&mut terminal, &mut app, auto_login, persist_token).await;
    restore_terminal(&mut terminal)?;

    if let Err(err) = run_result {
        eprintln!("hc-tui error: {err}");
    }
    Ok(())
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    auto_login: Option<config::AutoLoginConfig>,
    persist_token: bool,
) -> Result<()> {
    let (ws_tx, mut ws_rx) = mpsc::unbounded_channel::<WsAppMsg>();
    let (async_tx, mut async_rx) = mpsc::unbounded_channel::<AsyncMsg>();
    // The streaming-action SSE consumer needs the same channel as the
    // WS event stream so its messages land in the same drain.
    app.ws_sender = Some(ws_tx.clone());
    let mut ws_started = false;
    let mut auto_login_fired = false;
    let mut log_ws_started = false;
    let mut needs_draw = true;
    let mut last_login_animation_tick = Instant::now();

    // If already authenticated (restored session), start WS immediately
    if app.authenticated
        && let Some(token) = app.ws_token()
    {
        spawn_events_stream(app.ws_endpoint(), token.clone(), ws_tx.clone());
        ws_started = true;
    }

    loop {
        if needs_draw {
            terminal.draw(|frame| ui::draw(frame, app))?;
            needs_draw = false;
        }

        if app.should_quit {
            break;
        }

        // Fire auto-login on first iteration if configured and not yet authenticated
        if !app.authenticated
            && !auto_login_fired
            && let Some(ref al) = auto_login
        {
            auto_login_fired = true;
            let username = al.username.clone();
            let password = al.password.clone();
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
                let _ = tx.send(AsyncMsg::LoginFinished(Box::new(result)));
            });
            needs_draw = true;
        }

        let mut saw_ws_update = false;
        while let Ok(msg) = ws_rx.try_recv() {
            match msg {
                WsAppMsg::Connected => app.on_ws_connected(),
                WsAppMsg::Disconnected(reason) => app.on_ws_disconnected(reason),
                WsAppMsg::Event(event) => app.on_ws_event(event),
                WsAppMsg::LogConnected => app.on_log_ws_connected(),
                WsAppMsg::LogDisconnected(reason) => app.on_log_ws_disconnected(reason),
                WsAppMsg::LogLine(line) => app.on_log_line(line),
                WsAppMsg::StreamConnected => app.on_stream_connected(),
                WsAppMsg::StreamEvent(value) => app.on_stream_event(value),
                WsAppMsg::StreamClosed => app.on_stream_closed(),
                WsAppMsg::StreamError(reason) => app.on_stream_error(reason),
            }
            saw_ws_update = true;
        }
        if saw_ws_update {
            needs_draw = true;
        }

        let mut saw_async_update = false;
        while let Ok(msg) = async_rx.try_recv() {
            match msg {
                AsyncMsg::LoginFinished(boxed) => match *boxed {
                    Ok(result) => {
                        // Save the session token before applying the result
                        if persist_token {
                            let _ = app
                                .cache
                                .save_session(&result.auth.user.username, &result.auth.token)
                                .await;
                        }
                        app.apply_login_success(result);
                        if app.authenticated
                            && !ws_started
                            && let Some(token) = app.ws_token()
                        {
                            spawn_events_stream(app.ws_endpoint(), token.clone(), ws_tx.clone());
                            ws_started = true;
                        }
                    }
                    Err(error) => app.apply_login_failure(error),
                },
                AsyncMsg::LoginPhaseSynthesizing => app.set_login_phase_synthesizing(),
            }
            saw_async_update = true;
        }
        if saw_async_update {
            needs_draw = true;
        }

        if app.wants_log_stream()
            && !log_ws_started
            && let Some(token) = app.ws_token()
        {
            let log_url = app.ws_logs_endpoint();
            let level = app.log_level_filter.as_str().to_string();
            spawn_log_stream(log_url, token, level, String::new(), ws_tx.clone());
            log_ws_started = true;
            needs_draw = true;
        }

        if event::poll(Duration::from_millis(100))? {
            let evt = event::read()?;
            if let Event::Key(key) = evt {
                if !app.authenticated {
                    let submit = app.on_key_login(key);
                    if submit && let Some((username, password)) = app.begin_login() {
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
                            let _ = tx.send(AsyncMsg::LoginFinished(Box::new(result)));
                        });
                    }
                } else {
                    app.on_key_authenticated(key).await;
                }
                needs_draw = true;
            }
        } else if app.login_in_progress
            && last_login_animation_tick.elapsed() >= Duration::from_millis(125)
        {
            app.tick_login_animation();
            last_login_animation_tick = Instant::now();
            needs_draw = true;
        }
    }

    // Clear the persisted session on clean exit (Esc/q) only if the user
    // explicitly logged out; for now we keep it so next startup is seamless.
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

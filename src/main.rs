mod app;
mod cache;
mod config;
#[cfg(test)]
mod fixture_tests;
mod model;
mod net;
mod screens;
mod sync;
mod text;
mod ui;

use anyhow::Result;
use app::{App, AsyncMsg};
use cache::CacheStore;
use clap::Parser;
use config::Config;
use crossterm::event::{Event, EventStream, KeyEventKind};
use futures_util::StreamExt;
use ratatui::DefaultTerminal;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver;

const TICK: Duration = Duration::from_millis(250);

#[derive(Debug, Parser)]
#[command(name = "hc-tui", about = "Terminal UI client for homeCore")]
struct Args {
    /// Path to the config file. Default: $XDG_CONFIG_HOME/hc-tui/config.toml
    /// (~/.config/hc-tui/config.toml), or ./config/config.toml if only that
    /// exists.
    #[arg(long)]
    config: Option<PathBuf>,
    /// homeCore base URL (without /api/v1). Overrides the config file.
    #[arg(long)]
    base_url: Option<String>,
    /// Cache directory. Overrides the config file. Default:
    /// $XDG_CACHE_HOME/hc-tui (~/.cache/hc-tui).
    #[arg(long)]
    cache_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config_path = args.config.unwrap_or_else(config::default_config_path);
    let cfg = Config::load(&config_path)?;

    let base_url = args.base_url.unwrap_or(cfg.server.base_url);
    let cache_dir = args
        .cache_dir
        .or_else(|| cfg.cache.dir.map(PathBuf::from))
        .unwrap_or_else(config::default_cache_dir);
    let cache = CacheStore::new(cache_dir);

    let (mut app, rx) = App::new(base_url, cache);
    app.persist_session = cfg.session.persist_token;

    // A saved session first; failing that, the configured auto-login.
    let saved = if cfg.session.persist_token {
        app.cache.load_session()
    } else {
        None
    };
    match (saved, cfg.auto_login.as_ref()) {
        (Some(saved), _) => app.begin_restore(saved),
        (None, Some(al)) => app.begin_auto_login(al),
        (None, None) => {}
    }

    // ratatui::init installs a panic hook that restores the terminal.
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app, rx).await;
    ratatui::restore();
    if let Err(err) = result {
        eprintln!("hc-tui error: {err}");
    }
    Ok(())
}

/// The event loop: terminal input, background results and a tick, with
/// a redraw whenever something changed. Key handling never awaits the
/// network; requests run in spawned tasks that report back on `rx`.
async fn run(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    mut rx: UnboundedReceiver<AsyncMsg>,
) -> Result<()> {
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(TICK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut dirty = true;

    loop {
        if dirty {
            app.refresh_views();
            app.update_streams();
            terminal.draw(|frame| ui::draw(frame, app))?;
            dirty = false;
        }
        if app.should_quit {
            return Ok(());
        }
        tokio::select! {
            maybe = events.next() => match maybe {
                Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                    app.on_key(key);
                    dirty = true;
                }
                Some(Ok(Event::Resize(_, _))) => dirty = true,
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(e.into()),
                None => return Ok(()),
            },
            Some(msg) = rx.recv() => {
                app.on_msg(msg);
                // Apply everything else that is already waiting before
                // drawing once.
                while let Ok(msg) = rx.try_recv() {
                    app.on_msg(msg);
                }
                dirty = true;
            }
            _ = tick.tick() => {
                if app.on_tick() {
                    dirty = true;
                }
            }
        }
    }
}

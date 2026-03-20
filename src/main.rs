mod api;
mod app;
mod ui;

use anyhow::Result;
use app::App;
use clap::Parser;
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::time::Instant;
use std::{io, time::Duration};

#[derive(Debug, Parser)]
#[command(name = "hc-tui", about = "Terminal UI client for HomeCore")]
struct Args {
    /// HomeCore API base URL (without /api/v1)
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    base_url: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let mut terminal = setup_terminal()?;
    let mut app = App::new(args.base_url);

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
    let mut last_refresh = Instant::now();
    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;

        if app.should_quit {
            break;
        }

        if event::poll(Duration::from_millis(100))? {
            let evt = event::read()?;
            if let Event::Key(key) = evt {
                if !app.authenticated {
                    let submit = app.on_key_login(key);
                    if submit {
                        app.login().await;
                    }
                } else {
                    app.on_key_authenticated(key).await;
                }
            }
        }

        if app.authenticated && last_refresh.elapsed() >= Duration::from_secs(5) {
            last_refresh = Instant::now();
            if let Err(err) = app.refresh_all().await {
                app.error = Some(err.to_string());
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

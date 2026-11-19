//! Terminal front end for the Royal Game of Ur.

mod app;
mod cli;
mod ui;

use app::App;
use clap::Parser;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use std::io;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Upper bound on how long we block waiting for input, so resizes and
/// timers stay responsive.
const MAX_POLL: Duration = Duration::from_millis(250);
/// Redraw interval while something is animating (~30 fps).
const FRAME: Duration = Duration::from_millis(33);

fn main() -> io::Result<()> {
    // Parse before touching the terminal, so --help and errors print normally.
    let cli = cli::Cli::parse();
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x5EED);
    let mut app = cli.into_app(seed, Instant::now());
    ratatui::run(|terminal| run(terminal, &mut app))
}

fn run(terminal: &mut DefaultTerminal, app: &mut App) -> io::Result<()> {
    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, app))?;

        let now = Instant::now();
        let max_wait = if app.is_animating() { FRAME } else { MAX_POLL };
        let timeout = app
            .deadline()
            .map_or(max_wait, |d| d.saturating_duration_since(now).min(max_wait));
        if event::poll(timeout)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            app.handle_key(key, Instant::now());
        }
        app.update(Instant::now());
    }
    Ok(())
}

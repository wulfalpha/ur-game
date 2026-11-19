//! Terminal front end for the Royal Game of Ur.

mod app;
mod ui;

use app::App;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use std::io;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Upper bound on how long we block waiting for input, so resizes and
/// timers stay responsive.
const MAX_POLL: Duration = Duration::from_millis(250);

fn main() -> io::Result<()> {
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x5EED);
    let mut app = App::new(seed);
    ratatui::run(|terminal| run(terminal, &mut app))
}

fn run(terminal: &mut DefaultTerminal, app: &mut App) -> io::Result<()> {
    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, app))?;

        let now = Instant::now();
        let timeout = app
            .deadline()
            .map_or(MAX_POLL, |d| d.saturating_duration_since(now).min(MAX_POLL));
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

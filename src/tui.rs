//! Terminal front end: the event loop around [`App`] and [`crate::ui`].

use crate::app::App;
use crate::ui;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use std::io;
use std::time::{Duration, Instant};

/// Upper bound on how long we block waiting for input, so resizes and
/// timers stay responsive.
const MAX_POLL: Duration = Duration::from_millis(250);
/// Redraw interval while something is animating (~30 fps).
const FRAME: Duration = Duration::from_millis(33);

/// Take over the terminal and play until the player quits. `start` is the
/// moment the app's clock counts from.
pub fn run(mut app: App, start: Instant) -> io::Result<()> {
    ratatui::run(|terminal| event_loop(terminal, &mut app, start))
}

fn event_loop(terminal: &mut DefaultTerminal, app: &mut App, start: Instant) -> io::Result<()> {
    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, app))?;

        let now = start.elapsed();
        let max_wait = if app.is_animating() { FRAME } else { MAX_POLL };
        let timeout = app
            .deadline()
            .map_or(max_wait, |d| d.saturating_sub(now).min(max_wait));
        if event::poll(timeout)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            app.handle_key(key, start.elapsed());
        }
        app.update(start.elapsed());
    }
    Ok(())
}

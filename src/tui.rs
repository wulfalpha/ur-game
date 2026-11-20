//! Terminal front end: the event loop around [`App`] and [`crate::ui`].

use crate::app::App;
use crate::ui;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use std::io::{self, Write};
use std::process::{Command, Stdio};
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
    let mut audio = crate::sound::Audio::new();
    while !app.should_quit {
        audio.update(app);
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
        if let Some(text) = app.clipboard.take() {
            copy_to_clipboard(&text);
        }
    }
    Ok(())
}

/// Put `text` on the clipboard. OSC 52 asks the terminal to do it, which
/// reaches the player's own clipboard even over SSH, in terminals that allow
/// it. Apple's Terminal ignores OSC 52, so on a local Mac `pbcopy` is used too.
fn copy_to_clipboard(text: &str) {
    let mut out = io::stdout();
    let _ = out.write_all(crate::chronicle::osc52(text).as_bytes());
    let _ = out.flush();
    let remote = std::env::var_os("SSH_CONNECTION").is_some();
    if cfg!(target_os = "macos")
        && !remote
        && let Ok(mut child) = Command::new("pbcopy").stdin(Stdio::piped()).spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
    }
}

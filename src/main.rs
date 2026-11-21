//! The Royal Game of Ur: graphical by default, or in the terminal with
//! `--tui`.

mod app;
mod chronicle;
mod cli;
mod expert;
mod gui;
mod preferences;
mod save;
mod sound;
mod stats;
mod tui;
mod ui;

use clap::Parser;
use std::io;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

fn main() -> io::Result<()> {
    // Parse before touching the terminal or opening a window, so --help and
    // errors print normally.
    let cli = cli::Cli::parse();
    let terminal = cli.wants_terminal();
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x5EED);
    let start = Instant::now();
    let mut app = cli.into_app_with_preferences(
        seed,
        start.elapsed(),
        save::default_path(),
        preferences::default_path(),
        stats::default_path(),
        chronicle::default_dir(),
    );
    app.use_expert_table(expert::default_path());
    if terminal {
        tui::run(app, start)
    } else {
        // macroquad keeps its own clock, starting at zero like ours.
        gui::run(app);
        Ok(())
    }
}

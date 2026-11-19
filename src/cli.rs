//! Command-line options.

use crate::app::{App, Controller};
use clap::{Parser, ValueEnum};
use std::time::Instant;
use ur_game::ai::Difficulty;

/// The Royal Game of Ur, one of the oldest known board games.
#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli {
    /// Use the terminal interface (the default; currently the only one).
    #[arg(long)]
    pub tui: bool,

    /// Who plays Light, who moves first.
    #[arg(short, long, value_enum, value_name = "PLAYER", default_value_t = Seat::Human)]
    pub light: Seat,

    /// Who plays Dark.
    #[arg(short, long, value_enum, value_name = "PLAYER", default_value_t = Seat::Medium)]
    pub dark: Seat,

    /// Skip the menu and start a game straight away.
    #[arg(short, long)]
    pub start: bool,

    /// Turn off the dice and piece animations.
    #[arg(long)]
    pub no_animation: bool,

    /// Seed for the dice (and the Random computer player), so a game can be
    /// replayed with the same throws.
    #[arg(long, value_name = "NUMBER")]
    pub seed: Option<u64>,
}

/// Who sits at one side of the board.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Seat {
    Human,
    Random,
    Easy,
    Medium,
    Hard,
}

impl From<Seat> for Controller {
    fn from(seat: Seat) -> Self {
        match seat {
            Seat::Human => Controller::Human,
            Seat::Random => Controller::Computer(Difficulty::Random),
            Seat::Easy => Controller::Computer(Difficulty::Easy),
            Seat::Medium => Controller::Computer(Difficulty::Medium),
            Seat::Hard => Controller::Computer(Difficulty::Hard),
        }
    }
}

impl Cli {
    /// Build the app these options describe. `default_seed` is used when no
    /// `--seed` was given.
    pub fn into_app(self, default_seed: u64, now: Instant) -> App {
        let mut app = App::new(self.seed.unwrap_or(default_seed));
        app.controllers = [self.light.into(), self.dark.into()];
        app.animations = !self.no_animation;
        if self.start {
            app.new_game(now);
        }
        app
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Screen;
    use clap::CommandFactory;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("ur-game").chain(args.iter().copied())).unwrap()
    }

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn defaults_match_the_menu() {
        let app = parse(&[]).into_app(1, Instant::now());
        assert_eq!(app.screen, Screen::Menu);
        assert_eq!(
            app.controllers,
            [Controller::Human, Controller::Computer(Difficulty::Medium)]
        );
        assert!(app.animations);
    }

    #[test]
    fn all_options() {
        let cli = parse(&[
            "--tui",
            "--light",
            "hard",
            "-d",
            "human",
            "--start",
            "--no-animation",
            "--seed",
            "7",
        ]);
        assert!(cli.tui);
        assert_eq!(cli.seed, Some(7));
        let app = cli.into_app(1, Instant::now());
        assert_eq!(app.screen, Screen::Game);
        assert_eq!(
            app.controllers,
            [Controller::Computer(Difficulty::Hard), Controller::Human]
        );
        assert!(!app.animations);
    }

    #[test]
    fn same_seed_same_throws() {
        let throws = |seed: &str| {
            let app = parse(&[
                "-l",
                "easy",
                "-d",
                "easy",
                "-s",
                "--no-animation",
                "--seed",
                seed,
            ])
            .into_app(0, Instant::now());
            let mut app = app;
            let mut now = Instant::now();
            for _ in 0..40 {
                now += crate::app::PASS_DELAY;
                app.update(now);
            }
            app.log.clone()
        };
        assert_eq!(throws("42"), throws("42"));
        assert_ne!(throws("42"), throws("43"));
    }

    #[test]
    fn rejects_unknown_player() {
        let err = Cli::try_parse_from(["ur-game", "--dark", "grandmaster"]).unwrap_err();
        assert!(err.to_string().contains("possible values"));
    }
}

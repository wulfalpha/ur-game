//! Command-line options.

use crate::app::{App, Controller, Time};
use clap::{Parser, ValueEnum};
use std::path::PathBuf;
use ur_game::ai::Difficulty;

/// The Royal Game of Ur, one of the oldest known board games.
#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli {
    /// Play in the terminal instead of a window. This is automatic when
    /// there's no display to open a window on, such as over SSH.
    #[arg(long, conflicts_with = "gui")]
    pub tui: bool,

    /// Open a window even where the terminal would be chosen automatically.
    #[arg(long)]
    pub gui: bool,

    /// Who plays Light, who moves first.
    #[arg(short, long, value_enum, value_name = "PLAYER", default_value_t = Seat::Human)]
    pub light: Seat,

    /// Who plays Dark.
    #[arg(short, long, value_enum, value_name = "PLAYER", default_value_t = Seat::Medium)]
    pub dark: Seat,

    /// Skip the menu and start a new game straight away.
    #[arg(short, long)]
    pub start: bool,

    /// Skip the menu and continue the saved game (or start a new one if
    /// there isn't one). The saved game keeps its own players.
    #[arg(short = 'c', long = "continue", conflicts_with = "start")]
    pub resume: bool,

    /// Don't save the game, and ignore any saved game.
    #[arg(long, conflicts_with = "resume")]
    pub no_save: bool,

    /// Turn off the dice and piece animations.
    #[arg(long)]
    pub no_animation: bool,

    /// Start with sound muted. Press M to toggle sound during play.
    #[arg(long)]
    pub mute: bool,

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
    /// Perfect play. The first time, it takes a few minutes to work the
    /// game out, then it's saved for next time.
    Expert,
}

impl From<Seat> for Controller {
    fn from(seat: Seat) -> Self {
        match seat {
            Seat::Human => Controller::Human,
            Seat::Random => Controller::Computer(Difficulty::Random),
            Seat::Easy => Controller::Computer(Difficulty::Easy),
            Seat::Medium => Controller::Computer(Difficulty::Medium),
            Seat::Hard => Controller::Computer(Difficulty::Hard),
            Seat::Expert => Controller::Computer(Difficulty::Expert),
        }
    }
}

impl Cli {
    /// Whether to play in the terminal rather than open a window.
    pub fn wants_terminal(&self) -> bool {
        if self.tui || self.gui {
            return self.tui;
        }
        !display_available(|name| std::env::var_os(name).is_some_and(|v| !v.is_empty()))
    }

    /// Build the app these options describe. `default_seed` is used when no
    /// `--seed` was given; games are saved to `save_path` unless `--no-save`.
    #[cfg(test)]
    pub fn into_app(self, default_seed: u64, now: Time, save_path: Option<PathBuf>) -> App {
        self.into_app_with_preferences(default_seed, now, save_path, None, None, None)
    }

    pub fn into_app_with_preferences(
        self,
        default_seed: u64,
        now: Time,
        save_path: Option<PathBuf>,
        preferences_path: Option<PathBuf>,
        stats_path: Option<PathBuf>,
        chronicle_dir: Option<PathBuf>,
    ) -> App {
        let mut app = App::new(self.seed.unwrap_or(default_seed));
        if let Some(path) = preferences_path {
            app.use_preferences(path);
        }
        // The leaderboard and shared chronicles are kept even with
        // --no-save, which only affects the game in progress.
        if let Some(path) = stats_path {
            app.use_stats_file(path);
        }
        if let Some(dir) = chronicle_dir {
            app.use_chronicle_dir(dir);
        }
        app.controllers = [self.light.into(), self.dark.into()];
        if self.no_animation {
            app.animations = false;
        }
        if self.mute {
            app.sound_enabled = false;
        }
        if let Some(path) = save_path.filter(|_| !self.no_save) {
            app.use_save_file(path);
        }
        if self.resume {
            if !app.resume(now) {
                app.new_game(now);
            }
        } else if self.start {
            app.new_game(now);
        }
        app
    }
}

/// Can we open a window? Not over SSH (the window would appear on the far
/// machine, if anywhere), and on Linux and the BSDs only with an X11 or
/// Wayland display. `env_set` reports whether an environment variable is set.
fn display_available(env_set: impl Fn(&str) -> bool) -> bool {
    if env_set("SSH_CONNECTION") || env_set("SSH_TTY") {
        return false;
    }
    if cfg!(any(target_os = "macos", target_os = "windows")) {
        return true;
    }
    env_set("DISPLAY") || env_set("WAYLAND_DISPLAY")
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
        let app = parse(&[]).into_app(1, Time::ZERO, None);
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
        let app = cli.into_app(1, Time::ZERO, None);
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
            .into_app(0, Time::ZERO, None);
            let mut app = app;
            let mut now = Time::ZERO;
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
    fn continue_resumes_the_saved_game() {
        let path = std::env::temp_dir().join(format!("ur-game-cli-{}.txt", std::process::id()));
        // First run: a computer-vs-computer game, played a few turns, then
        // abandoned.
        let mut app = parse(&["-l", "easy", "-d", "hard", "-s", "--no-animation"]).into_app(
            9,
            Time::ZERO,
            Some(path.clone()),
        );
        let mut now = Time::ZERO;
        for _ in 0..12 {
            now += crate::app::PASS_DELAY;
            app.update(now);
        }
        let saved = app.resumable.clone().expect("saved during play");

        // Second run, with different players on the command line: the saved
        // game's players win.
        let app = parse(&["-c"]).into_app(1, Time::ZERO, Some(path.clone()));
        assert_eq!(app.screen, Screen::Game);
        assert_eq!(app.game, saved.game);
        assert_eq!(app.controllers, saved.controllers);

        // --no-save ignores it; a plain run offers it in the menu.
        let app = parse(&["--no-save"]).into_app(1, Time::ZERO, Some(path.clone()));
        assert!(app.resumable.is_none());
        let app = parse(&[]).into_app(1, Time::ZERO, Some(path.clone()));
        assert_eq!(app.menu_selected, crate::app::MenuItem::Continue);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn continue_without_a_save_starts_fresh() {
        let path = std::env::temp_dir().join("ur-game-cli-missing-save.txt");
        let app = parse(&["-c"]).into_app(1, Time::ZERO, Some(path));
        assert_eq!(app.screen, Screen::Game);
        assert_eq!(app.game, ur_game::game::GameState::new());
    }

    #[test]
    fn rejects_conflicting_options() {
        assert!(Cli::try_parse_from(["ur-game", "-c", "-s"]).is_err());
        assert!(Cli::try_parse_from(["ur-game", "-c", "--no-save"]).is_err());
    }

    #[test]
    fn chooses_terminal_or_window() {
        let env = |vars: &'static [&'static str]| move |name: &str| vars.contains(&name);
        assert!(!display_available(env(&["DISPLAY", "SSH_CONNECTION"])));
        assert!(!display_available(env(&["WAYLAND_DISPLAY", "SSH_TTY"])));
        if cfg!(target_os = "linux") {
            assert!(display_available(env(&["DISPLAY"])));
            assert!(display_available(env(&["WAYLAND_DISPLAY"])));
            assert!(!display_available(env(&[])));
        }
        assert!(parse(&["--tui"]).wants_terminal());
        assert!(!parse(&["--gui"]).wants_terminal());
        assert!(Cli::try_parse_from(["ur-game", "--tui", "--gui"]).is_err());
    }

    #[test]
    fn rejects_unknown_player() {
        let err = Cli::try_parse_from(["ur-game", "--dark", "grandmaster"]).unwrap_err();
        assert!(err.to_string().contains("possible values"));
    }
}

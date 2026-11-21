//! Presentation preferences, stored separately from the game in progress.
use std::path::{Path, PathBuf};
use std::{fs, io};
use ur_game::game::Rules;

const HEADER: &str = "ur-game preferences 1";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preferences {
    pub sound: bool,
    pub volume: f32,
    pub animations: bool,
    pub show_paths: bool,
    /// The rules new games are played by.
    pub rules: Rules,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            sound: true,
            volume: 0.5,
            animations: true,
            show_paths: false,
            rules: Rules::Finkel,
        }
    }
}

impl Preferences {
    fn parse(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        if lines.next()? != HEADER {
            return None;
        }
        let mut sound = None;
        let mut volume = None;
        let mut animations = None;
        let mut show_paths = false;
        // Files from before Masters had no rules line.
        let mut rules = Rules::Finkel;
        for line in lines {
            let (key, value) = line.split_once(' ')?;
            match key {
                "sound" => sound = Some(value.parse().ok()?),
                "volume" => {
                    let v: f32 = value.parse().ok()?;
                    if !v.is_finite() || !(0.0..=1.0).contains(&v) {
                        return None;
                    }
                    volume = Some(v);
                }
                "animations" => animations = Some(value.parse().ok()?),
                "show-paths" => show_paths = value.parse().ok()?,
                "rules" => {
                    rules = Rules::ALL
                        .into_iter()
                        .find(|r| r.to_string().to_lowercase() == value)?
                }
                _ => return None,
            }
        }
        Some(Self {
            sound: sound?,
            volume: volume?,
            animations: animations?,
            show_paths,
            rules,
        })
    }

    fn text(self) -> String {
        format!(
            "{HEADER}\nsound {}\nvolume {}\nanimations {}\nshow-paths {}\nrules {}\n",
            self.sound,
            self.volume,
            self.animations,
            self.show_paths,
            self.rules.to_string().to_lowercase()
        )
    }
}

pub fn default_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("ur-game").join("preferences.txt"))
}

pub fn load(path: &Path) -> Preferences {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| Preferences::parse(&s))
        .unwrap_or_default()
}

pub fn store(path: &Path, settings: Preferences) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, settings.text())?;
    fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_overrides_are_temporary_and_ui_changes_survive_restart() {
        use crate::{
            app::{App, MenuItem, Time},
            cli::Cli,
        };
        use clap::Parser;
        use ratatui::crossterm::event::{KeyCode, KeyEvent};

        let dir = std::env::temp_dir().join(format!("ur-preferences-{}", std::process::id()));
        let path = dir.join("preferences.txt");
        let original = Preferences::default();
        store(&path, original).unwrap();
        let cli = Cli::parse_from(["ur-game", "--mute", "--no-animation", "--no-save"]);
        let mut app =
            cli.into_app_with_preferences(9, Time::ZERO, None, Some(path.clone()), None, None);
        assert!(!app.sound_enabled && !app.animations);
        assert_eq!(load(&path), original);

        // Changing volume must not accidentally persist the CLI mute or animation overrides.
        app.handle_key(KeyEvent::from(KeyCode::Char('-')), Time::ZERO);
        let saved = load(&path);
        assert!(saved.sound && saved.animations);
        assert!((saved.volume - 0.4).abs() < 0.001);

        // Explicit toggles persist individually.
        app.handle_key(KeyEvent::from(KeyCode::Char('m')), Time::ZERO);
        app.handle_key(KeyEvent::from(KeyCode::Char('m')), Time::ZERO);
        app.menu_selected = MenuItem::Animation;
        app.handle_key(KeyEvent::from(KeyCode::Right), Time::ZERO);
        app.handle_key(KeyEvent::from(KeyCode::Right), Time::ZERO);
        app.screen = crate::app::Screen::Game;
        app.game = ur_game::game::GameState::with_rules(Rules::Masters);
        app.handle_key(KeyEvent::from(KeyCode::Char('p')), Time::ZERO);
        let mut reopened = App::new(10);
        reopened.use_preferences(path.clone());
        assert!(!reopened.sound_enabled && !reopened.animations);
        assert!(reopened.show_paths);
        assert!((reopened.sound_volume - 0.4).abs() < 0.001);

        fs::write(&path, "broken preferences").unwrap();
        assert_eq!(load(&path), Preferences::default());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn round_trip_and_reject_invalid_preferences() {
        let settings = Preferences {
            sound: false,
            volume: 0.3,
            animations: false,
            show_paths: true,
            rules: Rules::Masters,
        };
        assert_eq!(Preferences::parse(&settings.text()), Some(settings));
        let without_paths = settings.text().replace("show-paths true\n", "");
        assert!(!Preferences::parse(&without_paths).unwrap().show_paths);
        // Files from before Masters still load, as Finkel.
        let old = settings.text().replace("rules masters\n", "");
        assert_eq!(
            Preferences::parse(&old).map(|p| p.rules),
            Some(Rules::Finkel)
        );
        for bad in ["NaN", "inf", "-0.1", "1.1", "loud"] {
            assert_eq!(
                Preferences::parse(
                    &settings
                        .text()
                        .replace("volume 0.3", &format!("volume {bad}"))
                ),
                None
            );
        }
        assert_eq!(Preferences::parse("ur-game preferences 99"), None);
        assert_eq!(Preferences::parse(HEADER), None);
        assert_eq!(
            Preferences::parse(&settings.text().replace("sound false", "sound maybe")),
            None
        );
    }
}

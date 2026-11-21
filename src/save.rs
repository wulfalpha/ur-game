//! Saving a game in progress so it can be resumed later.
//!
//! The save is a small text file, written after every turn and every throw
//! of the dice, so closing the terminal (or losing the connection) never
//! costs more than the move being made. Because the throw is saved before
//! the player sees it, quitting and resuming can't buy a better roll.
//!
//! ```text
//! ur-game save 1
//! light human
//! dark medium
//! turn light
//! light-pieces 0 0 0 3 6 15 15
//! dark-pieces 0 0 0 0 0 1 9
//! dice 5
//! tally light 12 25 1 3 0
//! tally dark 11 20 0 2 0
//! decision 3 light 7 0 3 0 0 0 0 0 0 2 0 0 0 0 0 0 0
//! log New game: Light (Human) vs Dark (Computer · Medium).
//! log Light rolls 2: enters a piece on 2.
//! ```
//!
//! `dice` is only present when the dice have been thrown but the throw not
//! yet played; it holds the four tips as bits. `tally` holds each player's
//! throws, sum of throws, captures, rosettes and hints so far (missing in
//! older saves, which then count from zero). Each `decision` is a move
//! choice a human made, for the review at the end: the throw number, the
//! player to move, the dice tips, the move (from, to), then the position as
//! Light's seven piece positions and Dark's.

use crate::app::{Controller, Decision, Tally};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use ur_game::ai::Difficulty;
use ur_game::game::{FINISH, GameState, Move, PIECES_PER_PLAYER, Player, START, roll_from_tips};

const HEADER: &str = "ur-game save 1";

/// Everything needed to pick a game back up.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedGame {
    pub controllers: [Controller; 2],
    pub game: GameState,
    /// Tips of a throw not yet played, if any.
    pub pending_tips: Option<u8>,
    pub log: Vec<String>,
    /// Each player's running totals, Light first.
    pub tally: [Tally; 2],
    /// The humans' move choices so far.
    pub decisions: Vec<Decision>,
}

/// Where the game is saved: the per-user data folder
/// (`~/.local/share/ur-game` on Linux,
/// `~/Library/Application Support/ur-game` on macOS).
pub fn default_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("ur-game").join("save.txt"))
}

/// Read a saved game. A missing, unreadable or corrupt file counts as no
/// save at all.
pub fn load(path: &Path) -> Option<SavedGame> {
    SavedGame::from_text(&fs::read_to_string(path).ok()?)
}

/// Write a saved game, replacing any earlier one.
pub fn store(path: &Path, saved: &SavedGame) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    // Write then rename, so a crash mid-write can't leave half a save.
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, saved.to_text())?;
    fs::rename(tmp, path)
}

/// Delete the save, if there is one.
pub fn remove(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

impl SavedGame {
    pub fn to_text(&self) -> String {
        let mut out = format!("{HEADER}\n");
        for player in Player::ALL {
            let name = player_name(player);
            let controller = controller_name(self.controllers[player.index()]);
            out += &format!("{name} {controller}\n");
        }
        out += &format!("turn {}\n", player_name(self.game.turn()));
        for player in Player::ALL {
            let positions: Vec<String> = positions(&self.game, player)
                .iter()
                .map(u8::to_string)
                .collect();
            out += &format!("{}-pieces {}\n", player_name(player), positions.join(" "));
        }
        if let Some(tips) = self.pending_tips {
            out += &format!("dice {tips}\n");
        }
        for player in Player::ALL {
            let t = self.tally[player.index()];
            out += &format!(
                "tally {} {} {} {} {} {}\n",
                player_name(player),
                t.throws,
                t.pips,
                t.captures,
                t.rosettes,
                t.hints
            );
        }
        for d in &self.decisions {
            let mut fields = vec![
                d.throw.to_string(),
                player_name(d.game.turn()).to_string(),
                d.tips.to_string(),
                d.mv.from.to_string(),
                d.mv.to.to_string(),
            ];
            for player in Player::ALL {
                fields.extend(positions(&d.game, player).iter().map(u8::to_string));
            }
            out += &format!("decision {}\n", fields.join(" "));
        }
        for line in &self.log {
            // Log lines never contain newlines, but don't let one corrupt
            // the file if they ever do.
            out += &format!("log {}\n", line.replace('\n', " "));
        }
        out
    }

    pub fn from_text(text: &str) -> Option<SavedGame> {
        let mut lines = text.lines();
        if lines.next()? != HEADER {
            return None;
        }
        let mut controllers = [None; 2];
        let mut turn = None;
        let mut pieces = [None; 2];
        let mut pending_tips = None;
        let mut log = Vec::new();
        let mut tally = [Tally::default(); 2];
        let mut decisions = Vec::new();
        for line in lines {
            let (key, value) = line.split_once(' ').unwrap_or((line, ""));
            match key {
                "light" | "dark" => {
                    controllers[parse_player(key)?.index()] = Some(parse_controller(value)?)
                }
                "turn" => turn = Some(parse_player(value)?),
                "light-pieces" | "dark-pieces" => {
                    let player = parse_player(key.trim_end_matches("-pieces"))?;
                    pieces[player.index()] = Some(parse_positions(value)?);
                }
                "dice" => pending_tips = Some(value.parse::<u8>().ok().filter(|&t| t < 16)?),
                "tally" => {
                    let (who, numbers) = value.split_once(' ')?;
                    let n: Vec<u32> = numbers
                        .split_whitespace()
                        .map(|x| x.parse().ok())
                        .collect::<Option<_>>()?;
                    let [throws, pips, captures, rosettes, hints] = n[..] else {
                        return None;
                    };
                    tally[parse_player(who)?.index()] = Tally {
                        throws,
                        pips,
                        captures,
                        rosettes,
                        hints,
                    };
                }
                "decision" => decisions.push(parse_decision(value)?),
                "log" => log.push(value.to_string()),
                _ => return None,
            }
        }
        let game = GameState::from_positions(turn?, pieces[0]?, pieces[1]?).ok()?;
        if game.winner().is_some() {
            return None; // nothing left to resume
        }
        Some(SavedGame {
            controllers: [controllers[0]?, controllers[1]?],
            game,
            pending_tips,
            log,
            tally,
            decisions,
        })
    }
}

/// `throw player tips from to` then fourteen piece positions.
fn parse_decision(text: &str) -> Option<Decision> {
    let fields: Vec<&str> = text.split_whitespace().collect();
    let [throw, turn, tips, from, to, rest @ ..] = &fields[..] else {
        return None;
    };
    let n = PIECES_PER_PLAYER as usize;
    if rest.len() != 2 * n {
        return None;
    }
    let pieces = |part: &[&str]| parse_positions(&part.join(" "));
    let game = GameState::from_positions(
        parse_player(turn)?,
        pieces(&rest[..n])?,
        pieces(&rest[n..])?,
    )
    .ok()?;
    let tips: u8 = tips.parse().ok().filter(|&t| t < 16)?;
    let mv = Move {
        from: from.parse().ok()?,
        to: to.parse().ok()?,
    };
    // Only a move that was legal there can be reviewed.
    if !game.legal_moves(roll_from_tips(tips.into())).contains(&mv) {
        return None;
    }
    Some(Decision {
        throw: throw.parse().ok()?,
        game,
        tips,
        mv,
    })
}

/// Every piece's path position (`START` waiting, `FINISH` borne off).
fn positions(game: &GameState, player: Player) -> Vec<u8> {
    let mut out = vec![START; game.waiting(player) as usize];
    out.extend(game.pieces_on_board(player));
    out.extend(std::iter::repeat_n(FINISH, game.finished(player) as usize));
    out
}

fn parse_positions(text: &str) -> Option<[u8; PIECES_PER_PLAYER as usize]> {
    let values: Vec<u8> = text
        .split_whitespace()
        .map(|v| v.parse().ok())
        .collect::<Option<_>>()?;
    values.try_into().ok()
}

fn player_name(player: Player) -> &'static str {
    match player {
        Player::Light => "light",
        Player::Dark => "dark",
    }
}

fn parse_player(text: &str) -> Option<Player> {
    Player::ALL.into_iter().find(|&p| player_name(p) == text)
}

fn controller_name(controller: Controller) -> &'static str {
    match controller {
        Controller::Human => "human",
        Controller::Computer(Difficulty::Random) => "random",
        Controller::Computer(Difficulty::Easy) => "easy",
        Controller::Computer(Difficulty::Medium) => "medium",
        Controller::Computer(Difficulty::Hard) => "hard",
        Controller::Computer(Difficulty::Expert) => "expert",
    }
}

fn parse_controller(text: &str) -> Option<Controller> {
    Controller::CHOICES
        .into_iter()
        .find(|&c| controller_name(c) == text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SavedGame {
        SavedGame {
            controllers: [Controller::Human, Controller::Computer(Difficulty::Hard)],
            game: GameState::from_positions(
                Player::Dark,
                [0, 0, 3, 6, 8, 15, 15],
                [0, 0, 0, 0, 1, 9, 13],
            )
            .unwrap(),
            pending_tips: Some(0b1011),
            log: vec!["New game.".into(), "Light rolls 2: moves 6 → 8.".into()],
            tally: [
                Tally {
                    throws: 12,
                    pips: 25,
                    captures: 1,
                    rosettes: 3,
                    hints: 2,
                },
                Tally {
                    throws: 11,
                    pips: 20,
                    ..Tally::default()
                },
            ],
            decisions: vec![Decision {
                throw: 3,
                game: GameState::from_positions(Player::Light, [0, 0, 0, 0, 0, 0, 2], [0; 7])
                    .unwrap(),
                tips: 0b0111,
                mv: Move { from: 0, to: 3 },
            }],
        }
    }

    #[test]
    fn older_saves_without_tally_still_load() {
        let text: String = sample()
            .to_text()
            .lines()
            .filter(|l| !l.starts_with("tally"))
            .map(|l| format!("{l}\n"))
            .collect();
        let loaded = SavedGame::from_text(&text).unwrap();
        assert_eq!(loaded.tally, [Tally::default(); 2]);
    }

    #[test]
    fn round_trips_through_text() {
        let saved = sample();
        assert_eq!(SavedGame::from_text(&saved.to_text()), Some(saved));
        let no_dice = SavedGame {
            pending_tips: None,
            log: Vec::new(),
            ..sample()
        };
        assert_eq!(SavedGame::from_text(&no_dice.to_text()), Some(no_dice));
    }

    #[test]
    fn rejects_bad_files() {
        let good = sample().to_text();
        for bad in [
            String::new(),
            good.replace(HEADER, "ur-game save 999"),
            good.replace("dark hard", "dark grandmaster"),
            good.replace("turn dark", "turn purple"),
            good.replace("0 0 3 6 8 15 15", "0 0 3 6 8 15"), // six pieces
            good.replace("0 0 3 6 8 15 15", "0 0 3 6 9 15 15"), // collides on 9
            good.replace("dice 11", "dice 16"),
            good.replace("tally light 12 25 1 3 2", "tally light 12 25 1 3"),
            good.replace("tally light 12 25 1 3 2", "tally purple 12 25 1 3 2"),
            good.replace("turn dark\n", ""),
            good.replace("decision 3 light 7 0 3", "decision 3 light 7 0 4"), // illegal move
            good.replace("decision 3 light 7 0 3", "decision 3 light 7 0"),
            good + "nonsense line\n",
        ] {
            assert_eq!(SavedGame::from_text(&bad), None, "accepted:\n{bad}");
        }
    }

    #[test]
    fn finished_games_are_not_resumable() {
        let mut saved = sample();
        saved.game = GameState::from_positions(Player::Dark, [15; 7], [0; 7]).unwrap();
        assert_eq!(SavedGame::from_text(&saved.to_text()), None);
    }

    #[test]
    fn store_load_remove() {
        let dir = std::env::temp_dir().join(format!("ur-game-test-{}", std::process::id()));
        let path = dir.join("nested").join("save.txt");
        assert_eq!(load(&path), None);
        store(&path, &sample()).unwrap();
        assert_eq!(load(&path), Some(sample()));
        remove(&path).unwrap();
        assert_eq!(load(&path), None);
        remove(&path).unwrap(); // removing nothing is fine
        fs::remove_dir_all(dir).unwrap();
    }
}

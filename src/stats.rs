//! The local leaderboard: how a human has done against each computer level,
//! kept separately for each rule set.
//!
//! Only games between one human and one computer player count: finished
//! games, and unfinished ones abandoned for a new game, which count as losses.
//! The record is a small text file next to the saved game:
//!
//! ```text
//! ur-game stats 3
//! easy 12 7 2 4 1043 11 WLLWWAWW
//! hard 3 1 -1 1 290 3 LWL
//! total 15 8 -1 4 1333 14 WWAWWLWLWL
//! masters easy 2 1 1 1 130 2 LW
//! masters total 2 1 1 1 130 2 LW
//! ```
//!
//! Each line is a level (or `total`, across all levels in the order played),
//! then: games played, games won, the current streak (positive for wins in
//! a row, negative for losses), the best winning streak, the throws in all
//! finished games, how many games were finished, and the latest results,
//! oldest first (`W` won, `L` lost, `A` abandoned; `-` for none). Lines for
//! Masters games start with `masters`; the rest are Finkel's, so version 2
//! files (all Finkel) read the same. Version 1 files, with just played and
//! won, still load.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use ur_game::ai::Difficulty;
use ur_game::game::Rules;

const HEADER: &str = "ur-game stats 3";
const HEADER_V2: &str = "ur-game stats 2";
const HEADER_V1: &str = "ur-game stats 1";
/// How many of the latest results are kept.
pub const RECENT: usize = 10;

/// How a game against the computer ended for the human.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Won,
    Lost,
    /// Left unfinished for a new game: a loss.
    Abandoned,
}

impl Outcome {
    fn letter(self) -> char {
        match self {
            Outcome::Won => 'W',
            Outcome::Lost => 'L',
            Outcome::Abandoned => 'A',
        }
    }

    fn from_letter(c: char) -> Option<Outcome> {
        [Outcome::Won, Outcome::Lost, Outcome::Abandoned]
            .into_iter()
            .find(|o| o.letter() == c)
    }
}

/// Games played against one computer level (or all of them).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Record {
    pub played: u32,
    pub won: u32,
    /// Wins in a row so far if positive, losses in a row if negative.
    pub streak: i32,
    /// The longest run of wins.
    pub best_streak: u32,
    /// Throws, both players together, in all finished games.
    throws: u32,
    /// Finished games, which `throws` covers. Abandoned games aren't.
    finished: u32,
    /// The latest results, oldest first, packed at the end; empty slots
    /// (`None`) come first until there have been enough games.
    recent: [Option<Outcome>; RECENT],
}

impl Record {
    pub fn lost(self) -> u32 {
        self.played - self.won
    }

    /// Win percentage, rounded; `None` before the first game.
    pub fn win_percent(self) -> Option<u32> {
        (self.played > 0).then(|| (self.won * 100 + self.played / 2) / self.played)
    }

    /// Average length of a finished game, in throws by both players.
    pub fn average_throws(self) -> Option<f64> {
        (self.finished > 0).then(|| f64::from(self.throws) / f64::from(self.finished))
    }

    /// The latest results, oldest first.
    pub fn recent(&self) -> impl Iterator<Item = Outcome> + '_ {
        self.recent.iter().flatten().copied()
    }

    fn add(&mut self, outcome: Outcome, throws: Option<u32>) {
        self.played += 1;
        if outcome == Outcome::Won {
            self.won += 1;
            self.streak = self.streak.max(0) + 1;
            self.best_streak = self.best_streak.max(self.streak as u32);
        } else {
            self.streak = self.streak.min(0) - 1;
        }
        if let Some(n) = throws {
            self.throws += n;
            self.finished += 1;
        }
        // Drops the oldest result, or an empty slot while there's room.
        self.recent.rotate_left(1);
        self.recent[RECENT - 1] = Some(outcome);
    }

    fn to_text(self) -> String {
        let recent: String = self.recent().map(Outcome::letter).collect();
        format!(
            "{} {} {} {} {} {} {}",
            self.played,
            self.won,
            self.streak,
            self.best_streak,
            self.throws,
            self.finished,
            if recent.is_empty() { "-" } else { &recent }
        )
    }

    /// The fields after the name: version 1's two, or version 2's seven.
    fn from_fields(fields: &[&str]) -> Option<Record> {
        let number = |i: usize| fields.get(i)?.parse::<u32>().ok();
        let mut r = Record {
            played: number(0)?,
            won: number(1)?,
            ..Record::default()
        };
        match fields.len() {
            2 => {}
            7 => {
                r.streak = fields[2].parse().ok()?;
                r.best_streak = number(3)?;
                r.throws = number(4)?;
                r.finished = number(5)?;
                let letters: Vec<Outcome> = match fields[6] {
                    "-" => Vec::new(),
                    s => s.chars().map(Outcome::from_letter).collect::<Option<_>>()?,
                };
                if letters.len() > RECENT || letters.len() as u32 > r.played {
                    return None;
                }
                for (slot, o) in r.recent[RECENT - letters.len()..].iter_mut().zip(letters) {
                    *slot = Some(o);
                }
            }
            _ => return None,
        }
        let consistent = r.won <= r.played
            && r.best_streak <= r.won
            && r.streak.unsigned_abs() <= r.played
            && r.finished <= r.played;
        consistent.then_some(r)
    }
}

/// One [`Record`] per computer level, in [`Difficulty::ALL`] order, and one
/// across them all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Leaderboard {
    records: [Record; Difficulty::ALL.len()],
    overall: Record,
}

impl Leaderboard {
    pub fn record(&self, level: Difficulty) -> Record {
        self.records[level as usize]
    }

    /// Count a game against `level`, with its length in throws if it was
    /// played to the end.
    pub fn add(&mut self, level: Difficulty, outcome: Outcome, throws: Option<u32>) {
        self.records[level as usize].add(outcome, throws);
        self.overall.add(outcome, throws);
    }

    /// Every level together. Its streaks and latest results run across
    /// levels, in the order the games were played.
    pub fn total(&self) -> Record {
        self.overall
    }

    /// This board's lines, each starting with `prefix`.
    fn to_text(self, prefix: &str) -> String {
        let mut out = String::new();
        for level in Difficulty::ALL {
            let r = self.record(level);
            if r.played > 0 {
                out += &format!("{prefix}{} {}\n", key(level), r.to_text());
            }
        }
        if self.overall.played > 0 {
            out += &format!("{prefix}total {}\n", self.overall.to_text());
        }
        out
    }

    /// Version 1 files have no total line: add the levels up, without
    /// streaks, which can't be rebuilt across levels.
    fn sum_levels(&self) -> Record {
        self.records.iter().fold(Record::default(), |t, r| Record {
            played: t.played + r.played,
            won: t.won + r.won,
            throws: t.throws + r.throws,
            finished: t.finished + r.finished,
            ..Record::default()
        })
    }
}

/// A [`Leaderboard`] for each rule set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Leaderboards {
    boards: [Leaderboard; Rules::ALL.len()],
}

impl Leaderboards {
    pub fn board(&self, rules: Rules) -> &Leaderboard {
        &self.boards[rules as usize]
    }

    pub fn record(&self, rules: Rules, level: Difficulty) -> Record {
        self.board(rules).record(level)
    }

    pub fn total(&self, rules: Rules) -> Record {
        self.board(rules).total()
    }

    /// Count a game under `rules` against `level`, with its length in
    /// throws if it was played to the end.
    pub fn add(&mut self, rules: Rules, level: Difficulty, outcome: Outcome, throws: Option<u32>) {
        self.boards[rules as usize].add(level, outcome, throws);
    }

    fn to_text(self) -> String {
        let mut out = format!("{HEADER}\n");
        for rules in Rules::ALL {
            out += &self.board(rules).to_text(prefix(rules));
        }
        out
    }

    fn from_text(text: &str) -> Option<Leaderboards> {
        let mut lines = text.lines();
        let header = lines.next()?;
        if ![HEADER, HEADER_V2, HEADER_V1].contains(&header) {
            return None;
        }
        let mut boards = Leaderboards::default();
        let mut totals = [None; Rules::ALL.len()];
        for line in lines.filter(|l| !l.trim().is_empty()) {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let (rules, fields) = match fields.split_first() {
                Some((&"masters", rest)) if header == HEADER => (Rules::Masters, rest),
                _ => (Rules::Finkel, &fields[..]),
            };
            let (name, rest) = fields.split_first()?;
            if header == HEADER_V1 && rest.len() != 2 {
                return None;
            }
            let record = Record::from_fields(rest)?;
            if *name == "total" {
                totals[rules as usize] = Some(record);
            } else {
                let level = Difficulty::ALL.into_iter().find(|&d| key(d) == *name)?;
                boards.boards[rules as usize].records[level as usize] = record;
            }
        }
        for (board, total) in boards.boards.iter_mut().zip(totals) {
            board.overall = total.unwrap_or_else(|| board.sum_levels());
        }
        Some(boards)
    }
}

/// What a rule set's lines start with in the file.
fn prefix(rules: Rules) -> &'static str {
    match rules {
        Rules::Finkel => "",
        Rules::Masters => "masters ",
    }
}

fn key(level: Difficulty) -> String {
    level.to_string().to_lowercase()
}

/// Where the leaderboard lives: beside the saved game.
pub fn default_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("ur-game").join("stats.txt"))
}

/// A missing or unreadable file is an empty leaderboard.
pub fn load(path: &Path) -> Leaderboards {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| Leaderboards::from_text(&s))
        .unwrap_or_default()
}

pub fn store(path: &Path, board: &Leaderboards) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, board.to_text())
}

#[cfg(test)]
mod tests {
    use super::*;
    use Outcome::*;

    #[test]
    fn records_and_percentages() {
        let mut b = Leaderboard::default();
        assert_eq!(b.record(Difficulty::Easy).win_percent(), None);
        assert_eq!(b.record(Difficulty::Easy).average_throws(), None);
        b.add(Difficulty::Easy, Won, Some(40));
        b.add(Difficulty::Easy, Lost, Some(50));
        b.add(Difficulty::Easy, Won, Some(60));
        b.add(Difficulty::Hard, Abandoned, None);
        let easy = b.record(Difficulty::Easy);
        assert_eq!((easy.played, easy.won, easy.lost()), (3, 2, 1));
        assert_eq!(easy.win_percent(), Some(67));
        assert_eq!(easy.average_throws(), Some(50.0));
        let total = b.total();
        assert_eq!((total.played, total.won), (4, 2));
        assert_eq!(total.win_percent(), Some(50));
        // The abandoned game doesn't drag the average length down.
        assert_eq!(total.average_throws(), Some(50.0));
    }

    #[test]
    fn streaks_and_recent_results() {
        let mut b = Leaderboard::default();
        for o in [Won, Won, Lost, Won, Won, Won, Abandoned, Lost] {
            b.add(Difficulty::Medium, o, None);
        }
        let r = b.record(Difficulty::Medium);
        assert_eq!(r.streak, -2); // abandoned counts as a loss
        assert_eq!(r.best_streak, 3);
        assert_eq!(
            r.recent().collect::<Vec<_>>(),
            [Won, Won, Lost, Won, Won, Won, Abandoned, Lost]
        );
        // Only the latest ten are kept, oldest first.
        for _ in 0..5 {
            b.add(Difficulty::Medium, Won, None);
        }
        let r = b.record(Difficulty::Medium);
        assert_eq!((r.streak, r.best_streak), (5, 5));
        assert_eq!(
            r.recent().collect::<Vec<_>>(),
            [Won, Won, Won, Abandoned, Lost, Won, Won, Won, Won, Won]
        );
        // The total runs across levels in the order played.
        b.add(Difficulty::Easy, Lost, None);
        assert_eq!(b.total().streak, -1);
        assert_eq!(b.total().recent().last(), Some(Lost));
        assert_eq!(b.record(Difficulty::Medium).streak, 5);
    }

    #[test]
    fn text_round_trip_and_rejects_garbage() {
        let mut b = Leaderboards::default();
        b.add(Rules::Finkel, Difficulty::Medium, Won, Some(44));
        b.add(Rules::Finkel, Difficulty::Random, Lost, Some(51));
        b.add(Rules::Finkel, Difficulty::Random, Abandoned, None);
        b.add(Rules::Masters, Difficulty::Expert, Lost, Some(70));
        let text = b.to_text();
        assert!(
            text.contains("\nmasters expert 1 0 -1 0 70 1 L\n"),
            "{text}"
        );
        assert!(text.contains("\nmasters total "), "{text}");
        assert_eq!(Leaderboards::from_text(&text), Some(b));
        // The rule sets are kept apart.
        assert_eq!(b.total(Rules::Finkel).played, 3);
        assert_eq!(b.total(Rules::Masters).played, 1);
        let empty = Leaderboards::default();
        assert_eq!(Leaderboards::from_text(&empty.to_text()), Some(empty));
        for bad in [
            "",
            "ur-game stats 2\neasy 2 3 0 0 0 0 -\n", // more won than played
            "ur-game stats 2\nmaster 2 1 1 1 9 1 W\n", // no such level
            "ur-game stats 2\neasy 2 1 1 2 9 1 W\n", // best streak over wins
            "ur-game stats 2\neasy 2 1 1 1 9 1 WX\n", // unknown result
            "ur-game stats 2\neasy 1 1 1 1 9 1 WW\n", // more results than games
            "ur-game stats 2\neasy 2 1 1 1 9\n",     // missing a field
            "ur-game stats 1\neasy 2 1 1 1 9 1 W\n", // new fields, old header
            "ur-game stats 2\nmasters easy 2 1 1 1 9 1 W\n", // no Masters in v2
        ] {
            assert_eq!(Leaderboards::from_text(bad), None, "accepted {bad:?}");
        }
    }

    #[test]
    fn older_files_still_load_as_finkel() {
        let mut b = Leaderboards::from_text("ur-game stats 1\neasy 12 7\nhard 3 0\n").unwrap();
        let easy = b.record(Rules::Finkel, Difficulty::Easy);
        assert_eq!((easy.played, easy.won, easy.streak), (12, 7, 0));
        assert_eq!(easy.recent().count(), 0);
        assert_eq!(easy.average_throws(), None);
        let total = b.total(Rules::Finkel);
        assert_eq!((total.played, total.won), (15, 7));
        assert_eq!(b.total(Rules::Masters).played, 0);
        // …and carry on from there in the new format.
        b.add(Rules::Finkel, Difficulty::Easy, Won, Some(48));
        assert!(
            b.to_text()
                .starts_with("ur-game stats 3\neasy 13 8 1 1 48 1 W\n")
        );
        let v2 = Leaderboards::from_text("ur-game stats 2\nhard 3 1 -1 1 290 3 LWL\n").unwrap();
        assert_eq!(v2.record(Rules::Finkel, Difficulty::Hard).played, 3);
    }

    #[test]
    fn store_and_load() {
        let dir = std::env::temp_dir().join(format!("ur-stats-{}", std::process::id()));
        let path = dir.join("stats.txt");
        let mut b = Leaderboards::default();
        b.add(Rules::Masters, Difficulty::Hard, Won, Some(39));
        store(&path, &b).unwrap();
        assert_eq!(load(&path), b);
        fs::remove_dir_all(dir).unwrap();
        assert_eq!(load(&path), Leaderboards::default());
    }
}

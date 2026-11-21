//! The local leaderboard: how a human has done against each computer level.
//!
//! Only games between one human and one computer player count: finished
//! games, and unfinished ones abandoned for a new game, which count as losses.
//! The record is a small text file next to the saved game:
//!
//! ```text
//! ur-game stats 1
//! easy 12 7
//! hard 3 1
//! ```
//!
//! Each line is a level, the games played against it, and the games won.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use ur_game::ai::Difficulty;

const HEADER: &str = "ur-game stats 1";

/// Games played and won against one computer level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Record {
    pub played: u32,
    pub won: u32,
}

impl Record {
    pub fn lost(self) -> u32 {
        self.played - self.won
    }

    /// Win percentage, rounded; `None` before the first game.
    pub fn win_percent(self) -> Option<u32> {
        (self.played > 0).then(|| (self.won * 100 + self.played / 2) / self.played)
    }
}

/// One [`Record`] per computer level, in [`Difficulty::ALL`] order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Leaderboard {
    records: [Record; Difficulty::ALL.len()],
}

impl Leaderboard {
    pub fn record(&self, level: Difficulty) -> Record {
        self.records[level as usize]
    }

    pub fn add(&mut self, level: Difficulty, won: bool) {
        let r = &mut self.records[level as usize];
        r.played += 1;
        r.won += u32::from(won);
    }

    pub fn total(&self) -> Record {
        self.records.iter().fold(Record::default(), |t, r| Record {
            played: t.played + r.played,
            won: t.won + r.won,
        })
    }

    fn to_text(self) -> String {
        let mut out = format!("{HEADER}\n");
        for level in Difficulty::ALL {
            let r = self.record(level);
            if r.played > 0 {
                out += &format!("{} {} {}\n", key(level), r.played, r.won);
            }
        }
        out
    }

    fn from_text(text: &str) -> Option<Leaderboard> {
        let mut lines = text.lines();
        if lines.next()? != HEADER {
            return None;
        }
        let mut board = Leaderboard::default();
        for line in lines.filter(|l| !l.trim().is_empty()) {
            let mut parts = line.split_whitespace();
            let name = parts.next()?;
            let level = Difficulty::ALL.into_iter().find(|&d| key(d) == name)?;
            let played: u32 = parts.next()?.parse().ok()?;
            let won: u32 = parts.next()?.parse().ok()?;
            if won > played || parts.next().is_some() {
                return None;
            }
            board.records[level as usize] = Record { played, won };
        }
        Some(board)
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
pub fn load(path: &Path) -> Leaderboard {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| Leaderboard::from_text(&s))
        .unwrap_or_default()
}

pub fn store(path: &Path, board: &Leaderboard) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, board.to_text())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_percentages() {
        let mut b = Leaderboard::default();
        assert_eq!(b.record(Difficulty::Easy).win_percent(), None);
        b.add(Difficulty::Easy, true);
        b.add(Difficulty::Easy, false);
        b.add(Difficulty::Easy, true);
        b.add(Difficulty::Hard, false);
        let easy = b.record(Difficulty::Easy);
        assert_eq!((easy.played, easy.won, easy.lost()), (3, 2, 1));
        assert_eq!(easy.win_percent(), Some(67));
        assert_eq!(b.total(), Record { played: 4, won: 2 });
        assert_eq!(b.total().win_percent(), Some(50));
    }

    #[test]
    fn text_round_trip_and_rejects_garbage() {
        let mut b = Leaderboard::default();
        b.add(Difficulty::Medium, true);
        b.add(Difficulty::Random, false);
        assert_eq!(Leaderboard::from_text(&b.to_text()), Some(b));
        assert_eq!(Leaderboard::from_text(""), None);
        assert_eq!(Leaderboard::from_text("ur-game stats 1\neasy 2 3\n"), None);
        assert_eq!(
            Leaderboard::from_text("ur-game stats 1\nexpert 2 1\n"),
            None
        );
    }

    #[test]
    fn store_and_load() {
        let dir = std::env::temp_dir().join(format!("ur-stats-{}", std::process::id()));
        let path = dir.join("stats.txt");
        let mut b = Leaderboard::default();
        b.add(Difficulty::Hard, true);
        store(&path, &b).unwrap();
        assert_eq!(load(&path), b);
        fs::remove_dir_all(dir).unwrap();
        assert_eq!(load(&path), Leaderboard::default());
    }
}

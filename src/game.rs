//! Rules engine for the Royal Game of Ur, in two rule sets:
//!
//! - [`Rules::Finkel`]: Irving Finkel's reconstruction from the Babylonian
//!   tablet of Itti-Marduk-balāṭu (177 BC), the rules most sets come with.
//! - [`Rules::Masters`]: James Masters' rules, as played on RoyalUr.net: a
//!   longer route that crosses the far end of the board, three dice, and no
//!   safe squares.
//!
//! This module is pure game logic: no I/O, no terminal, no randomness beyond
//! an injected RNG. Both front ends drive it through
//! [`GameState::legal_moves`] and [`GameState::apply`].
//!
//! # Path positions
//!
//! Each player's piece position is a number along *their own* route:
//!
//! | position | Finkel                  | Masters                         |
//! |----------|-------------------------|---------------------------------|
//! | `0`      | waiting to enter        | waiting to enter                |
//! | `1..=4`  | own starting lane       | own starting lane               |
//! | `5..`    | `5..=12` the middle row (captures happen here) | `5..=16` the middle row, then round the far end (captures happen anywhere here) |
//! | then     | `13..=14` own finishing lane | –                          |
//! | finish   | `15`: borne off         | `17`: borne off                 |
//!
//! Both players walk the middle row in the same direction, so a shared
//! position is usually the *same physical square* for both. The exception
//! is the far end in Masters, where the routes cross in opposite directions:
//! see [`Rules::opposite`].

use rand::Rng;
use std::fmt;
use std::ops::RangeInclusive;

/// Pieces each player starts with.
pub const PIECES_PER_PLAYER: u8 = 7;
/// Pseudo-position for a piece that has not entered the board.
pub const START: u8 = 0;

/// Grid dimensions of the physical board (3 rows × 8 columns, with gaps).
pub const ROWS: usize = 3;
pub const COLS: usize = 8;

/// Which rules a game is played by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Rules {
    #[default]
    Finkel,
    Masters,
}

impl Rules {
    pub const ALL: [Rules; 2] = [Rules::Finkel, Rules::Masters];

    /// Number of squares on a player's route.
    pub fn path_len(self) -> u8 {
        match self {
            Rules::Finkel => 14,
            Rules::Masters => 16,
        }
    }

    /// Pseudo-position for a piece that has been borne off.
    pub fn finish(self) -> u8 {
        self.path_len() + 1
    }

    /// Path positions that are rosettes (landing grants another turn).
    pub fn rosettes(self) -> &'static [u8] {
        match self {
            Rules::Finkel => &[4, 8, 14],
            Rules::Masters => &[4, 8, 12, 16],
        }
    }

    pub fn is_rosette(self, pos: u8) -> bool {
        self.rosettes().contains(&pos)
    }

    /// The square where a piece can't be captured: the central rosette in
    /// Finkel's rules. Masters has none.
    pub fn safe_square(self) -> Option<u8> {
        match self {
            Rules::Finkel => Some(8),
            Rules::Masters => None,
        }
    }

    pub fn is_safe(self, pos: u8) -> bool {
        self.safe_square() == Some(pos)
    }

    /// Path positions both players use, where captures happen.
    pub fn shared(self) -> RangeInclusive<u8> {
        match self {
            Rules::Finkel => 5..=12,
            Rules::Masters => 5..=16,
        }
    }

    pub fn is_shared(self, pos: u8) -> bool {
        self.shared().contains(&pos)
    }

    /// The opponent's path position for the same physical square as shared
    /// position `pos`. The same number, except at the far end in Masters,
    /// where the routes run in opposite directions (12 ↔ 16, 13 ↔ 15).
    pub fn opposite(self, pos: u8) -> u8 {
        match self {
            Rules::Masters if pos >= 12 => 28 - pos,
            _ => pos,
        }
    }

    /// How many dice are thrown.
    pub fn dice(self) -> u8 {
        match self {
            Rules::Finkel => 4,
            Rules::Masters => 3,
        }
    }

    /// How many of 16 equally likely chances give each throw `0..=4`.
    /// Finkel: four binary dice. Masters: three, with no marks counting as
    /// 4 (so each eighth is two sixteenths here).
    pub fn roll_ways(self) -> [u8; 5] {
        match self {
            Rules::Finkel => [1, 4, 6, 4, 1],
            Rules::Masters => [0, 6, 6, 2, 2],
        }
    }

    /// The chance of each throw `0..=4`.
    pub fn roll_probabilities(self) -> [f64; 5] {
        self.roll_ways().map(|w| f64::from(w) / 16.0)
    }

    /// Turn random bits into a throw: bit `i` set means die `i` came up
    /// marked. Exposed so a UI can show the individual dice.
    pub fn roll_from_tips(self, bits: u32) -> u8 {
        let marked = (bits & ((1 << self.dice()) - 1)).count_ones() as u8;
        match self {
            Rules::Masters if marked == 0 => 4,
            _ => marked,
        }
    }

    /// Throw the dice.
    pub fn roll_dice<R: Rng + ?Sized>(self, rng: &mut R) -> u8 {
        self.roll_from_tips(rng.next_u32())
    }

    /// Grid cell `(row, col)` of a player's path position.
    pub fn cell_of(self, player: Player, pos: u8) -> Option<(usize, usize)> {
        let (own, other) = match player {
            Player::Light => (0, 2),
            Player::Dark => (2, 0),
        };
        let p = pos as usize;
        match (self, pos) {
            (_, 1..=4) => Some((own, 4 - p)),
            (Rules::Finkel, 5..=12) => Some((1, p - 5)),
            (Rules::Finkel, 13) => Some((own, 7)),
            (Rules::Finkel, 14) => Some((own, 6)),
            (Rules::Masters, 5..=11) => Some((1, p - 5)),
            (Rules::Masters, 12) => Some((other, 6)),
            (Rules::Masters, 13) => Some((other, 7)),
            (Rules::Masters, 14) => Some((1, 7)),
            (Rules::Masters, 15) => Some((own, 7)),
            (Rules::Masters, 16) => Some((own, 6)),
            _ => None,
        }
    }

    /// Inverse of [`cell_of`](Self::cell_of): which path position (if any)
    /// the cell is for `player`. Lets a GUI map a click back to a piece.
    pub fn path_position(self, player: Player, row: usize, col: usize) -> Option<u8> {
        (1..=self.path_len()).find(|&p| self.cell_of(player, p) == Some((row, col)))
    }
}

impl fmt::Display for Rules {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Rules::Finkel => "Finkel",
            Rules::Masters => "Masters",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Player {
    /// Plays along the top row; moves first.
    Light,
    /// Plays along the bottom row.
    Dark,
}

impl Player {
    pub const ALL: [Player; 2] = [Player::Light, Player::Dark];

    pub fn opponent(self) -> Player {
        match self {
            Player::Light => Player::Dark,
            Player::Dark => Player::Light,
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

impl fmt::Display for Player {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Player::Light => "Light",
            Player::Dark => "Dark",
        })
    }
}

/// A move for the player whose turn it is: one piece from path position
/// `from` to `to`. `from == START` enters a new piece; `to` equal to the
/// rules' [`finish`](Rules::finish) bears a piece off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Move {
    pub from: u8,
    pub to: u8,
}

impl Move {
    pub fn is_entry(self) -> bool {
        self.from == START
    }
}

/// What happened as a result of a move — useful for UI messages, animation
/// and AI evaluation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Outcome {
    /// An opponent piece was sent back to its start.
    pub captured: bool,
    /// The mover landed on a rosette and moves again.
    pub extra_turn: bool,
    /// The move ended the game.
    pub winner: Option<Player>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveError {
    GameOver,
    /// The move is not among the legal moves for this roll.
    Illegal,
    /// Tried to pass while a legal move exists.
    MustMove,
}

impl fmt::Display for MoveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            MoveError::GameOver => "the game is over",
            MoveError::Illegal => "that move is not legal for this roll",
            MoveError::MustMove => "cannot pass while a legal move exists",
        })
    }
}

impl std::error::Error for MoveError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PositionError {
    /// A position was past the finish.
    OutOfRange,
    /// Two pieces of the same player share a board square.
    Stacked,
    /// Pieces of both players share a square.
    Collision,
}

/// The legal moves in a position, held inline. There are at most seven:
/// one entry (only while a piece waits, so at most six are on the board)
/// plus one per piece on the board.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MoveList {
    moves: [Move; PIECES_PER_PLAYER as usize],
    len: usize,
}

impl Default for MoveList {
    fn default() -> Self {
        Self {
            moves: [Move { from: 0, to: 0 }; PIECES_PER_PLAYER as usize],
            len: 0,
        }
    }
}

impl MoveList {
    fn push(&mut self, mv: Move) {
        self.moves[self.len] = mv;
        self.len += 1;
    }
}

impl std::ops::Deref for MoveList {
    type Target = [Move];
    fn deref(&self) -> &[Move] {
        &self.moves[..self.len]
    }
}

/// Complete game state. Small and `Copy` so the AI can clone it freely.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GameState {
    /// Per player: bit `p` set ⇔ that player has a piece on path position `p`.
    board: [u32; 2],
    /// Per player: pieces not yet entered.
    waiting: [u8; 2],
    /// Per player: pieces borne off.
    finished: [u8; 2],
    turn: Player,
    rules: Rules,
}

impl Default for GameState {
    fn default() -> Self {
        Self::new()
    }
}

impl GameState {
    /// A fresh game under Finkel's rules; Light moves first.
    pub fn new() -> Self {
        Self::with_rules(Rules::Finkel)
    }

    /// A fresh game under `rules`; Light moves first.
    pub fn with_rules(rules: Rules) -> Self {
        GameState {
            board: [0; 2],
            waiting: [PIECES_PER_PLAYER; 2],
            finished: [0; 2],
            turn: Player::Light,
            rules,
        }
    }

    /// Build an arbitrary Finkel position from each player's seven piece
    /// positions (`START`, on the path, or finished). Handy for tests.
    pub fn from_positions(
        turn: Player,
        light: [u8; PIECES_PER_PLAYER as usize],
        dark: [u8; PIECES_PER_PLAYER as usize],
    ) -> Result<Self, PositionError> {
        Self::from_positions_in(Rules::Finkel, turn, light, dark)
    }

    /// [`from_positions`](Self::from_positions) under any rules. For
    /// puzzles, saves and tests.
    pub fn from_positions_in(
        rules: Rules,
        turn: Player,
        light: [u8; PIECES_PER_PLAYER as usize],
        dark: [u8; PIECES_PER_PLAYER as usize],
    ) -> Result<Self, PositionError> {
        let mut state = GameState {
            board: [0; 2],
            waiting: [0; 2],
            finished: [0; 2],
            turn,
            rules,
        };
        let finish = rules.finish();
        for (player, positions) in [(Player::Light, light), (Player::Dark, dark)] {
            let i = player.index();
            for pos in positions {
                match pos {
                    START => state.waiting[i] += 1,
                    p if p == finish => state.finished[i] += 1,
                    p if p > finish => return Err(PositionError::OutOfRange),
                    p => {
                        if state.board[i] & bit(p) != 0 {
                            return Err(PositionError::Stacked);
                        }
                        state.board[i] |= bit(p);
                    }
                }
            }
        }
        if state
            .pieces_on_board(Player::Light)
            .any(|p| state.enemy_on(Player::Light, p))
        {
            return Err(PositionError::Collision);
        }
        Ok(state)
    }

    pub fn rules(&self) -> Rules {
        self.rules
    }

    pub fn turn(&self) -> Player {
        self.turn
    }

    pub fn waiting(&self, player: Player) -> u8 {
        self.waiting[player.index()]
    }

    pub fn finished(&self, player: Player) -> u8 {
        self.finished[player.index()]
    }

    /// Does `mv` bear a piece off?
    pub fn is_bear_off(&self, mv: Move) -> bool {
        mv.to == self.rules.finish()
    }

    /// `player`'s pieces on the board: bit `p` set for path position `p`.
    pub(crate) fn board_bits(&self, player: Player) -> u32 {
        self.board[player.index()]
    }

    /// Does `player` have a piece on their path position `pos`?
    pub fn occupies(&self, player: Player, pos: u8) -> bool {
        (1..=self.rules.path_len()).contains(&pos) && self.board[player.index()] & bit(pos) != 0
    }

    /// Does `player`'s opponent have a piece on the square that is
    /// `player`'s path position `pos`?
    pub fn enemy_on(&self, player: Player, pos: u8) -> bool {
        self.rules.is_shared(pos) && self.occupies(player.opponent(), self.rules.opposite(pos))
    }

    /// How many of 16 equal chances would let `victim`'s opponent capture
    /// the piece on `pos` with their next throw. Zero if `victim` has no
    /// piece there, off the shared squares, and on a safe square.
    pub fn capture_ways(&self, victim: Player, pos: u8) -> u8 {
        let rules = self.rules;
        if !rules.is_shared(pos) || rules.is_safe(pos) || !self.occupies(victim, pos) {
            return 0;
        }
        let hunter = victim.opponent();
        let target = rules.opposite(pos);
        let ways = rules.roll_ways();
        (1..=4u8)
            .filter(|&d| target > d && self.occupies(hunter, target - d))
            .map(|d| ways[d as usize])
            .sum()
    }

    /// Path positions of `player`'s pieces on the board, ascending.
    pub fn pieces_on_board(&self, player: Player) -> impl Iterator<Item = u8> + '_ {
        let mask = self.board[player.index()];
        (1..=self.rules.path_len()).filter(move |&p| mask & bit(p) != 0)
    }

    /// Who occupies the physical cell at grid `(row, col)`, if anyone.
    pub fn piece_at_cell(&self, row: usize, col: usize) -> Option<Player> {
        Player::ALL.into_iter().find(|&player| {
            self.rules
                .path_position(player, row, col)
                .is_some_and(|pos| self.occupies(player, pos))
        })
    }

    /// The whole state packed into 48 bits, for use as a hash key.
    pub(crate) fn packed(&self) -> u64 {
        u64::from(self.board[0])
            | u64::from(self.board[1]) << 17
            | u64::from(self.waiting[0]) << 34
            | u64::from(self.waiting[1]) << 37
            | u64::from(self.finished[0]) << 40
            | u64::from(self.finished[1]) << 43
            | (self.turn.index() as u64) << 46
            | (self.rules as u64) << 47
    }

    pub fn winner(&self) -> Option<Player> {
        Player::ALL
            .into_iter()
            .find(|&p| self.finished(p) == PIECES_PER_PLAYER)
    }

    /// All legal moves for the current player with this roll. Empty means the
    /// player must [`pass`](Self::pass) (always the case on a roll of 0).
    /// Entering pieces are deduplicated into a single move.
    pub fn legal_moves(&self, roll: u8) -> Vec<Move> {
        self.move_list(roll).to_vec()
    }

    /// [`legal_moves`](Self::legal_moves) without allocating, for the AI.
    pub(crate) fn move_list(&self, roll: u8) -> MoveList {
        let mut moves = MoveList::default();
        if roll == 0 || self.winner().is_some() {
            return moves;
        }
        let me = self.turn.index();
        let finish = self.rules.finish();
        let mut consider = |from: u8| {
            let to = from + roll;
            let legal = match to {
                t if t > finish => false, // bearing off needs an exact roll
                t if t == finish => true,
                t if self.board[me] & bit(t) != 0 => false, // own piece there
                t if self.rules.is_safe(t) => !self.enemy_on(self.turn, t),
                _ => true,
            };
            if legal {
                moves.push(Move { from, to });
            }
        };
        if self.waiting[me] > 0 {
            consider(START);
        }
        for from in 1..=self.rules.path_len() {
            if self.board[me] & bit(from) != 0 {
                consider(from);
            }
        }
        moves
    }

    /// Play `mv` for the current player, after checking it is legal for `roll`.
    pub fn apply(&mut self, roll: u8, mv: Move) -> Result<Outcome, MoveError> {
        if self.winner().is_some() {
            return Err(MoveError::GameOver);
        }
        if !self.legal_moves(roll).contains(&mv) {
            return Err(MoveError::Illegal);
        }
        Ok(self.apply_unchecked(mv))
    }

    /// End the turn without moving. Only allowed when no legal move exists.
    pub fn pass(&mut self, roll: u8) -> Result<(), MoveError> {
        if self.winner().is_some() {
            return Err(MoveError::GameOver);
        }
        if !self.legal_moves(roll).is_empty() {
            return Err(MoveError::MustMove);
        }
        self.pass_unchecked();
        Ok(())
    }

    /// Hand the turn to the opponent without checking for legal moves.
    pub(crate) fn pass_unchecked(&mut self) {
        self.turn = self.turn.opponent();
    }

    /// Play a move known to come from [`legal_moves`](Self::legal_moves).
    /// Skips validation — for the AI's search loop.
    pub(crate) fn apply_unchecked(&mut self, mv: Move) -> Outcome {
        let mover = self.turn;
        let me = mover.index();
        let opp = mover.opponent().index();
        let mut outcome = Outcome::default();

        if mv.is_entry() {
            self.waiting[me] -= 1;
        } else {
            self.board[me] &= !bit(mv.from);
        }

        if self.is_bear_off(mv) {
            self.finished[me] += 1;
        } else {
            self.board[me] |= bit(mv.to);
            if self.enemy_on(mover, mv.to) {
                self.board[opp] &= !bit(self.rules.opposite(mv.to));
                self.waiting[opp] += 1;
                outcome.captured = true;
            }
            outcome.extra_turn = self.rules.is_rosette(mv.to);
        }

        if self.finished[me] == PIECES_PER_PLAYER {
            outcome.winner = Some(mover);
            outcome.extra_turn = false;
        } else if !outcome.extra_turn {
            self.turn = mover.opponent();
        }
        outcome
    }
}

fn bit(pos: u8) -> u32 {
    1 << pos
}

// ---------------------------------------------------------------------------
// Physical board geometry — shared by every front end.
//
//   col:  0   1   2   3   4   5   6   7
//  row 0 [*] [ ] [ ] [ ]         [*] [ ]   Light's side
//  row 1 [ ] [ ] [ ] [*] [ ] [ ] [ ] [ ]   middle row
//  row 2 [*] [ ] [ ] [ ]         [*] [ ]   Dark's side
//
// Finkel: own row cols 3→0 (1..=4), middle cols 0→7 (5..=12), own row
// cols 7→6 (13..=14), then off the board.
// Masters: own row cols 3→0 (1..=4), middle cols 0→6 (5..=11), the
// opponent's row cols 6→7 (12..=13), middle col 7 (14), own row cols 7→6
// (15..=16), then off the board.
// ---------------------------------------------------------------------------

/// Is `(row, col)` an actual square (the board has two gaps)?
pub fn is_board_cell(row: usize, col: usize) -> bool {
    row < ROWS && col < COLS && (row == 1 || !(4..=5).contains(&col))
}

/// Is `(row, col)` a rosette square? The same five under both rule sets.
pub fn is_rosette_cell(row: usize, col: usize) -> bool {
    matches!((row, col), (0 | 2, 0 | 6) | (1, 3))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    const S: u8 = START;
    const F: u8 = 15;

    fn pos(turn: Player, light: [u8; 7], dark: [u8; 7]) -> GameState {
        GameState::from_positions(turn, light, dark).expect("valid test position")
    }

    fn mv(from: u8, to: u8) -> Move {
        Move { from, to }
    }

    #[test]
    fn new_game_setup() {
        let g = GameState::new();
        assert_eq!(g.turn(), Player::Light);
        for p in Player::ALL {
            assert_eq!(g.waiting(p), 7);
            assert_eq!(g.finished(p), 0);
            assert_eq!(g.pieces_on_board(p).count(), 0);
        }
        assert_eq!(g.winner(), None);
    }

    #[test]
    fn capture_ways_counts_hunters_behind() {
        // Light on 9; Dark on 7 hits it with a 2 (6 ways), on 6 with a 3 (4).
        let g =
            GameState::from_positions(Player::Dark, [9, 0, 0, 0, 0, 0, 0], [7, 6, 0, 0, 0, 0, 0])
                .unwrap();
        assert_eq!(g.capture_ways(Player::Light, 9), 10);
        assert_eq!(g.capture_ways(Player::Dark, 7), 0); // nothing behind it
        assert_eq!(g.capture_ways(Player::Light, 10), 0); // no piece there
        // A piece on its own row can hit the start of the shared row.
        let g =
            GameState::from_positions(Player::Dark, [5, 0, 0, 0, 0, 0, 0], [4, 0, 0, 0, 0, 0, 0])
                .unwrap();
        assert_eq!(g.capture_ways(Player::Light, 5), 4);
        // Safe on the middle rosette and on your own row.
        let g =
            GameState::from_positions(Player::Dark, [8, 3, 0, 0, 0, 0, 0], [7, 6, 2, 0, 0, 0, 0])
                .unwrap();
        assert_eq!(g.capture_ways(Player::Light, 8), 0);
        assert_eq!(g.capture_ways(Player::Light, 3), 0);
    }

    #[test]
    fn roll_distribution() {
        for rules in Rules::ALL {
            let odds = rules.roll_probabilities();
            assert!((odds.iter().sum::<f64>() - 1.0).abs() < 1e-12);
            // Exhaustive over every combination of the dice matches the odds.
            let combos = 1u32 << rules.dice();
            let mut counts = [0u32; 5];
            for bits in 0..combos {
                counts[rules.roll_from_tips(bits) as usize] += 16 / combos;
            }
            assert_eq!(counts.map(|c| c as u8), rules.roll_ways(), "{rules}");
            let mut rng = StdRng::seed_from_u64(7);
            assert!((0..1000).all(|_| rules.roll_dice(&mut rng) <= 4));
        }
        // Masters never throws 0: no marks moves 4.
        assert_eq!(Rules::Masters.roll_from_tips(0), 4);
        assert_eq!(Rules::Masters.roll_from_tips(0b1000), 4, "only three dice");
    }

    #[test]
    fn zero_roll_means_pass() {
        let mut g = GameState::new();
        assert!(g.legal_moves(0).is_empty());
        g.pass(0).unwrap();
        assert_eq!(g.turn(), Player::Dark);
    }

    #[test]
    fn cannot_pass_with_a_legal_move() {
        let mut g = GameState::new();
        assert_eq!(g.pass(2), Err(MoveError::MustMove));
    }

    #[test]
    fn entering_pieces_is_one_move() {
        let g = GameState::new();
        assert_eq!(g.legal_moves(2), vec![mv(S, 2)]);
    }

    #[test]
    fn entering_on_rosette_grants_extra_turn() {
        let mut g = GameState::new();
        let out = g.apply(4, mv(S, 4)).unwrap();
        assert!(out.extra_turn);
        assert_eq!(g.turn(), Player::Light);
        assert!(g.occupies(Player::Light, 4));
        assert_eq!(g.waiting(Player::Light), 6);
    }

    #[test]
    fn normal_move_passes_turn() {
        let mut g = GameState::new();
        let out = g.apply(3, mv(S, 3)).unwrap();
        assert_eq!(out, Outcome::default());
        assert_eq!(g.turn(), Player::Dark);
    }

    #[test]
    fn cannot_land_on_own_piece_but_can_jump_it() {
        let g = pos(Player::Light, [2, 3, S, S, S, S, S], [S; 7]);
        // Roll 1: 2→3 blocked, 3→4 ok, enter →1 ok.
        assert_eq!(g.legal_moves(1), vec![mv(S, 1), mv(3, 4)]);
        // Roll 2: entering onto 2 is blocked; 2 jumps over 3 to 4.
        assert_eq!(g.legal_moves(2), vec![mv(2, 4), mv(3, 5)]);
    }

    #[test]
    fn capture_in_shared_lane() {
        let mut g = pos(Player::Light, [5, S, S, S, S, S, S], [7, S, S, S, S, S, S]);
        let out = g.apply(2, mv(5, 7)).unwrap();
        assert!(out.captured);
        assert!(!g.occupies(Player::Dark, 7));
        assert_eq!(g.waiting(Player::Dark), 7);
        assert!(g.occupies(Player::Light, 7));
        assert_eq!(g.turn(), Player::Dark);
    }

    #[test]
    fn private_lanes_do_not_interact() {
        // Both players at path 3 are on different physical squares.
        let mut g = pos(Player::Light, [1, S, S, S, S, S, S], [3, S, S, S, S, S, S]);
        let out = g.apply(2, mv(1, 3)).unwrap();
        assert!(!out.captured);
        assert!(g.occupies(Player::Dark, 3));
        assert!(g.occupies(Player::Light, 3));
        // Same for the finishing lane.
        let g = pos(
            Player::Light,
            [12, S, S, S, S, S, S],
            [13, S, S, S, S, S, S],
        );
        assert!(g.legal_moves(1).contains(&mv(12, 13)));
    }

    #[test]
    fn central_rosette_is_safe() {
        let g = pos(Player::Light, [6, S, S, S, S, S, S], [8, S, S, S, S, S, S]);
        assert!(!g.legal_moves(2).contains(&mv(6, 8)));
        // ...but can be jumped over.
        assert!(g.legal_moves(3).contains(&mv(6, 9)));
    }

    #[test]
    fn central_rosette_capture_impossible_but_extra_turn_works() {
        let mut g = pos(Player::Dark, [S; 7], [6, S, S, S, S, S, S]);
        let out = g.apply(2, mv(6, 8)).unwrap();
        assert!(out.extra_turn);
        assert_eq!(g.turn(), Player::Dark);
    }

    #[test]
    fn bear_off_needs_exact_roll() {
        let g = pos(Player::Light, [14, F, F, F, F, F, F], [S; 7]);
        assert_eq!(g.legal_moves(1), vec![mv(14, F)]);
        assert!(g.legal_moves(2).is_empty());
        let g = pos(Player::Light, [12, F, F, F, F, F, S], [S; 7]);
        assert!(g.legal_moves(3).contains(&mv(12, F)));
        assert!(!g.legal_moves(4).iter().any(|m| m.from == 12));
    }

    #[test]
    fn last_square_rosette_then_bear_off() {
        let mut g = pos(Player::Light, [12, S, S, S, S, S, S], [S; 7]);
        let out = g.apply(2, mv(12, 14)).unwrap();
        assert!(out.extra_turn);
        let out = g.apply(1, mv(14, F)).unwrap();
        assert!(!out.extra_turn);
        assert_eq!(g.finished(Player::Light), 1);
    }

    #[test]
    fn winning_ends_the_game() {
        let mut g = pos(Player::Dark, [S; 7], [F, F, F, F, F, F, 13]);
        let out = g.apply(2, mv(13, F)).unwrap();
        assert_eq!(out.winner, Some(Player::Dark));
        assert_eq!(g.winner(), Some(Player::Dark));
        assert!(g.legal_moves(2).is_empty());
        assert_eq!(g.apply(1, mv(S, 1)), Err(MoveError::GameOver));
        assert_eq!(g.pass(0), Err(MoveError::GameOver));
    }

    #[test]
    fn illegal_moves_rejected() {
        let mut g = GameState::new();
        assert_eq!(g.apply(2, mv(S, 3)), Err(MoveError::Illegal));
        assert_eq!(g.apply(2, mv(5, 7)), Err(MoveError::Illegal));
        assert_eq!(g, GameState::new());
    }

    #[test]
    fn invalid_positions_rejected() {
        use PositionError::*;
        assert_eq!(
            GameState::from_positions(Player::Light, [3, 3, S, S, S, S, S], [S; 7]),
            Err(Stacked)
        );
        assert_eq!(
            GameState::from_positions(Player::Light, [9, S, S, S, S, S, S], [9, S, S, S, S, S, S]),
            Err(Collision)
        );
        assert_eq!(
            GameState::from_positions(Player::Light, [16, S, S, S, S, S, S], [S; 7]),
            Err(OutOfRange)
        );
        // Same path number in private lanes is fine.
        assert!(
            GameState::from_positions(
                Player::Light,
                [2, 14, S, S, S, S, S],
                [2, 14, S, S, S, S, S]
            )
            .is_ok()
        );
    }

    #[test]
    fn geometry_matches_the_physical_board() {
        // 20 squares in total.
        let cells = (0..ROWS).flat_map(|r| (0..COLS).map(move |c| (r, c)));
        assert_eq!(
            cells.clone().filter(|&(r, c)| is_board_cell(r, c)).count(),
            20
        );
        // 5 rosettes: four corners-ish plus the centre.
        let rosettes: Vec<_> = cells.filter(|&(r, c)| is_rosette_cell(r, c)).collect();
        assert_eq!(rosettes, vec![(0, 0), (0, 6), (1, 3), (2, 0), (2, 6)]);
        for rules in Rules::ALL {
            for p in Player::ALL {
                for pos in 1..=rules.path_len() {
                    let (r, c) = rules.cell_of(p, pos).unwrap();
                    assert!(is_board_cell(r, c));
                    assert_eq!(rules.path_position(p, r, c), Some(pos));
                    assert_eq!(
                        rules.is_rosette(pos),
                        is_rosette_cell(r, c),
                        "{rules} {pos}"
                    );
                }
                assert_eq!(rules.cell_of(p, START), None);
                assert_eq!(rules.cell_of(p, rules.finish()), None);
            }
            // Shared squares are the same physical square for both players.
            for pos in rules.shared() {
                let other = rules.opposite(pos);
                assert_eq!(rules.opposite(other), pos);
                assert_eq!(
                    rules.cell_of(Player::Light, pos),
                    rules.cell_of(Player::Dark, other)
                );
            }
        }
        assert_eq!(Rules::Finkel.cell_of(Player::Light, 1), Some((0, 3)));
        assert_eq!(Rules::Finkel.cell_of(Player::Dark, 14), Some((2, 6)));
        // Masters' route goes round the far end through Dark's corner.
        assert_eq!(Rules::Masters.cell_of(Player::Light, 12), Some((2, 6)));
        assert_eq!(Rules::Masters.cell_of(Player::Light, 14), Some((1, 7)));
        assert_eq!(Rules::Masters.cell_of(Player::Light, 16), Some((0, 6)));
    }

    fn masters(turn: Player, light: [u8; 7], dark: [u8; 7]) -> GameState {
        GameState::from_positions_in(Rules::Masters, turn, light, dark).expect("valid position")
    }

    #[test]
    fn masters_rosettes_are_not_safe_but_still_give_a_throw() {
        // Light captures Dark on the central rosette, and throws again.
        let mut g = masters(Player::Light, [6, S, S, S, S, S, S], [8, S, S, S, S, S, S]);
        let out = g.apply(2, mv(6, 8)).unwrap();
        assert!(out.captured && out.extra_turn);
        assert_eq!(g.waiting(Player::Dark), 7);
        assert_eq!(g.turn(), Player::Light);
    }

    #[test]
    fn masters_routes_cross_at_the_far_end() {
        // Light's 12 is Dark's 16 (Dark's corner rosette), Light's 13 is
        // Dark's 15, and both use 14.
        let mut g = masters(
            Player::Light,
            [10, S, S, S, S, S, S],
            [16, S, S, S, S, S, S],
        );
        assert_eq!(
            g.capture_ways(Player::Dark, 16),
            6,
            "Light hits it with a 2"
        );
        let out = g.apply(2, mv(10, 12)).unwrap();
        assert!(out.captured);
        assert!(!g.occupies(Player::Dark, 16));
        // Pieces can't share a square there.
        assert_eq!(
            GameState::from_positions_in(
                Rules::Masters,
                Player::Light,
                [13, S, S, S, S, S, S],
                [15, S, S, S, S, S, S]
            ),
            Err(PositionError::Collision)
        );
        // Dark on its 13 is in Light's way at Light's 15.
        let g = masters(
            Player::Light,
            [14, S, S, S, S, S, S],
            [13, S, S, S, S, S, S],
        );
        assert!(g.enemy_on(Player::Light, 15));
        assert!(!g.enemy_on(Player::Light, 13), "Light's 13 is Dark's 15");
    }

    #[test]
    fn masters_bears_off_from_16_with_an_exact_throw() {
        let g = masters(Player::Light, [16, 17, 17, 17, 17, 17, 17], [S; 7]);
        assert_eq!(g.legal_moves(1), vec![mv(16, 17)]);
        assert!(g.legal_moves(2).is_empty());
        assert!(g.is_bear_off(mv(16, 17)));
        assert!(!GameState::new().is_bear_off(mv(16, 17)));
    }

    #[test]
    fn piece_at_cell_reports_owner() {
        let g = pos(Player::Light, [1, 9, S, S, S, S, S], [1, S, S, S, S, S, S]);
        assert_eq!(g.piece_at_cell(0, 3), Some(Player::Light));
        assert_eq!(g.piece_at_cell(2, 3), Some(Player::Dark));
        assert_eq!(g.piece_at_cell(1, 4), Some(Player::Light));
        assert_eq!(g.piece_at_cell(1, 0), None);
    }

    /// Play many random games: pieces are conserved, every game terminates,
    /// and both sides can win.
    #[test]
    fn random_playouts_hold_invariants() {
        for rules in Rules::ALL {
            playouts(rules);
        }
    }

    fn playouts(rules: Rules) {
        let mut rng = StdRng::seed_from_u64(0xC0FFEE);
        let mut wins = [0u32; 2];
        for _ in 0..2000 {
            let mut g = GameState::with_rules(rules);
            let mut turns = 0;
            while g.winner().is_none() {
                turns += 1;
                assert!(turns < 10_000, "game did not terminate");
                let roll = rules.roll_dice(&mut rng);
                let moves = g.legal_moves(roll);
                if moves.is_empty() {
                    g.pass(roll).unwrap();
                } else {
                    let m = moves[(rng.next_u32() as usize) % moves.len()];
                    g.apply(roll, m).unwrap();
                }
                for p in Player::ALL {
                    let total = g.waiting(p) + g.finished(p) + g.pieces_on_board(p).count() as u8;
                    assert_eq!(total, PIECES_PER_PLAYER);
                    for pos in g.pieces_on_board(p) {
                        assert!(!g.enemy_on(p, pos), "two pieces on one square");
                    }
                }
            }
            wins[g.winner().unwrap().index()] += 1;
        }
        assert!(wins[0] > 0 && wins[1] > 0);
    }
}

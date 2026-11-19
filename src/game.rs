//! Rules engine for the Royal Game of Ur, following Irving Finkel's
//! reconstruction from the Babylonian tablet of Itti-Marduk-balāṭu (177 BC).
//!
//! This module is pure game logic: no I/O, no terminal, no randomness beyond
//! an injected RNG. Both the TUI and a future GUI drive it through
//! [`GameState::legal_moves`] and [`GameState::apply`].
//!
//! # Path positions
//!
//! Each player's piece position is a number along *their own* route:
//!
//! | position | meaning                                         |
//! |----------|-------------------------------------------------|
//! | `0`      | waiting to enter (not yet on the board)         |
//! | `1..=4`  | private starting lane                           |
//! | `5..=12` | shared middle lane (captures happen here)       |
//! | `13..=14`| private finishing lane                          |
//! | `15`     | borne off — finished                            |
//!
//! Both players walk the shared lane in the same direction, so path position
//! `p` in `5..=12` is the *same physical square* for both players.

use rand::Rng;
use std::fmt;

/// Pieces each player starts with.
pub const PIECES_PER_PLAYER: u8 = 7;
/// Number of squares on a player's route.
pub const PATH_LEN: u8 = 14;
/// Pseudo-position for a piece that has not entered the board.
pub const START: u8 = 0;
/// Pseudo-position for a piece that has been borne off.
pub const FINISH: u8 = PATH_LEN + 1;
/// Path positions that are rosettes (landing grants another turn).
pub const ROSETTES: [u8; 3] = [4, 8, 14];
/// The central rosette: a piece here cannot be captured.
pub const SAFE_ROSETTE: u8 = 8;
/// First and last path positions of the shared lane.
pub const SHARED_FIRST: u8 = 5;
pub const SHARED_LAST: u8 = 12;

/// Grid dimensions of the physical board (3 rows × 8 columns, with gaps).
pub const ROWS: usize = 3;
pub const COLS: usize = 8;

/// Probability of each roll `0..=4` with four binary tetrahedral dice
/// (binomial with n = 4, p = ½).
pub const ROLL_PROBABILITIES: [f64; 5] =
    [1.0 / 16.0, 4.0 / 16.0, 6.0 / 16.0, 4.0 / 16.0, 1.0 / 16.0];

pub fn is_rosette(pos: u8) -> bool {
    ROSETTES.contains(&pos)
}

pub fn is_shared(pos: u8) -> bool {
    (SHARED_FIRST..=SHARED_LAST).contains(&pos)
}

/// Roll the four tetrahedral dice: each shows a marked tip with probability ½.
pub fn roll_dice<R: Rng + ?Sized>(rng: &mut R) -> u8 {
    roll_from_tips(rng.next_u32())
}

/// Turn four random bits into a roll. Exposed so a UI can show which of the
/// individual dice came up marked (bit `i` set = die `i` marked).
pub fn roll_from_tips(bits: u32) -> u8 {
    (bits & 0b1111).count_ones() as u8
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
/// `from` to `to`. `from == START` enters a new piece; `to == FINISH` bears
/// a piece off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Move {
    pub from: u8,
    pub to: u8,
}

impl Move {
    pub fn is_entry(self) -> bool {
        self.from == START
    }

    pub fn is_bear_off(self) -> bool {
        self.to == FINISH
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
    /// A position was greater than [`FINISH`].
    OutOfRange,
    /// Two pieces of the same player share a board square.
    Stacked,
    /// Pieces of both players share a square in the middle lane.
    Collision,
}

/// Complete game state. Small and `Copy` so the AI can clone it freely.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GameState {
    /// Per player: bit `p` set ⇔ that player has a piece on path position `p`
    /// (`1..=14`).
    board: [u16; 2],
    /// Per player: pieces not yet entered.
    waiting: [u8; 2],
    /// Per player: pieces borne off.
    finished: [u8; 2],
    turn: Player,
}

impl Default for GameState {
    fn default() -> Self {
        Self::new()
    }
}

impl GameState {
    /// A fresh game; Light moves first.
    pub fn new() -> Self {
        GameState {
            board: [0; 2],
            waiting: [PIECES_PER_PLAYER; 2],
            finished: [0; 2],
            turn: Player::Light,
        }
    }

    /// Build an arbitrary position from each player's seven piece positions
    /// (`START`, `1..=14`, or `FINISH`). Handy for tests, puzzles and saves.
    pub fn from_positions(
        turn: Player,
        light: [u8; PIECES_PER_PLAYER as usize],
        dark: [u8; PIECES_PER_PLAYER as usize],
    ) -> Result<Self, PositionError> {
        let mut state = GameState {
            board: [0; 2],
            waiting: [0; 2],
            finished: [0; 2],
            turn,
        };
        for (player, positions) in [(Player::Light, light), (Player::Dark, dark)] {
            let i = player.index();
            for pos in positions {
                match pos {
                    START => state.waiting[i] += 1,
                    FINISH => state.finished[i] += 1,
                    p if p > FINISH => return Err(PositionError::OutOfRange),
                    p => {
                        if state.board[i] & bit(p) != 0 {
                            return Err(PositionError::Stacked);
                        }
                        state.board[i] |= bit(p);
                    }
                }
            }
        }
        let shared_mask = (SHARED_FIRST..=SHARED_LAST).fold(0, |m, p| m | bit(p));
        if state.board[0] & state.board[1] & shared_mask != 0 {
            return Err(PositionError::Collision);
        }
        Ok(state)
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

    /// Does `player` have a piece on path position `pos` (`1..=14`)?
    pub fn occupies(&self, player: Player, pos: u8) -> bool {
        (1..=PATH_LEN).contains(&pos) && self.board[player.index()] & bit(pos) != 0
    }

    /// Path positions (`1..=14`) of `player`'s pieces on the board, ascending.
    pub fn pieces_on_board(&self, player: Player) -> impl Iterator<Item = u8> + '_ {
        let mask = self.board[player.index()];
        (1..=PATH_LEN).filter(move |&p| mask & bit(p) != 0)
    }

    /// Who occupies the physical cell at grid `(row, col)`, if anyone.
    pub fn piece_at_cell(&self, row: usize, col: usize) -> Option<Player> {
        Player::ALL.into_iter().find(|&player| {
            path_position(player, row, col).is_some_and(|pos| self.occupies(player, pos))
        })
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
        let mut moves = Vec::new();
        if roll == 0 || self.winner().is_some() {
            return moves;
        }
        let me = self.turn.index();
        let opp = self.turn.opponent().index();
        let mut consider = |from: u8| {
            let to = from + roll;
            let legal = match to {
                t if t > FINISH => false, // bearing off needs an exact roll
                FINISH => true,
                t if self.board[me] & bit(t) != 0 => false, // own piece there
                SAFE_ROSETTE => self.board[opp] & bit(SAFE_ROSETTE) == 0,
                _ => true,
            };
            if legal {
                moves.push(Move { from, to });
            }
        };
        if self.waiting[me] > 0 {
            consider(START);
        }
        for from in 1..=PATH_LEN {
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

        if mv.is_bear_off() {
            self.finished[me] += 1;
        } else {
            self.board[me] |= bit(mv.to);
            if is_shared(mv.to) && self.board[opp] & bit(mv.to) != 0 {
                self.board[opp] &= !bit(mv.to);
                self.waiting[opp] += 1;
                outcome.captured = true;
            }
            outcome.extra_turn = is_rosette(mv.to);
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

fn bit(pos: u8) -> u16 {
    1 << pos
}

// ---------------------------------------------------------------------------
// Physical board geometry — shared by every front end.
//
//   col:  0   1   2   3   4   5   6   7
//  row 0 [*] [ ] [ ] [ ]         [*] [ ]   Light's private lane
//  row 1 [ ] [ ] [ ] [*] [ ] [ ] [ ] [ ]   shared lane
//  row 2 [*] [ ] [ ] [ ]         [*] [ ]   Dark's private lane
//
// Route: own row cols 3→0 (1..=4), middle cols 0→7 (5..=12),
// own row cols 7→6 (13..=14), then off the board.
// ---------------------------------------------------------------------------

/// Grid cell `(row, col)` of a player's path position `1..=14`.
pub fn cell_of(player: Player, pos: u8) -> Option<(usize, usize)> {
    let own_row = match player {
        Player::Light => 0,
        Player::Dark => 2,
    };
    let p = pos as usize;
    match pos {
        1..=4 => Some((own_row, 4 - p)),
        5..=12 => Some((1, p - 5)),
        13 => Some((own_row, 7)),
        14 => Some((own_row, 6)),
        _ => None,
    }
}

/// Inverse of [`cell_of`]: which path position (if any) the cell is for
/// `player`. Lets a GUI map a click back to a piece.
pub fn path_position(player: Player, row: usize, col: usize) -> Option<u8> {
    (1..=PATH_LEN).find(|&p| cell_of(player, p) == Some((row, col)))
}

/// Is `(row, col)` an actual square (the board has two gaps)?
pub fn is_board_cell(row: usize, col: usize) -> bool {
    row < ROWS && col < COLS && (row == 1 || !(4..=5).contains(&col))
}

/// Is `(row, col)` a rosette square?
pub fn is_rosette_cell(row: usize, col: usize) -> bool {
    Player::ALL
        .into_iter()
        .any(|p| path_position(p, row, col).is_some_and(is_rosette))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    const S: u8 = START;
    const F: u8 = FINISH;

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
    fn roll_distribution() {
        let total: f64 = ROLL_PROBABILITIES.iter().sum();
        assert!((total - 1.0).abs() < 1e-12);
        // Exhaustive over all 16 tip combinations matches the table.
        let mut counts = [0u32; 5];
        for bits in 0..16 {
            counts[roll_from_tips(bits) as usize] += 1;
        }
        assert_eq!(counts, [1, 4, 6, 4, 1]);
        let mut rng = StdRng::seed_from_u64(7);
        assert!((0..1000).all(|_| roll_dice(&mut rng) <= 4));
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
        for p in Player::ALL {
            for pos in 1..=PATH_LEN {
                let (r, c) = cell_of(p, pos).unwrap();
                assert!(is_board_cell(r, c));
                assert_eq!(path_position(p, r, c), Some(pos));
            }
            assert_eq!(cell_of(p, START), None);
            assert_eq!(cell_of(p, FINISH), None);
        }
        // Shared lane is the same physical square for both players.
        for pos in SHARED_FIRST..=SHARED_LAST {
            assert_eq!(cell_of(Player::Light, pos), cell_of(Player::Dark, pos));
        }
        assert_eq!(cell_of(Player::Light, 1), Some((0, 3)));
        assert_eq!(cell_of(Player::Dark, 14), Some((2, 6)));
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
        let mut rng = StdRng::seed_from_u64(0xC0FFEE);
        let mut wins = [0u32; 2];
        for _ in 0..2000 {
            let mut g = GameState::new();
            let mut turns = 0;
            while g.winner().is_none() {
                turns += 1;
                assert!(turns < 10_000, "game did not terminate");
                let roll = roll_dice(&mut rng);
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
                }
            }
            wins[g.winner().unwrap().index()] += 1;
        }
        assert!(wins[0] > 0 && wins[1] > 0);
    }
}

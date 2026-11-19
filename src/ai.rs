//! Computer opponents.
//!
//! Every difficulty answers the same question: given a position and a roll,
//! which legal move should the current player make?
//!
//! - [`Difficulty::Random`]: picks any legal move. A sparring dummy.
//! - [`Difficulty::Easy`]: looks one move ahead with a simple evaluation.
//! - [`Difficulty::Medium`]: expectiminimax, 2 plies.
//! - [`Difficulty::Hard`]: expectiminimax, 4 plies.
//!
//! Expectiminimax is minimax with *chance nodes* for the dice: at the
//! player's own decisions it maximises, at the opponent's it minimises, and
//! between decisions it averages over the five possible rolls, weighted by
//! [`ROLL_PROBABILITIES`]. Rosette extra turns need no special handling:
//! a decision node simply checks whose turn it is.

use crate::game::{
    FINISH, GameState, Move, PATH_LEN, Player, ROLL_PROBABILITIES, SAFE_ROSETTE, is_shared,
};
use rand::Rng;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Difficulty {
    Random,
    Easy,
    Medium,
    Hard,
}

impl Difficulty {
    pub const ALL: [Difficulty; 4] = [
        Difficulty::Random,
        Difficulty::Easy,
        Difficulty::Medium,
        Difficulty::Hard,
    ];

    /// Search depth in decision plies (`None` for random play).
    fn depth(self) -> Option<u32> {
        match self {
            Difficulty::Random => None,
            Difficulty::Easy => Some(1),
            Difficulty::Medium => Some(2),
            Difficulty::Hard => Some(4),
        }
    }
}

impl fmt::Display for Difficulty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Difficulty::Random => "Random",
            Difficulty::Easy => "Easy",
            Difficulty::Medium => "Medium",
            Difficulty::Hard => "Hard",
        })
    }
}

/// Choose a move for the player to move in `state` with this `roll`.
/// Returns `None` when there is no legal move (the player must pass).
///
/// Non-random difficulties are deterministic: ties go to the first move in
/// [`GameState::legal_moves`] order. `rng` is only used by `Random`.
pub fn choose_move<R: Rng + ?Sized>(
    state: &GameState,
    roll: u8,
    difficulty: Difficulty,
    rng: &mut R,
) -> Option<Move> {
    let moves = state.legal_moves(roll);
    match (moves.len(), difficulty.depth()) {
        (0, _) => None,
        (1, _) => Some(moves[0]),
        (n, None) => Some(moves[rng.next_u32() as usize % n]),
        (_, Some(depth)) => {
            let me = state.turn();
            best_by_score(&moves, |&mv| {
                let mut next = *state;
                next.apply_unchecked(mv);
                expectation(&next, me, depth - 1)
            })
        }
    }
}

/// Score each of the moves (from the mover's point of view) — useful for a
/// "hint" feature or showing the AI's reasoning.
pub fn rank_moves(state: &GameState, roll: u8, depth: u32) -> Vec<(Move, f64)> {
    let me = state.turn();
    state
        .legal_moves(roll)
        .into_iter()
        .map(|mv| {
            let mut next = *state;
            next.apply_unchecked(mv);
            (mv, expectation(&next, me, depth.saturating_sub(1)))
        })
        .collect()
}

fn best_by_score(moves: &[Move], mut score: impl FnMut(&Move) -> f64) -> Option<Move> {
    let mut best: Option<(Move, f64)> = None;
    for mv in moves {
        let s = score(mv);
        if best.is_none_or(|(_, b)| s > b) {
            best = Some((*mv, s));
        }
    }
    best.map(|(mv, _)| mv)
}

/// Chance node: expected value (for `me`) before the next roll.
fn expectation(state: &GameState, me: Player, depth: u32) -> f64 {
    if depth == 0 || state.winner().is_some() {
        return evaluate(state, me);
    }
    ROLL_PROBABILITIES
        .iter()
        .enumerate()
        .map(|(roll, p)| p * decision(state, roll as u8, me, depth))
        .sum()
}

/// Decision node: the player to move picks their best move for this roll.
fn decision(state: &GameState, roll: u8, me: Player, depth: u32) -> f64 {
    let moves = state.legal_moves(roll);
    if moves.is_empty() {
        let mut next = *state;
        next.pass_unchecked();
        return expectation(&next, me, depth - 1);
    }
    let maximise = state.turn() == me;
    let values = moves.iter().map(|&mv| {
        let mut next = *state;
        next.apply_unchecked(mv);
        expectation(&next, me, depth - 1)
    });
    if maximise {
        values.fold(f64::NEG_INFINITY, f64::max)
    } else {
        values.fold(f64::INFINITY, f64::min)
    }
}

// ---------------------------------------------------------------------------
// Static evaluation
// ---------------------------------------------------------------------------

const WIN: f64 = 10_000.0;
/// A borne-off piece is worth a bit more than one on the last square: it can
/// never be lost and no longer needs an exact roll.
const FINISHED_VALUE: f64 = PATH_LEN as f64 + 4.0;
/// Bonus for holding the central rosette: safe, and it blocks the opponent.
const SAFE_ROSETTE_BONUS: f64 = 3.0;
/// Value of being the player to move (roughly one average roll of progress).
const TEMPO: f64 = 2.0;
/// How much a threat counts when its owner moves next vs. when it doesn't
/// (the threatened player gets a chance to run first).
const THREAT_WEIGHT_TO_MOVE: f64 = 1.0;
const THREAT_WEIGHT_WAITING: f64 = 0.4;

/// Heuristic value of `state` from `me`'s point of view (higher is better).
pub fn evaluate(state: &GameState, me: Player) -> f64 {
    let opp = me.opponent();
    match state.winner() {
        Some(w) if w == me => return WIN,
        Some(_) => return -WIN,
        None => {}
    }
    let mut score = material(state, me) - material(state, opp);
    let (my_weight, opp_weight) = if state.turn() == me {
        (THREAT_WEIGHT_TO_MOVE, THREAT_WEIGHT_WAITING)
    } else {
        (THREAT_WEIGHT_WAITING, THREAT_WEIGHT_TO_MOVE)
    };
    // My threats against them minus their threats against me.
    score += my_weight * exposure(state, opp) - opp_weight * exposure(state, me);
    score += if state.turn() == me { TEMPO } else { -TEMPO };
    score
}

/// Progress made by `player`'s pieces.
fn material(state: &GameState, player: Player) -> f64 {
    let mut v = state.finished(player) as f64 * FINISHED_VALUE;
    for pos in state.pieces_on_board(player) {
        v += pos as f64;
        if pos == SAFE_ROSETTE {
            v += SAFE_ROSETTE_BONUS;
        }
    }
    v
}

/// Expected progress `victim` would lose if their opponent moved now: for each
/// exposed piece, the chance the opponent rolls a distance that hits it,
/// times the progress that piece has made.
fn exposure(state: &GameState, victim: Player) -> f64 {
    let hunter = victim.opponent();
    state
        .pieces_on_board(victim)
        .filter(|&pos| is_shared(pos) && pos != SAFE_ROSETTE)
        .map(|pos| {
            let hit_chance: f64 = (1..=4u8)
                .filter(|&d| pos > d && state.occupies(hunter, pos - d))
                .map(|d| ROLL_PROBABILITIES[d as usize])
                .sum();
            hit_chance * pos as f64
        })
        .sum()
}

// Compile-time sanity: the evaluation assumes a 14-square path.
const _: () = assert!(FINISH == 15);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{START as S, roll_dice};
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    const F: u8 = FINISH;

    fn pos(turn: Player, light: [u8; 7], dark: [u8; 7]) -> GameState {
        GameState::from_positions(turn, light, dark).unwrap()
    }

    fn pick(state: &GameState, roll: u8, d: Difficulty) -> Option<Move> {
        choose_move(state, roll, d, &mut StdRng::seed_from_u64(1))
    }

    const SMART: [Difficulty; 3] = [Difficulty::Easy, Difficulty::Medium, Difficulty::Hard];

    #[test]
    fn no_moves_means_none() {
        for d in Difficulty::ALL {
            assert_eq!(pick(&GameState::new(), 0, d), None);
        }
    }

    #[test]
    fn always_returns_a_legal_move() {
        let mut rng = StdRng::seed_from_u64(3);
        let mut g = GameState::new();
        for _ in 0..200 {
            if g.winner().is_some() {
                g = GameState::new();
            }
            let roll = roll_dice(&mut rng);
            let legal = g.legal_moves(roll);
            for d in Difficulty::ALL {
                let choice = choose_move(&g, roll, d, &mut rng);
                assert_eq!(choice.is_some(), !legal.is_empty());
                if let Some(m) = choice {
                    assert!(legal.contains(&m), "{d} chose illegal {m:?}");
                }
            }
            match legal.first() {
                Some(&m) => drop(g.apply(roll, m)),
                None => g.pass(roll).unwrap(),
            }
        }
    }

    #[test]
    fn takes_the_winning_move() {
        let g = pos(
            Player::Light,
            [12, F, F, F, F, F, F],
            [S, S, S, S, S, S, 13],
        );
        for d in SMART {
            assert_eq!(pick(&g, 3, d), Some(Move { from: 12, to: F }), "{d}");
        }
    }

    #[test]
    fn prefers_capture() {
        // Light can capture Dark's advanced piece on 10, or enter a piece.
        let g = pos(Player::Light, [7, S, S, S, S, S, S], [10, S, S, S, S, S, S]);
        for d in SMART {
            assert_eq!(pick(&g, 3, d), Some(Move { from: 7, to: 10 }), "{d}");
        }
    }

    #[test]
    fn grabs_the_central_rosette() {
        // Roll 2: 6→8 lands on the safe rosette (extra turn); alternatives are
        // entering on 2 or a pointless private-lane shuffle.
        let g = pos(Player::Light, [6, S, S, S, S, S, S], [S; 7]);
        for d in SMART {
            assert_eq!(pick(&g, 2, d), Some(Move { from: 6, to: 8 }), "{d}");
        }
    }

    #[test]
    fn runs_from_danger() {
        // Dark sits right behind Light's piece on 9 (hit by a 2 or a 3).
        // With a 3, Light should run to 12, out of reach of both hunters,
        // instead of entering a new piece.
        let g = pos(Player::Light, [9, S, S, S, S, S, S], [7, 6, S, S, S, S, S]);
        for d in SMART {
            assert_eq!(pick(&g, 3, d), Some(Move { from: 9, to: 12 }), "{d}");
        }
    }

    #[test]
    fn evaluation_is_symmetric() {
        let g = pos(Player::Dark, [3, 9, 13, F, S, S, S], [8, 6, 1, S, S, S, S]);
        let diff = evaluate(&g, Player::Light) + evaluate(&g, Player::Dark);
        assert!(diff.abs() < 1e-9);
    }

    /// Play a match, alternating who goes first. Returns games won by `a`.
    fn duel(a: Difficulty, b: Difficulty, games: u32, seed: u64) -> u32 {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut a_wins = 0;
        for game in 0..games {
            let a_side = if game % 2 == 0 {
                Player::Light
            } else {
                Player::Dark
            };
            let mut g = GameState::new();
            while g.winner().is_none() {
                let roll = roll_dice(&mut rng);
                let d = if g.turn() == a_side { a } else { b };
                match choose_move(&g, roll, d, &mut rng) {
                    Some(m) => drop(g.apply(roll, m).unwrap()),
                    None => g.pass(roll).unwrap(),
                }
            }
            a_wins += u32::from(g.winner() == Some(a_side));
        }
        a_wins
    }

    #[test]
    fn easy_crushes_random() {
        let wins = duel(Difficulty::Easy, Difficulty::Random, 200, 11);
        assert!(wins > 160, "Easy won only {wins}/200 against Random");
    }

    #[test]
    fn medium_beats_easy() {
        let wins = duel(Difficulty::Medium, Difficulty::Easy, 200, 12);
        assert!(wins > 100, "Medium won only {wins}/200 against Easy");
    }
}

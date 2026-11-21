//! Computer opponents.
//!
//! Every difficulty answers the same question: given a position and a roll,
//! which legal move should the current player make?
//!
//! - [`Difficulty::Random`]: picks any legal move. A sparring dummy.
//! - [`Difficulty::Easy`]: looks one move ahead with a simple evaluation,
//!   and usually, but not always, plays the move it rates best: the worse a
//!   move looks, the less likely it is to be picked.
//! - [`Difficulty::Medium`]: expectiminimax, 2 plies.
//! - [`Difficulty::Hard`]: expectiminimax, 5 plies. (Odd depths suit the
//!   evaluation better: 5 plies beats 4 in about 55% of games, while 6 is
//!   no better than 5 at twice the cost.)
//! - [`Difficulty::Expert`]: perfect play, from the solved game in
//!   [`crate::solver`]. It needs a [`Table`](crate::solver::Table)
//!   [installed](crate::solver::install) first, and plays like Hard until
//!   there is one.
//!
//! Expectiminimax is minimax with *chance nodes* for the dice: at the
//! player's own decisions it maximises, at the opponent's it minimises, and
//! between decisions it averages over the five possible rolls, weighted by
//! [`ROLL_PROBABILITIES`]. Rosette extra turns need no special handling:
//! a decision node simply checks whose turn it is.

use crate::game::{FINISH, GameState, Move, PATH_LEN, Player, ROLL_PROBABILITIES, SAFE_ROSETTE};
use crate::solver;
use rand::Rng;
use std::collections::HashMap;
use std::fmt;
use std::hash::{BuildHasherDefault, Hasher};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Difficulty {
    Random,
    Easy,
    Medium,
    Hard,
    Expert,
}

impl Difficulty {
    pub const ALL: [Difficulty; 5] = [
        Difficulty::Random,
        Difficulty::Easy,
        Difficulty::Medium,
        Difficulty::Hard,
        Difficulty::Expert,
    ];

    /// Search depth in decision plies (`None` for random play). Expert
    /// searches like Hard while it has no solved table to play from.
    pub fn depth(self) -> Option<u32> {
        match self {
            Difficulty::Random => None,
            Difficulty::Easy => Some(1),
            Difficulty::Medium => Some(2),
            Difficulty::Hard | Difficulty::Expert => Some(5),
        }
    }

    /// How loosely this level picks among its moves: each move is chosen
    /// with weight `exp(-loss / temperature)`, where `loss` is how far it
    /// scores below the best move (about one point per square). `None`
    /// always plays the best move.
    pub fn temperature(self) -> Option<f64> {
        match self {
            Difficulty::Easy => Some(EASY_TEMPERATURE),
            _ => None,
        }
    }
}

/// Easy's [`Difficulty::temperature`]. A move one square worse than the best
/// is picked about a third as often as the best; one four squares worse,
/// about a fiftieth as often. Easy still plays its top move in about 87% of
/// real choices, and Medium beats it about 62% of the time (54% when Easy
/// always played its top move).
const EASY_TEMPERATURE: f64 = 1.0;

impl fmt::Display for Difficulty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Difficulty::Random => "Random",
            Difficulty::Easy => "Easy",
            Difficulty::Medium => "Medium",
            Difficulty::Hard => "Hard",
            Difficulty::Expert => "Expert",
        })
    }
}

/// Choose a move for the player to move in `state` with this `roll`.
/// Returns `None` when there is no legal move (the player must pass).
///
/// Medium, Hard and Expert are deterministic: ties go to the first move in
/// [`GameState::legal_moves`] order. `rng` is used by `Random` and `Easy`.
pub fn choose_move<R: Rng + ?Sized>(
    state: &GameState,
    roll: u8,
    difficulty: Difficulty,
    rng: &mut R,
) -> Option<Move> {
    if difficulty == Difficulty::Expert
        && let Some(table) = solver::installed()
    {
        let ranked = table.rank_moves(state, roll);
        let scores: Vec<f64> = ranked.iter().map(|&(_, v)| v).collect();
        return best_index(&scores).map(|i| ranked[i].0);
    }
    let moves = state.legal_moves(roll);
    match (moves.len(), difficulty.depth()) {
        (0, _) => None,
        (1, _) => Some(moves[0]),
        (n, None) => Some(moves[rng.next_u32() as usize % n]),
        (_, Some(depth)) => {
            let scores = score_moves(state, &moves, depth);
            let index = match difficulty.temperature() {
                Some(t) => sample_index(&scores, t, rng),
                None => best_index(&scores),
            };
            index.map(|i| moves[i])
        }
    }
}

/// Pick an index at random, favouring higher scores: weight
/// `exp((score - best) / temperature)`.
fn sample_index<R: Rng + ?Sized>(scores: &[f64], temperature: f64, rng: &mut R) -> Option<usize> {
    let best = scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let weights: Vec<f64> = scores
        .iter()
        .map(|s| ((s - best) / temperature).exp())
        .collect();
    // Uniform in [0, 1) from the top 53 bits.
    let unit = (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
    let mut r = unit * weights.iter().sum::<f64>();
    for (i, &w) in weights.iter().enumerate() {
        if r < w {
            return Some(i);
        }
        r -= w;
    }
    best_index(scores)
}

/// Score each legal move (from the mover's point of view) the way
/// `difficulty` would, in [`GameState::legal_moves`] order. Useful for a
/// "hint" feature or showing the AI's reasoning. Empty for `Random`.
/// Expert's scores, once it has its table, are chances of winning (`0..=1`);
/// the others' are evaluation points.
pub fn rank_moves(state: &GameState, roll: u8, difficulty: Difficulty) -> Vec<(Move, f64)> {
    if difficulty == Difficulty::Expert
        && let Some(table) = solver::installed()
    {
        return table.rank_moves(state, roll);
    }
    let Some(depth) = difficulty.depth() else {
        return Vec::new();
    };
    let moves = state.legal_moves(roll);
    let scores = score_moves(state, &moves, depth);
    moves.into_iter().zip(scores).collect()
}

/// Expectiminimax value of each move, searched `depth` decision plies deep.
fn score_moves(state: &GameState, moves: &[Move], depth: u32) -> Vec<f64> {
    let mut search = Search {
        me: state.turn(),
        cache: HashMap::default(),
    };
    moves
        .iter()
        .map(|&mv| {
            let mut next = *state;
            next.apply_unchecked(mv);
            search.expectation(&next, depth.saturating_sub(1))
        })
        .collect()
}

/// Index of the highest score; ties go to the first.
fn best_index(scores: &[f64]) -> Option<usize> {
    (0..scores.len()).reduce(|b, i| if scores[i] > scores[b] { i } else { b })
}

/// One expectiminimax search, from `me`'s point of view.
struct Search {
    me: Player,
    /// Values of chance nodes (and leaf evaluations) already worked out,
    /// keyed by position and remaining depth. The same position is often
    /// reached by different move orders; caching halves Hard's thinking time.
    cache: HashMap<u64, f64, BuildHasherDefault<KeyHasher>>,
}

impl Search {
    /// Chance node: expected value before the next roll.
    fn expectation(&mut self, state: &GameState, depth: u32) -> f64 {
        let key = state.packed() | u64::from(depth) << 48;
        if let Some(&v) = self.cache.get(&key) {
            return v;
        }
        let v = if depth == 0 || state.winner().is_some() {
            evaluate(state, self.me)
        } else {
            (0..ROLL_PROBABILITIES.len())
                .map(|roll| ROLL_PROBABILITIES[roll] * self.decision(state, roll as u8, depth))
                .sum()
        };
        self.cache.insert(key, v);
        v
    }

    /// Decision node: the player to move picks their best move for this roll.
    fn decision(&mut self, state: &GameState, roll: u8, depth: u32) -> f64 {
        let moves = state.move_list(roll);
        if moves.is_empty() {
            let mut next = *state;
            next.pass_unchecked();
            return self.expectation(&next, depth - 1);
        }
        let maximise = state.turn() == self.me;
        let mut best = if maximise {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
        for &mv in moves.iter() {
            let mut next = *state;
            next.apply_unchecked(mv);
            let v = self.expectation(&next, depth - 1);
            best = if maximise { best.max(v) } else { best.min(v) };
        }
        best
    }
}

/// A fast hasher for the packed 64-bit position keys.
#[derive(Default)]
struct KeyHasher(u64);

impl Hasher for KeyHasher {
    fn write(&mut self, _: &[u8]) {
        unreachable!("only u64 keys are hashed");
    }
    fn write_u64(&mut self, key: u64) {
        // Fibonacci hashing: one multiply mixes the bits well enough.
        self.0 = key.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
    fn finish(&self) -> u64 {
        // Fold the well-mixed high bits down to where the table looks.
        self.0 ^ self.0 >> 32
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
    state
        .pieces_on_board(victim)
        .map(|pos| f64::from(state.capture_ways(victim, pos)) / 16.0 * pos as f64)
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

    /// The move `d` rates best (what Medium and Hard always play, and what
    /// Easy plays most of the time).
    fn top(state: &GameState, roll: u8, d: Difficulty) -> Option<Move> {
        let ranked = rank_moves(state, roll, d);
        let scores: Vec<f64> = ranked.iter().map(|&(_, s)| s).collect();
        best_index(&scores).map(|i| ranked[i].0)
    }

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
            assert_eq!(top(&g, 3, d), Some(Move { from: 7, to: 10 }), "{d}");
        }
    }

    #[test]
    fn grabs_the_central_rosette() {
        // Roll 2: 6→8 lands on the safe rosette (extra turn); alternatives are
        // entering on 2 or a pointless private-lane shuffle.
        let g = pos(Player::Light, [6, S, S, S, S, S, S], [S; 7]);
        for d in SMART {
            assert_eq!(top(&g, 2, d), Some(Move { from: 6, to: 8 }), "{d}");
        }
    }

    #[test]
    fn runs_from_danger() {
        // Dark sits right behind Light's piece on 9 (hit by a 2 or a 3).
        // With a 3, Light should run to 12, out of reach of both hunters,
        // instead of entering a new piece.
        let g = pos(Player::Light, [9, S, S, S, S, S, S], [7, 6, S, S, S, S, S]);
        for d in SMART {
            assert_eq!(top(&g, 3, d), Some(Move { from: 9, to: 12 }), "{d}");
        }
    }

    #[test]
    fn medium_and_hard_play_their_top_move() {
        let g = pos(Player::Light, [9, S, S, S, S, S, S], [7, 6, S, S, S, S, S]);
        for d in [Difficulty::Medium, Difficulty::Hard] {
            assert_eq!(pick(&g, 3, d), top(&g, 3, d), "{d}");
        }
    }

    #[test]
    fn easy_usually_but_not_always_plays_its_top_move() {
        // Running from danger (9→12) is clearly best, entering a piece is a
        // reasonable alternative.
        let g = pos(Player::Light, [9, S, S, S, S, S, S], [7, 6, S, S, S, S, S]);
        let best = top(&g, 3, Difficulty::Easy);
        let mut rng = StdRng::seed_from_u64(8);
        let n = 2000;
        let hits = (0..n)
            .filter(|_| choose_move(&g, 3, Difficulty::Easy, &mut rng) == best)
            .count();
        assert!(
            hits > n / 2 && hits < n,
            "Easy chose the top move {hits}/{n} times"
        );
    }

    #[test]
    fn evaluation_is_symmetric() {
        let g = pos(Player::Dark, [3, 9, 13, F, S, S, S], [8, 6, 1, S, S, S, S]);
        let diff = evaluate(&g, Player::Light) + evaluate(&g, Player::Dark);
        assert!(diff.abs() < 1e-9);
    }

    /// Plain expectiminimax with no cache, to check the cached search against.
    fn reference(state: &GameState, me: Player, depth: u32) -> f64 {
        if depth == 0 || state.winner().is_some() {
            return evaluate(state, me);
        }
        (0..5u8)
            .map(|roll| {
                let moves = state.legal_moves(roll);
                let child = |mv: Option<Move>| {
                    let mut next = *state;
                    match mv {
                        Some(mv) => drop(next.apply_unchecked(mv)),
                        None => next.pass_unchecked(),
                    }
                    reference(&next, me, depth - 1)
                };
                let v = if moves.is_empty() {
                    child(None)
                } else {
                    let vals = moves.iter().map(|&mv| child(Some(mv)));
                    if state.turn() == me {
                        vals.fold(f64::NEG_INFINITY, f64::max)
                    } else {
                        vals.fold(f64::INFINITY, f64::min)
                    }
                };
                ROLL_PROBABILITIES[roll as usize] * v
            })
            .sum()
    }

    #[test]
    fn cache_does_not_change_scores() {
        let mut rng = StdRng::seed_from_u64(5);
        let mut g = GameState::new();
        let mut checked = 0;
        while checked < 25 {
            if g.winner().is_some() {
                g = GameState::new();
            }
            let roll = roll_dice(&mut rng);
            let moves = g.legal_moves(roll);
            if moves.len() > 1 {
                let scores = score_moves(&g, &moves, 3);
                for (mv, got) in moves.iter().zip(scores) {
                    let mut next = g;
                    next.apply_unchecked(*mv);
                    let want = reference(&next, g.turn(), 2);
                    assert!((got - want).abs() < 1e-9, "{mv:?}: {got} vs {want}");
                }
                checked += 1;
            }
            match moves.get(rng.next_u32() as usize % moves.len().max(1)) {
                Some(&m) => drop(g.apply(roll, m)),
                None => g.pass(roll).unwrap(),
            }
        }
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

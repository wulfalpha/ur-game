//! AI vs AI tournament for tuning the computer opponents.
//!
//!     cargo run --release --example arena [games]
//!
//! Plays every pairing of difficulties, alternating who moves first, and
//! reports win rates plus average thinking time per move.

use rand::SeedableRng;
use rand::rngs::StdRng;
use std::time::{Duration, Instant};
use ur_game::ai::{Difficulty, choose_move};
use ur_game::game::{GameState, Player, roll_dice};

fn main() {
    let games: u32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    let mut rng = StdRng::seed_from_u64(2026);
    let mut think = [(Duration::ZERO, 0u32); 4];

    println!("{games} games per pairing, sides alternate\n");
    for (i, &a) in Difficulty::ALL.iter().enumerate() {
        for &b in &Difficulty::ALL[..i] {
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
                    let t = Instant::now();
                    let choice = choose_move(&g, roll, d, &mut rng);
                    let slot = &mut think[d as usize];
                    slot.0 += t.elapsed();
                    slot.1 += 1;
                    match choice {
                        Some(m) => drop(g.apply(roll, m).unwrap()),
                        None => g.pass(roll).unwrap(),
                    }
                }
                a_wins += u32::from(g.winner() == Some(a_side));
            }
            let pct = 100.0 * a_wins as f64 / games as f64;
            println!("{a:>6} vs {b:<6}  {a_wins:>4}/{games}  ({pct:5.1}%)");
        }
    }
    println!("\naverage time per decision:");
    for d in Difficulty::ALL {
        let (total, n) = think[d as usize];
        if n > 0 {
            println!("  {d:>6}: {:?}", total / n);
        }
    }
}

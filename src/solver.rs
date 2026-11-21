//! The game solved: the exact chance of winning from every position.
//!
//! The Royal Game of Ur is small enough to solve outright. With the player
//! to move always called "me", there are 137,913,936 positions. Each one's
//! value is the chance that the player to move wins with perfect play by
//! both sides, and satisfies
//!
//! ```text
//! V(p) = Σ over throws r of P(r) · best move value for r
//! ```
//!
//! where a move's value is 1 if it wins, `V(next)` if it lands on a
//! rosette (same player again), and `1 − V(next)` otherwise (the opponent
//! moves next). No move, or a throw of 0, passes: `1 − V(p with sides
//! swapped)`.
//!
//! Captures make the game loop, so the values are found by iterating
//! that equation until nothing changes (value iteration). It goes much
//! faster in pieces: no move ever takes a piece back off the finished
//! pile, so positions are solved in groups by how many pieces are home,
//! most first, and each group only ever depends on itself and groups
//! already solved.
//!
//! [`Table`] stores each value as a 16-bit fraction (276 MB in all). The
//! solver works for any number of pieces per side, so small versions of the
//! game can be solved in tests.

use crate::game::{GameState, Move, PIECES_PER_PLAYER, ROLL_PROBABILITIES};
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering::Relaxed};

/// Squares only one player uses, in the order they map to pattern bits.
const PRIVATE: [u8; 6] = [1, 2, 3, 4, 13, 14];
const FINISH: u8 = 15;
const SAFE: u8 = 8;
const ROSETTES: u16 = 1 << 4 | 1 << 8 | 1 << 14;
const SHARED: u16 = 0xFF << 5;
/// Ways to fill the eight shared squares: empty, mine or theirs.
const PATTERNS: usize = 6561;
/// Stop iterating a group once no value moves by more than this.
const TOLERANCE: f32 = 1e-6;
/// Sweeps a group typically takes, for reporting progress.
const SWEEPS_EXPECTED: f64 = 80.0;

/// A position from the point of view of the player to move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pos {
    /// Bit `p` set: a piece on path square `p` (`1..=14`).
    me: u16,
    them: u16,
    me_home: u8,
    them_home: u8,
}

impl Pos {
    fn swap(self) -> Pos {
        Pos {
            me: self.them,
            them: self.me,
            me_home: self.them_home,
            them_home: self.me_home,
        }
    }
}

/// What a move leads to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Next {
    /// The mover has borne off their last piece.
    Win,
    /// Landed on a rosette: the same player moves again.
    Again(Pos),
    /// The opponent moves next (this is still from the mover's side).
    Over(Pos),
}

/// Every move for `roll`, in the same order as
/// [`GameState::legal_moves`]: entering first, then pieces from the back.
fn moves(pos: Pos, roll: u8, pieces: u8, mut f: impl FnMut(Move, Next)) {
    if roll == 0 {
        return;
    }
    let waiting = pieces - pos.me.count_ones() as u8 - pos.me_home;
    let froms = (waiting > 0)
        .then_some(0)
        .into_iter()
        .chain((1..=14).filter(|&p| pos.me & 1 << p != 0));
    for from in froms {
        let to = from + roll;
        let mv = Move { from, to };
        let mut me = pos.me & !(1u16 << from) & !1;
        if to > FINISH {
            continue;
        }
        if to == FINISH {
            let home = pos.me_home + 1;
            if home == pieces {
                f(mv, Next::Win);
            } else {
                let next = Pos {
                    me,
                    me_home: home,
                    ..pos
                };
                f(mv, Next::Over(next));
            }
            continue;
        }
        if pos.me & 1 << to != 0 || (to == SAFE && pos.them & 1 << SAFE != 0) {
            continue;
        }
        me |= 1 << to;
        let them = pos.them & !(SHARED & 1 << to);
        let next = Pos { me, them, ..pos };
        if ROSETTES & 1 << to != 0 {
            f(mv, Next::Again(next));
        } else {
            f(mv, Next::Over(next));
        }
    }
}

/// Numbers every position `0..len` for a given number of pieces a side.
///
/// Positions are grouped into layers by pieces home `(me, them)`; within a
/// layer by the pattern on the shared squares, then by each side's private
/// squares. The waiting pieces are whatever is left over.
struct Layout {
    pieces: u8,
    /// `count[home][on_shared]`: private-square patterns that still fit.
    count: [[u32; 9]; 8],
    /// `rank[(home * 9 + on_shared) * 64 + pattern]`.
    rank: Vec<u32>,
    /// Patterns in rank order, per `(home, on_shared)`.
    privates: Vec<Vec<u8>>,
    /// Ternary value of each 8-bit shared-square mask.
    ternary: [u16; 256],
    /// Every shared pattern `(index, mine, theirs)`, by index.
    shared: Vec<(usize, u8, u8)>,
    /// Start of each layer `(me_home, them_home)`.
    layer_start: [[u64; 8]; 8],
    /// `base[(layer) * PATTERNS + pattern]`: offset within the layer.
    base: Vec<u64>,
    /// Layers in the order they are stored and solved: most pieces home
    /// first, grouped by the total home.
    layers: Vec<(u8, u8)>,
    len: u64,
}

fn private_bits(board: u16) -> u8 {
    ((board >> 1) & 0xF | ((board >> 13) & 0x3) << 4) as u8
}

fn from_private(bits: u8) -> u16 {
    PRIVATE
        .iter()
        .enumerate()
        .filter(|(i, _)| bits & 1 << i != 0)
        .fold(0, |b, (_, &sq)| b | 1 << sq)
}

impl Layout {
    fn new(pieces: u8) -> Layout {
        assert!((1..=7).contains(&pieces));
        let mut count = [[0u32; 9]; 8];
        let mut rank = vec![u32::MAX; 8 * 9 * 64];
        let mut privates = vec![Vec::new(); 8 * 9];
        for home in 0..=pieces {
            for on_shared in 0..=8u8 {
                for bits in 0..64u8 {
                    if bits.count_ones() as u8 + on_shared + home <= pieces {
                        let slot = (home as usize * 9 + on_shared as usize) * 64 + bits as usize;
                        rank[slot] = count[home as usize][on_shared as usize];
                        count[home as usize][on_shared as usize] += 1;
                        privates[home as usize * 9 + on_shared as usize].push(bits);
                    }
                }
            }
        }
        let mut ternary = [0u16; 256];
        for (mask, t) in ternary.iter_mut().enumerate() {
            *t = (0..8)
                .filter(|i| mask & 1 << i != 0)
                .map(|i| 3u16.pow(i))
                .sum();
        }
        let mut shared = Vec::with_capacity(PATTERNS);
        for mine in 0..=255u8 {
            for theirs in 0..=255u8 {
                if mine & theirs == 0 {
                    let index =
                        ternary[mine as usize] as usize + 2 * ternary[theirs as usize] as usize;
                    shared.push((index, mine, theirs));
                }
            }
        }
        shared.sort_unstable();
        let mut layers = Vec::new();
        for total in (0..=2 * pieces).rev() {
            for me in (0..=pieces).rev() {
                if let Some(them) = total.checked_sub(me).filter(|&t| t <= pieces) {
                    layers.push((me, them));
                }
            }
        }
        let mut layer_start = [[0u64; 8]; 8];
        let mut base = vec![0u64; 64 * PATTERNS];
        let mut len = 0u64;
        for &(mh, th) in &layers {
            layer_start[mh as usize][th as usize] = len;
            let layer = mh as usize * 8 + th as usize;
            let mut offset = 0u64;
            for &(index, mine, theirs) in &shared {
                base[layer * PATTERNS + index] = offset;
                offset += u64::from(count[mh as usize][mine.count_ones() as usize])
                    * u64::from(count[th as usize][theirs.count_ones() as usize]);
            }
            len += offset;
        }
        Layout {
            pieces,
            count,
            rank,
            privates,
            ternary,
            shared,
            layer_start,
            base,
            layers,
            len,
        }
    }

    fn index(&self, p: Pos) -> usize {
        let mine = ((p.me & SHARED) >> 5) as usize;
        let theirs = ((p.them & SHARED) >> 5) as usize;
        let pattern = self.ternary[mine] as usize + 2 * self.ternary[theirs] as usize;
        let (a, b) = (mine.count_ones() as usize, theirs.count_ones() as usize);
        let (mh, th) = (p.me_home as usize, p.them_home as usize);
        let rank_me = self.rank[(mh * 9 + a) * 64 + private_bits(p.me) as usize];
        let rank_them = self.rank[(th * 9 + b) * 64 + private_bits(p.them) as usize];
        (self.layer_start[mh][th]
            + self.base[(mh * 8 + th) * PATTERNS + pattern]
            + u64::from(rank_me) * u64::from(self.count[th][b])
            + u64::from(rank_them)) as usize
    }

    /// Positions in one layer with one shared pattern, in index order,
    /// starting at index `start`.
    fn block(&self, (mh, th): (u8, u8), (_, mine, theirs): (usize, u8, u8)) -> (usize, Vec<Pos>) {
        let a = mine.count_ones() as usize;
        let b = theirs.count_ones() as usize;
        let mut out = Vec::new();
        for &pm in &self.privates[mh as usize * 9 + a] {
            for &pt in &self.privates[th as usize * 9 + b] {
                out.push(Pos {
                    me: from_private(pm) | u16::from(mine) << 5,
                    them: from_private(pt) | u16::from(theirs) << 5,
                    me_home: mh,
                    them_home: th,
                });
            }
        }
        let start = out.first().map_or(0, |&p| self.index(p));
        (start, out)
    }
}

/// The value of every position, as `value / 65535` chances of winning for
/// the player to move.
pub struct Table {
    layout: Layout,
    values: Vec<u16>,
}

impl Table {
    /// Positions in the full game.
    pub const LEN: usize = 137_913_936;

    /// Wrap values saved earlier by [`values`](Self::values). `None` if
    /// there are the wrong number of them.
    pub fn from_values(values: Vec<u16>) -> Option<Table> {
        (values.len() == Self::LEN).then(|| Table {
            layout: Layout::new(PIECES_PER_PLAYER),
            values,
        })
    }

    /// The raw values, for saving.
    pub fn values(&self) -> &[u16] {
        &self.values
    }

    fn value(&self, p: Pos) -> f64 {
        f64::from(self.values[self.layout.index(p)]) / 65535.0
    }

    /// The chance the player to move in `state` wins, with perfect play.
    pub fn win_chance(&self, state: &GameState) -> f64 {
        if let Some(winner) = state.winner() {
            return f64::from(u8::from(winner == state.turn()));
        }
        self.value(pos_of(state))
    }

    /// Each legal move for `roll` with the mover's chance of winning after
    /// it, in [`GameState::legal_moves`] order.
    pub fn rank_moves(&self, state: &GameState, roll: u8) -> Vec<(Move, f64)> {
        let mut out = Vec::new();
        if state.winner().is_some() {
            return out;
        }
        moves(pos_of(state), roll, self.layout.pieces, |mv, next| {
            let v = match next {
                Next::Win => 1.0,
                Next::Again(p) => self.value(p),
                Next::Over(p) => 1.0 - self.value(p.swap()),
            };
            out.push((mv, v));
        });
        out
    }
}

fn pos_of(state: &GameState) -> Pos {
    let me = state.turn();
    let them = me.opponent();
    Pos {
        me: state.board_bits(me),
        them: state.board_bits(them),
        me_home: state.finished(me),
        them_home: state.finished(them),
    }
}

/// Solve the full game. Takes about a minute on a recent laptop and half a
/// gigabyte of memory while it runs. `progress` is called now and then with
/// the fraction done.
pub fn solve(progress: impl Fn(f64) + Sync) -> Table {
    let (layout, values) = solve_pieces(PIECES_PER_PLAYER, progress);
    Table { layout, values }
}

fn solve_pieces(pieces: u8, progress: impl Fn(f64) + Sync) -> (Layout, Vec<u16>) {
    let layout = Layout::new(pieces);
    let mut values = vec![0u16; layout.len as usize];
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    // Layers solved together: those with the same total home, since
    // passing the turn swaps (me, them) home counts.
    let mut groups: Vec<Vec<(u8, u8)>> = Vec::new();
    for &(mh, th) in &layout.layers {
        match groups.last_mut() {
            Some(g) if g[0].0 + g[0].1 == mh + th => g.push((mh, th)),
            _ => groups.push(vec![(mh, th)]),
        }
    }
    let mut done = 0u64;
    for group in groups {
        let first = layout.layer_start[group[0].0 as usize][group[0].1 as usize] as usize;
        // Blocks of positions sharing a middle-row pattern, nearest the end
        // first: a move without a capture always leads further on, so most
        // of what a position depends on is then already up to date in the
        // same sweep. (Sorting single positions settles in fewer sweeps but
        // loses the memory locality of whole blocks, and is slower.)
        // Within a block, which is small enough to stay in cache, the
        // same goes for single positions.
        let mut blocks: Vec<Block> = group
            .iter()
            .flat_map(|&layer| layout.shared.iter().map(move |&s| (layer, s)))
            .map(|(layer, s)| layout.block(layer, s))
            .filter(|(_, b)| !b.is_empty())
            .map(|(start, b)| {
                let key = progress_of(b[0]) - private_progress(b[0]);
                let mut b: Vec<(u16, Pos)> = (0..).zip(b).collect();
                b.sort_by_key(|&(_, p)| std::cmp::Reverse(private_progress(p)));
                (key, start, b)
            })
            .collect();
        blocks.sort_by_key(|&(key, start, _)| (std::cmp::Reverse(key), start));
        let size: usize = blocks.iter().map(|(_, _, b)| b.len()).sum();
        let work: Vec<AtomicU32> = (0..size)
            .map(|_| AtomicU32::new(0.5f32.to_bits()))
            .collect();
        let solved = &values;
        let get = |i: usize| -> f32 {
            match i.checked_sub(first).filter(|&j| j < size) {
                Some(j) => f32::from_bits(work[j].load(Relaxed)),
                None => f32::from(solved[i]) / 65535.0,
            }
        };
        let mut sweeps = 0;
        loop {
            let next = AtomicUsize::new(0);
            let worst = std::thread::scope(|scope| {
                let handles: Vec<_> = (0..threads)
                    .map(|_| {
                        scope.spawn(|| {
                            let mut worst = 0f32;
                            while let Some((_, start, block)) =
                                blocks.get(next.fetch_add(1, Relaxed))
                            {
                                for &(i, p) in block {
                                    let v = update(&layout, p, &get);
                                    let slot = &work[start + usize::from(i) - first];
                                    let old = f32::from_bits(slot.swap(v.to_bits(), Relaxed));
                                    worst = worst.max((v - old).abs());
                                }
                            }
                            worst
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|h| h.join().expect("solver thread"))
                    .fold(0f32, f32::max)
            });
            sweeps += 1;
            // Rough progress within the group, which settles in a few
            // dozen sweeps.
            let within = (sweeps as f64 / SWEEPS_EXPECTED).min(0.95);
            progress((done as f64 + size as f64 * within) / layout.len as f64);
            if worst < TOLERANCE {
                if std::env::var_os("UR_SOLVER_DEBUG").is_some() {
                    eprintln!("group {:?} size {size} sweeps {sweeps}", group[0]);
                }
                break;
            }
        }
        for (j, slot) in work.iter().enumerate() {
            let v = f32::from_bits(slot.load(Relaxed)).clamp(0.0, 1.0);
            values[first + j] = (v * 65535.0).round() as u16;
        }
        done += size as u64;
        progress(done as f64 / layout.len as f64);
    }
    (layout, values)
}

/// The part of [`progress_of`] from private squares.
fn private_progress(p: Pos) -> u32 {
    let side = |board: u16| -> u32 {
        PRIVATE
            .iter()
            .filter(|&&q| board & 1 << q != 0)
            .map(|&q| u32::from(q))
            .sum()
    };
    side(p.me) + side(p.them)
}

/// Positions sharing a middle-row pattern: their sort key, the index of
/// the first, and each with its offset from there.
type Block = (u32, usize, Vec<(u16, Pos)>);

/// How far along both sides are, in squares, counting a piece home as 15.
fn progress_of(p: Pos) -> u32 {
    let side = |board: u16, home: u8| -> u32 {
        (1..=14u32).filter(|&q| board & 1 << q != 0).sum::<u32>() + 15 * u32::from(home)
    };
    side(p.me, p.me_home) + side(p.them, p.them_home)
}

/// One step of value iteration for `p`.
fn update(layout: &Layout, p: Pos, get: &impl Fn(usize) -> f32) -> f32 {
    let n = layout.pieces;
    if p.me_home == n {
        return 1.0;
    }
    if p.them_home == n {
        return 0.0;
    }
    let pass = 1.0 - get(layout.index(p.swap()));
    let mut v = ROLL_PROBABILITIES[0] as f32 * pass;
    for roll in 1..=4u8 {
        let mut best = f32::NEG_INFINITY;
        moves(p, roll, n, |_, next| {
            let value = match next {
                Next::Win => 1.0,
                Next::Again(q) => get(layout.index(q)),
                Next::Over(q) => 1.0 - get(layout.index(q.swap())),
            };
            best = best.max(value);
        });
        if best == f32::NEG_INFINITY {
            best = pass;
        }
        v += ROLL_PROBABILITIES[roll as usize] as f32 * best;
    }
    v
}

/// First line of a saved table.
const MAGIC: &[u8] = b"ur-game expert table 1\n";

impl Table {
    /// Save the table: a short header, then each value as two
    /// little-endian bytes. Written to a temporary file and renamed, so a
    /// crash can't leave half a table behind.
    pub fn write_to(&self, path: &Path) -> io::Result<()> {
        write_values(path, &self.values)
    }

    /// Load a table saved by [`write_to`](Self::write_to).
    pub fn read_from(path: &Path) -> io::Result<Table> {
        Table::from_values(read_values(path, Self::LEN)?).ok_or_else(not_a_table)
    }
}

fn not_a_table() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "not an expert table")
}

fn write_values(path: &Path, values: &[u16]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    let mut out = BufWriter::new(File::create(&tmp)?);
    out.write_all(MAGIC)?;
    for chunk in values.chunks(1 << 16) {
        let bytes: Vec<u8> = chunk.iter().flat_map(|v| v.to_le_bytes()).collect();
        out.write_all(&bytes)?;
    }
    out.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    fs::rename(tmp, path)
}

fn read_values(path: &Path, len: usize) -> io::Result<Vec<u16>> {
    let mut file = BufReader::new(File::open(path)?);
    if file.get_ref().metadata()?.len() != (MAGIC.len() + 2 * len) as u64 {
        return Err(not_a_table());
    }
    let mut magic = vec![0; MAGIC.len()];
    file.read_exact(&mut magic)?;
    if magic != MAGIC {
        return Err(not_a_table());
    }
    let mut values = Vec::with_capacity(len);
    let mut buf = vec![0u8; 1 << 17];
    while values.len() < len {
        let n = (2 * (len - values.len())).min(buf.len());
        file.read_exact(&mut buf[..n])?;
        values.extend(
            buf[..n]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&b| u16::from_le_bytes(b)),
        );
    }
    Ok(values)
}

static INSTALLED: OnceLock<Table> = OnceLock::new();

/// Make `table` the one [`Difficulty::Expert`](crate::ai::Difficulty::Expert)
/// plays from. Only the first call has any effect.
pub fn install(table: Table) {
    let _ = INSTALLED.set(table);
}

/// The table installed with [`install`], if any yet.
pub fn installed() -> Option<&'static Table> {
    INSTALLED.get()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::{Rng, SeedableRng};

    #[test]
    fn the_full_game_has_the_known_number_of_positions() {
        assert_eq!(Layout::new(7).len as usize, Table::LEN);
    }

    #[test]
    fn small_layouts_number_every_position_once() {
        for pieces in 1..=3 {
            let layout = Layout::new(pieces);
            let mut seen = vec![false; layout.len as usize];
            for &layer in &layout.layers {
                for &s in &layout.shared {
                    let (start, block) = layout.block(layer, s);
                    for (i, &p) in block.iter().enumerate() {
                        let index = layout.index(p);
                        assert_eq!(index, start + i);
                        assert!(!seen[index], "{p:?} numbered twice");
                        seen[index] = true;
                    }
                }
            }
            assert!(seen.iter().all(|&s| s), "gaps for {pieces} pieces");
        }
    }

    /// Positions from random games of the real thing.
    fn random_positions(count: usize) -> Vec<GameState> {
        let mut rng = StdRng::seed_from_u64(7);
        let mut out = Vec::new();
        while out.len() < count {
            let mut g = GameState::new();
            while g.winner().is_none() && out.len() < count {
                out.push(g);
                let roll = crate::game::roll_dice(&mut rng);
                let moves = g.legal_moves(roll);
                if moves.is_empty() {
                    g.pass(roll).unwrap();
                } else {
                    let mv = moves[rng.next_u32() as usize % moves.len()];
                    g.apply(roll, mv).unwrap();
                }
            }
        }
        out
    }

    #[test]
    fn moves_match_the_rules_engine() {
        let layout = Layout::new(7);
        for state in random_positions(20_000) {
            let p = pos_of(&state);
            assert!(layout.index(p) < Table::LEN);
            for roll in 0..=4 {
                let mut ours = Vec::new();
                moves(p, roll, 7, |mv, next| ours.push((mv, next)));
                let legal = state.legal_moves(roll);
                assert_eq!(ours.iter().map(|m| m.0).collect::<Vec<_>>(), legal);
                for (mv, next) in ours {
                    let mut after = state;
                    let outcome = after.apply(roll, mv).unwrap();
                    let expected = match outcome {
                        o if o.winner.is_some() => Next::Win,
                        o if o.extra_turn => Next::Again(pos_of(&after)),
                        _ => Next::Over(pos_of(&after).swap()),
                    };
                    assert_eq!(next, expected, "{state:?} roll {roll} {mv:?}");
                }
            }
        }
    }

    #[test]
    fn small_games_solve_to_a_fixed_point() {
        for pieces in 1..=3 {
            let (layout, values) = solve_pieces(pieces, |_| {});
            let get = |i: usize| f32::from(values[i]) / 65535.0;
            let mut worst = 0f32;
            for &layer in &layout.layers {
                for &s in &layout.shared {
                    for p in layout.block(layer, s).1 {
                        let v = get(layout.index(p));
                        assert!((0.0..=1.0).contains(&v));
                        worst = worst.max((update(&layout, p, &get) - v).abs());
                    }
                }
            }
            // Only rounding to 16 bits is left over.
            assert!(worst < 1e-4, "{pieces} pieces: residual {worst}");
            // Moving first is an advantage.
            let start = Pos {
                me: 0,
                them: 0,
                me_home: 0,
                them_home: 0,
            };
            let first = get(layout.index(start));
            assert!(first > 0.5 && first < 0.6, "{pieces} pieces: {first}");
        }
    }

    /// The full solve takes minutes, so it only runs when asked for:
    /// `cargo test --release -- --ignored full_game`.
    #[test]
    #[ignore]
    fn full_game_solves_to_a_fixed_point() {
        let table = solve(|_| {});
        let layout = &table.layout;
        let get = |i: usize| f32::from(table.values[i]) / 65535.0;
        let mut worst = 0f32;
        for state in random_positions(200_000) {
            let p = pos_of(&state);
            worst = worst.max((update(layout, p, &get) - get(layout.index(p))).abs());
        }
        assert!(worst < 1e-4, "residual {worst}");
        let first = table.win_chance(&GameState::new());
        assert!((0.5..0.53).contains(&first), "{first}");
    }

    #[test]
    fn values_survive_a_trip_through_a_file() {
        let dir = std::env::temp_dir().join(format!("ur-table-{}", std::process::id()));
        let path = dir.join("table.bin");
        let values: Vec<u16> = (0..200_000u32).map(|i| (i * 7919) as u16).collect();
        write_values(&path, &values).unwrap();
        assert_eq!(read_values(&path, values.len()).unwrap(), values);
        // A file of the wrong size, or a truncated one, is refused.
        assert!(read_values(&path, values.len() + 1).is_err());
        fs::write(&path, b"ur-game expert table 1\n\0\0").unwrap();
        assert!(read_values(&path, values.len()).is_err());
        assert!(Table::read_from(&path).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_table_plays_from_the_mover_s_side() {
        // A one-piece-a-side game is easy to reason about: with my piece
        // on 14 and nothing else left, a throw of 1 wins at once.
        let (layout, values) = solve_pieces(1, |_| {});
        let get = |p: Pos| f64::from(values[layout.index(p)]) / 65535.0;
        let close = Pos {
            me: 1 << 14,
            them: 0,
            me_home: 0,
            them_home: 0,
        };
        assert!(get(close) > 0.25);
        // Nearer home is better.
        let far = Pos {
            me: 1 << 5,
            ..close
        };
        assert!(get(close) > get(far));
    }
}

//! The game solved: the exact chance of winning from every position.
//!
//! The Royal Game of Ur is small enough to solve outright. With the player
//! to move always called "me", there are 137,913,936 positions under
//! Finkel's rules and 501,032,952 under Masters'. Each one's value is the
//! chance that the player to move wins with perfect play by both sides,
//! and satisfies
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
//! [`Table`] stores each value as a 16-bit fraction (276 MB for Finkel,
//! 1 GB for Masters). The solver works for any number of pieces per side,
//! so small versions of the game can be solved in tests.

use crate::game::{GameState, Move, PIECES_PER_PLAYER, Rules};
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering::Relaxed};

/// Stop iterating a group once no value moves by more than this.
const TOLERANCE: f32 = 1e-6;
/// Sweeps a group typically takes, for reporting progress.
const SWEEPS_EXPECTED: f64 = 80.0;
/// The shared squares start at path position 5 under both rule sets.
const SHARED_FROM: u8 = 5;
/// At most 12 shared squares, 6 private ones and 7 pieces home.
const MAX_SHARED: usize = 12;
const ON_SHARED: usize = MAX_SHARED + 1;
const HOMES: usize = PIECES_PER_PLAYER as usize + 1;

/// A position from the point of view of the player to move. Each side's
/// pieces are in their own path positions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pos {
    /// Bit `p` set: a piece on path square `p`.
    me: u32,
    them: u32,
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

/// Numbers every position `0..len` for one rule set and number of pieces,
/// and knows the moves.
///
/// Positions are grouped into layers by pieces home `(me, them)`; within a
/// layer by how many pieces each side has on the shared squares, then by
/// the pattern there, then by each side's private squares. The waiting
/// pieces are whatever is left over.
struct Layout {
    rules: Rules,
    pieces: u8,
    path_len: u8,
    finish: u8,
    /// Shared squares, private squares (in pattern-bit order).
    shared: u8,
    private: Vec<u8>,
    rosettes: u32,
    safe: u32,
    /// `enemy[to]`: the bit for square `to` in the opponent's own path
    /// positions, or 0 off the shared squares.
    enemy: [u32; 18],
    roll_odds: [f32; 5],
    /// `count[home][on_shared]`: private-square patterns that still fit.
    count: [[u32; ON_SHARED]; HOMES],
    /// `rank[(home * ON_SHARED + on_shared) << private.len() | pattern]`.
    rank: Vec<u32>,
    /// Patterns in rank order, per `home * ON_SHARED + on_shared`.
    privates: Vec<Vec<u8>>,
    /// Ternary value of each shared-square mask.
    ternary: Vec<u32>,
    /// Rank of each shared pattern (by ternary value) among those with the
    /// same number of pieces each side.
    pattern_rank: Vec<u32>,
    /// Every shared pattern `(mine, theirs)` with `theirs` as the squares
    /// would be in my path positions, by ternary value.
    patterns: Vec<(u16, u16)>,
    /// Start of each layer `(me_home, them_home)`, then of each
    /// `(on_shared mine, on_shared theirs)` within it.
    layer_start: [[u64; HOMES]; HOMES],
    counts_start: Vec<u64>,
    /// Layers in the order they are stored and solved: most pieces home
    /// first, grouped by the total home.
    layers: Vec<(u8, u8)>,
    len: u64,
}

fn choose(n: u32, k: u32) -> u64 {
    (0..k).fold(1u64, |c, i| c * u64::from(n - i) / u64::from(i + 1))
}

impl Layout {
    fn new(rules: Rules, pieces: u8) -> Layout {
        assert!((1..=PIECES_PER_PLAYER).contains(&pieces));
        let path_len = rules.path_len();
        let shared = (*rules.shared().end() - SHARED_FROM + 1) as usize;
        let private: Vec<u8> = (1..=path_len).filter(|&p| !rules.is_shared(p)).collect();
        let pbits = private.len();
        let mut enemy = [0u32; 18];
        for to in rules.shared() {
            enemy[to as usize] = 1 << rules.opposite(to);
        }
        let mut count = [[0u32; ON_SHARED]; HOMES];
        let mut rank = vec![u32::MAX; (HOMES * ON_SHARED) << pbits];
        let mut privates = vec![Vec::new(); HOMES * ON_SHARED];
        for home in 0..=pieces {
            for on_shared in 0..=shared as u8 {
                let slot = home as usize * ON_SHARED + on_shared as usize;
                for bits in 0..1u32 << pbits {
                    if bits.count_ones() as u8 + on_shared + home <= pieces {
                        rank[slot << pbits | bits as usize] =
                            count[home as usize][on_shared as usize];
                        count[home as usize][on_shared as usize] += 1;
                        privates[slot].push(bits as u8);
                    }
                }
            }
        }
        let ternary: Vec<u32> = (0..1u32 << shared)
            .map(|mask| {
                (0..shared as u32)
                    .filter(|i| mask & 1 << i != 0)
                    .map(|i| 3u32.pow(i))
                    .sum()
            })
            .collect();
        let size = 3usize.pow(shared as u32);
        let mut patterns = vec![(0u16, 0u16); size];
        for mine in 0..1u32 << shared {
            let free = !mine & ((1 << shared) - 1);
            // Every subset of the free squares.
            let mut theirs = free;
            loop {
                let t = (ternary[mine as usize] + 2 * ternary[theirs as usize]) as usize;
                patterns[t] = (mine as u16, theirs as u16);
                if theirs == 0 {
                    break;
                }
                theirs = (theirs - 1) & free;
            }
        }
        let mut pattern_rank = vec![0u32; size];
        let mut seen = vec![[0u32; ON_SHARED]; ON_SHARED];
        for (t, &(mine, theirs)) in patterns.iter().enumerate() {
            let slot = &mut seen[mine.count_ones() as usize][theirs.count_ones() as usize];
            pattern_rank[t] = *slot;
            *slot += 1;
        }
        let mut layers = Vec::new();
        for total in (0..=2 * pieces).rev() {
            for me in (0..=pieces).rev() {
                if let Some(them) = total.checked_sub(me).filter(|&t| t <= pieces) {
                    layers.push((me, them));
                }
            }
        }
        let mut layer_start = [[0u64; HOMES]; HOMES];
        let mut counts_start = vec![0u64; HOMES * HOMES * ON_SHARED * ON_SHARED];
        let mut len = 0u64;
        for &(mh, th) in &layers {
            layer_start[mh as usize][th as usize] = len;
            let mut offset = 0u64;
            for a in 0..=shared {
                for b in 0..=shared - a {
                    let slot =
                        ((mh as usize * HOMES + th as usize) * ON_SHARED + a) * ON_SHARED + b;
                    counts_start[slot] = offset;
                    let patterns =
                        choose(shared as u32, a as u32) * choose((shared - a) as u32, b as u32);
                    offset += patterns
                        * u64::from(count[mh as usize][a])
                        * u64::from(count[th as usize][b]);
                }
            }
            len += offset;
        }
        let odds = rules.roll_probabilities();
        Layout {
            rules,
            pieces,
            path_len,
            finish: rules.finish(),
            shared: shared as u8,
            private,
            rosettes: rules.rosettes().iter().fold(0, |m, &p| m | 1 << p),
            safe: rules.safe_square().map_or(0, |p| 1 << p),
            enemy,
            roll_odds: odds.map(|p| p as f32),
            count,
            rank,
            privates,
            ternary,
            pattern_rank,
            patterns,
            layer_start,
            counts_start,
            layers,
            len,
        }
    }

    fn shared_mask(&self) -> u32 {
        (1 << self.shared) - 1
    }

    /// Turn a side's shared squares between their own path positions and
    /// the other side's (the same, except where Masters' routes cross).
    fn mirror(&self, bits: u32) -> u32 {
        match self.rules {
            Rules::Finkel => bits,
            // Positions 12..=16 (bits 7..=11) run the other way.
            Rules::Masters => bits & 0x7F | ((bits >> 7) & 0x1F).reverse_bits() >> 27 << 7,
        }
    }

    fn private_bits(&self, board: u32) -> usize {
        match self.rules {
            Rules::Finkel => ((board >> 1) & 0xF | ((board >> 13) & 0x3) << 4) as usize,
            Rules::Masters => ((board >> 1) & 0xF) as usize,
        }
    }

    fn private_board(&self, bits: u8) -> u32 {
        self.private
            .iter()
            .enumerate()
            .filter(|(i, _)| bits & 1 << i != 0)
            .fold(0, |b, (_, &sq)| b | 1 << sq)
    }

    fn index(&self, p: Pos) -> usize {
        let mask = self.shared_mask();
        let mine = (p.me >> SHARED_FROM) & mask;
        let theirs = self.mirror((p.them >> SHARED_FROM) & mask);
        let pattern = (self.ternary[mine as usize] + 2 * self.ternary[theirs as usize]) as usize;
        let (a, b) = (mine.count_ones() as usize, theirs.count_ones() as usize);
        let (mh, th) = (p.me_home as usize, p.them_home as usize);
        let pbits = self.private.len();
        let rank_me = self.rank[(mh * ON_SHARED + a) << pbits | self.private_bits(p.me)];
        let rank_them = self.rank[(th * ON_SHARED + b) << pbits | self.private_bits(p.them)];
        let (count_me, count_them) = (u64::from(self.count[mh][a]), u64::from(self.count[th][b]));
        let slot = ((mh * HOMES + th) * ON_SHARED + a) * ON_SHARED + b;
        (self.layer_start[mh][th]
            + self.counts_start[slot]
            + u64::from(self.pattern_rank[pattern]) * count_me * count_them
            + u64::from(rank_me) * count_them
            + u64::from(rank_them)) as usize
    }

    /// Positions in layer `(mh, th)` with shared pattern `pattern`: how many,
    /// and the index of the first. The rest follow in order.
    fn block(&self, (mh, th): (u8, u8), pattern: usize) -> (usize, usize) {
        let (mine, theirs) = self.patterns[pattern];
        let a = mine.count_ones() as usize;
        let b = theirs.count_ones() as usize;
        let n = (self.count[mh as usize][a] * self.count[th as usize][b]) as usize;
        if n == 0 {
            return (0, 0);
        }
        (n, self.index(self.position((mh, th), pattern, 0)))
    }

    /// The `offset`th position of a block.
    fn position(&self, (mh, th): (u8, u8), pattern: usize, offset: usize) -> Pos {
        let (mine, theirs) = self.patterns[pattern];
        let a = mine.count_ones() as usize;
        let b = theirs.count_ones() as usize;
        let count_them = self.count[th as usize][b] as usize;
        let pm = self.privates[mh as usize * ON_SHARED + a][offset / count_them];
        let pt = self.privates[th as usize * ON_SHARED + b][offset % count_them];
        Pos {
            me: self.private_board(pm) | u32::from(mine) << SHARED_FROM,
            them: self.private_board(pt) | self.mirror(u32::from(theirs)) << SHARED_FROM,
            me_home: mh,
            them_home: th,
        }
    }

    /// Every move for `roll`, in the same order as
    /// [`GameState::legal_moves`]: entering first, then pieces from the back.
    fn moves(&self, pos: Pos, roll: u8, mut f: impl FnMut(Move, Next)) {
        if roll == 0 {
            return;
        }
        let waiting = self.pieces - pos.me.count_ones() as u8 - pos.me_home;
        let froms = (waiting > 0)
            .then_some(0)
            .into_iter()
            .chain((1..=self.path_len).filter(|&p| pos.me & 1 << p != 0));
        for from in froms {
            let to = from + roll;
            let mv = Move { from, to };
            if to > self.finish {
                continue;
            }
            let me = pos.me & !(1u32 << from) & !1;
            if to == self.finish {
                let home = pos.me_home + 1;
                if home == self.pieces {
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
            let hit = pos.them & self.enemy[to as usize];
            if pos.me & 1 << to != 0 || (hit != 0 && self.safe & 1 << to != 0) {
                continue;
            }
            let next = Pos {
                me: me | 1 << to,
                them: pos.them & !hit,
                ..pos
            };
            if self.rosettes & 1 << to != 0 {
                f(mv, Next::Again(next));
            } else {
                f(mv, Next::Over(next));
            }
        }
    }

    /// How far along a side's private squares are, in squares.
    fn private_progress(&self, bits: u8) -> u32 {
        self.private
            .iter()
            .enumerate()
            .filter(|(i, _)| bits & 1 << i != 0)
            .map(|(_, &q)| u32::from(q))
            .sum()
    }

    /// How far along both sides are on the shared squares, counting a piece
    /// home as one more than the path.
    fn shared_progress(&self, (mh, th): (u8, u8), pattern: usize) -> u32 {
        let (mine, theirs) = self.patterns[pattern];
        let side = |bits: u32, home: u8| -> u32 {
            (0..self.shared)
                .filter(|i| bits & 1 << i != 0)
                .map(|i| u32::from(SHARED_FROM + i))
                .sum::<u32>()
                + u32::from(self.finish) * u32::from(home)
        };
        side(u32::from(mine), mh) + side(self.mirror(u32::from(theirs)), th)
    }

    /// One step of value iteration for `p`.
    fn update(&self, p: Pos, get: &impl Fn(usize) -> f32) -> f32 {
        if p.me_home == self.pieces {
            return 1.0;
        }
        if p.them_home == self.pieces {
            return 0.0;
        }
        let pass = 1.0 - get(self.index(p.swap()));
        let mut v = self.roll_odds[0] * pass;
        for roll in 1..=4u8 {
            let mut best = f32::NEG_INFINITY;
            self.moves(p, roll, |_, next| {
                let value = match next {
                    Next::Win => 1.0,
                    Next::Again(q) => get(self.index(q)),
                    Next::Over(q) => 1.0 - get(self.index(q.swap())),
                };
                best = best.max(value);
            });
            if best == f32::NEG_INFINITY {
                best = pass;
            }
            v += self.roll_odds[roll as usize] * best;
        }
        v
    }
}

/// The value of every position under one rule set, as `value / 65535`
/// chances of winning for the player to move.
pub struct Table {
    layout: Layout,
    values: Vec<u16>,
}

impl Table {
    /// Positions in the full game under `rules`.
    pub fn len(rules: Rules) -> usize {
        match rules {
            Rules::Finkel => 137_913_936,
            Rules::Masters => 501_032_952,
        }
    }

    /// Wrap values saved earlier by [`values`](Self::values). `None` if
    /// there are the wrong number of them.
    pub fn from_values(rules: Rules, values: Vec<u16>) -> Option<Table> {
        (values.len() == Self::len(rules)).then(|| Table {
            layout: Layout::new(rules, PIECES_PER_PLAYER),
            values,
        })
    }

    pub fn rules(&self) -> Rules {
        self.layout.rules
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
        assert_eq!(state.rules(), self.rules(), "table for other rules");
        if let Some(winner) = state.winner() {
            return f64::from(u8::from(winner == state.turn()));
        }
        self.value(pos_of(state))
    }

    /// Each legal move for `roll` with the mover's chance of winning after
    /// it, in [`GameState::legal_moves`] order.
    pub fn rank_moves(&self, state: &GameState, roll: u8) -> Vec<(Move, f64)> {
        assert_eq!(state.rules(), self.rules(), "table for other rules");
        let mut out = Vec::new();
        if state.winner().is_some() {
            return out;
        }
        self.layout.moves(pos_of(state), roll, |mv, next| {
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

/// Solve the full game under `rules`. Finkel takes about three minutes on a
/// recent laptop and 800 MB of memory while it runs; Masters about four
/// times as long and twice the memory. `progress` is called now and then
/// with the fraction done.
pub fn solve(rules: Rules, progress: impl Fn(f64) + Sync) -> Table {
    let (layout, values) = solve_pieces(rules, PIECES_PER_PLAYER, progress);
    Table { layout, values }
}

/// A block of positions to update: its layer and shared pattern.
#[derive(Clone, Copy)]
struct Block {
    layer: (u8, u8),
    pattern: u32,
    start: usize,
    len: u16,
}

fn solve_pieces(rules: Rules, pieces: u8, progress: impl Fn(f64) + Sync) -> (Layout, Vec<u16>) {
    let layout = Layout::new(rules, pieces);
    let mut values = vec![0u16; layout.len as usize];
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    // Within a block, update positions further along first: the order
    // depends only on the block's piece counts, so work it out once each.
    let pbits = layout.private.len();
    let side_slots = HOMES * ON_SHARED;
    let orders: Vec<Vec<u16>> = (0..side_slots * side_slots)
        .map(|slot| {
            let (mine, theirs) = (
                &layout.privates[slot / side_slots],
                &layout.privates[slot % side_slots],
            );
            let mut order: Vec<u16> = (0..(mine.len() * theirs.len()) as u16).collect();
            order.sort_by_key(|&o| {
                let o = usize::from(o);
                let pm = mine[o / theirs.len()];
                let pt = theirs[o % theirs.len()];
                std::cmp::Reverse(layout.private_progress(pm) + layout.private_progress(pt))
            });
            order
        })
        .collect();
    debug_assert!(pbits <= 8);
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
        // Blocks of positions sharing a shared-square pattern, nearest the
        // end first: a move without a capture always leads further on, so
        // most of what a position depends on is then already up to date in
        // the same sweep.
        let mut keyed: Vec<(u32, Block)> = Vec::new();
        for &layer in &group {
            for pattern in 0..layout.patterns.len() {
                let (len, start) = layout.block(layer, pattern);
                if len > 0 {
                    let key = layout.shared_progress(layer, pattern);
                    let len = len as u16;
                    let pattern = pattern as u32;
                    keyed.push((
                        key,
                        Block {
                            layer,
                            pattern,
                            start,
                            len,
                        },
                    ));
                }
            }
        }
        keyed.sort_by_key(|&(key, b)| (std::cmp::Reverse(key), b.start));
        let blocks: Vec<Block> = keyed.into_iter().map(|(_, b)| b).collect();
        let size: usize = blocks.iter().map(|b| usize::from(b.len)).sum();
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
        // Hand out a few blocks at a time: Masters' blocks are small.
        let chunks: Vec<&[Block]> = blocks.chunks(16).collect();
        let mut sweeps = 0;
        loop {
            let next = AtomicUsize::new(0);
            let worst = std::thread::scope(|scope| {
                let handles: Vec<_> = (0..threads)
                    .map(|_| {
                        scope.spawn(|| {
                            let mut worst = 0f32;
                            while let Some(chunk) = chunks.get(next.fetch_add(1, Relaxed)) {
                                for b in *chunk {
                                    let (mine, theirs) = layout.patterns[b.pattern as usize];
                                    let slot_me =
                                        b.layer.0 as usize * ON_SHARED + mine.count_ones() as usize;
                                    let slot_them = b.layer.1 as usize * ON_SHARED
                                        + theirs.count_ones() as usize;
                                    let order = &orders[slot_me * side_slots + slot_them];
                                    for &o in order {
                                        let o = usize::from(o);
                                        let p = layout.position(b.layer, b.pattern as usize, o);
                                        let v = layout.update(p, &get);
                                        let slot = &work[b.start + o - first];
                                        let old = f32::from_bits(slot.swap(v.to_bits(), Relaxed));
                                        worst = worst.max((v - old).abs());
                                    }
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

/// First line of a saved table.
fn magic(rules: Rules) -> &'static [u8] {
    match rules {
        Rules::Finkel => b"ur-game expert table 2 finkel\n",
        Rules::Masters => b"ur-game expert table 2 masters\n",
    }
}

impl Table {
    /// Save the table: a short header, then each value as two
    /// little-endian bytes. Written to a temporary file and renamed, so a
    /// crash can't leave half a table behind.
    pub fn write_to(&self, path: &Path) -> io::Result<()> {
        write_values(path, magic(self.rules()), &self.values)
    }

    /// Load a table for `rules` saved by [`write_to`](Self::write_to).
    pub fn read_from(path: &Path, rules: Rules) -> io::Result<Table> {
        let values = read_values(path, magic(rules), Self::len(rules))?;
        Table::from_values(rules, values).ok_or_else(not_a_table)
    }
}

fn not_a_table() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "not an expert table")
}

fn write_values(path: &Path, magic: &[u8], values: &[u16]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    let mut out = BufWriter::new(File::create(&tmp)?);
    out.write_all(magic)?;
    for chunk in values.chunks(1 << 16) {
        let bytes: Vec<u8> = chunk.iter().flat_map(|v| v.to_le_bytes()).collect();
        out.write_all(&bytes)?;
    }
    out.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    fs::rename(tmp, path)
}

fn read_values(path: &Path, magic: &[u8], len: usize) -> io::Result<Vec<u16>> {
    let mut file = BufReader::new(File::open(path)?);
    if file.get_ref().metadata()?.len() != (magic.len() + 2 * len) as u64 {
        return Err(not_a_table());
    }
    let mut header = vec![0; magic.len()];
    file.read_exact(&mut header)?;
    if header != magic {
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

static INSTALLED: [OnceLock<Table>; 2] = [OnceLock::new(), OnceLock::new()];

/// Make `table` the one [`Difficulty::Expert`](crate::ai::Difficulty::Expert)
/// plays from under its rules. Only the first call per rule set has any
/// effect.
pub fn install(table: Table) {
    let _ = INSTALLED[table.rules() as usize].set(table);
}

/// The table installed for `rules` with [`install`], if any yet.
pub fn installed(rules: Rules) -> Option<&'static Table> {
    INSTALLED[rules as usize].get()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::{Rng, SeedableRng};

    #[test]
    fn the_full_games_have_the_known_number_of_positions() {
        for rules in Rules::ALL {
            assert_eq!(Layout::new(rules, 7).len as usize, Table::len(rules));
        }
    }

    #[test]
    fn small_layouts_number_every_position_once() {
        for rules in Rules::ALL {
            for pieces in 1..=3 {
                let layout = Layout::new(rules, pieces);
                let mut seen = vec![false; layout.len as usize];
                for &layer in &layout.layers {
                    for pattern in 0..layout.patterns.len() {
                        let (len, start) = layout.block(layer, pattern);
                        for o in 0..len {
                            let p = layout.position(layer, pattern, o);
                            let index = layout.index(p);
                            assert_eq!(index, start + o);
                            assert!(!seen[index], "{p:?} numbered twice");
                            seen[index] = true;
                        }
                    }
                }
                assert!(seen.iter().all(|&s| s), "gaps: {rules} {pieces} pieces");
            }
        }
    }

    #[test]
    fn mirroring_matches_the_rules() {
        let layout = Layout::new(Rules::Masters, 7);
        for p in 5..=16u8 {
            let bit = 1u32 << (p - SHARED_FROM);
            let other = Rules::Masters.opposite(p);
            assert_eq!(layout.mirror(bit), 1 << (other - SHARED_FROM), "{p}");
        }
        assert_eq!(Layout::new(Rules::Finkel, 7).mirror(0xA5), 0xA5);
    }

    /// Positions from random games of the real thing.
    fn random_positions(rules: Rules, count: usize) -> Vec<GameState> {
        let mut rng = StdRng::seed_from_u64(7);
        let mut out = Vec::new();
        while out.len() < count {
            let mut g = GameState::with_rules(rules);
            while g.winner().is_none() && out.len() < count {
                out.push(g);
                let roll = rules.roll_dice(&mut rng);
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
        for rules in Rules::ALL {
            let layout = Layout::new(rules, 7);
            for state in random_positions(rules, 20_000) {
                let p = pos_of(&state);
                assert!(layout.index(p) < Table::len(rules));
                for roll in 0..=4 {
                    let mut ours = Vec::new();
                    layout.moves(p, roll, |mv, next| ours.push((mv, next)));
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
    }

    #[test]
    fn small_games_solve_to_a_fixed_point() {
        for rules in Rules::ALL {
            for pieces in 1..=3 {
                let (layout, values) = solve_pieces(rules, pieces, |_| {});
                let get = |i: usize| f32::from(values[i]) / 65535.0;
                let mut worst = 0f32;
                for &layer in &layout.layers {
                    for pattern in 0..layout.patterns.len() {
                        for o in 0..layout.block(layer, pattern).0 {
                            let p = layout.position(layer, pattern, o);
                            let v = get(layout.index(p));
                            assert!((0.0..=1.0).contains(&v));
                            worst = worst.max((layout.update(p, &get) - v).abs());
                        }
                    }
                }
                // Only rounding to 16 bits is left over.
                assert!(worst < 1e-4, "{rules} {pieces} pieces: residual {worst}");
                // Moving first is an advantage.
                let start = Pos {
                    me: 0,
                    them: 0,
                    me_home: 0,
                    them_home: 0,
                };
                let first = get(layout.index(start));
                assert!(
                    first > 0.5 && first < 0.6,
                    "{rules} {pieces} pieces: {first}"
                );
            }
        }
    }

    /// The full solves take minutes, so they only run when asked for:
    /// `cargo test --release -- --ignored full_game`.
    fn full_game(rules: Rules) {
        let table = solve(rules, |_| {});
        let layout = &table.layout;
        let get = |i: usize| f32::from(table.values[i]) / 65535.0;
        let mut worst = 0f32;
        for state in random_positions(rules, 200_000) {
            let p = pos_of(&state);
            worst = worst.max((layout.update(p, &get) - get(layout.index(p))).abs());
        }
        assert!(worst < 1e-4, "residual {worst}");
        let first = table.win_chance(&GameState::with_rules(rules));
        println!("{rules}: the first player wins {first:.4}");
        assert!((0.5..0.6).contains(&first), "{first}");
    }

    #[test]
    #[ignore]
    fn full_game_finkel() {
        full_game(Rules::Finkel);
    }

    #[test]
    #[ignore]
    fn full_game_masters() {
        full_game(Rules::Masters);
    }

    #[test]
    fn values_survive_a_trip_through_a_file() {
        let dir = std::env::temp_dir().join(format!("ur-table-{}", std::process::id()));
        let path = dir.join("table.bin");
        let values: Vec<u16> = (0..200_000u32).map(|i| (i * 7919) as u16).collect();
        let magic = magic(Rules::Finkel);
        write_values(&path, magic, &values).unwrap();
        assert_eq!(read_values(&path, magic, values.len()).unwrap(), values);
        // The other rules' header, a file of the wrong size, or a truncated
        // one, is refused.
        assert!(read_values(&path, self::magic(Rules::Masters), values.len()).is_err());
        assert!(read_values(&path, magic, values.len() + 1).is_err());
        fs::write(&path, b"ur-game expert table 2 finkel\n\0\0").unwrap();
        assert!(read_values(&path, magic, values.len()).is_err());
        assert!(Table::read_from(&path, Rules::Finkel).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_table_plays_from_the_mover_s_side() {
        // A one-piece-a-side game is easy to reason about: with my piece
        // on the last square and nothing else left, a throw of 1 wins.
        for rules in Rules::ALL {
            let (layout, values) = solve_pieces(rules, 1, |_| {});
            let get = |p: Pos| f64::from(values[layout.index(p)]) / 65535.0;
            let close = Pos {
                me: 1 << rules.path_len(),
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
            assert!(get(close) > get(far), "{rules}");
        }
    }
}

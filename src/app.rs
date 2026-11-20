//! Application state and game flow for the terminal front end.
//!
//! Nothing here touches the terminal: `App` takes key events and the current
//! time, and exposes state for `ui` to draw. Computer turns are paced with
//! deadlines so a human can follow what the AI does.
//!
//! Animations (tumbling dice, pieces hopping square by square) are purely
//! presentational: the throw and the move are decided up front, and the
//! game state only changes once the animation has finished.

use crate::chronicle;
use crate::preferences::{self, Preferences};
use crate::save::{self, SavedGame};
use crate::sound::Cue;
use crate::stats::{self, Leaderboard};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;
use std::time::Duration;
use ur_game::ai::{self, Difficulty};
use ur_game::game::{
    FINISH, GameState, Move, Outcome, Player, ROLL_WAYS, SAFE_ROSETTE, START, is_shared,
    roll_from_tips,
};

/// A point in time, measured from when the program started. The front end
/// supplies it, which keeps this module free of any particular clock (a
/// browser build, for one, has no `std::time::Instant`).
pub type Time = Duration;

/// Pause before each computer action (roll, then move).
pub const AI_DELAY: Duration = Duration::from_millis(700);
/// How long a "no moves" roll stays on screen before the turn passes.
pub const PASS_DELAY: Duration = Duration::from_millis(1200);
/// How long the dice tumble before settling.
pub const DICE_ROLL_TIME: Duration = Duration::from_millis(650);
/// Time for a piece to hop one square.
pub const STEP_TIME: Duration = Duration::from_millis(120);
/// Pause after a piece lands, before the turn moves on.
pub const LAND_TIME: Duration = Duration::from_millis(180);
/// Extra pause on a capture so the hit registers.
pub const IMPACT_TIME: Duration = Duration::from_millis(350);
/// Enough for the longest games, so a shared Chronicle is complete.
const LOG_CAPACITY: usize = 2000;
/// Chronicle entries moved per scroll step.
pub const LOG_SCROLL_STEP: usize = 3;
/// The hint key asks this level for advice.
pub const HINT_LEVEL: Difficulty = Difficulty::Hard;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Controller {
    Human,
    Computer(Difficulty),
}

impl Controller {
    pub const CHOICES: [Controller; 5] = [
        Controller::Human,
        Controller::Computer(Difficulty::Random),
        Controller::Computer(Difficulty::Easy),
        Controller::Computer(Difficulty::Medium),
        Controller::Computer(Difficulty::Hard),
    ];

    pub fn label(self) -> String {
        match self {
            Controller::Human => "Human".into(),
            Controller::Computer(d) => format!("Computer · {d}"),
        }
    }

    fn cycle(self, step: isize) -> Controller {
        let n = Self::CHOICES.len() as isize;
        let i = Self::CHOICES.iter().position(|&c| c == self).unwrap_or(0) as isize;
        Self::CHOICES[(i + step).rem_euclid(n) as usize]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Menu,
    Game,
    Leaderboard,
}

/// Running totals for one player in the current game, for the summary at
/// the end and the shared Chronicle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub throws: u32,
    /// Sum of all throws.
    pub pips: u32,
    pub captures: u32,
    pub rosettes: u32,
    pub hints: u32,
}

/// Rows of the start menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItem {
    /// Only shown when there is a game to resume.
    Continue,
    Light,
    Dark,
    Animation,
    Start,
    Leaderboard,
    Quit,
}

impl MenuItem {
    pub const ALL: [MenuItem; 7] = [
        MenuItem::Continue,
        MenuItem::Light,
        MenuItem::Dark,
        MenuItem::Animation,
        MenuItem::Start,
        MenuItem::Leaderboard,
        MenuItem::Quit,
    ];
}

/// The dice as last thrown: bit `i` of `tips` set ⇔ die `i` shows a mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dice {
    pub player: Player,
    pub tips: u8,
    pub value: u8,
}

/// Where an animating piece currently is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PieceAnimation {
    pub player: Player,
    pub mv: Move,
    /// Current path position, `mv.from + 1 ..= mv.to` (may be `FINISH`).
    /// Hops square by square, for the terminal.
    pub pos: u8,
    /// Squares travelled so far, `0.0 ..= steps`, moving smoothly: for
    /// front ends that can draw a piece between squares.
    pub travelled: f32,
    /// The piece has reached its destination.
    pub landed: bool,
    /// Landed on an enemy piece (capture flash).
    pub impact: bool,
}

/// Advice for the current move choice, aligned with the options list.
#[derive(Clone, Debug, PartialEq)]
pub struct Hint {
    /// Index of the move the AI recommends.
    pub best: usize,
    /// How much worse than the best move each option is, in evaluation
    /// points (roughly one point per square of progress). `0.0` for the best.
    pub loss: Vec<f64>,
    /// A few words on each option: why the best move is good, or what each
    /// other move gives up compared with it.
    pub reasons: Vec<String>,
}

/// A legal move together with what it would cause, for the move list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveOption {
    pub mv: Move,
    pub outcome: Outcome,
    /// How many of the opponent's 16 equally likely throws could capture
    /// the moved piece on their next turn. See [`risk_percent`].
    pub risk: u8,
}

/// A risk in sixteenths as a whole percentage: 5 → 31.
pub fn risk_percent(sixteenths: u8) -> u32 {
    (u32::from(sixteenths) * 100 + 8) / 16
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Waiting for the current player to throw the dice.
    AwaitRoll,
    /// Dice thrown; the current player picks one of `options`.
    Choosing {
        roll: u8,
        options: Vec<MoveOption>,
        selected: usize,
    },
    /// The dice are tumbling; they will show `tips` when they settle.
    Rolling {
        tips: u8,
        started: Time,
    },
    /// A chosen move is being animated; it is applied when it finishes.
    Moving {
        roll: u8,
        option: MoveOption,
        started: Time,
    },
    /// Dice thrown but nothing can move; the turn passes shortly.
    NoMoves {
        roll: u8,
    },
    GameOver(Player),
}

pub struct App {
    pub screen: Screen,
    pub menu_selected: MenuItem,
    pub controllers: [Controller; 2],
    pub game: GameState,
    pub phase: Phase,
    pub dice: Option<Dice>,
    /// Newest entries last.
    pub log: Vec<String>,
    /// How many entries back from the newest the Chronicle is scrolled.
    pub log_scroll: usize,
    /// This game's running totals, per player.
    pub tally: [Tally; 2],
    pub leaderboard: Leaderboard,
    /// Text a front end should put on the clipboard (taken once).
    pub clipboard: Option<String>,
    pub show_help: bool,
    /// Advice shown for the current move choice, if the player asked.
    pub hint: Option<Hint>,
    pub should_quit: bool,
    pub animations: bool,
    pub sound_enabled: bool,
    pub sound_volume: f32,
    pub sound_cues: Vec<Cue>,
    /// Last completed animation step whose sound was handled.
    sound_step: u8,
    /// Time of the latest input or update; animations are drawn relative to it.
    pub now: Time,
    /// The game in progress (or saved from an earlier run) that "Continue"
    /// resumes. Kept up to date at every save point.
    pub resumable: Option<SavedGame>,
    /// Where to save; `None` keeps saves in memory only.
    preferences_path: Option<PathBuf>,
    preferences: Preferences,
    preferences_failed: bool,
    save_path: Option<PathBuf>,
    save_failed: bool,
    /// Where the leaderboard is kept; `None` keeps it in memory only.
    stats_path: Option<PathBuf>,
    /// Where shared chronicles are written; `None` only copies them.
    chronicle_dir: Option<PathBuf>,
    deadline: Option<Time>,
    rng: StdRng,
}

impl App {
    pub fn new(seed: u64) -> Self {
        App {
            screen: Screen::Menu,
            menu_selected: MenuItem::Start,
            controllers: [Controller::Human, Controller::Computer(Difficulty::Medium)],
            game: GameState::new(),
            phase: Phase::AwaitRoll,
            dice: None,
            log: Vec::new(),
            log_scroll: 0,
            tally: [Tally::default(); 2],
            leaderboard: Leaderboard::default(),
            clipboard: None,
            stats_path: None,
            chronicle_dir: None,
            show_help: false,
            hint: None,
            should_quit: false,
            animations: true,
            sound_enabled: true,
            sound_volume: 0.5,
            sound_cues: Vec::new(),
            sound_step: 0,
            now: Time::ZERO,
            resumable: None,
            preferences_path: None,
            preferences: Preferences::default(),
            preferences_failed: false,
            save_path: None,
            save_failed: false,
            deadline: None,
            rng: StdRng::seed_from_u64(seed),
        }
    }

    pub fn use_preferences(&mut self, path: PathBuf) {
        let saved = preferences::load(&path);
        self.sound_enabled = saved.sound;
        self.sound_volume = saved.volume;
        self.animations = saved.animations;
        self.preferences = saved;
        self.preferences_path = Some(path);
    }

    /// Only explicit UI changes update saved preferences. CLI overrides stay
    /// temporary, including when the user adjusts a different preference.
    fn save_preferences(&mut self) {
        let Some(path) = &self.preferences_path else {
            return;
        };
        if preferences::store(path, self.preferences).is_err() {
            if !self.preferences_failed {
                self.push_log("Could not save preferences; changes apply for this session.".into());
            }
            self.preferences_failed = true;
        } else {
            self.preferences_failed = false;
        }
    }

    /// Keep the leaderboard in `path`, starting from what's there.
    pub fn use_stats_file(&mut self, path: PathBuf) {
        self.leaderboard = stats::load(&path);
        self.stats_path = Some(path);
    }

    /// Write shared chronicles into `dir`.
    pub fn use_chronicle_dir(&mut self, dir: PathBuf) {
        self.chronicle_dir = Some(dir);
    }

    /// Save games to `path`, and offer to continue the game saved there.
    pub fn use_save_file(&mut self, path: PathBuf) {
        self.resumable = save::load(&path);
        self.save_path = Some(path);
        if self.resumable.is_some() {
            self.menu_selected = MenuItem::Continue;
        }
    }

    /// The menu rows currently shown.
    pub fn menu_items(&self) -> Vec<MenuItem> {
        MenuItem::ALL
            .into_iter()
            .filter(|&m| m != MenuItem::Continue || self.resumable.is_some())
            .collect()
    }

    /// When the app next wants [`update`](Self::update) called, if ever.
    pub fn deadline(&self) -> Option<Time> {
        self.deadline
    }

    pub fn controller(&self, player: Player) -> Controller {
        self.controllers[player.index()]
    }

    pub fn current_is_human(&self) -> bool {
        self.controller(self.game.turn()) == Controller::Human
    }

    /// The move currently highlighted (chosen by the AI, or under the human's
    /// cursor).
    pub fn selected_move(&self) -> Option<MoveOption> {
        match &self.phase {
            Phase::Choosing {
                options, selected, ..
            } => options.get(*selected).copied(),
            _ => None,
        }
    }

    /// Options in the move-choosing phase (empty otherwise).
    #[cfg(test)]
    pub fn options(&self) -> &[MoveOption] {
        match &self.phase {
            Phase::Choosing { options, .. } => options,
            _ => &[],
        }
    }

    /// Number of moves to choose from (0 outside the choosing phase).
    pub fn options_len(&self) -> usize {
        match &self.phase {
            Phase::Choosing { options, .. } => options.len(),
            _ => 0,
        }
    }

    /// The highlighted move while choosing, or the move being animated.
    pub fn selected_or_moving(&self) -> Option<MoveOption> {
        match self.phase {
            Phase::Moving { option, .. } => Some(option),
            _ => self.selected_move(),
        }
    }

    pub fn is_animating(&self) -> bool {
        self.screen == Screen::Game
            && matches!(self.phase, Phase::Rolling { .. } | Phase::Moving { .. })
    }

    /// Dice faces to show while tumbling: `(tips, settled)`. Flickers fast at
    /// first and slows down, then settles on the real throw.
    pub fn tumbling_dice(&self) -> Option<(u8, bool)> {
        let Phase::Rolling { tips, started } = self.phase else {
            return None;
        };
        let elapsed = self.now.saturating_sub(started);
        if elapsed >= DICE_ROLL_TIME.mul_f32(0.8) {
            return Some((tips, true));
        }
        // sqrt makes the face changes come further and further apart.
        let frame = ((elapsed.as_millis() as f64).sqrt() * 1.5) as u64;
        let salt = self.log.len() as u64;
        Some((
            (mix(frame.wrapping_add(salt.wrapping_mul(0x9E37))) & 0b1111) as u8,
            false,
        ))
    }

    pub fn piece_animation(&self) -> Option<PieceAnimation> {
        let Phase::Moving {
            option, started, ..
        } = self.phase
        else {
            return None;
        };
        let mv = option.mv;
        let steps = (mv.to - mv.from) as u128;
        let elapsed = self.now.saturating_sub(started).as_millis();
        let k = (elapsed / STEP_TIME.as_millis() + 1).min(steps);
        let landed = k == steps;
        let travelled = (elapsed as f32 / STEP_TIME.as_millis() as f32).min(steps as f32);
        Some(PieceAnimation {
            player: self.game.turn(),
            mv,
            pos: mv.from + k as u8,
            travelled,
            landed,
            impact: landed && option.outcome.captured,
        })
    }

    // ------------------------------------------------------------------
    // Input
    // ------------------------------------------------------------------

    /// Point the move cursor at option `index` (mouse hover). Only while a
    /// human is choosing.
    pub fn select_option(&mut self, index: usize) {
        if !self.current_is_human() || self.show_help {
            return;
        }
        if let Phase::Choosing {
            options, selected, ..
        } = &mut self.phase
            && index < options.len()
        {
            *selected = index;
        }
    }

    /// Play option `index` (mouse click). Only while a human is choosing.
    pub fn play_option(&mut self, index: usize, now: Time) {
        self.now = now;
        if self.current_is_human() && !self.show_help && self.screen == Screen::Game {
            self.play(index, now);
        }
    }

    fn cue(&mut self, cue: Cue) {
        if self.sound_enabled && self.sound_cues.len() < 16 {
            self.sound_cues.push(cue);
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent, now: Time) {
        self.now = now;
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }
        if self.show_help {
            self.show_help = false;
            return;
        }
        match key.code {
            KeyCode::Char('m') => {
                self.sound_enabled = !self.sound_enabled;
                self.preferences.sound = self.sound_enabled;
                self.save_preferences();
                self.sound_cues.clear();
                return;
            }
            KeyCode::Char('-' | '+') => {
                let delta = if key.code == KeyCode::Char('-') {
                    -0.1
                } else {
                    0.1
                };
                self.sound_volume = (self.sound_volume + delta).clamp(0.0, 1.0);
                self.preferences.volume = self.sound_volume;
                self.save_preferences();
                return;
            }
            KeyCode::Char('?') | KeyCode::F(1) => {
                self.show_help = true;
                return;
            }
            KeyCode::Char('q') => {
                self.should_quit = true;
                return;
            }
            _ => {}
        }
        match self.screen {
            Screen::Menu => self.menu_key(key.code, now),
            Screen::Game => self.game_key(key.code, now),
            Screen::Leaderboard => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char(' ')) {
                    self.screen = Screen::Menu;
                    self.cue(Cue::Menu);
                }
            }
        }
    }

    /// Scroll the Chronicle: positive goes back in time.
    pub fn scroll_log(&mut self, entries: isize) {
        let max = self.log.len().saturating_sub(1);
        self.log_scroll = self.log_scroll.saturating_add_signed(entries).min(max);
    }

    /// The game so far as plain text, for sharing.
    pub fn chronicle_text(&self, secs: u64) -> String {
        let throws = self.tally[0].throws + self.tally[1].throws;
        let result = match self.phase {
            Phase::GameOver(winner) => format!("{winner} won after {throws} throws."),
            _ => format!("In progress after {throws} throws."),
        };
        let mut out = format!(
            "The Royal Game of Ur: game record\n\
             Light: {}\n\
             Dark: {}\n\
             Result: {result}\n\
             Recorded: {}\n\n",
            self.controllers[0].label(),
            self.controllers[1].label(),
            chronicle::timestamp(secs),
        );
        for line in &self.log {
            out += line;
            out.push('\n');
        }
        out
    }

    /// Write the Chronicle to a file and hand it to the clipboard.
    pub fn share_chronicle(&mut self) {
        let secs = chronicle::now_secs();
        let text = self.chronicle_text(secs);
        let message = match &self.chronicle_dir {
            Some(dir) => match chronicle::write(dir, secs, &text) {
                Ok(path) => format!(
                    "Chronicle saved to {} and copied to the clipboard.",
                    path.display()
                ),
                Err(e) => format!("Couldn't save the Chronicle ({e}); copied to the clipboard."),
            },
            None => "Chronicle copied to the clipboard.".to_string(),
        };
        self.clipboard = Some(text);
        self.push_log(message);
    }

    fn menu_key(&mut self, code: KeyCode, now: Time) {
        let before = (
            self.menu_selected,
            self.controllers,
            self.animations,
            self.screen,
        );
        let items = self.menu_items();
        let n = items.len();
        let i = items
            .iter()
            .position(|&m| m == self.menu_selected)
            .unwrap_or(0);
        let item = items[i];
        match code {
            KeyCode::Up | KeyCode::Char('k') => self.menu_selected = items[(i + n - 1) % n],
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                self.menu_selected = items[(i + 1) % n]
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') | KeyCode::Enter => {
                let step = if code == KeyCode::Left { -1 } else { 1 };
                match item {
                    MenuItem::Light => self.controllers[0] = self.controllers[0].cycle(step),
                    MenuItem::Dark => self.controllers[1] = self.controllers[1].cycle(step),
                    MenuItem::Animation => {
                        self.animations = !self.animations;
                        self.preferences.animations = self.animations;
                        self.save_preferences();
                    }
                    MenuItem::Continue if matches!(code, KeyCode::Enter | KeyCode::Char(' ')) => {
                        self.resume(now);
                    }
                    MenuItem::Leaderboard
                        if matches!(code, KeyCode::Enter | KeyCode::Char(' ')) =>
                    {
                        self.screen = Screen::Leaderboard;
                    }
                    MenuItem::Start if matches!(code, KeyCode::Enter | KeyCode::Char(' ')) => {
                        self.new_game(now)
                    }
                    MenuItem::Quit if matches!(code, KeyCode::Enter | KeyCode::Char(' ')) => {
                        self.should_quit = true
                    }
                    _ => {}
                }
            }
            KeyCode::Esc => self.should_quit = true,
            _ => {}
        }
        let after = (
            self.menu_selected,
            self.controllers,
            self.animations,
            self.screen,
        );
        if before != after {
            self.cue(Cue::Menu);
        }
    }

    fn game_key(&mut self, code: KeyCode, now: Time) {
        if code == KeyCode::Esc {
            self.screen = Screen::Menu;
            self.deadline = None;
            if self.resumable.is_some() {
                self.menu_selected = MenuItem::Continue;
            }
            return;
        }
        match code {
            KeyCode::PageUp | KeyCode::Char('[') => {
                return self.scroll_log(LOG_SCROLL_STEP as isize);
            }
            KeyCode::PageDown | KeyCode::Char(']') => {
                return self.scroll_log(-(LOG_SCROLL_STEP as isize));
            }
            KeyCode::End => return self.log_scroll = 0,
            KeyCode::Char('e') => return self.share_chronicle(),
            _ => {}
        }
        let human = self.current_is_human();
        let confirm = matches!(code, KeyCode::Enter | KeyCode::Char(' '));
        match &mut self.phase {
            Phase::GameOver(_) => {
                if confirm || code == KeyCode::Char('n') {
                    self.new_game(now);
                }
            }
            // Space/Enter skips an animation, for anyone.
            Phase::Rolling { .. } | Phase::Moving { .. } => {
                if confirm {
                    self.deadline = None;
                    self.advance(now);
                }
            }
            // Space hurries the computer along.
            _ if !human => {
                if confirm && self.deadline.is_some() {
                    self.deadline = None;
                    self.advance(now);
                }
            }
            Phase::AwaitRoll => {
                if confirm || code == KeyCode::Char('r') {
                    self.roll(now);
                }
            }
            Phase::NoMoves { .. } => {
                if confirm {
                    self.pass(now);
                }
            }
            Phase::Choosing {
                options, selected, ..
            } => {
                let n = options.len();
                match code {
                    KeyCode::Left | KeyCode::Up | KeyCode::BackTab | KeyCode::Char('k') => {
                        *selected = (*selected + n - 1) % n
                    }
                    KeyCode::Right | KeyCode::Down | KeyCode::Tab | KeyCode::Char('j') => {
                        *selected = (*selected + 1) % n
                    }
                    KeyCode::Char(c @ '1'..='9') => {
                        let i = c as usize - '1' as usize;
                        if i < n {
                            self.play(i, now);
                        }
                    }
                    KeyCode::Char('h') => self.show_hint(),
                    _ if confirm => {
                        let i = *selected;
                        self.play(i, now);
                    }
                    _ => {}
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Time-driven progress (computer turns, auto-pass)
    // ------------------------------------------------------------------

    pub fn update(&mut self, now: Time) {
        self.now = now;
        if self.screen != Screen::Game || self.show_help {
            return;
        }
        self.movement_sound(now, false);
        if self.deadline.is_some_and(|d| now >= d) {
            self.deadline = None;
            self.advance(now);
        }
    }

    /// Emit at most one hop per update; delayed frames never replay a burst.
    /// Final effects follow physical arrival, before the post-move pause.
    fn movement_sound(&mut self, now: Time, finish: bool) {
        let Phase::Moving {
            option, started, ..
        } = self.phase
        else {
            return;
        };
        let steps = option.mv.to - option.mv.from;
        let reached = if finish {
            steps
        } else {
            (now.saturating_sub(started).as_millis() / STEP_TIME.as_millis()).min(u128::from(steps))
                as u8
        };
        if reached <= self.sound_step {
            return;
        }
        self.sound_step = reached;
        if reached == steps {
            self.landing_sound(option);
        } else {
            self.cue(Cue::Hop);
        }
    }

    fn landing_sound(&mut self, option: MoveOption) {
        self.cue(if option.outcome.captured {
            Cue::Capture
        } else if option.mv.is_bear_off() {
            Cue::Home
        } else {
            Cue::Move
        });
        if option.outcome.winner.is_some() {
            self.cue(Cue::Victory);
        } else if option.outcome.extra_turn {
            self.cue(Cue::Rosette);
        }
    }

    /// Take the next automatic step for the current phase.
    fn advance(&mut self, now: Time) {
        match self.phase {
            Phase::AwaitRoll if !self.current_is_human() => self.roll(now),
            Phase::Choosing { selected, .. } if !self.current_is_human() => {
                self.play(selected, now)
            }
            Phase::NoMoves { .. } => self.pass(now),
            Phase::Rolling { tips, .. } => self.resolve_roll(tips, now),
            Phase::Moving { roll, option, .. } => self.commit(roll, option, now),
            _ => {}
        }
    }

    // ------------------------------------------------------------------
    // Game flow
    // ------------------------------------------------------------------

    pub fn new_game(&mut self, now: Time) {
        self.screen = Screen::Game;
        self.game = GameState::new();
        self.dice = None;
        self.log.clear();
        self.log_scroll = 0;
        self.tally = [Tally::default(); 2];
        self.hint = None;
        self.push_log(format!(
            "New game: Light ({}) vs Dark ({}).",
            self.controllers[0].label(),
            self.controllers[1].label()
        ));
        self.start_turn(now);
    }

    /// Pick up the saved game. Returns `false` if there is none.
    pub fn resume(&mut self, now: Time) -> bool {
        let Some(saved) = self.resumable.clone() else {
            return false;
        };
        self.screen = Screen::Game;
        self.controllers = saved.controllers;
        self.game = saved.game;
        self.log = saved.log;
        self.log_scroll = 0;
        self.tally = saved.tally;
        self.dice = None;
        self.hint = None;
        match saved.pending_tips {
            // The dice were already thrown: show the same throw again.
            Some(tips) => self.resolve_roll(tips, now),
            None => self.start_turn(now),
        }
        true
    }

    fn start_turn(&mut self, now: Time) {
        self.phase = Phase::AwaitRoll;
        self.deadline = (!self.current_is_human()).then(|| now + AI_DELAY);
        self.autosave(None);
    }

    fn roll(&mut self, now: Time) {
        self.cue(Cue::Dice);
        let tips = (self.rng.next_u32() & 0b1111) as u8;
        let tally = &mut self.tally[self.game.turn().index()];
        tally.throws += 1;
        tally.pips += u32::from(roll_from_tips(tips.into()));
        // Saved before anyone sees it, so quitting can't undo a bad throw.
        self.autosave(Some(tips));
        if self.animations {
            self.phase = Phase::Rolling { tips, started: now };
            self.deadline = Some(now + DICE_ROLL_TIME);
        } else {
            self.resolve_roll(tips, now);
        }
    }

    /// The dice have settled on `tips`: work out what the player can do.
    fn resolve_roll(&mut self, tips: u8, now: Time) {
        self.hint = None;
        let player = self.game.turn();
        let roll = roll_from_tips(tips as u32);
        self.dice = Some(Dice {
            player,
            tips,
            value: roll,
        });

        let moves = self.game.legal_moves(roll);
        if moves.is_empty() {
            let why = if roll == 0 {
                "no move"
            } else {
                "no legal move"
            };
            let n = self.throw_number();
            self.push_log(format!("{n}. {player} rolls {roll}: {why}, turn passes."));
            self.phase = Phase::NoMoves { roll };
            self.deadline = Some(now + PASS_DELAY);
            return;
        }

        let options: Vec<MoveOption> = moves
            .iter()
            .map(|&mv| {
                let mut preview = self.game;
                let outcome = preview.apply(roll, mv).expect("legal move");
                let risk = preview.capture_ways(player, mv.to);
                MoveOption { mv, outcome, risk }
            })
            .collect();
        let selected = match self.controller(player) {
            Controller::Human => 0,
            Controller::Computer(d) => {
                let choice = ai::choose_move(&self.game, roll, d, &mut self.rng);
                options
                    .iter()
                    .position(|o| Some(o.mv) == choice)
                    .unwrap_or(0)
            }
        };
        self.phase = Phase::Choosing {
            roll,
            options,
            selected,
        };
        self.deadline = (!self.current_is_human()).then(|| now + AI_DELAY);
    }

    fn play(&mut self, index: usize, now: Time) {
        let Phase::Choosing {
            roll, ref options, ..
        } = self.phase
        else {
            return;
        };
        let Some(&option) = options.get(index) else {
            return;
        };
        self.hint = None;
        self.sound_step = 0;
        if self.animations {
            self.phase = Phase::Moving {
                roll,
                option,
                started: now,
            };
            self.deadline = Some(now + move_duration(option));
        } else {
            self.commit(roll, option, now);
        }
    }

    /// Ask the AI to rate every option, and move the cursor to its pick.
    fn show_hint(&mut self) {
        let Phase::Choosing { roll, .. } = self.phase else {
            return;
        };
        // Same order as `options`: both come from `legal_moves`.
        let scores: Vec<f64> = ai::rank_moves(&self.game, roll, HINT_LEVEL)
            .into_iter()
            .map(|(_, score)| score)
            .collect();
        let best = (0..scores.len()).fold(0, |b, i| if scores[i] > scores[b] { i } else { b });
        let loss: Vec<f64> = scores.iter().map(|s| scores[best] - s).collect();
        if let Phase::Choosing {
            options, selected, ..
        } = &mut self.phase
        {
            *selected = best;
            let traits: Vec<Traits> = options
                .iter()
                .map(|o| Traits::of(&self.game, roll, o))
                .collect();
            let reasons: Vec<String> = (0..options.len())
                .map(|i| {
                    if i == best {
                        traits[best].why_good()
                    } else {
                        traits[i].why_worse(&traits[best], loss[i])
                    }
                })
                .collect();
            let text = format!(
                "Hint for {}: {} ({}).",
                self.game.turn(),
                describe_move(options[best].mv),
                reasons[best]
            );
            if self.hint.is_none() {
                self.tally[self.game.turn().index()].hints += 1;
            }
            self.hint = Some(Hint {
                best,
                loss,
                reasons,
            });
            self.push_log(text);
        }
    }

    /// The hint's reason for the highlighted move, and whether that move is
    /// the recommended one. `None` without a hint.
    pub fn hint_reason(&self) -> Option<(bool, &str)> {
        let (Some(hint), Phase::Choosing { selected, .. }) = (&self.hint, &self.phase) else {
            return None;
        };
        let reason = hint.reasons.get(*selected)?;
        Some((*selected == hint.best, reason))
    }

    /// Apply a move to the game and log it.
    fn commit(&mut self, roll: u8, option: MoveOption, now: Time) {
        let mv = option.mv;
        let player = self.game.turn();
        let outcome = self
            .game
            .apply(roll, mv)
            .expect("move came from legal_moves");
        if matches!(self.phase, Phase::Moving { .. }) {
            self.movement_sound(now, true);
        } else {
            self.landing_sound(option);
        }
        let n = self.throw_number();
        let mut text = format!("{n}. {player} rolls {roll}: {}", describe_move(mv));
        let tally = &mut self.tally[player.index()];
        if outcome.captured {
            text.push_str(", captures");
            tally.captures += 1;
        }
        if outcome.extra_turn {
            text.push_str(", rosette: rolls again");
            tally.rosettes += 1;
        }
        text.push('.');
        self.push_log(text);

        if let Some(winner) = outcome.winner {
            self.finish_game(winner);
            self.phase = Phase::GameOver(winner);
            self.deadline = None;
            self.clear_save();
        } else {
            self.start_turn(now);
        }
    }

    /// The number of the throw being played, counting both players.
    fn throw_number(&self) -> u32 {
        self.tally[0].throws + self.tally[1].throws
    }

    /// Sum up the game in the Chronicle and record it on the leaderboard.
    fn finish_game(&mut self, winner: Player) {
        let n = self.throw_number();
        self.push_log(format!("{winner} wins the game after {n} throws!"));
        for player in Player::ALL {
            let t = self.tally[player.index()];
            let average = if t.throws > 0 {
                f64::from(t.pips) / f64::from(t.throws)
            } else {
                0.0
            };
            let plural =
                |n: u32, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
            let mut line = format!(
                "{player}: {}, average {average:.2}, {}, {}",
                plural(t.throws, "throw"),
                plural(t.captures, "capture"),
                plural(t.rosettes, "rosette"),
            );
            if t.hints > 0 {
                line += &format!(", {}", plural(t.hints, "hint"));
            }
            line.push('.');
            self.push_log(line);
        }
        // Only human-vs-computer games count, by the computer's level.
        let (human, level) = match self.controllers {
            [Controller::Human, Controller::Computer(d)] => (Player::Light, d),
            [Controller::Computer(d), Controller::Human] => (Player::Dark, d),
            _ => return,
        };
        self.leaderboard.add(level, winner == human);
        let r = self.leaderboard.record(level);
        self.push_log(format!(
            "Your record against {level}: {} of {} won ({}%).",
            r.won,
            r.played,
            r.win_percent().unwrap_or(0)
        ));
        if let Some(path) = &self.stats_path
            && let Err(e) = stats::store(path, &self.leaderboard)
        {
            self.push_log(format!("Couldn't save the leaderboard: {e}."));
        }
    }

    fn pass(&mut self, now: Time) {
        let Phase::NoMoves { roll } = self.phase else {
            return;
        };
        self.game.pass(roll).expect("no legal moves");
        self.start_turn(now);
    }

    /// Record the game as it stands (with a throw not yet played, if any).
    fn autosave(&mut self, pending_tips: Option<u8>) {
        let saved = SavedGame {
            controllers: self.controllers,
            game: self.game,
            pending_tips,
            log: self.log.clone(),
            tally: self.tally,
        };
        if let Some(path) = &self.save_path
            && let Err(e) = save::store(path, &saved)
            && !self.save_failed
        {
            // Say so once; the game itself carries on fine.
            self.save_failed = true;
            self.push_log(format!("Couldn't save the game: {e}."));
        }
        self.resumable = Some(saved);
    }

    fn clear_save(&mut self) {
        self.resumable = None;
        if let Some(path) = &self.save_path {
            // A stale save would only offer a finished game; nothing to do.
            let _ = save::remove(path);
        }
    }

    fn push_log(&mut self, line: String) {
        self.log.push(line);
        // Keep a scrolled-back view where it is.
        if self.log_scroll > 0 {
            self.log_scroll += 1;
        }
        if self.log.len() > LOG_CAPACITY {
            self.log.remove(0);
        }
        self.log_scroll = self.log_scroll.min(self.log.len().saturating_sub(1));
    }
}

/// Total length of a move animation.
pub fn move_duration(option: MoveOption) -> Duration {
    let steps = u32::from(option.mv.to - option.mv.from);
    let impact = if option.outcome.captured {
        IMPACT_TIME
    } else {
        Duration::ZERO
    };
    STEP_TIME * steps.saturating_sub(1) + LAND_TIME + impact
}

/// SplitMix64 finaliser: cheap, well-scrambled bits for the dice flicker.
fn mix(mut x: u64) -> u64 {
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D049BB133111EB);
    x ^ (x >> 31)
}

/// What a move does, in the terms a hint explains it with.
struct Traits {
    mv: Move,
    wins: bool,
    captures: bool,
    extra_turn: bool,
    /// Capture risk (sixteenths) of the moving piece before and after.
    risk_before: u8,
    risk_after: u8,
    /// Ways (sixteenths) the moved piece could hit an enemy piece ahead of
    /// it on the mover's next throw.
    threat: u8,
}

impl Traits {
    fn of(game: &GameState, roll: u8, option: &MoveOption) -> Self {
        let me = game.turn();
        let mv = option.mv;
        let mut after = *game;
        after.apply(roll, mv).expect("legal move");
        let threat = (1..=4u8)
            .filter(|&d| {
                let target = mv.to + d;
                is_shared(target) && target != SAFE_ROSETTE && after.occupies(me.opponent(), target)
            })
            .map(|d| ROLL_WAYS[d as usize])
            .sum();
        Traits {
            mv,
            wins: option.outcome.winner.is_some(),
            captures: option.outcome.captured,
            extra_turn: option.outcome.extra_turn,
            risk_before: game.capture_ways(me, mv.from),
            risk_after: option.risk,
            threat,
        }
    }

    fn escapes(&self) -> bool {
        self.risk_before > 0 && self.risk_after < self.risk_before
    }

    /// Why the recommended move is good: its most important effect.
    fn why_good(&self) -> String {
        if self.wins {
            "wins the game".into()
        } else if self.captures {
            format!("captures on {}", self.mv.to)
        } else if self.escapes() {
            format!("escapes {}% danger", risk_percent(self.risk_before))
        } else if self.mv.to == SAFE_ROSETTE {
            "takes middle rosette".into()
        } else if self.extra_turn {
            "extra throw".into()
        } else if self.mv.to == FINISH {
            "bears a piece off".into()
        } else if self.threat > 0 && self.risk_after == 0 {
            format!("sets up a {}% hit", risk_percent(self.threat))
        } else if self.risk_after == 0 {
            "safe progress".into()
        } else {
            "least risky choice".into()
        }
    }

    /// What this move gives up compared with the recommended `best`.
    fn why_worse(&self, best: &Traits, loss: f64) -> String {
        if loss < 0.5 {
            "nearly as good".into()
        } else if best.wins && !self.wins {
            "misses the win".into()
        } else if best.captures && !self.captures {
            format!("misses capture on {}", best.mv.to)
        } else if self.mv.from == SAFE_ROSETTE && best.mv.from != SAFE_ROSETTE {
            "gives up mid rosette".into()
        } else if best.escapes() && self.mv.from != best.mv.from {
            format!(
                "leaves {} at {}% risk",
                best.mv.from,
                risk_percent(best.risk_before)
            )
        } else if self.risk_after > best.risk_after {
            format!("{}% risk of capture", risk_percent(self.risk_after))
        } else if best.mv.to == SAFE_ROSETTE && self.mv.to != SAFE_ROSETTE {
            "misses middle rosette".into()
        } else if best.extra_turn && !self.extra_turn {
            "misses extra throw".into()
        } else if best.mv.to == FINISH && self.mv.to != FINISH {
            "doesn't bear off".into()
        } else if best.threat > self.threat {
            if self.threat == 0 {
                "sets up no hit".into()
            } else {
                "weaker hit chance".into()
            }
        } else {
            "weaker a few turns on".into()
        }
    }
}

/// Which player a Chronicle entry is about, if it's one of their throws or
/// hints: "12. Light rolls 3: …", "Hint for Dark: …".
pub fn entry_player(entry: &str) -> Option<Player> {
    let rest = entry.trim_start_matches(|c: char| c.is_ascii_digit());
    let rest = rest.strip_prefix(". ").unwrap_or(rest);
    let rest = rest.strip_prefix("Hint for ").unwrap_or(rest);
    Player::ALL
        .into_iter()
        .find(|p| rest.starts_with(&format!("{p} ")) || rest.starts_with(&format!("{p}:")))
}

/// Human-readable move, using the mover's path square numbers.
pub fn describe_move(mv: Move) -> String {
    match (mv.from, mv.to) {
        (START, to) => format!("enters a piece on {to}"),
        (from, FINISH) => format!("bears off from {from}"),
        (from, to) => format!("moves {from} → {to}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::from(code)
    }

    /// A game in instant mode (no animations), for testing turn flow.
    fn game_app(light: Controller, dark: Controller) -> (App, Time) {
        let mut app = App::new(42);
        app.controllers = [light, dark];
        app.animations = false;
        let now = Time::ZERO;
        app.new_game(now);
        (app, now)
    }

    #[test]
    fn animated_sound_follows_arrival_without_duplicates_or_catchup_bursts() {
        let (mut app, now) = game_app(Controller::Human, Controller::Human);
        app.animations = true;
        app.resolve_roll(0b1111, now);
        app.play(0, now);
        app.update(STEP_TIME);
        assert_eq!(std::mem::take(&mut app.sound_cues), vec![Cue::Hop]);
        app.update(STEP_TIME);
        assert!(app.sound_cues.is_empty());
        app.update(STEP_TIME * 3);
        assert_eq!(std::mem::take(&mut app.sound_cues), vec![Cue::Hop]);
        app.update(STEP_TIME * 4);
        assert_eq!(
            std::mem::take(&mut app.sound_cues),
            vec![Cue::Move, Cue::Rosette]
        );
        app.update(Time::from_secs(1));
        assert!(app.sound_cues.is_empty());
    }

    #[test]
    fn skipping_a_move_plays_landing_once_without_intermediate_hops() {
        let (mut app, now) = game_app(Controller::Human, Controller::Human);
        app.animations = true;
        app.resolve_roll(0b1111, now);
        app.play(0, now);
        app.handle_key(key(KeyCode::Enter), now);
        assert_eq!(
            std::mem::take(&mut app.sound_cues),
            vec![Cue::Move, Cue::Rosette]
        );
        app.update(Time::from_secs(1));
        assert!(app.sound_cues.is_empty());
    }

    #[test]
    fn bearing_off_has_a_home_cue_and_winning_adds_victory() {
        for winning in [false, true] {
            let (mut app, now) = game_app(Controller::Human, Controller::Human);
            let mut light = if winning { [FINISH; 7] } else { [START; 7] };
            light[0] = 14;
            app.game = GameState::from_positions(Player::Light, light, [START; 7]).unwrap();
            app.resolve_roll(1, now);
            let off = app
                .options()
                .iter()
                .position(|o| o.mv.is_bear_off())
                .unwrap();
            app.play(off, now);
            let expected = if winning {
                vec![Cue::Home, Cue::Victory]
            } else {
                vec![Cue::Home]
            };
            assert_eq!(app.sound_cues, expected);
        }
    }

    #[test]
    fn menu_ticks_only_when_keyboard_input_changes_something() {
        let mut app = App::new(9);
        app.handle_key(key(KeyCode::Char('x')), Time::ZERO);
        assert!(app.sound_cues.is_empty());
        app.handle_key(key(KeyCode::Up), Time::ZERO);
        assert_eq!(std::mem::take(&mut app.sound_cues), vec![Cue::Menu]);
        app.handle_key(key(KeyCode::Char('m')), Time::ZERO);
        app.handle_key(key(KeyCode::Down), Time::ZERO);
        assert!(app.sound_cues.is_empty());
    }

    #[test]
    fn sound_events_follow_actions_and_muting_discards_pending_cues() {
        let mut app = App::new(9);
        app.controllers = [Controller::Human; 2];
        app.animations = false;
        app.new_game(Time::ZERO);
        for _ in 0..32 {
            app.roll(Time::ZERO);
            assert_eq!(app.sound_cues.remove(0), Cue::Dice);
            if app.options_len() > 0 {
                let outcome = app.selected_move().unwrap().outcome;
                app.play(0, Time::ZERO);
                let expected = if outcome.captured {
                    Cue::Capture
                } else {
                    Cue::Move
                };
                assert_eq!(app.sound_cues.remove(0), expected);
                break;
            }
            app.pass(Time::ZERO);
        }
        app.handle_key(key(KeyCode::Char('m')), Time::ZERO);
        assert!(!app.sound_enabled);
        assert!(app.sound_cues.is_empty());
        app.cue(Cue::Dice);
        assert!(app.sound_cues.is_empty());
        for _ in 0..20 {
            app.handle_key(key(KeyCode::Char('-')), Time::ZERO);
        }
        assert_eq!(app.sound_volume, 0.0);
        for _ in 0..20 {
            app.handle_key(key(KeyCode::Char('+')), Time::ZERO);
        }
        assert_eq!(app.sound_volume, 1.0);
    }

    #[test]
    fn menu_cycles_controllers_and_starts() {
        let mut app = App::new(1);
        let now = Time::ZERO;
        assert_eq!(app.screen, Screen::Menu);
        app.handle_key(key(KeyCode::Up), now); // Animation
        app.handle_key(key(KeyCode::Enter), now);
        assert!(!app.animations);
        app.handle_key(key(KeyCode::Up), now); // Dark
        app.handle_key(key(KeyCode::Right), now);
        assert_eq!(app.controllers[1], Controller::Computer(Difficulty::Hard));
        app.handle_key(key(KeyCode::Right), now);
        assert_eq!(app.controllers[1], Controller::Human); // wraps
        app.handle_key(key(KeyCode::Down), now);
        app.handle_key(key(KeyCode::Down), now);
        app.handle_key(key(KeyCode::Enter), now);
        assert_eq!(app.screen, Screen::Game);
        assert_eq!(app.phase, Phase::AwaitRoll);
    }

    #[test]
    fn human_turn_roll_and_move() {
        let (mut app, now) = game_app(Controller::Human, Controller::Human);
        assert_eq!(app.deadline(), None, "humans are never auto-played");
        // Roll until we get a move (a 0 roll passes).
        for _ in 0..20 {
            app.handle_key(key(KeyCode::Char(' ')), now);
            match app.phase {
                Phase::Choosing { .. } => break,
                Phase::NoMoves { .. } => app.handle_key(key(KeyCode::Enter), now),
                _ => unreachable!(),
            }
        }
        let mover = app.game.turn();
        let Some(opt) = app.selected_move() else {
            panic!("expected a move")
        };
        app.handle_key(key(KeyCode::Enter), now);
        assert_eq!(app.game.waiting(mover), 6);
        assert!(app.game.occupies(mover, opt.mv.to));
        assert!(app.log.last().unwrap().contains("enters a piece"));
    }

    /// Light to move with `roll`, a hint requested; returns the app.
    fn hinted(light: [u8; 7], dark: [u8; 7], roll: u8) -> App {
        let mut app = App::new(1);
        app.animations = false;
        app.controllers = [Controller::Human, Controller::Human];
        app.screen = Screen::Game;
        app.game = GameState::from_positions(Player::Light, light, dark).unwrap();
        app.resolve_roll((1u8 << roll) - 1, Time::ZERO);
        app.handle_key(key(KeyCode::Char('h')), Time::ZERO);
        app
    }

    fn reason_for(app: &App, to: u8) -> String {
        let hint = app.hint.as_ref().unwrap();
        let i = app.options().iter().position(|o| o.mv.to == to).unwrap();
        hint.reasons[i].clone()
    }

    #[test]
    fn hint_explains_running_from_danger() {
        // Dark on 7 and 6 can hit Light's piece on 9 with a 2 or a 3.
        let app = hinted([9, 0, 0, 0, 0, 0, 0], [7, 6, 0, 0, 0, 0, 0], 3);
        assert_eq!(app.hint_reason(), Some((true, "escapes 63% danger")));
        assert_eq!(reason_for(&app, 3), "leaves 9 at 63% risk");
        assert!(app.log.last().unwrap().contains("(escapes 63% danger)"));
    }

    #[test]
    fn hint_explains_a_missed_capture() {
        let app = hinted([7, 0, 0, 0, 0, 0, 0], [10, 0, 0, 0, 0, 0, 0], 3);
        assert_eq!(app.hint_reason(), Some((true, "captures on 10")));
        assert_eq!(reason_for(&app, 3), "misses capture on 10");
    }

    #[test]
    fn hint_reason_follows_the_cursor() {
        let mut app = hinted([9, 0, 0, 0, 0, 0, 0], [7, 6, 0, 0, 0, 0, 0], 3);
        app.handle_key(key(KeyCode::Right), Time::ZERO);
        assert_eq!(app.hint_reason(), Some((false, "leaves 9 at 63% risk")));
    }

    #[test]
    fn move_options_carry_capture_risk() {
        let mut app = App::new(1);
        app.animations = false;
        app.controllers = [Controller::Human, Controller::Human];
        app.screen = Screen::Game;
        app.game =
            GameState::from_positions(Player::Light, [7, 0, 0, 0, 0, 0, 0], [6, 5, 0, 0, 0, 0, 0])
                .unwrap();
        app.resolve_roll(0b0011, Time::ZERO); // a 2
        let risk = |to| app.options().iter().find(|o| o.mv.to == to).unwrap().risk;
        // Dark reaches 9 with a 3 (from 6, 4 ways) or a 4 (from 5, 1 way).
        assert_eq!(risk(9), 5);
        assert_eq!(risk(2), 0); // entering: Light's own row
        assert_eq!(risk_percent(5), 31);
        assert_eq!(risk_percent(1), 6);
        assert_eq!(risk_percent(16), 100);
    }

    #[test]
    fn number_keys_pick_moves_and_selection_wraps() {
        let mut app = App::new(5);
        app.animations = false;
        app.controllers = [Controller::Human, Controller::Human];
        app.screen = Screen::Game;
        // Light has two pieces in play, so most rolls give several options.
        app.game = GameState::from_positions(Player::Light, [3, 6, 0, 0, 0, 0, 0], [0; 7]).unwrap();
        let now = Time::ZERO;
        let options: Vec<MoveOption> = app
            .game
            .legal_moves(1)
            .into_iter()
            .map(|mv| MoveOption {
                mv,
                outcome: Outcome::default(),
                risk: 0,
            })
            .collect();
        assert_eq!(options.len(), 3);
        app.phase = Phase::Choosing {
            roll: 1,
            options,
            selected: 0,
        };
        app.handle_key(key(KeyCode::Left), now);
        assert_eq!(app.selected_move().unwrap().mv, Move { from: 6, to: 7 });
        app.handle_key(key(KeyCode::Char('2')), now);
        assert!(app.game.occupies(Player::Light, 4), "3 → 4 played");
    }

    #[test]
    fn computer_waits_for_deadline() {
        let (mut app, now) = game_app(Controller::Computer(Difficulty::Easy), Controller::Human);
        assert_eq!(app.deadline(), Some(now + AI_DELAY));
        app.update(now + AI_DELAY / 2);
        assert_eq!(app.phase, Phase::AwaitRoll, "too early");
        app.update(now + AI_DELAY);
        assert!(matches!(
            app.phase,
            Phase::Choosing { .. } | Phase::NoMoves { .. }
        ));
        assert!(app.dice.is_some());
    }

    #[test]
    fn human_keys_ignored_on_computer_turn_except_hurry() {
        let (mut app, now) = game_app(Controller::Computer(Difficulty::Easy), Controller::Human);
        app.handle_key(key(KeyCode::Char('r')), now);
        assert_eq!(app.phase, Phase::AwaitRoll);
        app.handle_key(key(KeyCode::Char(' ')), now);
        assert_ne!(app.phase, Phase::AwaitRoll, "space hurries the AI");
    }

    #[test]
    fn computer_vs_computer_finishes() {
        let ai = Controller::Computer(Difficulty::Easy);
        let (mut app, mut now) = game_app(ai, ai);
        for _ in 0..20_000 {
            if matches!(app.phase, Phase::GameOver(_)) {
                break;
            }
            now += PASS_DELAY;
            app.update(now);
        }
        let Phase::GameOver(winner) = app.phase else {
            panic!("game never ended")
        };
        assert_eq!(app.game.winner(), Some(winner));
        assert!(
            app.log
                .iter()
                .any(|l| l.starts_with(&format!("{winner} wins the game after")))
        );
        assert!(app.log.iter().any(|l| l.starts_with("Light: ")));
        assert!(app.log.iter().any(|l| l.starts_with("Dark: ")));
        // Computer-vs-computer games stay off the leaderboard.
        assert_eq!(app.leaderboard.total().played, 0);
        assert_eq!(app.deadline(), None);
        app.handle_key(key(KeyCode::Char('n')), now);
        assert_eq!(app.game, GameState::new());
    }

    /// Play a whole game, the human always taking the first move offered.
    fn play_out(app: &mut App, mut now: Time) -> Player {
        for _ in 0..50_000 {
            if let Phase::GameOver(winner) = app.phase {
                return winner;
            }
            app.handle_key(key(KeyCode::Enter), now);
            now += PASS_DELAY;
            app.update(now);
        }
        panic!("game never ended");
    }

    #[test]
    fn finished_games_are_numbered_summed_up_and_recorded() {
        let (mut app, now) = game_app(Controller::Human, Controller::Computer(Difficulty::Easy));
        let winner = play_out(&mut app, now);
        let throws = app.tally[0].throws + app.tally[1].throws;
        assert!(throws > 10);
        // Throws are numbered from 1, both players together.
        assert!(app.log.iter().any(|l| l.starts_with("1. Light rolls")));
        assert!(
            app.log
                .iter()
                .any(|l| l.starts_with(&format!("{throws}. ")))
        );
        let pips: u32 = app.tally.iter().map(|t| t.pips).sum();
        assert!(pips > 0);
        assert!(
            app.log
                .iter()
                .any(|l| l == &format!("{winner} wins the game after {throws} throws!"))
        );
        let record = app.leaderboard.record(Difficulty::Easy);
        assert_eq!(record.played, 1);
        assert_eq!(record.won, u32::from(winner == Player::Light));
        let expected = format!(
            "Your record against Easy: {} of 1 won ({}%).",
            record.won,
            record.won * 100
        );
        assert_eq!(app.log.last(), Some(&expected));
        // A rematch counts again; human-vs-human doesn't count at all.
        app.handle_key(key(KeyCode::Char('n')), now);
        assert_eq!(app.tally, [Tally::default(); 2]);
        play_out(&mut app, now);
        assert_eq!(app.leaderboard.record(Difficulty::Easy).played, 2);
        let (mut friends, now) = game_app(Controller::Human, Controller::Human);
        play_out(&mut friends, now);
        assert_eq!(friends.leaderboard.total().played, 0);
    }

    #[test]
    fn leaderboard_is_saved_and_opened_from_the_menu() {
        let dir = std::env::temp_dir().join(format!("ur-board-{}", std::process::id()));
        let path = dir.join("stats.txt");
        let (mut app, now) = game_app(Controller::Computer(Difficulty::Hard), Controller::Human);
        app.use_stats_file(path.clone());
        play_out(&mut app, now);
        let mut again = App::new(0);
        again.use_stats_file(path);
        assert_eq!(again.leaderboard.record(Difficulty::Hard).played, 1);
        std::fs::remove_dir_all(dir).unwrap();

        again.menu_selected = MenuItem::Leaderboard;
        again.handle_key(key(KeyCode::Enter), now);
        assert_eq!(again.screen, Screen::Leaderboard);
        again.handle_key(key(KeyCode::Esc), now);
        assert_eq!(again.screen, Screen::Menu);
    }

    #[test]
    fn chronicle_scrolls_and_stays_put_while_scrolled() {
        let (mut app, now) = game_app(Controller::Human, Controller::Human);
        for i in 0..10 {
            app.push_log(format!("entry {i}"));
        }
        let len = app.log.len();
        app.handle_key(key(KeyCode::PageUp), now);
        assert_eq!(app.log_scroll, LOG_SCROLL_STEP);
        app.push_log("newer".into());
        assert_eq!(
            app.log_scroll,
            LOG_SCROLL_STEP + 1,
            "view stays on the same entries"
        );
        app.scroll_log(1000);
        assert_eq!(app.log_scroll, len, "clamped to the first entry");
        app.handle_key(key(KeyCode::Char(']')), now);
        assert_eq!(app.log_scroll, len - LOG_SCROLL_STEP);
        app.handle_key(key(KeyCode::End), now);
        assert_eq!(app.log_scroll, 0);
    }

    #[test]
    fn sharing_writes_a_record_and_fills_the_clipboard() {
        let dir = std::env::temp_dir().join(format!("ur-share-{}", std::process::id()));
        let (mut app, now) = game_app(Controller::Human, Controller::Computer(Difficulty::Medium));
        app.use_chronicle_dir(dir.clone());
        let winner = play_out(&mut app, now);
        app.handle_key(key(KeyCode::Char('e')), now);
        let text = app.clipboard.take().expect("clipboard text");
        assert!(text.starts_with("The Royal Game of Ur: game record\nLight: Human\n"));
        assert!(text.contains("Dark: Computer · Medium\n"));
        assert!(text.contains(&format!("Result: {winner} won after ")));
        assert!(text.contains("\n1. Light rolls"));
        let files: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
        assert_eq!(files.len(), 1);
        let path = files[0].as_ref().unwrap().path();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        assert!(app.log.last().unwrap().starts_with("Chronicle saved to "));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn entries_know_their_player() {
        assert_eq!(
            entry_player("12. Light rolls 3: moves 5 → 8."),
            Some(Player::Light)
        );
        assert_eq!(
            entry_player("Hint for Dark: enters a piece on 2."),
            Some(Player::Dark)
        );
        assert_eq!(
            entry_player("Dark: 30 throws, average 2.10."),
            Some(Player::Dark)
        );
        assert_eq!(entry_player("New game: Light (Human) vs Dark."), None);
        assert_eq!(entry_player("Lightning"), None);
    }

    fn human_app_with_position(light: [u8; 7], dark: [u8; 7]) -> (App, Time) {
        let (mut app, now) = game_app(Controller::Human, Controller::Human);
        app.animations = true;
        app.game = GameState::from_positions(Player::Light, light, dark).unwrap();
        (app, now)
    }

    /// Put the app into the move-choosing phase for `roll`, as if the dice
    /// had just settled on it.
    fn force_roll(app: &mut App, roll: u8, now: Time) {
        let tips = (1u8 << roll) - 1; // `roll` marked dice
        app.resolve_roll(tips, now);
        assert!(matches!(app.phase, Phase::Choosing { .. }));
    }

    #[test]
    fn dice_tumble_then_settle() {
        let (mut app, now) = game_app(Controller::Human, Controller::Human);
        app.animations = true;
        app.handle_key(key(KeyCode::Char(' ')), now);
        let Phase::Rolling { tips, .. } = app.phase else {
            panic!("expected rolling")
        };
        assert_eq!(app.deadline(), Some(now + DICE_ROLL_TIME));
        assert!(app.is_animating());
        assert!(!app.tumbling_dice().unwrap().1, "not settled at start");
        // Faces change while tumbling.
        let faces: std::collections::HashSet<u8> = (0..40)
            .map(|i| {
                app.update(now + Duration::from_millis(i * 12));
                app.tumbling_dice().unwrap().0
            })
            .collect();
        assert!(faces.len() > 3, "dice should flicker: {faces:?}");
        app.update(now + DICE_ROLL_TIME.mul_f32(0.9));
        assert_eq!(
            app.tumbling_dice(),
            Some((tips, true)),
            "settles on the real throw"
        );
        app.update(now + DICE_ROLL_TIME);
        assert!(!app.is_animating());
        assert_eq!(app.dice.unwrap().tips, tips);
    }

    #[test]
    fn piece_hops_square_by_square_then_commits() {
        let (mut app, now) = human_app_with_position([2, 0, 0, 0, 0, 0, 0], [0; 7]);
        force_roll(&mut app, 3, now);
        app.handle_key(key(KeyCode::Char('2')), now); // 2 → 5 (option 1 is entering)
        let before = app.game;
        let mut seen = Vec::new();
        for i in 0..3 {
            app.update(now + STEP_TIME * i);
            let a = app.piece_animation().unwrap();
            assert_eq!(a.mv, Move { from: 2, to: 5 });
            seen.push(a.pos);
            assert_eq!(app.game, before, "state unchanged while animating");
        }
        assert_eq!(seen, vec![3, 4, 5]);
        assert!(app.piece_animation().unwrap().landed);
        let end = now + move_duration(app.selected_or_moving().unwrap());
        app.update(end);
        assert!(app.piece_animation().is_none());
        assert!(app.game.occupies(Player::Light, 5));
        assert_eq!(app.game.turn(), Player::Dark);
    }

    #[test]
    fn capture_flashes_before_commit() {
        let (mut app, now) = human_app_with_position([5, 0, 0, 0, 0, 0, 0], [7, 0, 0, 0, 0, 0, 0]);
        force_roll(&mut app, 2, now);
        let i = app.options().iter().position(|o| o.mv.from == 5).unwrap();
        app.handle_key(key(KeyCode::Char((b'1' + i as u8) as char)), now);
        app.update(now + STEP_TIME);
        let a = app.piece_animation().unwrap();
        assert!(a.landed && a.impact);
        assert!(
            app.game.occupies(Player::Dark, 7),
            "victim still there during flash"
        );
        app.update(now + STEP_TIME + LAND_TIME + IMPACT_TIME);
        assert_eq!(app.game.waiting(Player::Dark), 7);
    }

    #[test]
    fn space_skips_animations() {
        let (mut app, now) = human_app_with_position([2, 0, 0, 0, 0, 0, 0], [0; 7]);
        force_roll(&mut app, 3, now);
        app.handle_key(key(KeyCode::Char('2')), now);
        assert!(app.is_animating());
        app.handle_key(key(KeyCode::Char(' ')), now);
        assert!(!app.is_animating());
        assert!(app.game.occupies(Player::Light, 5));
    }

    #[test]
    fn computer_vs_computer_finishes_with_animations() {
        let ai = Controller::Computer(Difficulty::Easy);
        let (mut app, mut now) = game_app(ai, ai);
        app.animations = true;
        for _ in 0..50_000 {
            if matches!(app.phase, Phase::GameOver(_)) {
                break;
            }
            now += Duration::from_millis(100);
            app.update(now);
        }
        assert!(matches!(app.phase, Phase::GameOver(_)));
    }

    #[test]
    fn hint_picks_the_hard_ai_move_and_rates_the_rest() {
        // Capturing Dark's advanced piece on 10 is clearly best.
        let (mut app, now) = human_app_with_position([7, 0, 0, 0, 0, 0, 0], [10, 0, 0, 0, 0, 0, 0]);
        force_roll(&mut app, 3, now);
        app.handle_key(key(KeyCode::Char('h')), now);
        assert!(!app.show_help, "h is hint now, not help");
        let hint = app.hint.clone().expect("hint shown");
        let best = app.options()[hint.best].mv;
        assert_eq!(best, Move { from: 7, to: 10 });
        assert_eq!(app.selected_move().unwrap().mv, best, "cursor moved to it");
        let mut rng = StdRng::seed_from_u64(0);
        assert_eq!(
            ai::choose_move(&app.game, 3, HINT_LEVEL, &mut rng),
            Some(best),
            "agrees with the Hard AI"
        );
        assert_eq!(hint.loss[hint.best], 0.0);
        assert!(hint.loss.iter().all(|&l| l >= 0.0));
        assert!(app.log.last().unwrap().starts_with("Hint for Light"));
        // Choosing a move clears the hint.
        app.handle_key(key(KeyCode::Enter), now);
        assert_eq!(app.hint, None);
    }

    #[test]
    fn hint_does_nothing_outside_choosing() {
        let (mut app, now) = game_app(Controller::Human, Controller::Human);
        app.handle_key(key(KeyCode::Char('h')), now);
        assert_eq!(app.hint, None);
        assert_eq!(app.phase, Phase::AwaitRoll);
    }

    #[test]
    fn escape_returns_to_menu_and_stops_clock() {
        let (mut app, now) = game_app(Controller::Computer(Difficulty::Easy), Controller::Human);
        app.handle_key(key(KeyCode::Esc), now);
        assert_eq!(app.screen, Screen::Menu);
        assert_eq!(app.deadline(), None);
        app.update(now + PASS_DELAY);
        assert_eq!(app.phase, Phase::AwaitRoll);
    }

    #[test]
    fn help_overlay_swallows_next_key() {
        let (mut app, now) = game_app(Controller::Human, Controller::Human);
        app.handle_key(key(KeyCode::Char('?')), now);
        assert!(app.show_help);
        app.handle_key(key(KeyCode::Char(' ')), now);
        assert!(!app.show_help);
        assert_eq!(app.phase, Phase::AwaitRoll, "key closed help, did not roll");
    }

    #[test]
    fn quit_keys() {
        let mut app = App::new(0);
        app.handle_key(key(KeyCode::Char('q')), Time::ZERO);
        assert!(app.should_quit);
        let mut app = App::new(0);
        app.handle_key(
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            Time::ZERO,
        );
        assert!(app.should_quit);
    }

    #[test]
    fn menu_offers_continue_only_with_a_game_to_resume() {
        let mut app = App::new(1);
        let now = Time::ZERO;
        assert!(!app.menu_items().contains(&MenuItem::Continue));
        app.handle_key(key(KeyCode::Enter), now); // New game
        assert!(app.resumable.is_some(), "saved at the start of the turn");
        app.handle_key(key(KeyCode::Esc), now);
        assert_eq!(app.screen, Screen::Menu);
        assert_eq!(app.menu_items()[0], MenuItem::Continue);
        assert_eq!(app.menu_selected, MenuItem::Continue);
        // Navigation wraps through the extra row.
        app.handle_key(key(KeyCode::Up), now);
        assert_eq!(app.menu_selected, MenuItem::Quit);
        app.handle_key(key(KeyCode::Down), now);
        assert_eq!(app.menu_selected, MenuItem::Continue);
    }

    #[test]
    fn resuming_after_a_throw_shows_the_same_throw() {
        let (mut app, now) = game_app(Controller::Human, Controller::Human);
        app.animations = true;
        app.handle_key(key(KeyCode::Char(' ')), now); // throw: dice tumbling
        let Phase::Rolling { tips, .. } = app.phase else {
            panic!("expected rolling")
        };
        assert_eq!(app.resumable.as_ref().unwrap().pending_tips, Some(tips));
        let log = app.log.clone();

        // Quit mid-tumble, come back: the same throw is waiting.
        app.handle_key(key(KeyCode::Esc), now);
        app.handle_key(key(KeyCode::Enter), now); // Continue
        assert_eq!(app.screen, Screen::Game);
        assert_eq!(app.dice.unwrap().tips, tips);
        let resumed_phase_ok = matches!(app.phase, Phase::Choosing { .. } | Phase::NoMoves { .. });
        assert!(resumed_phase_ok, "{:?}", app.phase);
        assert!(app.log.starts_with(&log));
    }

    #[test]
    fn resuming_between_turns_waits_for_a_throw() {
        let (mut app, now) = human_app_with_position([3, 0, 0, 0, 0, 0, 0], [6, 0, 0, 0, 0, 0, 0]);
        app.animations = false;
        force_roll(&mut app, 2, now);
        app.handle_key(key(KeyCode::Enter), now); // play 3 → 5
        let game = app.game;
        assert_eq!(game.turn(), Player::Dark);
        app.handle_key(key(KeyCode::Esc), now);
        app.controllers = [Controller::Computer(Difficulty::Easy); 2]; // menu fiddling
        app.handle_key(key(KeyCode::Enter), now);
        assert_eq!(app.game, game);
        assert_eq!(app.phase, Phase::AwaitRoll);
        assert_eq!(
            app.controllers,
            [Controller::Human; 2],
            "saved players kept"
        );
    }

    #[test]
    fn winning_clears_the_save() {
        let (mut app, now) = human_app_with_position([13, 15, 15, 15, 15, 15, 15], [0; 7]);
        app.animations = false;
        app.autosave(None);
        assert!(app.resumable.is_some());
        force_roll(&mut app, 2, now);
        app.handle_key(key(KeyCode::Enter), now);
        assert_eq!(app.phase, Phase::GameOver(Player::Light));
        assert!(app.resumable.is_none());
        app.handle_key(key(KeyCode::Esc), now);
        assert!(!app.menu_items().contains(&MenuItem::Continue));
    }

    #[test]
    fn saves_to_disk_and_reloads() {
        let path = std::env::temp_dir().join(format!("ur-game-app-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut app = App::new(4);
        app.use_save_file(path.clone());
        app.controllers = [Controller::Human, Controller::Computer(Difficulty::Hard)];
        app.animations = false;
        let now = Time::ZERO;
        app.new_game(now);
        app.handle_key(key(KeyCode::Char(' ')), now);

        let mut again = App::new(5);
        again.use_save_file(path.clone());
        assert_eq!(again.resumable, app.resumable);
        assert_eq!(again.menu_selected, MenuItem::Continue);
        std::fs::remove_file(path).unwrap();
    }
}

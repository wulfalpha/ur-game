//! Application state and game flow for the terminal front end.
//!
//! Nothing here touches the terminal: `App` takes key events and the current
//! time, and exposes state for `ui` to draw. Computer turns are paced with
//! deadlines so a human can follow what the AI does.
//!
//! Animations (tumbling dice, pieces hopping square by square) are purely
//! presentational: the throw and the move are decided up front, and the
//! game state only changes once the animation has finished.

use crate::save::{self, SavedGame};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;
use std::time::Duration;
use ur_game::ai::{self, Difficulty};
use ur_game::game::{FINISH, GameState, Move, Outcome, Player, START, roll_from_tips};

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
const LOG_CAPACITY: usize = 100;
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
    Quit,
}

impl MenuItem {
    pub const ALL: [MenuItem; 6] = [
        MenuItem::Continue,
        MenuItem::Light,
        MenuItem::Dark,
        MenuItem::Animation,
        MenuItem::Start,
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
}

/// A legal move together with what it would cause, for the move list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveOption {
    pub mv: Move,
    pub outcome: Outcome,
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
    pub show_help: bool,
    /// Advice shown for the current move choice, if the player asked.
    pub hint: Option<Hint>,
    pub should_quit: bool,
    pub animations: bool,
    /// Time of the latest input or update; animations are drawn relative to it.
    pub now: Time,
    /// The game in progress (or saved from an earlier run) that "Continue"
    /// resumes. Kept up to date at every save point.
    pub resumable: Option<SavedGame>,
    /// Where to save; `None` keeps saves in memory only.
    save_path: Option<PathBuf>,
    save_failed: bool,
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
            show_help: false,
            hint: None,
            should_quit: false,
            animations: true,
            now: Time::ZERO,
            resumable: None,
            save_path: None,
            save_failed: false,
            deadline: None,
            rng: StdRng::seed_from_u64(seed),
        }
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
        }
    }

    fn menu_key(&mut self, code: KeyCode, now: Time) {
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
                    MenuItem::Animation => self.animations = !self.animations,
                    MenuItem::Continue if matches!(code, KeyCode::Enter | KeyCode::Char(' ')) => {
                        self.resume(now);
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
        if self.deadline.is_some_and(|d| now >= d) {
            self.deadline = None;
            self.advance(now);
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
        let tips = (self.rng.next_u32() & 0b1111) as u8;
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
            self.push_log(format!("{player} rolls {roll}: {why}, turn passes."));
            self.phase = Phase::NoMoves { roll };
            self.deadline = Some(now + PASS_DELAY);
            return;
        }

        let options: Vec<MoveOption> = moves
            .iter()
            .map(|&mv| {
                let mut preview = self.game;
                let outcome = preview.apply(roll, mv).expect("legal move");
                MoveOption { mv, outcome }
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
        let loss = scores.iter().map(|s| scores[best] - s).collect();
        if let Phase::Choosing {
            options, selected, ..
        } = &mut self.phase
        {
            *selected = best;
            let text = format!(
                "Hint for {}: {}.",
                self.game.turn(),
                describe_move(options[best].mv)
            );
            self.hint = Some(Hint { best, loss });
            self.push_log(text);
        }
    }

    /// Apply a move to the game and log it.
    fn commit(&mut self, roll: u8, option: MoveOption, now: Time) {
        let mv = option.mv;
        let player = self.game.turn();
        let outcome = self
            .game
            .apply(roll, mv)
            .expect("move came from legal_moves");
        let mut text = format!("{player} rolls {roll}: {}", describe_move(mv));
        if outcome.captured {
            text.push_str(", captures");
        }
        if outcome.extra_turn {
            text.push_str(", rosette: rolls again");
        }
        text.push('.');
        self.push_log(text);

        if let Some(winner) = outcome.winner {
            self.push_log(format!("{winner} wins the game!"));
            self.phase = Phase::GameOver(winner);
            self.deadline = None;
            self.clear_save();
        } else {
            self.start_turn(now);
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
        if self.log.len() > LOG_CAPACITY {
            self.log.remove(0);
        }
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
        assert!(app.log.last().unwrap().contains("wins"));
        assert_eq!(app.deadline(), None);
        app.handle_key(key(KeyCode::Char('n')), now);
        assert_eq!(app.game, GameState::new());
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

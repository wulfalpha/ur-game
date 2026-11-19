//! Application state and game flow for the terminal front end.
//!
//! Nothing here touches the terminal: `App` takes key events and the current
//! time, and exposes state for `ui` to draw. Computer turns are paced with
//! deadlines so a human can follow what the AI does.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::{Duration, Instant};
use ur_game::ai::{self, Difficulty};
use ur_game::game::{FINISH, GameState, Move, Outcome, Player, START, roll_from_tips};

/// Pause before each computer action (roll, then move).
pub const AI_DELAY: Duration = Duration::from_millis(700);
/// How long a "no moves" roll stays on screen before the turn passes.
pub const PASS_DELAY: Duration = Duration::from_millis(1200);
const LOG_CAPACITY: usize = 100;

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
    Light,
    Dark,
    Start,
    Quit,
}

impl MenuItem {
    pub const ALL: [MenuItem; 4] = [
        MenuItem::Light,
        MenuItem::Dark,
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
    /// Dice thrown but nothing can move; the turn passes shortly.
    NoMoves {
        roll: u8,
    },
    GameOver(Player),
}

pub struct App {
    pub screen: Screen,
    pub menu_selected: usize,
    pub controllers: [Controller; 2],
    pub game: GameState,
    pub phase: Phase,
    pub dice: Option<Dice>,
    /// Newest entries last.
    pub log: Vec<String>,
    pub show_help: bool,
    pub should_quit: bool,
    deadline: Option<Instant>,
    rng: StdRng,
}

impl App {
    pub fn new(seed: u64) -> Self {
        App {
            screen: Screen::Menu,
            menu_selected: MenuItem::ALL
                .iter()
                .position(|&m| m == MenuItem::Start)
                .unwrap(),
            controllers: [Controller::Human, Controller::Computer(Difficulty::Medium)],
            game: GameState::new(),
            phase: Phase::AwaitRoll,
            dice: None,
            log: Vec::new(),
            show_help: false,
            should_quit: false,
            deadline: None,
            rng: StdRng::seed_from_u64(seed),
        }
    }

    /// When the app next wants [`update`](Self::update) called, if ever.
    pub fn deadline(&self) -> Option<Instant> {
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

    // ------------------------------------------------------------------
    // Input
    // ------------------------------------------------------------------

    pub fn handle_key(&mut self, key: KeyEvent, now: Instant) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }
        if self.show_help {
            self.show_help = false;
            return;
        }
        match key.code {
            KeyCode::Char('?') | KeyCode::Char('h') | KeyCode::F(1) => {
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

    fn menu_key(&mut self, code: KeyCode, now: Instant) {
        let n = MenuItem::ALL.len();
        let item = MenuItem::ALL[self.menu_selected];
        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.menu_selected = (self.menu_selected + n - 1) % n
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                self.menu_selected = (self.menu_selected + 1) % n
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') | KeyCode::Enter => {
                let step = if code == KeyCode::Left { -1 } else { 1 };
                match item {
                    MenuItem::Light => self.controllers[0] = self.controllers[0].cycle(step),
                    MenuItem::Dark => self.controllers[1] = self.controllers[1].cycle(step),
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

    fn game_key(&mut self, code: KeyCode, now: Instant) {
        if code == KeyCode::Esc {
            self.screen = Screen::Menu;
            self.deadline = None;
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

    pub fn update(&mut self, now: Instant) {
        if self.screen != Screen::Game || self.show_help {
            return;
        }
        if self.deadline.is_some_and(|d| now >= d) {
            self.deadline = None;
            self.advance(now);
        }
    }

    /// Take the next automatic step for the current phase.
    fn advance(&mut self, now: Instant) {
        match self.phase {
            Phase::AwaitRoll if !self.current_is_human() => self.roll(now),
            Phase::Choosing { selected, .. } if !self.current_is_human() => {
                self.play(selected, now)
            }
            Phase::NoMoves { .. } => self.pass(now),
            _ => {}
        }
    }

    // ------------------------------------------------------------------
    // Game flow
    // ------------------------------------------------------------------

    pub fn new_game(&mut self, now: Instant) {
        self.screen = Screen::Game;
        self.game = GameState::new();
        self.dice = None;
        self.log.clear();
        self.push_log(format!(
            "New game: Light ({}) vs Dark ({}).",
            self.controllers[0].label(),
            self.controllers[1].label()
        ));
        self.start_turn(now);
    }

    fn start_turn(&mut self, now: Instant) {
        self.phase = Phase::AwaitRoll;
        self.deadline = (!self.current_is_human()).then(|| now + AI_DELAY);
    }

    fn roll(&mut self, now: Instant) {
        let player = self.game.turn();
        let tips = (self.rng.next_u32() & 0b1111) as u8;
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

    fn play(&mut self, index: usize, now: Instant) {
        let Phase::Choosing {
            roll, ref options, ..
        } = self.phase
        else {
            return;
        };
        let Some(&MoveOption { mv, .. }) = options.get(index) else {
            return;
        };
        let player = self.game.turn();
        let outcome = self
            .game
            .apply(roll, mv)
            .expect("move came from legal_moves");
        let mut text = format!("{player} rolls {roll}: {}", describe_move(mv));
        if outcome.captured {
            text.push_str(", captures!");
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
        } else {
            self.start_turn(now);
        }
    }

    fn pass(&mut self, now: Instant) {
        let Phase::NoMoves { roll } = self.phase else {
            return;
        };
        self.game.pass(roll).expect("no legal moves");
        self.start_turn(now);
    }

    fn push_log(&mut self, line: String) {
        self.log.push(line);
        if self.log.len() > LOG_CAPACITY {
            self.log.remove(0);
        }
    }
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

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::from(code)
    }

    fn game_app(light: Controller, dark: Controller) -> (App, Instant) {
        let mut app = App::new(42);
        app.controllers = [light, dark];
        let now = Instant::now();
        app.new_game(now);
        (app, now)
    }

    #[test]
    fn menu_cycles_controllers_and_starts() {
        let mut app = App::new(1);
        let now = Instant::now();
        assert_eq!(app.screen, Screen::Menu);
        app.handle_key(key(KeyCode::Up), now); // Dark
        app.handle_key(key(KeyCode::Right), now);
        assert_eq!(app.controllers[1], Controller::Computer(Difficulty::Hard));
        app.handle_key(key(KeyCode::Right), now);
        assert_eq!(app.controllers[1], Controller::Human); // wraps
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
        app.controllers = [Controller::Human, Controller::Human];
        app.screen = Screen::Game;
        // Light has two pieces in play, so most rolls give several options.
        app.game = GameState::from_positions(Player::Light, [3, 6, 0, 0, 0, 0, 0], [0; 7]).unwrap();
        let now = Instant::now();
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
        app.handle_key(key(KeyCode::Char('q')), Instant::now());
        assert!(app.should_quit);
        let mut app = App::new(0);
        app.handle_key(
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            Instant::now(),
        );
        assert!(app.should_quit);
    }
}

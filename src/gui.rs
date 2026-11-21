//! Graphical front end, drawn with macroquad.
//!
//! It shares [`App`] with the terminal version, so the menu, turns,
//! computer players, animations and saving behave the same; this module only
//! draws and turns mouse and keyboard input into calls on `App`.
//!
//! Everything is laid out on a virtual 1280×720 landscape canvas and scaled
//! to fit the window, so the game looks the same at any size.

use crate::app::{
    App, Grade, MenuItem, MoveOption, Phase, Screen, Time, entry_player, risk_percent,
};
use crate::stats::{Outcome, Record};
use macroquad::prelude::*;

use ratatui::crossterm::event::{KeyCode as Key, KeyEvent, KeyModifiers};
use std::f32::consts::PI;
use ur_game::game::{
    COLS, Move, PIECES_PER_PLAYER, Player, ROWS, Rules, START, is_board_cell, is_rosette_cell,
};

// ---------------------------------------------------------------------------
// Layout (virtual canvas units)
// ---------------------------------------------------------------------------

const CANVAS_W: f32 = 1280.0;
const CANVAS_H: f32 = 720.0;
/// Baseline of the key hints along the bottom, clear of the border.
const FOOTER_Y: f32 = 688.0;

const CELL: f32 = 96.0;
const CELL_GAP: f32 = 8.0;
const BOARD_X: f32 = 48.0;
const BOARD_Y: f32 = 224.0;
const BOARD_W: f32 = COLS as f32 * CELL + (COLS - 1) as f32 * CELL_GAP;
const BOARD_H: f32 = ROWS as f32 * CELL + (ROWS - 1) as f32 * CELL_GAP;
const PIECE_R: f32 = 34.0;

/// Centre line of each side's row of waiting and finished pieces.
const RESERVE_Y: [f32; 2] = [BOARD_Y - 70.0, BOARD_Y + BOARD_H + 70.0];
const RESERVE_R: f32 = 17.0;
const RESERVE_STEP: f32 = 42.0;
const RESERVE_X: f32 = BOARD_X + 160.0;

const PANEL_X: f32 = 912.0;
const PANEL_W: f32 = 336.0;
const DICE_Y: f32 = 72.0;
const DICE_H: f32 = 168.0;
const MOVES_Y: f32 = 256.0;
const MOVES_H: f32 = 236.0;
const LOG_Y: f32 = 508.0;
const LOG_H: f32 = 172.0;
const MOVE_ROW_H: f32 = 30.0;

// ---------------------------------------------------------------------------
// Colours
// ---------------------------------------------------------------------------

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0)
}

const BACKGROUND: Color = rgb(9, 17, 30);
const PANEL: Color = rgb(17, 31, 49);
const PANEL_EDGE: Color = rgb(137, 108, 61);
const TEXT: Color = rgb(242, 231, 207);
const DIM: Color = rgb(167, 169, 167);
const ACCENT: Color = rgb(231, 190, 105);
const TARGET: Color = rgb(96, 204, 116);
const DANGER: Color = rgb(226, 72, 60);
const RISK: Color = rgb(240, 150, 70);
const WOOD: Color = rgb(179, 139, 71);
const WOOD_DARK: Color = rgb(61, 43, 31);
const TILE_A: Color = rgb(210, 193, 158);
const TILE_B: Color = rgb(231, 215, 183);
const TILE_DOT: Color = rgb(37, 69, 103);
const LAPIS: Color = rgb(27, 62, 117);
const SHELL: Color = rgb(226, 208, 168);
const CARNELIAN: Color = rgb(178, 52, 36);
const NUMBER: Color = rgb(105, 85, 57);

fn piece_colors(p: Player) -> (Color, Color, Color) {
    // (body, rim, dots)
    match p {
        Player::Light => (rgb(245, 238, 220), rgb(150, 132, 104), rgb(70, 52, 38)),
        Player::Dark => (rgb(141, 0, 23), rgb(232, 196, 150), rgb(240, 214, 180)),
    }
}

fn player_text_color(p: Player) -> Color {
    match p {
        Player::Light => rgb(245, 238, 220),
        Player::Dark => rgb(220, 70, 80),
    }
}

fn with_alpha(c: Color, a: f32) -> Color {
    Color::new(c.r, c.g, c.b, c.a * a)
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Open a window and play until the player quits or closes it.
pub fn run(app: App) {
    let conf = Conf {
        window_title: "The Royal Game of Ur".to_owned(),
        window_width: 1280,
        window_height: 720,
        window_resizable: true,
        high_dpi: true,
        sample_count: 4,
        ..Default::default()
    };
    macroquad::Window::from_config(conf, main_loop(app));
}

async fn main_loop(mut app: App) {
    let fonts = Fonts {
        regular: load_ttf_font_from_bytes(include_bytes!("../assets/fonts/NotoSans-Regular.ttf"))
            .expect("bundled font"),
        bold: load_ttf_font_from_bytes(include_bytes!("../assets/fonts/NotoSans-Bold.ttf"))
            .expect("bundled font"),
    };
    let mut audio = crate::sound::Audio::new();
    while !app.should_quit {
        let now = Time::from_secs_f64(get_time());
        let mut ui = Ui::new(&fonts);
        handle_keys(&mut app, now);
        app.update(now);
        app.now = now;

        // While the rules are open, a click only closes them.
        let help_open = app.show_help;
        let click = ui.clicked;
        ui.clicked = click && !help_open;

        clear_background(BACKGROUND);
        ui.backdrop();
        match app.screen {
            Screen::Menu => ui.menu(&mut app, now),
            Screen::Game => ui.game(&mut app, now),
            Screen::Leaderboard => ui.leaderboard(&mut app, now),
        }
        if let Some(text) = app.clipboard.take() {
            macroquad::miniquad::window::clipboard_set(&text);
        }
        if app.show_help {
            ui.help(&mut app, click && help_open);
        }
        ui.sound_control(&mut app, now);
        audio.update(&mut app);
        next_frame().await;
    }
}

/// Keyboard input goes through the same handler as the terminal version.
fn handle_keys(app: &mut App, now: Time) {
    let shift = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
    for key in get_keys_pressed() {
        let code = match key {
            KeyCode::Space => Key::Char(' '),
            KeyCode::Enter | KeyCode::KpEnter => Key::Enter,
            KeyCode::Escape => Key::Esc,
            KeyCode::Left => Key::Left,
            KeyCode::Right => Key::Right,
            KeyCode::Up => Key::Up,
            KeyCode::Down => Key::Down,
            KeyCode::Tab if shift => Key::BackTab,
            KeyCode::Tab => Key::Tab,
            KeyCode::F1 => Key::F(1),
            KeyCode::PageUp => Key::PageUp,
            KeyCode::PageDown => Key::PageDown,
            KeyCode::End => Key::End,
            _ => continue,
        };
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE), now);
    }
    while let Some(c) = get_char_pressed() {
        if c.is_ascii_graphic() {
            app.handle_key(KeyEvent::new(Key::Char(c), KeyModifiers::NONE), now);
        }
    }
}

fn press(app: &mut App, code: Key, now: Time) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE), now);
}

// ---------------------------------------------------------------------------
// Drawing helpers
// ---------------------------------------------------------------------------

struct Fonts {
    regular: Font,
    bold: Font,
}

#[derive(Clone, Copy, PartialEq)]
enum Align {
    Left,
    Center,
    Right,
}

/// One frame's drawing context: maps the virtual canvas onto the window.
struct Ui<'a> {
    fonts: &'a Fonts,
    scale: f32,
    origin: Vec2,
    mouse: Vec2,
    clicked: bool,
}

impl<'a> Ui<'a> {
    fn new(fonts: &'a Fonts) -> Self {
        let scale = (screen_width() / CANVAS_W).min(screen_height() / CANVAS_H);
        let origin = vec2(
            (screen_width() - CANVAS_W * scale) / 2.0,
            (screen_height() - CANVAS_H * scale) / 2.0,
        );
        let (mx, my) = mouse_position();
        Ui {
            fonts,
            scale,
            origin,
            mouse: (vec2(mx, my) - origin) / scale,
            clicked: is_mouse_button_pressed(MouseButton::Left),
        }
    }

    fn pt(&self, p: Vec2) -> Vec2 {
        self.origin + p * self.scale
    }

    fn rect(&self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        let p = self.pt(vec2(x, y));
        draw_rectangle(p.x, p.y, w * self.scale, h * self.scale, color);
    }

    fn rect_lines(&self, x: f32, y: f32, w: f32, h: f32, thickness: f32, color: Color) {
        let p = self.pt(vec2(x, y));
        draw_rectangle_lines(
            p.x,
            p.y,
            w * self.scale,
            h * self.scale,
            thickness * self.scale,
            color,
        );
    }

    fn circle(&self, c: Vec2, r: f32, color: Color) {
        let p = self.pt(c);
        draw_circle(p.x, p.y, r * self.scale, color);
    }

    fn circle_lines(&self, c: Vec2, r: f32, thickness: f32, color: Color) {
        let p = self.pt(c);
        draw_circle_lines(p.x, p.y, r * self.scale, thickness * self.scale, color);
    }

    fn line(&self, a: Vec2, b: Vec2, thickness: f32, color: Color) {
        let (a, b) = (self.pt(a), self.pt(b));
        draw_line(a.x, a.y, b.x, b.y, thickness * self.scale, color);
    }

    fn triangle(&self, a: Vec2, b: Vec2, c: Vec2, color: Color) {
        draw_triangle(self.pt(a), self.pt(b), self.pt(c), color);
    }

    fn font(&self, bold: bool) -> &Font {
        if bold {
            &self.fonts.bold
        } else {
            &self.fonts.regular
        }
    }

    fn text_width(&self, text: &str, size: f32, bold: bool) -> f32 {
        let px = (size * self.scale).round().max(1.0) as u16;
        measure_text(text, Some(self.font(bold)), px, 1.0).width / self.scale
    }

    /// Draw `text` with its baseline at `y`. (Many arguments, but each call
    /// reads plainly, like macroquad's own `draw_text`.)
    #[allow(clippy::too_many_arguments)]
    fn text(&self, text: &str, x: f32, y: f32, size: f32, color: Color, bold: bool, align: Align) {
        let w = self.text_width(text, size, bold);
        let x = match align {
            Align::Left => x,
            Align::Center => x - w / 2.0,
            Align::Right => x - w,
        };
        let p = self.pt(vec2(x, y));
        draw_text_ex(
            text,
            p.x,
            p.y,
            TextParams {
                font: Some(self.font(bold)),
                font_size: (size * self.scale).round().max(1.0) as u16,
                color,
                ..Default::default()
            },
        );
    }

    /// Split `text` into lines no wider than `width`.
    fn wrap(&self, text: &str, width: f32, size: f32) -> Vec<String> {
        let mut lines = Vec::new();
        let mut line = String::new();
        for word in text.split_whitespace() {
            // A word too long for a line on its own (a file path, say) is
            // split wherever it has to be.
            let mut word = word.to_string();
            while self.text_width(&word, size, false) > width && word.chars().count() > 1 {
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                }
                let mut n = word.chars().count() - 1;
                while n > 1 {
                    let head: String = word.chars().take(n).collect();
                    if self.text_width(&head, size, false) <= width {
                        break;
                    }
                    n -= 1;
                }
                lines.push(word.chars().take(n).collect());
                word = word.chars().skip(n).collect();
            }
            let word = word.as_str();
            let candidate = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };
            if !line.is_empty() && self.text_width(&candidate, size, false) > width {
                lines.push(std::mem::replace(&mut line, word.to_string()));
            } else {
                line = candidate;
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
        lines
    }

    fn hovered(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        let m = self.mouse;
        m.x >= x && m.x < x + w && m.y >= y && m.y < y + h
    }

    /// Quiet geometric inlay around the edge of the virtual table.
    fn backdrop(&self) {
        self.rect_lines(12.0, 12.0, 1256.0, 696.0, 1.0, with_alpha(PANEL_EDGE, 0.45));
        for x in (28..1260).step_by(24) {
            for y in [20.0, 700.0] {
                let c = vec2(x as f32, y);
                self.line(c - vec2(3.0, 0.0), c - vec2(0.0, 3.0), 1.0, PANEL_EDGE);
                self.line(c - vec2(0.0, 3.0), c + vec2(3.0, 0.0), 1.0, PANEL_EDGE);
            }
        }
    }

    fn panel(&self, x: f32, y: f32, w: f32, h: f32, title: &str) {
        self.rect(x + 4.0, y + 6.0, w, h, with_alpha(BLACK, 0.3));
        self.rect(x, y, w, h, PANEL);
        self.rect_lines(x, y, w, h, 2.0, PANEL_EDGE);
        self.line(
            vec2(x + 8.0, y + 5.0),
            vec2(x + w - 8.0, y + 5.0),
            1.0,
            with_alpha(ACCENT, 0.3),
        );
        for (cx, cy) in [(x, y), (x + w, y), (x, y + h), (x + w, y + h)] {
            self.circle(vec2(cx, cy), 3.0, ACCENT);
        }
        if !title.is_empty() {
            self.text(title, x + 14.0, y + 26.0, 17.0, ACCENT, true, Align::Left);
        }
    }

    /// A clickable button; returns true when clicked.
    fn button(&self, label: &str, x: f32, y: f32, w: f32, h: f32, selected: bool) -> bool {
        let hover = self.hovered(x, y, w, h);
        let (bg, fg) = if selected {
            (ACCENT, BACKGROUND)
        } else if hover {
            (rgb(38, 61, 84), TEXT)
        } else {
            (rgb(24, 43, 63), TEXT)
        };
        self.rect(x, y, w, h, bg);
        self.rect_lines(x, y, w, h, 1.5, PANEL_EDGE);
        self.text(
            label,
            x + w / 2.0,
            y + h / 2.0 + 7.0,
            19.0,
            fg,
            true,
            Align::Center,
        );
        hover && self.clicked
    }

    // -----------------------------------------------------------------------
    // Board pieces and squares
    // -----------------------------------------------------------------------

    fn draw_piece(&self, c: Vec2, r: f32, player: Player, alpha: f32) {
        let (body, rim, dots) = piece_colors(player);
        self.circle(c + vec2(0.0, r * 0.12), r, with_alpha(BLACK, 0.35 * alpha));
        self.circle(c, r, with_alpha(rim, alpha));
        self.circle(c, r * 0.84, with_alpha(body, alpha));
        self.circle_lines(c, r * 0.72, 1.0, with_alpha(rim, alpha * 0.6));
        self.circle_lines(
            c - vec2(0.0, r * 0.025),
            r * 0.94,
            1.0,
            with_alpha(SHELL, alpha * 0.5),
        );
        // Five inlaid dots, like the pieces found at Ur.
        let d = r * 0.4;
        for off in [
            vec2(0.0, 0.0),
            vec2(-d, -d),
            vec2(d, -d),
            vec2(-d, d),
            vec2(d, d),
        ] {
            self.circle(c + off, r * 0.1, with_alpha(dots, alpha));
        }
    }

    fn draw_rosette(&self, c: Vec2) {
        for k in 0..8 {
            let a = k as f32 * PI / 4.0;
            let petal = c + vec2(a.cos(), a.sin()) * 22.0;
            self.circle(petal, 12.0, ACCENT);
            self.circle(petal, 9.0, SHELL);
            self.circle(petal, 3.0, LAPIS);
        }
        self.circle(c, 14.0, SHELL);
        self.circle(c, 9.0, CARNELIAN);
    }
}

fn cell_rect(row: usize, col: usize) -> (f32, f32) {
    (
        BOARD_X + col as f32 * (CELL + CELL_GAP),
        BOARD_Y + row as f32 * (CELL + CELL_GAP),
    )
}

fn cell_center(row: usize, col: usize) -> Vec2 {
    let (x, y) = cell_rect(row, col);
    vec2(x + CELL / 2.0, y + CELL / 2.0)
}

/// Where a path position is drawn. Start and finish sit in the gap in the
/// player's own row, where pieces come onto and go off the board.
fn path_point(rules: Rules, player: Player, pos: u8) -> Vec2 {
    let row = if player == Player::Light { 0 } else { 2 };
    match pos {
        START => cell_center(row, 4),
        p if p == rules.finish() => cell_center(row, 5),
        _ => {
            let (r, c) = rules.cell_of(player, pos).expect("on the board");
            cell_center(r, c)
        }
    }
}

fn reserve_point(player: Player, i: usize) -> Vec2 {
    vec2(
        RESERVE_X + i as f32 * RESERVE_STEP,
        RESERVE_Y[player.index()],
    )
}

fn home_point(player: Player, i: usize) -> Vec2 {
    vec2(
        BOARD_X + BOARD_W - RESERVE_R - i as f32 * RESERVE_STEP,
        RESERVE_Y[player.index()],
    )
}

/// Points a player can click to make a move: the piece and its landing spot.
fn move_targets(app: &App, mv: Move) -> [Vec2; 2] {
    let player = app.game.turn();
    let from = if mv.is_entry() {
        reserve_point(player, app.game.waiting(player) as usize - 1)
    } else {
        path_point(app.game.rules(), player, mv.from)
    };
    let to = if app.game.is_bear_off(mv) {
        home_point(player, app.game.finished(player) as usize)
    } else {
        path_point(app.game.rules(), player, mv.to)
    };
    [from, to]
}

/// Plain-text version of a log line for the GUI font, which lacks arrows.
/// Colour for a reviewed move's grade.
fn grade_color(grade: Grade) -> Color {
    match grade {
        Grade::Best | Grade::Good => TARGET,
        Grade::Inaccuracy => RISK,
        Grade::Mistake => DANGER,
    }
}

fn plain(text: &str) -> String {
    text.replace(" → ", " to ").replace('→', "to")
}

fn short_move(rules: Rules, mv: Move) -> String {
    match (mv.from, mv.to) {
        (START, to) => format!("Enter on {to}"),
        (from, to) if to == rules.finish() => format!("{from} off the board"),
        (from, to) => format!("{from} to {to}"),
    }
}

// ---------------------------------------------------------------------------
// Screens
// ---------------------------------------------------------------------------

impl Ui<'_> {
    fn menu(&self, app: &mut App, now: Time) {
        let (w, x) = (600.0, (CANVAS_W - 600.0) / 2.0);
        self.panel(x, 60.0, w, 600.0, "");
        self.text(
            "The Royal Game of Ur",
            CANVAS_W / 2.0,
            128.0,
            44.0,
            TEXT,
            true,
            Align::Center,
        );
        self.text(
            "Played in Mesopotamia 4,500 years ago. Race all seven pieces home.",
            CANVAS_W / 2.0,
            164.0,
            18.0,
            DIM,
            false,
            Align::Center,
        );
        self.mini_board(vec2(CANVAS_W / 2.0, 222.0), 26.0);
        self.text(
            "L A P I S   ·   S H E L L   ·   C A R N E L I A N",
            CANVAS_W / 2.0,
            281.0,
            13.0,
            ACCENT,
            false,
            Align::Center,
        );

        let items = app.menu_items();
        // Eight rows fit between the board picture and the hint line.
        let row_h = 42.0;
        let top = 296.0 + (MenuItem::ALL.len() - items.len()) as f32 * row_h / 2.0;
        let mid = (row_h - 8.0) / 2.0;
        for (i, &item) in items.iter().enumerate() {
            let (rx, ry, rw) = (x + 60.0, top + i as f32 * row_h, w - 120.0);
            let selected = app.menu_selected == item;
            let hover = self.hovered(rx, ry, rw, row_h - 8.0);
            if hover && (self.clicked || mouse_delta_position() != Vec2::ZERO) {
                app.menu_selected = item;
            }
            let value = match item {
                MenuItem::Light => Some((Player::Light, app.controller_label(app.controllers[0]))),
                MenuItem::Dark => Some((Player::Dark, app.controller_label(app.controllers[1]))),
                _ => None,
            };
            match item {
                MenuItem::Light | MenuItem::Dark | MenuItem::Rules | MenuItem::Animation => {
                    let bg = if selected {
                        rgb(38, 61, 84)
                    } else {
                        rgb(21, 37, 56)
                    };
                    self.rect(rx, ry, rw, row_h - 8.0, bg);
                    if selected {
                        self.rect_lines(rx, ry, rw, row_h - 8.0, 2.0, ACCENT);
                    }
                    let base = ry + mid + 7.0;
                    let label = match value {
                        Some((p, _)) => {
                            self.draw_piece(vec2(rx + 26.0, ry + mid), 12.0, p, 1.0);
                            p.to_string()
                        }
                        None if item == MenuItem::Rules => "Rules".to_string(),
                        None => "Animation".to_string(),
                    };
                    self.text(&label, rx + 48.0, base, 20.0, TEXT, true, Align::Left);
                    let shown = match value {
                        Some((_, v)) => v.replace(" · ", ": "),
                        None if item == MenuItem::Rules => app.rules.to_string(),
                        None => (if app.animations { "On" } else { "Off" }).to_string(),
                    };
                    let vx = rx + rw - 150.0;
                    let color = if selected { ACCENT } else { TEXT };
                    self.text(&shown, vx, base, 20.0, color, false, Align::Center);
                    // Arrows: left half of the value area steps back.
                    self.arrow(vec2(vx - 110.0, ry + mid), -1.0, color);
                    self.arrow(vec2(vx + 110.0, ry + mid), 1.0, color);
                    if hover && self.clicked {
                        let code = if self.mouse.x < vx {
                            Key::Left
                        } else {
                            Key::Right
                        };
                        press(app, code, now);
                    }
                }
                MenuItem::Continue | MenuItem::Start | MenuItem::Leaderboard | MenuItem::Quit => {
                    let label = match item {
                        MenuItem::Continue => {
                            let saved = app.resumable.as_ref().map(|s| s.controllers);
                            let [l, d] = saved.unwrap_or(app.controllers);
                            let name = |c: crate::app::Controller| match c {
                                crate::app::Controller::Human => "Human".to_string(),
                                crate::app::Controller::Computer(d) => d.to_string(),
                            };
                            format!("Continue ({} vs {})", name(l), name(d))
                        }
                        MenuItem::Start if app.forfeit_level().is_some() => {
                            "New game (forfeits saved)".to_string()
                        }
                        MenuItem::Start => "New game".to_string(),
                        MenuItem::Leaderboard => "Leaderboard".to_string(),
                        _ => "Quit".to_string(),
                    };
                    if self.button(&label, rx, ry, rw, row_h - 8.0, selected) {
                        press(app, Key::Enter, now);
                    }
                }
            }
        }
        self.text(
            "Up/Down select · Left/Right change · Enter start · ? rules",
            CANVAS_W / 2.0,
            644.0,
            16.0,
            DIM,
            false,
            Align::Center,
        );
    }

    fn arrow(&self, c: Vec2, dir: f32, color: Color) {
        self.triangle(
            c + vec2(-7.0 * dir, -9.0),
            c + vec2(-7.0 * dir, 9.0),
            c + vec2(8.0 * dir, 0.0),
            color,
        );
    }

    /// A small empty board, for decoration.
    fn mini_board(&self, center: Vec2, cell: f32) {
        let gap = cell * 0.12;
        let w = COLS as f32 * cell + (COLS - 1) as f32 * gap;
        let h = ROWS as f32 * cell + (ROWS - 1) as f32 * gap;
        let origin = center - vec2(w, h) / 2.0;
        for row in 0..ROWS {
            for col in 0..COLS {
                if !is_board_cell(row, col) {
                    continue;
                }
                let p = origin + vec2(col as f32, row as f32) * (cell + gap);
                let color = if is_rosette_cell(row, col) {
                    LAPIS
                } else if (row + col) % 2 == 0 {
                    TILE_A
                } else {
                    TILE_B
                };
                self.rect(p.x, p.y, cell, cell, color);
                if is_rosette_cell(row, col) {
                    self.circle(p + vec2(cell, cell) / 2.0, cell * 0.22, SHELL);
                }
            }
        }
    }

    fn game(&self, app: &mut App, now: Time) {
        self.status(app);
        self.board(app);
        for player in Player::ALL {
            self.reserve(app, player);
        }
        self.board_pieces(app);
        self.board_input(app, now);
        self.dice(app, now);
        self.moves(app, now);
        self.log(app, now);
        self.footer(app);
        self.top_buttons(app, now);
        if let Phase::GameOver(winner) = app.phase {
            self.game_over(app, winner, now);
        }
    }

    fn status(&self, app: &App) {
        let turn = app.game.turn();
        if let Some(review) = &app.review {
            let step = review.step();
            let (grade, color) = match step.grade {
                Grade::Best => ("best move".to_string(), ACCENT),
                g => (
                    format!("{} ({})", g.label(), step.hint.loss_label(step.played)),
                    grade_color(g),
                ),
            };
            let roll = app.dice.map_or(0, |d| d.value);
            let head = format!(
                "Review {}/{} · {turn} threw {roll}: ",
                review.index + 1,
                review.steps.len()
            );
            let full = self.text_width(&head, 24.0, true) + self.text_width(&grade, 24.0, true);
            let x = 460.0 - full / 2.0;
            self.text(
                &head,
                x,
                44.0,
                24.0,
                player_text_color(turn),
                true,
                Align::Left,
            );
            let x = x + self.text_width(&head, 24.0, true);
            self.text(&grade, x, 44.0, 24.0, color, true, Align::Left);
            return;
        }
        let who = match app.controller(turn) {
            crate::app::Controller::Human => turn.to_string(),
            crate::app::Controller::Computer(d) => format!("{turn} ({d})"),
        };
        let thinking = !app.current_is_human();
        let text = match &app.phase {
            Phase::GameOver(w) => format!("{w} wins!"),
            Phase::Rolling { .. } => format!("{who} throws the dice…"),
            Phase::Moving { .. } => format!("{who} moves"),
            Phase::NoMoves { roll } => format!("{who} threw {roll}: no move, the turn passes"),
            _ if app.waiting_for_expert() => {
                let note = app.expert.note(app.game.rules()).unwrap_or_default();
                format!("{who} is {note}…")
            }
            _ if thinking => format!("{who} is thinking…"),
            Phase::AwaitRoll => format!("{who}: throw the dice"),
            Phase::Choosing { roll, .. } => format!("{who}: you threw {roll}, choose a move"),
        };
        let color = match app.phase {
            Phase::GameOver(w) => player_text_color(w),
            _ => player_text_color(turn),
        };
        self.text(&text, 460.0, 44.0, 26.0, color, true, Align::Center);
    }

    fn board(&self, app: &App) {
        // Wooden frame, following the board's shape.
        for (pad, color) in [(14.0, WOOD_DARK), (10.0, WOOD)] {
            for row in 0..ROWS {
                for col in 0..COLS {
                    if is_board_cell(row, col) {
                        // Expanding every square fills the gaps between them.
                        let (x, y) = cell_rect(row, col);
                        self.rect(x - pad, y - pad, CELL + 2.0 * pad, CELL + 2.0 * pad, color);
                    }
                }
            }
        }
        let selected = app.selected_or_moving();
        for row in 0..ROWS {
            for col in 0..COLS {
                if !is_board_cell(row, col) {
                    continue;
                }
                let (x, y) = cell_rect(row, col);
                let c = cell_center(row, col);
                if is_rosette_cell(row, col) {
                    self.rect(x, y, CELL, CELL, LAPIS);
                    self.draw_rosette(c);
                } else {
                    let base = if (row + col) % 2 == 0 { TILE_A } else { TILE_B };
                    self.rect(x, y, CELL, CELL, base);
                    for off in [
                        vec2(-26.0, -26.0),
                        vec2(26.0, -26.0),
                        vec2(-26.0, 26.0),
                        vec2(26.0, 26.0),
                    ] {
                        self.circle(c + off, 4.0, TILE_DOT);
                    }
                }
                self.rect_lines(
                    x + 3.0,
                    y + 3.0,
                    CELL - 6.0,
                    CELL - 6.0,
                    1.0,
                    if is_rosette_cell(row, col) {
                        ACCENT
                    } else {
                        TILE_DOT
                    },
                );
                if !is_rosette_cell(row, col) {
                    self.rect_lines(
                        x + 10.0,
                        y + 10.0,
                        CELL - 20.0,
                        CELL - 20.0,
                        1.0,
                        with_alpha(TILE_DOT, 0.35),
                    );
                    self.circle_lines(c, 12.0, 1.5, TILE_DOT);
                    self.circle(c, 4.0, CARNELIAN);
                }
                // The mover's numbers, so the far end in Masters, where the
                // routes cross, matches the move list.
                let rules = app.game.rules();
                let mover = app.game.turn();
                let number = rules.path_position(mover, row, col).or(rules.path_position(
                    mover.opponent(),
                    row,
                    col,
                ));
                if let Some(n) = number {
                    self.text(
                        &n.to_string(),
                        x + CELL - 6.0,
                        y + CELL - 6.0,
                        13.0,
                        NUMBER,
                        false,
                        Align::Right,
                    );
                }
            }
        }
        // Highlight the chosen move's piece and landing square.
        if let Some(opt) = selected.filter(|_| !matches!(app.phase, Phase::Moving { .. })) {
            let player = app.game.turn();
            let rules = app.game.rules();
            if !opt.mv.is_entry() {
                let (r, c) = rules.cell_of(player, opt.mv.from).unwrap();
                let (x, y) = cell_rect(r, c);
                self.rect_lines(x + 2.0, y + 2.0, CELL - 4.0, CELL - 4.0, 5.0, ACCENT);
            }
            if !app.game.is_bear_off(opt.mv) {
                let (r, c) = rules.cell_of(player, opt.mv.to).unwrap();
                let (x, y) = cell_rect(r, c);
                let color = if opt.outcome.captured { DANGER } else { TARGET };
                self.rect_lines(x + 2.0, y + 2.0, CELL - 4.0, CELL - 4.0, 5.0, color);
            }
        }
    }

    fn board_pieces(&self, app: &App) {
        let anim = app.piece_animation();
        let mover = app.game.turn();
        let rules = app.game.rules();
        let path_point = |player, pos| path_point(rules, player, pos);
        for player in Player::ALL {
            for pos in app.game.pieces_on_board(player) {
                let moving = anim.is_some_and(|a| player == mover && a.mv.from == pos);
                if moving {
                    continue;
                }
                let victim = anim.is_some_and(|a| {
                    a.impact
                        && player != mover
                        && path_point(player, pos) == path_point(mover, a.mv.to)
                });
                let alpha = if victim { 0.35 } else { 1.0 };
                self.draw_piece(path_point(player, pos), PIECE_R, player, alpha);
            }
        }
        // Ghost of the selected move's destination.
        if let (None, Some(opt)) = (anim, app.selected_move())
            && app.current_is_human()
        {
            let [_, to] = move_targets(app, opt.mv);
            let r = if app.game.is_bear_off(opt.mv) {
                RESERVE_R
            } else {
                PIECE_R
            };
            self.draw_piece(to, r, mover, 0.4);
        }
        if let Some(a) = anim {
            let p = a.mv.from as f32 + a.travelled;
            let (lo, hi) = (p.floor() as u8, (p.ceil() as u8).min(a.mv.to));
            let frac = p - p.floor();
            let start = if a.mv.is_entry() && lo == START {
                // Slide out of the waiting row onto the board.
                reserve_point(mover, app.game.waiting(mover) as usize - 1)
            } else {
                path_point(mover, lo)
            };
            let pos = start.lerp(path_point(mover, hi), frac);
            let hop = (frac * PI).sin() * 14.0;
            let r = if app.game.is_bear_off(a.mv) && a.travelled >= (a.mv.to - a.mv.from) as f32 {
                RESERVE_R
            } else {
                PIECE_R * (1.0 + hop / 120.0)
            };
            self.draw_piece(pos - vec2(0.0, hop), r, mover, 1.0);
            if a.impact {
                self.circle_lines(path_point(mover, a.mv.to), PIECE_R + 10.0, 5.0, DANGER);
            }
        }
    }

    fn reserve(&self, app: &App, player: Player) {
        let y = RESERVE_Y[player.index()];
        let anim = app.piece_animation().filter(|_| app.game.turn() == player);
        let left_reserve = anim.is_some_and(|a| a.mv.is_entry());
        let game = &app.game;
        let arrived_home =
            anim.is_some_and(|a| game.is_bear_off(a.mv) && a.pos == game.rules().finish());
        let waiting = app.game.waiting(player) - u8::from(left_reserve);
        let finished = app.game.finished(player) + u8::from(arrived_home);
        let active = app.game.turn() == player && !matches!(app.phase, Phase::GameOver(_));

        let ctrl = match app.controller(player) {
            crate::app::Controller::Human => "Human".to_string(),
            crate::app::Controller::Computer(d) => d.to_string(),
        };
        let color = player_text_color(player);
        self.text(
            &player.to_string(),
            BOARD_X,
            y - 2.0,
            24.0,
            color,
            true,
            Align::Left,
        );
        self.text(&ctrl, BOARD_X, y + 20.0, 15.0, DIM, false, Align::Left);
        if active {
            let w = self.text_width(&player.to_string(), 24.0, true);
            self.rect(BOARD_X, y + 26.0, w, 3.0, color);
        }

        let entry_selected = anim.is_none()
            && app.game.turn() == player
            && app.selected_move().is_some_and(|o| o.mv.is_entry());
        for i in 0..waiting as usize {
            let c = reserve_point(player, i);
            if entry_selected && i + 1 == waiting as usize {
                self.circle_lines(c, RESERVE_R + 6.0, 3.0, ACCENT);
            }
            self.draw_piece(c, RESERVE_R, player, 1.0);
        }
        let label_x = BOARD_X + BOARD_W - PIECES_PER_PLAYER as f32 * RESERVE_STEP - 4.0;
        self.text("home", label_x, y + 6.0, 16.0, DIM, false, Align::Right);
        for i in 0..PIECES_PER_PLAYER as usize {
            let c = home_point(player, i);
            if i < finished as usize {
                self.draw_piece(c, RESERVE_R, player, 1.0);
            } else {
                self.circle_lines(c, RESERVE_R - 2.0, 1.5, with_alpha(DIM, 0.5));
            }
        }
    }

    /// Hover a piece (or where it would land) to select its move; click to
    /// play it.
    fn board_input(&self, app: &mut App, now: Time) {
        if app.show_help || !app.current_is_human() {
            return;
        }
        let Phase::Choosing { options, .. } = &app.phase else {
            return;
        };
        let hit = options.iter().position(|o: &MoveOption| {
            move_targets(app, o.mv)
                .iter()
                .any(|p| p.distance(self.mouse) < CELL / 2.0)
        });
        if let Some(i) = hit {
            if mouse_delta_position() != Vec2::ZERO || self.clicked {
                app.select_option(i);
            }
            if self.clicked {
                app.play_option(i, now);
            }
        }
    }

    fn dice(&self, app: &mut App, now: Time) {
        let (x, y) = (PANEL_X, DICE_Y);
        self.panel(x, y, PANEL_W, DICE_H, "Dice");
        let shown = match (app.tumbling_dice(), app.dice) {
            (Some((tips, settled)), _) => Some((tips, settled)),
            (None, Some(d)) => Some((d.tips, true)),
            (None, None) => None,
        };
        let can_throw = app.current_is_human() && app.phase == Phase::AwaitRoll;
        let faded = shown.is_none() || can_throw;
        let count = app.game.rules().dice();
        // Centred: Masters throws three dice, Finkel four.
        let left = x + 62.0 + f32::from(4 - count) * 35.0;
        for i in 0..count {
            let c = vec2(left + f32::from(i) * 70.0, y + 82.0);
            let (marked, wobble) = match shown {
                Some((tips, settled)) => (
                    tips & (1 << i) != 0,
                    if settled {
                        0.0
                    } else {
                        ((tips as f32 + i as f32) * 1.7).sin() * 0.5
                    },
                ),
                None => (i % 2 == 0, 0.0),
            };
            self.die(c, wobble, marked, if faded { 0.45 } else { 1.0 });
        }
        let caption = match (&app.phase, app.dice) {
            _ if can_throw => "Click the dice or press Space to throw".to_string(),
            (Phase::Rolling { .. }, _) => "…".to_string(),
            (_, Some(d)) => format!("{} threw {}", d.player, d.value),
            _ => String::new(),
        };
        let color = if can_throw { ACCENT } else { TEXT };
        self.text(
            &caption,
            x + PANEL_W / 2.0,
            y + DICE_H - 18.0,
            18.0,
            color,
            !can_throw,
            Align::Center,
        );
        let clickable =
            can_throw || matches!(app.phase, Phase::NoMoves { .. } | Phase::Rolling { .. });
        if clickable && self.clicked && self.hovered(x, y, PANEL_W, DICE_H) {
            press(app, Key::Char(' '), now);
        }
    }

    /// A tetrahedral die seen from above; a marked tip shows a white dot.
    fn die(&self, c: Vec2, rotation: f32, marked: bool, alpha: f32) {
        let r = 30.0;
        let corner = |k: f32| {
            let a = rotation - PI / 2.0 + k * 2.0 * PI / 3.0;
            c + vec2(a.cos(), a.sin()) * r
        };
        let (a, b, d) = (corner(0.0), corner(1.0), corner(2.0));
        self.triangle(
            a + vec2(0.0, 3.0),
            b + vec2(0.0, 3.0),
            d + vec2(0.0, 3.0),
            with_alpha(BLACK, 0.4 * alpha),
        );
        self.triangle(a, b, d, with_alpha(rgb(214, 190, 150), alpha));
        for p in [a, b, d] {
            self.line(c, p, 1.5, with_alpha(rgb(150, 120, 84), alpha));
        }
        if marked {
            self.circle(c, 6.5, with_alpha(WHITE, alpha));
            self.circle_lines(c, 6.5, 1.0, with_alpha(rgb(120, 90, 60), alpha));
        }
    }

    fn moves(&self, app: &mut App, now: Time) {
        let (x, y) = (PANEL_X, MOVES_Y);
        let played = app.review_played();
        let title = if played.is_some() {
            "Moves · » played"
        } else {
            "Moves"
        };
        self.panel(x, y, PANEL_W, MOVES_H, title);
        let Phase::Choosing {
            options, selected, ..
        } = &app.phase
        else {
            let note = match &app.phase {
                Phase::AwaitRoll if app.current_is_human() => "Throw the dice first.",
                Phase::NoMoves { .. } => "No move this time.",
                _ => "",
            };
            self.text(note, x + 14.0, y + 60.0, 17.0, DIM, false, Align::Left);
            return;
        };
        let human = app.current_is_human();
        let mut clicked = None;
        for (i, opt) in options.iter().enumerate() {
            let ry = y + 40.0 + i as f32 * MOVE_ROW_H;
            let (rx, rw) = (x + 8.0, PANEL_W - 16.0);
            let is_sel = i == *selected;
            if is_sel {
                self.rect(rx, ry, rw, MOVE_ROW_H - 2.0, ACCENT);
            } else if human && self.hovered(rx, ry, rw, MOVE_ROW_H - 2.0) {
                self.rect(rx, ry, rw, MOVE_ROW_H - 2.0, rgb(62, 50, 38));
            }
            let fg = if is_sel { BACKGROUND } else { TEXT };
            let base = ry + 20.0;
            let number = match played {
                Some(p) if p == i => "»".to_string(),
                Some(_) => String::new(),
                None => format!("{}", i + 1),
            };
            self.text(
                &number,
                rx + 10.0,
                base,
                16.0,
                if is_sel { fg } else { DIM },
                true,
                Align::Left,
            );
            self.text(
                &short_move(app.game.rules(), opt.mv),
                rx + 32.0,
                base,
                17.0,
                fg,
                is_sel,
                Align::Left,
            );
            let mut tag_x = rx + rw - 8.0;
            let mut tag = |label: &str, color: Color| {
                let color = if is_sel { BACKGROUND } else { color };
                self.text(label, tag_x, base, 15.0, color, true, Align::Right);
                tag_x -= self.text_width(label, 15.0, true) + 10.0;
            };
            if let Some(hint) = &app.hint {
                if hint.best == i {
                    tag("best", ACCENT);
                } else {
                    tag(&hint.loss_label(i), DIM);
                }
            }
            if opt.outcome.winner.is_some() {
                tag("wins", ACCENT);
            }
            if opt.outcome.captured {
                tag("capture", DANGER);
            }
            if opt.outcome.extra_turn {
                tag("rosette", rgb(110, 150, 240));
            }
            if opt.risk > 0 {
                tag(&format!("risk {}%", risk_percent(opt.risk)), RISK);
            }
            if human && self.clicked && self.hovered(rx, ry, rw, MOVE_ROW_H - 2.0) {
                clicked = Some(i);
            }
        }
        if human {
            let hint_y = y + MOVES_H - 38.0;
            // In a review the reasons are always on, so no Hint button.
            let why_x = if played.is_some() {
                x + 14.0
            } else {
                if self.button("Hint (H)", x + 14.0, hint_y, 120.0, 28.0, false) {
                    press(app, Key::Char('h'), now);
                }
                x + 146.0
            };
            if let Some((is_best, why)) = app.hint_reason() {
                // Gold for the recommended move, which its row also tags "best".
                let color = if is_best { ACCENT } else { TEXT };
                self.text(why, why_x, hint_y + 19.0, 15.0, color, is_best, Align::Left);
            }
        }
        if let Some(i) = clicked {
            app.play_option(i, now);
        }
    }

    fn log(&self, app: &mut App, now: Time) {
        let (x, y) = (PANEL_X, LOG_Y);
        let title = if app.log_scroll > 0 {
            format!("Chronicle · {} back", app.log_scroll)
        } else {
            "Chronicle".to_string()
        };
        self.panel(x, y, PANEL_W, LOG_H, &title);
        if self.button("Share (E)", x + PANEL_W - 112.0, y + 9.0, 98.0, 26.0, false) {
            press(app, Key::Char('e'), now);
        }
        // The mouse wheel scrolls back through the game.
        if self.hovered(x, y, PANEL_W, LOG_H) {
            let (_, wheel) = mouse_wheel();
            if wheel != 0.0 {
                app.scroll_log(if wheel > 0.0 { 1 } else { -1 });
            }
        }
        let size = 15.0;
        let line_h = 20.0;
        let max_lines = ((LOG_H - 44.0) / line_h) as usize;
        let end = app.log.len() - app.log_scroll.min(app.log.len());
        // (text, player the entry is about, first line of its entry)
        let mut lines: Vec<(String, Option<Player>, bool)> = Vec::new();
        for entry in app.log[..end].iter().rev() {
            let who = entry_player(entry);
            let mut wrapped: Vec<_> = self
                .wrap(&plain(entry), PANEL_W - 40.0, size)
                .into_iter()
                .enumerate()
                .map(|(j, line)| (line, who, j == 0))
                .collect();
            wrapped.append(&mut lines);
            lines = wrapped;
            if lines.len() >= max_lines {
                break;
            }
        }
        let skip = lines.len().saturating_sub(max_lines);
        for (i, (line, who, first)) in lines.iter().skip(skip).enumerate() {
            let ty = y + 50.0 + i as f32 * line_h;
            if *first {
                let dot = who.map_or(ACCENT, |p| piece_colors(p).0);
                self.circle(vec2(x + 17.0, ty - 5.0), 3.5, dot);
            }
            self.text(line, x + 26.0, ty, size, TEXT, false, Align::Left);
        }
    }

    fn leaderboard(&self, app: &mut App, now: Time) {
        let (w, h) = (1040.0, 540.0);
        let (x, y) = ((CANVAS_W - w) / 2.0, 80.0);
        self.panel(x, y, w, h, "");
        self.text(
            "Leaderboard",
            CANVAS_W / 2.0,
            y + 68.0,
            40.0,
            TEXT,
            true,
            Align::Center,
        );
        self.text(
            &format!("{} rules · you against the computer", app.leaderboard_rules),
            CANVAS_W / 2.0,
            y + 102.0,
            18.0,
            DIM,
            false,
            Align::Center,
        );
        // Column anchors: name on the left, numbers right-aligned, then the
        // latest results as dots.
        let cols = [
            x + 290.0,
            x + 370.0,
            x + 450.0,
            x + 540.0,
            x + 640.0,
            x + 720.0,
            x + 810.0,
        ];
        let dots_x = x + 850.0;
        let header_y = y + 160.0;
        self.text(
            "Opponent",
            x + 60.0,
            header_y,
            17.0,
            ACCENT,
            true,
            Align::Left,
        );
        let labels = ["Played", "Won", "Lost", "Win %", "Streak", "Best", "Avg"];
        for (label, cx) in labels.iter().zip(cols) {
            self.text(label, cx, header_y, 17.0, ACCENT, true, Align::Right);
        }
        self.text("Last 10", dots_x, header_y, 17.0, ACCENT, true, Align::Left);
        let dot = |cx: f32, cy: f32, outcome: Outcome| match outcome {
            Outcome::Won => self.circle(vec2(cx, cy), 6.0, TARGET),
            Outcome::Lost => self.circle(vec2(cx, cy), 6.0, DANGER),
            Outcome::Abandoned => self.circle_lines(vec2(cx, cy), 5.0, 2.0, DANGER),
        };
        let row = |name: &str, r: Record, ry: f32, bold: bool| {
            self.text(name, x + 60.0, ry, 21.0, TEXT, bold, Align::Left);
            let dash = || "–".to_string();
            let cells = match r.win_percent() {
                Some(p) => [
                    r.played.to_string(),
                    r.won.to_string(),
                    r.lost().to_string(),
                    format!("{p}%"),
                    r.best_streak.to_string(),
                ],
                None => ["0".into(), dash(), dash(), dash(), dash()],
            };
            let avg = r.average_throws().map_or_else(dash, |a| format!("{a:.0}"));
            let (streak, streak_color) = match r.streak {
                0 => (dash(), TEXT),
                s if s > 0 => (format!("W{s}"), TARGET),
                s => (format!("L{}", -s), DANGER),
            };
            for (cell, cx) in cells[..4].iter().zip(cols) {
                self.text(cell, cx, ry, 21.0, TEXT, bold, Align::Right);
            }
            self.text(&streak, cols[4], ry, 21.0, streak_color, bold, Align::Right);
            self.text(&cells[4], cols[5], ry, 21.0, TEXT, bold, Align::Right);
            self.text(&avg, cols[6], ry, 21.0, TEXT, bold, Align::Right);
            for (i, outcome) in r.recent().enumerate() {
                dot(dots_x + 6.0 + i as f32 * 17.0, ry - 7.0, outcome);
            }
        };
        const ROW_H: f32 = 38.0;
        let levels = ur_game::ai::Difficulty::ALL;
        for (i, level) in levels.into_iter().enumerate() {
            let ry = header_y + 50.0 + i as f32 * ROW_H;
            let r = app.leaderboard.record(app.leaderboard_rules, level);
            row(&level.to_string(), r, ry, false);
        }
        let total_y = header_y + 50.0 + levels.len() as f32 * ROW_H + 14.0;
        self.line(
            vec2(x + 50.0, total_y - 32.0),
            vec2(x + w - 50.0, total_y - 32.0),
            1.0,
            with_alpha(ACCENT, 0.4),
        );
        row(
            "Total",
            app.leaderboard.total(app.leaderboard_rules),
            total_y,
            true,
        );
        // Legend: the dots, then what Avg means.
        let legend_y = y + h - 82.0;
        let parts: [(Option<Outcome>, &str); 4] = [
            (Some(Outcome::Won), "won"),
            (Some(Outcome::Lost), "lost"),
            (Some(Outcome::Abandoned), "abandoned (counts as a loss)"),
            (None, "·  Avg: throws in a finished game"),
        ];
        let width: f32 = parts
            .iter()
            .map(|(o, t)| {
                self.text_width(t, 16.0, false) + if o.is_some() { 20.0 } else { 0.0 } + 18.0
            })
            .sum::<f32>()
            - 18.0;
        let mut lx = CANVAS_W / 2.0 - width / 2.0;
        for (outcome, label) in parts {
            if let Some(o) = outcome {
                dot(lx + 6.0, legend_y - 5.0, o);
                lx += 20.0;
            }
            self.text(label, lx, legend_y, 16.0, DIM, false, Align::Left);
            lx += self.text_width(label, 16.0, false) + 18.0;
        }
        let other = match app.leaderboard_rules {
            Rules::Finkel => Rules::Masters,
            Rules::Masters => Rules::Finkel,
        };
        if self.button(
            &format!("{other} rules"),
            CANVAS_W / 2.0 - 190.0,
            y + h - 64.0,
            180.0,
            38.0,
            false,
        ) {
            press(app, Key::Right, now);
        }
        if self.button(
            "Back",
            CANVAS_W / 2.0 + 10.0,
            y + h - 64.0,
            180.0,
            38.0,
            false,
        ) {
            press(app, Key::Esc, now);
        }
    }

    fn footer(&self, app: &App) {
        if app.review.is_some() {
            let text =
                "Left/Right: step · Tab: next slip · Up/Down: options · E: share · Esc: done";
            self.text(text, 460.0, FOOTER_Y, 16.0, DIM, false, Align::Center);
            return;
        }
        let text = match &app.phase {
            Phase::GameOver(_) => "Enter: rematch · V: review your moves · E: share · Esc: menu",
            Phase::Rolling { .. } | Phase::Moving { .. } => {
                "Space: skip animation · Esc: menu · ?: rules"
            }
            _ if !app.current_is_human() => "Space: hurry the computer · Esc: menu · ?: rules",
            Phase::AwaitRoll => "Space: throw · E: share · Esc: menu · ?: rules",
            Phase::Choosing { .. } => {
                "Click a piece to move it · Left/Right + Enter · 1-9 · H: hint · ?: rules · Esc: menu"
            }
            Phase::NoMoves { .. } => "Space: continue · Esc: menu · ?: rules",
        };
        self.text(text, 460.0, FOOTER_Y, 16.0, DIM, false, Align::Center);
    }

    fn sound_control(&self, app: &mut App, now: Time) {
        let label = if app.sound_enabled {
            format!("Sound {}%", (app.sound_volume * 100.0).round() as u8)
        } else {
            "Sound off".to_owned()
        };
        let (x, y) = if app.screen != Screen::Game {
            (24.0, 656.0)
        } else {
            (PANEL_X, 16.0)
        };
        if self.button(&label, x, y, 140.0, 34.0, false) {
            press(app, Key::Char('m'), now);
        }
        if app.screen != Screen::Game {
            self.text(
                "M: mute · +/-: volume",
                24.0,
                644.0,
                14.0,
                DIM,
                false,
                Align::Left,
            );
        }
    }

    fn top_buttons(&self, app: &mut App, now: Time) {
        if self.button("Menu", BOARD_X - 8.0, 16.0, 90.0, 34.0, false) {
            press(app, Key::Esc, now);
        }
        if self.button("Rules", PANEL_X + PANEL_W - 90.0, 16.0, 90.0, 34.0, false) {
            press(app, Key::Char('?'), now);
        }
    }

    fn game_over(&self, app: &mut App, winner: Player, now: Time) {
        let (w, h) = (520.0, 150.0);
        let x = BOARD_X + (BOARD_W - w) / 2.0;
        let y = BOARD_Y + (BOARD_H - h) / 2.0;
        self.rect(x, y, w, h, with_alpha(PANEL, 0.95));
        self.rect_lines(x, y, w, h, 3.0, ACCENT);
        self.text(
            &format!("{winner} wins!"),
            x + w / 2.0,
            y + 62.0,
            40.0,
            player_text_color(winner),
            true,
            Align::Center,
        );
        if self.button("Rematch", x + 30.0, y + 90.0, 140.0, 40.0, true) {
            press(app, Key::Enter, now);
        }
        if !app.decisions.is_empty()
            && self.button("Review (V)", x + 190.0, y + 90.0, 140.0, 40.0, false)
        {
            press(app, Key::Char('v'), now);
        }
        if self.button("Menu", x + w - 170.0, y + 90.0, 140.0, 40.0, false) {
            press(app, Key::Esc, now);
        }
    }

    fn help(&self, app: &mut App, close: bool) {
        let screen = self.origin * -1.0 / self.scale;
        self.rect(
            screen.x,
            screen.y,
            screen_width() / self.scale,
            screen_height() / self.scale,
            with_alpha(BLACK, 0.6),
        );
        let rules = app.active_rules();
        let t = crate::app::rules_text(rules);
        let moving = format!(
            "Move one piece exactly the number thrown. You can't land on your own piece, but you may jump over pieces. If you can move, you must. {}",
            t.pass
        );
        let mut rosettes = String::from("Landing on a rosette: ");
        let mut chars = t.rosettes.chars();
        rosettes.extend(chars.next().map(|c| c.to_ascii_lowercase()));
        rosettes.push_str(chars.as_str());
        let sections = [
            (
                "Goal",
                "Be the first to move all seven of your pieces along your path and off the board.",
            ),
            ("Path", t.path),
            ("Dice", t.dice),
            ("Moving", moving.as_str()),
            ("Capture", t.capture),
            ("Rosettes", rosettes.as_str()),
            (
                "Risk",
                "In the move list, \"risk 31%\" is the chance the moved piece can be captured on the opponent's next throw.",
            ),
        ];
        let w = 820.0;
        let wrapped: Vec<_> = sections
            .iter()
            .map(|&(title, body)| (title, self.wrap(body, w - 210.0, 17.0)))
            .collect();
        // Tall enough for the text: the title, each section's lines, and
        // the footer.
        let lines: usize = wrapped.iter().map(|(_, l)| l.len()).sum();
        let h = (92.0 + lines as f32 * 24.0 + sections.len() as f32 * 14.0 + 50.0)
            .clamp(600.0, CANVAS_H - 20.0);
        let (x, y) = ((CANVAS_W - w) / 2.0, (CANVAS_H - h) / 2.0);
        self.panel(x, y, w, h, "");
        self.text(
            &format!("How to play: {rules} rules"),
            x + w / 2.0,
            y + 50.0,
            30.0,
            TEXT,
            true,
            Align::Center,
        );
        let mut ty = y + 92.0;
        for (title, lines) in wrapped {
            self.text(title, x + 40.0, ty, 19.0, ACCENT, true, Align::Left);
            for line in lines {
                self.text(&line, x + 170.0, ty, 17.0, TEXT, false, Align::Left);
                ty += 24.0;
            }
            ty += 14.0;
        }
        self.text(
            "Press any key or click to close",
            x + w / 2.0,
            y + h - 24.0,
            16.0,
            DIM,
            false,
            Align::Center,
        );
        if close {
            app.show_help = false;
        }
    }
}

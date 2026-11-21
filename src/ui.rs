//! Rendering. Reads `App`, never mutates it.

use crate::app::{
    App, Controller, Dice, MenuItem, Phase, Screen, describe_move, entry_player, risk_percent,
};
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Clear, List, ListItem, Padding, Paragraph, Widget, Wrap,
};
use ur_game::ai::Difficulty;
use ur_game::game::{
    COLS, FINISH, Move, PIECES_PER_PLAYER, Player, ROWS, START, cell_of, is_board_cell,
    is_rosette_cell, path_position,
};

const CELL_W: u16 = 6;
const CELL_H: u16 = 3;
const BOARD_W: u16 = CELL_W * COLS as u16;
const BOARD_H: u16 = CELL_H * ROWS as u16;
const SIDE_W: u16 = 32;
pub const MIN_W: u16 = BOARD_W + SIDE_W + 2;
pub const MIN_H: u16 = 22;

// Landscape (compact) layout for short screens such as a phone on its side:
// borderless coloured tiles instead of boxed squares.
const TILE_W: u16 = 4;
const TILE_GAP: u16 = 1;
const TILE_H: u16 = 2;
const TILE_BOARD_W: u16 = (TILE_W + TILE_GAP) * COLS as u16 - TILE_GAP;
const TILE_BOARD_H: u16 = TILE_H * ROWS as u16;
const COMPACT_SIDE_W: u16 = 26;
pub const COMPACT_MIN_W: u16 = TILE_BOARD_W + COMPACT_SIDE_W + 1;
pub const COMPACT_MIN_H: u16 = 12;

/// Which layout fits the terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Boxed squares, roomy side panel.
    Full,
    /// Landscape: tile board, narrow side panel, short texts.
    Compact,
}

pub fn mode_for(area: Rect) -> Option<Mode> {
    if area.width >= MIN_W && area.height >= MIN_H {
        Some(Mode::Full)
    } else if area.width >= COMPACT_MIN_W && area.height >= COMPACT_MIN_H {
        Some(Mode::Compact)
    } else {
        None
    }
}

const PIECE: &str = "●";
/// Outline of a piece: where a moving piece will land, or has just left.
const GHOST: &str = "○";
const ROSETTE: &str = "✦";

const ACCENT: Color = Color::Rgb(231, 190, 105);
const BACKGROUND: Color = Color::Rgb(9, 17, 30);
const PANEL: Color = Color::Rgb(17, 31, 49);
const TEXT: Color = Color::Rgb(242, 231, 207);
const TARGET: Color = Color::Green;
const DIM: Color = Color::Rgb(154, 163, 174);
const BOARD_EDGE: Color = Color::Rgb(156, 126, 75); // lapis-and-shell boards had wooden frames
const ROSETTE_COLOR: Color = Color::Rgb(132, 177, 235); // lapis lazuli
const RISK_COLOR: Color = Color::Rgb(240, 150, 70); // amber

// Tile backgrounds for the compact board.
const TILE_A: Color = Color::Rgb(31, 49, 67);
const TILE_B: Color = Color::Rgb(39, 60, 78);
const TILE_ROSETTE: Color = Color::Rgb(28, 48, 115);
const TILE_FROM: Color = Color::Rgb(125, 100, 15);
const TILE_TO: Color = Color::Rgb(25, 100, 50);
const TILE_IMPACT: Color = Color::Rgb(150, 30, 30);
const TILE_NUMBER: Color = Color::Rgb(165, 145, 115);

fn player_color(p: Player) -> Color {
    match p {
        // Shell-white and dark red, given as RGB so terminal themes can't
        // wash them out to similar pastels.
        Player::Light => Color::Rgb(245, 238, 220),
        Player::Dark => Color::Rgb(237, 119, 100),
    }
}

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(
        Block::new().style(Style::new().fg(TEXT).bg(BACKGROUND)),
        area,
    );
    let Some(mode) = mode_for(area) else {
        let msg = format!(
            "Terminal too small: {}×{}\n\nNeed {COMPACT_MIN_W}×{COMPACT_MIN_H} (landscape)\nor {MIN_W}×{MIN_H} (full).\n\nq to quit",
            area.width, area.height
        );
        frame.render_widget(
            Paragraph::new(msg)
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    };
    match app.screen {
        Screen::Menu => draw_menu(frame, app, area),
        Screen::Game => draw_game(frame, app, area, mode),
        Screen::Leaderboard => draw_leaderboard(frame, app, area),
    }
    if app.show_help {
        draw_help(frame, area, mode);
    }
}

// ---------------------------------------------------------------------------
// Menu
// ---------------------------------------------------------------------------

fn draw_menu(frame: &mut Frame, app: &App, area: Rect) {
    let box_area = area.centered(
        Constraint::Length(56),
        Constraint::Length(21.min(area.height)),
    );
    let block = Block::bordered()
        .border_type(BorderType::Double)
        .border_style(Style::new().fg(BOARD_EDGE))
        .style(Style::new().bg(PANEL).fg(TEXT))
        .title(
            Line::from(" ✦ The Royal Game of Ur ✦ ")
                .fg(ACCENT)
                .bold()
                .centered(),
        )
        .padding(Padding::uniform(1));
    let inner = block.inner(box_area);
    frame.render_widget(block, box_area);

    // On short screens, drop the intro and board preview.
    let roomy = inner.height >= 17;
    let gap = |n| Constraint::Length(if roomy { n } else { 0 });
    let [intro, preview, _, items, _, hint] = Layout::vertical([
        gap(2),
        gap(3),
        gap(1),
        Constraint::Length(MenuItem::ALL.len() as u16),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    frame.render_widget(
        Paragraph::new("M E S O P O T A M I A  ·  4,500 years ago\nRace all seven pieces home.")
            .alignment(Alignment::Center)
            .fg(DIM),
        intro,
    );
    frame.render_widget(
        Paragraph::new(vec![
            mini_row("✦···", "✦·"),
            Line::from(vec![
                Span::raw("···"),
                Span::styled("✦", Style::new().fg(ROSETTE_COLOR)),
                Span::raw("····"),
            ]),
            mini_row("✦···", "✦·"),
        ])
        .alignment(Alignment::Center),
        preview,
    );

    let lines: Vec<Line> = app
        .menu_items()
        .iter()
        .map(|item| {
            let selected = *item == app.menu_selected;
            let cursor = if selected { "▶ " } else { "  " };
            let style = if selected {
                Style::new().fg(ACCENT).bg(BACKGROUND).bold()
            } else {
                Style::new()
            };
            match item {
                MenuItem::Light | MenuItem::Dark => {
                    let p = if *item == MenuItem::Light {
                        Player::Light
                    } else {
                        Player::Dark
                    };
                    let value = app.controller(p).label();
                    Line::from(vec![
                        Span::styled(cursor, style),
                        Span::styled(PIECE, Style::new().fg(player_color(p))),
                        Span::styled(format!(" {:<8}", p.to_string()), style),
                        Span::styled(format!("◀ {value:^18} ▶"), style),
                    ])
                }
                MenuItem::Animation => Line::from(vec![
                    Span::styled(cursor, style),
                    Span::styled(format!("  {:<8}", "Animate"), style),
                    Span::styled(
                        format!("◀ {:^18} ▶", if app.animations { "On" } else { "Off" }),
                        style,
                    ),
                ]),
                MenuItem::Continue => {
                    let name = |c: Controller| match c {
                        Controller::Human => "Human".to_string(),
                        Controller::Computer(d) => d.to_string(),
                    };
                    let [light, dark] = app
                        .resumable
                        .as_ref()
                        .map_or(app.controllers, |s| s.controllers);
                    Line::from(Span::styled(
                        format!("{cursor}Continue ({} vs {})", name(light), name(dark)),
                        style,
                    ))
                }
                MenuItem::Start => {
                    let note = if app.forfeit_level().is_some() {
                        " (forfeits saved)"
                    } else {
                        ""
                    };
                    Line::from(Span::styled(format!("{cursor}New game{note}"), style))
                }
                MenuItem::Leaderboard => {
                    Line::from(Span::styled(format!("{cursor}Leaderboard"), style))
                }
                MenuItem::Quit => Line::from(Span::styled(format!("{cursor}Quit"), style)),
            }
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines),
        items.centered_horizontally(Constraint::Length(38)),
    );

    frame.render_widget(
        Paragraph::new("↑↓ select · ←→ change · Enter start · M sound · ? rules")
            .alignment(Alignment::Center)
            .fg(DIM),
        hint,
    );
}

/// "██████░░░░" for a percentage.
fn bar(percent: u32, width: usize) -> String {
    let filled = (percent as usize * width + 50) / 100;
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

fn draw_leaderboard(frame: &mut Frame, app: &App, area: Rect) {
    let box_area = area.centered(
        Constraint::Length(56.min(area.width)),
        Constraint::Length(13.min(area.height)),
    );
    let block = Block::bordered()
        .border_type(BorderType::Double)
        .border_style(Style::new().fg(BOARD_EDGE))
        .style(Style::new().bg(PANEL).fg(TEXT))
        .title(Line::from(" ✦ Leaderboard ✦ ").fg(ACCENT).bold().centered())
        .title_bottom(Line::from(" Enter / Esc back ").fg(DIM).centered())
        .padding(Padding::horizontal(1));
    let inner = block.inner(box_area);
    frame.render_widget(block, box_area);

    let row = |name: &str, r: crate::stats::Record, style: Style| {
        let (won, lost, pct, meter) = match r.win_percent() {
            Some(p) => (
                r.won.to_string(),
                r.lost().to_string(),
                format!("{p}%"),
                bar(p, 10),
            ),
            None => ("–".into(), "–".into(), "–".into(), String::new()),
        };
        Line::from(vec![
            Span::styled(
                format!("{name:<9}{:>7}{won:>6}{lost:>6}{pct:>7}  ", r.played),
                style,
            ),
            Span::styled(meter, Style::new().fg(ACCENT)),
        ])
    };
    let mut lines = vec![
        Line::from("You against the computer").fg(DIM).centered(),
        Line::from(""),
        Line::from(format!(
            "{:<9}{:>7}{:>6}{:>6}{:>7}",
            "Opponent", "Played", "Won", "Lost", "Win %"
        ))
        .fg(ACCENT)
        .bold(),
    ];
    for level in Difficulty::ALL {
        lines.push(row(
            &level.to_string(),
            app.leaderboard.record(level),
            Style::new(),
        ));
    }
    lines.push(row("Total", app.leaderboard.total(), Style::new().bold()));
    lines.push(Line::from(""));
    lines.push(
        Line::from("Finished games against a computer player count.")
            .fg(DIM)
            .centered(),
    );
    frame.render_widget(Paragraph::new(lines), inner);
}

fn mini_row(left: &str, right: &str) -> Line<'static> {
    let style_for = |c: char| {
        if c == '✦' {
            Style::new().fg(ROSETTE_COLOR)
        } else {
            Style::new()
        }
    };
    let mut spans: Vec<Span> = left
        .chars()
        .map(|c| Span::styled(c.to_string(), style_for(c)))
        .collect();
    spans.push(Span::raw("  "));
    spans.extend(
        right
            .chars()
            .map(|c| Span::styled(c.to_string(), style_for(c))),
    );
    Line::from(spans)
}

// ---------------------------------------------------------------------------
// Game screen
// ---------------------------------------------------------------------------

fn draw_game(frame: &mut Frame, app: &App, area: Rect, mode: Mode) {
    let [title, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(area);
    let (side_w, board_w, board_h, gap) = match mode {
        Mode::Full => (SIDE_W, BOARD_W, BOARD_H, 1),
        Mode::Compact => (COMPACT_SIDE_W, TILE_BOARD_W, TILE_BOARD_H, 0),
    };
    let [left, right] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(side_w)])
        .spacing(1)
        .areas(body);

    frame.render_widget(status_line(app), title);
    frame.render_widget(Paragraph::new(footer_hint(app)).fg(DIM).centered(), footer);

    // Board column, vertically centred. Reserve lines may be wider than the
    // board, so they get the whole column width.
    let stack = left.centered_vertically(Constraint::Length(board_h + 2 + 2 * gap));
    let [light_res, _, board, _, dark_res] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(gap),
        Constraint::Length(board_h),
        Constraint::Length(gap),
        Constraint::Length(1),
    ])
    .areas(stack);
    let board = board.centered_horizontally(Constraint::Length(board_w.min(left.width)));
    let res_x = |r: Rect| Rect {
        x: board.x,
        width: left.right() - board.x,
        ..r
    };
    frame.render_widget(reserve_line(app, Player::Light), res_x(light_res));
    frame.render_widget(reserve_line(app, Player::Dark), res_x(dark_res));
    match mode {
        Mode::Full => frame.render_widget(BoardView { app }, board),
        Mode::Compact => frame.render_widget(TileBoard { app }, board),
    }

    // Side panel.
    let (dice_h, moves_h) = match mode {
        Mode::Full => (5, 9),
        Mode::Compact => (3, (app.options_len().max(1) as u16 + 2).min(8)),
    };
    let [dice_area, moves_area, log_area] = Layout::vertical([
        Constraint::Length(dice_h),
        Constraint::Length(moves_h),
        Constraint::Fill(1),
    ])
    .areas(right);
    match mode {
        Mode::Full => draw_dice(frame, app, dice_area),
        Mode::Compact => draw_dice_compact(frame, app, dice_area),
    }
    draw_moves(frame, app, moves_area, mode);
    if log_area.height >= 3 {
        draw_log(frame, app, log_area);
    }
}

fn status_line(app: &App) -> Line<'static> {
    let turn = app.game.turn();
    let who = Span::styled(
        format!("{PIECE} {turn}"),
        Style::new().fg(player_color(turn)).bold(),
    );
    let tail = match &app.phase {
        Phase::GameOver(w) => {
            return Line::from(vec![
                Span::styled(
                    format!("{PIECE} {w} wins!"),
                    Style::new().fg(player_color(*w)).bold(),
                ),
                Span::raw("  Enter for a rematch, Esc for the menu"),
            ])
            .centered();
        }
        Phase::Rolling { .. } => " is throwing the dice…".to_string(),
        Phase::Moving { .. } => " is moving…".to_string(),
        _ if !app.current_is_human() => " (computer) is thinking…".to_string(),
        Phase::AwaitRoll => " to roll the dice".to_string(),
        Phase::Choosing { .. } => " to choose a move".to_string(),
        Phase::NoMoves { .. } => " cannot move".to_string(),
    };
    Line::from(vec![who, Span::raw(tail)]).centered()
}

fn footer_hint(app: &App) -> &'static str {
    match &app.phase {
        Phase::GameOver(_) => "Enter/n rematch · e share · [ ] scroll · Esc menu · q quit",
        Phase::Rolling { .. } | Phase::Moving { .. } => "Space skip · Esc menu · ? rules · q quit",
        _ if !app.current_is_human() => "Space hurry · Esc menu · ? rules · q quit",
        Phase::AwaitRoll => "Space roll · e share · [ ] scroll · M sound · Esc menu · ? rules",
        Phase::Choosing { .. } => "←→ select · Enter play · 1-9 pick · h hint · ? rules · Esc menu",
        Phase::NoMoves { .. } => "Space continue · Esc menu · ? rules",
    }
}

/// "Light  ●●●●●  ⟶  home ●●"
fn reserve_line(app: &App, player: Player) -> Paragraph<'static> {
    let color = player_color(player);
    let mine = app.game.turn() == player;
    let anim = app.piece_animation().filter(|_| mine);
    // While a piece animates, it has already left the waiting row, and a
    // borne-off piece counts as home once it hops off the board.
    let left_reserve = anim.is_some_and(|a| a.mv.is_entry());
    let arrived_home = anim.is_some_and(|a| a.mv.is_bear_off() && a.pos == FINISH);
    let waiting = app.game.waiting(player) - u8::from(left_reserve);
    let finished = app.game.finished(player) + u8::from(arrived_home);
    let target = app.selected_or_moving().filter(|_| mine);
    let entry_hl = anim.is_none() && target.is_some_and(|o| o.mv.is_entry());
    let home_hl = !arrived_home && target.is_some_and(|o| o.mv.is_bear_off());
    let active = app.game.turn() == player && !matches!(app.phase, Phase::GameOver(_));

    let name_style = if active {
        Style::new().fg(color).bold().underlined()
    } else {
        Style::new().fg(color)
    };
    let ctrl = match app.controller(player) {
        Controller::Human => String::new(),
        Controller::Computer(d) => format!(" [{d}]"),
    };
    let pieces = |n: u8, hl: bool| {
        let mut s: Vec<Span> = (0..n)
            .map(|i| {
                let style = if hl && i + 1 == n {
                    Style::new().fg(ACCENT).bold()
                } else {
                    Style::new().fg(color)
                };
                Span::styled(PIECE, style)
            })
            .collect();
        if n == 0 {
            s.push(Span::styled("–", Style::new().fg(DIM)));
        }
        s
    };

    let mut spans = vec![
        Span::styled(format!("{player}{ctrl}"), name_style),
        Span::raw("  "),
    ];
    spans.extend(pieces(waiting, entry_hl));
    spans.push(Span::styled("  ⟶  home ", Style::new().fg(DIM)));
    spans.extend(pieces(finished, false));
    if home_hl {
        spans.push(Span::styled(GHOST, Style::new().fg(TARGET).bold()));
    }
    spans.push(Span::styled(
        format!(" {finished}/{PIECES_PER_PLAYER}"),
        Style::new().fg(DIM),
    ));
    Paragraph::new(Line::from(spans))
}

/// How one square should look, shared by both board renderers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CellView {
    highlight: Highlight,
    content: Content,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Highlight {
    None,
    /// The piece that will move (while choosing).
    From,
    /// Where the selected or animating piece will land.
    To,
    /// Where the animating piece is right now.
    Moving,
    /// The animating piece has landed on an enemy.
    Impact,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Content {
    Empty,
    Rosette,
    Piece(Player),
    /// Destination of the selected move (empty square).
    Ghost(Player),
    /// Destination holds an enemy that would be captured.
    Victim(Player),
    /// The animating piece passing through or landing.
    Mover(Player),
    /// The animating piece striking an enemy.
    Strike(Player),
}

fn cell_view(app: &App, row: usize, col: usize) -> CellView {
    let game = &app.game;
    let mover = game.turn();
    let here = Some((row, col));
    let anim = app.piece_animation();
    let from_cell = app.selected_move().and_then(|o| cell_of(mover, o.mv.from));
    let to_cell = app
        .selected_or_moving()
        .filter(|_| !anim.is_some_and(|a| a.landed))
        .and_then(|o| cell_of(mover, o.mv.to));
    let hidden_cell = anim.and_then(|a| cell_of(mover, a.mv.from));
    let moving_cell = anim.and_then(|a| cell_of(mover, a.pos));
    let impact = anim.is_some_and(|a| a.impact);

    // The animating piece has left its start square.
    let occupant = game
        .piece_at_cell(row, col)
        .filter(|&p| !(p == mover && here == hidden_cell));
    let highlight = if here == moving_cell && impact {
        Highlight::Impact
    } else if here == moving_cell {
        Highlight::Moving
    } else if here == to_cell {
        Highlight::To
    } else if here == from_cell {
        Highlight::From
    } else {
        Highlight::None
    };
    let content = match occupant {
        _ if here == moving_cell && impact => Content::Strike(mover),
        _ if here == moving_cell => Content::Mover(mover),
        Some(p) if here == to_cell => Content::Victim(p),
        Some(p) => Content::Piece(p),
        None if here == to_cell => Content::Ghost(mover),
        None if is_rosette_cell(row, col) => Content::Rosette,
        None => Content::Empty,
    };
    CellView { highlight, content }
}

/// Path number shown on a square. The numbers are the same on both private
/// rows and the shared row is common, so one label per square is unambiguous.
fn square_number(row: usize, col: usize) -> Option<u8> {
    path_position(Player::Light, row, col).or(path_position(Player::Dark, row, col))
}

fn content_line(content: Content) -> Line<'static> {
    let style = |p: Player| Style::new().fg(player_color(p)).bold();
    match content {
        Content::Empty => Line::from(""),
        Content::Rosette => Line::from(Span::styled(ROSETTE, Style::new().fg(ROSETTE_COLOR))),
        Content::Piece(p) | Content::Mover(p) => Line::from(Span::styled(PIECE, style(p))),
        Content::Ghost(p) => Line::from(Span::styled(GHOST, style(p))),
        Content::Victim(p) => Line::from(Span::styled(
            format!("{GHOST}{PIECE}"),
            Style::new()
                .fg(player_color(p))
                .add_modifier(Modifier::CROSSED_OUT),
        )),
        Content::Strike(p) => Line::from(vec![
            Span::styled("⚔", Style::new().fg(Color::LightRed).bold()),
            Span::styled(PIECE, style(p)),
        ]),
    }
}

/// Full layout: each square is a small bordered box.
struct BoardView<'a> {
    app: &'a App,
}

impl Widget for BoardView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for row in 0..ROWS {
            for col in 0..COLS {
                if !is_board_cell(row, col) {
                    continue;
                }
                let rect = Rect::new(
                    area.x + col as u16 * CELL_W,
                    area.y + row as u16 * CELL_H,
                    CELL_W,
                    CELL_H,
                )
                .intersection(area);
                if rect.width < 3 || rect.height < 3 {
                    continue;
                }
                let view = cell_view(self.app, row, col);
                let (border_style, border_type) = match view.highlight {
                    Highlight::Impact => {
                        (Style::new().fg(Color::LightRed).bold(), BorderType::Thick)
                    }
                    Highlight::Moving | Highlight::From => {
                        (Style::new().fg(ACCENT).bold(), BorderType::Thick)
                    }
                    Highlight::To => (Style::new().fg(TARGET).bold(), BorderType::Thick),
                    Highlight::None if is_rosette_cell(row, col) => {
                        (Style::new().fg(ROSETTE_COLOR), BorderType::Double)
                    }
                    Highlight::None => (Style::new().fg(BOARD_EDGE), BorderType::Plain),
                };
                let number = square_number(row, col).map_or(String::new(), |n| n.to_string());
                let tile = if is_rosette_cell(row, col) {
                    TILE_ROSETTE
                } else if (row + col) % 2 == 0 {
                    TILE_A
                } else {
                    TILE_B
                };
                let block = Block::bordered()
                    .style(Style::new().bg(tile))
                    .border_type(border_type)
                    .border_style(border_style)
                    .title_bottom(Line::from(number).fg(DIM).right_aligned());
                let inner = block.inner(rect);
                block.render(rect, buf);
                Paragraph::new(content_line(view.content).centered()).render(inner, buf);
            }
        }
    }
}

/// Compact layout: each square is a borderless coloured tile, 2 rows tall,
/// path number on top and the piece below.
struct TileBoard<'a> {
    app: &'a App,
}

impl Widget for TileBoard<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for row in 0..ROWS {
            for col in 0..COLS {
                if !is_board_cell(row, col) {
                    continue;
                }
                let rect = Rect::new(
                    area.x + col as u16 * (TILE_W + TILE_GAP),
                    area.y + row as u16 * TILE_H,
                    TILE_W,
                    TILE_H,
                )
                .intersection(area);
                if rect.width < TILE_W || rect.height < TILE_H {
                    continue;
                }
                let view = cell_view(self.app, row, col);
                let bg = match view.highlight {
                    Highlight::Impact => TILE_IMPACT,
                    Highlight::Moving | Highlight::From => TILE_FROM,
                    Highlight::To => TILE_TO,
                    Highlight::None if is_rosette_cell(row, col) => TILE_ROSETTE,
                    // Checkerboard shading keeps adjacent tiles distinct.
                    Highlight::None if (row + col) % 2 == 0 => TILE_A,
                    Highlight::None => TILE_B,
                };
                buf.set_style(rect, Style::new().bg(bg));
                let number = square_number(row, col).map_or(String::new(), |n| n.to_string());
                let [top, bottom] =
                    Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(rect);
                Paragraph::new(Line::from(number).fg(TILE_NUMBER)).render(top, buf);
                let content = match view.content {
                    // Brighter star so it reads on the lapis tile.
                    Content::Rosette => Line::from(Span::styled(
                        ROSETTE,
                        Style::new().fg(Color::Rgb(150, 180, 245)),
                    )),
                    c => content_line(c),
                };
                Paragraph::new(content.centered()).render(bottom, buf);
            }
        }
    }
}

fn panel(title: &str) -> Block<'_> {
    Block::bordered()
        .border_type(BorderType::Double)
        .style(Style::new().bg(PANEL).fg(TEXT))
        .border_style(Style::new().fg(BOARD_EDGE))
        .title(Line::from(format!(" {title} ")).fg(ACCENT))
        .padding(Padding::horizontal(1))
}

fn draw_dice(frame: &mut Frame, app: &App, area: Rect) {
    let block = panel("Dice");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if let Some((tips, settled)) = app.tumbling_dice() {
        let color = player_color(app.game.turn());
        let dice: Vec<Span> = (0..4)
            .flat_map(|i| {
                let marked = tips & (1 << i) != 0;
                let sym = match (marked, settled) {
                    (true, _) => "▲",
                    (false, true) => "△",
                    // Mid-tumble, unmarked dice lie on their side.
                    (false, false) => "◁",
                };
                let style = if marked {
                    Style::new().fg(color).bold()
                } else {
                    Style::new().fg(DIM)
                };
                [Span::styled(sym, style), Span::raw("  ")]
            })
            .collect();
        // A little bounce: the dice hop between the top two rows while tumbling.
        let bounce = !settled && tips.count_ones() % 2 == 1;
        let mut lines = vec![Line::from(dice).centered()];
        if bounce {
            lines.insert(0, Line::from(""));
        }
        let label = if settled {
            format!("{}  {}", app.game.turn(), tips.count_ones())
        } else {
            "throwing…".to_string()
        };
        lines.push(
            Line::from(label)
                .fg(if settled { color } else { DIM })
                .centered(),
        );
        frame.render_widget(Paragraph::new(lines), inner);
        return;
    }

    let text = match app.dice {
        None => Text::from(vec![
            Line::from(""),
            Line::from("Space to throw").fg(DIM).centered(),
        ]),
        Some(Dice {
            player,
            tips,
            value,
        }) => {
            let stale = matches!(app.phase, Phase::AwaitRoll);
            let color = if stale { DIM } else { player_color(player) };
            let dice: Vec<Span> = (0..4)
                .flat_map(|i| {
                    let marked = tips & (1 << i) != 0;
                    let (sym, style) = if marked {
                        ("▲", Style::new().fg(color).bold())
                    } else {
                        ("△", Style::new().fg(DIM))
                    };
                    [Span::styled(sym, style), Span::raw("  ")]
                })
                .collect();
            let label = if stale {
                format!("{player} threw {value}")
            } else {
                format!("{player} threw")
            };
            Text::from(vec![
                Line::from(dice).centered(),
                Line::from(vec![
                    Span::styled(label, Style::new().fg(DIM)),
                    if stale {
                        Span::raw("")
                    } else {
                        Span::styled(format!("  {value}"), Style::new().fg(color).bold())
                    },
                ])
                .centered(),
                Line::from(match value {
                    0 => "no move",
                    4 => "the rarest throw!",
                    _ => "",
                })
                .fg(DIM)
                .centered(),
            ])
        }
    };
    frame.render_widget(Paragraph::new(text), inner);
}

/// One-line dice readout for the compact layout: "▲ ▲ △ △  2".
fn draw_dice_compact(frame: &mut Frame, app: &App, area: Rect) {
    let block = panel("Dice");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let (tips, color, value) = match (app.tumbling_dice(), app.dice) {
        (Some((tips, settled)), _) => {
            let color = player_color(app.game.turn());
            (tips, color, settled.then(|| tips.count_ones() as u8))
        }
        (
            None,
            Some(Dice {
                player,
                tips,
                value,
            }),
        ) => {
            let stale = matches!(app.phase, Phase::AwaitRoll);
            (
                tips,
                if stale { DIM } else { player_color(player) },
                Some(value),
            )
        }
        (None, None) => {
            frame.render_widget(Paragraph::new("Space to throw").fg(DIM).centered(), inner);
            return;
        }
    };
    let mut spans: Vec<Span> = (0..4)
        .map(|i| {
            if tips & (1 << i) != 0 {
                Span::styled("▲ ", Style::new().fg(color).bold())
            } else {
                Span::styled("△ ", Style::new().fg(DIM))
            }
        })
        .collect();
    spans.push(match value {
        Some(v) => Span::styled(format!(" {v}"), Style::new().fg(color).bold()),
        None => Span::styled(" …", Style::new().fg(DIM)),
    });
    frame.render_widget(Paragraph::new(Line::from(spans).centered()), inner);
}

/// Short move label for narrow panels: "new → 3", "5 → 9", "13 → off".
fn short_move(mv: Move) -> String {
    let from = if mv.from == START {
        "new".to_string()
    } else {
        mv.from.to_string()
    };
    let to = if mv.to == FINISH {
        "off".to_string()
    } else {
        mv.to.to_string()
    };
    format!("{from} → {to}")
}

fn draw_moves(frame: &mut Frame, app: &App, area: Rect, mode: Mode) {
    let describe = |mv: Move| match mode {
        Mode::Full => describe_move(mv),
        Mode::Compact => short_move(mv),
    };
    let block = panel("Moves");
    let items: Vec<ListItem> = match &app.phase {
        Phase::Choosing {
            options, selected, ..
        } => options
            .iter()
            .enumerate()
            .map(|(i, o)| {
                let mut spans = vec![Span::raw(format!("{} ", i + 1)), Span::raw(describe(o.mv))];
                if o.outcome.captured {
                    spans.push(Span::styled(" ⚔", Style::new().fg(Color::LightRed).bold()));
                }
                if o.outcome.extra_turn {
                    spans.push(Span::styled(" ✦", Style::new().fg(ROSETTE_COLOR).bold()));
                }
                if o.outcome.winner.is_some() {
                    spans.push(Span::styled(" ♛", Style::new().fg(ACCENT).bold()));
                }
                if o.risk > 0 {
                    spans.push(Span::styled(
                        format!(" ⚠{}%", risk_percent(o.risk)),
                        Style::new().fg(RISK_COLOR).bold(),
                    ));
                }
                if let Some(hint) = &app.hint {
                    spans.push(if i == hint.best {
                        Span::styled(" ★", Style::new().fg(ACCENT).bold())
                    } else {
                        Span::styled(format!(" −{:.1}", hint.loss[i]), Style::new().fg(DIM))
                    });
                }
                let line = Line::from(spans);
                if i == *selected {
                    ListItem::new(line).style(Style::new().fg(Color::Black).bg(ACCENT))
                } else {
                    ListItem::new(line)
                }
            })
            .collect(),
        Phase::Moving { option, .. } => vec![
            ListItem::new(format!("▶ {}", describe(option.mv)))
                .style(Style::new().fg(Color::Black).bg(ACCENT)),
        ],
        Phase::NoMoves { .. } => vec![ListItem::new("No legal move.").fg(DIM)],
        _ => vec![],
    };
    let block = match app.hint_reason() {
        Some((true, why)) => block.title_bottom(Line::from(format!(" ★ {why} ")).fg(ACCENT).bold()),
        Some((false, why)) => block.title_bottom(Line::from(format!(" {why} ")).fg(TEXT)),
        None => block,
    };
    frame.render_widget(List::new(items).block(block), area);
}

fn draw_log(frame: &mut Frame, app: &App, area: Rect) {
    let title = if app.log_scroll > 0 {
        format!("Chronicle ↑{}", app.log_scroll)
    } else {
        "Chronicle".to_string()
    };
    let mut block = panel(&title);
    if app.log_scroll > 0 {
        block = block.title_bottom(Line::from(" ] newer ").fg(DIM).right_aligned());
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    // Word-wrap each entry, then keep as many lines as fit, ending at the
    // newest entry (or where the reader has scrolled back to).
    let width = inner.width as usize;
    let shown = &app.log[..app.log.len() - app.log_scroll.min(app.log.len())];
    let mut lines: Vec<Line> = Vec::new();
    for (i, entry) in shown.iter().enumerate() {
        let newest = i + 1 == app.log.len();
        let style = if newest {
            Style::new()
        } else {
            Style::new().fg(DIM)
        };
        // The bullet takes the colour of the player the entry is about.
        let bullet = match entry_player(entry) {
            Some(p) => Style::new().fg(player_color(p)),
            None => Style::new().fg(ACCENT),
        };
        for (j, chunk) in word_wrap(entry, width.saturating_sub(2))
            .into_iter()
            .enumerate()
        {
            let prefix = if j == 0 { "• " } else { "  " };
            lines.push(Line::from(vec![
                Span::styled(prefix, bullet),
                Span::styled(chunk, style),
            ]));
        }
    }
    let start = lines.len().saturating_sub(inner.height as usize);
    frame.render_widget(Paragraph::new(lines.split_off(start)), inner);
}

/// Greedy word wrap; words longer than `width` are hard-split.
fn word_wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let mut word = word.to_string();
        loop {
            let needed = if line.is_empty() {
                0
            } else {
                line.chars().count() + 1
            };
            if needed + word.chars().count() <= width {
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(&word);
                break;
            }
            if line.is_empty() {
                // Hard-split an over-long word.
                let head: String = word.chars().take(width).collect();
                word = word.chars().skip(width).collect();
                out.push(head);
            } else {
                out.push(std::mem::take(&mut line));
            }
        }
    }
    if !line.is_empty() || out.is_empty() {
        out.push(line);
    }
    out
}

// ---------------------------------------------------------------------------
// Help overlay
// ---------------------------------------------------------------------------

fn draw_help(frame: &mut Frame, area: Rect, mode: Mode) {
    let popup = area.centered(
        Constraint::Length(74.min(area.width)),
        Constraint::Length(24.min(area.height)),
    );
    frame.render_widget(Clear, popup);
    let h = |s: &'static str| Line::from(s).fg(ACCENT).bold();
    let short = vec![
        Line::from("Race all 7 pieces along your path and off the board."),
        Line::from(vec![
            Span::styled("Path ", Style::new().fg(ACCENT).bold()),
            Span::raw(
                "your row 1–4 (left), middle row 5–12 (right), your row 13–14, then off with an exact throw.",
            ),
        ]),
        Line::from(vec![
            Span::styled("Dice ", Style::new().fg(ACCENT).bold()),
            Span::raw("count the marked tips, 0–4. 2 is the most likely."),
        ]),
        Line::from("You must move if you can. No move, or a 0: turn passes."),
        Line::from(vec![
            Span::styled("⚔ ", Style::new().fg(Color::LightRed).bold()),
            Span::raw("Land on an enemy in the middle row to send it back."),
        ]),
        Line::from(vec![
            Span::styled("✦ ", Style::new().fg(ROSETTE_COLOR).bold()),
            Span::raw("Rosettes give another throw; the middle one is safe."),
        ]),
    ];
    let full = vec![
        h("Goal"),
        Line::from("Be the first to move all 7 of your pieces along your path and off the board."),
        Line::from(""),
        h("Path (squares are numbered 1–14 on the board)"),
        Line::from(
            "Enter on your own row and run left (1–4), along the shared middle row to the right (5–12), back onto your own row (13–14), then off with an exact throw.",
        ),
        Line::from(""),
        h("Dice"),
        Line::from(
            "Four 4-sided dice with two marked tips each. Your throw is the marks showing (0–4); 2 is most common, 0 and 4 just 1 time in 16.",
        ),
        Line::from(""),
        h("Moving"),
        Line::from(
            "Move one piece exactly the number thrown. You cannot land on your own piece, but you may jump over pieces. No legal move (or a 0): your turn passes.",
        ),
        Line::from(vec![
            Span::styled("⚔ Capture  ", Style::new().fg(Color::LightRed).bold()),
            Span::raw("Land on an enemy in the middle row: it goes back to start."),
        ]),
        Line::from(vec![
            Span::styled("✦ Rosette  ", Style::new().fg(ROSETTE_COLOR).bold()),
            Span::raw("Throw again. The middle rosette is also safe from capture."),
        ]),
        Line::from(vec![
            Span::styled("⚠ Risk  ", Style::new().fg(RISK_COLOR).bold()),
            Span::raw("Chance the moved piece can be captured on the next throw."),
        ]),
    ];
    let text = match mode {
        Mode::Full => full,
        Mode::Compact => short,
    };
    let block = Block::bordered()
        .border_type(BorderType::Double)
        .border_style(Style::new().fg(BOARD_EDGE))
        .style(Style::new().bg(PANEL).fg(TEXT))
        .title(Line::from(" How to play ").bold().centered())
        .title_bottom(Line::from(" press any key to close ").fg(DIM).centered())
        .padding(Padding::horizontal(1));
    frame.render_widget(
        Paragraph::new(text).block(block).wrap(Wrap { trim: true }),
        popup,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Time;
    use crate::app::{AI_DELAY, App};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent};
    use ur_game::ai::Difficulty;

    fn render(app: &App, w: u16, h: u16) -> String {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw(f, app)).unwrap();
        let buf = term.backend().buffer().clone();
        let mut s = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                s.push_str(buf[(x, y)].symbol());
            }
            s.push('\n');
        }
        s
    }

    #[test]
    fn renders_menu() {
        let out = render(&App::new(0), 100, 30);
        assert!(out.contains("Royal Game of Ur"));
        assert!(out.contains("New game"));
    }

    #[test]
    fn too_small_terminal_shows_message() {
        let out = render(&App::new(0), 40, 10);
        assert!(out.contains("too small"));
    }

    #[test]
    fn renders_whole_game_at_minimum_and_large_sizes() {
        let ai = Controller::Computer(Difficulty::Easy);
        let mut app = App::new(9);
        app.controllers = [ai, ai];
        let mut now = Time::ZERO;
        app.new_game(now);
        app.show_help = false;
        // Render a full AI game, animations included, at two sizes: must
        // never panic. Small time steps catch mid-animation frames.
        let mut saw_rolling = false;
        let mut saw_moving = false;
        for _ in 0..20_000 {
            saw_rolling |= matches!(app.phase, Phase::Rolling { .. });
            saw_moving |= matches!(app.phase, Phase::Moving { .. });
            for (w, h) in [(MIN_W, MIN_H), (160, 50)] {
                render(&app, w, h);
            }
            if matches!(app.phase, Phase::GameOver(_)) {
                break;
            }
            now += AI_DELAY / 4;
            app.update(now);
        }
        assert!(matches!(app.phase, Phase::GameOver(_)));
        assert!(saw_rolling && saw_moving);
        assert!(render(&app, 120, 30).contains("wins!"));
    }

    #[test]
    fn word_wrap_behaviour() {
        assert_eq!(word_wrap("a bb ccc dddd", 6), vec!["a bb", "ccc", "dddd"]);
        assert_eq!(word_wrap("abcdefgh", 3), vec!["abc", "def", "gh"]);
        assert_eq!(word_wrap("", 5), vec![""]);
        assert_eq!(
            word_wrap("Light rolls 2 → 3", 40),
            vec!["Light rolls 2 → 3"]
        );
    }

    #[test]
    fn move_list_and_help_show_capture_risk() {
        use crate::app::MoveOption;
        use ur_game::game::Outcome;
        let mut app = App::new(0);
        app.controllers = [Controller::Human, Controller::Human];
        app.new_game(Time::ZERO);
        app.phase = Phase::Choosing {
            roll: 2,
            options: vec![MoveOption {
                mv: Move { from: 7, to: 9 },
                outcome: Outcome::default(),
                risk: 5,
            }],
            selected: 0,
        };
        for (w, h) in [(MIN_W, MIN_H), (COMPACT_MIN_W, COMPACT_MIN_H)] {
            let out = render(&app, w, h);
            assert!(out.contains("⚠31%"), "{w}×{h}:\n{out}");
        }
        app.hint = Some(crate::app::Hint {
            best: 0,
            loss: vec![0.0],
            reasons: vec!["escapes 63% danger".into()],
        });
        for (w, h) in [(MIN_W, MIN_H), (COMPACT_MIN_W, COMPACT_MIN_H)] {
            let out = render(&app, w, h);
            assert!(out.contains("★ escapes 63% danger"), "{w}×{h}:\n{out}");
        }
        app.show_help = true;
        let out = render(&app, MIN_W, MIN_H);
        assert!(out.contains("⚠ Risk"), "{out}");
    }

    #[test]
    fn leaderboard_screen_fits_and_shows_records() {
        let mut app = App::new(0);
        app.leaderboard.add(Difficulty::Easy, true);
        app.leaderboard.add(Difficulty::Easy, false);
        app.leaderboard.add(Difficulty::Easy, true);
        app.screen = Screen::Leaderboard;
        for (w, h) in [(COMPACT_MIN_W, COMPACT_MIN_H), (MIN_W, MIN_H)] {
            let out = render(&app, w, h);
            assert!(out.contains("Leaderboard"), "{out}");
            assert!(out.contains("Easy           3     2     1    67%"), "{out}");
            assert!(out.contains("Medium         0     –     –      –"), "{out}");
            assert!(out.contains("Total          3"), "{out}");
            assert!(out.contains("Enter / Esc back"), "{out}");
        }
        // The menu still fits all seven rows on the smallest screen.
        app.screen = Screen::Menu;
        let out = render(&app, COMPACT_MIN_W, COMPACT_MIN_H);
        assert!(out.contains("Leaderboard") && out.contains("Quit"), "{out}");
    }

    #[test]
    fn picks_layout_by_size() {
        let r = |w, h| Rect::new(0, 0, w, h);
        assert_eq!(mode_for(r(MIN_W, MIN_H)), Some(Mode::Full));
        assert_eq!(mode_for(r(200, 60)), Some(Mode::Full));
        // Phone in landscape: wide but short.
        assert_eq!(mode_for(r(100, 16)), Some(Mode::Compact));
        assert_eq!(
            mode_for(r(COMPACT_MIN_W, COMPACT_MIN_H)),
            Some(Mode::Compact)
        );
        // Narrow but tall also falls back to compact.
        assert_eq!(mode_for(r(70, 40)), Some(Mode::Compact));
        assert_eq!(mode_for(r(COMPACT_MIN_W - 1, 30)), None);
        assert_eq!(mode_for(r(100, COMPACT_MIN_H - 1)), None);
    }

    #[test]
    fn compact_layout_renders_a_whole_game() {
        let ai = Controller::Computer(Difficulty::Easy);
        let mut app = App::new(4);
        app.controllers = [ai, ai];
        let mut now = Time::ZERO;
        app.new_game(now);
        for _ in 0..20_000 {
            for (w, h) in [(COMPACT_MIN_W, COMPACT_MIN_H), (100, 16), (70, 40)] {
                render(&app, w, h);
            }
            if matches!(app.phase, Phase::GameOver(_)) {
                break;
            }
            now += AI_DELAY / 4;
            app.update(now);
        }
        assert!(matches!(app.phase, Phase::GameOver(_)));
        let out = render(&app, COMPACT_MIN_W, COMPACT_MIN_H);
        assert!(out.contains("wins!"));
        // Path numbers are on the tiles.
        assert!(out.contains("14") && out.contains("12"));
    }

    #[test]
    fn compact_menu_and_help_fit() {
        let mut app = App::new(0);
        let out = render(&app, COMPACT_MIN_W, COMPACT_MIN_H);
        assert!(out.contains("New game") && out.contains("Quit"));
        assert!(out.contains("Royal Game of Ur"));
        // With a game to continue, all six rows still fit.
        app.controllers[1] = Controller::Computer(Difficulty::Hard);
        app.new_game(Time::ZERO);
        app.handle_key(KeyEvent::from(KeyCode::Esc), Time::ZERO);
        for size in [(COMPACT_MIN_W, COMPACT_MIN_H), (MIN_W, MIN_H)] {
            let out = render(&app, size.0, size.1);
            assert!(out.contains("▶ Continue (Human vs Hard)"), "{out}");
            assert!(out.contains("Quit"), "{out}");
        }
        app.handle_key(KeyEvent::from(KeyCode::Char('?')), Time::ZERO);
        let out = render(&app, COMPACT_MIN_W, COMPACT_MIN_H);
        assert!(out.contains("Rosettes give another throw"), "{out}");
    }

    #[test]
    fn short_move_labels() {
        assert_eq!(short_move(Move { from: START, to: 3 }), "new → 3");
        assert_eq!(short_move(Move { from: 5, to: 9 }), "5 → 9");
        assert_eq!(
            short_move(Move {
                from: 13,
                to: FINISH
            }),
            "13 → off"
        );
    }

    #[test]
    fn renders_help_overlay() {
        let mut app = App::new(0);
        app.handle_key(KeyEvent::from(KeyCode::Char('?')), Time::ZERO);
        assert!(render(&app, 100, 30).contains("How to play"));
    }
}

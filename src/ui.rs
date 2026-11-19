//! Rendering. Reads `App`, never mutates it.

use crate::app::{App, Controller, Dice, MenuItem, Phase, Screen, describe_move};
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Clear, List, ListItem, Padding, Paragraph, Widget, Wrap,
};
use ur_game::game::{
    COLS, GameState, PIECES_PER_PLAYER, Player, ROWS, cell_of, is_board_cell, is_rosette_cell,
    path_position,
};

const CELL_W: u16 = 6;
const CELL_H: u16 = 3;
const BOARD_W: u16 = CELL_W * COLS as u16;
const BOARD_H: u16 = CELL_H * ROWS as u16;
const SIDE_W: u16 = 32;
pub const MIN_W: u16 = BOARD_W + SIDE_W + 2;
pub const MIN_H: u16 = 22;

const PIECE: &str = "●";
const GHOST: &str = "◌";
const ROSETTE: &str = "✦";

const ACCENT: Color = Color::Yellow;
const TARGET: Color = Color::Green;
const DIM: Color = Color::DarkGray;
const BOARD_EDGE: Color = Color::Rgb(150, 110, 60); // lapis-and-shell boards had wooden frames
const ROSETTE_COLOR: Color = Color::Rgb(70, 110, 200); // lapis lazuli

fn player_color(p: Player) -> Color {
    match p {
        Player::Light => Color::White,
        Player::Dark => Color::LightRed,
    }
}

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    if area.width < MIN_W || area.height < MIN_H {
        let msg = format!(
            "Terminal too small: {}×{}\nNeed at least {MIN_W}×{MIN_H}.\n\nq to quit",
            area.width, area.height
        );
        frame.render_widget(Paragraph::new(msg).alignment(Alignment::Center), area);
        return;
    }
    match app.screen {
        Screen::Menu => draw_menu(frame, app, area),
        Screen::Game => draw_game(frame, app, area),
    }
    if app.show_help {
        draw_help(frame, area);
    }
}

// ---------------------------------------------------------------------------
// Menu
// ---------------------------------------------------------------------------

fn draw_menu(frame: &mut Frame, app: &App, area: Rect) {
    let box_area = area.centered(Constraint::Length(56), Constraint::Length(20));
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(BOARD_EDGE))
        .title(Line::from(" 𒌷 The Royal Game of Ur ").bold().centered())
        .padding(Padding::uniform(1));
    let inner = block.inner(box_area);
    frame.render_widget(block, box_area);

    let [intro, preview, _, items, _, hint] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(4),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    frame.render_widget(
        Paragraph::new("Played in Mesopotamia 4,500 years ago.\nRace all seven pieces home.")
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

    let lines: Vec<Line> = MenuItem::ALL
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let selected = i == app.menu_selected;
            let cursor = if selected { "▶ " } else { "  " };
            let style = if selected {
                Style::new().fg(ACCENT).bold()
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
                        Span::styled(format!(" {:<7}", p.to_string()), style),
                        Span::styled(format!("◀ {value:^18} ▶"), style),
                    ])
                }
                MenuItem::Start => Line::from(Span::styled(format!("{cursor}Start game"), style)),
                MenuItem::Quit => Line::from(Span::styled(format!("{cursor}Quit"), style)),
            }
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines),
        items.centered_horizontally(Constraint::Length(38)),
    );

    frame.render_widget(
        Paragraph::new("↑↓ select · ←→ change · Enter start · ? rules")
            .alignment(Alignment::Center)
            .fg(DIM),
        hint,
    );
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

fn draw_game(frame: &mut Frame, app: &App, area: Rect) {
    let [title, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(area);
    let [left, right] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(SIDE_W)])
        .spacing(1)
        .areas(body);

    frame.render_widget(status_line(app), title);
    frame.render_widget(Paragraph::new(footer_hint(app)).fg(DIM).centered(), footer);

    // Board column, vertically centred.
    let stack_h = BOARD_H + 4;
    let col = left.centered(
        Constraint::Length(BOARD_W.min(left.width)),
        Constraint::Length(stack_h),
    );
    let [light_res, _, board, _, dark_res] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(BOARD_H),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(col);
    frame.render_widget(reserve_line(app, Player::Light), light_res);
    frame.render_widget(BoardView { app }, board);
    frame.render_widget(reserve_line(app, Player::Dark), dark_res);

    // Side panel.
    let [dice_area, moves_area, log_area] = Layout::vertical([
        Constraint::Length(5),
        Constraint::Length(9),
        Constraint::Fill(1),
    ])
    .areas(right);
    draw_dice(frame, app, dice_area);
    draw_moves(frame, app, moves_area);
    draw_log(frame, app, log_area);
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
        _ if !app.current_is_human() => " (computer) is thinking…".to_string(),
        Phase::AwaitRoll => " to roll the dice".to_string(),
        Phase::Choosing { .. } => " to choose a move".to_string(),
        Phase::NoMoves { .. } => " cannot move".to_string(),
    };
    Line::from(vec![who, Span::raw(tail)]).centered()
}

fn footer_hint(app: &App) -> &'static str {
    match &app.phase {
        Phase::GameOver(_) => "Enter/n rematch · Esc menu · q quit",
        _ if !app.current_is_human() => "Space hurry · Esc menu · ? rules · q quit",
        Phase::AwaitRoll => "Space roll · Esc menu · ? rules · q quit",
        Phase::Choosing { .. } => "←→ select · Enter play · 1-9 quick pick · Esc menu · ? rules",
        Phase::NoMoves { .. } => "Space continue · Esc menu · ? rules",
    }
}

/// "● Light  ○○○○○ waiting      home ●●"
fn reserve_line(app: &App, player: Player) -> Paragraph<'static> {
    let color = player_color(player);
    let waiting = app.game.waiting(player);
    let finished = app.game.finished(player);
    let selected = app.selected_move().filter(|_| app.game.turn() == player);
    let entry_hl = selected.is_some_and(|o| o.mv.is_entry());
    let home_hl = selected.is_some_and(|o| o.mv.is_bear_off());
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

struct BoardView<'a> {
    app: &'a App,
}

impl Widget for BoardView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let game: &GameState = &self.app.game;
        let selected = self.app.selected_move();
        let mover = game.turn();
        let from_cell = selected.and_then(|o| cell_of(mover, o.mv.from));
        let to_cell = selected.and_then(|o| cell_of(mover, o.mv.to));

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
                let here = Some((row, col));
                let rosette = is_rosette_cell(row, col);
                let (border_style, border_type) = if here == to_cell {
                    (Style::new().fg(TARGET).bold(), BorderType::Thick)
                } else if here == from_cell {
                    (Style::new().fg(ACCENT).bold(), BorderType::Thick)
                } else if rosette {
                    (Style::new().fg(ROSETTE_COLOR), BorderType::Double)
                } else {
                    (Style::new().fg(BOARD_EDGE), BorderType::Rounded)
                };
                // Path numbers are the same on both private rows and the
                // shared row is common, so one label per square is unambiguous.
                let number = path_position(Player::Light, row, col).or(path_position(
                    Player::Dark,
                    row,
                    col,
                ));
                let block = Block::bordered()
                    .border_type(border_type)
                    .border_style(border_style)
                    .title_bottom(
                        Line::from(number.map_or(String::new(), |n| n.to_string()))
                            .fg(DIM)
                            .right_aligned(),
                    );
                let inner = block.inner(rect);
                block.render(rect, buf);

                let content = match game.piece_at_cell(row, col) {
                    Some(p) if here == to_cell => {
                        // Capture preview: victim shown struck in the target colour.
                        Span::styled(
                            format!("{GHOST}{PIECE}"),
                            Style::new()
                                .fg(player_color(p))
                                .add_modifier(Modifier::CROSSED_OUT),
                        )
                    }
                    Some(p) => Span::styled(PIECE, Style::new().fg(player_color(p)).bold()),
                    None if here == to_cell => {
                        Span::styled(GHOST, Style::new().fg(player_color(mover)).bold())
                    }
                    None if rosette => Span::styled(ROSETTE, Style::new().fg(ROSETTE_COLOR)),
                    None => Span::raw(""),
                };
                Paragraph::new(Line::from(content).centered()).render(inner, buf);
            }
        }
    }
}

fn panel(title: &str) -> Block<'_> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(DIM))
        .title(format!(" {title} "))
        .padding(Padding::horizontal(1))
}

fn draw_dice(frame: &mut Frame, app: &App, area: Rect) {
    let block = panel("Dice");
    let inner = block.inner(area);
    frame.render_widget(block, area);

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

fn draw_moves(frame: &mut Frame, app: &App, area: Rect) {
    let block = panel("Moves");
    let items: Vec<ListItem> = match &app.phase {
        Phase::Choosing {
            options, selected, ..
        } => options
            .iter()
            .enumerate()
            .map(|(i, o)| {
                let mut spans = vec![
                    Span::raw(format!("{} ", i + 1)),
                    Span::raw(describe_move(o.mv)),
                ];
                if o.outcome.captured {
                    spans.push(Span::styled(" ⚔", Style::new().fg(Color::LightRed).bold()));
                }
                if o.outcome.extra_turn {
                    spans.push(Span::styled(" ✦", Style::new().fg(ROSETTE_COLOR).bold()));
                }
                if o.outcome.winner.is_some() {
                    spans.push(Span::styled(" ♛", Style::new().fg(ACCENT).bold()));
                }
                let line = Line::from(spans);
                if i == *selected {
                    ListItem::new(line).style(Style::new().fg(Color::Black).bg(ACCENT))
                } else {
                    ListItem::new(line)
                }
            })
            .collect(),
        Phase::NoMoves { .. } => vec![ListItem::new("No legal move.").fg(DIM)],
        _ => vec![],
    };
    frame.render_widget(List::new(items).block(block), area);
}

fn draw_log(frame: &mut Frame, app: &App, area: Rect) {
    let block = panel("Log");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    // Word-wrap each entry, then keep as many of the newest lines as fit,
    // so the latest event is always visible at the bottom.
    let width = inner.width as usize;
    let mut lines: Vec<Line> = Vec::new();
    for (i, entry) in app.log.iter().enumerate() {
        let newest = i + 1 == app.log.len();
        let style = if newest {
            Style::new()
        } else {
            Style::new().fg(DIM)
        };
        for (j, chunk) in word_wrap(entry, width.saturating_sub(2))
            .into_iter()
            .enumerate()
        {
            let prefix = if j == 0 { "• " } else { "  " };
            lines.push(Line::from(format!("{prefix}{chunk}")).style(style));
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

fn draw_help(frame: &mut Frame, area: Rect) {
    let popup = area.centered(
        Constraint::Length(74.min(area.width)),
        Constraint::Length(24.min(area.height)),
    );
    frame.render_widget(Clear, popup);
    let h = |s: &'static str| Line::from(s).fg(ACCENT).bold();
    let text = vec![
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
            "Four 4-sided dice, each with two marked tips. Your throw is the number of marks showing (0–4). 2 is most common; 0 and 4 come up only 1 time in 16.",
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
    ];
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(BOARD_EDGE))
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
    use crate::app::{AI_DELAY, App};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent};
    use std::time::Instant;
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
        assert!(out.contains("Start game"));
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
        let mut now = Instant::now();
        app.new_game(now);
        app.show_help = false;
        // Render every step of a full AI game at two sizes: must never panic.
        for _ in 0..5000 {
            for (w, h) in [(MIN_W, MIN_H), (160, 50)] {
                render(&app, w, h);
            }
            if matches!(app.phase, Phase::GameOver(_)) {
                break;
            }
            now += AI_DELAY * 2;
            app.update(now);
        }
        assert!(matches!(app.phase, Phase::GameOver(_)));
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
    fn renders_help_overlay() {
        let mut app = App::new(0);
        app.handle_key(KeyEvent::from(KeyCode::Char('?')), Instant::now());
        assert!(render(&app, 100, 30).contains("How to play"));
    }
}

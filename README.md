# The ancient Royal Game of Ur

One of the oldest board games known, playable in a window or in your
terminal.

The game was played in Mesopotamia about 4,500 years ago. The best-known
boards were dug up by Sir Leonard Woolley in the Royal Cemetery at Ur (in
modern Iraq) in the 1920s, and one is now in the British Museum. For a long
time nobody knew how it was played. Then Irving Finkel, a curator at the
British Museum, translated a Babylonian clay tablet written by a scribe named
Itti-Marduk-balāṭu in 177 BC that describes the rules. This game follows
Finkel's reconstruction, which is how the game is usually played today.

```
cargo run --release
```

You play against the computer, against a friend on the same keyboard, or
watch two computer players face each other.

## Look and feel

Both interfaces use a palette of midnight lapis, gold, shell, and carnelian.
The window has decorated tiles, inlaid rosettes, detailed pieces, and framed
panels. The terminal carries the same style into shaded board cells and
double-bordered panels, with a compact layout for smaller screens.
The **Chronicle** records throws, moves, captures, and hints.

## The rules

The goal is to move all seven of your pieces along your path and off the
board before your opponent does.

On your turn:

1. **Throw the dice.** Your throw is a number from 0 to 4.
2. **Move one piece exactly that many squares.** You can bring a new piece
   onto the board or move one that's already on it.
   - You can't land on one of your own pieces, but you can jump over pieces.
   - Moving a piece off the end needs the exact number. If you are on
     square 13, you need a 2.
   - If you have a legal move, you must make one. If you have none, or you
     threw a 0, your turn passes.
3. **Capture.** If you land on an opponent's piece in the shared middle
   row, it goes back to the start and has to begin again.
4. **Rosettes.** Landing on a rosette gives you another throw. The rosette
   in the middle of the board is also safe: a piece standing on it can't be
   captured, and an opponent can't land there.

## The board

The board has 20 squares, laid out in three rows with two gaps:

```
 ✦ ·  ·  ·        ✦  ·    ← Light's own row
 ·  ·  ·  ✦  ·  ·  ·  ·   ← shared middle row
 ✦ ·  ·  ·        ✦  ·    ← Dark's own row
```

Each player has a path of 14 squares, numbered on the board in the game:

- **1–4:** your own row, running left from where pieces enter.
- **5–12:** the shared middle row, running right. This is the only place
  where pieces can be captured.
- **13–14:** back onto your own row, then off the board.

The five rosettes (✦) are on squares 4, 8 and 14 of each path. Square 8,
the middle rosette, is shared by both players.

## The pieces

Each player has seven pieces, shown as `●`: shell white for Light and
carnelian red for Dark (brighter in the terminal for contrast). Light moves
first.

Pieces wait off the board until they enter, and are counted as home once
they have been moved off the far end. The rows above and below the board
show how many of each player's pieces are waiting and how many are home.

## The Dice

The original dice were tetrahedrons (four-sided pyramids), with two of the
four tips marked. You throw four of them and count how many marked tips
point up. Each die is a coin flip, so the throws are not equally likely:

| Throw | 0    | 1    | 2    | 3    | 4    |
|-------|------|------|------|------|------|
| Odds  | 1/16 | 4/16 | 6/16 | 4/16 | 1/16 |

A 2 is the most common throw, and a 0 or a 4 comes up only once in sixteen
throws. The game shows all four dice, with the marked tips filled in.

## Playing

You need [Rust](https://www.rust-lang.org/tools/install).

```
git clone https://github.com/wulfalpha/ur-game
cd ur-game
cargo run --release
```

This opens the game in a window. To play in the terminal instead, add
`--tui`:

```
cargo run --release -- --tui
```

The terminal version is chosen automatically when there's no display to
open a window on, for example over SSH. It needs a terminal at least 66
columns wide and 12 rows tall. Below 82×22 it switches to a compact
landscape layout, which fits a phone turned on its side.

In the window, click the dice to throw, then click a piece (or the square
it would land on) to move it. Hovering shows where it would go. All the
keys below work in the window too.

In the menu, choose Human or Computer for each side. The computer has four
levels: Random, Easy, Medium and Hard. You can also turn the dice and piece
animations off there.

| Key           | What it does                              |
|---------------|-------------------------------------------|
| Space / Enter | Throw the dice, play the highlighted move |
| ← → / Tab     | Choose between moves                      |
| 1–9           | Play that move from the list straight away|
| h             | Hint: show which move the Hard AI would play |
| Space         | Skip an animation, or hurry the computer  |
| m             | Toggle sound                              |
| + / -         | Raise / lower sound volume                |
| ?             | Show the rules                            |
| Esc           | Back to the menu                          |
| q / Ctrl+C    | Quit                                      |

When you choose a move, the piece that will move is highlighted in gold
and the square it lands on in green. In the move list, ⚔ marks a capture,
✦ a rosette and ♛ a winning move.

When you ask for a hint, the cursor jumps to the Hard AI's choice, marked
★. Every other move shows how much worse it is, such as −2.5. The scale is
roughly one point per square of progress, so −2.5 is about two and a half
squares worse. Hints are noted in the log.

### Sound

Both the GUI and TUI play short effects for dice, piece hops, movement,
captures, bearing off, rosettes, victory, and menu navigation. Press **M**
to toggle sound and **+ / -** to adjust volume.
The GUI also has a clickable sound button. Use `cargo run -- --mute` (or add
`--mute` to your TUI command) to start silently, including on remote terminals.
Audio plays on the machine running the game.

The embedded WAV files are original synthesized effects, generated by
`python3 tools/generate_sounds.py`. They use deterministic noise and damped
resonators, require no runtime asset downloads, and add no background music.
Sound and volume settings last for the current run.

### Saving

The game saves itself at the start of every turn and whenever the dice are
thrown, so you can quit (or lose your connection) at any point. Next time,
the menu offers **Continue**. The throw is saved before you see it, so
quitting and coming back won't give you a different roll. A finished game
removes the save.

Saves go in `~/.local/share/ur-game` on Linux and
`~/Library/Application Support/ur-game` on macOS.

### Command-line options

Options are passed after `--` when using `cargo run`:

```
cargo run --release -- --light human --dark hard --start
```

| Option               | What it does                                         |
|----------------------|------------------------------------------------------|
| `-l, --light PLAYER` | Who plays Light, who moves first (default: `human`)  |
| `-d, --dark PLAYER`  | Who plays Dark (default: `medium`)                   |
| `-s, --start`        | Skip the menu and start a new game straight away     |
| `-c, --continue`     | Skip the menu and continue the saved game            |
| `--no-save`          | Don't save, and ignore any saved game                |
| `--no-animation`     | Turn off the dice and piece animations               |
| `--mute`             | Start with sound muted; press M to enable it          |
| `--seed NUMBER`      | Replay the same dice throws                          |
| `--tui`              | Play in the terminal instead of a window             |
| `--gui`              | Open a window even over SSH                          |

`PLAYER` is one of `human`, `random`, `easy`, `medium` or `hard`. Set both
sides to computer players to watch them play each other. A continued game
keeps the players it was started with.

## The computer players

- **Random** picks any legal move.
- **Easy** looks one move ahead.
- **Medium** and **Hard** look two and five turns ahead, taking into account
  every possible throw of the dice and how likely each one is. Hard
  remembers positions it has already worked out (the same position often
  comes up through different move orders), so it still answers in a few
  thousandths of a second.

To score a position, the computer counts how far each side's pieces have
travelled, gives a bonus for holding the middle rosette, and subtracts the
risk of each piece being captured.

The dice matter so much in Ur that even a clearly stronger player loses
often. Looking five turns ahead instead of four wins about
55% of games, and looking six ahead gains nothing more.

To have the levels play each other and see the win rates:

```
cargo run --release --example arena 400
```

## Project layout

| File               | What it is                                                |
|--------------------|-----------------------------------------------------------|
| `src/game.rs`      | The rules: board, dice, legal moves, captures, rosettes   |
| `src/ai.rs`        | The computer players                                      |
| `src/app.rs`       | Game flow shared by both versions: menu, turns, input     |
| `src/cli.rs`       | Command-line options                                      |
| `src/sound.rs`     | Shared audio playback and embedded sound effects          |
| `src/save.rs`      | Saving and resuming a game                                |
| `src/gui.rs`       | The window version, drawn with macroquad                  |
| `src/tui.rs`       | The terminal version's main loop                          |
| `src/ui.rs`        | Drawing the terminal screens                              |
| `src/main.rs`      | Picks the window or the terminal and starts the game      |
| `examples/arena.rs`| Computer-vs-computer matches                              |
| `assets/sounds/`   | Original WAV effects used by both interfaces              |
| `tools/generate_sounds.py` | Rebuild the original sound effects                |
| `assets/fonts/`    | Noto Sans, the font used in the window                    |

The rules and computer players are a library with no screen code in them.
The window and terminal versions share the same game flow, so they play
exactly alike.

Run the tests with `cargo test`.

## License

MIT. See [LICENSE](LICENSE).

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
The **Chronicle** records every numbered throw, move, capture, and hint,
sums up the game at the end, and can be shared as a text file.

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
| r             | Throw the dice                            |
| ← → ↑ ↓ / Tab / Shift+Tab | Choose between moves          |
| 1–9           | Play that move from the list straight away|
| h             | Hint: show which move the Hard AI would play |
| Space / Enter | Skip an animation, or hurry the computer  |
| n             | Start a new game once one has finished    |
| e             | Share the Chronicle (file + clipboard)    |
| PgUp/PgDn, [ ] | Scroll the Chronicle back and forward    |
| End           | Jump back to the newest Chronicle entry   |
| m             | Toggle sound                              |
| + / -         | Raise / lower sound volume                |
| ? / F1        | Show the rules                            |
| Esc           | Back to the menu (quits from the menu)    |
| q / Ctrl+C    | Quit                                      |

When you choose a move, the piece that will move is highlighted in gold
and the square it lands on in green. In the move list, ⚔ marks a capture,
✦ a rosette and ♛ a winning move.

A move that leaves its piece where the opponent could hit it shows the
chance of that happening on their next throw, such as ⚠31% ("risk 31%" in
the window). It counts every opponent piece within reach and the odds of
each throw, so it's exact. The middle rosette and your own row are always
safe.

When you ask for a hint, the cursor jumps to the Hard AI's choice, marked
★. Every other move shows how much worse it is, such as −2.5. The scale is
roughly one point per square of progress, so −2.5 is about two and a half
squares worse. Hints are noted in the log.

Each move also gets a short reason, shown for the highlighted move at the
bottom of the move panel (next to the Hint button in the window). The
recommended move says why it's good, such as "captures on 10" or "escapes
63% danger". Moving the cursor to another move says what it gives up
compared with it, such as "leaves 9 at 63% risk", "gives up mid rosette" or
"misses extra throw". When the difference only shows up a few turns later,
it says "weaker a few turns on".

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
Sound, volume, and animation preferences are remembered across launches and
shared by the GUI and TUI. Changes made with the sound keys, sound button,
or animation menu row are saved immediately. The CLI flags `--mute` and
`--no-animation` override preferences for the current run; they do not change
your saved defaults.

### The Chronicle

The Chronicle keeps the whole game. Throws are numbered (both players
counted together), and each entry's bullet takes the colour of the player
it's about. Scroll back with PgUp/PgDn or `[` and `]` (or the mouse wheel
in the window); new entries don't move you while you're reading.

When a game ends, the Chronicle sums it up: how many throws each player
had and their average, captures, rosettes, and any hints used.

Press **e** (or click **Share** in the window) to share the game. The whole
Chronicle, with a short header naming the players and the result, is saved
as a text file and copied to the clipboard. Files go in a `chronicles`
folder beside the saved game, named by date and time (UTC). In the terminal
the copy uses OSC 52, which reaches your own clipboard even over SSH in
terminals that allow it; on a local Mac it uses `pbcopy` as well.

### Leaderboard

**Leaderboard** in the menu shows how you've done against each computer
level: games played, won and lost, and your win percentage. Every finished
game between a human and a computer player counts, by the computer's
level. Games between two humans or two computers don't count. Starting a
new game over an unfinished one against the computer counts that game as a
loss (the menu warns with "New game (forfeits saved)"); a game nobody has
thrown the dice in yet is let go free. The leaderboard is kept in `stats.txt` beside the
saved game, and `--no-save` doesn't affect it.

### Saving

The game saves itself at the start of every turn and whenever the dice are
thrown, so you can quit (or lose your connection) at any point. Next time,
the menu offers **Continue**. The throw is saved before you see it, so
quitting and coming back won't give you a different roll. A finished game
removes the save.

Saves go in `~/.local/share/ur-game` on Linux (or under `$XDG_DATA_HOME`),
`~/Library/Application Support/ur-game` on macOS, and
`%LOCALAPPDATA%\ur-game` on Windows.

### Preferences

Presentation preferences are kept separately from the saved match:

| Platform | File |
|----------|------|
| Linux | `~/.config/ur-game/preferences.txt` (or under `$XDG_CONFIG_HOME`) |
| macOS | `~/Library/Application Support/ur-game/preferences.txt` |
| Windows | `%APPDATA%\ur-game\preferences.txt` |

Missing or invalid preferences fall back to sound on, 50% volume, and
animations on. `--no-save` disables match saving; preferences are still
remembered.

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
| `--seed NUMBER`      | Replay the same dice throws (and Random/Easy choices)|
| `--tui`              | Play in the terminal instead of a window             |
| `--gui`              | Open a window even over SSH                          |

`PLAYER` is one of `human`, `random`, `easy`, `medium` or `hard`. Set both
sides to computer players to watch them play each other. A continued game
keeps the players it was started with.

## The computer players

- **Random** picks any legal move.
- **Easy** looks one move ahead and usually, but not always, plays the
  move it likes best. The worse a move looks, the less likely it is to
  pick it, so it makes the occasional slip a beginner can punish.
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
| `src/preferences.rs` | Loading and saving sound, volume, and animation preferences |
| `src/sound.rs`     | Shared audio playback and embedded sound effects          |
| `src/save.rs`      | Saving and resuming a game                                |
| `src/stats.rs`     | The local leaderboard                                     |
| `src/chronicle.rs` | Sharing the Chronicle as a file and on the clipboard      |
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

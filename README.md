# The ancient Royal Game of Ur

One of the oldest board games known, playable in a window or in your
terminal.

The game was played in Mesopotamia about 4,500 years ago. The best-known
boards were dug up by Sir Leonard Woolley in the Royal Cemetery at Ur (in
modern Iraq) in the 1920s, and one is now in the British Museum. For a long
time nobody knew how it was played. Then Irving Finkel, a curator at the
British Museum, translated a Babylonian clay tablet written by a scribe named
Itti-Marduk-balāṭu in 177 BC that describes the rules. This game follows
Finkel's reconstruction, which is how the game is usually played today, and
also offers the Masters rules (see [Masters rules](#masters-rules)).

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

## Masters rules

James Masters, of Masters Traditional Games, proposed different rules. As
RoyalUr.net explains them: most surviving sets came with three or six dice,
a longer path puts a rosette on every fourth square, and if rosettes were
safe, the ones on your own row (where you can't be captured anyway) would
do nothing. These rules, as played on RoyalUr.net, change four things:

- **A longer path that crosses.** After square 11 your path leaves the
  middle row early and goes round the far end of the board: through the
  *other side's* corner (12–13), the last square of the middle row (14), and
  your own corner (15–16), then off with an exact throw (17). The two routes
  meet head-on there, so a piece is only safe on its first four squares.
- **Rosettes every fourth square** of the path (4, 8, 12, 16), still giving
  another throw.
- **No safe squares.** The middle rosette can be captured on like any other.
- **Three dice instead of four,** and no marks counts as 4, so a throw is
  never wasted:

| Throw | 1   | 2   | 3   | 4                |
|-------|-----|-----|-----|------------------|
| Odds  | 3/8 | 3/8 | 1/8 | 1/8 (no marks)   |

Still seven pieces each, and Light still moves first. Because the routes
cross at the far end, the numbers on those five squares change with whose
turn it is, to match the move list. Press **?** in a game for the rules in
play.

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

In the menu, choose Human or Computer for each side, and the rules: Finkel
or Masters (remembered for next time). The computer has five levels:
Random, Easy, Medium, Hard and Expert. You can also turn the dice and piece
animations off there.

Expert plays perfectly. The first time you choose it under each set of
rules, it works out the whole game: about three minutes on a recent laptop
for Finkel's rules, and about fourteen minutes and 1.3 GB of memory for
Masters', which has nearly four times as many positions. The menu shows how
far along it is, and you can start a game straight away: the Expert waits
for the table before its first throw. The result is saved (276 MB for
Finkel, 1 GB for Masters, beside the saved game), so after that it is ready
in a moment. Once it's saved, it also makes hints and game
reviews exact, whoever you're playing.

| Key           | What it does                              |
|---------------|-------------------------------------------|
| Space / Enter | Throw the dice, play the highlighted move |
| r             | Throw the dice                            |
| ← → ↑ ↓ / Tab / Shift+Tab | Choose between moves          |
| 1–9           | Play that move from the list straight away|
| h             | Hint: show the best move and what the others cost |
| Space / Enter | Skip an animation, or hurry the computer  |
| n             | Start a new game once one has finished    |
| v             | Review your moves once a game has finished |
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

When you ask for a hint, the cursor jumps to the best move, marked ★.
Every other move shows how much worse it is. Once Expert's table has been
worked out (see above), hints are exact: −2.5% means that move lowers your
chance of winning by two and a half percentage points. Until then, the
Hard computer gives them, in points of roughly one square of progress each,
so −2.5 is about two and a half squares worse. Hints are noted in the log.

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

### Reviewing a game

When a game ends, press **v** (or click **Review** in the window) to step
back through every choice you made. Forced moves, where only one move was
possible, are left out. Each one is put back on the board with the dice you
threw, and every option is rated the way a hint does:
**★** marks its pick, the others show how much they give up, and the move
you played is marked **▸** (**»** in the window). Each choice gets a grade:

| Grade      | With Expert's table           | Without it (Hard's points) |
|------------|-------------------------------|----------------------------|
| best       | The best move, or one exactly as good | The move Hard would have played |
| good       | Costs under 0.5% of your winning chance | Under half a point |
| inaccuracy | Costs under 2%                | Under 3 points             |
| mistake    | Costs 2% or more: a missed win or capture, a piece left hanging | 3 points or more |

The two scales are set so they grade about the same share of a beginner's
moves as slips. ← and → step through the
game, Tab and Shift+Tab jump to the next or previous inaccuracy or
mistake, ↑ and ↓ read the reasons for the other options, and Esc goes back
to the finished game. The Chronicle follows along, and gets a one-line
summary such as "Review of Light's 24 choices: 15 best, 5 good, 3
inaccuracies, 1 mistake." Your choices are saved with the game, so a
resumed game can still be reviewed at the end.

### Leaderboard

**Leaderboard** in the menu shows how you've done against each computer
level, and across them all. Finkel and Masters games are kept apart, since
the two play so differently; ← and → (or the button in the window) switch
between them.

| Column  | Means                                                        |
|---------|--------------------------------------------------------------|
| Played, Won, Lost, Win % | Your games against that level              |
| Streak  | Your current run: `W3` is three wins in a row, `L2` two losses |
| Best    | Your longest winning streak                                  |
| Avg     | How many throws a finished game takes, both players counted  |
| Last 10 | Your latest results, oldest first: ● won, ● lost (red), ○ abandoned |

The Total row's streak and latest results run across levels, in the order
you played. When you win, the Chronicle also says how many wins in a row
that makes, and whether it's your best yet.

Every finished game between a human and a computer player counts, by the
computer's level. Games between two humans or two computers don't count.
Starting a new game over an unfinished one against the computer counts that
game as a loss (the menu warns with "New game (forfeits saved)"); a game
nobody has thrown the dice in yet is let go free. Abandoned games count
toward streaks but not toward the average length. The leaderboard is kept
in `stats.txt` beside the saved game, and `--no-save` doesn't affect it.
Older leaderboards carry over, starting their streaks and history from
the next game.

### Saving

The game saves itself at the start of every turn and whenever the dice are
thrown, so you can quit (or lose your connection) at any point. Next time,
the menu offers **Continue**. The throw is saved before you see it, so
quitting and coming back won't give you a different roll. A finished game
removes the save.

Saves go in `~/.local/share/ur-game` on Linux (or under `$XDG_DATA_HOME`),
`~/Library/Application Support/ur-game` on macOS, and
`%LOCALAPPDATA%\ur-game` on Windows. Expert's tables, `expert-finkel.bin`
(276 MB) and `expert-masters.bin` (1 GB), go in the same folder; delete one
to free the space, and it is worked out again the next time you choose
Expert under those rules. (`expert-table.bin`, from earlier versions, is
removed: the new tables are stored differently.)

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
| `-r, --rules RULES`  | `finkel` or `masters`, for this run (default: the menu's choice) |
| `--no-save`          | Don't save, and ignore any saved game                |
| `--no-animation`     | Turn off the dice and piece animations               |
| `--mute`             | Start with sound muted; press M to enable it          |
| `--seed NUMBER`      | Replay the same dice throws (and Random/Easy choices)|
| `--tui`              | Play in the terminal instead of a window             |
| `--gui`              | Open a window even over SSH                          |

`PLAYER` is one of `human`, `random`, `easy`, `medium`, `hard` or `expert`. Set both
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

- **Expert** never guesses. The game has 137,913,936 positions (counting
  only where the pieces are and whose turn it is), few enough to work out
  the exact chance of winning from every one of them. Expert plays the
  move with the best chance. It beats Hard in about 61% of games, and
  with perfect play on both sides, the player who moves first wins 51.5%
  of the time. Masters' rules have 501,032,952 positions, and there the
  first player's edge shrinks to 50.5%.

The solver (`src/solver.rs`) finds the chances by value iteration: it
repeatedly sets each position's chance to the average, over the five
throws, of the best move's result, until nothing changes by more than a
millionth. Captures send pieces back, so positions can repeat and the
answer can't be read off in a single pass. Pieces never leave the finished
pile, though, so it works through the positions in groups by how many
pieces are home, most first, and each group only depends on itself and the
groups already done. Within a group it updates the positions nearest the
end first, which settles a third faster.

To score a position, Easy, Medium and Hard count how far each side's pieces have
travelled, gives a bonus for holding the middle rosette, and subtracts the
risk of each piece being captured.

The dice matter so much in Ur that even a clearly stronger player loses
often. Looking five turns ahead instead of four wins about
55% of games, and looking six ahead gains nothing more.

To have the levels play each other and see the win rates:

```
cargo run --release --example arena 400
cargo run --release --example arena 400 masters
```

## Project layout

| File               | What it is                                                |
|--------------------|-----------------------------------------------------------|
| `src/game.rs`      | The rules, Finkel's and Masters': board, dice, legal moves, captures, rosettes |
| `src/ai.rs`        | The computer players                                      |
| `src/solver.rs`    | Solving the game exactly, for Expert                      |
| `src/expert.rs`    | Loading, or working out and saving, Expert's table        |
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
| `.github/workflows/ci.yml` | Formatting, clippy and tests on every push    |
| `assets/sounds/`   | Original WAV effects used by both interfaces              |
| `tools/generate_sounds.py` | Rebuild the original sound effects                |
| `assets/fonts/`    | Noto Sans, the font used in the window                    |

The rules and computer players are a library with no screen code in them.
The window and terminal versions share the same game flow, so they play
exactly alike.

Run the tests with `cargo test`. Checking the full solves takes a while
(Finkel a few minutes, Masters longer), so they only run when asked:
`cargo test --release -- --ignored full_game`.

## License

MIT. See [LICENSE](LICENSE).

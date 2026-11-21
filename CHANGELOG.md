# Changelog

## 0.2.0 — Unreleased

### Play and learn

- Added Masters rules alongside Finkel: crossing routes, three dice, and no safe rosettes. Choose the rules in the menu or with `--rules`.
- Added Expert AI using solved win probabilities for each ruleset. Verified tables also power exact hints and post-game reviews.
- Added capture-risk indicators and explanations for recommended moves. Easy makes occasional weighted mistakes.
- Added post-game review, including a biggest-mistake shortcut (`B`) and practice mode (`X`). Try a move before revealing its assessment; practice never changes recorded results.
- Added an optional Masters route overlay (`P`) with numbered arrows and entry/exit markers. The compact terminal shows the current player's route; the window and full terminal show both.

### Keep and share

- Added the scrollable, shareable Chronicle, recording throws, moves, captures, hints, and game summaries. Exports save a text file and copy it to the clipboard.
- Added separate leaderboards for Finkel and Masters, with streaks, best streak, average game length, and the last ten results.
- Abandoning a started human-versus-computer match for a new game counts as a loss. Untouched matches are exempt.
- Saved matches retain decisions for later review. Existing Finkel saves and older leaderboard files remain readable.

### Expert reliability

- Cache-save failures are visible in the Chronicle and menu. Expert remains playable from memory; `T` retries saving without solving again.
- Table preparation is serialized across rulesets. Solving uses at most four workers, leaving one CPU free where possible.
- Expert caches now include CRC32 integrity checks. Corrupt or unsupported tables are rejected before their values are used.

### Upgrade notes

- Existing Expert tables have no checksum and require a one-time rebuild. Select Expert under each ruleset to prepare its new cache; hints and reviews use Hard until the table is ready.
- Tables need approximately 276 MB of disk space for Finkel and 1 GB for Masters. Preparing Masters can use about 1.3 GB of memory and take many minutes, depending on the CPU and worker limit.
- Cache replacement uses a temporary file. During a rebuild, allow enough disk space for both the old cache and its replacement.
- Presentation preferences, including the route overlay, remain separate from match saves. CLI presentation overrides stay temporary.

### Development

- Added formatting, Clippy, and tests in CI across Linux, macOS, and Windows.
- Added a manual full-solver verification workflow for either ruleset or both, with sampled residual checks and first-player win probabilities in its logs.

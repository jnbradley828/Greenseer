# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Greenseer is a UCI-compatible chess engine written in Rust. It's built on [oxi_chess_lib](https://github.com/jnbradley828/oxi_chess_lib) (a separate crate, pulled via git dependency), which owns game state, move generation/making/unmaking, and game result detection. This repo owns search, evaluation, and the UCI protocol layer only.

`oxi_chess_lib` is the user's own repo, not a third-party dependency — it can be altered at any time to support Greenseer's needs. If something feels awkward or missing when working against its API, don't just work around it here: it's worth flagging that the lib itself may be the right place to fix it.

If asked what to work on next, check README.md's "Planned Improvements" section first.

The engine runs live on Lichess as `GreenseerEngine`. Every push to `main` triggers `.github/workflows/deploy.yml`, which SSHs into the production VPS, waits for the bot's in-progress game to finish, rebuilds with `cargo build --release`, and restarts it. **Treat `main` as a deploy trigger, not just a branch** — nothing should land there until it's been SPRT-validated.

## Hard rules for Claude in this repo

- **Never commit, and never run or launch an SPRT test.** SPRT runs (`benches/*.zsh`) are compute-heavy, run for hours, and commandeer multiple CPU cores — a human must kick these off and review results manually, then commit manually. Claude may write/edit the code, explain what a benchmark script does, or help interpret results a human already produced, but must not execute `benches/*.zsh` or `git commit`.
- **Always build/run with `--release` for anything performance-sensitive** (diagnostics, node-count comparisons, manual UCI testing, profiling). Debug builds are not representative — search speed (NPS) and depth reached are core things this engine is tuned around, and a debug build's numbers are meaningless for that purpose. Plain `cargo build`/`cargo check` is fine for compile-error iteration only.

## Common commands

```bash
cargo build --release        # build the engine binary (target/release/Greenseer)
cargo build                  # fast debug build, compile-checking only — not for perf work
cargo check                  # fastest correctness check, no binary
cargo test                   # run tests
```

There is no dedicated lint config beyond standard `cargo fmt`/`cargo clippy`. Piece-square tables in `eval_heuristics.rs` use `#[rustfmt::skip]` — don't reformat those blocks.

Manually driving the engine over UCI (after a release build):
```bash
./target/release/Greenseer
uci
isready
position startpos
go depth 6
```

## Architecture

### Module layout
- `src/main.rs` — entry point, just calls `uci::run()`
- `src/uci.rs` — UCI protocol loop: parses stdin commands (`position`, `go`, `setoption`, `stop`, etc.), owns time-management math for `go wtime/btime/winc/binc`, and spawns the search on its own thread so `stop` can interrupt it via a shared `AtomicBool`
- `src/engine/search.rs` — the search itself: `negamax` (alpha-beta with all pruning/reduction heuristics), `TT` (transposition table), `KillerTable`, `RootBest`, move reordering
- `src/engine/eval.rs` — `iteratively_deepen` (the iterative-deepening driver called by `uci.rs`) plus static evaluation (`evaluate`, `relative_evaluate`, `positional_mods`)
- `src/engine/eval_heuristics.rs` — **tunable evaluation weights only**: material values, piece-square tables, king-safety/pawn-structure bonuses. Candidates for future Texel tuning.
- `src/engine/search_heuristics.rs` — **tunable search-algorithm constants only** (pruning margins/depths, time-management fractions). Tuned via SPRT/node-count testing, not Texel tuning — kept in a separate file from `eval_heuristics.rs` specifically because the two are tuned by different methods.
- `src/engine/utils.rs` — shared helpers: TT encode/decode, pawn structure detection (passed/isolated/backward), king-zone masks, move classification (`is_capture`, `move_gives_check`)

### Search design (in `search.rs`)
- **Negamax formulation**: every node's returned score is from the perspective of the side to move *at that node*. A recursive call that makes a real move negates the child score and swaps `(alpha, beta)` to `(-beta, -alpha)`; a call that doesn't change the side to move (check-extension or quiescence dispatch) does neither.
- **`RootBest`** is the incrementally-published "best confirmed answer so far" at the root (ply 0) — updated as each root move completes at each depth, so a `stop` mid-depth still returns a valid move rather than nothing.
- **`completed: bool`** on `negamax`'s return tuple means the subtree wasn't cut short by a stop signal. When `false`, the returned eval/move are meaningless sentinels — callers must propagate the incompletion, never treat it as a real result.
- **Quiescence search** extends leaf nodes through capture sequences (bounded by `MAX_QDEPTH`) to avoid the horizon effect; check-extension searches one ply deeper on check instead of dropping to quiescence.
- **Pruning/reduction implemented**: reverse futility pruning (RFP), null move pruning (NMP), transposition table cutoffs, killer-move heuristic. Constants for all of these live in `search_heuristics.rs`.
- **TT entries** are `(full zobrist key, packed data)` pairs stored as atomics — the full key (not a truncated tag) is kept to avoid false-positive hits when probing.

### Evaluation design (in `eval.rs` / `eval_heuristics.rs`)
- Material + piece-square tables, blended between middlegame and endgame weights via a phase value derived from remaining non-pawn material (`MG_PHASE_SPAN`).
- Additional terms: king safety (attack-unit scoring against the king zone, pawn shield, open/semi-open file danger), pawn structure (passed/isolated/backward/doubled), mobility, bishop pair, rook on open/semi-open file, tempo.
- `relative_evaluate` returns the score from the side-to-move's perspective (what `negamax` needs); `evaluate` is the underlying absolute (white-positive) score.

### Benchmarking / SPRT (`benches/`)
Strength is measured with [fastchess](https://github.com/Disservin/fastchess), running the current `dev` build against a fresh clone+build of `main` under SPRT (elo0=0, elo1=10, alpha=0.05, beta=0.05) until statistical significance is reached, not a fixed game count.

- `full_test_suite.zsh` / `full_test_suite_quick.zsh` / `full_test_suite_alltime.zsh` — run the full battery (middlegame + endgame tournaments); `full_test_suite.zsh e`/`m` controls which runs first
- `vs_main_middlegames.zsh` / `vs_main_endgames.zsh` — full time control (`tc=10+0.1`), opening books from `benches/test_suites/` (e.g. `UHO_Lichess_4852_v1.epd`, `endgames.epd`)
- `*_quick.zsh` variants — fast time control (`tc=1+0.01`, hyperbullet) for rapid iteration, same SPRT bounds. **These are quick sanity checks only** — the user typically cuts them off around 100-200 games and does not run them to full SPRT completion. A pass/fail read here is noise-level, not a merge decision.
- The `tc=10+0.1` runs (`vs_main_middlegames.zsh` / `vs_main_endgames.zsh`) are the ones that must go to full SPRT completion — **only these determine whether a change is validated for `main`**.
- `vs_main_depth4.zsh` — fixed depth=4 (not time-based) comparison
- `vs_main_eigenmann_eg_puzzles*.zsh` — endgame tactical puzzle suite (`EigenmannEndgames.epd`)
- All scripts accept `--resume TIMESTAMP` to continue a previous SPRT run using its saved `benches/tournament_configs/<timestamp>.json` — still rebuilds both binaries, so only valid if no code changed since that run started
- Every run's PGN output is piped through `python/pgn_analysis.py <timestamp>` (invoke with a trailing `q` for quick-variant runs), which parses `nodes`/`nps`/`depth` annotations out of the PGN and writes a dev-vs-main comparison table (avg NPS, avg depth, absolute and percent diff) next to the tournament result
- Results land in `benches/results/` (full runs) or `benches/diagnostics/` (depth4/quick runs), with PGNs and tournament configs kept alongside

`python/` has its own venv (`python/.venv`) with `requirements.txt` (rich, markdown-it-py, Pygments) for `pgn_analysis.py`.

**Again: these SPRT scripts are for a human to run, not Claude** — they hold CPU cores for hours and their result determines whether code merges to `main`.

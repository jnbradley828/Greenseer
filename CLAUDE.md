# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Greenseer is a UCI-compatible chess engine written in Rust. It's built on [oxi_chess_lib](https://github.com/jnbradley828/oxi_chess_lib) (a separate crate, pulled via git dependency), which owns game state, move generation/making/unmaking, and game result detection. This repo owns search, evaluation, and the UCI protocol layer only.

`oxi_chess_lib` is the user's own repo, not a third-party dependency — it can be altered at any time to support Greenseer's needs. If something feels awkward or missing when working against its API, don't just work around it here: it's worth flagging that the lib itself may be the right place to fix it.

If asked what to work on next, check README.md's "Planned Improvements" section first.

The engine runs live on Lichess as `GreenseerEngine`. Every push to `main` triggers `.github/workflows/deploy.yml`, which SSHs into the production VPS, waits for the bot's in-progress game to finish, rebuilds with `cargo build --release`, and restarts it. **Treat `main` as a deploy trigger, not just a branch** — nothing should land there until it's been SPRT-validated.

## Hard rules for Claude in this repo

- **Never commit, and never run or launch an SPRT test.** SPRT runs (`python/benches/fastchess_sprt.py`) are compute-heavy, run for hours, and commandeer multiple CPU cores — a human must kick these off and review results manually, then commit manually. Claude may write/edit the code, explain what the benchmark tool does, or help interpret results a human already produced, but must not execute `python/benches/fastchess_sprt.py` or `git commit`.
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

### Benchmarking / SPRT (`python/benches/fastchess_sprt.py`)
Strength is measured with [fastchess](https://github.com/Disservin/fastchess), running the current `dev` build against a fresh clone+build of `main` under SPRT (elo0=0, elo1=10, alpha=0.05, beta=0.05) until statistical significance is reached, not a fixed game count.

- `python/benches/fastchess_sprt.py` is the single interactive entry point — it replaced the earlier family of `.zsh` scripts (there are no `.zsh` benchmark scripts left in this repo). It builds both binaries, then prompts for each tournament's SPRT bounds, opening suite (from `python/benches/test_suites/` — e.g. `UHO_Lichess_4852_v1.epd`, `endgames.epd`, `EigenmannEndgames.epd`, `8moves_v3.pgn`), concurrency, time control (or a fixed max depth instead), and max rounds. Multiple tournaments can be queued to run back-to-back in one invocation.
- **Quick, low-time-control runs are sanity checks only, not a merge decision** — same principle as before, it's just a prompted time control now rather than a separate `_quick` script. Only a run taken to full SPRT completion at a meaningful time control determines whether a change is validated for `main`.
- While a tournament runs, a live terminal dashboard (Rich `Live`) shows SPRT progress (Elo estimate, LOS, draw/pairs ratio, games/points, Ptnml, LLR vs bounds) alongside engine performance (avg NPS/depth/nodes/time for `dev` vs `main`) — the latter parsed live from the match PGN as fastchess writes it, no separate post-hoc analysis step.
- While running: `'s'` + Enter stops and skips to the next queued tournament, `'p'` + Enter pauses (sends SIGINT, then lets you skip or queue a resume from the saved config), `'r'` + Enter resumes a paused tournament.
- A previous run can be resumed from its saved `python/benches/tournament_configs/<timestamp>.json`.
- Outputs land under `python/benches/`: `results/<timestamp>.txt` (final dashboard snapshot), plus `tournament_logs/`, `tournament_pgns/`, `tournament_stderrs/`, `tournament_stdouts/`, and `tournament_configs/`.

`python/` has its own venv (`python/.venv`) with `requirements.txt` (click, questionary, rich) for `fastchess_sprt.py`.

**Again: this SPRT tool is for a human to run, not Claude** — it holds CPU cores for hours and its result determines whether code merges to `main`.

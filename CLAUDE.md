# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Greenseer is a UCI-compatible chess engine written in Rust. It's built on [oxi_chess_lib](https://github.com/jnbradley828/oxi_chess_lib) (a separate crate, pulled via git dependency), which owns game state, move generation/making/unmaking, and game result detection. This repo owns search, evaluation, and the UCI protocol layer only.

`oxi_chess_lib` is the user's own repo, not a third-party dependency — it can be altered at any time to support Greenseer's needs. If something feels awkward or missing when working against its API, don't just work around it here: it's worth flagging that the lib itself may be the right place to fix it.

If asked what to work on next, check README.md's "Planned Improvements" section first.

The engine runs live on Lichess as `GreenseerEngine`. Every push to `main` triggers `.github/workflows/deploy.yml`, which SSHs into the production VPS, waits for the bot's in-progress game to finish, rebuilds with `cargo build --release`, and restarts it. **Treat `main` as a deploy trigger, not just a branch** — nothing should land there until it's been SPRT-validated.

## Hard rules for Claude in this repo

- **Never commit, and never run or launch an SPRT test.** SPRT runs (`python/benches/fastchess_sprt.py`) are compute-heavy, run for hours, and commandeer multiple CPU cores — a human must kick these off and review results manually, then commit manually. Claude may write/edit the code, explain what the benchmark tool does, or help interpret results a human already produced, but must not execute `python/benches/fastchess_sprt.py` or `git commit`.
- **Never run Texel tuning (`python/tuning/texel.py`).** Runs take hours and overwrite `src/engine/eval_heuristics.toml` in place — a human must launch and review them. Claude may edit the tuning code or help interpret results.
- **Always build/run with `--release` for anything performance-sensitive** (diagnostics, node-count comparisons, manual UCI testing, profiling). Debug builds are not representative — search speed (NPS) and depth reached are core things this engine is tuned around, and a debug build's numbers are meaningless for that purpose. Plain `cargo build`/`cargo check` is fine for compile-error iteration only.

## Common commands

```bash
cargo build --release        # build the engine binary (target/release/Greenseer)
cargo build                  # fast debug build, compile-checking only — not for perf work
cargo check                  # fastest correctness check, no binary
cargo test                   # run tests
cargo build --release --features tuning --bin texel   # Texel eval binary (target/release/texel), needed by python/tuning/texel.py
```

There is no dedicated lint config beyond standard `cargo fmt`/`cargo clippy`. `src/engine/eval_heuristics.toml` is rewritten wholesale by `python/tuning/texel.py` on every tuning epoch — don't hand-format it, and expect hand edits to its values to be overwritten by the next tuning run.

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
- `src/lib.rs` — exposes `engine` as a library so `src/bin/texel.rs` can call `evaluate` directly
- `src/uci.rs` — UCI protocol loop: parses stdin commands (`position`, `go`, `setoption`, `stop`, `ponderhit`, etc.), owns time-management math for `go wtime/btime/winc/binc`, and spawns the search on its own thread(s) so `stop` can interrupt it via a shared `AtomicBool`. UCI options: `Hash`, `Threads` (Lazy SMP — `spawn_lazy_smp` runs N searches sharing the TT and takes the deepest result), `Ponder`.
- `src/engine/search.rs` — the search itself: `negamax` (alpha-beta with all pruning/reduction heuristics), `TT` (transposition table), `KillerTable`, `RootBest`, `SearchState` (stop/ponder flags), move reordering, SEE (`capture_gains`/`unfold_gains`, `good_captures_only`)
- `src/engine/eval.rs` — `iteratively_deepen` (the iterative-deepening driver called by `uci.rs`) plus static evaluation (`evaluate`, `relative_evaluate`, `positional_mods`)
- `src/engine/params.rs` — `EvalParams` / `SearchParams` structs and the `ep()` / `sp()` accessors every tunable value is read through. `PAWN_VALUE` (100) is a fixed const, not tunable — all eval terms are in centipawns relative to it.
- `src/engine/eval_heuristics.toml` — **tunable evaluation weights only**: material values, piece-square tables, king-safety/pawn-structure bonuses. Tuned by Texel tuning (`python/tuning/texel.py`).
- `src/engine/search_heuristics.toml` — **tunable search-algorithm constants only** (pruning margins/depths, time-management fractions, TT age factor). Tuned via SPRT/node-count testing, not Texel tuning — kept in a separate file from `eval_heuristics.toml` specifically because the two are tuned by different methods.
- `build.rs` — reads both TOMLs and generates `params_const.rs` in `OUT_DIR`, so a normal build bakes every param in as a `const` (no runtime lookup cost). `rerun-if-changed` on both TOMLs, so editing one triggers a rebuild.
- `src/bin/texel.rs` — tuning-only binary (`required-features = ["tuning"]`): reads FENs on stdin, prints `evaluate` (white-relative cp) per line.
- `src/engine/utils.rs` — shared helpers: TT encode/decode and replacement scoring, pawn structure detection (passed/isolated/backward), king-zone masks and pawn shield/file weakness, move classification (`is_capture`, `move_gives_check`)

### Parameters: baked vs. tuning builds (`params.rs`)
- **Default build** (`#[cfg(not(feature = "tuning"))]`): `ep()`/`sp()` return the `build.rs`-generated consts. This is what ships and what SPRT tests.
- **`--features tuning` build**: params are parsed from the TOMLs at startup into `OnceLock`s (~5% NPS cost, so never ship or benchmark this build). `--params_path <absolute dir>` points it at a folder containing `eval_heuristics.toml` + `search_heuristics.toml` instead of `src/engine/` — this is how `texel.py` evaluates perturbed param sets from a temp dir.
- Adding a param means adding the field to the struct in `params.rs` **and** a `[name]` block (`value`/`step`/`min`/`max`) to the matching TOML — `build.rs` emits every TOML key as a struct field, so the two must match exactly.

### Search design (in `search.rs`)
- **Negamax formulation**: every node's returned score is from the perspective of the side to move *at that node*. A recursive call that makes a real move negates the child score and swaps `(alpha, beta)` to `(-beta, -alpha)`; a call that doesn't change the side to move (check-extension or quiescence dispatch) does neither.
- **`RootBest`** is the incrementally-published "best confirmed answer so far" at the root (ply 0) — updated as each root move completes at each depth, so a `stop` mid-depth still returns a valid move rather than nothing.
- **`completed: bool`** on `negamax`'s return tuple means the subtree wasn't cut short by a stop signal. When `false`, the returned eval/move are meaningless sentinels — callers must propagate the incompletion, never treat it as a real result.
- **Quiescence search** extends leaf nodes through capture sequences (bounded by `max_qdepth`), searching only SEE ≥ 0 captures (`good_captures_only`); check-extension searches one ply deeper on check instead of dropping to quiescence.
- **Move ordering** (`reorder_moves`): TT move, then SEE ≥ 0 captures (sorted by SEE), then killers, then quiets, then SEE < 0 captures last.
- **Pruning/reduction implemented**: reverse futility pruning (RFP), null move pruning (NMP), transposition table cutoffs, killer-move heuristic, principal variation search (PVS — null-window probe on non-first moves, full-window re-search only on fail-high), SEE pruning in qsearch. Not yet implemented: LMR, LMP, futility pruning, aspiration windows, history heuristic. Constants for the pruning margins/depths live in `search_heuristics.toml`.
- **TT entries** are `(full zobrist key, packed data)` pairs stored as atomics — the full key (not a truncated tag) is kept to avoid false-positive hits when probing.

### Evaluation design (in `eval.rs` / `eval_heuristics.toml`)
- Material + piece-square tables, blended between middlegame and endgame weights via a phase value derived from remaining non-pawn material (`mg_phase_span`).
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

### Texel tuning (`python/tuning/`)
- `texel.py` tunes `eval_heuristics.toml` against Stockfish evals from a random shard of the Lichess `chess-position-evaluations` HuggingFace dataset (deepest eval per FEN, mate scores dropped). Positions are filtered to quiet ones: not in check, no legal captures, no chess960 castling rights. Sample size is `DESIRED_SAMPLE_SIZE`.
- Loss is MSE between `sigmoid(cp * K)` of the Stockfish label and of the engine eval (`K = 1/400`).
- Optimizer is coordinate descent: each epoch perturbs every flattened param by ±1 (±0.01 for floats, `helper.STEP_SIZES`), keeps any improvement, and writes the TOML back to `src/engine/` after each epoch. Runs until an epoch changes nothing. The TOML's per-param `step`/`min`/`max` fields are loaded by `helper.flatten_params` but not currently used by the optimizer — bounds are not enforced.
- Evaluation goes through `target/release/texel` (must be built with `--features tuning` first), fed via `--params_path` to a temp dir per perturbation.
- `helper.py` handles TOML ↔ flat numpy array conversion.
- Each run draws a fresh random shard, so a resumed/restarted run tunes on different positions.

### Python tooling
`python/` is a uv project (`pyproject.toml` / `uv.lock`; deps: click, numpy, polars, python-chess, questionary, rich, tomli-w) shared by `benches/` and `tuning/`.

**Again: this SPRT tool is for a human to run, not Claude** — it holds CPU cores for hours and its result determines whether code merges to `main`.

# Greenseer

A UCI-compatible chess engine written in Rust, named after the prophetic wargs of *A Song of Ice and Fire* — seers who look forward and backward through time, much like a search tree.

Built on top of [oxi_chess_lib](https://github.com/jnbradley828/oxi_chess_lib), which handles game state, move generation, move making/unmaking, and game result detection.

---

## Play Against Greenseer

Greenseer is live on Lichess — you can challenge it directly:

**[GreenseerEngine on Lichess](https://lichess.org/@/GreenseerEngine)**

The bot runs 24/7 on a self-managed [DigitalOcean](https://www.digitalocean.com/) VPS. Deployment is fully automated via a GitHub Actions CD pipeline that triggers on every push to `main`, waits for any in-progress game to finish, rebuilds the engine binary, and restarts the bot — no manual intervention required.

---

## How It Works

### Search: Minimax with Alpha-Beta Pruning
Greenseer uses **minimax** to explore the game tree, maximizing the engine's score and minimizing the opponent's at alternating depths. **Alpha-beta pruning** cuts branches that can't affect the final result, dramatically reducing the number of nodes evaluated.

### Iterative Deepening
Rather than searching directly to a fixed depth, Greenseer uses **iterative deepening** — completing full searches at depth 1, 2, 3, ... up to the target depth. The best move found at each depth is used to **reorder moves** at the next iteration, placing the most promising candidate first. The best move is also **published incrementally** as each root move completes, so an interrupted search (via `stop` or time running out mid-depth) always returns a valid, up-to-date answer rather than nothing.

### Move Ordering
Legal moves at each node are ordered to maximize alpha-beta cutoffs: the transposition table's stored best move first, then captures, then checks, then **killer moves** (quiet moves that caused a beta cutoff at the same ply in a sibling node), then everything else.

### Principal Variation Search (PVS)
Move ordering means the first move searched at each node is usually the best one. Greenseer exploits this with **PVS**: after searching the first move with a full alpha-beta window, every subsequent move is first probed with a cheap **null window** (`[alpha, alpha+1]`) just to test whether it can beat alpha at all. Most of the time it can't, and the probe alone is enough to prove the move is worse — no full-width search needed. Only if a probe unexpectedly fails high is that move re-searched with the full window to get its real score.

### Search Pruning
Beyond alpha-beta, Greenseer prunes with:
- **Reverse futility pruning (RFP)** — at shallow depths, if the static evaluation already exceeds beta by a safe margin, the node is cut without searching further.
- **Null move pruning (NMP)** — if skipping a move entirely ("passing") still produces a position good enough to fail high after a reduced-depth search, the real subtree is pruned, since an actual move should only do better.

### Quiescence Search
To avoid evaluating positions mid-capture (the "horizon effect"), Greenseer extends the search at leaf nodes with a **quiescence search** — continuing to explore capture sequences until a quiet position is reached before applying the evaluation function. This prevents the engine from making short-sighted decisions based on incomplete tactical sequences.

### Transposition Table
Greenseer uses a **transposition table** with 64-bit **Zobrist hashing** (eliminating key collisions) to cache previously evaluated positions. Since the same position can be reached via different move orders, the table allows the engine to skip redundant work and retrieve scores for positions it has already analyzed. Entries are tagged with an **age**, so stale results from earlier in the game can be preferentially overwritten.

### Evaluation
The position evaluator combines **material balance** (pawn=1, knight/bishop=3, rook=5, queen=9) with **piece-square tables** — per-piece bonus/penalty grids that reward good squares (e.g. centralized knights, advanced passed pawns) and penalize poor ones. Separate middlegame and endgame weights blend based on remaining material. On top of that, the evaluator scores:
- **King safety** — attacker-weighted pressure on the king zone, pawn shield quality, and open/semi-open file danger near the king
- **Pawn structure** — passed, isolated, backward, and doubled pawn terms
- **Mobility** — legal move count per piece type
- **Bishop pair, rook on open/semi-open file, and tempo bonuses**

Terminal states (checkmate, draw) are handled explicitly. The addition of piece-square tables alone measured a **+478 Elo gain** in self-play benchmarking.

### UCI Compatibility
The engine speaks UCI, including support for `go depth`, `go movetime`, and full time control (`wtime`/`btime`/`winc`/`binc`). A shared atomic flag allows the search to abort mid-tree on a `stop` command and return the best move found so far. Each search reports standard `info` lines with `depth`, `score cp`, `nodes`, `nps`, and `time`. Time management is tuned down to hyperbullet time controls, and no longer artificially caps the first few moves of a game, since [lichess-bot](https://github.com/lichess-bot-devs/lichess-bot) is configured to play from an opening book.

### Benchmarking
Engine strength is measured using [fastchess](https://github.com/Disservin/fastchess) to run automated matches between the current `dev` build and a fresh clone+build of `main`. An interactive Python tool (`python/benches/fastchess_sprt.py`) drives the whole process — building both binaries, then configuring and queuing one or more tournaments (opening book, time control or a fixed depth instead, concurrency, rounds) under **SPRT** (Sequential Probability Ratio Test) with elo0=0, elo1=10, alpha=0.05, beta=0.05. Rather than a fixed game count, each test runs until it reaches statistical certainty that the new version is better or no better than the baseline. A live terminal dashboard tracks the running SPRT statistics and parses the match PGN as it's written, reporting average nodes-per-second and search depth for `dev` vs. `main` in real time, so raw Elo results can be cross-checked against actual search performance as the match runs. Long-running tournaments can be paused, stopped, or resumed from a saved config without losing accumulated SPRT statistics.

---

## Planned Improvements

- **Texel tuning** — fit evaluation weights via machine learning instead of hand-tuning
- **Neural network experimentation** — explore learned evaluation functions as an alternative to hand-crafted heuristics
- **More positional evaluation terms** — connected/protected passed pawns, knight outposts, connected rooks, piece batteries (e.g. queen/rook stacked on a file — "Alekhine's gun"), and space
- **More search pruning/reduction techniques** — late move reductions (LMR), late move pruning (LMP), futility pruning, aspiration windows, history heuristic, static exchange evaluation (SEE), internal iterative reduction (IIR), and singular extensions

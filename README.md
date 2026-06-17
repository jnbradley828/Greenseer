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
Rather than searching directly to a fixed depth, Greenseer uses **iterative deepening** — completing full searches at depth 1, 2, 3, ... up to the target depth. The best move found at each depth is used to **reorder moves** at the next iteration, placing the most promising candidate first. This improves alpha-beta efficiency significantly.

### Quiescence Search
To avoid evaluating positions mid-capture (the "horizon effect"), Greenseer extends the search at leaf nodes with a **quiescence search** — continuing to explore capture sequences until a quiet position is reached before applying the evaluation function. This prevents the engine from making short-sighted decisions based on incomplete tactical sequences.

### Transposition Table
Greenseer uses a **transposition table** with **Zobrist hashing** to cache previously evaluated positions. Since the same position can be reached via different move orders, the table allows the engine to skip redundant work and retrieve scores for positions it has already analyzed, improving search efficiency significantly.

### Evaluation
The position evaluator combines **material balance** (pawn=1, knight/bishop=3, rook=5, queen=9) with **piece-square tables** — per-piece bonus/penalty grids that reward good squares (e.g. centralized knights, advanced passed pawns) and penalize poor ones. Separate middlegame and endgame weights blend based on remaining material. Terminal states (checkmate, draw) are handled explicitly. The addition of piece-square tables measured a **+478 Elo gain** in self-play benchmarking.

### UCI Compatibility
The engine speaks UCI, including support for `go depth`, `go movetime`, and full time control (`wtime`/`btime`/`winc`/`binc`). A shared atomic flag allows the search to abort mid-tree on a `stop` command and return the best move found so far. Each search reports standard `info` lines with `depth`, `score cp`, `nodes`, `nps`, and `time`.

### Benchmarking
Engine strength is measured using [fastchess](https://github.com/Disservin/fastchess) to run automated matches between the current `dev` build and the previous `main` build. A Bash script automates the process — building both binaries and running the match under **SPRT** (Sequential Probability Ratio Test) with elo0=0, elo1=10, alpha=0.05, beta=0.05. Rather than a fixed game count, the test runs until it reaches statistical certainty that the new version is better or no better than the baseline, then reports the estimated Elo delta before merging.

---

## Planned Improvements

- **Deeper evaluation** — king safety, pawn structure, and mobility terms; weights tuned via machine learning (Texel tuning)
- **Neural network experimentation** — explore learned evaluation functions as an alternative to hand-crafted heuristics

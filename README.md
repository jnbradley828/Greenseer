# Greenseer

A UCI-compatible chess engine written in Rust, named after the prophetic wargs of *A Song of Ice and Fire* — seers who look forward and backward through time, much like a search tree.

Built on top of [oxi_chess_lib](https://github.com/jnbradley828/oxi_chess_lib), which handles game state, move generation, move making/unmaking, and game result detection.

---

## Play Against Greenseer

Greenseer is live on Lichess — you can challenge it directly:

**[GreenseerEngine on Lichess](https://lichess.org/@/GreenseerEngine)**

The bot is hosted on [DigitalOcean](https://www.digitalocean.com/).

---

## How It Works

### Search: Minimax with Alpha-Beta Pruning
Greenseer uses **minimax** to explore the game tree, maximizing the engine's score and minimizing the opponent's at alternating depths. **Alpha-beta pruning** cuts branches that can't affect the final result, dramatically reducing the number of nodes evaluated.

### Iterative Deepening
Rather than searching directly to a fixed depth, Greenseer uses **iterative deepening** — completing full searches at depth 1, 2, 3, ... up to the target depth. The best move found at each depth is used to **reorder moves** at the next iteration, placing the most promising candidate first. This improves alpha-beta efficiency significantly.

### Evaluation
The position evaluator combines **material balance** (pawn=1, knight/bishop=3, rook=5, queen=9) with **piece-square tables** — per-piece bonus/penalty grids that reward good squares (e.g. centralized knights, advanced passed pawns) and penalize poor ones. Terminal states (checkmate, draw) are handled explicitly. The addition of piece-square tables measured a **+478 Elo gain** in self-play benchmarking.

### UCI Compatibility
The engine speaks UCI, including support for `go depth`, `go movetime`, and full time control (`wtime`/`btime`/`winc`/`binc`). A shared atomic flag allows the search to abort mid-tree on a `stop` command and return the best move found so far. Each search reports standard `info` lines with `depth`, `score cp`, `nodes`, `nps`, and `time`.

### Benchmarking
Engine strength is measured using [fastchess](https://github.com/Disservin/fastchess) to run automated 100-game matches between the current `dev` build and the previous `main` build. This makes it straightforward to quantify Elo deltas before merging improvements.

---

## Planned Improvements

- **Deeper evaluation** — king safety, pawn structure, and mobility terms; weights tuned via linear regression on a game database
- **Neural network experimentation** — explore learned evaluation functions as an alternative to hand-crafted heuristics

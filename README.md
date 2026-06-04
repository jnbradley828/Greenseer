# Greenseer

A UCI-compatible chess engine written in Rust, named after the prophetic wargs of *A Song of Ice and Fire* — seers who look forward and backward through time, much like a search tree.

Built on top of [oxi_chess_lib](https://github.com/jnbradley828/oxi_chess_lib), which handles game state, move generation, move making/unmaking, and game result detection.

---

## How It Works

### Search: Minimax with Alpha-Beta Pruning
Greenseer uses **minimax** to explore the game tree, maximizing the engine's score and minimizing the opponent's at alternating depths. **Alpha-beta pruning** cuts branches that can't affect the final result, dramatically reducing the number of nodes evaluated.

### Iterative Deepening
Rather than searching directly to a fixed depth, Greenseer uses **iterative deepening** — completing full searches at depth 1, 2, 3, ... up to the target depth. The best move found at each depth is used to **reorder moves** at the next iteration, placing the most promising candidate first. This improves alpha-beta efficiency significantly.

### Evaluation
The current position evaluator uses **material balance** (pawn=1, knight/bishop=3, rook=5, queen=9), with terminal states (checkmate, draw) handled explicitly.

### UCI Compatibility
The engine speaks UCI, including support for the `stop` command — a shared atomic flag lets the search abort mid-tree and return the best move found so far.

---

## Demo

A playable demo is in the works — stay tuned.

---

## Planned Improvements

- **Better evaluation function** — add positional parameters (piece-square tables, king safety, pawn structure) with weights tuned via linear regression on a game database
- **Clock management** — time-aware search that allocates compute based on remaining clock
- **Neural network experimentation** — explore learned evaluation functions as an alternative to hand-crafted heuristics

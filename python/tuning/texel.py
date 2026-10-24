import helper
import math
import polars as pl
import random
from pathlib import Path
import chess

ENGINE_DIR = Path(__file__).resolve().parent.parent.parent / "src" / "engine"

def sigmoid(col: pl.Expr) -> pl.Expr:
    return 1 / (1 + (-col).exp())

# Imports lichess's chess-positions-evaluations dataset from
# https://huggingface.co/datasets/Lichess/chess-position-evaluations
# Contains over 400 million positions evaluated by Stockfish (best engine in the world currently)
# We select a random shard of 20 million and try to select roughly 1 million positions to use.
# The same position has multiple evaluations, so we must first discard all rows of each position
# except for the highest depth evaluation.
# Then, a random 3.5 million positions are selected and after filtering only for quiet positions
# (not check, no captures available), we should get roughly 1 million unique positions since roughly 30% of positions
# match this requirement!
# The randomness is to ensure parameters are not overfit and allow for each training run to have different results,
# that may or may not improve the engine.

def import_positions_dataset() -> pl.DataFrame:
    shard_id = f'{random.randint(0, 19):04d}'
    dataset_url = f"hf://datasets/Lichess/chess-position-evaluations/data/data_{shard_id}.parquet"
    print(f"Loading random dataset from \"{dataset_url}\"...")

    df = (
        pl.scan_parquet(dataset_url)
        .drop_nulls("cp")   # null cp means the position has a checkmate score
        .sort("depth", descending = True)   # take only the copy of duplicate position with highest depth
        .unique("fen", keep="first")    # ^^
        .select("fen", "cp")
        .collect()
        .sample(6_000_000)
        .filter(pl.col("fen").map_elements(_is_quiet, return_dtype=pl.Boolean))
    )

    print("Dataset loaded.")
    return df

def _is_quiet(fen: str) -> bool:
    board = chess.Board(fen)
    return not board.is_check() and not any(board.is_capture(m) for m in board.legal_moves)

def main():
    df = import_positions_dataset()
    print(df.height)
    print(df.head(10))

if __name__ == "__main__":
    main()

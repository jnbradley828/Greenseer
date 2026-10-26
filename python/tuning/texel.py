import helper
import polars as pl
import random
from pathlib import Path
import chess
import subprocess
import numpy as np
import tempfile
import shutil
import os

# This module runs coordinate descent on sigmoid-MSE loss over evaluation parameters based on "Texel's Tuning Method".

ENGINE_DIR = Path(__file__).resolve().parent.parent.parent / "src" / "engine"
# this script calls Greenseer's texel binary directly to retrieve evaluations.
ENGINE_TEXEL_BIN = Path(__file__).resolve().parent.parent.parent / "target" / "release" / "texel"

def sigmoid(col: pl.Expr) -> pl.Expr:
    return 1 / (1 + (-col).exp())

# Loads Stockfish-evaluated positions from a random shard of
# https://huggingface.co/datasets/Lichess/chess-position-evaluations
# Keeps the deepest eval per position, oversamples, then filters to quiet, usable positions
# (not in check, no captures available, no chess960 castling rights).

DESIRED_SAMPLE_SIZE = 200_000
QUIET_POSITION_RATE = 0.16
OVERSAMPLE = round(DESIRED_SAMPLE_SIZE / QUIET_POSITION_RATE)
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
        .sample(OVERSAMPLE)
        .filter(pl.col("fen").map_elements(_is_usable, return_dtype=pl.Boolean))
    )

    print(f"Dataset of sample size {df.height} loaded.")
    return df

def _is_usable(fen: str) -> bool:
    board = chess.Board(fen)
    return (
        not board.is_check()
        and not any(board.is_capture(m) for m in board.legal_moves)
        and not board.has_chess960_castling_rights()
    )

EVAL_FILE_NAME = "eval_heuristics.toml"
def evaluate_positions(binary_path: Path, params: Path, dataset: pl.DataFrame) -> pl.DataFrame:
    texel_rs = subprocess.Popen(
        [binary_path, "--params_path", params],
        stdin = subprocess.PIPE,
        stdout = subprocess.PIPE,
        text = True,
    )

    positions = "\n".join(dataset["fen"])
    output, _ = texel_rs.communicate(positions)
    cp_engine = [int(line) for line in output.splitlines()]
    return dataset.with_columns(pl.Series("cp_engine", cp_engine))

# scaling factor K determines what eval ranges count as "big":
# sigmoid(cp/400) for cp = 0 is 0.5, for cp = 800 is 0.88, for cp = 2000 is 0.99
K = 1/400
def normalize_evals(dataset: pl.DataFrame) -> pl.DataFrame:
    cp_norm = sigmoid(pl.col("cp") * K)
    cp_engine_norm = sigmoid(pl.col("cp_engine") * K)
    dataset = dataset.with_columns(
        cp_norm = cp_norm,
        cp_engine_norm = cp_engine_norm,
    )
    return dataset

def calculate_losses(dataset: pl.DataFrame) -> pl.DataFrame:
    # uses squared difference as individual loss function
    losses = (pl.col("cp_norm") - pl.col("cp_engine_norm")) ** 2
    dataset = dataset.with_columns(losses = losses)
    return dataset

def calculate_loss(dataset: pl.DataFrame) -> float:
    # uses MSE (mean squared error)
    loss = dataset["losses"].mean()
    return loss

def _one_epoch(initial_loss: float, dataset: pl.DataFrame, binary_path: Path, params_dir_path: Path) -> tuple[float, bool]:
    loss_final = initial_loss

    # import param values - fp is "flattened params".
    ep = helper.load_eval_params(params_dir_path)
    fp = helper.flatten_params(ep)[0]
    fp_final = fp.copy()

    # perturb each parameter by +- 1, test loss, and keep best option.
    with tempfile.TemporaryDirectory() as d:
        temp_params_dir = Path(d)
        shutil.copy(params_dir_path / "search_heuristics.toml", temp_params_dir)

        for i in range(len(fp)):
            fp_minus, fp_plus = helper.perturb_one_parameter(fp_final, i)
            for param_set in [fp_minus, fp_plus]:
                ufp = helper.unflatten_params(ep, param_set)
                helper.save_params(ufp, temp_params_dir / EVAL_FILE_NAME)
                df1 = evaluate_positions(binary_path, temp_params_dir, dataset)
                df1 = normalize_evals(df1)
                df1 = calculate_losses(df1)
                loss1 = calculate_loss(df1)
                if loss1 < loss_final:
                    fp_final = param_set
                    loss_final = loss1

    # save new parameters!
    ufp = helper.unflatten_params(ep, fp_final)
    output_file = params_dir_path / EVAL_FILE_NAME
    helper.save_params(ufp, output_file)

    return (loss_final, np.array_equal(fp_final, fp))

def tuning_run(binary_path: Path, params_dir_path: Path) -> tuple[float, float]:
    dataset = import_positions_dataset()
    df = evaluate_positions(binary_path, params_dir_path, dataset)
    df = normalize_evals(df)
    df = calculate_losses(df)
    initial_loss = calculate_loss(df)
    last_loss = initial_loss
    final_loss = initial_loss

    iterations = 0
    convergence = False
    while not convergence:
        iterations += 1
        print(f'Current iteration: {iterations}')

        final_loss, convergence = _one_epoch(final_loss, dataset, binary_path, params_dir_path)
        pct_improvement_epoch = (last_loss - final_loss) / last_loss * 100
        last_loss = final_loss
        pct_improvement_total = (initial_loss - final_loss) / initial_loss * 100
        print(f"Loss impoved by {pct_improvement_epoch:.2f}% this epoch.")
        print(f"Loss improved by {pct_improvement_total:.2f}% total.")

    return (initial_loss, final_loss)

def main():
    # keep the mac awake until this script exits.
    subprocess.Popen(["caffeinate", "-s", "-i", "-m", "-w", str(os.getpid())])

    initial_loss, final_loss = tuning_run(ENGINE_TEXEL_BIN, ENGINE_DIR)
    print("Tuning run complete!")
    print(f"Initial loss: {initial_loss}")
    print(f"Final loss: {final_loss}")

if __name__ == "__main__":
    main()

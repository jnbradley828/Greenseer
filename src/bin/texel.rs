use Greenseer::engine::{eval::evaluate, params::initialize_params};
use oxi_chess_lib::board::ChessBoard;
use std::io::{self, BufRead, Write};

fn main() {
    initialize_params();
    let mut out = io::BufWriter::new(io::stdout().lock());

    for line in io::stdin().lock().lines() {
        let fen = format!("{} 0 1", line.unwrap());
        let board = ChessBoard::initialize_from_fen(&fen).unwrap_or_else(|e| panic!("{e}: {fen}"));
        let cp_eval = evaluate(&board);
        writeln!(out, "{}", cp_eval).unwrap();
    }
}

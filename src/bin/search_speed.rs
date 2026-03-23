use Greenseer::engine;
use oxi_chess_lib::game::ChessGame;
use std::fs::OpenOptions;
use std::io::Write;
use std::time::{Duration, Instant};

fn main() {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open("benchmarks/results/search_speed_results.txt")
        .unwrap();

    for depth in 1..=5 {
        let mut total_elapsed: Duration = Duration::ZERO;

        let rounds = 5;
        for i in 1..=rounds {
            let start = Instant::now();
            let mut game = ChessGame::initialize((1, 1), None);
            engine::eval::best_move(&mut game, depth);
            let elapsed = start.elapsed();
            total_elapsed += elapsed;
        }
        let avg_elapsed = total_elapsed / rounds;

        writeln!(
            file,
            "depth {}: best move found in {:?}",
            depth, avg_elapsed
        )
        .unwrap();
    }
}

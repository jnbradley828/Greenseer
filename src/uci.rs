use crate::engine::eval::iteratively_deepen;
use crate::engine::search::SearchState;
use oxi_chess_lib::board::ChessBoard;
use oxi_chess_lib::game::ChessGame;
use oxi_chess_lib::utils::decode_to_uci;
use std::cmp::min;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::{
    io::{self, BufRead},
    sync::Arc,
    thread,
};

pub fn run() {
    let mut game = ChessGame::initialize((1, 1), None);
    let state = Arc::new(SearchState::new());
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = line.unwrap();
        handle_command(&line, &mut game, Arc::clone(&state));
    }
}

pub fn handle_command(cmd: &str, game: &mut ChessGame, state: Arc<SearchState>) {
    let parts: Vec<&str> = cmd.trim().split_whitespace().collect();
    if parts.is_empty() {
        return;
    }
    match parts[0] {
        "uci" => uci_response(),
        "isready" => println!("readyok"),
        "ucinewgame" => *game = ChessGame::initialize((1, 1), None),
        "position" => handle_position(&parts, game),
        "go" => handle_go(&parts, game, Arc::clone(&state)),
        "quit" => std::process::exit(0),
        "stop" => handle_stop(Arc::clone(&state)),
        _ => {}
    }
}

fn uci_response() {
    println!("id name Greenseer 0.1");
    println!("id author Joshua Bradley");
    println!("uciok");
}

fn handle_position(parts: &[&str], game: &mut ChessGame) {
    // parts[0] == "position"
    // parts[1] == "startpos" or "fen"
    if parts.len() < 2 {
        return;
    }

    let moves_idx = match parts[1] {
        "startpos" => {
            *game = ChessGame::initialize((1, 1), None);
            parts.iter().position(|&p| p == "moves").map(|i| i + 1)
        }
        "fen" => {
            // FEN is parts[2..7] (6 fields), then optionally "moves"
            let fen = parts[2..parts.len().min(8)].join(" ");
            *game = ChessGame::initialize((1, 1), Some(&fen));
            parts.iter().position(|&p| p == "moves").map(|i| i + 1)
        }
        _ => return,
    };

    if let Some(start) = moves_idx {
        for uci_move in &parts[start..] {
            let _ = game.make_move_from_uci(uci_move);
        }
    }
}

fn handle_go(parts: &[&str], game: &mut ChessGame, state: Arc<SearchState>) {
    state.stop.store(false, Ordering::Relaxed);
    let depth = parts
        .iter()
        .position(|&p| p == "depth")
        .and_then(|i| parts.get(i + 1))
        .and_then(|d| d.parse::<u8>().ok());

    let movetime = parts
        .iter()
        .position(|&p| p == "movetime")
        .and_then(|i| parts.get(i + 1))
        .and_then(|d| d.parse::<u32>().ok());

    let wtime = parts
        .iter()
        .position(|&p| p == "wtime")
        .and_then(|i| parts.get(i + 1))
        .and_then(|d| d.parse::<u32>().ok());
    let btime = parts
        .iter()
        .position(|&p| p == "btime")
        .and_then(|i| parts.get(i + 1))
        .and_then(|d| d.parse::<u32>().ok());
    let winc = parts
        .iter()
        .position(|&p| p == "winc")
        .and_then(|i| parts.get(i + 1))
        .and_then(|d| d.parse::<u32>().ok());
    let binc = parts
        .iter()
        .position(|&p| p == "binc")
        .and_then(|i| parts.get(i + 1))
        .and_then(|d| d.parse::<u32>().ok());

    match (depth, movetime, wtime, btime, winc, binc) {
        (_, Some(mt), _, _, _, _) => {
            let cancel = Arc::new(AtomicBool::new(false));
            let cancel_c = Arc::clone(&cancel);

            let state_c = Arc::clone(&state);
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(mt as u64));
                if !cancel_c.load(Ordering::Relaxed) {
                    state_c.stop.store(true, Ordering::Relaxed);
                }
            });

            let mut game_clone = game.clone();
            thread::spawn(move || {
                let _ = iteratively_deepen(&mut game_clone, u8::MAX, Arc::clone(&state));
                cancel.store(true, Ordering::Relaxed);
                let uci_move = decode_to_uci(state.best_move.load(Ordering::Relaxed)).unwrap();
                println!("bestmove {}", uci_move);
            });
        }
        (Some(d), _, None, None, None, None) => {
            let mut game_clone = game.clone();
            thread::spawn(move || {
                let _ = iteratively_deepen(&mut game_clone, depth.unwrap(), Arc::clone(&state));
                let uci_move = decode_to_uci(state.best_move.load(Ordering::Relaxed)).unwrap();
                println!("bestmove {}", uci_move);
            });
        }
        (_, _, Some(wt), Some(bt), _, _) => {
            let (time, inc) = if game.board.side_to_move {
                (wt, winc.unwrap_or(0))
            } else {
                (bt, binc.unwrap_or(0))
            };

            let mut mt = (time / 20) + (inc / 2);

            // cap thinking at 3 seconds if this is move 1.
            if game.moves.len() < 2 {
                mt = min(mt, 3000);
            }

            let cancel = Arc::new(AtomicBool::new(false));
            let cancel_c = Arc::clone(&cancel);

            let state_c = Arc::clone(&state);
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(mt as u64));
                if !cancel_c.load(Ordering::Relaxed) {
                    state_c.stop.store(true, Ordering::Relaxed);
                }
            });

            let mut game_clone = game.clone();
            thread::spawn(move || {
                let _ = iteratively_deepen(&mut game_clone, u8::MAX, Arc::clone(&state));
                cancel.store(true, Ordering::Relaxed);
                let uci_move = decode_to_uci(state.best_move.load(Ordering::Relaxed)).unwrap();
                println!("bestmove {}", uci_move);
            });
        }
        _ => {}
    }
}

fn handle_stop(state: Arc<SearchState>) {
    state.stop.store(true, Ordering::Relaxed);
}

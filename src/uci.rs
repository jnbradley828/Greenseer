use crate::engine::eval::iteratively_deepen;
use crate::engine::search::{RootBest, SearchState, TT};
use crate::engine::search_heuristics::{
    INC_FRACTION_DENOM, INC_FRACTION_NUM, MAX_TIME_FRACTION, MOVE_OVERHEAD, PANIC_THRESHOLD_MS,
    SINGLE_LEGAL_MOVE_TIME_MS, TIME_DIVISOR,
};
use oxi_chess_lib::game::ChessGame;
use oxi_chess_lib::game::GameResult::InProgress;
use oxi_chess_lib::utils::decode_to_uci;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::time::Duration;
use std::{
    io::{self, BufRead},
    sync::Arc,
    thread,
};

pub fn run() {
    let mut game = ChessGame::initialize(None);
    let state = Arc::new(SearchState::new());
    let mut tt = TT::new(128);
    let mut threads: usize = 1;
    let mut ponder_enabled = true;
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = line.unwrap();
        handle_command(
            &line,
            &mut game,
            Arc::clone(&state),
            &mut tt,
            &mut threads,
            &mut ponder_enabled,
        );
    }
}

pub fn handle_command(
    cmd: &str,
    game: &mut ChessGame,
    state: Arc<SearchState>,
    tt: &mut TT,
    threads: &mut usize,
    ponder_enabled: &mut bool,
) {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        return;
    }
    match parts[0] {
        "uci" => uci_response(),
        "setoption" => handle_setoption(&parts, tt, threads, ponder_enabled),
        "isready" => println!("readyok"),
        "ucinewgame" => {
            *game = ChessGame::initialize(None);
            tt.clear();
        }
        "position" => handle_position(&parts, game),
        "go" => handle_go(&parts, game, Arc::clone(&state), tt, threads, ponder_enabled),
        "quit" => std::process::exit(0),
        "stop" => handle_stop(Arc::clone(&state)),
        "ponderhit" => handle_ponderhit(Arc::clone(&state)),
        _ => {}
    }
}

fn uci_response() {
    println!("id name Greenseer 0.1");
    println!("id author Joshua Bradley");
    println!("option name Hash type spin default 128 min 1 max 65536");
    println!("option name Threads type spin default 1 min 1 max 1024");
    println!("option name Ponder type check default true");
    println!("uciok");
}

fn handle_setoption(parts: &[&str], tt: &mut TT, threads: &mut usize, ponder_enabled: &mut bool) {
    if parts.get(1) != Some(&"name") || parts.get(3) != Some(&"value") {
        return;
    }
    let Some(value) = parts.get(4) else {
        return;
    };

    match parts.get(2) {
        Some(&"Hash") => {
            if let Ok(size_mb) = value.parse::<usize>() {
                *tt = TT::new(size_mb);
            }
        }
        Some(&"Threads") => {
            if let Ok(n) = value.parse::<usize>() {
                *threads = n.max(1);
            }
        }
        Some(&"Ponder") => {
            *ponder_enabled = *value == "true";
        }
        _ => {}
    }
}

fn handle_position(parts: &[&str], game: &mut ChessGame) {
    // parts[0] == "position"
    // parts[1] == "startpos" or "fen"
    if parts.len() < 2 {
        return;
    }

    let moves_idx = match parts[1] {
        "startpos" => {
            *game = ChessGame::initialize(None);
            parts.iter().position(|&p| p == "moves").map(|i| i + 1)
        }
        "fen" => {
            // FEN is parts[2..7] (6 fields), then optionally "moves"
            let fen = parts[2..parts.len().min(8)].join(" ");
            *game = ChessGame::initialize(Some(&fen));
            parts.iter().position(|&p| p == "moves").map(|i| i + 1)
        }
        _ => return,
    };

    if let Some(start) = moves_idx {
        for uci_move in &parts[start..] {
            let _ = game.make_move_from_uci(uci_move, true, false);
        }
    }
}

// looks up a "<name> <value>" pair anywhere in a "go" command's parts and parses the value.
fn parse_go_param<T: std::str::FromStr>(parts: &[&str], name: &str) -> Option<T> {
    parts
        .iter()
        .position(|&p| p == name)
        .and_then(|i| parts.get(i + 1))
        .and_then(|d| d.parse::<T>().ok())
}

// reports the chosen move, plus a ponder suggestion if the GUI hasn't disabled pondering.
fn print_bestmove(best: &RootBest, ponder_enabled: bool) {
    let uci_move = decode_to_uci(best.best_move).unwrap();
    if ponder_enabled
        && let Some(&ponder_move) = best.pv.get(1)
    {
        println!(
            "bestmove {} ponder {}",
            uci_move,
            decode_to_uci(ponder_move).unwrap()
        );
    } else {
        println!("bestmove {}", uci_move);
    }
}

// spawns `threads` searches sharing tt, picks the deepest result, reports bestmove.
fn spawn_lazy_smp(
    game: &ChessGame,
    tt: &TT,
    state: Arc<SearchState>,
    threads: usize,
    max_depth: u8,
    cancel: Option<Arc<AtomicBool>>,
    mt_suggested: Option<u32>,
    ponder_enabled: bool,
) {
    if threads == 1 {
        // single thread - skip the scope/Vec machinery.
        let mut game_clone = game.clone();
        let mut tt_clone = tt.clone();
        thread::spawn(move || {
            let nodes = AtomicU64::new(0);
            let depth_reported = AtomicU8::new(0);
            let best = iteratively_deepen(
                &mut game_clone,
                max_depth,
                state,
                &mut tt_clone,
                &nodes,
                &depth_reported,
                mt_suggested,
            );
            if let Some(cancel) = cancel {
                cancel.store(true, Ordering::Relaxed);
            }
            print_bestmove(&best, ponder_enabled);
        });
        return;
    }

    let worker_inputs: Vec<(ChessGame, TT)> =
        (0..threads).map(|_| (game.clone(), tt.clone())).collect();

    thread::spawn(move || {
        let mut results: Vec<Option<RootBest>> = (0..threads).map(|_| None).collect();
        // shared across workers: live node total + first-to-report-depth tracker.
        let nodes = AtomicU64::new(0);
        let depth_reported = AtomicU8::new(0);

        thread::scope(|s| {
            for (slot, (mut game_owned, mut tt_owned)) in results.iter_mut().zip(worker_inputs) {
                let state_clone = Arc::clone(&state);
                let nodes_ref = &nodes;
                let depth_reported_ref = &depth_reported;
                s.spawn(move || {
                    *slot = Some(iteratively_deepen(
                        &mut game_owned,
                        max_depth,
                        state_clone,
                        &mut tt_owned,
                        nodes_ref,
                        depth_reported_ref,
                        mt_suggested,
                    ));
                });
            }
        });

        if let Some(cancel) = cancel {
            cancel.store(true, Ordering::Relaxed);
        }

        if let Some(best) = results.into_iter().flatten().max_by_key(|b| b.best_depth) {
            print_bestmove(&best, ponder_enabled);
        }
    });
}

fn handle_go(
    parts: &[&str],
    game: &mut ChessGame,
    state: Arc<SearchState>,
    tt: &TT,
    threads: &usize,
    ponder_enabled: &bool,
) {
    if game.result != InProgress {
        println!("bestmove 0000");
        return;
    }
    state.stop.store(false, Ordering::Relaxed);
    let depth = parse_go_param::<u8>(parts, "depth");
    let movetime = parse_go_param::<u32>(parts, "movetime");
    let wtime = parse_go_param::<u32>(parts, "wtime");
    let btime = parse_go_param::<u32>(parts, "btime");
    let winc = parse_go_param::<u32>(parts, "winc");
    let binc = parse_go_param::<u32>(parts, "binc");
    let pondering = parts.contains(&"ponder");
    state.ponder.store(pondering, Ordering::Relaxed);

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

            spawn_lazy_smp(
                game,
                tt,
                Arc::clone(&state),
                *threads,
                u8::MAX,
                Some(cancel),
                None,
                *ponder_enabled,
            );
        }
        (Some(d), _, None, None, None, None) => {
            spawn_lazy_smp(
                game,
                tt,
                Arc::clone(&state),
                *threads,
                d,
                None,
                None,
                *ponder_enabled,
            );
        }
        (_, _, Some(wt), Some(bt), _, _) => {
            let (time, inc, opptime) = if game.board.side_to_move {
                (wt.saturating_sub(MOVE_OVERHEAD), winc.unwrap_or(0), bt)
            } else {
                (bt.saturating_sub(MOVE_OVERHEAD), binc.unwrap_or(0), wt)
            };

            // mt_suggested: internal target passed to iteratively_deepen. external_mt: hard
            // ceiling enforced by the timer thread below regardless of mt_suggested.
            let mt_suggested: Option<u32>;
            let external_mt: u32;

            if time <= PANIC_THRESHOLD_MS {
                mt_suggested = None;
                external_mt = 1;
            } else if game.legal_moves.len() == 1 {
                mt_suggested = None;
                external_mt = SINGLE_LEGAL_MOVE_TIME_MS;
            } else {
                let mut mt = (time / TIME_DIVISOR) + (inc * INC_FRACTION_NUM / INC_FRACTION_DENOM);
                mt = ((mt as f32) * (time as f32 / opptime as f32) + 0.5) as u32; // scales time based on time differential.
                mt_suggested = Some(mt);
                external_mt = (time as f32 * MAX_TIME_FRACTION) as u32;
            }

            let cancel = Arc::new(AtomicBool::new(false));
            let cancel_c = Arc::clone(&cancel);
            let state_c = Arc::clone(&state);

            thread::spawn(move || {
                // don't start the mt timer if pondering.
                while state_c.ponder.load(Ordering::Relaxed)
                    && !state_c.stop.load(Ordering::Relaxed)
                {
                    thread::sleep(Duration::from_millis(10));
                }
                // don't sleep for mt if stop is already called (ponder miss scenario)
                if !state_c.stop.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(external_mt as u64));
                }

                if !cancel_c.load(Ordering::Relaxed) {
                    state_c.stop.store(true, Ordering::Relaxed);
                }
            });

            spawn_lazy_smp(
                game,
                tt,
                Arc::clone(&state),
                *threads,
                u8::MAX,
                Some(cancel),
                mt_suggested,
                *ponder_enabled,
            );
        }
        _ => {
            println!("bestmove 0000")
        }
    }
}

fn handle_ponderhit(state: Arc<SearchState>) {
    state.ponder.store(false, Ordering::Relaxed);
}

fn handle_stop(state: Arc<SearchState>) {
    state.stop.store(true, Ordering::Relaxed);
}

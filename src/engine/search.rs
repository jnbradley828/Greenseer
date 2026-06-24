use oxi_chess_lib::{
    self,
    game::{ChessGame, GameResult},
    rules, utils,
};

use crate::engine::utils::{retrieve_tt_or_none, update_tt};
use crate::engine::{self, eval::unsigned_evaluate};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};

pub struct SearchState {
    pub stop: AtomicBool,
    pub best_move: AtomicU16,
}
impl SearchState {
    pub fn new() -> Self {
        Self {
            stop: AtomicBool::new(false),
            best_move: AtomicU16::new(0),
        }
    }
}

#[derive(Clone)]
pub struct TT {
    pub entries: Arc<Vec<AtomicU64>>,
    pub size_mb: usize,
}
impl TT {
    pub fn new(size_mb: usize) -> Self {
        let no_entries = (size_mb * (2 as usize).pow(20)) / 8;
        let entry_vec = std::iter::repeat_with(|| AtomicU64::new(0))
            .take(no_entries)
            .collect();
        Self {
            entries: Arc::new(entry_vec),
            size_mb: size_mb,
        }
    }
}

pub const MAX_QDEPTH: u8 = 6;
// depth first search tree where you maximize your score and minimize opponent score at each node.
// returns (position eval, nodes visited) **pruned nodes are not counted**
// for ease of iterating, just leaving this function name as minimax even though it will be iterated.
// max_side = true means evaluate for white, false means evaluate for black.
// quiescence search means search until no more captures are available and position is not check, even if depth expires.
pub fn minimax(
    game: &mut ChessGame,
    depth: u8,
    max_side: bool,
    alpha: i16,
    beta: i16,
    state: Arc<SearchState>,
    quiescence: bool,
    check_extension: bool,
    qdepth: u8,
    tt: &mut TT,
) -> (i16, u64) {
    let mut nodes: u64 = 1;
    if game.result != GameResult::InProgress {
        return (unsigned_evaluate(game, max_side), nodes);
    }

    // skip tt is position is a repetition. (reduces 3fold repetition in won positions)
    let mut skip_tt = false;
    if let Some(&count) = game.positions_count.get(&game.board.zobrist_hash)
        && count >= 2
    {
        skip_tt = true;
    }
    // check tt before calculation logic
    if let Some(tt_entry) = retrieve_tt_or_none(tt, game.board.zobrist_hash)
        && !skip_tt
    {
        // case to use: tt_depth >= search depth
        // if flag is exact: return tt score
        // if flag is lower bound & tt score >= beta: return tt score (it will trigger a cutoff at parent node)
        // if flag is upper bound & tt score <= alpha: return tt score (it will trigger a cutoff at parent node)
        if tt_entry.2 >= depth {
            match tt_entry.3 {
                0 => return (tt_entry.1, nodes),
                1 => {
                    if tt_entry.1 >= beta {
                        return (tt_entry.1, nodes);
                    }
                }
                2 => {
                    if tt_entry.1 <= alpha {
                        return (tt_entry.1, nodes);
                    }
                }
                _ => {}
            }
        }
        reorder_moves(game, vec![tt_entry.4]);
    }

    if depth == 0 {
        // check extension: if the position is check: search depth 1.
        if rules::is_check(&game.board, game.board.side_to_move) {
            return minimax(
                game, 1, max_side, alpha, beta, state, false, true, qdepth, tt,
            );
        } else {
            // quiescence search: continue search until all captures are complete.
            let stand_pat_eval = (unsigned_evaluate(game, max_side), nodes).0;
            if max_side == game.board.side_to_move {
                if stand_pat_eval >= beta {
                    return (stand_pat_eval, nodes);
                }
            } else {
                if stand_pat_eval <= alpha {
                    return (stand_pat_eval, nodes);
                }
            }
            if qdepth == 0 {
                return (stand_pat_eval, nodes);
            } else {
                let (q_eval, q_nodes) = minimax(
                    game,
                    1,
                    max_side,
                    alpha,
                    beta,
                    state,
                    true,
                    false,
                    qdepth - 1,
                    tt,
                );
                nodes += q_nodes;
                if max_side == game.board.side_to_move {
                    // maximizing node: stand_pat is a floor
                    return (q_eval.max(stand_pat_eval), nodes);
                } else {
                    // minimizing node: stand_pat is a ceiling
                    return (q_eval.min(stand_pat_eval), nodes);
                }
            }
        }
    } else {
        if max_side == game.board.side_to_move {
            let mut alpha = alpha;
            let mut max_eval = i16::MIN;
            let mut cutoff = false;
            let mut best_move = game.legal_moves[0];
            for movei in game.legal_moves.clone() {
                if state.stop.load(Ordering::Relaxed) {
                    return (0, nodes);
                }
                if quiescence && ![1, 3, 8, 9, 10, 11].contains(&utils::decode_move(movei)[2]) {
                    // if quiescence only search: skip non captures.
                    continue;
                }
                _ = game.make_move(movei);
                let (eval, child_nodes) = minimax(
                    game,
                    depth - 1,
                    max_side,
                    alpha,
                    beta,
                    Arc::clone(&state),
                    false,
                    false,
                    qdepth,
                    tt,
                );
                nodes += child_nodes;
                _ = game.unmake_move();
                if eval > max_eval {
                    max_eval = eval;
                    best_move = movei;
                }
                if max_eval > alpha {
                    alpha = max_eval
                }
                if alpha >= beta {
                    cutoff = true;
                    break;
                }
            }

            // tt update logic
            if cutoff && !(quiescence || check_extension) {
                update_tt(
                    tt,
                    game.board.zobrist_hash,
                    max_eval,
                    depth,
                    engine::utils::TT_LOWERB_FLAG,
                    best_move,
                );
            } else if !(quiescence || check_extension) {
                update_tt(
                    tt,
                    game.board.zobrist_hash,
                    max_eval,
                    depth,
                    engine::utils::TT_EXACT_FLAG,
                    best_move,
                );
            }

            return (max_eval, nodes);
        } else {
            let mut beta = beta;
            let mut min_eval = i16::MAX;
            let mut cutoff = false;
            let mut best_move = game.legal_moves[0];
            for movei in game.legal_moves.clone() {
                if state.stop.load(Ordering::Relaxed) {
                    return (0, nodes);
                }
                if quiescence && ![1, 3, 8, 9, 10, 11].contains(&utils::decode_move(movei)[2]) {
                    // if quiescence only search: skip non captures.
                    continue;
                }
                _ = game.make_move(movei);
                let (eval, child_nodes) = minimax(
                    game,
                    depth - 1,
                    max_side,
                    alpha,
                    beta,
                    Arc::clone(&state),
                    false,
                    false,
                    qdepth,
                    tt,
                );
                nodes += child_nodes;
                _ = game.unmake_move();
                if eval < min_eval {
                    min_eval = eval;
                    best_move = movei;
                }
                if min_eval < beta {
                    beta = min_eval
                }
                if alpha >= beta {
                    cutoff = true;
                    break;
                }
            }

            // tt update logic
            if cutoff && !(quiescence || check_extension) {
                update_tt(
                    tt,
                    game.board.zobrist_hash,
                    min_eval,
                    depth,
                    engine::utils::TT_UPPERB_FLAG,
                    best_move,
                );
            } else if !(quiescence || check_extension) {
                update_tt(
                    tt,
                    game.board.zobrist_hash,
                    min_eval,
                    depth,
                    engine::utils::TT_EXACT_FLAG,
                    best_move,
                );
            }

            return (min_eval, nodes);
        }
    }
}

pub fn reorder_moves(game: &mut ChessGame, promising_moves: Vec<u16>) -> () {
    // put promising moves in the front!
    let mut front = 0;
    for i in 0..promising_moves.len() {
        if let Some(j) = game.legal_moves[front..]
            .iter()
            .position(|m| m == &promising_moves[i])
        {
            game.legal_moves.swap(front, j + front);
            front += 1;
        }
    }
}

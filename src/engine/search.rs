use arrayvec::ArrayVec;
use oxi_chess_lib::{
    self,
    board::ChessBoard,
    game::{
        ChessGame,
        GameResult::{self},
    },
    moves::get_legal_moves,
    rules,
};

use crate::engine::utils::{
    from_tt_score, is_capture, move_gives_check, retrieve_tt_or_none, to_tt_score, update_tt,
};
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

// each slot is (full zobrist key, packed data) - full key stored to eliminate
// false-positive verification hits from a truncated key tag.
#[derive(Clone)]
pub struct TT {
    pub entries: Arc<Vec<(AtomicU64, AtomicU64)>>,
    pub size_mb: usize,
}
impl TT {
    pub fn new(size_mb: usize) -> Self {
        let no_entries = (size_mb * (2 as usize).pow(20)) / 16;
        let entry_vec = std::iter::repeat_with(|| (AtomicU64::new(0), AtomicU64::new(0)))
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
    ply: u8,
    age: u8,
) -> (i16, u16, u64) {
    let mut nodes: u64 = 1;
    if game.result != GameResult::InProgress {
        // if game is over in this position
        if !matches!(game.result, GameResult::Draw(_)) {
            // if checkmate
            if max_side != game.board.side_to_move {
                return (10000 - ply as i16, 0, nodes);
            } else {
                return (-10000 + ply as i16, 0, nodes);
            }
        } else {
            return (0, 0, nodes);
        }
    }

    // skip tt is position is a repetition. (reduces 3fold repetition in won positions)
    let mut skip_tt = false;
    if let Some(&count) = game.positions_count.get(&game.board.zobrist_hash)
        && count >= 2
    {
        skip_tt = true;
    }
    // check tt before calculation logic
    let mut tt_move: Option<u16> = None;
    if let Some(tt_entry) = retrieve_tt_or_none(tt, game.board.zobrist_hash)
        && !skip_tt
    {
        // if flag is exact: return tt score
        // if flag is lower bound & tt score >= beta: return tt score (it will trigger a cutoff at parent node)
        // if flag is upper bound & tt score <= alpha: return tt score (it will trigger a cutoff at parent node)
        if tt_entry.2 >= depth {
            let tt_score = from_tt_score(tt_entry.1, ply);
            match tt_entry.3 {
                0 => return (tt_score, tt_entry.4, nodes),
                1 => {
                    if tt_score >= beta {
                        return (tt_score, tt_entry.4, nodes);
                    }
                }
                2 => {
                    if tt_score <= alpha {
                        return (tt_score, tt_entry.4, nodes);
                    }
                }
                _ => {}
            }
        }
        tt_move = Some(tt_entry.4);
    }
    // move generation and reordering are deferred to the depth != 0 branch below, right where
    // the list is actually consumed - nothing above this point (game-over check, tt lookup,
    // check-extension/quiescence dispatch) ever needs the move list, so nothing above it should
    // pay to generate one.

    if depth == 0 {
        // check extension: if the position is check: search depth 1.
        if rules::is_check(&game.board, game.board.side_to_move) {
            return minimax(
                game, 1, max_side, alpha, beta, state, false, true, qdepth, tt, ply, age,
            );
        } else {
            // quiescence search: continue search until all captures are complete.
            let stand_pat_eval = (unsigned_evaluate(game, max_side), nodes).0;
            if max_side == game.board.side_to_move {
                if stand_pat_eval >= beta {
                    return (stand_pat_eval, 0, nodes);
                }
            } else {
                if stand_pat_eval <= alpha {
                    return (stand_pat_eval, 0, nodes);
                }
            }
            if qdepth == 0 {
                return (stand_pat_eval, 0, nodes);
            } else {
                let (q_eval, mv, q_nodes) = minimax(
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
                    ply,
                    age,
                );
                nodes += q_nodes;
                if max_side == game.board.side_to_move {
                    // maximizing node: stand_pat is a floor
                    if q_eval.max(stand_pat_eval) == stand_pat_eval {
                        return (stand_pat_eval, 0, nodes);
                    } else {
                        return (q_eval, mv, nodes);
                    }
                } else {
                    // minimizing node: stand_pat is a ceiling
                    if q_eval.min(stand_pat_eval) == stand_pat_eval {
                        return (stand_pat_eval, 0, nodes);
                    } else {
                        return (q_eval, mv, nodes);
                    }
                }
            }
        }
    } else {
        // generate once per node, right where it's actually needed - reorder in place (no
        // clone of the move list) using the tt move if we have one, otherwise the usual
        // check/capture-first heuristic (skipped during quiescence/check-extension, which are
        // already restricted to a small, targeted move set).
        let mut legal_moves: ArrayVec<u16, 256> = get_legal_moves(&mut game.board);
        if let Some(mv) = tt_move {
            reorder_moves(
                &game.board,
                &mut legal_moves,
                &mut ArrayVec::from_iter([mv]),
            );
        } else if !quiescence && !check_extension {
            reorder_moves(&game.board, &mut legal_moves, &mut ArrayVec::new());
        }

        if max_side == game.board.side_to_move {
            let mut alpha = alpha;
            let alpha_orig = alpha;
            let mut max_eval = i16::MIN;
            let mut cutoff = false;
            let mut best_move = legal_moves[0];
            for movei in legal_moves {
                if state.stop.load(Ordering::Relaxed) {
                    return (0, best_move, nodes);
                }
                if quiescence && !is_capture(movei) {
                    // if quiescence only search: skip non captures.
                    continue;
                }
                _ = game.make_move(movei, false, true);
                let (eval, _, child_nodes) = minimax(
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
                    ply + 1,
                    age,
                );
                nodes += child_nodes;
                _ = game.unmake_move(false);
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

            // tt update logic — 3-way classification
            if !(quiescence || check_extension) {
                let flag = if cutoff {
                    engine::utils::TT_LOWERB_FLAG // fail-high: beta cutoff
                } else if max_eval <= alpha_orig {
                    engine::utils::TT_UPPERB_FLAG // fail-low: never beat alpha
                } else {
                    engine::utils::TT_EXACT_FLAG // landed strictly inside window
                };
                update_tt(
                    tt,
                    game.board.zobrist_hash,
                    to_tt_score(max_eval, ply),
                    depth,
                    flag,
                    best_move,
                    age,
                );
            }

            return (max_eval, best_move, nodes);
        } else {
            let mut beta = beta;
            let beta_orig = beta;
            let mut min_eval = i16::MAX;
            let mut cutoff = false;
            let mut best_move = legal_moves[0];
            for movei in legal_moves {
                if state.stop.load(Ordering::Relaxed) {
                    return (0, best_move, nodes);
                }
                if quiescence && !is_capture(movei) {
                    // if quiescence only search: skip non captures.
                    continue;
                }
                _ = game.make_move(movei, false, true);
                let (eval, _, child_nodes) = minimax(
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
                    ply + 1,
                    age,
                );
                nodes += child_nodes;
                _ = game.unmake_move(false);
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

            // tt update logic — 3-way classification
            if !(quiescence || check_extension) {
                let flag = if cutoff {
                    engine::utils::TT_UPPERB_FLAG // fail-low: alpha cutoff
                } else if min_eval >= beta_orig {
                    engine::utils::TT_LOWERB_FLAG // fail-high: never dropped below beta
                } else {
                    engine::utils::TT_EXACT_FLAG // landed strictly inside window
                };
                update_tt(
                    tt,
                    game.board.zobrist_hash,
                    to_tt_score(min_eval, ply),
                    depth,
                    flag,
                    best_move,
                    age,
                );
            }
            return (min_eval, best_move, nodes);
        }
    }
}

pub fn reorder_moves(
    board: &ChessBoard,
    legal_moves: &mut ArrayVec<u16, 256>,
    promising_moves: &mut ArrayVec<u16, 256>,
) -> () {
    let mut checks: ArrayVec<u16, 256> = ArrayVec::new();
    let mut captures: ArrayVec<u16, 256> = ArrayVec::new();
    for &mv in legal_moves.iter() {
        if promising_moves.contains(&mv) {
            continue;
        } else {
            if move_gives_check(board, mv) {
                checks.push(mv)
            } else if is_capture(mv) {
                captures.push(mv);
            }
        }
    }
    promising_moves.extend(checks);
    promising_moves.extend(captures);

    // put promising moves from last depth in the front!
    let mut front = 0;
    for i in 0..promising_moves.len() {
        if let Some(j) = legal_moves[front..]
            .iter()
            .position(|m| m == &promising_moves[i])
        {
            legal_moves.swap(front, j + front);
            front += 1;
        }
    }
}

use oxi_chess_lib::{
    self,
    game::{ChessGame, GameResult},
    rules, utils,
};

use crate::engine::eval::unsigned_evaluate;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};

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
    qdepth: u8,
) -> (i16, u64) {
    let mut nodes: u64 = 1;
    if game.result != GameResult::InProgress {
        return (unsigned_evaluate(game, max_side), nodes);
    }
    if depth == 0 {
        // check extension: if the position is check: search depth 1.
        if rules::is_check(&game.board, game.board.side_to_move) {
            return minimax(game, 1, max_side, alpha, beta, state, false, qdepth);
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
                let (q_eval, q_nodes) =
                    minimax(game, 1, max_side, alpha, beta, state, true, qdepth - 1);
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
                    qdepth,
                );
                nodes += child_nodes;
                _ = game.unmake_move();
                if eval > max_eval {
                    max_eval = eval;
                }
                if max_eval > alpha {
                    alpha = max_eval
                }
                if alpha >= beta {
                    break;
                }
            }
            return (max_eval, nodes);
        } else {
            let mut beta = beta;
            let mut min_eval = i16::MAX;
            for movei in game.legal_moves.clone() {
                if state.stop.load(Ordering::Relaxed) {
                    return (0, nodes);
                }
                if quiescence && ![1, 8, 9, 10, 11].contains(&utils::decode_move(movei)[2]) {
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
                    qdepth,
                );
                nodes += child_nodes;
                _ = game.unmake_move();
                if eval < min_eval {
                    min_eval = eval;
                }
                if min_eval < beta {
                    beta = min_eval
                }
                if alpha >= beta {
                    break;
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    /*
    #[test]
    fn test_minimax() {
        let mut game = ChessGame::initialize((1, 1), Some("k7/7P/8/8/8/8/8/K7 w - - 0 1"));
        let mm0 = minimax(
            &mut game,
            0,
            true,
            i16::MIN,
            i16::MAX,
            Arc::new(SearchState::new()),
        )
        .0;
        assert_eq!(mm0, 1);
        let mm1 = minimax(
            &mut game,
            1,
            true,
            i16::MIN,
            i16::MAX,
            Arc::new(SearchState::new()),
        )
        .0;
        assert_eq!(mm1, 9);

        let mut game = ChessGame::initialize((1, 1), Some("3r4/8/8/8/8/k7/8/K7 b - - 0 1"));
        let mm0 = minimax(
            &mut game,
            0,
            false,
            i16::MIN,
            i16::MAX,
            Arc::new(SearchState::new()),
        )
        .0;
        assert_eq!(mm0, 5);
        let mm1 = minimax(
            &mut game,
            1,
            false,
            i16::MIN,
            i16::MAX,
            Arc::new(SearchState::new()),
        )
        .0;
        assert_eq!(mm1, 10000);
    }
    */
}

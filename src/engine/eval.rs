use crate::engine::search::{SearchState, minimax, reorder_moves};
use oxi_chess_lib;
use oxi_chess_lib::game::GameResult;
use oxi_chess_lib::utils::decode_to_uci;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

// returns (best move encoded as u16, score of said move, nodes searched, runner_ups)
pub fn best_move(
    game: &mut oxi_chess_lib::game::ChessGame,
    depth: u8,
    state: Arc<SearchState>,
) -> (u16, i16, u64, VecDeque<u16>) {
    let mut best_move = game.legal_moves[0];
    const RUNNER_UPS_MAX: usize = 3;
    let mut runner_ups = VecDeque::with_capacity(RUNNER_UPS_MAX);
    _ = game.make_move(game.legal_moves[0]);
    let mut nodes: u64 = 0;
    let (mut alpha, m_nodes) = minimax(
        game,
        depth - 1,
        !game.board.side_to_move,
        i16::MIN,
        i16::MAX,
        Arc::clone(&state),
    );
    nodes += m_nodes;
    _ = game.unmake_move();

    let remaining_moves: Vec<u16> = game.legal_moves[1..].to_vec();
    for movei in remaining_moves {
        if state.stop.load(Ordering::Relaxed) {
            return (
                state.best_move.load(Ordering::Relaxed),
                alpha,
                nodes,
                runner_ups,
            );
        }
        _ = game.make_move(movei);
        let (eval, m_nodes) = minimax(
            game,
            depth - 1,
            !game.board.side_to_move,
            alpha,
            i16::MAX,
            Arc::clone(&state),
        );
        nodes += m_nodes;
        _ = game.unmake_move();
        if eval > alpha {
            if runner_ups.len() == RUNNER_UPS_MAX {
                runner_ups.pop_front();
            }
            runner_ups.push_back(best_move);
            alpha = eval;
            best_move = movei;
        }
    }

    return (best_move, alpha, nodes, runner_ups);
}

pub fn iteratively_deepen(
    game: &mut oxi_chess_lib::game::ChessGame,
    max_depth: u8,
    state: Arc<SearchState>,
) -> u16 {
    let start_time = Instant::now();
    let mut nodes = 0;
    // stores first move in the list just in case "stop" is called immediately.
    state
        .best_move
        .store(game.legal_moves[0], Ordering::Relaxed);

    let mut moves_of_interest: Vec<u16> = vec![state.best_move.load(Ordering::Relaxed)];
    for d in 1..=max_depth {
        if state.stop.load(Ordering::Relaxed) {
            return state.best_move.load(Ordering::Relaxed);
        } else {
            reorder_moves(game, moves_of_interest);
            let (best_move, score, dnodes, runner_ups) = best_move(game, d, Arc::clone(&state));
            state.best_move.store(best_move, Ordering::Relaxed);
            moves_of_interest = vec![best_move];
            moves_of_interest.extend(runner_ups.iter().rev());
            nodes += dnodes;
            let best_uci = decode_to_uci(best_move).unwrap();
            if !state.stop.load(Ordering::Relaxed) {
                let elapsed = start_time.elapsed().as_millis().max(1);
                let nps = ((nodes * 1000) as u128) / elapsed;
                println!(
                    "info depth {d} score cp {score} nodes {nodes} nps {nps} time {elapsed} pv {best_uci}"
                );
            }
        }
    }
    return state.best_move.load(Ordering::Relaxed);
}

// returns objective material count
const PIECE_VALUES: [i16; 5] = [100, 300, 300, 500, 900]; // [p, n, b, r, q]
fn count_material(game: &oxi_chess_lib::game::ChessGame) -> i16 {
    let mut material: i16 = 0;

    material +=
        PIECE_VALUES[0] as i16 * (game.board.pawns & game.board.white_pieces).count_ones() as i16;
    material +=
        PIECE_VALUES[1] as i16 * (game.board.knights & game.board.white_pieces).count_ones() as i16;
    material +=
        PIECE_VALUES[2] as i16 * (game.board.bishops & game.board.white_pieces).count_ones() as i16;
    material +=
        PIECE_VALUES[3] as i16 * (game.board.rooks & game.board.white_pieces).count_ones() as i16;
    material +=
        PIECE_VALUES[4] as i16 * (game.board.queens & game.board.white_pieces).count_ones() as i16;

    material -=
        PIECE_VALUES[0] as i16 * (game.board.pawns & game.board.black_pieces).count_ones() as i16;
    material -=
        PIECE_VALUES[1] as i16 * (game.board.knights & game.board.black_pieces).count_ones() as i16;
    material -=
        PIECE_VALUES[2] as i16 * (game.board.bishops & game.board.black_pieces).count_ones() as i16;
    material -=
        PIECE_VALUES[3] as i16 * (game.board.rooks & game.board.black_pieces).count_ones() as i16;
    material -=
        PIECE_VALUES[4] as i16 * (game.board.queens & game.board.black_pieces).count_ones() as i16;

    return material;
}

#[rustfmt::skip]
const PAWN_MOD: [i8; 64] = [
    0,   0,   0,   0,   0,   0,   0,   0,   // rank 1
    5,  10,  10, -20, -20,  10,  10,   5,   // rank 2
    5,  -5, -10,   0,   0, -10,  -5,   5,   // rank 3
    0,   0,  20,  20,  20,  20,   0,   0,   // rank 4
    5,   5,  15,  25,  25,  15,   5,   5,   // rank 5
   10,  10,  20,  30,  30,  20,  10,  10,   // rank 6
   50,  50,  50,  50,  50,  50,  50,  50,   // rank 7
    0,   0,   0,   0,   0,   0,   0,   0    // rank 8
];

#[rustfmt::skip]
const KNIGHT_MOD: [i8; 64] = [
  -50, -40, -30, -30, -30, -30, -40, -50,   // rank 1
  -40, -20,   0,   5,   5,   0, -20, -40,   // rank 2
  -30,   5,  10,  15,  15,  10,   5, -30,   // rank 3
  -30,   0,  15,  20,  20,  15,   0, -30,   // rank 4
  -30,   5,  15,  20,  20,  15,   5, -30,   // rank 5
  -30,   0,  10,  15,  15,  10,   0, -30,   // rank 6
  -40, -20,   0,   0,   0,   0, -20, -40,   // rank 7
  -50, -40, -30, -30, -30, -30, -40, -50    // rank 8
];

#[rustfmt::skip]
const BISHOP_MOD: [i8; 64] = [
  -20, -10, -10, -10, -10, -10, -10, -20,   // rank 1
  -10,   5,   0,   0,   0,   0,   5, -10,   // rank 2
  -10,   0,   5,  10,  10,   5,   0, -10,   // rank 3
  -10,   0,  10,  15,  15,  10,   0, -10,   // rank 4
  -10,   0,  10,  15,  15,  10,   0, -10,   // rank 5
  -10,   0,   5,  10,  10,   5,   0, -10,   // rank 6
  -10,   5,   0,   0,   0,   0,   5, -10,   // rank 7
  -20, -10, -10, -10, -10, -10, -10, -20    // rank 8
];

#[rustfmt::skip]
const ROOK_MOD: [i8; 64] = [
    0,   0,   3,   5,   5,   3,   0,   0,   // rank 1
   -5,   0,   0,   0,   0,   0,   0,  -5,   // rank 2
   -5,   0,   0,   0,   0,   0,   0,  -5,   // rank 3
   -5,   0,   0,   0,   0,   0,   0,  -5,   // rank 4
   -5,   0,   0,   0,   0,   0,   0,  -5,   // rank 5
   -5,   0,   0,   0,   0,   0,   0,  -5,   // rank 6
    5,  10,  10,  10,  10,  10,  10,   5,   // rank 7
    0,   0,   0,   0,   0,   0,   0,   0    // rank 8
];

#[rustfmt::skip]
const QUEEN_MOD: [i8; 64] = [
  -20, -10, -10,  -5,  -5, -10, -10, -20,   // rank 1
  -10,   0,   0,   0,   0,   0,   0, -10,   // rank 2
  -10,   0,   5,   5,   5,   5,   0, -10,   // rank 3
   -5,   0,   5,   5,   5,   5,   0,  -5,   // rank 4
   -5,   0,   5,   5,   5,   5,   0,  -5,   // rank 5
  -10,   0,   5,   5,   5,   5,   0, -10,   // rank 6
  -10,   0,   0,   0,   0,   0,   0, -10,   // rank 7
  -20, -10, -10,  -5,  -5, -10, -10, -20    // rank 8
];

#[rustfmt::skip]
const KING_MOD: [i8; 64] = [
   20,  30,  10,   0,   0,  10,  30,  20,   // rank 1
   20,  20,   0,   0,   0,   0,  20,  20,   // rank 2
  -10, -20, -20, -20, -20, -20, -20, -10,   // rank 3
  -20, -30, -30, -40, -40, -30, -30, -20,   // rank 4
  -30, -40, -40, -50, -50, -40, -40, -30,   // rank 5
  -30, -40, -40, -50, -50, -40, -40, -30,   // rank 6
  -30, -40, -40, -50, -50, -40, -40, -30,   // rank 7
  -30, -40, -40, -50, -50, -40, -40, -30    // rank 8
];

// returns objective static evaluation.
pub fn evaluate(game: &oxi_chess_lib::game::ChessGame) -> i16 {
    // evaluates WITHOUT future calculation. use minimax to calculate at depth.
    if matches!(game.result, GameResult::Draw(_)) {
        return 0;
    } else if matches!(game.result, GameResult::WhiteWins(_)) {
        return 10000;
    } else if matches!(game.result, GameResult::BlackWins(_)) {
        return -10000;
    } else {
        let mut eval: i16 = 0;
        eval += count_material(game);
        eval += positional_mods(game);
        return eval;
    }
}

pub fn positional_mods(game: &oxi_chess_lib::game::ChessGame) -> i16 {
    let mut modifier: i16 = 0;

    let w_bbs: [u64; 6] = [
        game.board.pawns & game.board.white_pieces,
        game.board.knights & game.board.white_pieces,
        game.board.bishops & game.board.white_pieces,
        game.board.rooks & game.board.white_pieces,
        game.board.queens & game.board.white_pieces,
        game.board.kings & game.board.white_pieces,
    ];

    let b_bbs: [u64; 6] = [
        game.board.pawns & game.board.black_pieces,
        game.board.knights & game.board.black_pieces,
        game.board.bishops & game.board.black_pieces,
        game.board.rooks & game.board.black_pieces,
        game.board.queens & game.board.black_pieces,
        game.board.kings & game.board.black_pieces,
    ];

    for i in 0..6 {
        modifier += bb_to_posmod(w_bbs[i], i as u8, true);
        modifier += bb_to_posmod(b_bbs[i], i as u8, false);
    }

    return modifier;
}

// piece type 0-5 = pawn, knight, bishop, rook, queen, king
// returns objective positional score modifications
pub fn bb_to_posmod(bb: u64, piece_type: u8, to_move: bool) -> i16 {
    let mod_mask: &[i8; 64];
    let mut modifier: i16 = 0;
    match piece_type {
        0 => mod_mask = &PAWN_MOD,
        1 => mod_mask = &KNIGHT_MOD,
        2 => mod_mask = &BISHOP_MOD,
        3 => mod_mask = &ROOK_MOD,
        4 => mod_mask = &QUEEN_MOD,
        5 => mod_mask = &KING_MOD,
        _ => panic!("unexpected piece type value: {}", piece_type),
    }
    let mut mbb: u64 = bb;
    if to_move {
        while mbb != 0 {
            let i = mbb.trailing_zeros() as usize;
            mbb &= mbb - 1;
            modifier += mod_mask[i] as i16;
        }
    } else {
        while mbb != 0 {
            let i = mbb.trailing_zeros() as usize;
            mbb &= mbb - 1;
            modifier -= mod_mask[63 - i] as i16;
        }
    }

    return modifier;
}

// returns score for argument max_side (true = white)
pub fn unsigned_evaluate(game: &oxi_chess_lib::game::ChessGame, max_side: bool) -> i16 {
    if max_side {
        return evaluate(game);
    } else {
        return -evaluate(game);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_best_move() {
        // White promotes pawn to queen
        let mut game = oxi_chess_lib::game::ChessGame::initialize(
            (1, 1),
            Some("k7/7P/8/8/8/8/8/K7 w - - 0 1"),
        );
        let best_move_uci = oxi_chess_lib::utils::decode_to_uci(
            best_move(&mut game, 1, Arc::new(SearchState::new())).0,
        )
        .unwrap();
        assert_eq!(best_move_uci, "h7h8q".to_string());

        // White Qb7# or Qa7# (mate in 1): Ka6, Qb6 vs Ka8
        let mut game = oxi_chess_lib::game::ChessGame::initialize(
            (1, 1),
            Some("k7/8/KQ6/8/8/8/8/8 w - - 0 1"),
        );
        let best_move_uci = oxi_chess_lib::utils::decode_to_uci(
            best_move(&mut game, 1, Arc::new(SearchState::new())).0,
        )
        .unwrap();
        assert!(best_move_uci == "b6b7".to_string() || best_move_uci == "b6a7".to_string());

        // White Rxd5: captures undefended black queen
        let mut game = oxi_chess_lib::game::ChessGame::initialize(
            (1, 1),
            Some("k7/8/8/3q4/8/8/8/K2R4 w - - 0 1"),
        );
        let best_move_uci = oxi_chess_lib::utils::decode_to_uci(
            best_move(&mut game, 1, Arc::new(SearchState::new())).0,
        )
        .unwrap();
        assert_eq!(best_move_uci, "d1d5".to_string());

        // Black Rxd1: captures undefended white rook
        let mut game = oxi_chess_lib::game::ChessGame::initialize(
            (1, 1),
            Some("k2r4/8/8/8/8/8/8/K2R4 b - - 0 1"),
        );
        let best_move_uci = oxi_chess_lib::utils::decode_to_uci(
            best_move(&mut game, 1, Arc::new(SearchState::new())).0,
        )
        .unwrap();
        assert_eq!(best_move_uci, "d8d1".to_string());
    }
}

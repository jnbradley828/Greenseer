use crate::engine::eval_heuristics::{
    EARLY_QUEEN_FACTOR, EG_BISHOP_MOD, EG_BISHOP_PAIR_BONUS, EG_KING_MOD, EG_KNIGHT_MOD,
    EG_PAWN_MOD, EG_QUEEN_MOD, EG_ROOK_MOD, MAX_MAJOR_PIECE_MATERIAL, MG_BISHOP_MOD,
    MG_BISHOP_PAIR_BONUS, MG_KING_MOD, MG_KNIGHT_MOD, MG_PAWN_MOD, MG_QUEEN_MOD, MG_ROOK_MOD,
    OPEN_FILE_ROOK, PIECE_VALUES, SEMIOPEN_FILE_ROOK, TEMPO_BONUS,
};
use crate::engine::search::{self, SearchState, TT, minimax, reorder_moves};
use oxi_chess_lib;
use oxi_chess_lib::moves::get_legal_moves;
use oxi_chess_lib::utils::decode_to_uci;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

// returns (best move encoded as u16, score of said move, nodes searched, runner_ups)
// DEPRECATED: MOVED LOGIC TO minimax() IN search.rs
pub fn best_move(
    game: &mut oxi_chess_lib::game::ChessGame,
    depth: u8,
    state: Arc<SearchState>,
    tt: &mut TT,
    age: u8,
) -> (u16, i16, u64, VecDeque<u16>) {
    let mut best_move = game.legal_moves[0];
    const RUNNER_UPS_MAX: usize = 2;
    let legal_moves = game.legal_moves.clone();
    let mut runner_ups = VecDeque::with_capacity(RUNNER_UPS_MAX);
    _ = game.make_move(legal_moves[0], true, true);
    let mut nodes: u64 = 0;
    let (mut alpha, _, m_nodes) = minimax(
        game,
        depth - 1,
        !game.board.side_to_move,
        i16::MIN,
        i16::MAX,
        Arc::clone(&state),
        false,
        false,
        search::MAX_QDEPTH,
        tt,
        1,
        age,
    );
    nodes += m_nodes;
    _ = game.unmake_move(false);

    let remaining_moves: Vec<u16> = legal_moves[1..].to_vec();
    for movei in remaining_moves {
        if state.stop.load(Ordering::Relaxed) {
            return (
                state.best_move.load(Ordering::Relaxed),
                alpha,
                nodes,
                runner_ups,
            );
        }
        _ = game.make_move(movei, true, true);
        let (eval, _, m_nodes) = minimax(
            game,
            depth - 1,
            !game.board.side_to_move,
            alpha,
            i16::MAX,
            Arc::clone(&state),
            false,
            false,
            search::MAX_QDEPTH,
            tt,
            1,
            age,
        );
        nodes += m_nodes;
        _ = game.unmake_move(false);
        if eval > alpha {
            if runner_ups.len() == RUNNER_UPS_MAX {
                runner_ups.pop_front();
            }
            runner_ups.push_back(best_move);
            alpha = eval;
            best_move = movei;
        }
    }
    game.legal_moves = legal_moves;
    return (best_move, alpha, nodes, runner_ups);
}

pub fn iteratively_deepen(
    game: &mut oxi_chess_lib::game::ChessGame,
    max_depth: u8,
    state: Arc<SearchState>,
    tt: &mut TT,
) -> u16 {
    println!("info string start eval {}", evaluate(game));
    let start_time = Instant::now();
    let mut nodes = 0;
    let age = (game.moves.len() / 2) as u8; // age for transposition table entries
    // stores first move in the list just in case "stop" is called immediately.
    state
        .best_move
        .store(game.legal_moves[0], Ordering::Relaxed);

    for d in 1..=max_depth {
        if state.stop.load(Ordering::Relaxed) {
            return state.best_move.load(Ordering::Relaxed);
        } else {
            let legal_moves = game.legal_moves.clone();
            let (score, best_move, dnodes) = minimax(
                game,
                d,
                game.board.side_to_move,
                i16::MIN,
                i16::MAX,
                Arc::clone(&state),
                false,
                false,
                search::MAX_QDEPTH,
                tt,
                0,
                age,
            );
            // let (best_move, score, dnodes, runner_ups) = best_move(game, d, Arc::clone(&state), tt, age);
            nodes += dnodes;
            let best_uci = decode_to_uci(best_move).unwrap();
            if !state.stop.load(Ordering::Relaxed) {
                state.best_move.store(best_move, Ordering::Relaxed);
                let elapsed = start_time.elapsed().as_millis().max(1);
                let nps = ((nodes * 1000) as u128) / elapsed;
                println!(
                    "info depth {d} score cp {score} nodes {nodes} nps {nps} time {elapsed} pv {best_uci}"
                );
            }
            game.legal_moves = legal_moves; // restore legal_moves
        }
    }
    return state.best_move.load(Ordering::Relaxed);
}

// returns objective material count
fn count_material(game: &oxi_chess_lib::game::ChessGame, pawns: bool, total: bool) -> i16 {
    let mut material: i16 = 0;

    if pawns {
        material += PIECE_VALUES[0] as i16
            * (game.board.pawns & game.board.white_pieces).count_ones() as i16;
        let bpawns = PIECE_VALUES[0] as i16
            * (game.board.pawns & game.board.black_pieces).count_ones() as i16;
        if total {
            material += bpawns;
        } else {
            material -= bpawns;
        }
    }
    material +=
        PIECE_VALUES[1] as i16 * (game.board.knights & game.board.white_pieces).count_ones() as i16;
    material +=
        PIECE_VALUES[2] as i16 * (game.board.bishops & game.board.white_pieces).count_ones() as i16;
    material +=
        PIECE_VALUES[3] as i16 * (game.board.rooks & game.board.white_pieces).count_ones() as i16;
    material +=
        PIECE_VALUES[4] as i16 * (game.board.queens & game.board.white_pieces).count_ones() as i16;

    let mut b_material = 0;
    b_material +=
        PIECE_VALUES[1] as i16 * (game.board.knights & game.board.black_pieces).count_ones() as i16;
    b_material +=
        PIECE_VALUES[2] as i16 * (game.board.bishops & game.board.black_pieces).count_ones() as i16;
    b_material +=
        PIECE_VALUES[3] as i16 * (game.board.rooks & game.board.black_pieces).count_ones() as i16;
    b_material +=
        PIECE_VALUES[4] as i16 * (game.board.queens & game.board.black_pieces).count_ones() as i16;
    if total {
        material += b_material;
    } else {
        material -= b_material;
    }

    return material;
}

// returns objective static evaluation.
pub fn evaluate(game: &oxi_chess_lib::game::ChessGame) -> i16 {
    // evaluates WITHOUT future calculation. use minimax to calculate at depth.
    let mut eval: i16 = 0;
    let net_material = count_material(game, true, false);
    let total_material = count_material(game, true, true);
    eval += net_material;
    eval += positional_mods(game, net_material, total_material);
    return eval;
}

// value tuned based on fastchess match results.
const C: f32 = 15.0;

// returns objective positional modifications to score.
pub fn positional_mods(
    game: &oxi_chess_lib::game::ChessGame,
    net_material: i16,
    total_material: i16,
) -> i16 {
    let major_piece_count = count_material(game, false, true);

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

    let mut mg_modifier: i16 = 0;
    let mut eg_modifier: i16 = 0;

    for i in 0..6 {
        mg_modifier += bb_to_posmod(w_bbs[i], i as u8, true, false)
            + bb_to_posmod(b_bbs[i], i as u8, false, false);
        eg_modifier += bb_to_posmod(w_bbs[i], i as u8, true, true)
            + bb_to_posmod(b_bbs[i], i as u8, false, true);
    }

    // give a bonus for rooks on open files
    let mut w_rooks = game.board.rooks & game.board.white_pieces;
    let mut b_rooks = game.board.rooks & game.board.black_pieces;

    while w_rooks != 0 {
        let sq: u64 = 1 << w_rooks.trailing_zeros();
        let file_i = (oxi_chess_lib::utils::file_value(sq) - 1) as usize;

        if ROOK_FILE_MASK[file_i] & game.board.pawns == 0 {
            mg_modifier += OPEN_FILE_ROOK;
        } else if ROOK_FILE_MASK[file_i] & (game.board.pawns & game.board.white_pieces) == 0 {
            mg_modifier += SEMIOPEN_FILE_ROOK;
        }
        w_rooks &= w_rooks - 1;
    }
    while b_rooks != 0 {
        let sq: u64 = 1 << b_rooks.trailing_zeros();
        let file_i = (oxi_chess_lib::utils::file_value(sq) - 1) as usize;

        if ROOK_FILE_MASK[file_i] & game.board.pawns == 0 {
            mg_modifier -= OPEN_FILE_ROOK;
        } else if ROOK_FILE_MASK[file_i] & (game.board.pawns & game.board.black_pieces) == 0 {
            mg_modifier -= SEMIOPEN_FILE_ROOK;
        }
        b_rooks &= b_rooks - 1;
    }

    // give a bonus for bishop pair
    let w_bishops = (game.board.bishops & game.board.white_pieces).count_ones();
    let b_bishops = (game.board.bishops & game.board.black_pieces).count_ones();
    if w_bishops >= 2 {
        mg_modifier += MG_BISHOP_PAIR_BONUS;
        eg_modifier += EG_BISHOP_PAIR_BONUS;
    }
    if b_bishops >= 2 {
        mg_modifier -= MG_BISHOP_PAIR_BONUS;
        eg_modifier -= EG_BISHOP_PAIR_BONUS;
    }

    // weight middlegame vs endgame relevancy
    let eg_weighted = ((MAX_MAJOR_PIECE_MATERIAL - major_piece_count) as f32
        / MAX_MAJOR_PIECE_MATERIAL as f32)
        * eg_modifier as f32;
    let mg_weighted =
        (major_piece_count as f32 / MAX_MAJOR_PIECE_MATERIAL as f32) * mg_modifier as f32;

    // if down material, try to keep total material count high (don't trade).
    // if up material, try to total material count low..
    // Do this by applying a positional bonus/decrement = (material_net/material_total) * c
    // c determines how impactful this is to evaluation.

    let trading_incentive =
        (100.0 * C * (net_material as f32 / total_material as f32)).round() as i16;

    // give a penalty for early queen moves.
    let mut early_queen_mod: i16 = 0;
    if game.board.fullmove_number <= 6 {
        let w_queen_home: bool =
            (game.board.queens & game.board.white_pieces) & 0x0000000000000008 != 0;
        let b_queen_home: bool =
            (game.board.queens & game.board.black_pieces) & 0x0800000000000000 != 0;
        if !w_queen_home {
            early_queen_mod -= ((7 - game.board.fullmove_number) as i16) * EARLY_QUEEN_FACTOR;
        }
        if !b_queen_home {
            early_queen_mod += ((7 - game.board.fullmove_number) as i16) * EARLY_QUEEN_FACTOR;
        }
    }

    let mut result = (mg_weighted + eg_weighted).round() as i16;
    result += trading_incentive;
    result += early_queen_mod;
    if game.board.side_to_move {
        result += TEMPO_BONUS;
    } else {
        result -= TEMPO_BONUS;
    }

    return result;
}

// piece type 0-5 = pawn, knight, bishop, rook, queen, king
// returns objective positional score modifications
pub fn bb_to_posmod(bb: u64, piece_type: u8, to_move: bool, endgame: bool) -> i16 {
    let mod_mask: &[i8; 64];
    let mut modifier: i16 = 0;
    if endgame {
        match piece_type {
            0 => mod_mask = &EG_PAWN_MOD,
            1 => mod_mask = &EG_KNIGHT_MOD,
            2 => mod_mask = &EG_BISHOP_MOD,
            3 => mod_mask = &EG_ROOK_MOD,
            4 => mod_mask = &EG_QUEEN_MOD,
            5 => mod_mask = &EG_KING_MOD,
            _ => panic!("unexpected piece type value: {}", piece_type),
        }
    } else {
        match piece_type {
            0 => mod_mask = &MG_PAWN_MOD,
            1 => mod_mask = &MG_KNIGHT_MOD,
            2 => mod_mask = &MG_BISHOP_MOD,
            3 => mod_mask = &MG_ROOK_MOD,
            4 => mod_mask = &MG_QUEEN_MOD,
            5 => mod_mask = &MG_KING_MOD,
            _ => panic!("unexpected piece type value: {}", piece_type),
        }
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

// ROOK_FILE_MASK[0] = a file.
const ROOK_FILE_MASK: [u64; 8] = [
    0x0101010101010101,
    0x0202020202020202,
    0x0404040404040404,
    0x0808080808080808,
    0x1010101010101010,
    0x2020202020202020,
    0x4040404040404040,
    0x8080808080808080,
];

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
        let mut tt = TT::new(128);
        let best_move_uci = oxi_chess_lib::utils::decode_to_uci(
            best_move(&mut game, 1, Arc::new(SearchState::new()), &mut tt, 0).0,
        )
        .unwrap();
        assert_eq!(best_move_uci, "h7h8q".to_string());

        // White Qb7# or Qa7# (mate in 1): Ka6, Qb6 vs Ka8
        let mut game = oxi_chess_lib::game::ChessGame::initialize(
            (1, 1),
            Some("k7/8/KQ6/8/8/8/8/8 w - - 0 1"),
        );
        let mut tt = TT::new(128);
        let best_move_uci = oxi_chess_lib::utils::decode_to_uci(
            best_move(&mut game, 1, Arc::new(SearchState::new()), &mut tt, 0).0,
        )
        .unwrap();
        assert!(best_move_uci == "b6b7".to_string() || best_move_uci == "b6a7".to_string());

        // White Rxd5: captures undefended black queen
        let mut game = oxi_chess_lib::game::ChessGame::initialize(
            (1, 1),
            Some("k7/8/8/3q4/8/8/8/K2R4 w - - 0 1"),
        );
        let mut tt = TT::new(128);
        let best_move_uci = oxi_chess_lib::utils::decode_to_uci(
            best_move(&mut game, 1, Arc::new(SearchState::new()), &mut tt, 0).0,
        )
        .unwrap();
        assert_eq!(best_move_uci, "d1d5".to_string());

        // Black Rxd1: captures undefended white rook
        let mut game = oxi_chess_lib::game::ChessGame::initialize(
            (1, 1),
            Some("k2r4/8/8/8/8/8/8/K2R4 b - - 0 1"),
        );
        let mut tt = TT::new(128);
        let best_move_uci = oxi_chess_lib::utils::decode_to_uci(
            best_move(&mut game, 1, Arc::new(SearchState::new()), &mut tt, 0).0,
        )
        .unwrap();
        assert_eq!(best_move_uci, "d8d1".to_string());
    }
}

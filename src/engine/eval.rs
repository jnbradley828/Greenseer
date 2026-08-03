use crate::engine::eval_heuristics::*;
use crate::engine::search::{self, MAX_PV_LEN, RootBest, SearchState, TT, negamax};
use crate::engine::search_heuristics::MAX_QDEPTH;
use crate::engine::utils::{
    KING_ZONE_MASKS, KING_ZONE_PAWN_ATTACKERS, king_file_weakness_mult, pawn_backward,
    pawn_isolated, pawn_passed, pawn_shield_score,
};
use arrayvec::ArrayVec;
use oxi_chess_lib;
use oxi_chess_lib::board::ChessBoard;
use oxi_chess_lib::magic_tables::ROOK_ATTACKS;
use oxi_chess_lib::moves::{
    get_bishop_attacks, get_legal_moves, get_queen_attacks, get_rook_attacks, knight_attacks,
};
use oxi_chess_lib::utils::decode_to_uci;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

pub fn iteratively_deepen(
    game: &mut oxi_chess_lib::game::ChessGame,
    max_depth: u8,
    state: Arc<SearchState>,
    tt: &mut TT,
) -> (u16, ArrayVec<u16, 255>) {
    println!("info string start eval {}", evaluate(game));
    let start_time = Instant::now();
    let mut nodes = 0;
    let age = (game.moves.len() / 2) as u8; // age for transposition table entries
    // stores first move in the list just in case "stop" is called immediately.
    let mut best = RootBest::new(game.legal_moves[0]);
    // persists (and accumulates) across all depth iterations of this search, not reset per depth.
    let mut killers = search::KillerTable::new();

    for d in 1..=max_depth {
        if state.stop.load(Ordering::Relaxed) {
            return (best.best_move, best.pv);
        } else {
            let legal_moves = game.legal_moves.clone();
            let mut pv: ArrayVec<u16, MAX_PV_LEN> = ArrayVec::new();
            let (_, _, dnodes, _) = negamax(
                game,
                d,
                -i16::MAX,
                i16::MAX,
                Arc::clone(&state),
                false,
                false,
                MAX_QDEPTH,
                tt,
                &mut killers,
                0,
                age,
                &mut best,
                &mut pv,
                true,
            );
            nodes += dnodes;
            // negamax publishes to best incrementally as root moves complete, so this is always
            // the authoritative answer regardless of whether the depth finished or was
            // interrupted mid-loop. best_depth (not the loop variable d) reflects which depth
            // this result actually came from.
            if !state.stop.load(Ordering::Relaxed) {
                let elapsed = start_time.elapsed().as_millis().max(1);
                let nps = ((nodes * 1000) as u128) / elapsed;
                let (score, reported_depth) = (best.best_score, best.best_depth);
                // pv can be shorter than reported_depth (early returns don't extend it) - fall
                // back to just best_move if it's somehow empty.
                let pv_uci = if best.pv.is_empty() {
                    decode_to_uci(best.best_move).unwrap()
                } else {
                    best.pv
                        .iter()
                        .map(|&mv| decode_to_uci(mv).unwrap())
                        .collect::<Vec<_>>()
                        .join(" ")
                };
                println!(
                    "info depth {reported_depth} score cp {score} nodes {nodes} nps {nps} time {elapsed} pv {pv_uci}"
                );
            }
            game.legal_moves = legal_moves; // restore legal_moves
        }
    }
    return (best.best_move, best.pv);
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
    // evaluates WITHOUT future calculation. use negamax to calculate at depth.
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
    let mut mobility_score: i16 = 0;
    // raw (unscaled) king attack units: white's attack on black's king zone, and
    // black's attack on white's king zone. summed across all pieces before applying the
    // nonlinear scaling once per side, rather than per-piece.
    let mut w_king_attack_units: i16 = 0;
    let mut b_king_attack_units: i16 = 0;

    // each side's king square/zone is fixed for the whole position - compute once here rather
    // than re-deriving it inside bb_to_posmod on every one of the 12 calls below.
    let w_king_sq = (game.board.kings & game.board.white_pieces).trailing_zeros() as usize;
    let b_king_sq = (game.board.kings & game.board.black_pieces).trailing_zeros() as usize;
    let w_king_zone = KING_ZONE_MASKS[w_king_sq];
    let b_king_zone = KING_ZONE_MASKS[b_king_sq];

    for i in 0..6 {
        let (w_mg, w_eg, w_ms, w_au) =
            bb_to_posmod(w_bbs[i], i as u8, true, &game.board, b_king_sq, b_king_zone);
        let (b_mg, b_eg, b_ms, b_au) = bb_to_posmod(
            b_bbs[i],
            i as u8,
            false,
            &game.board,
            w_king_sq,
            w_king_zone,
        );
        mg_modifier += w_mg + b_mg;
        eg_modifier += w_eg + b_eg;
        mobility_score += w_ms + b_ms;
        w_king_attack_units += w_au;
        b_king_attack_units += b_au;
    }

    // king safety: polynomial (linear + quadratic) scaling of raw attack units against each
    // king, so danger compounds as attackers pile up rather than growing linearly, while still
    // giving small attacker counts a nonzero contribution. clamped first so a degenerate
    // position with many overlapping attackers can't produce an unbounded eval swing.
    // middlegame only - fades out along with the rest of mg_modifier via the phase weighting
    // below.
    let w_units = w_king_attack_units.min(MAX_KING_ATTACK_UNITS) as f32;
    let b_units = b_king_attack_units.min(MAX_KING_ATTACK_UNITS) as f32;
    let w_base_danger = KING_ATTACK_LINEAR * w_units + KING_ATTACK_QUADRATIC * w_units * w_units;
    let b_base_danger = KING_ATTACK_LINEAR * b_units + KING_ATTACK_QUADRATIC * b_units * b_units;
    // open/semi-open files near the king amplify existing attack pressure rather than adding
    // danger on their own - a weak file with zero real attackers still contributes zero here,
    // avoiding false positives like a normal pre-castling central pawn trade.
    let w_king_danger = (w_base_danger
        * king_file_weakness_mult(b_king_sq as u8, false, &game.board))
    .round() as i16;
    let b_king_danger = (b_base_danger
        * king_file_weakness_mult(w_king_sq as u8, true, &game.board))
    .round() as i16;
    mg_modifier += w_king_danger - b_king_danger;

    // give a bonus for rooks on open files
    let mut w_rooks = game.board.rooks & game.board.white_pieces;
    let mut b_rooks = game.board.rooks & game.board.black_pieces;

    while w_rooks != 0 {
        let sq: u64 = 1 << w_rooks.trailing_zeros();
        let file_i = (oxi_chess_lib::utils::file_value(sq) - 1) as usize;

        if FILE_MASK[file_i] & game.board.pawns == 0 {
            mg_modifier += OPEN_FILE_ROOK;
        } else if FILE_MASK[file_i] & (game.board.pawns & game.board.white_pieces) == 0 {
            mg_modifier += SEMIOPEN_FILE_ROOK;
        }
        w_rooks &= w_rooks - 1;
    }
    while b_rooks != 0 {
        let sq: u64 = 1 << b_rooks.trailing_zeros();
        let file_i = (oxi_chess_lib::utils::file_value(sq) - 1) as usize;

        if FILE_MASK[file_i] & game.board.pawns == 0 {
            mg_modifier -= OPEN_FILE_ROOK;
        } else if FILE_MASK[file_i] & (game.board.pawns & game.board.black_pieces) == 0 {
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

    // give a penalty for doubled pawns
    let mut wdb_pawns_count: i16 = 0;
    let mut bdb_pawns_count: i16 = 0;
    for i in 0..8 {
        let file_mask = FILE_MASK[i];
        wdb_pawns_count +=
            ((file_mask & game.board.white_pieces & game.board.pawns).count_ones() as i16 - 1)
                .max(0);
        bdb_pawns_count +=
            ((file_mask & game.board.black_pieces & game.board.pawns).count_ones() as i16 - 1)
                .max(0);
    }
    mg_modifier -= (wdb_pawns_count - bdb_pawns_count) * MG_DOUBLED_PAWN_PENALTY;
    eg_modifier -= (wdb_pawns_count - bdb_pawns_count) * EG_DOUBLED_PAWN_PENALTY;

    // weight middlegame vs endgame relevancy - complementary fractions of the same phase, so
    // eg's fraction is just 1 - mg's rather than a separately computed ratio.
    let mg_frac = major_piece_count as f32 / MG_PHASE_SPAN as f32;
    let mg_weighted = mg_frac * mg_modifier as f32;
    let eg_weighted = (1.0 - mg_frac) * eg_modifier as f32;

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

    let mut result = (mg_weighted + eg_weighted).round() as i16 + mobility_score;
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
// returns (mg, eg, mobility_score, king_attack_units) objective positional score modifications
// in a single pass. king_attack_units is raw (not yet power-scaled) and always non-negative -
// callers sum it across pieces before applying the nonlinear king safety scaling once per side.
pub fn bb_to_posmod(
    bb: u64,
    piece_type: u8,
    to_move: bool,
    board: &ChessBoard,
    enemy_king_sq: usize,
    enemy_king_zone: u64,
) -> (i16, i16, i16, i16) {
    let mg_mask: &[i8; 64];
    let eg_mask: &[i8; 64];
    let mobility_factor: i16;
    match piece_type {
        0 => {
            mg_mask = &MG_PAWN_MOD;
            eg_mask = &EG_PAWN_MOD;
            mobility_factor = 0;
        }
        1 => {
            mg_mask = &MG_KNIGHT_MOD;
            eg_mask = &EG_KNIGHT_MOD;
            mobility_factor = MOBILITY_BONUS[0];
        }
        2 => {
            mg_mask = &MG_BISHOP_MOD;
            eg_mask = &EG_BISHOP_MOD;
            mobility_factor = MOBILITY_BONUS[1];
        }
        3 => {
            mg_mask = &MG_ROOK_MOD;
            eg_mask = &EG_ROOK_MOD;
            mobility_factor = MOBILITY_BONUS[2];
        }
        4 => {
            mg_mask = &MG_QUEEN_MOD;
            eg_mask = &EG_QUEEN_MOD;
            mobility_factor = MOBILITY_BONUS[3];
        }
        5 => {
            mg_mask = &MG_KING_MOD;
            eg_mask = &EG_KING_MOD;
            mobility_factor = 0;
        }
        _ => panic!("unexpected piece type value: {}", piece_type),
    }

    let mut mg_modifier: i16 = 0;
    let mut eg_modifier: i16 = 0;
    let mut mobility_score: i16 = 0;
    let mut king_attack_units: i16 = 0;
    let mut mbb: u64 = bb;
    if to_move {
        if piece_type == 0 {
            // O(1): AND the whole pawn bitboard against a precomputed mask instead of
            // generating attacks per pawn - see KING_ZONE_PAWN_ATTACKERS.
            king_attack_units += (bb & KING_ZONE_PAWN_ATTACKERS[0][enemy_king_sq]).count_ones()
                as i16
                * KING_ATTACK_WEIGHT[0];
        }
        while mbb != 0 {
            let i = mbb.trailing_zeros() as usize;
            mbb &= mbb - 1;
            mg_modifier += mg_mask[i] as i16;
            eg_modifier += eg_mask[i] as i16;
            if mobility_factor != 0 {
                let attacks: u64 = match piece_type {
                    1 => knight_attacks(true, 1 << i, board),
                    2 => get_bishop_attacks(board, true, i as u8),
                    3 => get_rook_attacks(board, true, i as u8),
                    4 => get_queen_attacks(board, true, i as u8),
                    _ => 0,
                };
                mobility_score += attacks.count_ones() as i16 * mobility_factor;
                king_attack_units += (attacks & enemy_king_zone).count_ones() as i16
                    * KING_ATTACK_WEIGHT[piece_type as usize];
            }
            if piece_type == 0 {
                let rank = oxi_chess_lib::utils::rank_value(1u64 << i);
                // rank 7 is always passed (no pawn can ever reach rank 8 to block it) and its
                // bonus is baked into MG_PAWN_MOD/EG_PAWN_MOD's rank 7 row instead.
                if rank < 7 && pawn_passed(i as u8, true, board) {
                    let idx = (rank - 2) as usize;
                    mg_modifier += MG_PASSED_PAWN_BONUS[idx];
                    eg_modifier += EG_PASSED_PAWN_BONUS[idx];
                }
                if pawn_isolated(i as u8, true, board) {
                    mg_modifier -= MG_ISOLATED_PAWN_PENALTY;
                    eg_modifier -= EG_ISOLATED_PAWN_PENALTY;
                } else if pawn_backward(i as u8, true, board) {
                    mg_modifier -= MG_BACKWARD_PAWN_PENALTY;
                    eg_modifier -= EG_BACKWARD_PAWN_PENALTY;
                }
            }
            if piece_type == 3 {
                let file_i = (oxi_chess_lib::utils::file_value(1u64 << i) - 1) as usize;
                if FILE_MASK[file_i] & board.pawns == 0 {
                    mg_modifier += OPEN_FILE_ROOK;
                } else if FILE_MASK[file_i] & (board.pawns & board.white_pieces) == 0 {
                    mg_modifier += SEMIOPEN_FILE_ROOK;
                }
            }
            if piece_type == 5 {
                mg_modifier += pawn_shield_score(i as u8, true, board);
            }
        }
    } else {
        if piece_type == 0 {
            // O(1): AND the whole pawn bitboard against a precomputed mask instead of
            // generating attacks per pawn - see KING_ZONE_PAWN_ATTACKERS.
            king_attack_units += (bb & KING_ZONE_PAWN_ATTACKERS[1][enemy_king_sq]).count_ones()
                as i16
                * KING_ATTACK_WEIGHT[0];
        }
        while mbb != 0 {
            let i = mbb.trailing_zeros() as usize;
            mbb &= mbb - 1;
            mg_modifier -= mg_mask[63 - i] as i16;
            eg_modifier -= eg_mask[63 - i] as i16;
            if mobility_factor != 0 {
                let attacks: u64 = match piece_type {
                    1 => knight_attacks(false, 1 << i, board),
                    2 => get_bishop_attacks(board, false, i as u8),
                    3 => get_rook_attacks(board, false, i as u8),
                    4 => get_queen_attacks(board, false, i as u8),
                    _ => 0,
                };
                mobility_score -= attacks.count_ones() as i16 * mobility_factor;
                king_attack_units += (attacks & enemy_king_zone).count_ones() as i16
                    * KING_ATTACK_WEIGHT[piece_type as usize];
            }
            if piece_type == 0 {
                let rank = oxi_chess_lib::utils::rank_value(1u64 << i);
                // rank 2 is always passed (no pawn can ever reach rank 1 to block it) and its
                // bonus is baked into MG_PAWN_MOD/EG_PAWN_MOD's rank 7 row instead.
                if rank > 2 && pawn_passed(i as u8, false, board) {
                    let idx = (7 - rank) as usize;
                    mg_modifier -= MG_PASSED_PAWN_BONUS[idx];
                    eg_modifier -= EG_PASSED_PAWN_BONUS[idx];
                }
                if pawn_isolated(i as u8, false, board) {
                    mg_modifier += MG_ISOLATED_PAWN_PENALTY;
                    eg_modifier += EG_ISOLATED_PAWN_PENALTY;
                } else if pawn_backward(i as u8, false, board) {
                    mg_modifier += MG_BACKWARD_PAWN_PENALTY;
                    eg_modifier += EG_BACKWARD_PAWN_PENALTY;
                }
            }
            if piece_type == 3 {
                let file_i = (oxi_chess_lib::utils::file_value(1u64 << i) - 1) as usize;
                if FILE_MASK[file_i] & board.pawns == 0 {
                    mg_modifier -= OPEN_FILE_ROOK;
                } else if FILE_MASK[file_i] & (board.pawns & board.black_pieces) == 0 {
                    mg_modifier -= SEMIOPEN_FILE_ROOK;
                }
            }
            if piece_type == 5 {
                mg_modifier -= pawn_shield_score(i as u8, false, board);
            }
        }
    }

    return (mg_modifier, eg_modifier, mobility_score, king_attack_units);
}

// returns score from the perspective of the side currently to move (positive = good for them).
pub fn relative_evaluate(game: &oxi_chess_lib::game::ChessGame) -> i16 {
    if game.board.side_to_move {
        return evaluate(game);
    } else {
        return -evaluate(game);
    }
}

// FILE_MASK[0] = a file.
pub const FILE_MASK: [u64; 8] = [
    0x0101010101010101,
    0x0202020202020202,
    0x0404040404040404,
    0x0808080808080808,
    0x1010101010101010,
    0x2020202020202020,
    0x4040404040404040,
    0x8080808080808080,
];

pub const RANK_MASK: [u64; 8] = [
    0x00000000000000FF,
    0x000000000000FF00,
    0x0000000000FF0000,
    0x00000000FF000000,
    0x000000FF00000000,
    0x0000FF0000000000,
    0x00FF000000000000,
    0xFF00000000000000,
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
        let mut pv: ArrayVec<u16, MAX_PV_LEN> = ArrayVec::new();
        let best_move_uci = oxi_chess_lib::utils::decode_to_uci(
            negamax(
                &mut game,
                1,
                -i16::MAX,
                i16::MAX,
                Arc::new(SearchState::new()),
                false,
                false,
                MAX_QDEPTH,
                &mut tt,
                &mut search::KillerTable::new(),
                0,
                0,
                &mut RootBest::new(0),
                &mut pv,
                true,
            )
            .1,
        )
        .unwrap();
        assert_eq!(best_move_uci, "h7h8q".to_string());

        // White Qb7# or Qa7# (mate in 1): Ka6, Qb6 vs Ka8
        let mut game = oxi_chess_lib::game::ChessGame::initialize(
            (1, 1),
            Some("k7/8/KQ6/8/8/8/8/8 w - - 0 1"),
        );
        let mut tt = TT::new(128);
        let mut pv: ArrayVec<u16, MAX_PV_LEN> = ArrayVec::new();
        let best_move_uci = oxi_chess_lib::utils::decode_to_uci(
            negamax(
                &mut game,
                1,
                -i16::MAX,
                i16::MAX,
                Arc::new(SearchState::new()),
                false,
                false,
                MAX_QDEPTH,
                &mut tt,
                &mut search::KillerTable::new(),
                0,
                0,
                &mut RootBest::new(0),
                &mut pv,
                true,
            )
            .1,
        )
        .unwrap();
        assert!(best_move_uci == "b6b7".to_string() || best_move_uci == "b6a7".to_string());

        // White Rxd5: captures undefended black queen
        let mut game = oxi_chess_lib::game::ChessGame::initialize(
            (1, 1),
            Some("k7/8/8/3q4/8/8/8/K2R4 w - - 0 1"),
        );
        let mut tt = TT::new(128);
        let mut pv: ArrayVec<u16, MAX_PV_LEN> = ArrayVec::new();
        let best_move_uci = oxi_chess_lib::utils::decode_to_uci(
            negamax(
                &mut game,
                1,
                -i16::MAX,
                i16::MAX,
                Arc::new(SearchState::new()),
                false,
                false,
                MAX_QDEPTH,
                &mut tt,
                &mut search::KillerTable::new(),
                0,
                0,
                &mut RootBest::new(0),
                &mut pv,
                true,
            )
            .1,
        )
        .unwrap();
        assert_eq!(best_move_uci, "d1d5".to_string());

        // Black Rxd1: captures undefended white rook
        let mut game = oxi_chess_lib::game::ChessGame::initialize(
            (1, 1),
            Some("k2r4/8/8/8/8/8/8/K2R4 b - - 0 1"),
        );
        let mut tt = TT::new(128);
        let mut pv: ArrayVec<u16, MAX_PV_LEN> = ArrayVec::new();
        let best_move_uci = oxi_chess_lib::utils::decode_to_uci(
            negamax(
                &mut game,
                1,
                -i16::MAX,
                i16::MAX,
                Arc::new(SearchState::new()),
                false,
                false,
                MAX_QDEPTH,
                &mut tt,
                &mut search::KillerTable::new(),
                0,
                0,
                &mut RootBest::new(0),
                &mut pv,
                true,
            )
            .1,
        )
        .unwrap();
        assert_eq!(best_move_uci, "d8d1".to_string());
    }
}

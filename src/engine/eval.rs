use crate::engine::eval_heuristics::*;
use crate::engine::search::{self, MAX_PV_LEN, RootBest, SearchState, TT, negamax};
use crate::engine::search_heuristics::{MATE_THRESHOLD, MAX_QDEPTH};
use crate::engine::utils::{
    KING_ZONE_MASKS, KING_ZONE_PAWN_ATTACKERS, king_file_weakness_mult, pawn_backward,
    pawn_isolated, pawn_passed, pawn_shield_score, pv_to_uci,
};
use arrayvec::ArrayVec;
use oxi_chess_lib::board::ChessBoard;
use oxi_chess_lib::game::ChessGame;
use oxi_chess_lib::moves::{
    get_bishop_attacks, get_queen_attacks, get_rook_attacks, knight_attacks,
};
use oxi_chess_lib::utils::{file_value, rank_value};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::time::Instant;

// nodes: shared running node total. depth_reported: highest depth reported so far (one thread per depth).
pub fn iteratively_deepen(
    game: &mut ChessGame,
    max_depth: u8,
    state: Arc<SearchState>,
    tt: &mut TT,
    nodes: &AtomicU64,
    depth_reported: &AtomicU8,
    mt_suggested: Option<u32>,
) -> RootBest {
    println!("info string start eval {}", evaluate(&game.board));
    let start_time = Instant::now();
    let age = (game.moves.len() / 2) as u8; // age for transposition table entries
    let mut best = RootBest::new(game.legal_moves[0]); // fallback if stop hits immediately
    let mut killers = search::KillerTable::new(); // accumulates across all depths, not reset per-depth
    let search_time_multiplier: f32 = 1.0; // scales mt_suggested - see TODO below

    for d in 1..=max_depth {
        if state.stop.load(Ordering::Relaxed) {
            return best;
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
            nodes.fetch_add(dnodes, Ordering::Relaxed);
            // best is authoritative regardless of interruption - negamax publishes it incrementally.
            // CAS-claim: only the first thread to finish depth d reports it.
            let mut last_reported = depth_reported.load(Ordering::Relaxed);
            let won_report = !state.stop.load(Ordering::Relaxed)
                && loop {
                    if d <= last_reported {
                        break false;
                    }
                    match depth_reported.compare_exchange_weak(
                        last_reported,
                        d,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    ) {
                        Ok(_) => break true,
                        Err(actual) => last_reported = actual,
                    }
                };
            if won_report {
                let elapsed = start_time.elapsed().as_millis().max(1);
                let total_nodes = nodes.load(Ordering::Relaxed);
                let nps = ((total_nodes * 1000) as u128) / elapsed;
                let (score, reported_depth) = (best.best_score, best.best_depth);
                let pv_uci = pv_to_uci(&best.pv, best.best_move);
                println!(
                    "info depth {reported_depth} score cp {score} nodes {total_nodes} nps {nps} time {elapsed} pv {pv_uci}"
                );
            }
            // a mate proven within the fully-searched depth (not check-extension/quiescence) is
            // trustworthy - every root move was compared at this depth, nothing deeper can improve it.
            if best.best_score.abs() > MATE_THRESHOLD && 10000 - best.best_score.abs() <= d as i16 {
                state.stop.store(true, Ordering::Relaxed);
            } else if let Some(budget) = mt_suggested {
                // TODO: adjust search_time_multiplier here based on accuracy-pressure
                // heuristics (tactical density, positional tension, eval variance).
                if start_time.elapsed().as_millis() as f32 >= budget as f32 * search_time_multiplier
                {
                    state.stop.store(true, Ordering::Relaxed);
                }
            }
            game.legal_moves = legal_moves; // restore legal_moves
        }
    }
    best
}

// returns objective material count
fn count_material(board: &ChessBoard, pawns: bool, total: bool) -> i16 {
    let mut material: i16 = 0;

    if pawns {
        material += PIECE_VALUES[0] * (board.pawns & board.white_pieces).count_ones() as i16;
        let bpawns = PIECE_VALUES[0] * (board.pawns & board.black_pieces).count_ones() as i16;
        if total {
            material += bpawns;
        } else {
            material -= bpawns;
        }
    }
    material += PIECE_VALUES[1] * (board.knights & board.white_pieces).count_ones() as i16;
    material += PIECE_VALUES[2] * (board.bishops & board.white_pieces).count_ones() as i16;
    material += PIECE_VALUES[3] * (board.rooks & board.white_pieces).count_ones() as i16;
    material += PIECE_VALUES[4] * (board.queens & board.white_pieces).count_ones() as i16;

    let mut b_material = 0;
    b_material += PIECE_VALUES[1] * (board.knights & board.black_pieces).count_ones() as i16;
    b_material += PIECE_VALUES[2] * (board.bishops & board.black_pieces).count_ones() as i16;
    b_material += PIECE_VALUES[3] * (board.rooks & board.black_pieces).count_ones() as i16;
    b_material += PIECE_VALUES[4] * (board.queens & board.black_pieces).count_ones() as i16;
    if total {
        material += b_material;
    } else {
        material -= b_material;
    }

    material
}

// objective static evaluation - no lookahead, use negamax for that.
pub fn evaluate(board: &ChessBoard) -> i16 {
    let mut eval: i16 = 0;
    let net_material = count_material(board, true, false);
    let total_material = count_material(board, true, true);
    eval += net_material;
    eval += positional_mods(board, net_material, total_material);
    eval
}

// value tuned based on fastchess match results.
const C: f32 = 15.0;

// returns objective positional modifications to score.
pub fn positional_mods(board: &ChessBoard, net_material: i16, total_material: i16) -> i16 {
    let major_piece_count = count_material(board, false, true);

    let w_bbs: [u64; 6] = [
        board.pawns & board.white_pieces,
        board.knights & board.white_pieces,
        board.bishops & board.white_pieces,
        board.rooks & board.white_pieces,
        board.queens & board.white_pieces,
        board.kings & board.white_pieces,
    ];

    let b_bbs: [u64; 6] = [
        board.pawns & board.black_pieces,
        board.knights & board.black_pieces,
        board.bishops & board.black_pieces,
        board.rooks & board.black_pieces,
        board.queens & board.black_pieces,
        board.kings & board.black_pieces,
    ];

    let mut mg_modifier: i16 = 0;
    let mut eg_modifier: i16 = 0;
    let mut mobility_score: i16 = 0;
    // raw attack units per side, summed across pieces before the nonlinear scaling below.
    let mut w_king_attack_units: i16 = 0;
    let mut b_king_attack_units: i16 = 0;

    // fixed for the whole position - compute once rather than per bb_to_posmod call.
    let w_king_sq = (board.kings & board.white_pieces).trailing_zeros() as usize;
    let b_king_sq = (board.kings & board.black_pieces).trailing_zeros() as usize;
    let w_king_zone = KING_ZONE_MASKS[w_king_sq];
    let b_king_zone = KING_ZONE_MASKS[b_king_sq];

    for i in 0..6 {
        let (w_mg, w_eg, w_ms, w_au) =
            bb_to_posmod(w_bbs[i], i as u8, true, board, b_king_sq, b_king_zone);
        let (b_mg, b_eg, b_ms, b_au) =
            bb_to_posmod(b_bbs[i], i as u8, false, board, w_king_sq, w_king_zone);
        mg_modifier += w_mg + b_mg;
        eg_modifier += w_eg + b_eg;
        mobility_score += w_ms + b_ms;
        w_king_attack_units += w_au;
        b_king_attack_units += b_au;
    }

    // king safety: nonlinear scaling of raw attack units, clamped to avoid unbounded swings.
    // mg-only - fades out via phase weighting below.
    let w_units = w_king_attack_units.min(MAX_KING_ATTACK_UNITS) as f32;
    let b_units = b_king_attack_units.min(MAX_KING_ATTACK_UNITS) as f32;
    let w_base_danger = KING_ATTACK_LINEAR * w_units + KING_ATTACK_QUADRATIC * w_units * w_units;
    let b_base_danger = KING_ATTACK_LINEAR * b_units + KING_ATTACK_QUADRATIC * b_units * b_units;
    // open/semi-open files amplify existing pressure rather than adding danger on their own.
    let w_king_danger =
        (w_base_danger * king_file_weakness_mult(b_king_sq as u8, false, board)).round() as i16;
    let b_king_danger =
        (b_base_danger * king_file_weakness_mult(w_king_sq as u8, true, board)).round() as i16;
    mg_modifier += w_king_danger - b_king_danger;

    // give a bonus for bishop pair
    let w_bishops = (board.bishops & board.white_pieces).count_ones();
    let b_bishops = (board.bishops & board.black_pieces).count_ones();
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
    for file_mask in FILE_MASK {
        wdb_pawns_count +=
            ((file_mask & board.white_pieces & board.pawns).count_ones() as i16 - 1).max(0);
        bdb_pawns_count +=
            ((file_mask & board.black_pieces & board.pawns).count_ones() as i16 - 1).max(0);
    }
    mg_modifier -= (wdb_pawns_count - bdb_pawns_count) * MG_DOUBLED_PAWN_PENALTY;
    eg_modifier -= (wdb_pawns_count - bdb_pawns_count) * EG_DOUBLED_PAWN_PENALTY;

    let mg_frac = major_piece_count as f32 / MG_PHASE_SPAN as f32;
    let mg_weighted = mg_frac * mg_modifier as f32;
    let eg_weighted = (1.0 - mg_frac) * eg_modifier as f32;

    // trade incentive: down material -> keep total material high; up material -> keep it low.
    let trading_incentive =
        (100.0 * C * (net_material as f32 / total_material as f32)).round() as i16;

    // give a penalty for early queen moves.
    let mut early_queen_mod: i16 = 0;
    if board.fullmove_number <= 6 {
        let w_queen_home: bool = (board.queens & board.white_pieces) & 0x0000000000000008 != 0;
        let b_queen_home: bool = (board.queens & board.black_pieces) & 0x0800000000000000 != 0;
        if !w_queen_home {
            early_queen_mod -= ((7 - board.fullmove_number) as i16) * EARLY_QUEEN_FACTOR;
        }
        if !b_queen_home {
            early_queen_mod += ((7 - board.fullmove_number) as i16) * EARLY_QUEEN_FACTOR;
        }
    }

    let mut result = (mg_weighted + eg_weighted).round() as i16 + mobility_score;
    result += trading_incentive;
    result += early_queen_mod;
    if board.side_to_move {
        result += TEMPO_BONUS;
    } else {
        result -= TEMPO_BONUS;
    }

    result
}

// piece_type 0-5 = pawn, knight, bishop, rook, queen, king.
// returns (mg, eg, mobility_score, king_attack_units) for one piece type, one side.
// king_attack_units is raw (unscaled) - callers apply the nonlinear scaling once per side.
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
    let occupancy = board.white_pieces | board.black_pieces;
    if to_move {
        if piece_type == 0 {
            // O(1) via precomputed mask instead of generating attacks per pawn.
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
                    1 => knight_attacks(1 << i, board.white_pieces),
                    2 => get_bishop_attacks(i as u8, board.white_pieces, occupancy),
                    3 => get_rook_attacks(i as u8, board.white_pieces, occupancy),
                    4 => get_queen_attacks(i as u8, board.white_pieces, occupancy),
                    _ => 0,
                };
                mobility_score += attacks.count_ones() as i16 * mobility_factor;
                king_attack_units += (attacks & enemy_king_zone).count_ones() as i16
                    * KING_ATTACK_WEIGHT[piece_type as usize];
            }
            if piece_type == 0 {
                let rank = rank_value(1u64 << i);
                // rank 7 is always passed - baked into MG_PAWN_MOD/EG_PAWN_MOD's rank 7 row instead.
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
                let file_i = (file_value(1u64 << i) - 1) as usize;
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
            // O(1) via precomputed mask instead of generating attacks per pawn.
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
                    1 => knight_attacks(1 << i, board.black_pieces),
                    2 => get_bishop_attacks(i as u8, board.black_pieces, occupancy),
                    3 => get_rook_attacks(i as u8, board.black_pieces, occupancy),
                    4 => get_queen_attacks(i as u8, board.black_pieces, occupancy),
                    _ => 0,
                };
                mobility_score -= attacks.count_ones() as i16 * mobility_factor;
                king_attack_units += (attacks & enemy_king_zone).count_ones() as i16
                    * KING_ATTACK_WEIGHT[piece_type as usize];
            }
            if piece_type == 0 {
                let rank = rank_value(1u64 << i);
                // rank 2 is always passed - baked into MG_PAWN_MOD/EG_PAWN_MOD's rank 7 row instead.
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
                let file_i = (file_value(1u64 << i) - 1) as usize;
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

    (mg_modifier, eg_modifier, mobility_score, king_attack_units)
}

// returns score from the perspective of the side currently to move (positive = good for them).
pub fn relative_evaluate(board: &ChessBoard) -> i16 {
    if board.side_to_move {
        evaluate(board)
    } else {
        -evaluate(board)
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
    use oxi_chess_lib::utils::decode_to_uci;

    #[test]
    fn test_best_move() {
        // White promotes pawn to queen
        let mut game = ChessGame::initialize(Some("k7/7P/8/8/8/8/8/K7 w - - 0 1"));
        let mut tt = TT::new(128);
        let mut pv: ArrayVec<u16, MAX_PV_LEN> = ArrayVec::new();
        let best_move_uci = decode_to_uci(
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
        let mut game = ChessGame::initialize(Some("k7/8/KQ6/8/8/8/8/8 w - - 0 1"));
        let mut tt = TT::new(128);
        let mut pv: ArrayVec<u16, MAX_PV_LEN> = ArrayVec::new();
        let best_move_uci = decode_to_uci(
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
        assert!(best_move_uci == "b6b7" || best_move_uci == "b6a7");

        // White Rxd5: captures undefended black queen
        let mut game = ChessGame::initialize(Some("k7/8/8/3q4/8/8/8/K2R4 w - - 0 1"));
        let mut tt = TT::new(128);
        let mut pv: ArrayVec<u16, MAX_PV_LEN> = ArrayVec::new();
        let best_move_uci = decode_to_uci(
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
        let mut game = ChessGame::initialize(Some("k2r4/8/8/8/8/8/8/K2R4 b - - 0 1"));
        let mut tt = TT::new(128);
        let mut pv: ArrayVec<u16, MAX_PV_LEN> = ArrayVec::new();
        let best_move_uci = decode_to_uci(
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

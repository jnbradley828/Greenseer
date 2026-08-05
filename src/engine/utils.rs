use std::sync::atomic::Ordering;

use arrayvec::ArrayVec;
use oxi_chess_lib::board::ChessBoard;
use oxi_chess_lib::moves::{BLACK_PAWN_ATTACKS, WHITE_PAWN_ATTACKS, square_attacked};
use oxi_chess_lib::utils::{decode_move, decode_to_uci, file_value, rank_value};

use crate::engine::eval::{FILE_MASK, RANK_MASK};
use crate::engine::eval_heuristics::{
    KING_OPEN_FILE_MULT, KING_SEMIOPEN_FILE_MULT, MG_PAWN_SHIELD_BONUS, TT_AGE_FACTOR,
};
use crate::engine::search::{MAX_PV_LEN, TT};
use crate::engine::search_heuristics::MATE_THRESHOLD;

// tt slot: (full zobrist key, packed data). data (msb->lsb): score:16, depth:6, flag:2, best_move:16, age:8.
pub const TT_EXACT_FLAG: u8 = 0;
pub const TT_LOWERB_FLAG: u8 = 1;
pub const TT_UPPERB_FLAG: u8 = 2;
pub const PASSED_PAWN_MASKS: [[u64; 64]; 2] = generate_passed_pawns_masks();

// returns a mask for the squares to check for opposing pawns to see if your pawn is passed.
const fn passed_pawn_mask(color: bool, sq_i: u8) -> u64 {
    let sq: u64 = 1 << sq_i;
    let file = file_value(sq);
    let rank = rank_value(sq);

    let file_mask: u64 = match file {
        1 => FILE_MASK[0] | FILE_MASK[1],
        2 => FILE_MASK[0] | FILE_MASK[1] | FILE_MASK[2],
        3 => FILE_MASK[1] | FILE_MASK[2] | FILE_MASK[3],
        4 => FILE_MASK[2] | FILE_MASK[3] | FILE_MASK[4],
        5 => FILE_MASK[3] | FILE_MASK[4] | FILE_MASK[5],
        6 => FILE_MASK[4] | FILE_MASK[5] | FILE_MASK[6],
        7 => FILE_MASK[5] | FILE_MASK[6] | FILE_MASK[7],
        8 => FILE_MASK[6] | FILE_MASK[7],
        _ => 0,
    };

    let mut rank_mask: u64 = 0;
    if color {
        let mut rank = rank;
        while rank < 8 {
            rank_mask |= RANK_MASK[rank as usize];
            rank += 1;
        }
    } else {
        let max_rank = rank - 1;
        let mut rank = 1;
        while rank < max_rank {
            rank_mask |= RANK_MASK[rank as usize];
            rank += 1;
        }
    }

    file_mask & rank_mask
}

const fn generate_passed_pawns_masks() -> [[u64; 64]; 2] {
    let mut passed_pawn_masks = [[0u64; 64]; 2];

    let mut sq = 0;
    while sq < 64 {
        passed_pawn_masks[0][sq as usize] = passed_pawn_mask(false, sq);
        passed_pawn_masks[1][sq as usize] = passed_pawn_mask(true, sq);
        sq += 1;
    }

    passed_pawn_masks
}

pub fn pawn_passed(sq_i: u8, color: bool, board: &ChessBoard) -> bool {
    let passer_defender_mask: u64 = if color {
        PASSED_PAWN_MASKS[1][sq_i as usize]
    } else {
        PASSED_PAWN_MASKS[0][sq_i as usize]
    };
    let opponent_pawns: u64 = if color {
        board.black_pieces & board.pawns
    } else {
        board.white_pieces & board.pawns
    };
    passer_defender_mask & opponent_pawns == 0
}

// mask of the two adjacent files (excludes own file) - used for isolated pawn detection.
pub const ADJACENT_FILE_MASKS: [u64; 8] = generate_adjacent_file_masks();

const fn generate_adjacent_file_masks() -> [u64; 8] {
    let mut masks = [0u64; 8];
    let mut file = 0;
    while file < 8 {
        let mut mask = 0u64;
        if file > 0 {
            mask |= FILE_MASK[file - 1];
        }
        if file < 7 {
            mask |= FILE_MASK[file + 1];
        }
        masks[file] = mask;
        file += 1;
    }
    masks
}

pub fn pawn_isolated(sq_i: u8, color: bool, board: &ChessBoard) -> bool {
    let file_i = (file_value(1u64 << sq_i) - 1) as usize;
    let friendly_pawns: u64 = if color {
        board.white_pieces & board.pawns
    } else {
        board.black_pieces & board.pawns
    };
    ADJACENT_FILE_MASKS[file_i] & friendly_pawns == 0
}

// squares (adjacent files, this rank or further back) a friendly pawn needs to defend the stop square.
pub const BACKWARD_PAWN_MASKS: [[u64; 64]; 2] = generate_backward_pawn_masks();

const fn backward_pawn_mask(color: bool, sq_i: u8) -> u64 {
    let sq: u64 = 1 << sq_i;
    let file_i = (file_value(sq) - 1) as usize;
    let rank = rank_value(sq);

    let mut rank_mask: u64 = 0;
    if color {
        // white: a defender must be on this rank or below.
        let mut r = 1;
        while r <= rank {
            rank_mask |= RANK_MASK[(r - 1) as usize];
            r += 1;
        }
    } else {
        // black: a defender must be on this rank or above.
        let mut r = rank;
        while r <= 8 {
            rank_mask |= RANK_MASK[(r - 1) as usize];
            r += 1;
        }
    }

    ADJACENT_FILE_MASKS[file_i] & rank_mask
}

const fn generate_backward_pawn_masks() -> [[u64; 64]; 2] {
    let mut masks = [[0u64; 64]; 2];
    let mut sq = 0;
    while sq < 64 {
        masks[0][sq as usize] = backward_pawn_mask(false, sq);
        masks[1][sq as usize] = backward_pawn_mask(true, sq);
        sq += 1;
    }
    masks
}

// only meaningful for pawns that aren't isolated - callers should skip this check otherwise.
pub fn pawn_backward(sq_i: u8, color: bool, board: &ChessBoard) -> bool {
    let friendly_pawns: u64 = if color {
        board.white_pieces & board.pawns
    } else {
        board.black_pieces & board.pawns
    };
    let support_mask: u64 = if color {
        BACKWARD_PAWN_MASKS[1][sq_i as usize]
    } else {
        BACKWARD_PAWN_MASKS[0][sq_i as usize]
    };
    if support_mask & friendly_pawns != 0 {
        return false;
    }

    // same-color attack table at the stop square mirrors the squares an opposing pawn would need.
    let stop_sq_i = if color { sq_i + 8 } else { sq_i - 8 };
    let attacker_mask: u64 = if color {
        WHITE_PAWN_ATTACKS[stop_sq_i as usize]
    } else {
        BLACK_PAWN_ATTACKS[stop_sq_i as usize]
    };
    let opponent_pawns: u64 = if color {
        board.black_pieces & board.pawns
    } else {
        board.white_pieces & board.pawns
    };
    attacker_mask & opponent_pawns != 0
}

// would this move give check, without making it - distinct from rules::is_check (current position).
pub fn move_gives_check(board: &ChessBoard, mv: u16) -> bool {
    let king_sq: u64 = if board.side_to_move {
        board.kings & board.black_pieces
    } else {
        board.kings & board.white_pieces
    };
    let [from_sqi, to_sqi, _] = decode_move(mv);
    square_attacked(
        board.side_to_move,
        king_sq,
        board,
        Some(1 << from_sqi),
        Some(1 << to_sqi),
    )
}

// flags 1/3/8/9/10/11 are the capture move-type flags (capture, en passant, promo-captures).
pub fn is_capture(mv: u16) -> bool {
    [1, 3, 8, 9, 10, 11].contains(&decode_move(mv)[2])
}

// formats a pv line for UCI output, falling back to just the best move if pv is empty.
pub fn pv_to_uci(pv: &ArrayVec<u16, MAX_PV_LEN>, best_move: u16) -> String {
    if pv.is_empty() {
        decode_to_uci(best_move).unwrap()
    } else {
        pv.iter()
            .map(|&mv| decode_to_uci(mv).unwrap())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

// king's square plus up to 8 surrounding squares - for king safety attack detection.
pub const KING_ZONE_MASKS: [u64; 64] = generate_king_zone_masks();

const fn king_zone_mask(sq_i: u8) -> u64 {
    let sq: u64 = 1 << sq_i;
    let file = file_value(sq) as i8; // 1-8
    let rank = rank_value(sq) as i8; // 1-8

    let mut mask: u64 = 0;
    let mut df = -1;
    while df <= 1 {
        let mut dr = -1;
        while dr <= 1 {
            let f = file + df;
            let r = rank + dr;
            if f >= 1 && f <= 8 && r >= 1 && r <= 8 {
                let idx = (r - 1) * 8 + (f - 1);
                mask |= 1u64 << idx;
            }
            dr += 1;
        }
        df += 1;
    }
    mask
}

const fn generate_king_zone_masks() -> [u64; 64] {
    let mut masks = [0u64; 64];
    let mut sq = 0;
    while sq < 64 {
        masks[sq as usize] = king_zone_mask(sq);
        sq += 1;
    }
    masks
}

// squares an enemy pawn needs to attack any square in a king's zone - lets the pawn king-safety
// check be a single AND + popcount instead of generating attacks per pawn per node.
pub const KING_ZONE_PAWN_ATTACKERS: [[u64; 64]; 2] = generate_king_zone_pawn_attacker_masks();

const fn king_zone_pawn_attacker_mask(color: bool, king_sq_i: u8) -> u64 {
    let mut zone = KING_ZONE_MASKS[king_sq_i as usize];
    let mut mask: u64 = 0;
    while zone != 0 {
        let sq = zone.trailing_zeros() as usize;
        zone &= zone - 1;
        mask |= if color {
            WHITE_PAWN_ATTACKS[sq]
        } else {
            BLACK_PAWN_ATTACKS[sq]
        };
    }
    mask
}

const fn generate_king_zone_pawn_attacker_masks() -> [[u64; 64]; 2] {
    let mut masks = [[0u64; 64]; 2];
    let mut sq = 0;
    while sq < 64 {
        masks[0][sq as usize] = king_zone_pawn_attacker_mask(false, sq);
        masks[1][sq as usize] = king_zone_pawn_attacker_mask(true, sq);
        sq += 1;
    }
    masks
}

// sums MG_PAWN_SHIELD_BONUS over the king's file and both adjacent files.
pub fn pawn_shield_score(king_sq_i: u8, color: bool, board: &ChessBoard) -> i16 {
    let king_sq: u64 = 1 << king_sq_i;
    let king_file = file_value(king_sq) as i8; // 1-8
    let king_rank = rank_value(king_sq) as i8; // 1-8

    let friendly_pawns: u64 = if color {
        board.white_pieces & board.pawns
    } else {
        board.black_pieces & board.pawns
    };

    let mut score: i16 = 0;
    let mut file = king_file - 1;
    while file <= king_file + 1 {
        if (1..=8).contains(&file) {
            let file_pawns = FILE_MASK[(file - 1) as usize] & friendly_pawns;
            score += MG_PAWN_SHIELD_BONUS[shield_file_tier(file_pawns, king_rank, color)];
        }
        file += 1;
    }
    score
}

// multiplier on king-attack danger from open/semi-open files near the king - see KING_OPEN_FILE_MULT.
pub fn king_file_weakness_mult(king_sq_i: u8, color: bool, board: &ChessBoard) -> f32 {
    let king_sq: u64 = 1 << king_sq_i;
    let king_file = file_value(king_sq) as i8; // 1-8

    let friendly_pawns: u64 = if color {
        board.white_pieces & board.pawns
    } else {
        board.black_pieces & board.pawns
    };

    let mut mult: f32 = 1.0;
    let mut file = king_file - 1;
    while file <= king_file + 1 {
        if (1..=8).contains(&file) {
            let file_mask = FILE_MASK[(file - 1) as usize];
            if file_mask & board.pawns == 0 {
                mult += KING_OPEN_FILE_MULT;
            } else if file_mask & friendly_pawns == 0 {
                mult += KING_SEMIOPEN_FILE_MULT;
            }
        }
        file += 1;
    }
    mult
}

// tier 0 = pawn 1 rank ahead, tier 1 = 2 ranks ahead, tier 2 = further or missing.
fn shield_file_tier(file_pawns: u64, king_rank: i8, color: bool) -> usize {
    for dist in 1..=2i8 {
        let target_rank = if color {
            king_rank + dist
        } else {
            king_rank - dist
        };
        if !(1..=8).contains(&target_rank) {
            continue;
        }
        if RANK_MASK[(target_rank - 1) as usize] & file_pawns != 0 {
            return (dist - 1) as usize;
        }
    }
    2
}

// higher is more relevant - depth, penalized per move of staleness.
pub fn relevance_score(depth: u8, entry_age: u8, current_age: u8) -> i16 {
    depth as i16 - (current_age.wrapping_sub(entry_age) as i16) * TT_AGE_FACTOR
}

// root-relative mate score -> ply-independent score for tt storage.
pub fn to_tt_score(score: i16, ply: u8) -> i16 {
    if score > MATE_THRESHOLD {
        score + ply as i16
    } else if score < -MATE_THRESHOLD {
        score - ply as i16
    } else {
        score
    }
}

// ply-independent tt score -> root-relative score.
pub fn from_tt_score(score: i16, ply: u8) -> i16 {
    if score > MATE_THRESHOLD {
        score - ply as i16
    } else if score < -MATE_THRESHOLD {
        score + ply as i16
    } else {
        score
    }
}

pub fn encode_tt_entry(score: i16, depth: u8, flag: u8, best_move: u16, age: u8) -> u64 {
    let mut result: u64 = 0;

    result |= (score as u16 as u64) << 32;
    result |= (depth as u64) << 26;
    result |= (flag as u64) << 24;
    result |= (best_move as u64) << 8;
    result |= age as u64;

    result
}

pub fn decode_tt_entry(entry_value: u64) -> (i16, u8, u8, u16, u8) {
    let score = (entry_value >> 32) as i16;
    let depth = ((entry_value >> 26) as u8) & 0x3F;
    let flag = ((entry_value >> 24) as u8) & 0x3;
    let best_move = (entry_value >> 8) as u16;
    let age = entry_value as u8;

    (score, depth, flag, best_move, age)
}

pub fn update_tt(
    tt: &TT,
    zobrist_key: u64,
    score: i16,
    depth: u8,
    flag: u8,
    best_move: u16,
    age: u8,
) {
    let zobrist_index = zobrist_key % (tt.entries.len() as u64);
    let entry_value = encode_tt_entry(score, depth, flag, best_move, age);

    let existing_data = tt.entries[zobrist_index as usize].1.load(Ordering::Relaxed);
    let existing_entry = decode_tt_entry(existing_data);
    let existing_relevance = relevance_score(existing_entry.1, existing_entry.4, age);

    if existing_relevance == depth as i16 {
        // overwrite only if ours is exact and the existing entry isn't
        if flag == 0 && existing_entry.2 != 0 {
            tt.entries[zobrist_index as usize]
                .0
                .store(zobrist_key ^ entry_value, Ordering::Relaxed);
            tt.entries[zobrist_index as usize]
                .1
                .store(entry_value, Ordering::Relaxed);
        }
    } else if existing_relevance < depth as i16 {
        tt.entries[zobrist_index as usize]
            .0
            .store(zobrist_key ^ entry_value, Ordering::Relaxed);
        tt.entries[zobrist_index as usize]
            .1
            .store(entry_value, Ordering::Relaxed);
    }
}

// slot 0 stores zobrist_key ^ data - a torn read reconstructs to a bogus key (rejected as a miss).
pub fn retrieve_tt(tt: &TT, zobrist_key: u64) -> (u64, i16, u8, u8, u16, u8) {
    let zobrist_index = zobrist_key % (tt.entries.len() as u64);
    let stored_key_xor_data = tt.entries[zobrist_index as usize].0.load(Ordering::Relaxed);
    let tt_value = tt.entries[zobrist_index as usize].1.load(Ordering::Relaxed);
    let stored_key = stored_key_xor_data ^ tt_value;
    let (score, depth, flag, best_move, age) = decode_tt_entry(tt_value);
    (stored_key, score, depth, flag, best_move, age)
}

// returns None if zobrist keys don't match.
pub fn retrieve_tt_or_none(tt: &TT, zobrist_key: u64) -> Option<(u64, i16, u8, u8, u16, u8)> {
    let tt_entry = retrieve_tt(tt, zobrist_key);

    if tt_entry.0 != zobrist_key {
        None
    } else {
        Some(tt_entry)
    }
}

// does the side to move have any non-pawn pieces - used to detect zugzwang.
pub fn has_pieces(board: &ChessBoard) -> bool {
    let side_pieces = if board.side_to_move {
        board.white_pieces
    } else {
        board.black_pieces
    };

    (board.knights | board.bishops | board.rooks | board.queens) & side_pieces != 0
}

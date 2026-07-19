use std::sync::atomic::Ordering;

use crate::engine::search::TT;

// tt slot is (full 64-bit zobrist key, packed data word).
// packed data schema (from msb to lsb):
// score: 16 bits
// depth: 6 bits (maxes out at 64)
// flag: 2 bits (0 = exact, 1 = lower_bound, 2 = upper_bound)
// best move: 16 bits
// age: 8 bits (real move number this entry was written on, wraps at 256)
pub const TT_EXACT_FLAG: u8 = 0;
pub const TT_LOWERB_FLAG: u8 = 1;
pub const TT_UPPERB_FLAG: u8 = 2;
const TT_AGE_FACTOR: i16 = 2;
pub const MATE_THRESHOLD: i16 = 9000;

// higher is more relevant. depth alone when age matches (age_diff == 0); penalized per move of staleness otherwise.
pub fn relevance_score(depth: u8, entry_age: u8, current_age: u8) -> i16 {
    depth as i16 - (current_age.wrapping_sub(entry_age) as i16) * TT_AGE_FACTOR
}

// converts a root-relative mate score into a ply-independent (from this node) score before storing in the tt.
pub fn to_tt_score(score: i16, ply: u8) -> i16 {
    if score > MATE_THRESHOLD {
        score + ply as i16
    } else if score < -MATE_THRESHOLD {
        score - ply as i16
    } else {
        score
    }
}

// converts a ply-independent (from this node) mate score retrieved from the tt back into a root-relative score.
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

    return result;
}

pub fn decode_tt_entry(entry_value: u64) -> (i16, u8, u8, u16, u8) {
    let score = (entry_value >> 32) as i16;
    let depth = ((entry_value >> 26) as u8) & 0x3F;
    let flag = ((entry_value >> 24) as u8) & 0x3;
    let best_move = (entry_value >> 8) as u16;
    let age = entry_value as u8;

    return (score, depth, flag, best_move, age);
}

pub fn update_tt(
    tt: &mut TT,
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

    if existing_relevance > depth as i16 {
        return;
    } else if existing_relevance == depth as i16 {
        // if the existing entry is not exact but ours is
        if flag == 0 && existing_entry.2 != 0 {
            tt.entries[zobrist_index as usize]
                .0
                .store(zobrist_key, Ordering::Relaxed);
            tt.entries[zobrist_index as usize]
                .1
                .store(entry_value, Ordering::Relaxed);
        }
    } else {
        tt.entries[zobrist_index as usize]
            .0
            .store(zobrist_key, Ordering::Relaxed);
        tt.entries[zobrist_index as usize]
            .1
            .store(entry_value, Ordering::Relaxed);
    }
}

pub fn retrieve_tt(tt: &mut TT, zobrist_key: u64) -> (u64, i16, u8, u8, u16, u8) {
    let zobrist_index = zobrist_key % (tt.entries.len() as u64);
    let stored_key = tt.entries[zobrist_index as usize].0.load(Ordering::Relaxed);
    let tt_value = tt.entries[zobrist_index as usize].1.load(Ordering::Relaxed);
    let (score, depth, flag, best_move, age) = decode_tt_entry(tt_value);
    return (stored_key, score, depth, flag, best_move, age);
}

// returns None if zobrist keys don't match.
pub fn retrieve_tt_or_none(tt: &mut TT, zobrist_key: u64) -> Option<(u64, i16, u8, u8, u16, u8)> {
    let tt_entry = retrieve_tt(tt, zobrist_key);

    if tt_entry.0 != zobrist_key {
        return None;
    } else {
        return Some(tt_entry);
    }
}

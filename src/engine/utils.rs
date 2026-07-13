use std::sync::atomic::Ordering;

use crate::engine::search::TT;

// tt_entry schema (from msb to lsb):
// zobrist key: 16 bits (16 msb from zobrist key)
// score: 16 bits
// depth: 6 bits (maxes out at 64)
// flag: 2 bits (0 = exact, 1 = lower_bound, 2 = upper_bound)
// best move: 16 bits
// age: 8 bits (real move number this entry was written on, wraps at 256)
pub const TT_EXACT_FLAG: u8 = 0;
pub const TT_LOWERB_FLAG: u8 = 1;
pub const TT_UPPERB_FLAG: u8 = 2;
const TT_AGE_FACTOR: i16 = 2;

// higher is more relevant. depth alone when age matches (age_diff == 0); penalized per move of staleness otherwise.
pub fn relevance_score(depth: u8, entry_age: u8, current_age: u8) -> i16 {
    depth as i16 - (current_age.wrapping_sub(entry_age) as i16) * TT_AGE_FACTOR
}

pub fn encode_tt_entry(
    zobrist_key: u16,
    score: i16,
    depth: u8,
    flag: u8,
    best_move: u16,
    age: u8,
) -> u64 {
    let mut result: u64 = 0;

    result |= (zobrist_key as u64) << 48;
    result |= (score as u16 as u64) << 32;
    result |= (depth as u64) << 26;
    result |= (flag as u64) << 24;
    result |= (best_move as u64) << 8;
    result |= age as u64;

    return result;
}

pub fn decode_tt_entry(entry_value: u64) -> (u16, i16, u8, u8, u16, u8) {
    let zobrist_key = (entry_value >> 48) as u16;
    let score = (entry_value >> 32) as i16;
    let depth = ((entry_value >> 26) as u8) & 0x3F;
    let flag = ((entry_value >> 24) as u8) & 0x3;
    let best_move = (entry_value >> 8) as u16;
    let age = entry_value as u8;

    return (zobrist_key, score, depth, flag, best_move, age);
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
    let entry_value = encode_tt_entry(
        (zobrist_key >> 48) as u16,
        score,
        depth,
        flag,
        best_move,
        age,
    );

    let existing_value = tt.entries[zobrist_index as usize].load(Ordering::Relaxed);
    let existing_entry = decode_tt_entry(existing_value);
    let existing_relevance = relevance_score(existing_entry.2, existing_entry.5, age);

    if existing_relevance > depth as i16 {
        return;
    } else if existing_relevance == depth as i16 {
        // if the existing entry is not exact but ours is
        if flag == 0 && existing_entry.3 != 0 {
            tt.entries[zobrist_index as usize].store(entry_value, Ordering::Relaxed);
        }
    } else {
        tt.entries[zobrist_index as usize].store(entry_value, Ordering::Relaxed);
    }
}

pub fn retrieve_tt(tt: &mut TT, zobrist_key: u64) -> (u16, i16, u8, u8, u16, u8) {
    let zobrist_index = zobrist_key % (tt.entries.len() as u64);
    let tt_value = tt.entries[zobrist_index as usize].load(Ordering::Relaxed);
    return decode_tt_entry(tt_value);
}

// returns None if zobrist keys don't match.
pub fn retrieve_tt_or_none(tt: &mut TT, zobrist_key: u64) -> Option<(u16, i16, u8, u8, u16, u8)> {
    let tt_entry = retrieve_tt(tt, zobrist_key);

    let z16 = (zobrist_key >> 48) as u16;
    if tt_entry.0 != z16 {
        return None;
    } else {
        return Some(tt_entry);
    }
}

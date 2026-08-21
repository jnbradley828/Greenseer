use arrayvec::ArrayVec;
use oxi_chess_lib::{
    board::ChessBoard,
    game::{ChessGame, GameResult},
    moves::get_legal_moves,
    rules,
    utils::decode_move,
};

use crate::engine::eval::relative_evaluate;
use crate::engine::eval_heuristics::PIECE_VALUES;
use crate::engine::search_heuristics::{
    MATE_THRESHOLD, NMP_MIN_DEPTH, NMP_REDUCTION, RFP_MARGIN_BASE, RFP_MARGIN_PER_DEPTH,
    RFP_MAX_DEPTH, VICTIM_WEIGHT,
};
use crate::engine::utils::{
    TT_EXACT_FLAG, TT_LOWERB_FLAG, TT_UPPERB_FLAG, from_tt_score, has_pieces, is_capture,
    move_gives_check, retrieve_tt_or_none, to_tt_score, update_tt,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub struct SearchState {
    pub stop: AtomicBool,
    pub ponder: AtomicBool,
}
impl Default for SearchState {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchState {
    pub fn new() -> Self {
        Self {
            stop: AtomicBool::new(false),
            ponder: AtomicBool::new(false),
        }
    }
}

// bound on line length - matches negamax's depth/ply type (u8), so pv can't overflow.
pub const MAX_PV_LEN: usize = u8::MAX as usize;

// incrementally-published "best confirmed answer so far" at the root, valid even mid-search.
pub struct RootBest {
    pub best_move: u16,
    pub best_score: i16,
    pub best_depth: u8, // depth this result actually came from, may lag the nominal search depth
    pub pv: ArrayVec<u16, MAX_PV_LEN>, // may be shorter than best_depth - early returns truncate it
}
impl RootBest {
    pub fn new(fallback_move: u16) -> Self {
        Self {
            best_move: fallback_move,
            best_score: -i16::MAX,
            best_depth: 0,
            pv: ArrayVec::new(),
        }
    }
}

// each slot is (full zobrist key, packed data) - full key avoids false hits from a truncated tag.
#[derive(Clone)]
pub struct TT {
    pub entries: Arc<Vec<(AtomicU64, AtomicU64)>>,
}
impl TT {
    pub fn new(size_mb: usize) -> Self {
        let no_entries = (size_mb * 2_usize.pow(20)) / 16;
        let entry_vec = std::iter::repeat_with(|| (AtomicU64::new(0), AtomicU64::new(0)))
            .take(no_entries)
            .collect();
        Self {
            entries: Arc::new(entry_vec),
        }
    }

    // zeroes every slot in place, visible to all clones immediately.
    pub fn clear(&self) {
        for (key, data) in self.entries.iter() {
            key.store(0, Ordering::Relaxed);
            data.store(0, Ordering::Relaxed);
        }
    }
}

// 2 killer (quiet, cutoff-causing) moves per ply. 255 slots, not 256 - ply 0 (root) has no
// siblings so is never stored.
pub struct KillerTable([[Option<u16>; 2]; 255]);

impl Default for KillerTable {
    fn default() -> Self {
        Self::new()
    }
}

impl KillerTable {
    pub fn new() -> Self {
        Self([[None; 2]; 255])
    }

    // both killers at this ply (primary first). empty for ply == 0.
    pub fn get(&self, ply: u8) -> [Option<u16>; 2] {
        if ply == 0 {
            [None, None]
        } else {
            self.0[(ply - 1) as usize]
        }
    }

    // shifts primary killer to secondary. no-op for ply 0 or an already-primary move.
    pub fn store(&mut self, ply: u8, mv: u16) {
        if ply == 0 {
            return;
        }
        let slot = &mut self.0[(ply - 1) as usize];
        if slot[0] != Some(mv) {
            slot[1] = slot[0];
            slot[0] = Some(mv);
        }
    }
}
// negamax: score is always from the perspective of whoever is to move at that node. a call that
// makes a real move negates the child score and swaps (alpha, beta) to (-beta, -alpha); a call
// that doesn't (check-extension/quiescence dispatch) does neither.
// returns (eval, best move, nodes visited, completed). completed=false means a stop signal cut
// this subtree short - eval/move are meaningless sentinels, propagate the incompletion upward.
pub fn negamax(
    game: &mut ChessGame,
    depth: u8,
    alpha: i16,
    beta: i16,
    state: Arc<SearchState>,
    quiescence: bool,
    check_extension: bool,
    qdepth: u8,
    tt: &mut TT,
    killers: &mut KillerTable,
    ply: u8,
    age: u8,
    best: &mut RootBest,
    pv: &mut ArrayVec<u16, MAX_PV_LEN>,
    allow_null: bool,
) -> (i16, u16, u64, bool) {
    let mut nodes: u64 = 1;
    if game.result != GameResult::InProgress {
        if !matches!(game.result, GameResult::Draw(_)) {
            // checkmate: bad for the mover, discounted by ply to prefer faster mates.
            return (ply as i16 - 10000, 0, nodes, true);
        } else {
            return (0, 0, nodes, true);
        }
    }

    // skip tt on a repetition (reduces 3fold repetition in won positions)
    let mut skip_tt = false;
    if let Some(&count) = game.positions_count.get(&game.board.zobrist_hash)
        && count >= 2
    {
        skip_tt = true;
    }
    let mut tt_move: Option<u16> = None;
    if let Some(tt_entry) = retrieve_tt_or_none(tt, game.board.zobrist_hash)
        && !skip_tt
    {
        // flag: 0=exact returns immediately, 1=lower-bound returns if tt_score>=beta,
        // 2=upper-bound returns if tt_score<=alpha (either triggers a cutoff at the parent).
        if tt_entry.2 >= depth {
            let tt_score = from_tt_score(tt_entry.1, ply);
            // root only: publish before returning early, since a root tt hit skips the moves
            // loop entirely and best would otherwise never hear about it.
            let publish = |best: &mut RootBest| {
                if ply == 0 {
                    best.best_move = tt_entry.4;
                    best.best_score = tt_score;
                    best.best_depth = depth;
                    // one ply deeper can be stale - only this move is trustworthy, not a line.
                    best.pv.clear();
                    best.pv.push(tt_entry.4);
                }
            };
            match tt_entry.3 {
                0 => {
                    publish(best);
                    return (tt_score, tt_entry.4, nodes, true);
                }
                1 => {
                    if tt_score >= beta {
                        publish(best);
                        return (tt_score, tt_entry.4, nodes, true);
                    }
                }
                2 => {
                    if tt_score <= alpha {
                        publish(best);
                        return (tt_score, tt_entry.4, nodes, true);
                    }
                }
                _ => {}
            }
        }
        tt_move = Some(tt_entry.4);
    }
    // move generation/reordering deferred to depth != 0 below, right where it's consumed.

    if depth == 0 {
        // check extension: search 1 more ply. no move made, so no negation/swap.
        if rules::is_check(&game.board, game.board.side_to_move) {
            // same node conceptually - forward pv itself, not a child buffer.
            negamax(
                game, 1, alpha, beta, state, false, true, qdepth, tt, killers, ply, age, best, pv,
                allow_null,
            )
        } else {
            // quiescence search: continue until all captures are complete.
            let stand_pat_eval = relative_evaluate(&game.board);
            if stand_pat_eval >= beta {
                return (stand_pat_eval, 0, nodes, true);
            }
            if qdepth == 0 {
                (stand_pat_eval, 0, nodes, true)
            } else {
                // no move made - same node conceptually, so forward pv itself.
                let (q_eval, mv, q_nodes, q_completed) = negamax(
                    game,
                    1,
                    alpha,
                    beta,
                    state,
                    true,
                    false,
                    qdepth - 1,
                    tt,
                    killers,
                    ply,
                    age,
                    best,
                    pv,
                    allow_null,
                );
                nodes += q_nodes;
                // stand_pat is a floor: a quiet position is never worse than not capturing.
                if q_eval > stand_pat_eval {
                    (q_eval, mv, nodes, q_completed)
                } else {
                    (stand_pat_eval, 0, nodes, q_completed)
                }
            }
        }
    } else {
        // move list generated here, right where it's consumed - skipped in
        // quiescence/check-extension, which use a smaller targeted set already.
        let mut legal_moves: ArrayVec<u16, 256>;

        if !quiescence && !check_extension {
            // RFP_MAX_DEPTH >= NMP_MIN_DEPTH so every depth is eligible for at least one of RFP/NMP
            // below - computing this once avoids a redundant is_check on nodes eligible for both.
            let in_check = rules::is_check(&game.board, game.board.side_to_move);

            // reverse futility pruning: eval >= beta + margin means deeper search is unneeded.
            // low depths, non-mate beta, non-check only. not tt-stored (only a static eval).
            if depth <= RFP_MAX_DEPTH && beta > -MATE_THRESHOLD && !in_check {
                let static_eval = relative_evaluate(&game.board);
                let margin = RFP_MARGIN_BASE + (depth as i16 * RFP_MARGIN_PER_DEPTH);
                if static_eval - margin >= beta {
                    return (static_eval, 0, nodes, true);
                }
            }

            // null move pruning: skipping your move still scores >= beta -> prune the subtree.
            // high depths, non-mate beta, non-check, has major pieces (no null move in zugzwang).
            if allow_null
                && depth >= NMP_MIN_DEPTH
                && beta < MATE_THRESHOLD
                && has_pieces(&game.board)
                && !in_check
            {
                game.make_null_move().unwrap();
                if game.result == GameResult::InProgress {
                    // not a real move in our line - scratch buffer, discarded.
                    let mut null_pv: ArrayVec<u16, MAX_PV_LEN> = ArrayVec::new();
                    let (score, _, null_nodes, completed) = negamax(
                        game,
                        depth - 1 - NMP_REDUCTION,
                        -beta,
                        -beta + 1,
                        state.clone(),
                        false,
                        false,
                        qdepth,
                        tt,
                        killers,
                        ply + 1,
                        age,
                        best,
                        &mut null_pv,
                        false,
                    );
                    game.unmake_null_move().unwrap();
                    nodes += null_nodes;
                    if !completed {
                        return (0, 0, nodes, false);
                    } else if -score >= beta {
                        return (-score, 0, nodes, true);
                    }
                } else {
                    game.unmake_null_move().unwrap();
                }
            }

            legal_moves = get_legal_moves(&game.board);

            // root: force the incumbent move to the front so it's re-evaluated first each depth -
            // the tt isn't reliable enough for this (skip_tt, slot overwrites).
            let move_to_front = if ply == 0 {
                Some(best.best_move)
            } else {
                tt_move
            };
            reorder_moves(
                &game.board,
                &mut legal_moves,
                move_to_front,
                killers.get(ply),
            );
        } else {
            legal_moves = get_legal_moves(&game.board);
        }

        let mut alpha = alpha;
        let alpha_orig = alpha;
        let mut best_eval = -i16::MAX;
        let mut cutoff = false;
        let mut best_move = legal_moves[0];
        for (i, movei) in legal_moves.into_iter().enumerate() {
            if state.stop.load(Ordering::Relaxed) {
                return (0, best_move, nodes, false);
            }
            if quiescence && !is_capture(movei) {
                continue;
            }

            // scratch buffer - only spliced into our pv below if movei becomes the new best.
            let mut child_pv: ArrayVec<u16, MAX_PV_LEN> = ArrayVec::new();
            _ = game.make_move(movei, false, true);
            // real move made - negate child score, swap alpha/beta.
            let (child_eval, _, child_nodes, child_completed) = if i == 0 || quiescence {
                // principal variation search: if first move or quiescence search, search full window.
                negamax(
                    game,
                    depth - 1,
                    -beta,
                    -alpha,
                    Arc::clone(&state),
                    false,
                    false,
                    qdepth,
                    tt,
                    killers,
                    ply + 1,
                    age,
                    best,
                    &mut child_pv,
                    true,
                )
            } else {
                // later moves, cheap null-window probe first
                let probe = negamax(
                    game,
                    depth - 1,
                    -alpha - 1,
                    -alpha,
                    Arc::clone(&state),
                    false,
                    false,
                    qdepth,
                    tt,
                    killers,
                    ply + 1,
                    age,
                    best,
                    &mut child_pv,
                    true,
                );
                if -probe.0 > alpha {
                    // it beat our test, we need its real score
                    child_pv.clear();
                    let full_eval = negamax(
                        game,
                        depth - 1,
                        -beta,
                        -alpha,
                        Arc::clone(&state),
                        false,
                        false,
                        qdepth,
                        tt,
                        killers,
                        ply + 1,
                        age,
                        best,
                        &mut child_pv,
                        true,
                    );
                    (full_eval.0, full_eval.1, full_eval.2 + probe.2, full_eval.3)
                } else {
                    // it is not better than alpha
                    probe
                }
            };

            let eval = -child_eval;
            nodes += child_nodes;
            _ = game.unmake_move(false);
            if !child_completed {
                // subtree abandoned mid-search - eval is a meaningless sentinel, propagate up.
                return (0, best_move, nodes, false);
            }
            if eval > best_eval {
                best_eval = eval;
                best_move = movei;
                pv.clear();
                pv.push(movei);
                pv.extend(child_pv.iter().copied());
            }
            // root only: publish every move (not just new incumbents) - either movei is the
            // previously-published move getting its re-evaluation, or the local best already
            // beats the stored score outright.
            if ply == 0 && (movei == best.best_move || best_eval > best.best_score) {
                best.best_move = best_move;
                best.best_score = best_eval;
                best.best_depth = depth;
                best.pv = pv.clone();
            }
            if best_eval > alpha {
                alpha = best_eval;
            }
            if alpha >= beta {
                cutoff = true;
                // board is back to pre-move (unmake ran above) - what move_gives_check needs.
                if !is_capture(movei) && !move_gives_check(&game.board, movei) {
                    killers.store(ply, movei);
                }
                break;
            }
        }

        // tt update logic — 3-way classification
        if !(quiescence || check_extension) {
            let flag = if cutoff {
                TT_LOWERB_FLAG // fail-high: beta cutoff
            } else if best_eval <= alpha_orig {
                TT_UPPERB_FLAG // fail-low: never beat alpha
            } else {
                TT_EXACT_FLAG // landed strictly inside window
            };
            update_tt(
                tt,
                game.board.zobrist_hash,
                to_tt_score(best_eval, ply),
                depth,
                flag,
                best_move,
                age,
            );
        }

        (best_eval, best_move, nodes, true)
    }
}

// orders legal_moves in place: tt best move, then captures, then killer moves, then the rest.
// a best_move/killer not actually legal here (killers are ply-indexed, so can carry over from a
// different position sharing this ply) is silently skipped.
pub fn reorder_moves(
    board: &ChessBoard,
    legal_moves: &mut ArrayVec<u16, 256>,
    best_move: Option<u16>,
    killer_moves: [Option<u16>; 2],
) {
    let mut ordered: ArrayVec<u16, 256> = ArrayVec::new();
    let mut captures: ArrayVec<(u16, i16), 256> = ArrayVec::new();
    let mut rest: ArrayVec<u16, 256> = ArrayVec::new();

    if let Some(mv) = best_move
        && legal_moves.contains(&mv)
    {
        ordered.push(mv);
    }

    for &mv in legal_moves.iter() {
        if Some(mv) == best_move {
            continue;
        } else if is_capture(mv) {
            captures.push((mv, 0));
        } else if !killer_moves.contains(&Some(mv)) {
            rest.push(mv);
        }
        // else: a stored killer - placed in the killer pass below.
    }

    // MVV-LVA: capture_score = k * value(victim) - value(aggressor), sorted descending.
    for i in 0..captures.len() {
        let [from_sqi, to_sqi, _] = decode_move(captures[i].0);

        let aggressor_type: u8;
        let victim_type: u8;
        if 1u64 << to_sqi == board.en_passant {
            aggressor_type = 0;
            victim_type = 0;
        } else {
            aggressor_type = board.piece_type_at(from_sqi).unwrap();
            victim_type = board.piece_type_at(to_sqi).unwrap(); // unwrap should be safe since there must be a piece there during a capture.
        }
        let aggressor_score = PIECE_VALUES[aggressor_type as usize];
        let victim_score = VICTIM_WEIGHT * PIECE_VALUES[victim_type as usize];
        captures[i].1 = victim_score - aggressor_score;
    }
    captures.sort_unstable_by_key(|&(_, score)| std::cmp::Reverse(score));

    ordered.extend(captures.iter().map(|&(mv, _)| mv));

    for killer in killer_moves {
        if let Some(mv) = killer
            && legal_moves.contains(&mv)
            && !ordered.contains(&mv)
        {
            ordered.push(mv);
        }
    }
    ordered.extend(rest);

    *legal_moves = ordered;
}

#[cfg(test)]
mod tt_clone_share_test {
    use super::TT;
    use std::sync::atomic::Ordering;
    use std::thread;

    #[test]
    fn clone_shares_the_same_underlying_entries() {
        let tt = TT::new(1);
        let tt_a = tt.clone();
        let tt_b = tt.clone();

        assert!(
            std::sync::Arc::ptr_eq(&tt_a.entries, &tt_b.entries),
            "clones point at different allocations - TT is NOT sharing entries"
        );

        let idx = 5usize;
        thread::spawn(move || {
            tt_a.entries[idx].1.store(123456789, Ordering::Relaxed);
        })
        .join()
        .unwrap();

        let seen = tt_b.entries[idx].1.load(Ordering::Relaxed);
        assert_eq!(
            seen, 123456789,
            "write through tt_a's clone was not visible through tt_b's clone"
        );
    }
}

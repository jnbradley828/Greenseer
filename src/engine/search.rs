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
use crate::engine::{self, eval::relative_evaluate};
use crate::engine::{
    search_heuristics::{
        FP_MARGIN_BASE, FP_MARGIN_PER_DEPTH, FP_MAX_DEPTH, MATE_THRESHOLD, NMP_MIN_DEPTH,
        NMP_REDUCTION, RFP_MARGIN_BASE, RFP_MARGIN_PER_DEPTH, RFP_MAX_DEPTH,
    },
    utils,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub struct SearchState {
    pub stop: AtomicBool,
}
impl SearchState {
    pub fn new() -> Self {
        Self {
            stop: AtomicBool::new(false),
        }
    }
}

// published incrementally by the root of negamax as root moves complete - always readable as
// "the best confirmed answer so far", valid even if the search is interrupted mid-depth. lives
// only on the search thread's own stack (passed down by &mut through the recursion), since
// nothing outside that thread ever needs to see it.
pub struct RootBest {
    pub best_move: u16,
    pub best_score: i16,
    // the depth that best_move/best_score actually came from - not necessarily the depth
    // iteratively_deepen's loop is nominally on, since a depth that hasn't yet re-evaluated the
    // incumbent move (see negamax's root loop) can leave this reflecting an earlier depth.
    pub best_depth: u8,
}
impl RootBest {
    pub fn new(fallback_move: u16) -> Self {
        Self {
            best_move: fallback_move,
            best_score: -i16::MAX,
            best_depth: 0,
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

// 2 killer (quiet, cutoff-causing) moves per ply. sized to 255 (not 256) since ply is a u8 and
// ply 0 (the root - it has no siblings, so a killer stored there could never be read by
// anyone) is excluded from storage. the ply -> index mapping is encapsulated here rather than
// duplicated at each call site.
pub struct KillerTable([[Option<u16>; 2]; 255]);

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

    // records a cutoff-causing quiet move, shifting the existing primary killer to secondary.
    // no-op for ply == 0, and for a move that's already this ply's primary killer.
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
// negamax formulation: every node's returned score is from the perspective of whoever is to
// move AT THAT NODE (positive = good for the current mover), not a fixed global side. a
// recursive call that actually makes a move negates the returned score and swaps alpha/beta
// into (-beta, -alpha); a call that doesn't move (check-extension/quiescence dispatch) does
// neither, since the side to move hasn't changed.
// returns (position eval, best move, nodes visited, completed) **pruned nodes are not counted**.
// completed is false only when a stop signal cut this subtree short mid-search, in which case
// the eval/move are a meaningless sentinel (always score 0) - callers must not treat them as a
// real result, only propagate the incompletion upward.
// quiescence search means search until no more captures are available and position is not check, even if depth expires.
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
    allow_null: bool,
) -> (i16, u16, u64, bool) {
    let mut nodes: u64 = 1;
    if game.result != GameResult::InProgress {
        // if game is over in this position
        if !matches!(game.result, GameResult::Draw(_)) {
            // checkmate is always bad for whoever is to move (they have no moves and are in
            // check) - discounted by ply to prefer faster mates.
            return (ply as i16 - 10000, 0, nodes, true);
        } else {
            return (0, 0, nodes, true);
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
            // root only: this position's TT entry already covers >= depth, so it's a valid
            // confirmed answer for this iteration - publish it before returning early, otherwise
            // a root TT hit (routine once the TT holds entries from prior moves' searches)
            // bypasses the moves loop entirely and best never hears about it.
            let publish = |best: &mut RootBest| {
                if ply == 0 {
                    best.best_move = tt_entry.4;
                    best.best_score = tt_score;
                    best.best_depth = depth;
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
    // move generation and reordering are deferred to the depth != 0 branch below, right where
    // the list is actually consumed - nothing above this point (game-over check, tt lookup,
    // check-extension/quiescence dispatch) ever needs the move list, so nothing above it should
    // pay to generate one.

    if depth == 0 {
        // check extension: if the position is check: search depth 1. no move made - same
        // board/side, so no negation or alpha/beta swap.
        if rules::is_check(&game.board, game.board.side_to_move) {
            return negamax(
                game, 1, alpha, beta, state, false, true, qdepth, tt, killers, ply, age, best,
                allow_null,
            );
        } else {
            // quiescence search: continue search until all captures are complete.
            let stand_pat_eval = relative_evaluate(game);
            if stand_pat_eval >= beta {
                return (stand_pat_eval, 0, nodes, true);
            }
            if qdepth == 0 {
                return (stand_pat_eval, 0, nodes, true);
            } else {
                // no move made here either (same position, just now searching captures only) -
                // no negation or swap.
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
                    allow_null,
                );
                nodes += q_nodes;
                // stand_pat is a floor: a quiet position is never worse than not capturing.
                if q_eval > stand_pat_eval {
                    return (q_eval, mv, nodes, q_completed);
                } else {
                    return (stand_pat_eval, 0, nodes, q_completed);
                }
            }
        }
    } else {
        // generate once per node, right where it's actually needed - reorder in place (no
        // clone of the move list), folding in the tt move (or, at the root, the incumbent move
        // going into this depth - see below) and this ply's killers if any exist, on top of the
        // usual capture/check-first heuristic. skipped entirely during quiescence/check-extension,
        // which are already restricted to a small, targeted move set - not worth the
        // classification cost reorder_moves always pays regardless of what's passed in.
        let mut legal_moves: ArrayVec<u16, 256>;
        let mut first_quiet_idx: usize = usize::MAX;
        // shared between RFP and FP below so a node that qualifies for both only pays for
        // relative_evaluate once.
        let mut static_eval: i16 = 0;
        let mut se_calculated = false;

        if !quiescence && !check_extension {
            // reverse futility pruning: if the position is so good that eval >= beta + margin,
            // don't even bother searching deeper. improves speed, accuracy irons itself out
            // over increased depth.
            // only use for low depths, non-mate beta values, and non-check positions.
            // not stored in transposition table, as it is only a static eval of a position.
            if depth <= RFP_MAX_DEPTH
                && beta > -MATE_THRESHOLD
                && !rules::is_check(&game.board, game.board.side_to_move)
            {
                static_eval = relative_evaluate(game);
                se_calculated = true;
                let margin = RFP_MARGIN_BASE + (depth as i16 * RFP_MARGIN_PER_DEPTH);
                if static_eval - margin >= beta {
                    return (static_eval, 0, nodes, true);
                }
            }

            // null move pruning: if giving up your move (making a "null" move) still leads
            // to a position so good it fails high (score >= beta) after a shallow serach,
            // your actual best move would be too - so you can prune the subtree without
            // searching real moves.
            // only use for high depths, non-mate beta values, non-check-positions, and positions
            // where player to move has major pieces on the board (no null move in zugzwang!)
            // Do not allow consecutive null moves (set allow_null = false)
            if allow_null
                && depth >= NMP_MIN_DEPTH
                && beta < MATE_THRESHOLD
                && utils::has_pieces(&game.board)
                && !rules::is_check(&game.board, game.board.side_to_move)
            {
                game.make_null_move().unwrap();
                if game.result == GameResult::InProgress {
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

            legal_moves = get_legal_moves(&mut game.board);

            // at the root, force best.best_move to the front regardless of what (if anything)
            // the tt lookup returned - the incremental publish logic below depends on this move
            // being re-evaluated as early as possible each depth, and the tt isn't a reliable way
            // to guarantee that (skip_tt on repetitions, or the root's slot simply getting
            // overwritten by an unrelated node in a huge, shared table).
            let move_to_front = if ply == 0 {
                Some(best.best_move)
            } else {
                tt_move
            };
            first_quiet_idx = reorder_moves(
                &game.board,
                &mut legal_moves,
                move_to_front,
                killers.get(ply),
            );
        } else {
            legal_moves = get_legal_moves(&mut game.board);
        }

        let mut alpha = alpha;
        let alpha_orig = alpha;
        let mut best_eval = -i16::MAX;
        let mut cutoff = false;
        let mut best_move = legal_moves[0];
        for (move_idx, movei) in legal_moves.into_iter().enumerate() {
            if state.stop.load(Ordering::Relaxed) {
                return (0, best_move, nodes, false);
            }
            if quiescence && !is_capture(movei) {
                // if quiescence only search: skip non captures.
                continue;
            }

            // TO BE RETRIED AFTER IMPLEMENTING QUIET MOVE HISTORY TABLES
            // futility pruning: at shallow depth, a quiet move whose static eval - even given a
            // generous margin - can't climb up to alpha is assumed hopeless and skipped without
            // being searched. move_idx != 0 guarantees at least one move is fully searched at
            // this node regardless of bucket; move_idx >= first_quiet_idx additionally excludes
            // the tt/capture/check/killer buckets reorder_moves placed ahead of it.
            // disabled: SPRT'd across several margin/depth configs (see search_heuristics.rs)
            // with no config clearing a positive result. root cause suspected to be
            // reorder_moves's quiet bucket having no internal ordering, so the eligibility test
            // doesn't correlate with "this move is probably bad" the way it needs to.
            // if !quiescence
            //     && !check_extension
            //     && depth <= FP_MAX_DEPTH
            //     && move_idx != 0
            //     && move_idx >= first_quiet_idx
            //     && alpha < MATE_THRESHOLD
            //     && !rules::is_check(&game.board, game.board.side_to_move)
            // {
            //     if !se_calculated {
            //         static_eval = relative_evaluate(game);
            //         se_calculated = true;
            //     }
            //     let margin = FP_MARGIN_BASE + (depth as i16 * FP_MARGIN_PER_DEPTH);
            //     if static_eval + margin <= alpha {
            //         continue;
            //     }
            // }

            _ = game.make_move(movei, false, true);
            // a move was actually made here - negate the child's score (it's from the
            // opponent's perspective) and swap alpha/beta accordingly.
            let (child_eval, _, child_nodes, child_completed) = negamax(
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
                true,
            );
            let eval = -child_eval;
            nodes += child_nodes;
            _ = game.unmake_move(false);
            if !child_completed {
                // movei's subtree was abandoned mid-search - eval is a meaningless sentinel, not
                // a real result. propagate the incompletion upward without letting it pollute
                // best_eval/best_move or get published below as if it were genuine.
                return (0, best_move, nodes, false);
            }
            if eval > best_eval {
                best_eval = eval;
                best_move = movei;
            }
            // root only: publish incrementally so best.best_move/best_score always reflect the
            // best confirmed answer, even if interrupted mid-depth. checked every move, not
            // nested inside "if eval > best_eval" above - movei can be worth publishing even on
            // an iteration where it didn't just become the local incumbent. two safe cases:
            // (1) movei is the previously-published move - it's had its fair shot at this depth,
            // so whatever's locally best now wins by default, even if lower than before.
            // (2) movei isn't that move, but the local best already outright beats the stored
            // score - no fairness concern needed there either.
            if ply == 0 {
                if movei == best.best_move {
                    best.best_move = best_move;
                    best.best_score = best_eval;
                    best.best_depth = depth;
                } else if best_eval > best.best_score {
                    best.best_move = best_move;
                    best.best_score = best_eval;
                    best.best_depth = depth;
                }
            }
            if best_eval > alpha {
                alpha = best_eval;
            }
            if alpha >= beta {
                cutoff = true;
                // board is back to the pre-move position here (unmake already ran above), which
                // is exactly what move_gives_check needs.
                if !is_capture(movei) && !move_gives_check(&game.board, movei) {
                    killers.store(ply, movei);
                }
                break;
            }
        }

        // tt update logic — 3-way classification
        if !(quiescence || check_extension) {
            let flag = if cutoff {
                engine::utils::TT_LOWERB_FLAG // fail-high: beta cutoff
            } else if best_eval <= alpha_orig {
                engine::utils::TT_UPPERB_FLAG // fail-low: never beat alpha
            } else {
                engine::utils::TT_EXACT_FLAG // landed strictly inside window
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

        return (best_eval, best_move, nodes, true);
    }
}

// orders legal_moves in place: tt best move, then captures, then checks, then killer moves,
// then everything else untouched. best_move/killer_moves entries not actually legal here (a
// killer is ply-indexed, not position-indexed, so it can carry over from a different position
// that shared this ply) are silently skipped.
// returns the index at which the trailing "rest" block (quiet, non-checking, non-killer moves)
// begins - callers use this as the futility-pruning eligibility boundary.
pub fn reorder_moves(
    board: &ChessBoard,
    legal_moves: &mut ArrayVec<u16, 256>,
    best_move: Option<u16>,
    killer_moves: [Option<u16>; 2],
) -> usize {
    let mut ordered: ArrayVec<u16, 256> = ArrayVec::new();
    let mut captures: ArrayVec<u16, 256> = ArrayVec::new();
    let mut checks: ArrayVec<u16, 256> = ArrayVec::new();
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
            // cheap flag check first - only pay for the pricier check test below on moves that
            // aren't already captures (a move that's both is treated as a capture).
            captures.push(mv);
        } else if move_gives_check(board, mv) {
            checks.push(mv);
        } else if !killer_moves.contains(&Some(mv)) {
            rest.push(mv);
        }
        // else: quiet, non-checking, and a stored killer - placed in the killer pass below.
    }
    ordered.extend(captures);
    ordered.extend(checks);

    for killer in killer_moves {
        if let Some(mv) = killer
            && legal_moves.contains(&mv)
            && !ordered.contains(&mv)
        {
            ordered.push(mv);
        }
    }
    let first_quiet_idx = ordered.len();
    ordered.extend(rest);

    *legal_moves = ordered;
    first_quiet_idx
}

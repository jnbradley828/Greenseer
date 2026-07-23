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
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};

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

pub const MAX_QDEPTH: u8 = 6;
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
// returns (position eval, nodes visited) **pruned nodes are not counted**
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
) -> (i16, u16, u64) {
    let mut nodes: u64 = 1;
    if game.result != GameResult::InProgress {
        // if game is over in this position
        if !matches!(game.result, GameResult::Draw(_)) {
            // checkmate is always bad for whoever is to move (they have no moves and are in
            // check) - discounted by ply to prefer faster mates.
            return (ply as i16 - 10000, 0, nodes);
        } else {
            return (0, 0, nodes);
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
            match tt_entry.3 {
                0 => return (tt_score, tt_entry.4, nodes),
                1 => {
                    if tt_score >= beta {
                        return (tt_score, tt_entry.4, nodes);
                    }
                }
                2 => {
                    if tt_score <= alpha {
                        return (tt_score, tt_entry.4, nodes);
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
                game, 1, alpha, beta, state, false, true, qdepth, tt, killers, ply, age,
            );
        } else {
            // quiescence search: continue search until all captures are complete.
            let stand_pat_eval = relative_evaluate(game);
            if stand_pat_eval >= beta {
                return (stand_pat_eval, 0, nodes);
            }
            if qdepth == 0 {
                return (stand_pat_eval, 0, nodes);
            } else {
                // no move made here either (same position, just now searching captures only) -
                // no negation or swap.
                let (q_eval, mv, q_nodes) = negamax(
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
                );
                nodes += q_nodes;
                // stand_pat is a floor: a quiet position is never worse than not capturing.
                if q_eval > stand_pat_eval {
                    return (q_eval, mv, nodes);
                } else {
                    return (stand_pat_eval, 0, nodes);
                }
            }
        }
    } else {
        // generate once per node, right where it's actually needed - reorder in place (no
        // clone of the move list), folding in the tt move and this ply's killers if any exist,
        // on top of the usual capture/check-first heuristic. skipped entirely during
        // quiescence/check-extension, which are already restricted to a small, targeted move
        // set - not worth the classification cost reorder_moves always pays regardless of
        // what's passed in.
        let mut legal_moves: ArrayVec<u16, 256> = get_legal_moves(&mut game.board);
        if !quiescence && !check_extension {
            reorder_moves(&game.board, &mut legal_moves, tt_move, killers.get(ply));
        }

        let mut alpha = alpha;
        let alpha_orig = alpha;
        let mut best_eval = -i16::MAX;
        let mut cutoff = false;
        let mut best_move = legal_moves[0];
        for movei in legal_moves {
            if state.stop.load(Ordering::Relaxed) {
                return (0, best_move, nodes);
            }
            if quiescence && !is_capture(movei) {
                // if quiescence only search: skip non captures.
                continue;
            }
            _ = game.make_move(movei, false, true);
            // a move was actually made here - negate the child's score (it's from the
            // opponent's perspective) and swap alpha/beta accordingly.
            let (child_eval, _, child_nodes) = negamax(
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
            );
            let eval = -child_eval;
            nodes += child_nodes;
            _ = game.unmake_move(false);
            if eval > best_eval {
                best_eval = eval;
                best_move = movei;
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

        return (best_eval, best_move, nodes);
    }
}

// orders legal_moves in place: tt best move, then captures, then checks, then killer moves,
// then everything else untouched. best_move/killer_moves entries not actually legal here (a
// killer is ply-indexed, not position-indexed, so it can carry over from a different position
// that shared this ply) are silently skipped.
pub fn reorder_moves(
    board: &ChessBoard,
    legal_moves: &mut ArrayVec<u16, 256>,
    best_move: Option<u16>,
    killer_moves: [Option<u16>; 2],
) -> () {
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
    ordered.extend(rest);

    *legal_moves = ordered;
}

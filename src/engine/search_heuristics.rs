// tunable constants that shape how the search behaves - not static evaluation weights, so not
// subject to Texel tuning. tuned instead by search-level testing (SPRT, node counts, time losses).

pub const MAX_QDEPTH: u8 = 6;

// score magnitude beyond which a value is considered mate-distance rather than a normal
// evaluation - used both for TT mate-score adjustment and to gate search heuristics (e.g. null
// move pruning) away from windows that already border on a forced mate.
pub const MATE_THRESHOLD: i16 = 9000;

// ms subtracted from the clock's reported remaining time before any time-management math, to
// leave headroom for engine/GUI/network overhead so a move is never lost to flagging.
pub const MOVE_OVERHEAD: u32 = 20;

// time management (handle_go's time-control branch)
// below this many ms remaining, stop reasoning about budgets entirely and just move instantly.
pub const PANIC_THRESHOLD_MS: u32 = 50;
// time allotted when it's a forced/only-legal move - deliberately small, no thinking needed.
pub const SINGLE_LEGAL_MOVE_TIME_MS: u32 = 50;
// base move time = time_remaining / TIME_DIVISOR, before the increment and opponent-time scaling
// below are applied.
pub const TIME_DIVISOR: u32 = 20;
// fraction of the increment folded into the base move time budget.
pub const INC_FRACTION_NUM: u32 = 3;
pub const INC_FRACTION_DENOM: u32 = 4;

// reverse futility pruning prunes nodes where the static eval is so good,
// it is determined that searching deeper is unneccessary.
// condition to prune: static_eval - margin >= beta
// margin = C * depth + B
pub const RFP_MARGIN_PER_DEPTH: i16 = 150;
pub const RFP_MARGIN_BASE: i16 = 120;
pub const RFP_MAX_DEPTH: u8 = 6;

// null move pruning prunes nodes where the position is so good, that even
// skipping your move leads to a score >= beta.
pub const NMP_MIN_DEPTH: u8 = 3;
pub const NMP_REDUCTION: u8 = 2;

// futility pruning skips individual quiet moves at shallow depth where the static eval, even
// given a generous margin, can't climb high enough to raise alpha.
// condition to prune: static_eval + margin <= alpha
// margin = C * depth + B
pub const FP_MARGIN_PER_DEPTH: i16 = 200;
pub const FP_MARGIN_BASE: i16 = 100;
pub const FP_MAX_DEPTH: u8 = 2;

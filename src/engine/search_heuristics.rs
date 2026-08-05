// search-algorithm constants, tuned via SPRT/node counts - not eval weights (see eval_heuristics.rs).

pub const MAX_QDEPTH: u8 = 6;
pub const MATE_THRESHOLD: i16 = 9000; // score magnitude beyond which a value is mate-distance

pub const MOVE_OVERHEAD: u32 = 20; // ms reserved off the clock before time-management math

// time management (handle_go's time-control branch)
pub const PANIC_THRESHOLD_MS: u32 = 50; // below this, skip budgeting and move instantly
pub const SINGLE_LEGAL_MOVE_TIME_MS: u32 = 50; // forced move - no thinking needed
pub const TIME_DIVISOR: u32 = 20; // base move time = time_remaining / TIME_DIVISOR
pub const INC_FRACTION_NUM: u32 = 3; // fraction of increment folded into the base budget
pub const INC_FRACTION_DENOM: u32 = 4;
pub const MAX_TIME_FRACTION: f32 = 0.5; // ceiling on remaining time a move can use

// reverse futility pruning: prune when static_eval - margin >= beta. margin = C * depth + B.
pub const RFP_MARGIN_PER_DEPTH: i16 = 150;
pub const RFP_MARGIN_BASE: i16 = 120;
pub const RFP_MAX_DEPTH: u8 = 6;

pub const NMP_MIN_DEPTH: u8 = 3; // null move pruning: skipping your move still scores >= beta
pub const NMP_REDUCTION: u8 = 2;

pub const VICTIM_WEIGHT: i16 = 10; // MVV-LVA victim weight

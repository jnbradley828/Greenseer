use serde::Deserialize;
use serde_big_array::BigArray;

// search-algorithm constants, tuned via SPRT/node counts - not eval weights (see EvalParams).
#[derive(Deserialize)]
pub struct SearchParams {
    pub max_qdepth: u8,

    pub move_overhead: u32, // ms reserved off the clock before time-management math

    // time management (handle_go's time-control branch)
    pub panic_threshold_ms: u32, // below this, skip budgeting and move instantly
    pub single_legal_move_time_ms: u32, // forced move - no thinking needed
    pub time_divisor: u32,       // base move time = time_remaining / time_divisor
    pub inc_fraction_num: u32,   // fraction of increment folded into the base budget
    pub inc_fraction_denom: u32,
    pub max_time_fraction: f32, // ceiling on remaining time a move can use

    // reverse futility pruning: prune when static_eval - margin >= beta. margin = C * depth + B.
    pub rfp_margin_per_depth: i16,
    pub rfp_margin_base: i16,
    pub rfp_max_depth: u8,

    pub nmp_min_depth: u8, // null move pruning: skipping your move still scores >= beta
    pub nmp_reduction: u8,

    pub victim_weight: i16, // MVV-LVA victim weight
}

// tunable evaluation weights.
#[derive(Deserialize)]
pub struct EvalParams {
    pub piece_values: [i16; 5], // [p, n, b, r, q]
    pub mg_phase_span: i16,     // non-pawn material span for mg/eg phase blend

    pub tt_age_factor: i16,
    pub early_queen_factor: i16,
    pub open_file_rook: i16,
    pub semiopen_file_rook: i16,
    pub tempo_bonus: i16,
    pub mg_bishop_pair_bonus: i16,
    pub eg_bishop_pair_bonus: i16,
    pub mobility_bonus: [i16; 4], // knight, bishop, rook, queen
    pub mg_doubled_pawn_penalty: i16,
    pub eg_doubled_pawn_penalty: i16,
    pub mg_isolated_pawn_penalty: i16,
    pub eg_isolated_pawn_penalty: i16,
    pub mg_backward_pawn_penalty: i16,
    pub eg_backward_pawn_penalty: i16,
    pub mg_pawn_shield_bonus: [i16; 3], // 1 rank ahead, 2 ranks ahead, weak

    // king safety: king_attack_weight[piece_type] per attacked square in the enemy king zone.
    pub king_attack_weight: [i16; 5], // pawn, knight, bishop, rook, queen
    pub king_attack_linear: f32,      // danger = linear*units + quadratic*units^2
    pub king_attack_quadratic: f32,
    pub max_king_attack_units: i16, // clamp before the curve is applied
    pub king_open_file_mult: f32,   // danger multiplier per open/semi-open file near king
    pub king_semiopen_file_mult: f32,
    pub mg_passed_pawn_bonus: [i16; 5], // ranks 2-6 (rank 7 in mg_pawn_mod)
    pub eg_passed_pawn_bonus: [i16; 5],

    // piece-square tables
    #[serde(with = "BigArray")]
    pub mg_pawn_mod: [i8; 64],
    #[serde(with = "BigArray")]
    pub mg_knight_mod: [i8; 64],
    #[serde(with = "BigArray")]
    pub mg_bishop_mod: [i8; 64],
    #[serde(with = "BigArray")]
    pub mg_rook_mod: [i8; 64],
    #[serde(with = "BigArray")]
    pub mg_queen_mod: [i8; 64],
    #[serde(with = "BigArray")]
    pub mg_king_mod: [i8; 64],
    #[serde(with = "BigArray")]
    pub eg_pawn_mod: [i8; 64],
    #[serde(with = "BigArray")]
    pub eg_knight_mod: [i8; 64],
    #[serde(with = "BigArray")]
    pub eg_bishop_mod: [i8; 64],
    #[serde(with = "BigArray")]
    pub eg_rook_mod: [i8; 64],
    #[serde(with = "BigArray")]
    pub eg_queen_mod: [i8; 64],
    #[serde(with = "BigArray")]
    pub eg_king_mod: [i8; 64],
}

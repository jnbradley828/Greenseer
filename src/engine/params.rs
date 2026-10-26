use serde::Deserialize;
use serde_big_array::BigArray;

// search-algorithm constants, tuned via SPRT/node counts - not eval weights (see EvalParams).
#[derive(Deserialize, Debug)]
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

    pub tt_age_factor: i16, // TT replacement: entry age penalty, in depth units per age step
}

// pawn is the fixed unit of material - not tunable, so it lives outside piece_values.
pub const PAWN_VALUE: i16 = 100;

// tunable evaluation weights.
#[derive(Deserialize, Debug)]
pub struct EvalParams {
    pub piece_values: [i16; 4], // [n, b, r, q] - pawn is PAWN_VALUE
    pub phase_weights: [i16; 4], // [n, b, r, q] phase contribution per piece - starting position = full mg

    pub open_file_rook: i16,
    pub semiopen_file_rook: i16,
    pub tempo_bonus: i16,
    pub trade_incentive_factor: i16, // scales net/total material ratio: trade down when ahead, keep material when behind
    pub mg_bishop_pair_bonus: i16,
    pub eg_bishop_pair_bonus: i16,
    pub mg_mobility_bonus: [f32; 4], // knight, bishop, rook, queen - per attacked square
    pub eg_mobility_bonus: [f32; 4],
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
    pub mg_pawn_mod: [i16; 64],
    #[serde(with = "BigArray")]
    pub mg_knight_mod: [i16; 64],
    #[serde(with = "BigArray")]
    pub mg_bishop_mod: [i16; 64],
    #[serde(with = "BigArray")]
    pub mg_rook_mod: [i16; 64],
    #[serde(with = "BigArray")]
    pub mg_queen_mod: [i16; 64],
    #[serde(with = "BigArray")]
    pub mg_king_mod: [i16; 64],
    #[serde(with = "BigArray")]
    pub eg_pawn_mod: [i16; 64],
    #[serde(with = "BigArray")]
    pub eg_knight_mod: [i16; 64],
    #[serde(with = "BigArray")]
    pub eg_bishop_mod: [i16; 64],
    #[serde(with = "BigArray")]
    pub eg_rook_mod: [i16; 64],
    #[serde(with = "BigArray")]
    pub eg_queen_mod: [i16; 64],
    #[serde(with = "BigArray")]
    pub eg_king_mod: [i16; 64],
}

#[cfg(not(feature = "tuning"))]
mod baked_params {
    use super::*;
    include!(concat!(env!("OUT_DIR"), "/params_const.rs"));

    pub fn initialize_params() {}
    pub fn sp() -> &'static SearchParams {
        &SEARCH_PARAMS
    }
    pub fn ep() -> &'static EvalParams {
        &EVAL_PARAMS
    }
}

#[cfg(not(feature = "tuning"))]
pub use baked_params::*;

// TUNING ONLY BUILD: BUILD PARAMS AT STARTUP AND INCUR A ROUGHLY 5% SLOWDOWN COST
// DUE TO PARAMETER LOOKUPS INSTEAD OF BEING FOLDED INTO THE BINARY

#[cfg(feature = "tuning")]
mod tuning {
    use super::*;
    use std::sync::OnceLock;

    pub static SEARCH_PARAMS: OnceLock<SearchParams> = OnceLock::new();
    pub static EVAL_PARAMS: OnceLock<EvalParams> = OnceLock::new();

    // fills both boxes from the toml files. called once by main; getters also call it if the boxes
    // are still empty (e.g. under cargo test, where main never runs).
    pub fn initialize_params() {
        let args: Vec<String> = std::env::args().collect();

        // returns None if no flag, else returns path to folder containing params.
        let params_path = args.iter().position(|a| a == "--params_path").map(|i| {
            args.get(i + 1)
                .expect("--params_path needs a path to a folder after it")
                .as_str()
        });
        if let Some(p) = params_path {
            if std::path::Path::new(&p).is_relative() {
                panic!("--params_path must be absolute.");
            }
        }

        let eval_path: Option<&str>;
        let search_path: Option<&str>;
        let eps: String;
        let sps: String;
        if params_path.is_some() {
            eps = [params_path.unwrap(), "/eval_heuristics.toml"].concat();
            eval_path = Some(&eps);
            sps = [params_path.unwrap(), "/search_heuristics.toml"].concat();
            search_path = Some(&sps);
        } else {
            eval_path = None;
            search_path = None;
        }

        let eval_config = parse_param_toml(ParameterType::Evaluation, eval_path);
        let search_config = parse_param_toml(ParameterType::Search, search_path);

        // set() fails if already filled - fine to ignore, since parallel test threads can race here.
        if let ParamConfig::Eval(params) = eval_config {
            let _ = EVAL_PARAMS.set(params);
        }
        if let ParamConfig::Search(params) = search_config {
            let _ = SEARCH_PARAMS.set(params);
        }
    }

    pub fn sp() -> &'static SearchParams {
        match SEARCH_PARAMS.get() {
            Some(params) => params,
            None => {
                initialize_params();
                SEARCH_PARAMS.get().unwrap()
            }
        }
    }

    pub fn ep() -> &'static EvalParams {
        match EVAL_PARAMS.get() {
            Some(params) => params,
            None => {
                initialize_params();
                EVAL_PARAMS.get().unwrap()
            }
        }
    }

    enum ParameterType {
        Evaluation,
        Search,
    }

    enum ParamConfig {
        Eval(EvalParams),
        Search(SearchParams),
    }

    fn parse_param_toml(param_type: ParameterType, path: Option<&str>) -> ParamConfig {
        let params_path: &str;
        if path.is_none() {
            match param_type {
                ParameterType::Evaluation => {
                    params_path = concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/src/engine/eval_heuristics.toml"
                    )
                }
                ParameterType::Search => {
                    params_path = concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/src/engine/search_heuristics.toml"
                    )
                }
            }
        } else {
            params_path = path.unwrap();
        }

        let params_str = std::fs::read_to_string(params_path).unwrap();
        let params_table: toml::Table = toml::from_str(&params_str).unwrap();
        let params_vals: toml::Table = params_table
            .into_iter()
            .map(|(name, entry)| (name, entry["value"].clone()))
            .collect();

        match param_type {
            ParameterType::Evaluation => {
                let params: EvalParams = params_vals.try_into().unwrap();
                ParamConfig::Eval(params)
            }
            ParameterType::Search => {
                let params: SearchParams = params_vals.try_into().unwrap();
                ParamConfig::Search(params)
            }
        }
    }
}

#[cfg(feature = "tuning")]
pub use tuning::*;

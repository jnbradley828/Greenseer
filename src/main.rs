mod engine;
mod uci;
use serde::Deserialize;
use toml::{Table, from_str};

use crate::engine::params::{EvalParams, SearchParams};

fn main() {
    let eval_params = parse_param_toml(ParameterType::Evaluation, None);
    let search_params = parse_param_toml(ParameterType::Search, None);

    uci::run();
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
            ParameterType::Evaluation => params_path = "src/engine/eval_heuristics.toml",
            ParameterType::Search => params_path = "src/engine/search_heuristics.toml",
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

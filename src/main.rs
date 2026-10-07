mod engine;
mod uci;

use crate::engine::params::initialize_params;

fn main() {
    initialize_params();
    uci::run();
}

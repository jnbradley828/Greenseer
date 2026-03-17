use oxi_chess_lib;
use rand::Rng;
use rand::prelude::IndexedRandom;

pub fn evaluate(game: &oxi_chess_lib::game::ChessGame) -> u16 {
    let move_sel = game.legal_moves.choose(&mut rand::rng()).unwrap();
    return *move_sel;
}

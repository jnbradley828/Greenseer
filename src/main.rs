mod evaluate;
use oxi_chess_lib::{
    game::{ChessGame, GameResult},
    utils::{self, decode_to_uci, render_board},
};

use crate::evaluate::evaluate;

fn main() {
    let mut game = ChessGame::initialize((1, 1), None);
    while game.result == GameResult::InProgress {
        render_board(&game.board);
        let move_num = game.board.fullmove_number;
        let move_made = evaluate(&game);
        let move_uci = decode_to_uci(move_made).unwrap();
        println!("{move_num}: {move_uci}");
        match game.make_move(move_made) {
            Ok(T) => {}
            Err(e) => {
                println!("{e}");
                for bb in [
                    game.board.pawns,
                    game.board.knights,
                    game.board.bishops,
                    game.board.rooks,
                    game.board.queens,
                    game.board.kings,
                    game.board.white_pieces,
                    game.board.black_pieces,
                ] {
                    utils::print_board_binary(&bb);
                }
            }
        };
    }
    render_board(&game.board);
    let result = game.result;
    println!("{result:?}");
}

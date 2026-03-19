use crate::engine::search::minimax;
use oxi_chess_lib;
use oxi_chess_lib::game::GameResult;

pub fn best_move(game: &mut oxi_chess_lib::game::ChessGame, depth: u8) -> u16 {
    let mut best_move = game.legal_moves[0];
    _ = game.make_move(game.legal_moves[0]);
    let mut best_eval = minimax(game, depth - 1, !game.board.side_to_move);
    _ = game.unmake_move();

    let remaining_moves: Vec<u16> = game.legal_moves[1..].to_vec();
    for movei in remaining_moves {
        _ = game.make_move(movei);
        let movei_eval = minimax(game, depth - 1, !game.board.side_to_move);
        _ = game.unmake_move();
        if movei_eval > best_eval {
            best_eval = movei_eval;
            best_move = movei;
        }
    }

    return best_move;
}

const PIECE_VALUES: [u8; 5] = [1, 3, 3, 5, 9]; // [p, n, b, r, q]
fn count_material(game: &oxi_chess_lib::game::ChessGame) -> i16 {
    let mut material: i16 = 0;

    material +=
        PIECE_VALUES[0] as i16 * (game.board.pawns & game.board.white_pieces).count_ones() as i16;
    material +=
        PIECE_VALUES[1] as i16 * (game.board.knights & game.board.white_pieces).count_ones() as i16;
    material +=
        PIECE_VALUES[2] as i16 * (game.board.bishops & game.board.white_pieces).count_ones() as i16;
    material +=
        PIECE_VALUES[3] as i16 * (game.board.rooks & game.board.white_pieces).count_ones() as i16;
    material +=
        PIECE_VALUES[4] as i16 * (game.board.queens & game.board.white_pieces).count_ones() as i16;

    material -=
        PIECE_VALUES[0] as i16 * (game.board.pawns & game.board.black_pieces).count_ones() as i16;
    material -=
        PIECE_VALUES[1] as i16 * (game.board.knights & game.board.black_pieces).count_ones() as i16;
    material -=
        PIECE_VALUES[2] as i16 * (game.board.bishops & game.board.black_pieces).count_ones() as i16;
    material -=
        PIECE_VALUES[3] as i16 * (game.board.rooks & game.board.black_pieces).count_ones() as i16;
    material -=
        PIECE_VALUES[4] as i16 * (game.board.queens & game.board.black_pieces).count_ones() as i16;

    return material;
}

pub fn evaluate(game: &oxi_chess_lib::game::ChessGame) -> i16 {
    // evaluates WITHOUT future calculation. use minimax to calculate at depth.
    if matches!(game.result, GameResult::Draw(_)) {
        return 0;
    } else if matches!(game.result, GameResult::WhiteWins(_)) {
        return 10000;
    } else if matches!(game.result, GameResult::BlackWins(_)) {
        return -10000;
    } else {
        let mut eval: i16 = 0;
        eval += count_material(game);
        return eval;
    }
}

pub fn unsigned_evaluate(game: &oxi_chess_lib::game::ChessGame, max_side: bool) -> i16 {
    if max_side {
        return evaluate(game);
    } else {
        return -evaluate(game);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_best_move() {
        // White promotes pawn to queen
        let mut game = oxi_chess_lib::game::ChessGame::initialize(
            (1, 1),
            Some("k7/7P/8/8/8/8/8/K7 w - - 0 1"),
        );
        let best_move_uci = oxi_chess_lib::utils::decode_to_uci(best_move(&mut game, 1)).unwrap();
        assert_eq!(best_move_uci, "h7h8q".to_string());

        // White Qb7# or Qa7# (mate in 1): Ka6, Qb6 vs Ka8
        let mut game = oxi_chess_lib::game::ChessGame::initialize(
            (1, 1),
            Some("k7/8/KQ6/8/8/8/8/8 w - - 0 1"),
        );
        let best_move_uci = oxi_chess_lib::utils::decode_to_uci(best_move(&mut game, 1)).unwrap();
        assert!(best_move_uci == "b6b7".to_string() || best_move_uci == "b6a7".to_string());

        // White Rxd5: captures undefended black queen
        let mut game = oxi_chess_lib::game::ChessGame::initialize(
            (1, 1),
            Some("k7/8/8/3q4/8/8/8/K2R4 w - - 0 1"),
        );
        let best_move_uci = oxi_chess_lib::utils::decode_to_uci(best_move(&mut game, 1)).unwrap();
        assert_eq!(best_move_uci, "d1d5".to_string());

        // Black Rxd1: captures undefended white rook
        let mut game = oxi_chess_lib::game::ChessGame::initialize(
            (1, 1),
            Some("k2r4/8/8/8/8/8/8/K2R4 b - - 0 1"),
        );
        let best_move_uci = oxi_chess_lib::utils::decode_to_uci(best_move(&mut game, 1)).unwrap();
        assert_eq!(best_move_uci, "d8d1".to_string());
    }
}

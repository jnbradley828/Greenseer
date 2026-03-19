use oxi_chess_lib::{
    self,
    game::{ChessGame, GameResult},
};

use crate::engine::eval::unsigned_evaluate;

// depth first search tree where you maximize your score and minimize opponent score at each node.
// returns position eval
pub fn minimax(game: &mut ChessGame, depth: u8, max_side: bool) -> i16 {
    if depth == 0 || game.result != GameResult::InProgress {
        return unsigned_evaluate(game, max_side);
    } else {
        let mut evals = Vec::new();
        for movei in game.legal_moves.clone() {
            _ = game.make_move(movei);
            evals.push(minimax(game, depth - 1, max_side));
            _ = game.unmake_move();
        }
        if max_side == game.board.side_to_move {
            return *evals.iter().max().unwrap();
        } else {
            return *evals.iter().min().unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minimax() {
        let mut game = ChessGame::initialize((1, 1), Some("k7/7P/8/8/8/8/8/K7 w - - 0 1"));
        let mm0 = minimax(&mut game, 0, true);
        assert_eq!(mm0, 1);
        let mm1 = minimax(&mut game, 1, true);
        assert_eq!(mm1, 9);

        let mut game = ChessGame::initialize((1, 1), Some("3r4/8/8/8/8/k7/8/K7 b - - 0 1"));
        let mm0 = minimax(&mut game, 0, false);
        assert_eq!(mm0, 5);
        let mm1 = minimax(&mut game, 1, false);
        assert_eq!(mm1, 10000);
    }
}

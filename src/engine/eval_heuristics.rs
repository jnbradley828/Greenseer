pub const PIECE_VALUES: [i16; 6] = [100, 300, 300, 500, 900, 0]; // [p, n, b, r, q, k]
pub const MG_PHASE_SPAN: i16 = 6200; // non-pawn material span for mg/eg phase blend

pub const TT_AGE_FACTOR: i16 = 2;
pub const EARLY_QUEEN_FACTOR: i16 = 10;
pub const OPEN_FILE_ROOK: i16 = 40;
pub const SEMIOPEN_FILE_ROOK: i16 = 20;
pub const TEMPO_BONUS: i16 = 20;
pub const MG_BISHOP_PAIR_BONUS: i16 = 30;
pub const EG_BISHOP_PAIR_BONUS: i16 = 50;
pub const MOBILITY_BONUS: [i16; 4] = [4, 3, 2, 1]; // knight, bishop, rook, queen
pub const MG_DOUBLED_PAWN_PENALTY: i16 = 10;
pub const EG_DOUBLED_PAWN_PENALTY: i16 = 20;
pub const MG_ISOLATED_PAWN_PENALTY: i16 = 10;
pub const EG_ISOLATED_PAWN_PENALTY: i16 = 20;
pub const MG_BACKWARD_PAWN_PENALTY: i16 = 8;
pub const EG_BACKWARD_PAWN_PENALTY: i16 = 15;
pub const MG_PAWN_SHIELD_BONUS: [i16; 3] = [12, 6, 0]; // 1 rank ahead, 2 ranks ahead, weak

// king safety: KING_ATTACK_WEIGHT[piece_type] per attacked square in the enemy king zone.
pub const KING_ATTACK_WEIGHT: [i16; 5] = [1, 2, 2, 3, 5]; // pawn, knight, bishop, rook, queen
pub const KING_ATTACK_LINEAR: f32 = 0.3; // danger = LINEAR*units + QUADRATIC*units^2
pub const KING_ATTACK_QUADRATIC: f32 = 0.07;
pub const MAX_KING_ATTACK_UNITS: i16 = 55; // clamp before the curve is applied
pub const KING_OPEN_FILE_MULT: f32 = 0.15; // danger multiplier per open/semi-open file near king
pub const KING_SEMIOPEN_FILE_MULT: f32 = 0.08;
pub const MG_PASSED_PAWN_BONUS: [i16; 5] = [0, 5, 10, 20, 35]; // ranks 2-6 (rank 7 in MG_PAWN_MOD)
pub const EG_PASSED_PAWN_BONUS: [i16; 5] = [0, 10, 20, 40, 65];

#[rustfmt::skip]
pub const MG_PAWN_MOD: [i8; 64] = [
    0,   0,   0,   0,   0,   0,   0,   0,   // rank 1
    5,  10,  10, -20, -20,  10,  10,   5,   // rank 2
    5,  -5, -10,   0,   0, -10,  -5,   5,   // rank 3
    0,   0,  20,  22,  22,  20,   0,   0,   // rank 4
    5,   5,  15,  25,  25,  15,   5,   5,   // rank 5
   10,  10,  20,  30,  30,  20,  10,  10,   // rank 6
   60,  60,  60,  60,  60,  60,  60,  60,   // rank 7 (includes guaranteed-passed bonus)
    0,   0,   0,   0,   0,   0,   0,   0    // rank 8
];

#[rustfmt::skip]
pub const MG_KNIGHT_MOD: [i8; 64] = [
  -50, -40, -30, -30, -30, -30, -40, -50,   // rank 1
  -40, -20,   0,   5,   5,   0, -20, -40,   // rank 2
  -30,   5,  10,  15,  15,  10,   5, -30,   // rank 3
  -30,   0,  15,  20,  20,  15,   0, -30,   // rank 4
  -30,   5,  15,  20,  20,  15,   5, -30,   // rank 5
  -30,   0,  10,  15,  15,  10,   0, -30,   // rank 6
  -40, -20,   0,   0,   0,   0, -20, -40,   // rank 7
  -50, -40, -30, -30, -30, -30, -40, -50    // rank 8
];

#[rustfmt::skip]
pub const MG_BISHOP_MOD: [i8; 64] = [
  -20, -10, -10, -10, -10, -10, -10, -20,   // rank 1
  -10,   5,   0,   0,   0,   0,   5, -10,   // rank 2
  -10,   0,   5,  10,  10,   5,   0, -10,   // rank 3
  -10,   0,  10,  15,  15,  10,   0, -10,   // rank 4
  -10,   0,  10,  15,  15,  10,   0, -10,   // rank 5
  -10,   0,   5,  10,  10,   5,   0, -10,   // rank 6
  -10,   5,   0,   0,   0,   0,   5, -10,   // rank 7
  -20, -10, -10, -10, -10, -10, -10, -20    // rank 8
];

#[rustfmt::skip]
pub const MG_ROOK_MOD: [i8; 64] = [
    0,   0,   3,   5,   5,   3,   0,   0,   // rank 1
   -5,   0,   0,   0,   0,   0,   0,  -5,   // rank 2
   -5,   0,   0,   0,   0,   0,   0,  -5,   // rank 3
   -5,   0,   0,   0,   0,   0,   0,  -5,   // rank 4
   -5,   0,   0,   0,   0,   0,   0,  -5,   // rank 5
   -5,   0,   0,   0,   0,   0,   0,  -5,   // rank 6
    5,  10,  10,  10,  10,  10,  10,   5,   // rank 7
    0,   0,   0,   0,   0,   0,   0,   0    // rank 8
];

#[rustfmt::skip]
pub const MG_QUEEN_MOD: [i8; 64] = [
  -20, -10, -10,  -5,  -5, -10, -10, -20,   // rank 1
  -10,   0,   0,   0,   0,   0,   0, -10,   // rank 2
  -10,   0,   5,   5,   5,   5,   0, -10,   // rank 3
   -5,   0,   5,   5,   5,   5,   0,  -5,   // rank 4
   -5,   0,   5,   5,   5,   5,   0,  -5,   // rank 5
  -10,   0,   5,   5,   5,   5,   0, -10,   // rank 6
  -10,   0,   0,   0,   0,   0,   0, -10,   // rank 7
  -20, -10, -10,  -5,  -5, -10, -10, -20    // rank 8
];

#[rustfmt::skip]
pub const MG_KING_MOD: [i8; 64] = [
   20,  30,  10,   0,   0,  10,  30,  20,   // rank 1
   20,  20,   0,   0,   0,   0,  20,  20,   // rank 2
  -10, -20, -20, -20, -20, -20, -20, -10,   // rank 3
  -20, -30, -30, -40, -40, -30, -30, -20,   // rank 4
  -30, -40, -40, -50, -50, -40, -40, -30,   // rank 5
  -30, -40, -40, -50, -50, -40, -40, -30,   // rank 6
  -30, -40, -40, -50, -50, -40, -40, -30,   // rank 7
  -30, -40, -40, -50, -50, -40, -40, -30    // rank 8
];

#[rustfmt::skip]
pub const EG_PAWN_MOD: [i8; 64] = [
    0,   0,   0,   0,   0,   0,   0,   0,
   -5,  -5,  -5,  -5,  -5,  -5,  -5,  -5,
    0,   0,   0,   0,   0,   0,   0,   0,
    5,   5,   5,   5,   5,   5,   5,   5,
   15,  15,  15,  15,  15,  15,  15,  15,
   30,  30,  30,  30,  30,  30,  30,  30,
   95, 100, 100, 100, 100, 100, 100,  95,   // rank 7 (includes guaranteed-passed bonus)
    0,   0,   0,   0,   0,   0,   0,   0
];

#[rustfmt::skip]
pub const EG_KNIGHT_MOD: [i8; 64] = [
  -60, -50, -40, -40, -40, -40, -50, -60,
  -50, -30, -15, -10, -10, -15, -30, -50,
  -40, -15,   5,   8,   8,   5, -15, -40,
  -40, -10,   8,  15,  15,   8, -10, -40,
  -40, -10,   8,  15,  15,   8, -10, -40,
  -40, -15,   5,   8,   8,   5, -15, -40,
  -50, -30, -15, -10, -10, -15, -30, -50,
  -60, -50, -40, -40, -40, -40, -50, -60
];

#[rustfmt::skip]
pub const EG_BISHOP_MOD: [i8; 64] = [
  -15,  -5,  -5,  -5,  -5,  -5,  -5, -15,
   -5,   0,   0,   0,   0,   0,   0,  -5,
   -5,   0,   8,  10,  10,   8,   0,  -5,
   -5,   0,  10,  15,  15,  10,   0,  -5,
   -5,   0,  10,  15,  15,  10,   0,  -5,
   -5,   0,   8,  10,  10,   8,   0,  -5,
   -5,   5,   0,   0,   0,   0,   5,  -5,
  -15,  -5,  -5,  -5,  -5,  -5,  -5, -15
];

#[rustfmt::skip]
pub const EG_ROOK_MOD: [i8; 64] = [
   -5,  -5,  -5,  -5,  -5,  -5,  -5,  -5,
    0,   0,   0,   0,   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   0,
    5,   5,   5,   5,   5,   5,   5,   5,
   20,  20,  20,  20,  20,  20,  20,  20,
    5,   5,   5,   5,   5,   5,   5,   5
];

#[rustfmt::skip]
pub const EG_QUEEN_MOD: [i8; 64] = [
  -20, -10, -10,  -5,  -5, -10, -10, -20,
  -10,   0,   5,   5,   5,   5,   0, -10,
  -10,   5,   8,  10,  10,   8,   5, -10,
   -5,   5,  10,  12,  12,  10,   5,  -5,
   -5,   5,  10,  12,  12,  10,   5,  -5,
  -10,   5,   8,  10,  10,   8,   5, -10,
  -10,   0,   5,   5,   5,   5,   0, -10,
  -20, -10, -10,  -5,  -5, -10, -10, -20
];

#[rustfmt::skip]
pub const EG_KING_MOD: [i8; 64] = [
  -20, -10,   0,   5,   5,   0, -10, -20,
  -10,   5,  15,  20,  20,  15,   5, -10,
    0,  15,  25,  30,  30,  25,  15,   0,
    5,  20,  30,  35,  35,  30,  20,   5,
    5,  20,  30,  35,  35,  30,  20,   5,
    0,  15,  25,  30,  30,  25,  15,   0,
  -10,   5,  15,  20,  20,  15,   5, -10,
  -20, -10,   0,   5,   5,   0, -10, -20
];

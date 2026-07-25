pub const PIECE_VALUES: [i16; 5] = [100, 300, 300, 500, 900]; // [p, n, b, r, q]
// span of combined (both sides) non-pawn material over which the mg/eg phase blend ramps
// linearly from 0 to 1 - standalone tunable constant rather than derived from PIECE_VALUES,
// since it's fit independently during Texel tuning like everything else here. starting value =
// a full board's non-pawn material (2*(2N+2B+2R+Q) = 6200).
pub const MG_PHASE_SPAN: i16 = 6200;

pub const TT_AGE_FACTOR: i16 = 2;
pub const EARLY_QUEEN_FACTOR: i16 = 10;
pub const OPEN_FILE_ROOK: i16 = 20;
pub const SEMIOPEN_FILE_ROOK: i16 = 10;
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
// per shield file: [pawn one rank ahead of king (ideal), pawn two ranks ahead (pushed once),
// pawn further advanced or missing entirely (weak)]. middlegame-only - fades out via the
// existing mg/eg phase weighting since king safety isn't relevant once queens are traded off.
pub const MG_PAWN_SHIELD_BONUS: [i16; 3] = [12, 6, 0];

// weight per attacked square, indexed by piece_type [pawn, knight, bishop, rook, queen] - how
// many "attack units" a single square of overlap between a piece's attacks and the enemy king
// zone is worth.
pub const KING_ATTACK_WEIGHT: [i16; 5] = [1, 2, 2, 3, 5];
// scales total raw attack units into a middlegame score via a polynomial blend:
// KING_ATTACK_LINEAR * units + KING_ATTACK_QUADRATIC * units^2. the quadratic term makes
// danger compound superlinearly as attackers pile up, while the linear term keeps small
// attacker counts from rounding away to nothing. both are continuous and cheap (plain
// multiplication, no powf/sqrt) - the ratio between them lets tuning shift smoothly between
// "barely superlinear" and "sharply compounding" without needing a tunable exponent.
pub const KING_ATTACK_LINEAR: f32 = 0.3;
pub const KING_ATTACK_QUADRATIC: f32 = 0.07;
// raw attack units are clamped to this before the curve is applied, so a degenerate position
// with many overlapping attackers can't produce an unbounded eval swing.
pub const MAX_KING_ATTACK_UNITS: i16 = 55;
// multiplier on the king-attack danger score (not a standalone score) per open/semi-open file
// among the king's own file and both adjacent files. structured as a multiplier rather than an
// independent penalty so a weak file with no real attackers contributes exactly zero danger -
// it only matters once combined with actual attacking pressure, avoiding false positives like
// a normal pre-castling central pawn trade being treated as a permanent weakness.
pub const KING_OPEN_FILE_MULT: f32 = 0.15;
pub const KING_SEMIOPEN_FILE_MULT: f32 = 0.08;
// ranks 2-6 (white) / 7-3 (black). rank 7/2 (guaranteed passed - no enemy pawn can ever
// occupy rank 8/1) is baked directly into MG_PAWN_MOD/EG_PAWN_MOD's rank 7 row instead,
// since every pawn there is passed by definition and doesn't need a separate check.
pub const MG_PASSED_PAWN_BONUS: [i16; 5] = [0, 5, 10, 20, 35];
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

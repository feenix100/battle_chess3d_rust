//! Small built-in chess CPU.
//!
//! The browser Battle Chess uses a shallow alpha-beta search with material and
//! positional evaluation. This native port mirrors that approach so it stays
//! self-contained and does not require Stockfish or another external process.

use shakmaty::{Chess, Color, Move, Position, Role};

use crate::game::{square_from_indices, square_indices};

const MATE_SCORE: f32 = 100_000.0;

pub fn choose_cpu_move(position: &Chess, cpu_color: Color) -> Option<Move> {
    let mut moves: Vec<Move> = position.legal_moves().into_iter().collect();
    if moves.is_empty() {
        return None;
    }

    order_moves(&mut moves);
    let depth = if moves.len() <= 16 { 3 } else { 2 };
    let mut best_score = f32::NEG_INFINITY;
    let mut best_move = moves[0];

    for mv in moves {
        let Ok(next) = position.clone().play(mv) else {
            continue;
        };
        let score = search(
            &next,
            cpu_color,
            depth - 1,
            f32::NEG_INFINITY,
            f32::INFINITY,
        );
        if score > best_score {
            best_score = score;
            best_move = mv;
        }
    }

    Some(best_move)
}

fn search(position: &Chess, cpu_color: Color, depth: usize, mut alpha: f32, mut beta: f32) -> f32 {
    if depth == 0 || position.is_checkmate() || position.is_stalemate() {
        return evaluate(position, cpu_color);
    }

    let maximizing = position.turn() == cpu_color;
    let mut moves: Vec<Move> = position.legal_moves().into_iter().collect();
    if moves.is_empty() {
        return evaluate(position, cpu_color);
    }
    order_moves(&mut moves);

    if maximizing {
        let mut best = f32::NEG_INFINITY;
        for mv in moves {
            let Ok(next) = position.clone().play(mv) else {
                continue;
            };
            best = best.max(search(&next, cpu_color, depth - 1, alpha, beta));
            alpha = alpha.max(best);
            if beta <= alpha {
                break;
            }
        }
        best
    } else {
        let mut best = f32::INFINITY;
        for mv in moves {
            let Ok(next) = position.clone().play(mv) else {
                continue;
            };
            best = best.min(search(&next, cpu_color, depth - 1, alpha, beta));
            beta = beta.min(best);
            if beta <= alpha {
                break;
            }
        }
        best
    }
}

fn evaluate(position: &Chess, cpu_color: Color) -> f32 {
    if position.is_checkmate() {
        return if position.turn() == cpu_color {
            -MATE_SCORE
        } else {
            MATE_SCORE
        };
    }
    if position.is_stalemate() || position.is_insufficient_material() {
        return 0.0;
    }

    let mut total_material = 0.0;
    for rank in 0..8 {
        for file in 0..8 {
            if let Some(piece) = position.board().piece_at(square_from_indices(file, rank))
                && piece.role != Role::King
            {
                total_material += piece_value(piece.role);
            }
        }
    }

    let mut score = 0.0;
    for rank in 0..8 {
        for file in 0..8 {
            let square = square_from_indices(file, rank);
            let Some(piece) = position.board().piece_at(square) else {
                continue;
            };
            let value = piece_value(piece.role);
            let positional = square_bonus(piece.role, piece.color, square, total_material);
            let signed = value + positional;
            score += if piece.color == cpu_color {
                signed
            } else {
                -signed
            };
        }
    }

    if position.is_check() {
        score += if position.turn() == cpu_color {
            -28.0
        } else {
            28.0
        };
    }
    score
}

fn order_moves(moves: &mut [Move]) {
    moves.sort_by(|a, b| move_order_score(b).total_cmp(&move_order_score(a)));
}

fn move_order_score(mv: &Move) -> f32 {
    let mut score = 0.0;
    if let Some(captured) = (*mv).capture() {
        score += piece_value(captured) * 10.0 - piece_value((*mv).role());
    }
    if let Some(promotion) = (*mv).promotion() {
        score += piece_value(promotion) + 700.0;
    }
    if (*mv).is_castle() {
        score += 45.0;
    }
    score
}

fn piece_value(role: Role) -> f32 {
    match role {
        Role::Pawn => 100.0,
        Role::Knight => 320.0,
        Role::Bishop => 330.0,
        Role::Rook => 500.0,
        Role::Queen => 900.0,
        Role::King => 0.0,
    }
}

fn square_bonus(role: Role, color: Color, square: shakmaty::Square, total_material: f32) -> f32 {
    let (file, rank) = square_indices(square);
    let file = file as f32;
    let rank = rank as f32;
    let center_distance = (file - 3.5).abs() + (rank - 3.5).abs();
    let center = (4.0 - center_distance).max(0.0);
    let advance = match color {
        Color::White => rank,
        Color::Black => 7.0 - rank,
    };

    match role {
        Role::Pawn => advance * 4.0 + center * 2.0,
        Role::Knight => center * 10.0,
        Role::Bishop => center * 6.0,
        Role::Rook => advance * 1.5 + center * 1.5,
        Role::Queen => center * 2.0,
        Role::King => {
            if total_material < 2600.0 {
                center * 7.0
            } else {
                -center * 5.0
            }
        }
    }
}

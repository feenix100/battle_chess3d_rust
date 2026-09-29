//! Pure chess rules, history, captures, draw tracking, and undo support.
//!
//! Rendering and input intentionally stay out of this module so the same model can
//! be reused by the native scene, CPU search, tests, or a future headless mode.

use std::collections::HashMap;

use anyhow::{Result, anyhow};
use shakmaty::{Chess, Color, File, Move, Outcome, Piece, Position, Rank, Role, Square, san::San};

/// A high-level terminal game result used by the native UI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GameOutcome {
    Checkmate { winner: Color },
    Stalemate,
    InsufficientMaterial,
    FiftyMoveRule,
    ThreefoldRepetition,
}

/// Captured material recorded with the move that removed it.
#[derive(Clone, Copy, Debug)]
pub struct CapturedPiece {
    pub piece: Piece,
    pub captured_by: Color,
}

/// Immutable information for one played move.
#[derive(Clone, Debug)]
struct MoveRecord {
    san: String,
    played_move: Move,
    previous_position: Chess,
    captured: Option<CapturedPiece>,
}

/// Rules state for one chess game.
#[derive(bevy::prelude::Resource)]
pub struct BattleGame {
    position: Chess,
    history: Vec<MoveRecord>,
    repetition_counts: HashMap<Chess, u32>,
}

impl Default for BattleGame {
    fn default() -> Self {
        Self::new()
    }
}

impl BattleGame {
    /// Creates the standard chess starting position.
    pub fn new() -> Self {
        let position = Chess::default();
        let mut repetition_counts = HashMap::new();
        repetition_counts.insert(position.clone(), 1);

        Self {
            position,
            history: Vec::new(),
            repetition_counts,
        }
    }

    pub fn position(&self) -> &Chess {
        &self.position
    }

    pub fn piece_at(&self, square: Square) -> Option<Piece> {
        self.position.board().piece_at(square)
    }

    pub fn side_to_move(&self) -> Color {
        self.position.turn()
    }

    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    pub fn has_started(&self) -> bool {
        !self.history.is_empty()
    }

    pub fn can_undo(&self) -> bool {
        !self.history.is_empty()
    }

    pub fn is_check(&self) -> bool {
        self.position.is_check()
    }

    pub fn legal_moves_from(&self, square: Square) -> Vec<Move> {
        self.position
            .legal_moves()
            .into_iter()
            .filter(|candidate| move_from(candidate) == Some(square))
            .collect()
    }

    pub fn legal_moves_between(&self, from: Square, to: Square) -> Vec<Move> {
        self.legal_moves_from(from)
            .into_iter()
            .filter(|candidate| display_move_to(candidate) == Some(to))
            .collect()
    }

    /// Plays a legal move and records SAN, previous position, and captured material.
    pub fn play(&mut self, mv: Move) -> Result<()> {
        if !self.position.is_legal(mv) {
            return Err(anyhow!("illegal move"));
        }

        let previous_position = self.position.clone();
        let moving_color = previous_position.turn();
        let captured = captured_piece_for_move(&mv, moving_color);
        let san = San::from_move(&previous_position, mv).to_string();
        let next_position = previous_position.clone().play(mv)?;

        self.position = next_position.clone();
        *self.repetition_counts.entry(next_position).or_insert(0) += 1;
        self.history.push(MoveRecord {
            san,
            played_move: mv,
            previous_position,
            captured,
        });
        Ok(())
    }

    /// Undoes the most recent move and returns it.
    pub fn undo(&mut self) -> Option<Move> {
        let record = self.history.pop()?;

        if let Some(count) = self.repetition_counts.get_mut(&self.position) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.repetition_counts.remove(&self.position);
            }
        }

        self.position = record.previous_position;
        Some(record.played_move)
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn san_history(&self) -> Vec<&str> {
        self.history
            .iter()
            .map(|record| record.san.as_str())
            .collect()
    }

    pub fn formatted_history(&self) -> Vec<String> {
        let san = self.san_history();
        let mut rows = Vec::new();
        for index in (0..san.len()).step_by(2) {
            let move_number = index / 2 + 1;
            let white = san[index];
            let black = san.get(index + 1).copied().unwrap_or("");
            rows.push(format!("{move_number:>2}. {white:<9} {black}"));
        }
        rows
    }

    pub fn captured_pieces(&self) -> Vec<CapturedPiece> {
        self.history
            .iter()
            .filter_map(|record| record.captured)
            .collect()
    }

    pub fn last_move_squares(&self) -> Option<(Square, Square)> {
        let mv = &self.history.last()?.played_move;
        Some((move_from(mv)?, display_move_to(mv)?))
    }

    pub fn outcome(&self) -> Option<GameOutcome> {
        if self.position.is_checkmate() {
            return Some(GameOutcome::Checkmate {
                winner: opposite_color(self.position.turn()),
            });
        }
        if self.position.is_stalemate() {
            return Some(GameOutcome::Stalemate);
        }
        if self.position.is_insufficient_material() {
            return Some(GameOutcome::InsufficientMaterial);
        }
        if self.position.halfmoves() >= 100 {
            return Some(GameOutcome::FiftyMoveRule);
        }
        if self
            .repetition_counts
            .get(&self.position)
            .copied()
            .unwrap_or_default()
            >= 3
        {
            return Some(GameOutcome::ThreefoldRepetition);
        }

        match self.position.outcome() {
            Outcome::Unknown => None,
            Outcome::Known(_) => None,
        }
    }
}

/// User-facing status text shared by the 3D HUD.
pub fn status_text(game: &BattleGame) -> String {
    if let Some(outcome) = game.outcome() {
        return match outcome {
            GameOutcome::Checkmate {
                winner: Color::White,
            } => "Checkmate — White wins".to_owned(),
            GameOutcome::Checkmate {
                winner: Color::Black,
            } => "Checkmate — Black wins".to_owned(),
            GameOutcome::Stalemate => "Stalemate — draw".to_owned(),
            GameOutcome::InsufficientMaterial => "Draw — insufficient material".to_owned(),
            GameOutcome::FiftyMoveRule => "Draw — 50-move rule".to_owned(),
            GameOutcome::ThreefoldRepetition => "Draw — threefold repetition".to_owned(),
        };
    }

    let side = color_name(game.side_to_move());
    if game.is_check() {
        format!("Check! {side} to move")
    } else {
        format!("{side} to move")
    }
}

pub fn color_name(color: Color) -> &'static str {
    match color {
        Color::White => "White",
        Color::Black => "Black",
    }
}

pub fn opposite_color(color: Color) -> Color {
    match color {
        Color::White => Color::Black,
        Color::Black => Color::White,
    }
}

pub fn square_from_indices(file_index: usize, rank_index: usize) -> Square {
    Square::from_coords(File::new(file_index as u32), Rank::new(rank_index as u32))
}

pub fn square_indices(square: Square) -> (usize, usize) {
    let (file, rank) = square.coords();
    (file.to_usize(), rank.to_usize())
}

pub fn move_from(mv: &Move) -> Option<Square> {
    match mv {
        Move::Normal { from, .. } => Some(*from),
        Move::EnPassant { from, .. } => Some(*from),
        Move::Castle { king, .. } => Some(*king),
        Move::Put { .. } => None,
    }
}

/// Returns the square that the moving piece visually occupies after a move.
///
/// Shakmaty encodes castling with the rook square as the move target. The native
/// renderer uses the king destination instead.
pub fn display_move_to(mv: &Move) -> Option<Square> {
    match mv {
        Move::Normal { to, .. } => Some(*to),
        Move::EnPassant { to, .. } => Some(*to),
        Move::Castle { king, .. } => mv
            .castling_side()
            .map(|side| Square::from_coords(side.king_to_file(), king.rank())),
        Move::Put { to, .. } => Some(*to),
    }
}

/// Returns the actual square occupied by a capture victim before the move.
///
/// This differs from the destination for en passant.
pub fn capture_square(mv: &Move) -> Option<Square> {
    match mv {
        Move::Normal {
            capture: Some(_),
            to,
            ..
        } => Some(*to),
        Move::EnPassant { from, to } => Some(Square::from_coords(to.file(), from.rank())),
        _ => None,
    }
}

fn captured_piece_for_move(mv: &Move, moving_color: Color) -> Option<CapturedPiece> {
    let role = match mv {
        Move::Normal {
            capture: Some(role),
            ..
        } => *role,
        Move::EnPassant { .. } => Role::Pawn,
        _ => return None,
    };

    Some(CapturedPiece {
        piece: Piece {
            color: opposite_color(moving_color),
            role,
        },
        captured_by: moving_color,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play_between(game: &mut BattleGame, from: Square, to: Square) {
        let mv = game
            .legal_moves_between(from, to)
            .into_iter()
            .next()
            .expect("expected a legal move between test squares");
        game.play(mv).expect("test move should be legal");
    }

    #[test]
    fn starting_position_has_twenty_legal_moves() {
        let game = BattleGame::new();
        assert_eq!(game.position().legal_moves().len(), 20);
        assert_eq!(game.side_to_move(), Color::White);
        assert_eq!(game.history_len(), 0);
    }

    #[test]
    fn play_capture_and_undo_restore_state() {
        let mut game = BattleGame::new();
        play_between(&mut game, Square::E2, Square::E4);
        play_between(&mut game, Square::D7, Square::D5);
        play_between(&mut game, Square::E4, Square::D5);

        let captured = game.captured_pieces();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].piece.role, Role::Pawn);
        assert_eq!(captured[0].piece.color, Color::Black);
        assert_eq!(captured[0].captured_by, Color::White);
        assert_eq!(game.piece_at(Square::D5), Some(Color::White.pawn()));

        assert!(game.undo().is_some());
        assert!(game.captured_pieces().is_empty());
        assert_eq!(game.piece_at(Square::E4), Some(Color::White.pawn()));
        assert_eq!(game.piece_at(Square::D5), Some(Color::Black.pawn()));
        assert_eq!(game.side_to_move(), Color::White);
    }

    #[test]
    fn en_passant_removes_the_actual_victim_square() {
        let mut game = BattleGame::new();
        play_between(&mut game, Square::E2, Square::E4);
        play_between(&mut game, Square::A7, Square::A6);
        play_between(&mut game, Square::E4, Square::E5);
        play_between(&mut game, Square::D7, Square::D5);

        let mv = game
            .legal_moves_between(Square::E5, Square::D6)
            .into_iter()
            .find(|candidate| candidate.is_en_passant())
            .expect("en passant should be legal");
        assert_eq!(capture_square(&mv), Some(Square::D5));

        game.play(mv).expect("en passant should play");
        assert_eq!(game.piece_at(Square::D5), None);
        assert_eq!(game.piece_at(Square::D6), Some(Color::White.pawn()));

        let captured = game.captured_pieces();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].piece, Color::Black.pawn());
    }
}

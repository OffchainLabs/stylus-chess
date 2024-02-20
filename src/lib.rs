// Only run this as a WASM if the export-abi feature is not set.
#![cfg_attr(not(feature = "export-abi"), no_main)]
extern crate alloc;

/// Initializes a custom, global allocator for Rust programs compiled to WASM.
#[global_allocator]
static ALLOC: mini_alloc::MiniAlloc = mini_alloc::MiniAlloc::INIT;

use alloy_primitives::{Address, U8};
use chess_engine::{Board, BoardBuilder, Color, Evaluate, Move, Piece, Position};

/// Import the Stylus SDK along with alloy primitive types for use in our program.
use stylus_sdk::{alloy_primitives::U256, msg, prelude::*, storage::StorageU8};

/// Game Status
const PENDING: u8 = 0;
const CONTINUING: u8 = 1;
const STALEMATE: u8 = 2;
const VICTORY: u8 = 3;

/// Colors
const WHITE: u8 = 1;
const BLACK: u8 = 2;

/// Piece types
const PAWN: u8 = 1;
const KNIGHT: u8 = 2;
const BISHOP: u8 = 3;
const ROOK: u8 = 4;
const QUEEN: u8 = 5;
const KING: u8 = 6;

sol_storage! {
  #[entrypoint]
  pub struct StylusChess {
    /// Total games of chess started
    uint256 total_games;
    /// Used to store a single pending game while waiting for a player two to join.
    uint256 pending_game;
    /// Stores info for each chess game
    mapping(uint256 => GameInfo) games;
  }

  pub struct GameInfo {
    /// Player 1 is WHITE
    address player_one;
    /// Player 2 is BLACK
    address player_two;
    /// PENDING (waiting second player) = 0, CONTINUING = 1, STALEMATE = 2, or VICTORY = 3
    uint8 game_status;
    /// 1 = WHITE; 2 = BLACK
    uint8 victor;
    /// All the info needed to rebuild the board
    StorageChessPiece[8][8] pieces;
  }

  pub struct StorageChessPiece {
    /// 1 for WHITE; 2 for BLACK
    uint8 color;
    /// 1 = Pawn, 2 = Knight, 3 = Bishop, 4 = Rook, 5 = Queen, 6 = King
    uint8 piece_type;
  }
}

type GameInfoReturnType = (Address, Address, U256, U256);
type GamePiecesReturnType = Vec<(U256, U256, U256, U256)>;

struct ChessPiece {
    color: U8,
    row: U8,
    col: U8,
    piece_type: U8,
}

#[external]
impl StylusChess {
    /// Gets the game number from storage.
    pub fn total_games(&self) -> Result<U256, Vec<u8>> {
        Ok(self.total_games.get())
    }

    /// Game info
    pub fn game_by_number(&self, game_number: U256) -> Result<GameInfoReturnType, Vec<u8>> {
        let game_info = self.games.getter(game_number);

        let player_one = game_info.player_one.get();
        let player_two = game_info.player_two.get();
        let game_status = game_info.game_status.get();
        let victor = game_info.victor.get();

        let data: GameInfoReturnType = (
            player_one,
            player_two,
            U256::from(game_status),
            U256::from(victor),
        );

        Ok(data)
    }

    /// The board layout for a particular game
    pub fn game_pieces_by_game_number(
        &self,
        game_number: U256,
    ) -> Result<GamePiecesReturnType, Vec<u8>> {
        let game_info = self.games.getter(game_number);

        // let pieces_length = game_info.pieces.len();
        let mut pieces = GamePiecesReturnType::new();

        for row in 0..8_u8 {
            for col in 0..8_u8 {
                if let Some(row_vec) = game_info.pieces.getter(row) {
                    if let Some(piece) = row_vec.getter(col) {
                        pieces.push((
                            U256::from(piece.color.get()),
                            U256::from(row),
                            U256::from(col),
                            U256::from(piece.piece_type.get()),
                        ));
                    }
                }
            }
        }

        Ok(pieces)
    }

    /// Either creates a new game or joins a pending game if it exists
    /// Returns the game number
    pub fn create_or_join(&mut self) -> Result<U256, Vec<u8>> {
        let pending_game = self.pending_game.get();

        // Sets pending_game number and initializes a new board
        if pending_game == U256::from(0) {
            let game_number = self.get_next_game_number();
            self.pending_game.set(game_number);
            self.create_game(game_number);
            return Ok(game_number);
        }

        self.join_game(pending_game);

        Ok(pending_game)
    }
}

impl StylusChess {
    fn get_next_game_number(&mut self) -> U256 {
        let game_number = self.total_games.get() + U256::from(1);
        self.total_games.set(game_number);
        game_number
    }

    fn create_game(&mut self, game_number: U256) {
        let board = Board::default();
        // Set up pieces for serialization
        let pieces = self.serialize_board(board);

        let mut game_info = self.games.setter(game_number);
        game_info.player_one.set(msg::sender());

        for piece in pieces {
            let row = piece.row;
            let col = piece.col;

            if let Some(mut row_vec) = game_info.pieces.setter(row) {
                if let Some(mut chess_piece) = row_vec.setter(col) {
                    chess_piece.color.set(piece.color);
                    chess_piece.piece_type.set(piece.piece_type);
                }
            }
        }
    }

    fn join_game(&mut self, game_number: U256) {
        let mut game_info = self.games.setter(game_number);
        // join as player two
        game_info.player_two.set(msg::sender());
        // change status to continuing
        game_info.game_status.set(U8::from(CONTINUING));
        // empty out pending_game
        self.pending_game.set(U256::from(0));
    }

    fn serialize_board(&self, board: Board) -> Vec<ChessPiece> {
        let mut chess_pieces = Vec::<ChessPiece>::new();

        for row in 0..8 {
            for col in 0..8 {
                let position = Position::new(row, col);
                if let Some(board_piece) = board.get_piece(position) {
                    let color = if board_piece.get_color() == Color::White {
                        U8::from(WHITE)
                    } else {
                        U8::from(BLACK)
                    };

                    let piece_type = match board_piece {
                        Piece::Pawn(_, _) => U8::from(PAWN),
                        Piece::Knight(_, _) => U8::from(KNIGHT),
                        Piece::Bishop(_, _) => U8::from(BISHOP),
                        Piece::Rook(_, _) => U8::from(ROOK),
                        Piece::Queen(_, _) => U8::from(QUEEN),
                        Piece::King(_, _) => U8::from(KING),
                    };

                    let piece_tuple = ChessPiece {
                        color,
                        row: U8::from(row),
                        col: U8::from(col),
                        piece_type,
                    };

                    chess_pieces.push(piece_tuple)
                }
            }
        }

        chess_pieces
    }
}

// fn deserialize_board(&self, game_number: U256) -> Board {
//     let game_info = self.games.getter(game_number);

//     let mut board_builder: BoardBuilder;

//     for row in 0..8_i32 {
//         for col in 0..8_i32 {
//             if let Some(row_vec) = game_info.pieces.getter(row) {
//                 if let Some(storage_piece) = row_vec.getter(col) {
//                     let color = storage_piece.color.get();
//                     let piece_type = storage_piece.piece_type.get();

//                     if (color != 0 && piece_type != 0) {
//                         let pos = Position::new(row, col);
//                         let color_enum = match color {
//                             U8::from(WHITE) => Color::White,
//                             _ => Color::Black,
//                         };

//                         // let piece = match piece_type {
//                         //     _ => Piece::Pawn(color_enum, pos),
//                         //     U8::from(KNIGHT) => Piece::Knight(color_enum, pos),
//                         //     U8::from(BISHOP) => Piece::Bishop(color_enum, pos),
//                         //     U8::from(ROOK) => Piece::Rook(color_enum, pos),
//                         //     U8::from(QUEEN) => Piece::Queen(color_enum, pos),
//                         //     U8::from(KING) => Piece::King(color_enum, pos),
//                         // };

//                         // board_builder = board_builder.piece()
//                     }
//                 }
//             }
//         }
//     }

//     Board::default()
// }

// Only run this as a WASM if the export-abi feature is not set.
#![cfg_attr(not(feature = "export-abi"), no_main)]
extern crate alloc;

/// Initializes a custom, global allocator for Rust programs compiled to WASM.
#[global_allocator]
static ALLOC: mini_alloc::MiniAlloc = mini_alloc::MiniAlloc::INIT;

use alloy_primitives::{hex, Address, Uint, B256, U64, U8};
use chess_engine::{Board, BoardBuilder, Color, Evaluate, GameResult, Move, Piece, Position};

use stylus_sdk::console;
/// Import the Stylus SDK along with alloy primitive types for use in our program.
use stylus_sdk::{alloy_primitives::U256, msg, prelude::*, storage::StorageU8};

/// Game Status
const PENDING: u8 = 0;
const CONTINUING: u8 = 1;
const ILLEGAL_MOVE: u8 = 2;
const STALEMATE: u8 = 3;
const VICTORY: u8 = 4;

/// Colors
const WHITE: u8 = 0;
const BLACK: u8 = 1;

/// Piece types
const PAWN: u8 = 1;
const KNIGHT: u8 = 2;
const BISHOP: u8 = 3;
const ROOK: u8 = 4;
const QUEEN: u8 = 5;
const KING: u8 = 6;

/// Bit masks
const COLOR_MASK: u8 = 1;
const PIECE_TYPE_MASK: u8 = 7;

sol_storage! {
  #[entrypoint]
  pub struct StylusChess {
    /// Total games of chess started
    uint64 total_games;
    /// Used to store a single pending game while waiting for a player two to join.
    uint64 pending_game;
    /// Stores info for each chess game
    mapping(uint64 => GameInfo) games;
  }

  pub struct GameInfo {
    /// Player 1 is WHITE
    address player_one;
    /// Player 2 is BLACK
    address player_two;
    /// PENDING (waiting second player) = 0, CONTINUING = 1, STALEMATE = 3, or VICTORY = 4
    uint8 game_status;
    /// 0 = WHITE; 1 = BLACK
    uint8 victor;
    /// All the info needed to rebuild the board
    uint256 board_state;
  }
}

type GameInfoReturnType = (Address, Address, U256, U256);

#[external]
impl StylusChess {
    /// Gets the game number from storage.
    pub fn total_games(&self) -> Result<U256, Vec<u8>> {
        Ok(U256::from(self.total_games.get()))
    }

    /// Get the color of the current player
    pub fn get_turn_color(&self, game_number: U256) -> Result<U256, Vec<u8>> {
        let current_board = self.get_board_from_game_number(U64::from(game_number));
        let current_color = match current_board.get_turn_color() {
            Color::White => U256::from(WHITE),
            Color::Black => U256::from(BLACK),
        };

        Ok(current_color)
    }

    /// Get the address of the current player
    pub fn get_current_player(&self, game_number: U256) -> Result<Address, Vec<u8>> {
        let current_player = self.get_current_player_address(U64::from(game_number));
        Ok(current_player)
    }

    /// Play a Move
    pub fn play_move(
        &mut self,
        game_number: U256,
        from_pos: (U256, U256),
        to_pos: (U256, U256),
    ) -> Result<U256, Vec<u8>> {
        let current_player = self.get_current_player_address(U64::from(game_number));

        if msg::sender() != current_player {
            return Ok(U256::from(ILLEGAL_MOVE));
        }

        let board = self.get_board_from_game_number(U64::from(game_number));

        let from_position = Position::new(from_pos.0.to(), from_pos.0.to());
        let to_position = Position::new(to_pos.0.to(), to_pos.1.to());
        let player_move = Move::Piece(from_position, to_position);
        let move_result = board.play_move(player_move);

        let mut game_info = self.games.setter(U64::from(game_number));

        let response = match move_result {
            GameResult::Continuing(_) => U256::from(CONTINUING),
            GameResult::Victory(_) => {
                let current_color = match board.get_turn_color() {
                    Color::White => U8::from(WHITE),
                    Color::Black => U8::from(BLACK),
                };

                game_info.victor.set(current_color);
                game_info.game_status.set(U8::from(VICTORY));

                U256::from(VICTORY)
            }
            GameResult::Stalemate => {
                game_info.game_status.set(U8::from(STALEMATE));

                U256::from(STALEMATE)
            }
            _ => U256::from(ILLEGAL_MOVE),
        };

        Ok(response)
    }

    // pub fn print_game_state(&self, game_number: U256) -> Result<(), Vec<u8>> {
    //     let game_info = self.games.get(U64::from(game_number));
    //     let board_state = game_info.board_state.get();
    //     let board = self.deserialize_board(board_state);
    //     self.print_board(&board);

    //     Ok(())
    // }

    /// Game info
    pub fn game_by_number(&self, game_number: U256) -> Result<GameInfoReturnType, Vec<u8>> {
        let game_info = self.games.getter(U64::from(game_number));

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
    pub fn game_pieces_by_game_number(&self, game_number: U256) -> Result<U256, Vec<u8>> {
        let game_info = self.games.get(U64::from(game_number));
        Ok(game_info.board_state.get())
    }

    /// Either creates a new game or joins a pending game if it exists
    /// Returns the game number
    pub fn create_or_join(&mut self) -> Result<U256, Vec<u8>> {
        let pending_game = self.pending_game.get();

        // Sets pending_game number and initializes a new board
        if pending_game == U64::from(0) {
            let game_number = self.get_next_game_number();
            self.pending_game.set(game_number);
            self.create_game(game_number);
            return Ok(U256::from(game_number));
        }

        self.join_game(pending_game);

        Ok(U256::from(pending_game))
    }
}

impl StylusChess {
    pub fn get_current_player_address(&self, game_number: U64) -> Address {
        let game_info = self.games.get(U64::from(game_number));
        let current_board = self.get_board_from_game_number(U64::from(game_number));
        let current_player = match current_board.get_turn_color() {
            Color::White => game_info.player_one.get(),
            Color::Black => game_info.player_two.get(),
        };
        current_player
    }

    fn get_board_from_game_number(&self, game_number: U64) -> Board {
        let game_info = self.games.get(game_number);
        let board_state = game_info.board_state.get();
        self.deserialize_board(board_state)
    }

    fn get_next_game_number(&mut self) -> U64 {
        let game_number = self.total_games.get() + U64::from(1);
        self.total_games.set(game_number);
        game_number
    }

    fn create_game(&mut self, game_number: U64) {
        let board = Board::default();
        // Set up pieces for serialization
        let board_state = self.serialize_board(board);

        let mut game_info = self.games.setter(game_number);
        game_info.player_one.set(msg::sender());
        game_info.board_state.set(board_state);
    }

    fn join_game(&mut self, game_number: U64) {
        let mut game_info = self.games.setter(game_number);
        // join as player two
        game_info.player_two.set(msg::sender());
        // change status to continuing
        game_info.game_status.set(U8::from(CONTINUING));
        // empty out pending_game
        self.pending_game.set(U64::from(0));
    }

    fn deserialize_board(&self, board_state: U256) -> Board {
        let mut board_builder: BoardBuilder = BoardBuilder::default();
        board_builder = board_builder.enable_castling();

        for row in 0..8_u8 {
            for col in 0..8_u8 {
                let base_offset: usize = ((row * 8 + col) * 4).into();
                let color_offset: usize = base_offset + 3;
                let piece_type_offset: usize = base_offset;

                let color = (board_state >> color_offset) & U256::from(COLOR_MASK);
                let piece_type = (board_state >> piece_type_offset) & U256::from(PIECE_TYPE_MASK);

                if piece_type != U256::ZERO {
                    let position = Position::new(row.into(), col.into());
                    let color_enum = match U8::from(color).to() {
                        WHITE => Color::White,
                        _ => Color::Black,
                    };
                    let piece = match U8::from(piece_type).to() {
                        KNIGHT => Piece::Knight(color_enum, position),
                        BISHOP => Piece::Bishop(color_enum, position),
                        ROOK => Piece::Rook(color_enum, position),
                        QUEEN => Piece::Queen(color_enum, position),
                        KING => Piece::King(color_enum, position),
                        _ => Piece::Pawn(color_enum, position),
                    };

                    board_builder = board_builder.piece(piece);
                }
            }
        }

        board_builder.build()
    }

    fn serialize_board(&self, board: Board) -> U256 {
        let mut board_state = U256::from(0);

        for row in 0..8_u8 {
            for col in 0..8_u8 {
                let position = Position::new(row.into(), col.into());
                let base_offset: usize = ((row * 8 + col) * 4).into();
                let color_offset: usize = base_offset + 3;
                let piece_type_offset: usize = base_offset;

                if let Some(board_piece) = board.get_piece(position) {
                    let color = if board_piece.get_color() == Color::White {
                        WHITE
                    } else {
                        BLACK
                    };

                    let piece_type = match board_piece {
                        Piece::Pawn(_, _) => PAWN,
                        Piece::Knight(_, _) => KNIGHT,
                        Piece::Bishop(_, _) => BISHOP,
                        Piece::Rook(_, _) => ROOK,
                        Piece::Queen(_, _) => QUEEN,
                        Piece::King(_, _) => KING,
                    };

                    board_state |= U256::from(color) << color_offset;
                    board_state |= U256::from(piece_type) << piece_type_offset;
                } else {
                    board_state |= U256::from(0) << (base_offset + 4);
                }
            }
        }

        board_state
    }

    // fn print_board(&self, board: &Board) {
    //     let turn = board.get_current_player_color();
    //     let abc = if turn == Color::White {
    //         "abcdefgh"
    //     } else {
    //         "hgfedcba"
    //     };

    //     console!("   {}", abc);
    //     console!("  ╔════════╗");
    //     let mut square_color = !turn;
    //     let height = 8;
    //     let width = 8;

    //     for row in 0..height {
    //         let print_row = match turn {
    //             Color::White => height - row - 1,
    //             Color::Black => row,
    //         };
    //         let mut row_text = String::new();
    //         let row_label = format!("{} ║", print_row + 1);
    //         row_text.push_str(row_label.as_str());

    //         for col in 0..width {
    //             let print_col = match turn {
    //                 Color::Black => width - col - 1,
    //                 Color::White => col,
    //             };

    //             let pos = Position::new(print_row, print_col);

    //             let s = if let Some(piece) = board.get_piece(pos) {
    //                 piece.to_string()
    //             } else {
    //                 String::from(match square_color {
    //                     Color::White => "░",
    //                     Color::Black => "▓",
    //                 })
    //             };
    //             if Some(pos) == board.get_en_passant() {
    //                 row_text.push_str(format!("\x1b[34m{}\x1b[m\x1b[0m", s).as_str());
    //             } else if board.is_threatened(pos, turn) {
    //                 row_text.push_str(format!("\x1b[31m{}\x1b[m\x1b[0m", s).as_str());
    //             } else if board.is_threatened(pos, !turn) {
    //                 row_text.push_str(format!("\x1b[32m{}\x1b[m\x1b[0m", s).as_str());
    //             } else {
    //                 row_text.push_str(s.as_str());
    //             }

    //             square_color = !square_color;
    //         }
    //         row_text.push('║');
    //         console!("{}", row_text);

    //         square_color = !square_color;
    //     }

    //     console!("  ╚════════╝");
    //     console!("   {}", abc);
    // }
}

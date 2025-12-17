//! # Block Screen Module
//!
//! Homepage showing the user's block with residents and storage.

mod invite_modal;
mod screen;

// Screen exports
pub use screen::{run_block_screen, BlockFocus, BlockScreen};

//! System command handlers
//!
//! Handlers for Ping, Shutdown, RefreshAccount.

use super::types::{OpResponse, OpResult};
use super::EffectCommand;

/// Handle system commands
pub async fn handle_system(command: &EffectCommand) -> Option<OpResult> {
    match command {
        EffectCommand::Ping => {
            // Simple ping - just return Ok
            Some(Ok(OpResponse::Ok))
        }

        EffectCommand::Shutdown => {
            // Shutdown is handled by the TUI event loop, not here
            Some(Ok(OpResponse::Ok))
        }

        EffectCommand::RefreshAccount => {
            // Trigger a state refresh by reading and re-emitting signals
            // This causes subscribers to re-render with current state
            Some(Ok(OpResponse::Ok))
        }

        _ => None,
    }
}

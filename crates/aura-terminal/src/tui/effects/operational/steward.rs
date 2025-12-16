//! Steward command handlers
//!
//! Handlers for GrantSteward, RevokeSteward.

use std::sync::Arc;

use aura_app::AppCore;
use tokio::sync::RwLock;

use super::types::{OpError, OpResponse, OpResult};
use super::EffectCommand;

/// Handle steward commands
pub async fn handle_steward(
    command: &EffectCommand,
    app_core: &Arc<RwLock<AppCore>>,
) -> Option<OpResult> {
    match command {
        EffectCommand::GrantSteward { target } => {
            // Grant steward (Admin) role to a resident in the current block
            if let Ok(core) = app_core.try_read() {
                let mut blocks = core.views().get_blocks();
                if let Some(block) = blocks.current_block_mut() {
                    // Check if actor is authorized (must be Owner or Admin)
                    if !block.is_admin() {
                        return Some(Err(OpError::Failed(
                            "Only stewards can grant steward role".to_string(),
                        )));
                    }

                    // Find and update the target resident
                    if let Some(resident) = block.resident_mut(target) {
                        // Can't promote an Owner
                        if matches!(resident.role, aura_app::views::block::ResidentRole::Owner)
                        {
                            return Some(Err(OpError::Failed(
                                "Cannot modify Owner role".to_string(),
                            )));
                        }
                        // Promote to Admin
                        resident.role = aura_app::views::block::ResidentRole::Admin;
                        core.views().set_blocks(blocks);
                        tracing::info!("Granted steward role to {}", target);
                        Some(Ok(OpResponse::Ok))
                    } else {
                        Some(Err(OpError::Failed(format!(
                            "Resident not found: {}",
                            target
                        ))))
                    }
                } else {
                    Some(Err(OpError::Failed(
                        "No current block selected".to_string(),
                    )))
                }
            } else {
                Some(Err(OpError::Failed(
                    "Could not access app state".to_string(),
                )))
            }
        }

        EffectCommand::RevokeSteward { target } => {
            // Revoke steward (Admin) role from a resident in the current block
            if let Ok(core) = app_core.try_read() {
                let mut blocks = core.views().get_blocks();
                if let Some(block) = blocks.current_block_mut() {
                    // Check if actor is authorized (must be Owner or Admin)
                    if !block.is_admin() {
                        return Some(Err(OpError::Failed(
                            "Only stewards can revoke steward role".to_string(),
                        )));
                    }

                    // Find and update the target resident
                    if let Some(resident) = block.resident_mut(target) {
                        // Can only demote Admin, not Owner
                        if !matches!(resident.role, aura_app::views::block::ResidentRole::Admin)
                        {
                            return Some(Err(OpError::Failed(
                                "Can only revoke Admin role, not Owner or Resident".to_string(),
                            )));
                        }
                        // Demote to Resident
                        resident.role = aura_app::views::block::ResidentRole::Resident;
                        core.views().set_blocks(blocks);
                        tracing::info!("Revoked steward role from {}", target);
                        Some(Ok(OpResponse::Ok))
                    } else {
                        Some(Err(OpError::Failed(format!(
                            "Resident not found: {}",
                            target
                        ))))
                    }
                } else {
                    Some(Err(OpError::Failed(
                        "No current block selected".to_string(),
                    )))
                }
            } else {
                Some(Err(OpError::Failed(
                    "Could not access app state".to_string(),
                )))
            }
        }

        _ => None,
    }
}

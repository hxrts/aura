//! # Command Dispatch Helper
//!
//! Handles command dispatch through AppCore (intents) and OperationalHandler.

use std::sync::Arc;

use async_lock::RwLock;
use aura_app::AppCore;

use crate::error::TerminalError;
use crate::tui::effects::{
    command_to_intent, CommandContext, EffectCommand, OpResponse, OperationalHandler,
};

/// Helper for dispatching commands
#[derive(Clone)]
pub struct DispatchHelper {
    app_core: Arc<RwLock<AppCore>>,
    operational: Arc<OperationalHandler>,
}

impl DispatchHelper {
    /// Create a new dispatch helper
    pub fn new(app_core: Arc<RwLock<AppCore>>) -> Self {
        let operational = Arc::new(OperationalHandler::new(app_core.clone()));
        Self {
            app_core,
            operational,
        }
    }

    /// Get reference to operational handler
    pub fn operational(&self) -> &Arc<OperationalHandler> {
        &self.operational
    }

    /// Dispatch a command through the appropriate handler (Intent or Operational).
    ///
    /// This is the main entry point for command execution:
    /// 1. Try to map command to Intent → dispatch through AppCore (journaled)
    /// 2. Otherwise dispatch through OperationalHandler (non-journaled)
    ///
    /// Returns OpResponse for operational commands, or Ok(None) for intent commands.
    pub async fn dispatch_command(
        &self,
        command: &EffectCommand,
        cmd_ctx: &CommandContext,
    ) -> Result<Option<OpResponse>, TerminalError> {
        // Try to map command to intent for unified dispatch
        if let Some(intent) = command_to_intent(command, cmd_ctx) {
            // Dispatch through AppCore (journaled operation)
            let mut core = self.app_core.write().await;
            match core.dispatch(intent) {
                Ok(_fact_id) => {
                    // Commit pending facts and emit to reactive signals
                    if let Err(e) = core.commit_pending_facts_and_emit().await {
                        tracing::warn!("Failed to commit facts or emit signals: {}", e);
                    }
                    Ok(None)
                }
                Err(e) => {
                    let msg = format!("Intent dispatch failed: {}", e);
                    self.operational
                        .emit_error(TerminalError::Operation(msg.clone()))
                        .await;
                    Err(TerminalError::Operation(msg))
                }
            }
        } else if let Some(result) = self.operational.execute(command).await {
            // Handle operational command
            match result {
                Ok(response) => Ok(Some(response)),
                Err(e) => {
                    let terr: TerminalError = e.into();
                    self.operational.emit_error(terr.clone()).await;
                    Err(terr)
                }
            }
        } else {
            // Unknown command
            tracing::warn!(
                "Unknown command not handled by Intent or Operational: {:?}",
                command
            );
            let msg = format!("Unknown command: {:?}", command);
            self.operational
                .emit_error(TerminalError::Operation(msg.clone()))
                .await;
            Err(TerminalError::Operation(msg))
        }
    }

    /// Dispatch a command and convert result to Result<(), String>
    ///
    /// This is a convenience wrapper that matches the legacy dispatch_and_wait signature.
    pub async fn dispatch_and_wait(
        &self,
        command: &EffectCommand,
        cmd_ctx: &CommandContext,
    ) -> Result<(), String> {
        self.dispatch_command(command, cmd_ctx)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

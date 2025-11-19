//! Ledger Effects
//!
//! Placeholder module for ledger effects that were previously in aura-protocol.
//! These need to be refactored to use the new authority-centric architecture.

use aura_core::{AuraError, AuraResult};
use async_trait::async_trait;

/// Ledger error types
#[derive(Debug, Clone, thiserror::Error)]
pub enum LedgerError {
    #[error("Not implemented: {0}")]
    NotImplemented(String),
    #[error("Ledger error: {0}")]
    Other(String),
}

impl From<LedgerError> for AuraError {
    fn from(err: LedgerError) -> Self {
        AuraError::internal(err.to_string())
    }
}

/// Ledger effects trait (stub)
#[async_trait]
pub trait LedgerEffects: Send + Sync {
    /// Get current ledger state
    async fn get_state(&self) -> Result<Vec<u8>, LedgerError> {
        Err(LedgerError::NotImplemented("get_state".to_string()))
    }
}

//! Effect API Effects
//!
//! Placeholder module for effect API effects that were previously in aura-protocol.
//! These need to be refactored to use the new authority-centric architecture.

use async_trait::async_trait;
use aura_core::{AuraError, AuraResult};

/// Effect API error types
#[derive(Debug, Clone, thiserror::Error)]
pub enum EffectApiError {
    #[error("Not implemented: {0}")]
    NotImplemented(String),
    #[error("Effect API error: {0}")]
    Other(String),
}

impl From<EffectApiError> for AuraError {
    fn from(err: EffectApiError) -> Self {
        AuraError::internal(err.to_string())
    }
}

/// Effect API effects trait (stub)
#[async_trait]
pub trait EffectApiEffects: Send + Sync {
    /// Get current effect API state
    async fn get_state(&self) -> Result<Vec<u8>, EffectApiError> {
        Err(EffectApiError::NotImplemented("get_state".to_string()))
    }
}

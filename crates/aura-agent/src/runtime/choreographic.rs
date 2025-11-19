//! Choreographic Effects
//!
//! Placeholder module for choreographic effects integration.
//! This integrates with rumpsteak choreography runtime.

use aura_core::{AuraError, AuraResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Choreography error
#[derive(Debug, Clone, thiserror::Error)]
pub enum ChoreographyError {
    #[error("Choreography not implemented: {0}")]
    NotImplemented(String),
    #[error("Choreography execution failed: {0}")]
    ExecutionFailed(String),
}

impl From<ChoreographyError> for AuraError {
    fn from(err: ChoreographyError) -> Self {
        AuraError::internal(err.to_string())
    }
}

/// Role in a choreography
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChoreographicRole {
    Initiator,
    Responder,
    Observer,
}

/// Choreographic effects trait (stub)
#[async_trait]
pub trait ChoreographicEffects: Send + Sync {
    /// Execute a choreography
    async fn execute_choreography(&self, _name: &str) -> AuraResult<()> {
        Err(AuraError::internal("Choreographic effects not implemented"))
    }
}

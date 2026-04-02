//! Runtime capability admission effects.
//!
//! This trait exposes runtime capability inventory and admission checks used by
//! theorem-pack style protocol gating.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Stable key for a runtime capability contract.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub struct CapabilityKey(String);

impl CapabilityKey {
    /// Create a capability key from a stable identifier.
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    /// Borrow the underlying key string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CapabilityKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for CapabilityKey {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for CapabilityKey {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

/// Admission errors for theorem-pack/runtime capability checks.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
pub enum AdmissionError {
    /// A required runtime capability is missing or disabled.
    #[error("missing runtime capability: {capability}")]
    MissingCapability { capability: CapabilityKey },
    /// A required theorem pack is not declared or not admitted by Aura.
    #[error("missing required theorem pack: {theorem_pack}")]
    MissingTheoremPack { theorem_pack: String },
    /// One required theorem-pack capability is missing or disabled.
    #[error("missing theorem-pack capability `{capability}` for `{theorem_pack}`")]
    MissingTheoremPackCapability {
        theorem_pack: String,
        capability: CapabilityKey,
    },
    /// Capability inventory could not be loaded.
    #[error("runtime capability inventory unavailable: {reason}")]
    InventoryUnavailable { reason: String },
    /// Runtime contracts were required but unavailable.
    #[error("missing runtime contracts for capability admission")]
    MissingRuntimeContracts,
    /// Internal admission failure.
    #[error("runtime capability admission internal error: {reason}")]
    Internal { reason: String },
}

/// Runtime capability query/admission interface.
#[async_trait]
pub trait RuntimeCapabilityEffects: Send + Sync {
    /// Fetch the currently admitted runtime capability inventory.
    async fn capability_inventory(&self) -> Result<Vec<(CapabilityKey, bool)>, AdmissionError>;

    /// Require all listed capabilities to be present and enabled.
    async fn require_capabilities(&self, required: &[CapabilityKey]) -> Result<(), AdmissionError> {
        let inventory = self.capability_inventory().await?;
        for required_key in required {
            let admitted = inventory
                .iter()
                .find(|(present_key, _)| present_key == required_key)
                .is_some_and(|(_, admitted)| *admitted);
            if !admitted {
                return Err(AdmissionError::MissingCapability {
                    capability: required_key.clone(),
                });
            }
        }
        Ok(())
    }
}

#[async_trait]
impl<T: RuntimeCapabilityEffects + ?Sized> RuntimeCapabilityEffects for std::sync::Arc<T> {
    async fn capability_inventory(&self) -> Result<Vec<(CapabilityKey, bool)>, AdmissionError> {
        (**self).capability_inventory().await
    }

    async fn require_capabilities(&self, required: &[CapabilityKey]) -> Result<(), AdmissionError> {
        (**self).require_capabilities(required).await
    }
}

//! Agent-Specific Effect Trait Definitions
//!
//! This module defines effect traits specific to device-side agent operations.
//! These effects compose core system effects (from aura-protocol) into
//! higher-level device workflows and capabilities.

pub mod device;

// Re-export all agent-specific effect traits
pub use device::{
    AgentEffects, AuthMethod, AuthenticationEffects, AuthenticationResult, BiometricType,
    ConfigurationEffects, DeviceInfo, DeviceStorageEffects, HealthStatus, SessionManagementEffects,
};

// Re-export core effect traits from aura-protocol for convenience
pub use aura_protocol::effects::{
    ChoreographicEffects, ConsoleEffects, CryptoEffects, JournalEffects, LedgerEffects,
    NetworkEffects, RandomEffects, StorageEffects, TimeEffects,
};

// Re-export unified effect system from the effects module
pub use aura_protocol::effects::AuraEffectSystem;

//! Agent-Specific Effect Trait Definitions
//!
//! This module defines effect traits specific to device-side agent operations.
//! These effects compose core system effects (from aura-protocol) into 
//! higher-level device workflows and capabilities.

pub mod device;

// Re-export all agent-specific effect traits
pub use device::{
    AgentEffects, DeviceStorageEffects, AuthenticationEffects, 
    SessionManagementEffects, ConfigurationEffects, DeviceInfo,
    AuthenticationResult, AuthMethod, BiometricType, HealthStatus
};

// Re-export core effect traits from aura-protocol for convenience
pub use aura_protocol::effects::{
    CryptoEffects, StorageEffects, NetworkEffects, TimeEffects, 
    ConsoleEffects, RandomEffects, LedgerEffects, JournalEffects, 
    ChoreographicEffects
};

// Re-export unified effect system from the effects module
pub use aura_protocol::effects::AuraEffectSystem;
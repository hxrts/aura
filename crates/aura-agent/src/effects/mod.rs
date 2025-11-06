//! Agent-Specific Effect Trait Definitions
//!
//! This module defines effect traits specific to device-side agent operations.
//! These effects compose core system effects (from aura-protocol) into 
//! higher-level device workflows and capabilities.

pub mod device;

// Re-export all agent-specific effect traits
pub use device::{
    AgentEffects, DeviceStorageEffects, AuthenticationEffects, 
    SessionManagementEffects, ConfigurationEffects, DeviceInfo
};

// TODO: Re-enable when aura-protocol compiles
// Re-export core effect traits from aura-protocol for convenience
// pub use aura_protocol::effects::{
//     AuraEffectSystem, Effects, CryptoEffects, StorageEffects, 
//     NetworkEffects, TimeEffects, ConsoleEffects, RandomEffects,
//     LedgerEffects, JournalEffects, ChoreographicEffects
// };
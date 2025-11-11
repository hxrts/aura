//! ID generation effects - TEMPORARY COMPATIBILITY BRIDGE
//!
//! ⚠️  WARNING: THIS IS A TEMPORARY ARCHITECTURAL COMPATIBILITY LAYER ⚠️
//!
//! This module provides the EffectsLike trait and extension traits that were previously
//! in aura-core but violated the interface layer separation. This is a TEMPORARY bridge
//! to prevent immediate build breakage while we fix the proper architectural violations.
//!
//! # ARCHITECTURAL VIOLATIONS TO FIX:
//!
//! ## Problem: Domain crates using Effects-based ID generation
//! Domain crates (aura-crypto, aura-transport, aura-verify, etc.) should NOT depend on
//! effect handlers for basic functionality like ID generation.
//!
//! ## Correct Architecture:
//! 1. **For Tests**: Use `Uuid::new_v4()` or hardcoded test values
//! 2. **For Production**: Accept IDs as parameters from higher layers
//! 3. **For Deterministic Testing**: Use aura-testkit utilities
//!
//! ## Files to Fix:
//! - `aura-crypto/src/middleware/key_rotation.rs` - replace DeviceIdExt with Uuid::new_v4()
//! - `aura-crypto/src/middleware/secure_random_complex.rs` - same fix
//! - `aura-testkit/src/` - move ID generation utilities here (proper layer)
//! - `aura-transport/src/privacy/manifest_manager.rs` - replace with Uuid::new_v4()
//! - `aura-verify/src/` - replace with Uuid::new_v4() in tests
//!
//! ## Correct Pattern Example:
//! ```rust
//! // WRONG (domain crate depending on effects):
//! let device_id = DeviceId::new_with_effects(&effects);
//!
//! // CORRECT (accept from higher layer):
//! fn create_operation(device_id: DeviceId) -> Operation { ... }
//!
//! // CORRECT (in tests):
//! let device_id = DeviceId(Uuid::new_v4());
//! ```
//!
//! # TODO: Remove this entire module after fixing the violations listed above

use aura_core::{AccountId, DeviceId, EventId, GuardianId};
use async_trait::async_trait;
use uuid::Uuid;

/// Trait for Effects-like objects that support UUID generation
/// Used to abstract over different Effects implementations for ID generation
pub trait EffectsLike {
    /// Generate a deterministic UUID
    fn gen_uuid(&self) -> Uuid;
}

/// Extension trait for DeviceId with Effects support
pub trait DeviceIdExt {
    /// Create a new device ID using Effects for deterministic randomness
    fn new_with_effects(effects: &impl EffectsLike) -> Self;
    /// Create from a string identifier using Effects
    fn from_string_with_effects(id_str: &str, effects: &impl EffectsLike) -> Self;
}

/// Extension trait for GuardianId with Effects support
pub trait GuardianIdExt {
    /// Create a new guardian ID using Effects for deterministic randomness
    fn new_with_effects(effects: &impl EffectsLike) -> Self;
    /// Create from a string identifier using Effects
    fn from_string_with_effects(id_str: &str, effects: &impl EffectsLike) -> Self;
}

/// Extension trait for AccountId with Effects support
pub trait AccountIdExt {
    /// Create a new account ID using Effects for deterministic randomness
    fn new_with_effects(effects: &impl EffectsLike) -> Self;
    /// Create from a string identifier using Effects
    fn from_string_with_effects(id_str: &str, effects: &impl EffectsLike) -> Self;
}

/// Extension trait for EventId with Effects support
pub trait EventIdExt {
    /// Create a new event ID using Effects for deterministic randomness
    fn new_with_effects(effects: &impl EffectsLike) -> Self;
}

impl DeviceIdExt for DeviceId {
    fn new_with_effects(effects: &impl EffectsLike) -> Self {
        DeviceId(effects.gen_uuid())
    }

    fn from_string_with_effects(id_str: &str, _effects: &impl EffectsLike) -> Self {
        // Create a deterministic UUID from the string
        let namespace = Uuid::NAMESPACE_DNS;
        DeviceId(Uuid::new_v5(&namespace, id_str.as_bytes()))
    }
}

impl GuardianIdExt for GuardianId {
    fn new_with_effects(effects: &impl EffectsLike) -> Self {
        GuardianId(effects.gen_uuid())
    }

    fn from_string_with_effects(id_str: &str, _effects: &impl EffectsLike) -> Self {
        // Create a deterministic UUID from the string
        let namespace = Uuid::NAMESPACE_DNS;
        GuardianId(Uuid::new_v5(&namespace, id_str.as_bytes()))
    }
}

impl AccountIdExt for AccountId {
    fn new_with_effects(effects: &impl EffectsLike) -> Self {
        AccountId(effects.gen_uuid())
    }

    fn from_string_with_effects(id_str: &str, _effects: &impl EffectsLike) -> Self {
        // Create a deterministic UUID from the string
        let namespace = Uuid::NAMESPACE_DNS;
        AccountId(Uuid::new_v5(&namespace, id_str.as_bytes()))
    }
}

impl EventIdExt for EventId {
    fn new_with_effects(effects: &impl EffectsLike) -> Self {
        EventId(effects.gen_uuid())
    }
}

// Implement EffectsLike for any effect system that has RandomEffects
impl<T> EffectsLike for T
where
    T: aura_core::effects::RandomEffects + Send + Sync,
{
    fn gen_uuid(&self) -> Uuid {
        // Note: This is a sync method but RandomEffects is async
        // In practice, this should be implemented by specific effect handlers
        // For now, fall back to random generation
        Uuid::new_v4()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestEffects {
        seed: u64,
    }

    impl EffectsLike for TestEffects {
        fn gen_uuid(&self) -> Uuid {
            // Generate deterministic UUID based on seed
            let mut bytes = [0u8; 16];
            let seed_bytes = self.seed.to_le_bytes();
            for (i, byte) in bytes.iter_mut().enumerate() {
                *byte = seed_bytes[i % 8];
            }
            Uuid::from_bytes(bytes)
        }
    }

    #[test]
    fn test_device_id_with_effects() {
        let effects = TestEffects { seed: 42 };
        let id1 = DeviceId::new_with_effects(&effects);
        let id2 = DeviceId::new_with_effects(&effects);
        
        // Should be deterministic
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_device_id_from_string() {
        let effects = TestEffects { seed: 42 };
        let id1 = DeviceId::from_string_with_effects("test", &effects);
        let id2 = DeviceId::from_string_with_effects("test", &effects);
        
        // Should be deterministic based on string
        assert_eq!(id1, id2);
    }
}
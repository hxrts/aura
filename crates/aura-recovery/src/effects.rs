//! Effect composition for recovery operations.
//!
//! This module defines composed effect traits that bound only the capabilities
//! actually needed by recovery operations, enabling focused testing with minimal mocks.
//!
//! # Architecture
//!
//! Recovery operations require a subset of the full `AuraEffects` trait:
//!
//! - **PhysicalTimeEffects**: Timestamps, cooldowns, expiration checks
//! - **RandomEffects**: Nonce generation, ceremony IDs
//! - **JournalEffects**: Fact emission and journal operations
//! - **CryptoEffects**: Signature verification, key operations
//!
//! For network-involved operations (multi-party coordination), we additionally need:
//!
//! - **NetworkEffects**: Message transport between guardians
//!
//! # Usage
//!
//! ```ignore
//! use aura_recovery::effects::RecoveryEffects;
//!
//! // Use minimal bounds for coordinators that don't need network
//! async fn local_operation<E: RecoveryEffects>(effects: &E) {
//!     let now = effects.now_physical().await;
//!     // ...
//! }
//!
//! // Use extended bounds for network operations
//! async fn network_operation<E: RecoveryNetworkEffects>(effects: &E) {
//!     effects.send_message(...).await;
//!     // ...
//! }
//! ```
//!
//! # Migration Path
//!
//! Coordinators can be gradually migrated from `AuraEffects` to these composed traits:
//!
//! 1. Start with `AuraEffects` (current state)
//! 2. Replace with `RecoveryEffects` for local-only operations
//! 3. Replace with `RecoveryNetworkEffects` for distributed operations
//!
//! Since `AuraEffects` is a supertrait of both, existing code continues to work.

use aura_core::effects::{
    CryptoEffects, JournalEffects, NetworkEffects, PhysicalTimeEffects, RandomEffects,
};

/// Composed effects required for local recovery operations.
///
/// This trait bounds only the effects actually needed by recovery coordinators
/// that don't involve network communication, enabling focused testing with
/// minimal mocks.
///
/// # Included Effects
///
/// - **PhysicalTimeEffects**: For timestamps, cooldowns, and expiration
/// - **CryptoEffects**: For signature verification and key operations
/// - **JournalEffects**: For fact emission and journal operations
/// - **RandomEffects**: For nonce generation and ceremony IDs
///
/// # Example
///
/// ```ignore
/// use aura_recovery::effects::RecoveryEffects;
///
/// struct MyCoordinator<E: RecoveryEffects> {
///     effects: Arc<E>,
/// }
/// ```
pub trait RecoveryEffects:
    PhysicalTimeEffects + CryptoEffects + JournalEffects + RandomEffects + Send + Sync
{
}

/// Blanket implementation for any type that implements all required traits.
impl<T> RecoveryEffects for T where
    T: PhysicalTimeEffects + CryptoEffects + JournalEffects + RandomEffects + Send + Sync
{
}

/// Extended effects for network-involved recovery operations.
///
/// This trait adds `NetworkEffects` to `RecoveryEffects` for operations
/// that require communication between multiple parties (e.g., guardian
/// coordination, distributed key recovery).
///
/// # Included Effects
///
/// All of `RecoveryEffects` plus:
/// - **NetworkEffects**: For message transport between guardians
///
/// # Example
///
/// ```ignore
/// use aura_recovery::effects::RecoveryNetworkEffects;
///
/// struct DistributedCoordinator<E: RecoveryNetworkEffects> {
///     effects: Arc<E>,
/// }
/// ```
pub trait RecoveryNetworkEffects: RecoveryEffects + NetworkEffects {}

/// Blanket implementation for any type that implements all required traits.
impl<T> RecoveryNetworkEffects for T where T: RecoveryEffects + NetworkEffects {}

#[cfg(test)]
mod tests {
    use super::*;

    // Verify that AuraEffects implements RecoveryEffects
    // This test ensures backward compatibility
    fn _verify_aura_effects_compatibility<T: aura_protocol::effects::AuraEffects>() {
        fn requires_recovery_effects<E: RecoveryEffects>() {}
        requires_recovery_effects::<T>();
    }

    // Verify that AuraEffects implements RecoveryNetworkEffects
    fn _verify_aura_effects_network_compatibility<T: aura_protocol::effects::AuraEffects>() {
        fn requires_recovery_network_effects<E: RecoveryNetworkEffects>() {}
        requires_recovery_network_effects::<T>();
    }
}

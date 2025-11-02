//! Protocol Effects Module
//!
//! This module contains all side-effect operations used by the protocol layer.
//! By centralizing effects here, we maintain clear separation between pure protocol
//! logic and effectful operations, enabling deterministic testing and clean architecture.
//!
//! ## Architecture Principles
//!
//! 1. **Effect Isolation**: All side effects are contained within this module
//! 2. **Algebraic Effects**: Effects are designed to work with the aura_crypto::Effects system
//! 3. **Testability**: All effects can be mocked/injected for deterministic testing
//! 4. **Pure Core**: Protocol logic remains pure and accepts effects as parameters
//!
//! ## Effect Categories
//!
//! - **Cryptographic Effects** (`signing.rs`): Event signing, verification, key operations
//! - **Time Effects** (`time.rs`): Scheduling, timeouts, cooperative yielding
//! - **Error Effects**: Unified error handling for protocol operations
//! - **Future**: Network, storage, random effects as needed
//!
//! ## Usage Pattern
//!
//! ```rust,ignore
//! use crate::effects::{ProtocolEffects, SigningEffects, TimeEffects};
//!
//! // Pure protocol function that accepts effects
//! fn execute_protocol_phase(
//!     state: ProtocolState,
//!     effects: &impl ProtocolEffects,
//! ) -> Result<ProtocolState, ProtocolError> {
//!     // Use effects for side-effect operations
//!     let signature = effects.sign_event(&event)?;
//!     let current_time = effects.current_epoch();
//!
//!     // Pure logic using the effect results
//!     Ok(state.with_signature(signature).at_time(current_time))
//! }
//! ```

pub mod console;
pub mod signing;
pub mod time;

// Re-export core effect types
pub use console::{
    ChannelConsoleEffects, ConsoleEffect, ConsoleEffects, NoOpConsoleEffects,
    RecordingConsoleEffects,
};
pub use signing::EventSigner;
pub use time::{
    ProductionTimeSource, SimulatedTimeSource, TimeSource as ProtocolTimeSource, WakeCondition,
};

// ========== Error Effects ==========

/// Re-export unified error system for consistent error handling across protocols
///
/// These error types are designed to work seamlessly with the effects system,
/// allowing protocol functions to return consistent error types that can be
/// handled uniformly by middleware and higher-level systems.
pub use aura_types::{AuraError, AuraResult, ErrorCode, ErrorSeverity};

use aura_journal::{Event, LedgerError};
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use uuid::Uuid;

/// Unified protocol effects interface
///
/// This trait combines all the effect categories needed by protocol operations.
/// It can be implemented by combining the individual effect traits or by
/// creating a unified effects provider that integrates with aura_crypto::Effects.
pub trait ProtocolEffects: SigningEffects + TimeEffects + ConsoleEffects + Send + Sync {
    /// Get the device ID for this effects context
    fn device_id(&self) -> Uuid;

    /// Check if we're running in a simulated environment
    fn is_simulation(&self) -> bool;
}

/// Cryptographic effects interface
///
/// Provides cryptographic operations needed by protocol logic.
/// This should eventually integrate with aura_crypto::Effects.
pub trait SigningEffects {
    /// Sign an event with the provided signing key
    fn sign_event(&self, event: &Event, key: &SigningKey) -> Result<Signature, LedgerError>;

    /// Verify an event signature
    fn verify_signature(
        &self,
        event: &Event,
        signature: &Signature,
        public_key: &VerifyingKey,
    ) -> bool;

    /// Get the public key for a signing key
    fn get_public_key(&self, signing_key: &SigningKey) -> VerifyingKey;
}

/// Time and scheduling effects interface
///
/// Provides time-related operations for protocol coordination.
/// This should eventually integrate with aura_crypto::Effects.time.
pub trait TimeEffects {
    /// Get the current epoch/timestamp
    fn current_epoch(&self) -> u64;

    /// Yield execution until a condition is met
    fn yield_until(
        &self,
        condition: WakeCondition,
    ) -> impl std::future::Future<Output = Result<(), crate::types::ProtocolError>> + Send;

    /// Register a protocol context for wake notifications
    fn register_context(&self, context_id: Uuid);

    /// Unregister a protocol context
    fn unregister_context(&self, context_id: Uuid);

    /// Notify that new events are available
    fn notify_events_available(&self) -> impl std::future::Future<Output = ()> + Send;
}

/// Default implementation that combines individual effect providers
pub struct CombinedEffects<S, T, C> {
    signing: S,
    time: T,
    console: C,
    device_id: Uuid,
    is_simulation: bool,
}

impl<S, T, C> CombinedEffects<S, T, C>
where
    S: SigningEffects,
    T: TimeEffects,
    C: ConsoleEffects,
{
    /// Create a new combined effects provider
    pub fn new(signing: S, time: T, console: C, device_id: Uuid, is_simulation: bool) -> Self {
        Self {
            signing,
            time,
            console,
            device_id,
            is_simulation,
        }
    }
}

impl<S, T, C> ProtocolEffects for CombinedEffects<S, T, C>
where
    S: SigningEffects + Send + Sync,
    T: TimeEffects + Send + Sync,
    C: ConsoleEffects + Send + Sync,
{
    fn device_id(&self) -> Uuid {
        self.device_id
    }

    fn is_simulation(&self) -> bool {
        self.is_simulation
    }
}

impl<S, T, C> SigningEffects for CombinedEffects<S, T, C>
where
    S: SigningEffects,
    T: TimeEffects,
    C: ConsoleEffects,
{
    fn sign_event(&self, event: &Event, key: &SigningKey) -> Result<Signature, LedgerError> {
        self.signing.sign_event(event, key)
    }

    fn verify_signature(
        &self,
        event: &Event,
        signature: &Signature,
        public_key: &VerifyingKey,
    ) -> bool {
        self.signing.verify_signature(event, signature, public_key)
    }

    fn get_public_key(&self, signing_key: &SigningKey) -> VerifyingKey {
        self.signing.get_public_key(signing_key)
    }
}

impl<S, T, C> TimeEffects for CombinedEffects<S, T, C>
where
    S: SigningEffects + Sync,
    T: TimeEffects + Sync,
    C: ConsoleEffects + Sync,
{
    fn current_epoch(&self) -> u64 {
        self.time.current_epoch()
    }

    async fn yield_until(
        &self,
        condition: WakeCondition,
    ) -> Result<(), crate::types::ProtocolError> {
        self.time.yield_until(condition).await
    }

    fn register_context(&self, context_id: Uuid) {
        self.time.register_context(context_id)
    }

    fn unregister_context(&self, context_id: Uuid) {
        self.time.unregister_context(context_id)
    }

    async fn notify_events_available(&self) {
        self.time.notify_events_available().await
    }
}

#[async_trait::async_trait]
impl<S, T, C> ConsoleEffects for CombinedEffects<S, T, C>
where
    S: SigningEffects + Send + Sync,
    T: TimeEffects + Send + Sync,
    C: ConsoleEffects + Send + Sync,
{
    async fn emit_choreo_event(&self, event: crate::protocols::choreographic::ChoreoEvent) {
        self.console.emit_choreo_event(event).await
    }

    async fn protocol_started(&self, protocol_id: Uuid, protocol_type: &str) {
        self.console
            .protocol_started(protocol_id, protocol_type)
            .await
    }

    async fn protocol_completed(&self, protocol_id: Uuid, success: bool) {
        self.console.protocol_completed(protocol_id, success).await
    }

    async fn emit_marker(&self, marker_type: &str, data: serde_json::Value) {
        self.console.emit_marker(marker_type, data).await
    }

    async fn flush(&self) {
        self.console.flush().await
    }
}

/// Adapter that bridges aura_crypto::Effects to our protocol effects interface
///
/// This adapts the aura_crypto::Effects system to work with our protocol-specific
/// effects interfaces, enabling unified effects injection across the entire system.
pub struct AuraEffectsAdapter {
    effects: aura_crypto::Effects,
    device_id: Uuid,
    device_key: Option<[u8; 32]>, // Store key bytes instead of SigningKey
}

impl AuraEffectsAdapter {
    /// Create a new adapter that bridges to aura_crypto::Effects
    pub fn new(effects: aura_crypto::Effects, device_id: Uuid) -> Self {
        Self {
            effects,
            device_id,
            device_key: None,
        }
    }

    /// Create with a specific device signing key
    pub fn with_device_key(
        effects: aura_crypto::Effects,
        device_id: Uuid,
        device_key: SigningKey,
    ) -> Self {
        Self {
            effects,
            device_id,
            device_key: Some(device_key.to_bytes()),
        }
    }

    /// Get or generate a device signing key using the effects randomness
    fn get_device_key(&self) -> SigningKey {
        if let Some(key_bytes) = self.device_key {
            SigningKey::from_bytes(&key_bytes)
        } else {
            // Generate deterministic key based on device_id
            self.effects.generate_signing_key()
        }
    }
}

impl ProtocolEffects for AuraEffectsAdapter {
    fn device_id(&self) -> Uuid {
        self.device_id
    }

    fn is_simulation(&self) -> bool {
        self.effects.is_simulated()
    }
}

impl SigningEffects for AuraEffectsAdapter {
    fn sign_event(&self, event: &Event, key: &SigningKey) -> Result<Signature, LedgerError> {
        let event_hash = event.hash()?;
        Ok(self.effects.sign_data(&event_hash, key))
    }

    fn verify_signature(
        &self,
        event: &Event,
        signature: &Signature,
        public_key: &VerifyingKey,
    ) -> bool {
        if let Ok(event_hash) = event.hash() {
            self.effects
                .verify_signature(&event_hash, signature, public_key)
        } else {
            false
        }
    }

    fn get_public_key(&self, signing_key: &SigningKey) -> VerifyingKey {
        signing_key.verifying_key()
    }
}

impl TimeEffects for AuraEffectsAdapter {
    fn current_epoch(&self) -> u64 {
        self.effects.now().unwrap_or(0)
    }

    async fn yield_until(
        &self,
        condition: WakeCondition,
    ) -> Result<(), crate::types::ProtocolError> {
        match condition {
            WakeCondition::NewEvents => {
                // For basic implementation, just yield control
                tokio::task::yield_now().await;
                Ok(())
            }
            WakeCondition::EpochReached(target) => {
                let current = self.current_epoch();
                if target > current {
                    // Use aura_crypto::Effects delay mechanism
                    let duration = std::time::Duration::from_secs(target - current);
                    self.effects.delay(duration).await;
                }
                Ok(())
            }
            WakeCondition::TimeoutAt(target) => {
                let current = self.current_epoch();
                if target > current {
                    let duration = std::time::Duration::from_secs(target - current);
                    self.effects.delay(duration).await;
                }
                Ok(())
            }
            WakeCondition::EventMatching(_) | WakeCondition::ThresholdEvents { .. } => {
                // For basic implementation, just yield control
                tokio::task::yield_now().await;
                Ok(())
            }
        }
    }

    fn register_context(&self, _context_id: Uuid) {
        // No-op for now - would integrate with simulation scheduler in future
    }

    fn unregister_context(&self, _context_id: Uuid) {
        // No-op for now
    }

    async fn notify_events_available(&self) {
        // No-op for now - would notify waiting contexts in future
    }
}

#[async_trait::async_trait]
impl ConsoleEffects for AuraEffectsAdapter {
    async fn emit_choreo_event(&self, _event: crate::protocols::choreographic::ChoreoEvent) {
        // No-op by default - would need to be configured with a console sink
    }

    async fn protocol_started(&self, _protocol_id: Uuid, _protocol_type: &str) {
        // No-op by default
    }

    async fn protocol_completed(&self, _protocol_id: Uuid, _success: bool) {
        // No-op by default
    }

    async fn emit_marker(&self, _marker_type: &str, _data: serde_json::Value) {
        // No-op by default
    }

    async fn flush(&self) {
        // No-op by default
    }
}

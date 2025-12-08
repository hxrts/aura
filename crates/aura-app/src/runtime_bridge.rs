//! # RuntimeBridge: Abstract Runtime Operations
//!
//! This module defines the `RuntimeBridge` trait, which abstracts runtime operations
//! that require system resources (networking, storage, cryptography). This enables
//! `aura-app` to remain a pure application core without direct dependencies on
//! runtime infrastructure.
//!
//! ## Design
//!
//! ```text
//! aura-app (pure)          aura-agent (runtime)
//! ┌─────────────────┐      ┌─────────────────┐
//! │ AppCore         │      │ AuraAgent       │
//! │   ┌───────────┐ │      │   implements    │
//! │   │RuntimeBridge│◄─────│   RuntimeBridge │
//! │   └───────────┘ │      │                 │
//! └─────────────────┘      └─────────────────┘
//! ```
//!
//! ## Usage
//!
//! ```rust,ignore
//! // In aura-terminal (or other frontend)
//! let agent = AgentBuilder::new()
//!     .with_authority(authority_id)
//!     .build_production()
//!     .await?;
//!
//! // Create app with runtime bridge
//! let app = AppCore::with_runtime(config, agent.as_runtime_bridge())?;
//!
//! // Or for offline/demo mode
//! let app = AppCore::new(config)?; // No runtime bridge
//! ```

use crate::core::IntentError;
use async_trait::async_trait;
use aura_core::identifiers::AuthorityId;
use aura_core::threshold::{SigningContext, ThresholdConfig, ThresholdSignature};
use aura_core::tree::{AttestedOp, TreeOp};
use aura_core::DeviceId;
use aura_journal::JournalFact;
use std::sync::Arc;

/// Status of the runtime's sync service
#[derive(Debug, Clone, Default)]
pub struct SyncStatus {
    /// Whether the sync service is currently running
    pub is_running: bool,
    /// Number of connected peers
    pub connected_peers: usize,
    /// Last sync timestamp (milliseconds since epoch)
    pub last_sync_ms: Option<u64>,
    /// Pending facts waiting to be synced
    pub pending_facts: usize,
}

/// Status of the runtime's rendezvous service
#[derive(Debug, Clone, Default)]
pub struct RendezvousStatus {
    /// Whether the rendezvous service is running
    pub is_running: bool,
    /// Number of cached peers
    pub cached_peers: usize,
}

/// Overall runtime status
#[derive(Debug, Clone, Default)]
pub struct RuntimeStatus {
    /// Sync service status
    pub sync: SyncStatus,
    /// Rendezvous service status
    pub rendezvous: RendezvousStatus,
    /// Whether the runtime is authenticated
    pub is_authenticated: bool,
}

/// Bridge trait for runtime operations
///
/// This trait defines the interface between the pure application core (`aura-app`)
/// and the runtime infrastructure (`aura-agent`). It enables:
///
/// - **Decoupling**: App core doesn't know about agent internals
/// - **Testability**: Mock implementations for unit tests
/// - **Portability**: Different runtimes for different platforms
///
/// ## Implementation
///
/// The primary implementation is in `aura-agent`, where `AuraAgent` implements
/// this trait. For testing, mock implementations can be provided.
#[async_trait]
pub trait RuntimeBridge: Send + Sync {
    // =========================================================================
    // Identity & Authority
    // =========================================================================

    /// Get the authority ID for this runtime
    fn authority_id(&self) -> AuthorityId;

    // =========================================================================
    // Fact Persistence
    // =========================================================================

    /// Persist facts to durable storage
    ///
    /// This commits facts to the journal and triggers any necessary
    /// synchronization with peers.
    async fn persist_facts(&self, facts: &[JournalFact]) -> Result<(), IntentError>;

    // =========================================================================
    // Sync Operations
    // =========================================================================

    /// Get current sync status
    async fn get_sync_status(&self) -> SyncStatus;

    /// Get list of known sync peers
    async fn get_sync_peers(&self) -> Vec<DeviceId>;

    /// Trigger sync with peers (if sync service is available)
    async fn trigger_sync(&self) -> Result<(), IntentError>;

    // =========================================================================
    // Peer Discovery
    // =========================================================================

    /// Get list of discovered peers from rendezvous
    async fn get_discovered_peers(&self) -> Vec<AuthorityId>;

    /// Get rendezvous status
    async fn get_rendezvous_status(&self) -> RendezvousStatus;

    // =========================================================================
    // Threshold Signing
    // =========================================================================

    /// Sign a tree operation using threshold signing
    ///
    /// Returns an attested operation with the threshold signature.
    async fn sign_tree_op(&self, op: &TreeOp) -> Result<AttestedOp, IntentError>;

    /// Bootstrap signing keys for the authority
    ///
    /// Returns the public key package bytes for signature verification.
    async fn bootstrap_signing_keys(&self) -> Result<Vec<u8>, IntentError>;

    /// Get threshold configuration for the authority
    async fn get_threshold_config(&self) -> Option<ThresholdConfig>;

    /// Check if this runtime has signing capability
    async fn has_signing_capability(&self) -> bool;

    /// Get the public key package for signature verification
    async fn get_public_key_package(&self) -> Option<Vec<u8>>;

    /// Sign with a custom signing context
    async fn sign_with_context(
        &self,
        context: SigningContext,
    ) -> Result<ThresholdSignature, IntentError>;

    // =========================================================================
    // Invitation Operations
    // =========================================================================

    /// Export an invitation code for sharing
    ///
    /// Returns a shareable code that another user can use to establish
    /// a connection with this authority.
    async fn export_invitation(&self, invitation_id: &str) -> Result<String, IntentError>;

    // =========================================================================
    // Authentication
    // =========================================================================

    /// Check if the runtime is authenticated
    async fn is_authenticated(&self) -> bool;

    /// Get overall runtime status
    async fn get_status(&self) -> RuntimeStatus {
        RuntimeStatus {
            sync: self.get_sync_status().await,
            rendezvous: self.get_rendezvous_status().await,
            is_authenticated: self.is_authenticated().await,
        }
    }
}

/// Type alias for boxed runtime bridge
pub type BoxedRuntimeBridge = Arc<dyn RuntimeBridge>;

/// A no-op runtime bridge for offline/demo mode
///
/// This implementation returns sensible defaults and errors for operations
/// that require a real runtime.
#[derive(Debug, Clone)]
pub struct OfflineRuntimeBridge {
    authority_id: AuthorityId,
}

impl OfflineRuntimeBridge {
    /// Create a new offline runtime bridge
    pub fn new(authority_id: AuthorityId) -> Self {
        Self { authority_id }
    }
}

#[async_trait]
impl RuntimeBridge for OfflineRuntimeBridge {
    fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }

    async fn persist_facts(&self, _facts: &[JournalFact]) -> Result<(), IntentError> {
        // In offline mode, facts are stored locally only
        Ok(())
    }

    async fn get_sync_status(&self) -> SyncStatus {
        SyncStatus::default()
    }

    async fn get_sync_peers(&self) -> Vec<DeviceId> {
        Vec::new()
    }

    async fn trigger_sync(&self) -> Result<(), IntentError> {
        Err(IntentError::no_agent("Sync not available in offline mode"))
    }

    async fn get_discovered_peers(&self) -> Vec<AuthorityId> {
        Vec::new()
    }

    async fn get_rendezvous_status(&self) -> RendezvousStatus {
        RendezvousStatus::default()
    }

    async fn sign_tree_op(&self, _op: &TreeOp) -> Result<AttestedOp, IntentError> {
        Err(IntentError::no_agent(
            "Threshold signing not available in offline mode",
        ))
    }

    async fn bootstrap_signing_keys(&self) -> Result<Vec<u8>, IntentError> {
        Err(IntentError::no_agent(
            "Key bootstrapping not available in offline mode",
        ))
    }

    async fn get_threshold_config(&self) -> Option<ThresholdConfig> {
        None
    }

    async fn has_signing_capability(&self) -> bool {
        false
    }

    async fn get_public_key_package(&self) -> Option<Vec<u8>> {
        None
    }

    async fn sign_with_context(
        &self,
        _context: SigningContext,
    ) -> Result<ThresholdSignature, IntentError> {
        Err(IntentError::no_agent(
            "Threshold signing not available in offline mode",
        ))
    }

    async fn export_invitation(&self, _invitation_id: &str) -> Result<String, IntentError> {
        Err(IntentError::no_agent(
            "Invitation export not available in offline mode",
        ))
    }

    async fn is_authenticated(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_offline_bridge_defaults() {
        let authority = AuthorityId::new_from_entropy([42u8; 32]);
        let bridge = OfflineRuntimeBridge::new(authority);

        assert_eq!(bridge.authority_id(), authority);
        assert!(!bridge.is_authenticated().await);
        assert!(!bridge.has_signing_capability().await);
        assert!(bridge.get_sync_peers().await.is_empty());
        assert!(bridge.get_discovered_peers().await.is_empty());
    }

    #[tokio::test]
    async fn test_offline_bridge_operations_fail() {
        let authority = AuthorityId::new_from_entropy([42u8; 32]);
        let bridge = OfflineRuntimeBridge::new(authority);

        // Operations that require runtime should fail gracefully
        assert!(bridge.trigger_sync().await.is_err());
        assert!(bridge.bootstrap_signing_keys().await.is_err());
    }
}

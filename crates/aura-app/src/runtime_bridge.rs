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

/// Status of a guardian ceremony
#[derive(Debug, Clone)]
pub struct CeremonyStatus {
    /// Ceremony identifier
    pub ceremony_id: String,
    /// Number of guardians who have accepted
    pub accepted_count: u16,
    /// Total number of guardians
    pub total_count: u16,
    /// Threshold required for completion
    pub threshold: u16,
    /// Whether the ceremony is complete
    pub is_complete: bool,
    /// Whether the ceremony has failed
    pub has_failed: bool,
    /// List of guardian IDs who have accepted
    pub accepted_guardians: Vec<String>,
    /// Optional error message if failed
    pub error_message: Option<String>,
}

/// Information about a peer discovered via LAN (mDNS/UDP broadcast)
#[derive(Debug, Clone)]
pub struct LanPeerInfo {
    /// Authority ID of the discovered peer
    pub authority_id: AuthorityId,
    /// Network address (IP:port)
    pub address: String,
    /// When this peer was discovered (ms since epoch)
    pub discovered_at_ms: u64,
    /// Display name if available from the descriptor
    pub display_name: Option<String>,
}

// =============================================================================
// Invitation Bridge Types
// =============================================================================

/// Bridge-level invitation type (for RuntimeBridge API)
///
/// This is a minimal type for crossing the bridge boundary.
/// Workflows convert this to view types with display fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvitationBridgeType {
    /// Contact invitation with optional petname
    Contact { petname: Option<String> },
    /// Guardian invitation for a subject authority
    Guardian { subject_authority: AuthorityId },
    /// Channel/block invitation
    Channel { block_id: String },
}

/// Bridge-level invitation status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvitationBridgeStatus {
    /// Invitation is pending response
    Pending,
    /// Invitation was accepted
    Accepted,
    /// Invitation was declined
    Declined,
    /// Invitation was cancelled by sender
    Cancelled,
    /// Invitation has expired
    Expired,
}

/// Bridge-level invitation info returned from RuntimeBridge
///
/// Contains core invitation data without UI-specific display fields.
/// Workflows convert this to `views::invitations::Invitation` with resolved names.
#[derive(Debug, Clone)]
pub struct InvitationInfo {
    /// Unique invitation identifier
    pub invitation_id: String,
    /// Sender authority ID
    pub sender_id: AuthorityId,
    /// Receiver authority ID
    pub receiver_id: AuthorityId,
    /// Type of invitation
    pub invitation_type: InvitationBridgeType,
    /// Current status
    pub status: InvitationBridgeStatus,
    /// Creation timestamp (ms since epoch)
    pub created_at_ms: u64,
    /// Expiration timestamp (ms since epoch), if any
    pub expires_at_ms: Option<u64>,
    /// Optional message from sender
    pub message: Option<String>,
}

// =============================================================================
// Settings Bridge Types
// =============================================================================

/// Bridge-level settings state returned from RuntimeBridge
///
/// Contains persisted settings data. Device and contact lists
/// are derived views obtained from signals, not from here.
#[derive(Debug, Clone, Default)]
pub struct SettingsBridgeState {
    /// User's display name
    pub display_name: String,
    /// MFA policy setting
    pub mfa_policy: String,
    /// Threshold signing configuration (k of n)
    pub threshold_k: u16,
    /// Total guardians in threshold scheme
    pub threshold_n: u16,
    /// Number of registered devices
    pub device_count: usize,
    /// Number of contacts
    pub contact_count: usize,
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

    /// Trigger an on-demand discovery refresh
    ///
    /// This initiates an immediate discovery cycle rather than waiting
    /// for the next scheduled discovery interval.
    async fn trigger_discovery(&self) -> Result<(), IntentError>;

    // =========================================================================
    // LAN Discovery
    // =========================================================================

    /// Get list of peers discovered via LAN (mDNS/UDP broadcast)
    ///
    /// Returns peers that have been discovered on the local network.
    /// These are typically more immediately reachable than peers from
    /// internet rendezvous.
    async fn get_lan_peers(&self) -> Vec<LanPeerInfo>;

    /// Send an invitation to a LAN peer
    ///
    /// Sends an invitation code directly to a peer discovered on the LAN.
    /// This bypasses the need for manual code sharing when peers are on
    /// the same local network.
    async fn send_lan_invitation(
        &self,
        peer: &LanPeerInfo,
        invitation_code: &str,
    ) -> Result<(), IntentError>;

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

    /// Rotate guardian keys for a new threshold configuration
    ///
    /// Generates new FROST threshold keys for the given guardian configuration.
    /// The operation creates keys at a new epoch without invalidating the old keys
    /// until `commit_guardian_key_rotation` is called.
    ///
    /// # Arguments
    /// * `threshold_k` - Minimum signers required (k)
    /// * `total_n` - Total number of guardians (n)
    /// * `guardian_ids` - IDs of contacts who will become guardians
    ///
    /// # Returns
    /// A tuple of (new_epoch, key_packages, public_key_package) on success
    async fn rotate_guardian_keys(
        &self,
        threshold_k: u16,
        total_n: u16,
        guardian_ids: &[String],
    ) -> Result<(u64, Vec<Vec<u8>>, Vec<u8>), IntentError>;

    /// Commit a guardian key rotation after successful ceremony
    ///
    /// Called when all guardians have accepted and stored their key shares.
    /// This makes the new epoch authoritative.
    async fn commit_guardian_key_rotation(&self, new_epoch: u64) -> Result<(), IntentError>;

    /// Rollback a guardian key rotation after ceremony failure
    ///
    /// Called when the ceremony fails (guardian declined, user cancelled, or timeout).
    /// This discards the new epoch's keys and keeps the previous configuration active.
    async fn rollback_guardian_key_rotation(&self, failed_epoch: u64) -> Result<(), IntentError>;

    /// Initiate a guardian ceremony
    ///
    /// This method orchestrates the complete guardian ceremony:
    /// 1. Generates FROST threshold keys at a new epoch
    /// 2. Sends guardian invitations with key packages to each guardian
    /// 3. Returns a ceremony ID for tracking progress
    ///
    /// Guardians process invitations through their full runtimes and respond
    /// via the proper protocol. GuardianBinding facts are committed when
    /// threshold is reached.
    ///
    /// # Arguments
    /// * `threshold_k` - Minimum signers required (k)
    /// * `total_n` - Total number of guardians (n)
    /// * `guardian_ids` - IDs of contacts who will become guardians
    ///
    /// # Returns
    /// A ceremony ID for tracking progress
    async fn initiate_guardian_ceremony(
        &self,
        threshold_k: u16,
        total_n: u16,
        guardian_ids: &[String],
    ) -> Result<String, IntentError>;

    /// Get status of a guardian ceremony
    ///
    /// Returns the current state of the ceremony including:
    /// - Number of guardians who have accepted
    /// - Whether threshold has been reached
    /// - Whether ceremony is complete or failed
    ///
    /// # Arguments
    /// * `ceremony_id` - The ceremony ID returned from initiate_guardian_ceremony
    ///
    /// # Returns
    /// CeremonyStatus with current state
    async fn get_ceremony_status(&self, ceremony_id: &str) -> Result<CeremonyStatus, IntentError>;

    // =========================================================================
    // Invitation Operations
    // =========================================================================

    /// Export an invitation code for sharing
    ///
    /// Returns a shareable code that another user can use to establish
    /// a connection with this authority.
    async fn export_invitation(&self, invitation_id: &str) -> Result<String, IntentError>;

    /// Create a contact invitation
    ///
    /// # Arguments
    /// * `receiver` - Authority to invite as contact
    /// * `petname` - Optional petname for the contact
    /// * `message` - Optional message to include
    /// * `ttl_ms` - Optional time-to-live in milliseconds
    async fn create_contact_invitation(
        &self,
        receiver: AuthorityId,
        petname: Option<String>,
        message: Option<String>,
        ttl_ms: Option<u64>,
    ) -> Result<InvitationInfo, IntentError>;

    /// Create a guardian invitation
    ///
    /// # Arguments
    /// * `receiver` - Authority to invite as guardian
    /// * `subject` - Authority to be guarded
    /// * `message` - Optional message to include
    /// * `ttl_ms` - Optional time-to-live in milliseconds
    async fn create_guardian_invitation(
        &self,
        receiver: AuthorityId,
        subject: AuthorityId,
        message: Option<String>,
        ttl_ms: Option<u64>,
    ) -> Result<InvitationInfo, IntentError>;

    /// Create a channel/block invitation
    ///
    /// # Arguments
    /// * `receiver` - Authority to invite to channel
    /// * `block_id` - Block/channel identifier
    /// * `message` - Optional message to include
    /// * `ttl_ms` - Optional time-to-live in milliseconds
    async fn create_channel_invitation(
        &self,
        receiver: AuthorityId,
        block_id: String,
        message: Option<String>,
        ttl_ms: Option<u64>,
    ) -> Result<InvitationInfo, IntentError>;

    /// Accept a received invitation
    async fn accept_invitation(&self, invitation_id: &str) -> Result<(), IntentError>;

    /// Decline a received invitation
    async fn decline_invitation(&self, invitation_id: &str) -> Result<(), IntentError>;

    /// Cancel a sent invitation
    async fn cancel_invitation(&self, invitation_id: &str) -> Result<(), IntentError>;

    /// List pending invitations (sent and received)
    async fn list_pending_invitations(&self) -> Vec<InvitationInfo>;

    /// Import an invitation from a shareable code
    ///
    /// Parses the code and returns invitation info without accepting it.
    async fn import_invitation(&self, code: &str) -> Result<InvitationInfo, IntentError>;

    /// Get IDs of peers we have sent pending invitations to
    ///
    /// Returns a set of authority IDs for peers that have pending invitations
    /// from us. Used to mark discovered peers as "invited" in the UI.
    async fn get_invited_peer_ids(&self) -> Vec<String>;

    // =========================================================================
    // Settings Operations
    // =========================================================================

    /// Get current settings state
    async fn get_settings(&self) -> SettingsBridgeState;

    /// Update display name
    async fn set_display_name(&self, name: &str) -> Result<(), IntentError>;

    /// Update MFA policy
    async fn set_mfa_policy(&self, policy: &str) -> Result<(), IntentError>;

    // =========================================================================
    // Recovery Operations
    // =========================================================================

    /// Respond to a guardian ceremony invitation
    ///
    /// Called by a guardian to accept or decline participation in a ceremony.
    ///
    /// # Arguments
    /// * `ceremony_id` - The ceremony being responded to
    /// * `accept` - Whether to accept (true) or decline (false)
    /// * `reason` - Optional reason (used when declining)
    async fn respond_to_guardian_ceremony(
        &self,
        ceremony_id: &str,
        accept: bool,
        reason: Option<String>,
    ) -> Result<(), IntentError>;

    // =========================================================================
    // Authentication
    // =========================================================================

    /// Check if the runtime is authenticated
    async fn is_authenticated(&self) -> bool;

    // =========================================================================
    // Time Operations
    // =========================================================================

    /// Get current time in milliseconds since Unix epoch
    ///
    /// This provides a deterministic time source for simulation and testing.
    /// Production implementations use wall-clock time; test implementations
    /// can provide controlled time for reproducible tests.
    fn current_time_ms(&self) -> u64;

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

    async fn trigger_discovery(&self) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Discovery not available in offline mode",
        ))
    }

    async fn get_lan_peers(&self) -> Vec<LanPeerInfo> {
        Vec::new()
    }

    async fn send_lan_invitation(
        &self,
        _peer: &LanPeerInfo,
        _invitation_code: &str,
    ) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "LAN invitation not available in offline mode",
        ))
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

    async fn rotate_guardian_keys(
        &self,
        _threshold_k: u16,
        _total_n: u16,
        _guardian_ids: &[String],
    ) -> Result<(u64, Vec<Vec<u8>>, Vec<u8>), IntentError> {
        Err(IntentError::no_agent(
            "Key rotation not available in offline mode",
        ))
    }

    async fn commit_guardian_key_rotation(&self, _new_epoch: u64) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Key rotation not available in offline mode",
        ))
    }

    async fn rollback_guardian_key_rotation(&self, _failed_epoch: u64) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Key rotation not available in offline mode",
        ))
    }

    async fn initiate_guardian_ceremony(
        &self,
        _threshold_k: u16,
        _total_n: u16,
        _guardian_ids: &[String],
    ) -> Result<String, IntentError> {
        Err(IntentError::no_agent(
            "Guardian ceremony not available in offline mode",
        ))
    }

    async fn get_ceremony_status(&self, _ceremony_id: &str) -> Result<CeremonyStatus, IntentError> {
        Err(IntentError::no_agent(
            "Guardian ceremony not available in offline mode",
        ))
    }

    async fn export_invitation(&self, _invitation_id: &str) -> Result<String, IntentError> {
        Err(IntentError::no_agent(
            "Invitation export not available in offline mode",
        ))
    }

    async fn create_contact_invitation(
        &self,
        _receiver: AuthorityId,
        _petname: Option<String>,
        _message: Option<String>,
        _ttl_ms: Option<u64>,
    ) -> Result<InvitationInfo, IntentError> {
        Err(IntentError::no_agent(
            "Invitation creation not available in offline mode",
        ))
    }

    async fn create_guardian_invitation(
        &self,
        _receiver: AuthorityId,
        _subject: AuthorityId,
        _message: Option<String>,
        _ttl_ms: Option<u64>,
    ) -> Result<InvitationInfo, IntentError> {
        Err(IntentError::no_agent(
            "Invitation creation not available in offline mode",
        ))
    }

    async fn create_channel_invitation(
        &self,
        _receiver: AuthorityId,
        _block_id: String,
        _message: Option<String>,
        _ttl_ms: Option<u64>,
    ) -> Result<InvitationInfo, IntentError> {
        Err(IntentError::no_agent(
            "Invitation creation not available in offline mode",
        ))
    }

    async fn accept_invitation(&self, _invitation_id: &str) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Invitation acceptance not available in offline mode",
        ))
    }

    async fn decline_invitation(&self, _invitation_id: &str) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Invitation decline not available in offline mode",
        ))
    }

    async fn cancel_invitation(&self, _invitation_id: &str) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Invitation cancellation not available in offline mode",
        ))
    }

    async fn list_pending_invitations(&self) -> Vec<InvitationInfo> {
        Vec::new()
    }

    async fn import_invitation(&self, _code: &str) -> Result<InvitationInfo, IntentError> {
        Err(IntentError::no_agent(
            "Invitation import not available in offline mode",
        ))
    }

    async fn get_invited_peer_ids(&self) -> Vec<String> {
        Vec::new()
    }

    async fn get_settings(&self) -> SettingsBridgeState {
        SettingsBridgeState::default()
    }

    async fn set_display_name(&self, _name: &str) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Settings update not available in offline mode",
        ))
    }

    async fn set_mfa_policy(&self, _policy: &str) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Settings update not available in offline mode",
        ))
    }

    async fn respond_to_guardian_ceremony(
        &self,
        _ceremony_id: &str,
        _accept: bool,
        _reason: Option<String>,
    ) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Guardian ceremony response not available in offline mode",
        ))
    }

    async fn is_authenticated(&self) -> bool {
        false
    }

    fn current_time_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
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

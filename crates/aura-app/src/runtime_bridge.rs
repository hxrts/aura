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
use aura_core::effects::amp::{
    AmpCiphertext, ChannelCloseParams, ChannelCreateParams, ChannelJoinParams, ChannelLeaveParams,
    ChannelSendParams,
};
use aura_core::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::threshold::{
    ParticipantIdentity, SigningContext, ThresholdConfig, ThresholdSignature,
};
use aura_core::tree::{AttestedOp, TreeOp};
use aura_core::types::FrostThreshold;
use aura_core::DeviceId;
use aura_effects::PhysicalTimeHandler;
use aura_effects::ReactiveHandler;
use aura_journal::fact::RelationalFact;
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

/// High-level ceremony kind exposed across the runtime bridge boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CeremonyKind {
    /// Guardian threshold key rotation ceremony for an account authority.
    GuardianRotation,
    /// Device enrollment ceremony (account authority membership change + rotation).
    DeviceEnrollment,
    /// Device removal ceremony (account authority membership change + rotation).
    DeviceRemoval,
}

/// Result of starting a device enrollment ceremony.
#[derive(Debug, Clone)]
pub struct DeviceEnrollmentStart {
    /// Ceremony identifier for status polling / cancellation.
    pub ceremony_id: String,
    /// Shareable enrollment code (e.g. QR/copy-paste) to import on the new device.
    pub enrollment_code: String,
    /// Pending epoch created during prepare.
    pub pending_epoch: u64,
    /// Device id being enrolled.
    pub device_id: DeviceId,
}

/// Status of a key-rotation / membership-change ceremony.
///
/// This is intentionally generic so multiple ceremony kinds can share the same
/// UI and workflow infrastructure.
#[derive(Debug, Clone)]
pub struct KeyRotationCeremonyStatus {
    /// Ceremony identifier
    pub ceremony_id: String,
    /// What kind of ceremony this is
    pub kind: CeremonyKind,
    /// Number of participants who have accepted
    pub accepted_count: u16,
    /// Total number of required participants
    pub total_count: u16,
    /// Threshold required for completion
    pub threshold: u16,
    /// Whether the ceremony is complete
    pub is_complete: bool,
    /// Whether the ceremony has failed
    pub has_failed: bool,
    /// List of participants who have accepted
    pub accepted_participants: Vec<ParticipantIdentity>,
    /// Optional error message if failed
    pub error_message: Option<String>,
    /// Pending epoch for key rotation (if applicable)
    pub pending_epoch: Option<u64>,
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
    /// Pending epoch for key rotation
    ///
    /// This is the epoch that was created when the ceremony started.
    /// If the ceremony is canceled, this epoch's keys should be rolled back.
    /// If the ceremony succeeds, this becomes the active epoch.
    pub pending_epoch: Option<u64>,
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
    /// Contact invitation with optional nickname
    Contact { nickname: Option<String> },
    /// Guardian invitation for a subject authority
    Guardian { subject_authority: AuthorityId },
    /// Channel/block invitation
    Channel { block_id: String },
    /// Device enrollment invitation (out-of-band transfer).
    DeviceEnrollment {
        subject_authority: AuthorityId,
        initiator_device_id: DeviceId,
        device_id: DeviceId,
        device_name: Option<String>,
        ceremony_id: String,
        pending_epoch: u64,
    },
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

/// Bridge-level device summary.
///
/// This is used to populate UI settings screens without requiring the UI layer
/// to understand commitment-tree internals.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BridgeDeviceInfo {
    /// Stable device identifier
    pub id: String,
    /// Human-friendly label (best effort)
    pub name: String,
    /// Whether this is the current device
    pub is_current: bool,
    /// Last-seen timestamp (ms since epoch), if known
    pub last_seen: Option<u64>,
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

    /// Get the shared reactive handler used for UI-facing signals.
    ///
    /// In production/demo runtimes, this handler is owned by the runtime and
    /// driven by the ReactiveScheduler. Frontends should subscribe/read from
    /// this handler rather than maintaining parallel state.
    fn reactive_handler(&self) -> ReactiveHandler;

    // =========================================================================
    // Typed Fact Commit (Canonical)
    // =========================================================================

    /// Commit typed relational facts to the runtime journal.
    ///
    /// This is the canonical fact pipeline. The runtime is responsible for:
    /// - Attaching timestamps/order tokens
    /// - Persisting the committed facts
    /// - Publishing them to the ReactiveScheduler for UI signal updates
    async fn commit_relational_facts(&self, facts: &[RelationalFact]) -> Result<(), IntentError>;

    // =========================================================================
    // AMP Channel Operations
    // =========================================================================

    async fn amp_create_channel(
        &self,
        params: ChannelCreateParams,
    ) -> Result<ChannelId, IntentError>;

    async fn amp_close_channel(&self, params: ChannelCloseParams) -> Result<(), IntentError>;

    async fn amp_join_channel(&self, params: ChannelJoinParams) -> Result<(), IntentError>;

    async fn amp_leave_channel(&self, params: ChannelLeaveParams) -> Result<(), IntentError>;

    async fn amp_send_message(
        &self,
        params: ChannelSendParams,
    ) -> Result<AmpCiphertext, IntentError>;

    // =========================================================================
    // Moderation Operations
    // =========================================================================

    async fn moderation_kick(
        &self,
        context_id: ContextId,
        channel_id: ChannelId,
        target: AuthorityId,
        reason: Option<String>,
    ) -> Result<(), IntentError>;

    async fn moderation_ban(
        &self,
        context_id: ContextId,
        channel_id: ChannelId,
        target: AuthorityId,
        reason: Option<String>,
    ) -> Result<(), IntentError>;

    async fn moderation_unban(
        &self,
        context_id: ContextId,
        channel_id: ChannelId,
        target: AuthorityId,
    ) -> Result<(), IntentError>;

    async fn moderation_mute(
        &self,
        context_id: ContextId,
        channel_id: ChannelId,
        target: AuthorityId,
        duration_secs: Option<u64>,
    ) -> Result<(), IntentError>;

    async fn moderation_unmute(
        &self,
        context_id: ContextId,
        channel_id: ChannelId,
        target: AuthorityId,
    ) -> Result<(), IntentError>;

    async fn moderation_pin(
        &self,
        context_id: ContextId,
        channel_id: ChannelId,
        message_id: String,
    ) -> Result<(), IntentError>;

    async fn moderation_unpin(
        &self,
        context_id: ContextId,
        channel_id: ChannelId,
        message_id: String,
    ) -> Result<(), IntentError>;

    async fn channel_set_topic(
        &self,
        context_id: ContextId,
        channel_id: ChannelId,
        topic: String,
        timestamp_ms: u64,
    ) -> Result<(), IntentError>;

    // =========================================================================
    // Sync Operations
    // =========================================================================

    /// Get current sync status
    async fn get_sync_status(&self) -> SyncStatus;

    /// Get list of known sync peers
    async fn get_sync_peers(&self) -> Vec<DeviceId>;

    /// Trigger sync with peers (if sync service is available)
    async fn trigger_sync(&self) -> Result<(), IntentError>;

    /// Sync with a specific peer by ID
    ///
    /// Initiates targeted synchronization with the specified peer.
    /// This is useful for requesting state updates from a known good peer.
    async fn sync_with_peer(&self, peer_id: &str) -> Result<(), IntentError>;

    // =========================================================================
    // Peer Availability
    // =========================================================================

    /// Check whether a peer appears online/reachable.
    ///
    /// This is intentionally a best-effort signal intended for UI status (e.g. footer
    /// peer count). Implementations may use transport channel health, rendezvous
    /// reachability, or other heuristics.
    ///
    /// Default implementation returns `false`.
    async fn is_peer_online(&self, _peer: AuthorityId) -> bool {
        false
    }

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
    /// * `threshold_k` - Minimum signers required (k), must be >= 2 for FROST
    /// * `total_n` - Total number of guardians (n)
    /// * `guardian_ids` - IDs of contacts who will become guardians
    ///
    /// # Returns
    /// A tuple of (new_epoch, key_packages, public_key_package) on success
    async fn rotate_guardian_keys(
        &self,
        threshold_k: FrostThreshold,
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
    /// * `threshold_k` - Minimum signers required (k), must be >= 2 for FROST
    /// * `total_n` - Total number of guardians (n)
    /// * `guardian_ids` - IDs of contacts who will become guardians
    ///
    /// # Returns
    /// A ceremony ID for tracking progress
    async fn initiate_guardian_ceremony(
        &self,
        threshold_k: FrostThreshold,
        total_n: u16,
        guardian_ids: &[String],
    ) -> Result<String, IntentError>;

    /// Initiate a device enrollment ("add device") ceremony.
    ///
    /// Returns a shareable enrollment code for the invited device to import.
    async fn initiate_device_enrollment_ceremony(
        &self,
        device_name: String,
    ) -> Result<DeviceEnrollmentStart, IntentError>;

    /// Initiate a device removal ("remove device") ceremony.
    ///
    /// The runtime is responsible for rotating threshold keys and updating the
    /// account commitment tree to remove the specified device leaf.
    async fn initiate_device_removal_ceremony(
        &self,
        device_id: String,
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

    /// Get status of a key rotation ceremony (generic form).
    async fn get_key_rotation_ceremony_status(
        &self,
        ceremony_id: &str,
    ) -> Result<KeyRotationCeremonyStatus, IntentError>;

    /// Cancel an in-progress key rotation ceremony (best effort).
    ///
    /// Implementations should:
    /// - mark the ceremony failed/canceled
    /// - rollback any pending epoch (if present)
    async fn cancel_key_rotation_ceremony(&self, ceremony_id: &str) -> Result<(), IntentError>;

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
    /// * `nickname` - Optional nickname for the contact
    /// * `message` - Optional message to include
    /// * `ttl_ms` - Optional time-to-live in milliseconds
    async fn create_contact_invitation(
        &self,
        receiver: AuthorityId,
        nickname: Option<String>,
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

    /// List devices for the current account (best effort).
    async fn list_devices(&self) -> Vec<BridgeDeviceInfo>;

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
    // Authorization / Capabilities
    // =========================================================================

    /// Check if the user has a specific command capability
    ///
    /// This method integrates with the Biscuit authorization system to verify
    /// that the user has permission to execute commands requiring specific
    /// capabilities. Used by CommandDispatcher for fine-grained authorization.
    ///
    /// # Arguments
    /// * `capability` - The capability string (e.g., "send_dm", "moderate:kick")
    ///
    /// # Returns
    /// `true` if the user has the capability, `false` otherwise
    ///
    /// # Default Implementation
    /// Returns `true` for all capabilities. Override in implementations that
    /// integrate with Biscuit tokens or other authorization systems.
    async fn has_command_capability(&self, _capability: &str) -> bool {
        // Default: allow all capabilities
        // Implementations with Biscuit integration should check the token
        true
    }

    // =========================================================================
    // Time Operations
    // =========================================================================

    /// Get current time in milliseconds since Unix epoch
    ///
    /// This provides a deterministic time source for simulation and testing.
    /// Production implementations use wall-clock time; test implementations
    /// can provide controlled time for reproducible tests.
    async fn current_time_ms(&self) -> Result<u64, IntentError>;

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
#[derive(Clone)]
pub struct OfflineRuntimeBridge {
    authority_id: AuthorityId,
    reactive: ReactiveHandler,
}

impl OfflineRuntimeBridge {
    /// Create a new offline runtime bridge
    pub fn new(authority_id: AuthorityId) -> Self {
        Self {
            authority_id,
            reactive: ReactiveHandler::new(),
        }
    }
}

#[async_trait]
impl RuntimeBridge for OfflineRuntimeBridge {
    fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }

    fn reactive_handler(&self) -> ReactiveHandler {
        self.reactive.clone()
    }

    async fn commit_relational_facts(&self, _facts: &[RelationalFact]) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Relational fact commit not available in offline mode",
        ))
    }

    async fn amp_create_channel(
        &self,
        _params: ChannelCreateParams,
    ) -> Result<ChannelId, IntentError> {
        Err(IntentError::no_agent("AMP not available in offline mode"))
    }

    async fn amp_close_channel(&self, _params: ChannelCloseParams) -> Result<(), IntentError> {
        Err(IntentError::no_agent("AMP not available in offline mode"))
    }

    async fn amp_join_channel(&self, _params: ChannelJoinParams) -> Result<(), IntentError> {
        Err(IntentError::no_agent("AMP not available in offline mode"))
    }

    async fn amp_leave_channel(&self, _params: ChannelLeaveParams) -> Result<(), IntentError> {
        Err(IntentError::no_agent("AMP not available in offline mode"))
    }

    async fn amp_send_message(
        &self,
        _params: ChannelSendParams,
    ) -> Result<AmpCiphertext, IntentError> {
        Err(IntentError::no_agent("AMP not available in offline mode"))
    }

    async fn moderation_kick(
        &self,
        _context_id: ContextId,
        _channel_id: ChannelId,
        _target: AuthorityId,
        _reason: Option<String>,
    ) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Moderation not available in offline mode",
        ))
    }

    async fn moderation_ban(
        &self,
        _context_id: ContextId,
        _channel_id: ChannelId,
        _target: AuthorityId,
        _reason: Option<String>,
    ) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Moderation not available in offline mode",
        ))
    }

    async fn moderation_unban(
        &self,
        _context_id: ContextId,
        _channel_id: ChannelId,
        _target: AuthorityId,
    ) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Moderation not available in offline mode",
        ))
    }

    async fn moderation_mute(
        &self,
        _context_id: ContextId,
        _channel_id: ChannelId,
        _target: AuthorityId,
        _duration_secs: Option<u64>,
    ) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Moderation not available in offline mode",
        ))
    }

    async fn moderation_unmute(
        &self,
        _context_id: ContextId,
        _channel_id: ChannelId,
        _target: AuthorityId,
    ) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Moderation not available in offline mode",
        ))
    }

    async fn moderation_pin(
        &self,
        _context_id: ContextId,
        _channel_id: ChannelId,
        _message_id: String,
    ) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Moderation not available in offline mode",
        ))
    }

    async fn moderation_unpin(
        &self,
        _context_id: ContextId,
        _channel_id: ChannelId,
        _message_id: String,
    ) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Moderation not available in offline mode",
        ))
    }

    async fn channel_set_topic(
        &self,
        _context_id: ContextId,
        _channel_id: ChannelId,
        _topic: String,
        _timestamp_ms: u64,
    ) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Channel metadata not available in offline mode",
        ))
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

    async fn sync_with_peer(&self, _peer_id: &str) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Peer-targeted sync not available in offline mode",
        ))
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
        _threshold_k: FrostThreshold,
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
        _threshold_k: FrostThreshold,
        _total_n: u16,
        _guardian_ids: &[String],
    ) -> Result<String, IntentError> {
        Err(IntentError::no_agent(
            "Guardian ceremony not available in offline mode",
        ))
    }

    async fn initiate_device_enrollment_ceremony(
        &self,
        _device_name: String,
    ) -> Result<DeviceEnrollmentStart, IntentError> {
        Err(IntentError::no_agent(
            "Device enrollment not available in offline mode",
        ))
    }

    async fn initiate_device_removal_ceremony(
        &self,
        _device_id: String,
    ) -> Result<String, IntentError> {
        Err(IntentError::no_agent(
            "Device removal not available in offline mode",
        ))
    }

    async fn get_ceremony_status(&self, _ceremony_id: &str) -> Result<CeremonyStatus, IntentError> {
        Err(IntentError::no_agent(
            "Guardian ceremony not available in offline mode",
        ))
    }

    async fn get_key_rotation_ceremony_status(
        &self,
        _ceremony_id: &str,
    ) -> Result<KeyRotationCeremonyStatus, IntentError> {
        Err(IntentError::no_agent(
            "Key rotation ceremonies not available in offline mode",
        ))
    }

    async fn cancel_key_rotation_ceremony(&self, _ceremony_id: &str) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Key rotation ceremonies not available in offline mode",
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
        _nickname: Option<String>,
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

    async fn list_devices(&self) -> Vec<BridgeDeviceInfo> {
        Vec::new()
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

    async fn current_time_ms(&self) -> Result<u64, IntentError> {
        // Offline bridge uses best-effort physical time for UI surfaces.
        let now_ms = PhysicalTimeHandler::new().physical_time_now_ms();
        Ok(now_ms)
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

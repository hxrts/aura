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
//!  aura-app (pure)         aura-agent (runtime)
//! ┌───────────────────┐    ┌──────────────────┐
//! │ AppCore           │    │ AuraAgent        │
//! │   ┌─────────────┐ │    │   implements     │
//! │   │RuntimeBridge│◄─────│   RuntimeBridge  │
//! │   └─────────────┘ │    │                  │
//! └───────────────────┘    └──────────────────┘
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
use crate::views::naming::{truncate_id_for_display, EffectiveName};
use crate::ReactiveHandler;
use async_trait::async_trait;
use aura_core::effects::amp::{
    AmpCiphertext, ChannelBootstrapPackage, ChannelCloseParams, ChannelCreateParams,
    ChannelJoinParams, ChannelLeaveParams, ChannelSendParams,
};
use aura_core::threshold::{
    AgreementMode, ParticipantIdentity, SigningContext, ThresholdConfig, ThresholdSignature,
};
use aura_core::tree::{AttestedOp, TreeOp};
use aura_core::types::identifiers::{AuthorityId, CeremonyId, ChannelId, ContextId, InvitationId};
use aura_core::types::{Epoch, FrostThreshold};
use aura_core::{DeviceId, OwnedShutdownToken, OwnedTaskSpawner};
use aura_journal::fact::{FactOptions, RelationalFact};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

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
    /// Number of active sync sessions (currently syncing with N peers)
    pub active_sessions: usize,
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
    /// Device threshold key rotation ceremony (multifactor authority).
    DeviceRotation,
    /// Device enrollment ceremony (account authority membership change + rotation).
    DeviceEnrollment,
    /// Device removal ceremony (account authority membership change + rotation).
    DeviceRemoval,
    /// Guardian-based recovery ceremony.
    Recovery,
    /// Invitation ceremony (contact/guardian/channel).
    Invitation,
    /// Rendezvous secure-channel ceremony.
    RendezvousSecureChannel,
    /// OTA hard-fork activation ceremony.
    OtaActivation,
}

/// Result of starting a device enrollment ceremony.
#[derive(Debug, Clone)]
pub struct DeviceEnrollmentStart {
    /// Ceremony identifier for status polling / cancellation.
    pub ceremony_id: CeremonyId,
    /// Shareable enrollment code (e.g. QR/copy-paste) to import on the new device.
    pub enrollment_code: String,
    /// Pending epoch created during prepare.
    pub pending_epoch: Epoch,
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
    pub ceremony_id: CeremonyId,
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
    pub pending_epoch: Option<Epoch>,
    /// Agreement mode (A1/A2/A3)
    pub agreement_mode: AgreementMode,
    /// Whether reversion is still possible
    pub reversion_risk: bool,
}

/// Status of a guardian ceremony
#[derive(Debug, Clone)]
pub struct CeremonyStatus {
    /// Ceremony identifier
    pub ceremony_id: CeremonyId,
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
    pub accepted_guardians: Vec<AuthorityId>,
    /// Optional error message if failed
    pub error_message: Option<String>,
    /// Pending epoch for key rotation
    ///
    /// This is the epoch that was created when the ceremony started.
    /// If the ceremony is canceled, this epoch's keys should be rolled back.
    /// If the ceremony succeeds, this becomes the active epoch.
    pub pending_epoch: Option<Epoch>,
    /// Agreement mode (A1/A2/A3)
    pub agreement_mode: AgreementMode,
    /// Whether reversion is still possible
    pub reversion_risk: bool,
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
    /// Nickname suggestion if available from the descriptor
    pub nickname_suggestion: Option<String>,
}

// =============================================================================
// Invitation Bridge Types
// =============================================================================

/// Bridge-level invitation type (for RuntimeBridge API)
///
/// This is a minimal type for crossing the bridge boundary.
/// Workflows convert this to view types with display fields.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(missing_docs)] // Field docs not required for bridge types
pub enum InvitationBridgeType {
    /// Contact invitation with optional nickname
    Contact { nickname: Option<String> },
    /// Guardian invitation for a subject authority
    Guardian { subject_authority: AuthorityId },
    /// Channel/home invitation with optional nickname suggestion
    Channel {
        home_id: String,
        context_id: Option<ContextId>,
        nickname_suggestion: Option<String>,
    },
    /// Device enrollment invitation (out-of-band transfer).
    DeviceEnrollment {
        subject_authority: AuthorityId,
        initiator_device_id: DeviceId,
        device_id: DeviceId,
        nickname_suggestion: Option<String>,
        ceremony_id: CeremonyId,
        pending_epoch: Epoch,
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
    /// Unique invitation identifier (typed for type safety)
    pub invitation_id: InvitationId,
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

/// Authoritative moderation status for an authority in a home-scoped context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AuthoritativeModerationStatus {
    /// Whether the authority is banned from the home/context.
    pub is_banned: bool,
    /// Whether the authority is muted at the queried time.
    pub is_muted: bool,
    /// Whether the authoritative home roster is populated.
    pub roster_known: bool,
    /// Whether the authority is an authoritative member of the home roster.
    pub is_member: bool,
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
    /// User's nickname suggestion (what they want to be called)
    pub nickname_suggestion: String,
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

impl SettingsBridgeState {
    /// Returns `true` if this state was populated from a real runtime.
    ///
    /// The `Default` implementation produces k=0, n=0 which is
    /// cryptographically invalid.  UI code should check this before
    /// displaying threshold information.
    pub fn has_valid_threshold(&self) -> bool {
        self.threshold_k >= 2 && self.threshold_n >= self.threshold_k
    }
}

/// Bridge-level device summary.
///
/// This is used to populate UI settings screens without requiring the UI layer
/// to understand commitment-tree internals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeDeviceInfo {
    /// Stable device identifier
    pub id: DeviceId,
    /// Human-friendly label (best effort, computed for display)
    pub name: String,
    /// Local nickname override (user-assigned name for this device)
    pub nickname: Option<String>,
    /// Nickname suggestion (what the device wants to be called, from enrollment)
    pub nickname_suggestion: Option<String>,
    /// Whether this is the current device
    pub is_current: bool,
    /// Last-seen timestamp (ms since epoch), if known
    pub last_seen: Option<u64>,
}

impl EffectiveName for BridgeDeviceInfo {
    fn nickname(&self) -> Option<&str> {
        self.nickname.as_deref().filter(|s| !s.is_empty())
    }

    fn nickname_suggestion(&self) -> Option<&str> {
        self.nickname_suggestion
            .as_deref()
            .filter(|s| !s.is_empty())
    }

    fn fallback_id(&self) -> String {
        truncate_id_for_display(&self.id.to_string())
    }
}

/// Bridge-level authority summary for settings and authority switching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeAuthorityInfo {
    /// Stable authority identifier.
    pub id: AuthorityId,
    /// Best-effort display label or nickname suggestion.
    pub nickname_suggestion: Option<String>,
    /// Whether this is the currently active authority for the runtime.
    pub is_current: bool,
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
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[allow(missing_docs)] // Trait method docs evolving with API
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

    /// Optional runtime task spawner for background work.
    ///
    /// Runtime implementations can provide a portable task spawner so
    /// `aura-app` can schedule background work without binding to tokio.
    fn task_spawner(&self) -> Option<OwnedTaskSpawner> {
        None
    }

    /// Optional runtime cancellation token for background work.
    fn cancellation_token(&self) -> Option<OwnedShutdownToken> {
        self.task_spawner()
            .map(|spawner| spawner.shutdown_token().clone())
    }

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

    /// Commit typed relational facts with additional options.
    ///
    /// Same as `commit_relational_facts` but allows specifying options like
    /// ack tracking. Uses default options if not specified.
    ///
    /// # Arguments
    /// * `facts` - The facts to commit
    /// * `options` - Options controlling fact behavior (e.g., ack tracking)
    /// # Important
    ///
    /// The default implementation **silently drops the `options` parameter**
    /// and delegates to [`commit_relational_facts`].  Production
    /// implementations MUST override this method if callers rely on
    /// `FactOptions` (e.g., ack tracking for delivery receipts).  Relying
    /// on the default when options carry semantic meaning will silently
    /// disable that behavior.
    async fn commit_relational_facts_with_options(
        &self,
        facts: &[RelationalFact],
        options: FactOptions,
    ) -> Result<(), IntentError> {
        let _ = options;
        self.commit_relational_facts(facts).await
    }

    /// Send a chat relational fact to a peer over transport.
    ///
    /// Runtime implementations should forward this fact out-of-band so remote
    /// peers can ingest it without relying on anti-entropy sync.
    async fn send_chat_fact(
        &self,
        _peer: AuthorityId,
        _context: ContextId,
        _fact: &RelationalFact,
    ) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Chat fact transport not available in offline mode",
        ))
    }

    // =========================================================================
    // AMP Channel Operations
    // =========================================================================

    async fn amp_create_channel(
        &self,
        params: ChannelCreateParams,
    ) -> Result<ChannelId, IntentError>;

    /// Create or retrieve a bootstrap key for provisional AMP messaging.
    async fn amp_create_channel_bootstrap(
        &self,
        context: ContextId,
        channel: ChannelId,
        recipients: Vec<AuthorityId>,
    ) -> Result<ChannelBootstrapPackage, IntentError>;

    /// Return whether canonical AMP state is materialized for the given channel.
    ///
    /// This is the authoritative runtime-side readiness boundary for workflows
    /// that require a channel checkpoint to exist before later operations such
    /// as invitation bootstrap creation can succeed.
    async fn amp_channel_state_exists(
        &self,
        context: ContextId,
        channel: ChannelId,
    ) -> Result<bool, IntentError>;

    /// Return the authoritative current participant set for an AMP channel.
    async fn amp_list_channel_participants(
        &self,
        context: ContextId,
        channel: ChannelId,
    ) -> Result<Vec<AuthorityId>, IntentError>;

    /// Return authoritative moderation status for an authority within a
    /// home-scoped context.
    async fn moderation_status(
        &self,
        context_id: ContextId,
        channel_id: ChannelId,
        authority_id: AuthorityId,
        current_time_ms: u64,
    ) -> Result<AuthoritativeModerationStatus, IntentError> {
        let _ = (context_id, channel_id, authority_id, current_time_ms);
        Err(IntentError::no_agent(
            "Authoritative moderation status not available in offline mode",
        ))
    }

    /// Resolve the authoritative checkpoint context for a channel from runtime-owned facts.
    async fn resolve_amp_channel_context(
        &self,
        channel: ChannelId,
    ) -> Result<Option<ContextId>, IntentError>;

    /// Resolve authoritative channel identifiers by normalized display name.
    ///
    /// This is the runtime-owned replacement for projection-backed channel name
    /// lookup in workflow code.
    async fn resolve_authoritative_channel_ids_by_name(
        &self,
        channel_name: &str,
    ) -> Result<Vec<ChannelId>, IntentError>;

    /// Repair local AMP membership after a checkpoint repair.
    ///
    /// This exists because bootstrap repair has stronger preconditions than a
    /// generic join flow: the caller has already established or repaired the
    /// channel checkpoint and only needs to materialize local membership.
    async fn amp_repair_local_channel_membership(
        &self,
        params: ChannelJoinParams,
    ) -> Result<(), IntentError>;

    async fn amp_close_channel(&self, params: ChannelCloseParams) -> Result<(), IntentError>;

    async fn amp_join_channel(&self, params: ChannelJoinParams) -> Result<(), IntentError>;

    async fn amp_leave_channel(&self, params: ChannelLeaveParams) -> Result<(), IntentError>;

    /// Bump channel epoch to rotate the group key.
    async fn bump_channel_epoch(
        &self,
        context: ContextId,
        channel: ChannelId,
        reason: String,
    ) -> Result<(), IntentError>;

    /// Start monitoring channel invitations and bump the epoch once all accept.
    async fn start_channel_invitation_monitor(
        &self,
        invitation_ids: Vec<String>,
        context: ContextId,
        channel: ChannelId,
    ) -> Result<(), IntentError>;

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

    /// Get current sync status, distinguishing runtime unavailability from a real
    /// zero-value status.
    async fn try_get_sync_status(&self) -> Result<SyncStatus, IntentError>;

    /// Get list of known sync peers, distinguishing runtime unavailability from
    /// a real empty peer set.
    async fn try_get_sync_peers(&self) -> Result<Vec<DeviceId>, IntentError>;

    /// Trigger sync with peers (if sync service is available)
    async fn trigger_sync(&self) -> Result<(), IntentError>;

    /// Process any pending ceremony envelopes/messages.
    ///
    /// This is required for flows like device enrollment where the invitee must
    /// ingest ceremony commit messages before signal-backed state can converge.
    async fn process_ceremony_messages(&self) -> Result<(), IntentError>;

    /// Sync with a specific peer by ID
    ///
    /// Initiates targeted synchronization with the specified peer.
    /// This is useful for requesting state updates from a known good peer.
    async fn sync_with_peer(&self, peer_id: &str) -> Result<(), IntentError>;

    /// Ensure a transport/rendezvous channel exists for a specific authority
    /// within the provided context before parity-critical flows rely on remote
    /// delivery.
    async fn ensure_peer_channel(
        &self,
        _context: ContextId,
        _peer: AuthorityId,
    ) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Peer channel establishment not available in offline mode",
        ))
    }

    // =========================================================================
    // Peer Availability
    // =========================================================================

    /// Check whether a peer appears online/reachable.
    ///
    /// This is intentionally a best-effort signal intended for UI status (e.g. footer
    /// peer count). Implementations may use transport channel health, rendezvous
    /// reachability, or other heuristics.
    ///
    /// All implementations must provide this method — there is no default so
    /// that forgetting to implement it is a compile-time error rather than a
    /// silent "all peers offline" bug.
    async fn is_peer_online(&self, peer: AuthorityId) -> bool;

    // =========================================================================
    // Peer Discovery
    // =========================================================================

    /// Get list of discovered peers, distinguishing runtime unavailability from
    /// a real empty discovery set.
    async fn try_get_discovered_peers(&self) -> Result<Vec<AuthorityId>, IntentError>;

    /// Get rendezvous status, distinguishing runtime unavailability from a real
    /// zero-value status.
    async fn try_get_rendezvous_status(&self) -> Result<RendezvousStatus, IntentError>;

    /// Trigger an on-demand discovery refresh
    ///
    /// This initiates an immediate discovery cycle rather than waiting
    /// for the next scheduled discovery interval.
    async fn trigger_discovery(&self) -> Result<(), IntentError>;

    // =========================================================================
    // LAN Discovery
    // =========================================================================

    /// Get list of peers discovered via LAN, distinguishing runtime
    /// unavailability from a real empty LAN set.
    async fn try_get_lan_peers(&self) -> Result<Vec<LanPeerInfo>, IntentError>;

    /// Send an invitation to a LAN peer
    ///
    /// Sends an invite code directly to a peer discovered on the LAN.
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
        guardian_ids: &[AuthorityId],
    ) -> Result<(Epoch, Vec<Vec<u8>>, Vec<u8>), IntentError>;

    /// Commit a guardian key rotation after successful ceremony
    ///
    /// Called when all guardians have accepted and stored their key shares.
    /// This makes the new epoch authoritative.
    async fn commit_guardian_key_rotation(&self, new_epoch: Epoch) -> Result<(), IntentError>;

    /// Rollback a guardian key rotation after ceremony failure
    ///
    /// Called when the ceremony fails (guardian declined, user cancelled, or timeout).
    /// This discards the new epoch's keys and keeps the previous configuration active.
    async fn rollback_guardian_key_rotation(&self, failed_epoch: Epoch) -> Result<(), IntentError>;

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
        guardian_ids: &[AuthorityId],
    ) -> Result<CeremonyId, IntentError>;

    /// Initiate a device threshold (multifactor) ceremony.
    ///
    /// Rotates the threshold signing keys for the selected device set and
    /// distributes key packages to the participating devices.
    ///
    /// # Arguments
    /// * `threshold_k` - Minimum signers required (k), must be >= 2 for FROST
    /// * `total_n` - Total number of devices (n)
    /// * `device_ids` - IDs of devices participating in the threshold set
    ///
    /// # Returns
    /// A ceremony ID for tracking progress
    async fn initiate_device_threshold_ceremony(
        &self,
        threshold_k: FrostThreshold,
        total_n: u16,
        device_ids: &[String],
    ) -> Result<CeremonyId, IntentError>;

    /// Initiate a device enrollment ("add device") ceremony.
    ///
    /// Returns a shareable enrollment code for the invited device to import.
    ///
    /// For the two-step exchange flow:
    /// 1. The new device creates its own authority first
    /// 2. The new device shares its authority_id with the initiator
    /// 3. The initiator passes the invitee's authority_id to this function
    /// 4. An addressed enrollment invitation is created
    ///
    /// # Arguments
    /// * `nickname_suggestion` - Suggested name for the device
    /// * `invitee_authority_id` - The authority ID of the new device.
    ///   Device enrollment always creates an addressed invitation bound to this
    ///   authority.
    async fn initiate_device_enrollment_ceremony(
        &self,
        nickname_suggestion: String,
        invitee_authority_id: AuthorityId,
    ) -> Result<DeviceEnrollmentStart, IntentError>;

    /// Initiate a device removal ("remove device") ceremony.
    ///
    /// The runtime is responsible for rotating threshold keys and updating the
    /// account commitment tree to remove the specified device leaf.
    async fn initiate_device_removal_ceremony(
        &self,
        device_id: String,
    ) -> Result<CeremonyId, IntentError>;

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
    async fn get_ceremony_status(
        &self,
        ceremony_id: &CeremonyId,
    ) -> Result<CeremonyStatus, IntentError>;

    /// Get status of a key rotation ceremony (generic form).
    async fn get_key_rotation_ceremony_status(
        &self,
        ceremony_id: &CeremonyId,
    ) -> Result<KeyRotationCeremonyStatus, IntentError>;

    /// Cancel an in-progress key rotation ceremony (best effort).
    ///
    /// Implementations should:
    /// - mark the ceremony failed/canceled
    /// - rollback any pending epoch (if present)
    async fn cancel_key_rotation_ceremony(
        &self,
        ceremony_id: &CeremonyId,
    ) -> Result<(), IntentError>;

    // =========================================================================
    // Invitation Operations
    // =========================================================================

    /// Export an invite code for sharing
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

    /// Create a channel/home invitation
    ///
    /// # Arguments
    /// * `receiver` - Authority to invite to channel
    /// * `home_id` - Home/channel identifier
    /// * `context_id` - Optional explicit channel context override
    /// * `bootstrap` - Optional bootstrap key package for provisional AMP
    /// * `message` - Optional message to include
    /// * `ttl_ms` - Optional time-to-live in milliseconds
    async fn create_channel_invitation(
        &self,
        receiver: AuthorityId,
        home_id: String,
        context_id: Option<ContextId>,
        channel_name_hint: Option<String>,
        bootstrap: Option<ChannelBootstrapPackage>,
        message: Option<String>,
        ttl_ms: Option<u64>,
    ) -> Result<InvitationInfo, IntentError>;

    /// Accept a received invitation
    async fn accept_invitation(&self, invitation_id: &str) -> Result<(), IntentError>;

    /// Decline a received invitation
    async fn decline_invitation(&self, invitation_id: &str) -> Result<(), IntentError>;

    /// Cancel a sent invitation
    async fn cancel_invitation(&self, invitation_id: &str) -> Result<(), IntentError>;

    /// List pending invitations, distinguishing runtime unavailability from a
    /// real empty pending set.
    async fn try_list_pending_invitations(&self) -> Result<Vec<InvitationInfo>, IntentError>;

    /// Import an invitation from a shareable code
    ///
    /// Parses the code and returns invitation info without accepting it.
    async fn import_invitation(&self, code: &str) -> Result<InvitationInfo, IntentError>;

    /// Get IDs of peers with pending invitations, distinguishing runtime
    /// unavailability from a real empty invited set.
    async fn try_get_invited_peer_ids(&self) -> Result<Vec<AuthorityId>, IntentError>;

    // =========================================================================
    // Settings Operations
    // =========================================================================

    /// Get current settings state, distinguishing runtime unavailability from a
    /// real stored settings snapshot.
    async fn try_get_settings(&self) -> Result<SettingsBridgeState, IntentError>;

    /// Returns true when an account configuration has been persisted for this runtime.
    async fn has_account_config(&self) -> Result<bool, IntentError>;

    /// Initialize account configuration for the current authority/runtime.
    ///
    /// This persists the current authority/context bootstrap metadata and
    /// nickname suggestion for first-run onboarding.
    async fn initialize_account(&self, nickname_suggestion: &str) -> Result<(), IntentError>;

    /// List devices for the current account, distinguishing runtime
    /// unavailability from a real empty device list.
    async fn try_list_devices(&self) -> Result<Vec<BridgeDeviceInfo>, IntentError>;

    /// List authorities available to this runtime/device, distinguishing
    /// runtime unavailability from a real empty authority list.
    async fn try_list_authorities(&self) -> Result<Vec<BridgeAuthorityInfo>, IntentError>;

    /// Update nickname suggestion (what the user wants to be called)
    async fn set_nickname_suggestion(&self, name: &str) -> Result<(), IntentError>;

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
        ceremony_id: &CeremonyId,
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
        // Default: allow all capabilities.
        //
        // WARNING: This default is permissive by design so that offline and test
        // bridges work without Biscuit integration.  Production RuntimeBridge
        // implementations MUST override this to evaluate Biscuit tokens or an
        // equivalent capability check.  Relying on this default in a production
        // deployment disables command-level authorization.
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

    /// Sleep for the specified number of milliseconds.
    ///
    /// This provides a runtime-agnostic sleep mechanism. Production implementations
    /// delegate to the runtime's sleep primitive; simulation implementations can
    /// use virtual time.
    async fn sleep_ms(&self, ms: u64);

    /// Get overall runtime status.
    async fn get_status(&self) -> Result<RuntimeStatus, IntentError> {
        Ok(RuntimeStatus {
            sync: self.try_get_sync_status().await?,
            rendezvous: self.try_get_rendezvous_status().await?,
            is_authenticated: self.is_authenticated().await,
        })
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
    pending_invitations: Arc<Mutex<Option<Vec<InvitationInfo>>>>,
    amp_channel_contexts: Arc<Mutex<HashMap<ChannelId, ContextId>>>,
    authoritative_channel_name_matches: Arc<Mutex<HashMap<String, Vec<ChannelId>>>>,
    amp_channel_states: Arc<Mutex<HashMap<(ContextId, ChannelId), bool>>>,
    amp_channel_participants: Arc<Mutex<HashMap<(ContextId, ChannelId), Vec<AuthorityId>>>>,
    moderation_statuses:
        Arc<Mutex<HashMap<(ContextId, ChannelId, AuthorityId), AuthoritativeModerationStatus>>>,
}

impl OfflineRuntimeBridge {
    /// Create a new offline runtime bridge
    pub fn new(authority_id: AuthorityId) -> Self {
        Self {
            authority_id,
            reactive: ReactiveHandler::new(),
            pending_invitations: Arc::new(Mutex::new(None)),
            amp_channel_contexts: Arc::new(Mutex::new(HashMap::new())),
            authoritative_channel_name_matches: Arc::new(Mutex::new(HashMap::new())),
            amp_channel_states: Arc::new(Mutex::new(HashMap::new())),
            amp_channel_participants: Arc::new(Mutex::new(HashMap::new())),
            moderation_statuses: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[cfg(test)]
    /// Configure the pending invitation snapshot returned by the offline bridge.
    pub fn set_pending_invitations(&self, invitations: Vec<InvitationInfo>) {
        *self.pending_invitations.lock().expect("pending invitations mutex") = Some(invitations);
    }

    #[cfg(test)]
    /// Configure a runtime-owned moderation status answer for the offline bridge.
    pub fn set_moderation_status(
        &self,
        context_id: ContextId,
        channel_id: ChannelId,
        authority_id: AuthorityId,
        status: AuthoritativeModerationStatus,
    ) {
        self.moderation_statuses
            .lock()
            .expect("moderation statuses mutex")
            .insert((context_id, channel_id, authority_id), status);
    }

    #[cfg(test)]
    /// Configure authoritative AMP context for a channel.
    pub fn set_amp_channel_context(&self, channel_id: ChannelId, context_id: ContextId) {
        self.amp_channel_contexts
            .lock()
            .expect("amp channel contexts mutex")
            .insert(channel_id, context_id);
    }

    #[cfg(test)]
    /// Configure authoritative channel-name lookup results.
    pub fn set_authoritative_channel_name_matches(
        &self,
        channel_name: impl Into<String>,
        channel_ids: Vec<ChannelId>,
    ) {
        self.authoritative_channel_name_matches
            .lock()
            .expect("authoritative channel name matches mutex")
            .insert(
                channel_name.into().trim().to_ascii_lowercase(),
                channel_ids,
            );
    }

    #[cfg(test)]
    /// Configure authoritative AMP participants for a channel.
    pub fn set_amp_channel_participants(
        &self,
        context_id: ContextId,
        channel_id: ChannelId,
        participants: Vec<AuthorityId>,
    ) {
        self.amp_channel_contexts
            .lock()
            .expect("amp channel contexts mutex")
            .insert(channel_id, context_id);
        self.amp_channel_participants
            .lock()
            .expect("amp channel participants mutex")
            .insert((context_id, channel_id), participants);
    }

    #[cfg(test)]
    /// Configure whether authoritative AMP channel state exists for a channel.
    pub fn set_amp_channel_state_exists(
        &self,
        context_id: ContextId,
        channel_id: ChannelId,
        exists: bool,
    ) {
        self.amp_channel_contexts
            .lock()
            .expect("amp channel contexts mutex")
            .insert(channel_id, context_id);
        self.amp_channel_states
            .lock()
            .expect("amp channel states mutex")
            .insert((context_id, channel_id), exists);
    }

    #[cfg(test)]
    /// Configure authoritative AMP channel state without populating channel ->
    /// context resolution. This is used to prove parity-critical flows do not
    /// re-derive context after authoritative context is already known.
    pub fn set_amp_channel_state_exists_without_resolution(
        &self,
        context_id: ContextId,
        channel_id: ChannelId,
        exists: bool,
    ) {
        self.amp_channel_states
            .lock()
            .expect("amp channel states mutex")
            .insert((context_id, channel_id), exists);
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
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

    async fn amp_create_channel_bootstrap(
        &self,
        _context: ContextId,
        _channel: ChannelId,
        _recipients: Vec<AuthorityId>,
    ) -> Result<ChannelBootstrapPackage, IntentError> {
        Err(IntentError::no_agent(
            "AMP bootstrap not available in offline mode",
        ))
    }

    async fn amp_channel_state_exists(
        &self,
        context: ContextId,
        channel: ChannelId,
    ) -> Result<bool, IntentError> {
        self.amp_channel_states
            .lock()
            .expect("amp channel states mutex")
            .get(&(context, channel))
            .copied()
            .ok_or_else(|| {
                IntentError::no_agent(format!(
                    "authoritative AMP state unavailable in offline mode for channel {channel} in context {context}"
                ))
            })
    }

    async fn amp_list_channel_participants(
        &self,
        context: ContextId,
        channel: ChannelId,
    ) -> Result<Vec<AuthorityId>, IntentError> {
        self.amp_channel_participants
            .lock()
            .expect("amp channel participants mutex")
            .get(&(context, channel))
            .cloned()
            .ok_or_else(|| {
                IntentError::no_agent(format!(
                    "authoritative AMP participants unavailable in offline mode for channel {channel} in context {context}"
                ))
            })
    }

    async fn moderation_status(
        &self,
        context_id: ContextId,
        channel_id: ChannelId,
        authority_id: AuthorityId,
        _current_time_ms: u64,
    ) -> Result<AuthoritativeModerationStatus, IntentError> {
        self.moderation_statuses
            .lock()
            .expect("moderation statuses mutex")
            .get(&(context_id, channel_id, authority_id))
            .copied()
            .ok_or_else(|| {
                IntentError::no_agent(format!(
                    "authoritative moderation status unavailable in offline mode for channel {channel_id} in context {context_id}"
                ))
            })
    }

    async fn resolve_amp_channel_context(
        &self,
        channel: ChannelId,
    ) -> Result<Option<ContextId>, IntentError> {
        self.amp_channel_contexts
            .lock()
            .expect("amp channel contexts mutex")
            .get(&channel)
            .copied()
            .map(Some)
            .ok_or_else(|| {
                IntentError::no_agent(format!(
                    "authoritative AMP context unavailable in offline mode for channel {channel}"
                ))
            })
    }

    async fn resolve_authoritative_channel_ids_by_name(
        &self,
        channel_name: &str,
    ) -> Result<Vec<ChannelId>, IntentError> {
        self.authoritative_channel_name_matches
            .lock()
            .expect("authoritative channel name matches mutex")
            .get(&channel_name.trim().to_ascii_lowercase())
            .cloned()
            .ok_or_else(|| {
                IntentError::no_agent(format!(
                    "authoritative channel-name lookup unavailable in offline mode for channel {channel_name}"
                ))
            })
    }

    async fn amp_repair_local_channel_membership(
        &self,
        _params: ChannelJoinParams,
    ) -> Result<(), IntentError> {
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

    async fn bump_channel_epoch(
        &self,
        _context: ContextId,
        _channel: ChannelId,
        _reason: String,
    ) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Channel epoch bump not available in offline mode",
        ))
    }

    async fn start_channel_invitation_monitor(
        &self,
        _invitation_ids: Vec<String>,
        _context: ContextId,
        _channel: ChannelId,
    ) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Channel invitation monitoring not available in offline mode",
        ))
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

    async fn try_get_sync_status(&self) -> Result<SyncStatus, IntentError> {
        Err(IntentError::no_agent(
            "Sync status not available in offline mode",
        ))
    }

    async fn try_get_sync_peers(&self) -> Result<Vec<DeviceId>, IntentError> {
        Err(IntentError::no_agent(
            "Sync peers not available in offline mode",
        ))
    }

    async fn trigger_sync(&self) -> Result<(), IntentError> {
        Err(IntentError::no_agent("Sync not available in offline mode"))
    }

    async fn process_ceremony_messages(&self) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Ceremony processing not available in offline mode",
        ))
    }

    async fn sync_with_peer(&self, _peer_id: &str) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Peer-targeted sync not available in offline mode",
        ))
    }

    async fn ensure_peer_channel(
        &self,
        _context: ContextId,
        _peer: AuthorityId,
    ) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Peer channel establishment not available in offline mode",
        ))
    }

    async fn try_get_discovered_peers(&self) -> Result<Vec<AuthorityId>, IntentError> {
        Err(IntentError::no_agent(
            "Discovered peers not available in offline mode",
        ))
    }

    async fn try_get_rendezvous_status(&self) -> Result<RendezvousStatus, IntentError> {
        Err(IntentError::no_agent(
            "Rendezvous status not available in offline mode",
        ))
    }

    async fn trigger_discovery(&self) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Discovery not available in offline mode",
        ))
    }

    async fn try_get_lan_peers(&self) -> Result<Vec<LanPeerInfo>, IntentError> {
        Err(IntentError::no_agent(
            "LAN peers not available in offline mode",
        ))
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
        _guardian_ids: &[AuthorityId],
    ) -> Result<(Epoch, Vec<Vec<u8>>, Vec<u8>), IntentError> {
        Err(IntentError::no_agent(
            "Key rotation not available in offline mode",
        ))
    }

    async fn commit_guardian_key_rotation(&self, _new_epoch: Epoch) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Key rotation not available in offline mode",
        ))
    }

    async fn rollback_guardian_key_rotation(
        &self,
        _failed_epoch: Epoch,
    ) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Key rotation not available in offline mode",
        ))
    }

    async fn initiate_guardian_ceremony(
        &self,
        _threshold_k: FrostThreshold,
        _total_n: u16,
        _guardian_ids: &[AuthorityId],
    ) -> Result<CeremonyId, IntentError> {
        Err(IntentError::no_agent(
            "Guardian ceremony not available in offline mode",
        ))
    }

    async fn initiate_device_threshold_ceremony(
        &self,
        _threshold_k: FrostThreshold,
        _total_n: u16,
        _device_ids: &[String],
    ) -> Result<CeremonyId, IntentError> {
        Err(IntentError::no_agent(
            "Device threshold ceremony not available in offline mode",
        ))
    }

    async fn initiate_device_enrollment_ceremony(
        &self,
        _nickname_suggestion: String,
        _invitee_authority_id: AuthorityId,
    ) -> Result<DeviceEnrollmentStart, IntentError> {
        Err(IntentError::no_agent(
            "Device enrollment not available in offline mode",
        ))
    }

    async fn initiate_device_removal_ceremony(
        &self,
        _device_id: String,
    ) -> Result<CeremonyId, IntentError> {
        Err(IntentError::no_agent(
            "Device removal not available in offline mode",
        ))
    }

    async fn get_ceremony_status(
        &self,
        _ceremony_id: &CeremonyId,
    ) -> Result<CeremonyStatus, IntentError> {
        Err(IntentError::no_agent(
            "Guardian ceremony not available in offline mode",
        ))
    }

    async fn get_key_rotation_ceremony_status(
        &self,
        _ceremony_id: &CeremonyId,
    ) -> Result<KeyRotationCeremonyStatus, IntentError> {
        Err(IntentError::no_agent(
            "Key rotation ceremonies not available in offline mode",
        ))
    }

    async fn cancel_key_rotation_ceremony(
        &self,
        _ceremony_id: &CeremonyId,
    ) -> Result<(), IntentError> {
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
        _home_id: String,
        _context_id: Option<ContextId>,
        _channel_name_hint: Option<String>,
        _bootstrap: Option<ChannelBootstrapPackage>,
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

    async fn try_list_pending_invitations(&self) -> Result<Vec<InvitationInfo>, IntentError> {
        self.pending_invitations
            .lock()
            .expect("pending invitations mutex")
            .as_ref()
            .map(|invitations| {
                invitations
                    .iter()
                    .filter(|invitation| invitation.status == InvitationBridgeStatus::Pending)
                    .cloned()
                    .collect()
            })
            .ok_or_else(|| {
                IntentError::no_agent("pending invitations unavailable in offline mode")
            })
    }

    async fn import_invitation(&self, _code: &str) -> Result<InvitationInfo, IntentError> {
        Err(IntentError::no_agent(
            "Invitation import not available in offline mode",
        ))
    }

    async fn try_get_invited_peer_ids(&self) -> Result<Vec<AuthorityId>, IntentError> {
        Err(IntentError::no_agent(
            "Invited peer ids not available in offline mode",
        ))
    }

    async fn try_get_settings(&self) -> Result<SettingsBridgeState, IntentError> {
        Err(IntentError::no_agent(
            "Settings not available in offline mode",
        ))
    }

    async fn has_account_config(&self) -> Result<bool, IntentError> {
        Ok(false)
    }

    async fn initialize_account(&self, _nickname_suggestion: &str) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Account initialization not available in offline mode",
        ))
    }

    async fn try_list_devices(&self) -> Result<Vec<BridgeDeviceInfo>, IntentError> {
        Err(IntentError::no_agent(
            "Devices not available in offline mode",
        ))
    }

    async fn try_list_authorities(&self) -> Result<Vec<BridgeAuthorityInfo>, IntentError> {
        Err(IntentError::no_agent(
            "Authorities not available in offline mode",
        ))
    }

    async fn set_nickname_suggestion(&self, _name: &str) -> Result<(), IntentError> {
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
        _ceremony_id: &CeremonyId,
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
        cfg_if::cfg_if! {
            if #[cfg(all(target_arch = "wasm32", feature = "wasm"))] {
                Ok(js_sys::Date::now() as u64)
            } else {
                let now = std::time::UNIX_EPOCH.elapsed().map_err(|err| {
                    IntentError::internal_error(format!("System clock error: {err}"))
                })?;
                Ok(now.as_millis() as u64)
            }
        }
    }

    async fn is_peer_online(&self, _peer: AuthorityId) -> bool {
        false
    }

    async fn sleep_ms(&self, ms: u64) {
        // Offline bridge yields without actual sleep since there's no runtime
        // event loop to advance.  The duration is intentionally ignored —
        // retry/backoff loops in offline mode execute at yield-speed so they
        // terminate quickly rather than blocking.
        //
        // WARNING: This means exponential backoff timing is meaningless in
        // offline mode.  Code that depends on real elapsed time for
        // correctness must not run against OfflineRuntimeBridge.
        let _ = ms;
        crate::workflows::runtime::cooperative_yield().await;
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
        assert!(
            !bridge
                .is_peer_online(AuthorityId::new_from_entropy([43u8; 32]))
                .await
        );
        assert!(bridge.try_get_sync_peers().await.is_err());
        assert!(bridge.try_get_discovered_peers().await.is_err());
        assert!(bridge.try_get_lan_peers().await.is_err());
        assert!(bridge.try_list_pending_invitations().await.is_err());
        let settings_error = bridge
            .try_get_settings()
            .await
            .expect_err("offline bridge settings must fail explicitly");
        assert!(settings_error.to_string().contains("No agent configured"));
        assert!(bridge.try_list_devices().await.is_err());
        assert!(bridge.try_list_authorities().await.is_err());
        let channel = ChannelId::from_bytes([44u8; 32]);
        let context = ContextId::new_from_entropy([45u8; 32]);
        let amp_context_error = bridge
            .resolve_amp_channel_context(channel)
            .await
            .expect_err("offline AMP context query must fail explicitly");
        assert!(amp_context_error.to_string().contains("offline mode"));
        let participants_error = bridge
            .amp_list_channel_participants(context, channel)
            .await
            .expect_err("offline AMP participants query must fail explicitly");
        assert!(participants_error.to_string().contains("offline mode"));
        let moderation_error = bridge
            .moderation_status(context, channel, authority, 1_000)
            .await
            .expect_err("offline moderation query must fail explicitly");
        assert!(moderation_error.to_string().contains("offline mode"));
    }

    #[tokio::test]
    async fn test_offline_bridge_explicit_authoritative_overrides_remain_usable() {
        let authority = AuthorityId::new_from_entropy([46u8; 32]);
        let peer = AuthorityId::new_from_entropy([47u8; 32]);
        let channel = ChannelId::from_bytes([48u8; 32]);
        let context = ContextId::new_from_entropy([49u8; 32]);
        let bridge = OfflineRuntimeBridge::new(authority);

        bridge.set_amp_channel_context(channel, context);
        bridge.set_amp_channel_participants(context, channel, Vec::new());
        bridge.set_moderation_status(
            context,
            channel,
            authority,
            AuthoritativeModerationStatus {
                roster_known: true,
                is_member: true,
                is_banned: false,
                is_muted: true,
            },
        );

        assert_eq!(
            bridge
                .resolve_amp_channel_context(channel)
                .await
                .expect("configured AMP context query should succeed"),
            Some(context)
        );
        assert!(
            bridge
                .amp_list_channel_participants(context, channel)
                .await
                .expect("configured AMP participants query should succeed")
                .is_empty()
        );
        let status = bridge
            .moderation_status(context, channel, authority, 1_000)
            .await
            .expect("configured moderation query should succeed");
        assert!(status.roster_known);
        assert!(status.is_member);
        assert!(status.is_muted);

        bridge.set_amp_channel_participants(context, channel, vec![peer]);
        assert_eq!(
            bridge
                .amp_list_channel_participants(context, channel)
                .await
                .expect("updated AMP participants query should succeed"),
            vec![peer]
        );
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

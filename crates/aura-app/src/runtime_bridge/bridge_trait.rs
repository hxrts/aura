//! RuntimeBridge trait and boxed trait alias.

use super::{
    AuthenticationStatus, AuthoritativeChannelBinding, AuthoritativeModerationStatus,
    BootstrapCandidateInfo, BridgeAuthorityInfo, BridgeDeviceInfo, CeremonyProcessingOutcome,
    CeremonyStatus, DeviceEnrollmentStart, DiscoveryTriggerOutcome, InvitationInfo,
    InvitationMutationOutcome, KeyRotationCeremonyStatus, RendezvousStatus, RuntimeStatus,
    SettingsBridgeState, SyncStatus,
};
use crate::core::IntentError;
use crate::ReactiveHandler;
use async_trait::async_trait;
use aura_core::effects::amp::{
    AmpCiphertext, ChannelBootstrapPackage, ChannelCloseParams, ChannelCreateParams,
    ChannelJoinParams, ChannelLeaveParams, ChannelSendParams,
};
use aura_core::threshold::{SigningContext, ThresholdConfig, ThresholdSignature};
use aura_core::tree::{AttestedOp, TreeOp};
use aura_core::types::identifiers::{AuthorityId, CeremonyId, ChannelId, ContextId};
use aura_core::types::{Epoch, FrostThreshold};
use aura_core::{DeviceId, OwnedShutdownToken, OwnedTaskSpawner};
use aura_journal::fact::{FactOptions, RelationalFact};
use std::sync::Arc;

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[allow(missing_docs)] // Trait method docs evolving with API
pub trait RuntimeBridge: Send + Sync {
    // =========================================================================
    // Runtime Identity & Lifecycle
    // =========================================================================

    /// Get the authority ID for this runtime
    fn authority_id(&self) -> AuthorityId;

    /// Get the shared reactive handler used for UI-facing signals.
    ///
    /// In production/demo runtimes, this handler is owned by the runtime and
    /// driven by the ReactiveScheduler. Frontends should subscribe/read from
    /// this handler rather than maintaining parallel state.
    fn reactive_handler(&self) -> ReactiveHandler;

    /// Runtime task spawner for background work.
    ///
    /// All runtime bridges must provide a portable spawner so `aura-app` can
    /// schedule background work without binding to tokio or optional owner
    /// fallbacks.
    fn task_spawner(&self) -> OwnedTaskSpawner;

    /// Runtime cancellation token for background work.
    fn cancellation_token(&self) -> OwnedShutdownToken {
        self.task_spawner().shutdown_token().clone()
    }

    /// Query the explicit runtime authentication status.
    async fn authentication_status(&self) -> Result<AuthenticationStatus, IntentError>;

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

    /// Retry channel-invitation acceptance notifications for an already
    /// accepted imported invitation once the receiver has established
    /// connectivity on the shared channel.
    async fn resend_channel_invitation_acceptance_notifications(
        &self,
        context: ContextId,
        channel: ChannelId,
    ) -> Result<(), IntentError> {
        let _ = (context, channel);
        Err(IntentError::no_agent(
            "Channel invitation acceptance resend not available in offline mode",
        ))
    }

    /// Resolve the authoritative checkpoint context for a channel from runtime-owned facts.
    async fn resolve_amp_channel_context(
        &self,
        channel: ChannelId,
    ) -> Result<Option<ContextId>, IntentError>;

    /// Identify already-materialized channel identifiers by normalized display
    /// name.
    ///
    /// Name lookup is an identification step only. Callers must not treat the
    /// resulting ids as a strong witness unless they also hold a runtime-owned
    /// bound context.
    async fn identify_materialized_channel_ids_by_name(
        &self,
        channel_name: &str,
    ) -> Result<Vec<ChannelId>, IntentError>;

    /// Identify already-materialized channel bindings by normalized display
    /// name.
    async fn identify_materialized_channel_bindings_by_name(
        &self,
        channel_name: &str,
    ) -> Result<Vec<AuthoritativeChannelBinding>, IntentError> {
        let mut bindings = Vec::new();
        for channel_id in self
            .identify_materialized_channel_ids_by_name(channel_name)
            .await?
        {
            if let Some(context_id) = self.resolve_amp_channel_context(channel_id).await? {
                bindings.push(AuthoritativeChannelBinding {
                    channel_id,
                    context_id,
                });
            }
        }
        Ok(bindings)
    }

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

    /// Trigger sync with peers.
    ///
    /// Implementations must fail explicitly when the sync service is absent or
    /// when no sync peers are available, rather than reporting a no-op as
    /// success.
    async fn trigger_sync(&self) -> Result<(), IntentError>;

    /// Process any pending ceremony envelopes/messages.
    ///
    /// This is required for flows like device enrollment where the invitee must
    /// ingest ceremony commit messages before signal-backed state can converge.
    async fn process_ceremony_messages(&self) -> Result<CeremonyProcessingOutcome, IntentError>;

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
    async fn trigger_discovery(&self) -> Result<DiscoveryTriggerOutcome, IntentError>;

    // =========================================================================
    // Bootstrap Discovery
    // =========================================================================

    /// Get list of bootstrap candidates, distinguishing runtime
    /// unavailability from a real empty candidate set.
    async fn try_get_bootstrap_candidates(
        &self,
    ) -> Result<Vec<BootstrapCandidateInfo>, IntentError>;

    /// Refresh this runtime's bootstrap-candidate self-registration.
    ///
    /// Browser runtimes use this to advertise themselves to a broker-backed
    /// startup discovery plane after the broker becomes reachable.
    async fn refresh_bootstrap_candidate_registration(&self) -> Result<(), IntentError>;

    /// Send an invitation to a bootstrap candidate.
    ///
    /// Sends an invite code directly to a discovered bootstrap candidate.
    /// This bypasses manual code sharing when the candidate can be reached
    /// through a supported bootstrap discovery path.
    async fn send_bootstrap_invitation(
        &self,
        peer: &BootstrapCandidateInfo,
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
    async fn accept_invitation(
        &self,
        invitation_id: &str,
    ) -> Result<InvitationMutationOutcome, IntentError>;

    /// Decline a received invitation
    async fn decline_invitation(
        &self,
        invitation_id: &str,
    ) -> Result<InvitationMutationOutcome, IntentError>;

    /// Cancel a sent invitation
    async fn cancel_invitation(
        &self,
        invitation_id: &str,
    ) -> Result<InvitationMutationOutcome, IntentError>;

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
            authentication: self.authentication_status().await?,
        })
    }
}

/// Type alias for boxed runtime bridge
pub type BoxedRuntimeBridge = Arc<dyn RuntimeBridge>;

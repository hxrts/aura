//! Offline runtime bridge implementation for demos and tests.

use super::types::{
    AmpChannelContexts, AmpChannelParticipants, AmpChannelStates, MaterializedChannelNameMatches,
    ModerationStatuses, PendingInvitationsState,
};
#[cfg(test)]
use super::types::{OfflineAcceptInvitationResult, OfflineProcessCeremonyResult};
use super::{
    AuthenticationStatus, AuthoritativeModerationStatus, BootstrapCandidateInfo,
    BridgeAuthorityInfo, BridgeDeviceInfo, CeremonyProcessingOutcome, CeremonyStatus,
    DeviceEnrollmentStart, DiscoveryTriggerOutcome, InvitationBridgeStatus, InvitationInfo,
    InvitationMutationOutcome, KeyRotationCeremonyStatus, RendezvousStatus, RuntimeBridge,
    SettingsBridgeState, SyncStatus,
};
use crate::core::IntentError;
use crate::ReactiveHandler;
use async_lock::Mutex;
use async_trait::async_trait;
use aura_core::effects::amp::{
    AmpCiphertext, ChannelBootstrapPackage, ChannelCloseParams, ChannelCreateParams,
    ChannelJoinParams, ChannelLeaveParams, ChannelSendParams,
};
use aura_core::effects::task::{CancellationToken, NeverCancel, TaskSpawner};
use aura_core::threshold::{SigningContext, ThresholdConfig, ThresholdSignature};
use aura_core::tree::{AttestedOp, TreeOp};
use aura_core::types::identifiers::{AuthorityId, CeremonyId, ChannelId, ContextId};
use aura_core::types::{Epoch, FrostThreshold};
use aura_core::{DeviceId, OwnedShutdownToken, OwnedTaskSpawner};
use aura_journal::fact::RelationalFact;
use std::collections::HashMap;
use std::sync::Arc;

pub struct OfflineRuntimeBridge {
    authority_id: AuthorityId,
    reactive: ReactiveHandler,
    task_spawner: OwnedTaskSpawner,
    pending_invitations: PendingInvitationsState,
    amp_channel_contexts: AmpChannelContexts,
    materialized_channel_name_matches: MaterializedChannelNameMatches,
    amp_channel_states: AmpChannelStates,
    amp_channel_participants: AmpChannelParticipants,
    moderation_statuses: ModerationStatuses,
    #[cfg(test)]
    accept_invitation_result: OfflineAcceptInvitationResult,
    #[cfg(test)]
    process_ceremony_result: OfflineProcessCeremonyResult,
}

impl OfflineRuntimeBridge {
    /// Create a new offline runtime bridge
    pub fn new(authority_id: AuthorityId) -> Self {
        Self {
            authority_id,
            reactive: ReactiveHandler::new(),
            task_spawner: OwnedTaskSpawner::new(
                Arc::new(OfflineRuntimeTaskSpawner),
                OwnedShutdownToken::detached(),
            ),
            pending_invitations: Arc::new(Mutex::new(None)),
            amp_channel_contexts: Arc::new(Mutex::new(HashMap::new())),
            materialized_channel_name_matches: Arc::new(Mutex::new(HashMap::new())),
            amp_channel_states: Arc::new(Mutex::new(HashMap::new())),
            amp_channel_participants: Arc::new(Mutex::new(HashMap::new())),
            moderation_statuses: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(test)]
            accept_invitation_result: Arc::new(Mutex::new(None)),
            #[cfg(test)]
            process_ceremony_result: Arc::new(Mutex::new(None)),
        }
    }

    #[cfg(test)]
    /// Configure the pending invitation snapshot returned by the offline bridge.
    pub fn set_pending_invitations(&self, invitations: Vec<InvitationInfo>) {
        let mut guard = self
            .pending_invitations
            .try_lock()
            .unwrap_or_else(|| panic!("pending invitations mutex already locked"));
        *guard = Some(invitations);
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
            .try_lock()
            .unwrap_or_else(|| panic!("moderation statuses mutex already locked"))
            .insert((context_id, channel_id, authority_id), status);
    }

    #[cfg(test)]
    /// Configure authoritative AMP context for a channel.
    pub fn set_amp_channel_context(&self, channel_id: ChannelId, context_id: ContextId) {
        self.amp_channel_contexts
            .try_lock()
            .unwrap_or_else(|| panic!("amp channel contexts mutex already locked"))
            .insert(channel_id, context_id);
    }

    #[cfg(test)]
    /// Configure materialized channel-name lookup results.
    pub fn set_materialized_channel_name_matches(
        &self,
        channel_name: impl Into<String>,
        channel_ids: Vec<ChannelId>,
    ) {
        self.materialized_channel_name_matches
            .try_lock()
            .unwrap_or_else(|| panic!("materialized channel name matches mutex already locked"))
            .insert(channel_name.into().trim().to_ascii_lowercase(), channel_ids);
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
            .try_lock()
            .unwrap_or_else(|| panic!("amp channel contexts mutex already locked"))
            .insert(channel_id, context_id);
        self.amp_channel_participants
            .try_lock()
            .unwrap_or_else(|| panic!("amp channel participants mutex already locked"))
            .insert((context_id, channel_id), participants);
    }

    #[cfg(test)]
    /// Configure authoritative AMP participants without populating channel ->
    /// context resolution. This is used to prove parity-critical flows do not
    /// re-derive context after authoritative context is already known.
    pub fn set_amp_channel_participants_without_resolution(
        &self,
        context_id: ContextId,
        channel_id: ChannelId,
        participants: Vec<AuthorityId>,
    ) {
        self.amp_channel_participants
            .try_lock()
            .unwrap_or_else(|| panic!("amp channel participants mutex already locked"))
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
            .try_lock()
            .unwrap_or_else(|| panic!("amp channel contexts mutex already locked"))
            .insert(channel_id, context_id);
        self.amp_channel_states
            .try_lock()
            .unwrap_or_else(|| panic!("amp channel states mutex already locked"))
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
            .try_lock()
            .unwrap_or_else(|| panic!("amp channel states mutex already locked"))
            .insert((context_id, channel_id), exists);
    }

    #[cfg(test)]
    /// Configure the result returned by `accept_invitation`.
    pub fn set_accept_invitation_result(
        &self,
        result: Result<InvitationMutationOutcome, IntentError>,
    ) {
        let mut guard = self
            .accept_invitation_result
            .try_lock()
            .unwrap_or_else(|| panic!("accept invitation result mutex already locked"));
        *guard = Some(result);
    }

    #[cfg(test)]
    /// Configure the result returned by `process_ceremony_messages`.
    pub fn set_process_ceremony_result(
        &self,
        result: Result<CeremonyProcessingOutcome, IntentError>,
    ) {
        let mut guard = self
            .process_ceremony_result
            .try_lock()
            .unwrap_or_else(|| panic!("process ceremony result mutex already locked"));
        *guard = Some(result);
    }
}

#[derive(Debug)]
struct OfflineRuntimeTaskSpawner;

impl TaskSpawner for OfflineRuntimeTaskSpawner {
    fn spawn(&self, fut: futures::future::BoxFuture<'static, ()>) {
        drop(fut);
    }

    fn spawn_cancellable(
        &self,
        fut: futures::future::BoxFuture<'static, ()>,
        _token: Arc<dyn CancellationToken>,
    ) {
        drop(fut);
    }

    fn spawn_local(&self, fut: futures::future::LocalBoxFuture<'static, ()>) {
        drop(fut);
    }

    fn spawn_local_cancellable(
        &self,
        fut: futures::future::LocalBoxFuture<'static, ()>,
        _token: Arc<dyn CancellationToken>,
    ) {
        drop(fut);
    }

    fn cancellation_token(&self) -> Arc<dyn CancellationToken> {
        Arc::new(NeverCancel)
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

    fn task_spawner(&self) -> OwnedTaskSpawner {
        self.task_spawner.clone()
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
            .await
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
            .await
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
            .await
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
            .await
            .get(&channel)
            .copied()
            .map(Some)
            .ok_or_else(|| {
                IntentError::no_agent(format!(
                    "authoritative AMP context unavailable in offline mode for channel {channel}"
                ))
            })
    }

    async fn identify_materialized_channel_ids_by_name(
        &self,
        channel_name: &str,
    ) -> Result<Vec<ChannelId>, IntentError> {
        self.materialized_channel_name_matches
            .lock()
            .await
            .get(&channel_name.trim().to_ascii_lowercase())
            .cloned()
            .ok_or_else(|| {
                IntentError::no_agent(format!(
                    "materialized channel-name lookup unavailable in offline mode for channel {channel_name}"
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

    async fn process_ceremony_messages(&self) -> Result<CeremonyProcessingOutcome, IntentError> {
        #[cfg(test)]
        if let Some(result) = self.process_ceremony_result.lock().await.clone() {
            return result;
        }
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

    async fn trigger_discovery(&self) -> Result<DiscoveryTriggerOutcome, IntentError> {
        Err(IntentError::no_agent(
            "Discovery not available in offline mode",
        ))
    }

    async fn try_get_bootstrap_candidates(
        &self,
    ) -> Result<Vec<BootstrapCandidateInfo>, IntentError> {
        Err(IntentError::no_agent(
            "Bootstrap candidates not available in offline mode",
        ))
    }

    async fn refresh_bootstrap_candidate_registration(&self) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Bootstrap registration not available in offline mode",
        ))
    }

    async fn send_bootstrap_invitation(
        &self,
        _peer: &BootstrapCandidateInfo,
        _invitation_code: &str,
    ) -> Result<(), IntentError> {
        Err(IntentError::no_agent(
            "Bootstrap invitation not available in offline mode",
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

    async fn accept_invitation(
        &self,
        _invitation_id: &str,
    ) -> Result<InvitationMutationOutcome, IntentError> {
        #[cfg(test)]
        if let Some(result) = self.accept_invitation_result.lock().await.clone() {
            return result;
        }
        Err(IntentError::no_agent(
            "Invitation acceptance not available in offline mode",
        ))
    }

    async fn decline_invitation(
        &self,
        _invitation_id: &str,
    ) -> Result<InvitationMutationOutcome, IntentError> {
        Err(IntentError::no_agent(
            "Invitation decline not available in offline mode",
        ))
    }

    async fn cancel_invitation(
        &self,
        _invitation_id: &str,
    ) -> Result<InvitationMutationOutcome, IntentError> {
        Err(IntentError::no_agent(
            "Invitation cancellation not available in offline mode",
        ))
    }

    async fn try_list_pending_invitations(&self) -> Result<Vec<InvitationInfo>, IntentError> {
        self.pending_invitations
            .lock()
            .await
            .as_ref()
            .map(|invitations: &Vec<InvitationInfo>| {
                invitations
                    .iter()
                    .filter(|invitation| invitation.status == InvitationBridgeStatus::Pending)
                    .cloned()
                    .collect()
            })
            .ok_or_else(|| IntentError::no_agent("pending invitations unavailable in offline mode"))
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

    async fn authentication_status(&self) -> Result<AuthenticationStatus, IntentError> {
        Ok(AuthenticationStatus::Unauthenticated)
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

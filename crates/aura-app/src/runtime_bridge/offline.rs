//! Offline runtime bridge implementation for demos and tests.

use super::types::{
    AmpChannelContexts, AmpChannelParticipants, AmpChannelStates, MaterializedChannelNameMatches,
    ModerationStatuses, PendingInvitationsState,
};
#[cfg(test)]
use super::types::{OfflineAcceptInvitationResult, OfflineProcessCeremonyResult};
use super::RuntimeBridgeError;
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
use aura_chat::view::CanonicalChannelCreation;
use aura_chat::{ChatDelta, ChatFact, ChatViewReducer, CHAT_FACT_TYPE_ID};
use aura_composition::{downcast_delta_owned, ViewDeltaReducer};
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
use aura_journal::DomainFact;
use std::collections::HashMap;
use std::sync::Arc;

#[cfg(test)]
type OfflineChannelStateAnswers = Arc<
    Mutex<
        HashMap<
            (ContextId, ChannelId),
            std::collections::VecDeque<Result<bool, RuntimeBridgeError>>,
        >,
    >,
>;

#[cfg(test)]
type OfflineAmpCreateResults =
    Arc<Mutex<std::collections::VecDeque<Result<ChannelId, super::RuntimeBridgeError>>>>;
#[cfg(test)]
type OfflineAmpJoinResults =
    Arc<Mutex<std::collections::VecDeque<Result<(), super::RuntimeBridgeError>>>>;

#[cfg(test)]
type OfflineClockAnswers = Arc<Mutex<std::collections::VecDeque<Result<u64, RuntimeBridgeError>>>>;
#[cfg(test)]
type OfflineSleepAnswers = Arc<
    Mutex<std::collections::VecDeque<Result<aura_core::time::PhysicalTime, RuntimeBridgeError>>>,
>;
#[cfg(test)]
type OfflineCallAnswers<T, E = IntentError> =
    Arc<Mutex<std::collections::VecDeque<futures::future::BoxFuture<'static, Result<T, E>>>>>;

#[cfg(test)]
struct OfflinePhysicalTimeFixture {
    provider: Arc<dyn aura_core::effects::PhysicalTimeEffects>,
    clock_answers: OfflineClockAnswers,
    deadline_answers: OfflineSleepAnswers,
    sleep_requests: Arc<Mutex<std::collections::VecDeque<u64>>>,
}

#[cfg(test)]
#[async_trait::async_trait]
impl aura_core::effects::PhysicalTimeEffects for OfflinePhysicalTimeFixture {
    async fn physical_time(
        &self,
    ) -> Result<aura_core::time::PhysicalTime, aura_core::effects::TimeError> {
        if let Some(answer) = self.clock_answers.lock().await.pop_front() {
            return answer
                .map(aura_core::time::PhysicalTime::exact)
                .map_err(|source| aura_core::effects::TimeError::ProviderFailure {
                    operation: aura_core::effects::time::TimeProviderOperation::ReadPhysicalClock,
                    source: Some(Arc::new(source)),
                });
        }
        self.provider.physical_time().await
    }
    async fn sleep_ms(&self, ms: u64) -> Result<(), aura_core::effects::TimeError> {
        self.sleep_requests.lock().await.push_back(ms);
        self.provider.sleep_ms(ms).await
    }
    async fn wait_until_physical_deadline(
        &self,
        deadline: aura_core::types::window::WindowPosition<
            aura_core::types::window::PhysicalMillis,
        >,
    ) -> Result<aura_core::time::PhysicalTime, aura_core::effects::TimeError> {
        if let Some(answer) = self.deadline_answers.lock().await.pop_front() {
            return answer.map_err(|source| aura_core::effects::TimeError::ProviderFailure {
                operation: aura_core::effects::time::TimeProviderOperation::WaitTimer,
                source: Some(Arc::new(source)),
            });
        }
        self.provider.wait_until_physical_deadline(deadline).await
    }
}

pub struct OfflineRuntimeBridge {
    physical_time: Arc<dyn aura_core::effects::PhysicalTimeEffects>,
    #[cfg(test)]
    clock_answers: OfflineClockAnswers,
    #[cfg(test)]
    deadline_answers: OfflineSleepAnswers,
    #[cfg(test)]
    sync_answers: OfflineCallAnswers<()>,
    #[cfg(test)]
    import_answers: OfflineCallAnswers<InvitationInfo>,
    #[cfg(test)]
    sync_status_answers: OfflineCallAnswers<SyncStatus>,
    #[cfg(test)]
    accept_invitation_calls: Arc<Mutex<usize>>,
    #[cfg(test)]
    sleep_requests: Arc<Mutex<std::collections::VecDeque<u64>>>,
    #[cfg(test)]
    guardian_outcome_answers: OfflineCallAnswers<Option<super::CeremonyTerminalOutcome>>,
    #[cfg(test)]
    guardian_outcome_calls: Arc<Mutex<usize>>,
    #[cfg(test)]
    amp_context_resolve_calls: Arc<Mutex<usize>>,
    #[cfg(test)]
    accept_answers: OfflineCallAnswers<InvitationMutationOutcome, RuntimeBridgeError>,
    #[cfg(test)]
    background_refresh_failure: Arc<Mutex<Option<RuntimeBridgeError>>>,
    authority_id: AuthorityId,
    reactive: ReactiveHandler,
    task_spawner: OwnedTaskSpawner,
    pending_invitations: PendingInvitationsState,
    amp_channel_contexts: AmpChannelContexts,
    canonical_channel_creations: Arc<Mutex<HashMap<(ContextId, ChannelId), ChatFact>>>,
    materialized_channel_name_matches: MaterializedChannelNameMatches,
    amp_channel_states: AmpChannelStates,
    #[cfg(test)]
    amp_channel_state_answers: OfflineChannelStateAnswers,
    #[cfg(test)]
    amp_join_calls: Arc<Mutex<usize>>,
    #[cfg(test)]
    amp_create_calls: Arc<Mutex<usize>>,
    #[cfg(test)]
    amp_create_results: OfflineAmpCreateResults,
    #[cfg(test)]
    amp_join_results: OfflineAmpJoinResults,
    amp_channel_participants: AmpChannelParticipants,
    moderation_statuses: ModerationStatuses,
    #[cfg(test)]
    accept_invitation_result: OfflineAcceptInvitationResult,
    #[cfg(test)]
    process_ceremony_result: OfflineProcessCeremonyResult,
    #[cfg(test)]
    enrollment_outcomes: Arc<Mutex<HashMap<CeremonyId, Option<super::CeremonyTerminalOutcome>>>>,
    #[cfg(test)]
    recorded_relational_facts: Arc<Mutex<Option<Vec<RelationalFact>>>>,
}

impl OfflineRuntimeBridge {
    #[cfg(test)]
    pub(crate) fn queue_guardian_outcome_answers(
        &self,
        answers: Vec<
            futures::future::BoxFuture<
                'static,
                Result<Option<super::CeremonyTerminalOutcome>, IntentError>,
            >,
        >,
    ) {
        self.guardian_outcome_answers
            .try_lock()
            .expect("guardian fixture is idle")
            .extend(answers);
    }
    #[cfg(test)]
    pub(crate) fn guardian_outcome_call_count(&self) -> usize {
        *self
            .guardian_outcome_calls
            .try_lock()
            .expect("guardian fixture is idle")
    }
    #[cfg(test)]
    pub(crate) fn queue_accept_answers(
        &self,
        answers: Vec<
            futures::future::BoxFuture<
                'static,
                Result<InvitationMutationOutcome, RuntimeBridgeError>,
            >,
        >,
    ) {
        self.accept_answers
            .try_lock()
            .expect("accept fixture is idle")
            .extend(answers);
    }
    #[cfg(test)]
    pub(crate) fn queue_import_answers(
        &self,
        answers: Vec<futures::future::BoxFuture<'static, Result<InvitationInfo, IntentError>>>,
    ) {
        self.import_answers
            .try_lock()
            .expect("import fixture is idle")
            .extend(answers);
    }
    #[cfg(test)]
    pub(crate) fn queue_sync_status_answers(
        &self,
        answers: Vec<futures::future::BoxFuture<'static, Result<SyncStatus, IntentError>>>,
    ) {
        self.sync_status_answers
            .try_lock()
            .expect("sync status fixture is idle")
            .extend(answers);
    }
    #[cfg(test)]
    pub(crate) fn amp_context_resolve_call_count(&self) -> usize {
        *self
            .amp_context_resolve_calls
            .try_lock()
            .expect("context resolver fixture is idle")
    }
    #[cfg(test)]
    pub(crate) fn take_sleep_request(&self) -> Option<u64> {
        self.sleep_requests
            .try_lock()
            .expect("sleep fixture is idle")
            .pop_front()
    }
    #[cfg(test)]
    pub(crate) fn accept_invitation_call_count(&self) -> usize {
        *self
            .accept_invitation_calls
            .try_lock()
            .expect("accept fixture is idle")
    }
    #[cfg(test)]
    pub(crate) fn use_time_provider(
        &mut self,
        provider: Arc<dyn aura_core::effects::PhysicalTimeEffects>,
    ) {
        self.physical_time = Arc::new(OfflinePhysicalTimeFixture {
            provider,
            clock_answers: self.clock_answers.clone(),
            deadline_answers: self.deadline_answers.clone(),
            sleep_requests: self.sleep_requests.clone(),
        });
    }
    #[cfg(test)]
    pub(crate) fn queue_sync_answers(
        &self,
        answers: Vec<futures::future::BoxFuture<'static, Result<(), IntentError>>>,
    ) {
        self.sync_answers
            .try_lock()
            .expect("sync fixture is idle")
            .extend(answers);
    }
    #[cfg(test)]
    pub(crate) fn queue_clock_answers(&self, answers: Vec<Result<u64, RuntimeBridgeError>>) {
        *self
            .clock_answers
            .try_lock()
            .expect("clock fixture not concurrently borrowed") = answers.into();
    }
    #[cfg(test)]
    pub(crate) fn queue_deadline_answers(
        &self,
        answers: Vec<Result<aura_core::time::PhysicalTime, RuntimeBridgeError>>,
    ) {
        *self
            .deadline_answers
            .try_lock()
            .expect("sleep fixture not concurrently borrowed") = answers.into();
    }

    #[cfg(all(test, feature = "signals"))]
    pub(crate) fn queue_amp_create_results(
        &self,
        results: Vec<Result<ChannelId, super::RuntimeBridgeError>>,
    ) {
        self.amp_create_results
            .try_lock()
            .expect("creation fixture is idle")
            .extend(results);
    }

    #[cfg(all(test, feature = "signals"))]
    pub(crate) fn queue_amp_join_results(
        &self,
        results: Vec<Result<(), super::RuntimeBridgeError>>,
    ) {
        self.amp_join_results
            .try_lock()
            .expect("join fixture is idle")
            .extend(results);
    }

    #[cfg(all(test, feature = "signals"))]
    pub(crate) fn amp_create_call_count(&self) -> usize {
        *self
            .amp_create_calls
            .try_lock()
            .expect("creation fixture is idle")
    }

    /// Queue exact query outcomes to exercise a later required read failing
    /// after an earlier read completed. Unqueued reads keep normal bridge behavior.
    #[cfg(all(test, feature = "signals"))]
    pub(crate) fn queue_amp_channel_state_answers(
        &self,
        context: ContextId,
        channel: ChannelId,
        answers: Vec<Result<bool, RuntimeBridgeError>>,
    ) {
        self.amp_channel_state_answers
            .try_lock()
            .expect("channel state answer fixture is not concurrently borrowed")
            .insert((context, channel), answers.into());
    }

    #[cfg(all(test, feature = "signals"))]
    pub(crate) fn remaining_amp_channel_state_answers(
        &self,
        context: ContextId,
        channel: ChannelId,
    ) -> usize {
        self.amp_channel_state_answers
            .try_lock()
            .expect("channel state answer fixture is not concurrently borrowed")
            .get(&(context, channel))
            .map_or(0, std::collections::VecDeque::len)
    }

    #[cfg(all(test, feature = "signals"))]
    pub(crate) fn amp_join_call_count(&self) -> usize {
        *self
            .amp_join_calls
            .try_lock()
            .expect("join call fixture is not concurrently borrowed")
    }

    #[cfg(test)]
    pub(crate) fn use_test_task_spawner(&mut self, spawner: OwnedTaskSpawner) {
        self.task_spawner = spawner;
    }

    #[cfg(test)]
    pub(crate) async fn fail_next_background_refresh(&self, source: RuntimeBridgeError) {
        *self.background_refresh_failure.lock().await = Some(source);
    }

    /// Create a new offline runtime bridge
    pub fn new(authority_id: AuthorityId) -> Self {
        let bridge = Self {
            physical_time: Arc::new(aura_effects::time::PhysicalTimeHandler::new()),
            #[cfg(test)]
            clock_answers: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            #[cfg(test)]
            deadline_answers: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            #[cfg(test)]
            sync_answers: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            #[cfg(test)]
            import_answers: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            #[cfg(test)]
            sync_status_answers: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            #[cfg(test)]
            accept_invitation_calls: Arc::new(Mutex::new(0)),
            #[cfg(test)]
            sleep_requests: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            #[cfg(test)]
            guardian_outcome_answers: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            #[cfg(test)]
            guardian_outcome_calls: Arc::new(Mutex::new(0)),
            #[cfg(test)]
            amp_context_resolve_calls: Arc::new(Mutex::new(0)),
            #[cfg(test)]
            accept_answers: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            #[cfg(test)]
            background_refresh_failure: Arc::new(Mutex::new(None)),
            authority_id,
            reactive: ReactiveHandler::new(),
            task_spawner: OwnedTaskSpawner::new(
                Arc::new(OfflineRuntimeTaskSpawner),
                OwnedShutdownToken::detached(),
            ),
            pending_invitations: Arc::new(Mutex::new(None)),
            amp_channel_contexts: Arc::new(Mutex::new(HashMap::new())),
            canonical_channel_creations: Arc::new(Mutex::new(HashMap::new())),
            materialized_channel_name_matches: Arc::new(Mutex::new(HashMap::new())),
            amp_channel_states: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(test)]
            amp_channel_state_answers: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(test)]
            amp_join_calls: Arc::new(Mutex::new(0)),
            #[cfg(test)]
            amp_create_calls: Arc::new(Mutex::new(0)),
            #[cfg(test)]
            amp_create_results: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            #[cfg(test)]
            amp_join_results: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            amp_channel_participants: Arc::new(Mutex::new(HashMap::new())),
            moderation_statuses: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(test)]
            accept_invitation_result: Arc::new(Mutex::new(None)),
            #[cfg(test)]
            process_ceremony_result: Arc::new(Mutex::new(None)),
            #[cfg(test)]
            enrollment_outcomes: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(test)]
            recorded_relational_facts: Arc::new(Mutex::new(None)),
        };
        #[cfg(test)]
        {
            let mut bridge = bridge;
            let provider = bridge.physical_time.clone();
            bridge.use_time_provider(provider);
            bridge
        }
        #[cfg(not(test))]
        bridge
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
    /// Seed a committed channel creation fact for offline join tests.
    pub fn set_canonical_channel_created_fact(&self, fact: ChatFact) {
        let ChatFact::ChannelCreated {
            context_id,
            channel_id,
            ..
        } = fact
        else {
            panic!("offline canonical channel creation requires ChannelCreated");
        };
        self.canonical_channel_creations
            .try_lock()
            .unwrap_or_else(|| panic!("canonical channel creations mutex already locked"))
            .insert((context_id, channel_id), fact);
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
    /// Accept relational fact commits and record them instead of failing.
    pub fn record_relational_facts(&self) {
        *self
            .recorded_relational_facts
            .try_lock()
            .unwrap_or_else(|| panic!("recorded relational facts mutex already locked")) =
            Some(Vec::new());
    }

    #[cfg(test)]
    /// Relational facts committed since recording was enabled.
    pub fn recorded_relational_facts(&self) -> Vec<RelationalFact> {
        self.recorded_relational_facts
            .try_lock()
            .unwrap_or_else(|| panic!("recorded relational facts mutex already locked"))
            .clone()
            .unwrap_or_default()
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
        result: Result<InvitationMutationOutcome, super::RuntimeBridgeError>,
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

    #[cfg(test)]
    /// Retain one runtime-owned enrollment result across app hook attachments.
    pub fn set_enrollment_outcome(
        &self,
        ceremony_id: CeremonyId,
        outcome: Option<super::CeremonyTerminalOutcome>,
    ) {
        self.enrollment_outcomes
            .try_lock()
            .unwrap_or_else(|| panic!("enrollment outcomes mutex already locked"))
            .insert(ceremony_id, outcome);
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

    async fn commit_relational_facts(&self, facts: &[RelationalFact]) -> Result<(), IntentError> {
        #[cfg(test)]
        if let Some(recorded) = self.recorded_relational_facts.lock().await.as_mut() {
            recorded.extend_from_slice(facts);
            return Ok(());
        }
        let _ = facts;
        Err(IntentError::no_agent(
            "Relational fact commit not available in offline mode",
        ))
    }

    async fn causal_stamp(
        &self,
        key: crate::runtime_bridge::CausalStampKey,
    ) -> Result<aura_core::time::CausalMetadata, IntentError> {
        // Recording test bridges stamp contact facts as one fixed device
        // whose clock follows the contact facts it has recorded.
        #[cfg(test)]
        if let (Some(recorded), crate::runtime_bridge::CausalStampKey::Contact(contact_key)) =
            (self.recorded_relational_facts.lock().await.as_ref(), key)
        {
            let observed: Vec<_> = recorded
                .iter()
                .filter_map(|fact| match fact {
                    RelationalFact::Generic { envelope, .. } => {
                        aura_relational::ContactFact::from_envelope(envelope)
                            .map(aura_relational::TaggedContactFact::new)
                    }
                    _ => None,
                })
                .collect();
            return Ok(aura_relational::contacts::test_support::causal(
                0x0f,
                contact_key,
                &observed,
            ));
        }
        let _ = key;
        Err(IntentError::no_agent(
            "Causally stamped facts cannot be authored in offline mode",
        ))
    }

    async fn amp_create_channel(
        &self,
        _params: ChannelCreateParams,
    ) -> Result<ChannelId, crate::runtime_bridge::RuntimeBridgeError> {
        #[cfg(test)]
        {
            *self.amp_create_calls.lock().await += 1;
            if let Some(result) = self.amp_create_results.lock().await.pop_front() {
                return result;
            }
        }
        Err(IntentError::no_agent("AMP not available in offline mode").into())
    }

    async fn amp_create_channel_bootstrap(
        &self,
        _context: ContextId,
        _channel: ChannelId,
        _recipients: Vec<AuthorityId>,
    ) -> Result<Option<ChannelBootstrapPackage>, RuntimeBridgeError> {
        Err(IntentError::no_agent("AMP bootstrap not available in offline mode").into())
    }

    async fn amp_channel_state_exists(
        &self,
        context: ContextId,
        channel: ChannelId,
    ) -> Result<bool, RuntimeBridgeError> {
        #[cfg(test)]
        if let Some(answer) = self
            .amp_channel_state_answers
            .lock()
            .await
            .get_mut(&(context, channel))
            .and_then(std::collections::VecDeque::pop_front)
        {
            return answer;
        }
        self.amp_channel_states
            .lock()
            .await
            .get(&(context, channel))
            .copied()
            .ok_or_else(|| {
                let error = IntentError::no_agent(format!(
                    "authoritative AMP state unavailable in offline mode for channel {channel} in context {context}"
                ));
                RuntimeBridgeError::with_source(error.clone(), error)
            })
    }

    async fn amp_list_channel_participants(
        &self,
        context: ContextId,
        channel: ChannelId,
    ) -> Result<Vec<AuthorityId>, RuntimeBridgeError> {
        self.amp_channel_participants
            .lock()
            .await
            .get(&(context, channel))
            .cloned()
            .ok_or_else(|| {
                IntentError::no_agent(format!(
                    "authoritative AMP participants unavailable in offline mode for channel {channel} in context {context}"
                )).into()
            })
    }

    async fn amp_channel_transition_diagnostics(
        &self,
        context: ContextId,
        channel: ChannelId,
    ) -> Result<Option<crate::ui_contract::AmpChannelTransitionSnapshot>, RuntimeBridgeError> {
        let _ = (context, channel);
        Ok(None)
    }

    async fn moderation_status(
        &self,
        context_id: ContextId,
        channel_id: ChannelId,
        authority_id: AuthorityId,
        _current_time_ms: u64,
    ) -> Result<AuthoritativeModerationStatus, RuntimeBridgeError> {
        self.moderation_statuses
            .lock()
            .await
            .get(&(context_id, channel_id, authority_id))
            .copied()
            .ok_or_else(|| {
                IntentError::no_agent(format!(
                    "authoritative moderation status unavailable in offline mode for channel {channel_id} in context {context_id}"
                ))
            }).map_err(RuntimeBridgeError::from)
    }

    async fn resolve_amp_channel_context(
        &self,
        channel: ChannelId,
    ) -> Result<Option<ContextId>, RuntimeBridgeError> {
        #[cfg(test)]
        {
            *self.amp_context_resolve_calls.lock().await += 1;
        }
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
                .into()
            })
    }

    async fn canonical_channel_creation(
        &self,
        binding: super::AuthoritativeChannelBinding,
    ) -> Result<Option<CanonicalChannelCreation>, IntentError> {
        let fact = self
            .canonical_channel_creations
            .lock()
            .await
            .get(&(binding.context_id, binding.channel_id))
            .cloned();
        Ok(fact.and_then(|fact| {
            ChatViewReducer
                .reduce_fact(CHAT_FACT_TYPE_ID, &fact.to_bytes(), None)
                .into_iter()
                .filter_map(downcast_delta_owned::<ChatDelta>)
                .find_map(|delta| match delta {
                    ChatDelta::ChannelAdded(creation) => Some(creation),
                    _ => None,
                })
        }))
    }

    async fn identify_materialized_channel_ids_by_name(
        &self,
        channel_name: &str,
    ) -> Result<Vec<ChannelId>, RuntimeBridgeError> {
        self.materialized_channel_name_matches
            .lock()
            .await
            .get(&channel_name.trim().to_ascii_lowercase())
            .cloned()
            .ok_or_else(|| {
                IntentError::no_agent(format!(
                    "materialized channel-name lookup unavailable in offline mode for channel {channel_name}"
                )).into()
            })
    }

    async fn amp_repair_local_channel_membership(
        &self,
        _params: ChannelJoinParams,
    ) -> Result<(), RuntimeBridgeError> {
        Err(IntentError::no_agent("AMP not available in offline mode").into())
    }

    async fn amp_close_channel(
        &self,
        _params: ChannelCloseParams,
    ) -> Result<(), crate::runtime_bridge::RuntimeBridgeError> {
        Err(IntentError::no_agent("AMP not available in offline mode").into())
    }

    async fn amp_join_channel(
        &self,
        _params: ChannelJoinParams,
    ) -> Result<(), crate::runtime_bridge::RuntimeBridgeError> {
        #[cfg(test)]
        {
            *self.amp_join_calls.lock().await += 1;
            if let Some(result) = self.amp_join_results.lock().await.pop_front() {
                return result;
            }
        }
        Err(IntentError::no_agent("AMP not available in offline mode").into())
    }

    async fn amp_leave_channel(
        &self,
        _params: ChannelLeaveParams,
    ) -> Result<(), crate::runtime_bridge::RuntimeBridgeError> {
        Err(IntentError::no_agent("AMP not available in offline mode").into())
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
    ) -> Result<AmpCiphertext, crate::runtime_bridge::RuntimeBridgeError> {
        Err(IntentError::no_agent("AMP not available in offline mode").into())
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
        #[cfg(test)]
        {
            let answer = self.sync_status_answers.lock().await.pop_front();
            if let Some(answer) = answer {
                return answer.await;
            }
        }
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
        #[cfg(test)]
        if let Some(answer) = {
            let mut answers = self.sync_answers.lock().await;
            answers.pop_front()
        } {
            return answer.await;
        }
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
    ) -> Result<(), crate::runtime_bridge::RuntimeBridgeError> {
        Err(
            IntentError::no_agent("Peer channel establishment not available in offline mode")
                .into(),
        )
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

    async fn bootstrap_signing_keys(
        &self,
    ) -> Result<Vec<u8>, crate::runtime_bridge::RuntimeBridgeError> {
        Err(IntentError::no_agent("Key bootstrapping not available in offline mode").into())
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
        _setup: crate::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
    ) -> Result<DeviceEnrollmentStart, aura_invitation::enrollment_setup::EnrollmentIssuanceError>
    {
        Err(aura_invitation::enrollment_setup::EnrollmentIssuanceError::Unavailable)
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
    ) -> Result<CeremonyStatus, crate::runtime_bridge::RuntimeBridgeError> {
        Err(IntentError::no_agent("Guardian ceremony not available in offline mode").into())
    }

    #[cfg(test)]
    async fn get_ceremony_terminal_outcome(
        &self,
        ceremony_id: &CeremonyId,
    ) -> Result<Option<super::CeremonyTerminalOutcome>, crate::runtime_bridge::RuntimeBridgeError>
    {
        self.enrollment_outcomes
            .lock()
            .await
            .get(ceremony_id)
            .copied()
            .ok_or_else(|| IntentError::no_agent("enrollment ceremony is unknown").into())
    }

    #[cfg(test)]
    async fn list_device_enrollment_ceremonies(
        &self,
    ) -> Result<Vec<CeremonyId>, crate::runtime_bridge::RuntimeBridgeError> {
        Ok(self
            .enrollment_outcomes
            .lock()
            .await
            .keys()
            .cloned()
            .collect())
    }

    async fn get_key_rotation_ceremony_status(
        &self,
        _ceremony_id: &CeremonyId,
    ) -> Result<KeyRotationCeremonyStatus, crate::runtime_bridge::RuntimeBridgeError> {
        Err(IntentError::no_agent("Key rotation ceremonies not available in offline mode").into())
    }

    async fn cancel_key_rotation_ceremony(
        &self,
        _ceremony_id: &CeremonyId,
    ) -> Result<(), crate::runtime_bridge::RuntimeBridgeError> {
        Err(IntentError::no_agent("Key rotation ceremonies not available in offline mode").into())
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
        _receiver_nickname: Option<String>,
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
    ) -> Result<InvitationMutationOutcome, super::RuntimeBridgeError> {
        #[cfg(test)]
        {
            *self.accept_invitation_calls.lock().await += 1;
        }
        #[cfg(test)]
        {
            let answer = self.accept_answers.lock().await.pop_front();
            if let Some(answer) = answer {
                return answer.await;
            }
        }
        #[cfg(test)]
        if let Some(result) = self.accept_invitation_result.lock().await.clone() {
            return result;
        }
        Err(IntentError::no_agent("Invitation acceptance not available in offline mode").into())
    }

    async fn get_guardian_invitation_terminal_outcome(
        &self,
        _invitation_id: &aura_core::InvitationId,
    ) -> Result<Option<super::CeremonyTerminalOutcome>, IntentError> {
        #[cfg(test)]
        {
            *self.guardian_outcome_calls.lock().await += 1;
            let answer = self.guardian_outcome_answers.lock().await.pop_front();
            if let Some(answer) = answer {
                return answer.await;
            }
        }
        Err(IntentError::no_agent(
            "guardian invitation completion evidence is unavailable from this runtime",
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
        #[cfg(test)]
        {
            let answer = self.import_answers.lock().await.pop_front();
            if let Some(answer) = answer {
                return answer.await;
            }
        }
        Err(IntentError::no_agent(
            "Invitation import not available in offline mode",
        ))
    }

    async fn try_get_invited_peer_ids(&self) -> Result<Vec<AuthorityId>, IntentError> {
        Err(IntentError::no_agent(
            "Invited peer ids not available in offline mode",
        ))
    }

    async fn try_get_settings(&self) -> Result<SettingsBridgeState, RuntimeBridgeError> {
        Err(IntentError::no_agent("Settings not available in offline mode").into())
    }

    async fn has_account_config(&self) -> Result<bool, RuntimeBridgeError> {
        Ok(false)
    }

    async fn initialize_account(
        &self,
        _nickname_suggestion: &str,
    ) -> Result<(), RuntimeBridgeError> {
        Err(IntentError::no_agent("Account initialization not available in offline mode").into())
    }

    async fn try_list_devices(&self) -> Result<Vec<BridgeDeviceInfo>, RuntimeBridgeError> {
        Err(IntentError::no_agent("Devices not available in offline mode").into())
    }

    async fn try_list_authorities(&self) -> Result<Vec<BridgeAuthorityInfo>, RuntimeBridgeError> {
        Err(IntentError::no_agent("Authorities not available in offline mode").into())
    }

    async fn set_nickname_suggestion(&self, _name: &str) -> Result<(), RuntimeBridgeError> {
        Err(IntentError::no_agent("Settings update not available in offline mode").into())
    }

    async fn set_mfa_policy(&self, _policy: &str) -> Result<(), RuntimeBridgeError> {
        Err(IntentError::no_agent("Settings update not available in offline mode").into())
    }

    async fn set_device_signing_consent(
        &self,
        _consent: super::DeviceSigningConsent,
    ) -> Result<(), RuntimeBridgeError> {
        Err(IntentError::no_agent("Settings update not available in offline mode").into())
    }

    async fn try_list_pending_signing_requests(
        &self,
    ) -> Result<Vec<super::PendingSigningRequest>, RuntimeBridgeError> {
        Err(IntentError::no_agent("Signing requests not available in offline mode").into())
    }

    async fn decide_pending_signing_request(
        &self,
        _request_id: &str,
        _approve: bool,
    ) -> Result<(), RuntimeBridgeError> {
        Err(IntentError::no_agent("Signing requests not available in offline mode").into())
    }

    async fn set_peer_flow_allowance(
        &self,
        _context: aura_core::types::identifiers::ContextId,
        _peer: aura_core::types::identifiers::AuthorityId,
        _window: u64,
    ) -> Result<(), RuntimeBridgeError> {
        Err(IntentError::no_agent("Settings update not available in offline mode").into())
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

    async fn authentication_status(&self) -> Result<AuthenticationStatus, RuntimeBridgeError> {
        Ok(AuthenticationStatus::Unauthenticated)
    }

    fn physical_time_provider(&self) -> Arc<dyn aura_core::effects::PhysicalTimeEffects> {
        self.physical_time.clone()
    }

    async fn current_time_ms(&self) -> Result<u64, super::RuntimeBridgeError> {
        self.physical_time
            .physical_time()
            .await
            .map(|time| time.ts_ms)
            .map_err(|error| {
                RuntimeBridgeError::with_source(
                    IntentError::service_error("offline local clock failed"),
                    error,
                )
            })
    }

    async fn is_peer_online(&self, _peer: AuthorityId) -> bool {
        false
    }

    async fn sleep_ms(&self, ms: u64) -> Result<(), RuntimeBridgeError> {
        self.physical_time.sleep_ms(ms).await.map_err(|error| {
            RuntimeBridgeError::with_source(
                IntentError::service_error("offline local timer failed"),
                error,
            )
        })
    }

    async fn wait_until_physical_deadline(
        &self,
        deadline: aura_core::types::window::WindowPosition<
            aura_core::types::window::PhysicalMillis,
        >,
    ) -> Result<aura_core::time::PhysicalTime, RuntimeBridgeError> {
        self.physical_time
            .wait_until_physical_deadline(deadline)
            .await
            .map_err(|error| {
                RuntimeBridgeError::with_source(
                    IntentError::service_error("offline absolute timer failed"),
                    error,
                )
            })
    }

    async fn wait_for_background_refresh(&self, _ms: u64) -> Result<(), RuntimeBridgeError> {
        #[cfg(test)]
        if let Some(source) = self.background_refresh_failure.lock().await.take() {
            return Err(source);
        }
        futures::future::pending::<Result<(), RuntimeBridgeError>>().await
    }
}

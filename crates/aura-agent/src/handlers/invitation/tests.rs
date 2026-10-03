use super::*;
use crate::core::AgentConfig;
use crate::core::config::StorageConfig;
use crate::reactive::app_signal_views;
use crate::runtime::effects::AuraEffectSystem;
use crate::runtime::services::ceremony_runner::CeremonyRunner;
use crate::runtime::services::{CeremonyTracker, RendezvousManager, RendezvousManagerConfig};
use crate::runtime::TaskSupervisor;
use aura_app::signal_defs::{register_app_signals, HOMES_SIGNAL, INVITATIONS_SIGNAL};
use aura_app::views::home::{HomeRole, HomesState};
use aura_chat::{ChatFact, CHAT_FACT_TYPE_ID};
use aura_core::effects::CryptoCoreEffects;
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::hash::hash;
use aura_core::threshold::ThresholdSignature;
use aura_core::types::identifiers::{
    AuthorityId, CeremonyId, ChannelId, ContextId, InvitationId,
};
use aura_core::DeviceId;
use aura_effects::reactive::ReactiveHandler;
use aura_invitation::guards::{EffectCommand, GuardOutcome};
use aura_journal::fact::{FactContent, RelationalFact};
use aura_journal::DomainFact;
use aura_rendezvous::{RendezvousDescriptor, TransportHint};
use aura_relational::{ContactFact, CONTACT_FACT_TYPE_ID};
use aura_social::moderation::facts::HomeGrantModeratorFact;
use base64::Engine;
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, timeout};

fn create_test_authority(seed: u8) -> AuthorityContext {
    let authority_id = AuthorityId::new_from_entropy([seed; 32]);
    AuthorityContext::new(authority_id)
}

#[track_caller]
fn handler_for(authority: AuthorityContext) -> InvitationHandler {
    InvitationHandler::new(authority).unwrap()
}

#[track_caller]
fn handler_for_id(authority_id: AuthorityId) -> InvitationHandler {
    handler_for(AuthorityContext::new(authority_id))
}

async fn send_invitation_test_raw_envelope(
    effects: &Arc<AuraEffectSystem>,
    envelope: TransportEnvelope,
) -> Result<(), aura_core::effects::transport::TransportError> {
    aura_core::effects::TransportEffects::send_envelope(effects.as_ref(), envelope).await
}

fn test_transport_receipt_for_envelope(
    envelope: &TransportEnvelope,
) -> aura_core::effects::transport::TransportReceipt {
    aura_core::effects::transport::TransportReceipt {
        context: envelope.context,
        src: envelope.source,
        dst: envelope.destination,
        epoch: 1,
        cost: 1,
        nonce: 1,
        prev: [0u8; 32],
        sig: vec![1u8],
    }
}

async fn send_invitation_test_verified_envelope(
    effects: &Arc<AuraEffectSystem>,
    mut envelope: TransportEnvelope,
) -> Result<(), aura_core::effects::transport::TransportError> {
    envelope.receipt = Some(test_transport_receipt_for_envelope(&envelope));
    crate::runtime::transport_boundary::send_guarded_transport_envelope(effects.as_ref(), envelope)
        .await
}

fn install_full_invitation_biscuit_cache(
    effects: &Arc<AuraEffectSystem>,
    authority: AuthorityId,
) {
    let issuer = aura_authorization::TokenAuthority::new(authority);
    let token = issuer
        .create_token(
            authority,
            crate::token_profiles::TokenCapabilityProfile::StandardDevice,
        )
        .expect("full invitation biscuit should build");
    let engine = base64::engine::general_purpose::STANDARD;
    effects.set_biscuit_cache(crate::runtime::effects::BiscuitCache {
        token_b64: engine.encode(token.to_vec().expect("token should serialize")),
        issuer_authority: authority,
        root_pk_b64: engine.encode(issuer.root_public_key().to_bytes()),
    });
}

#[track_caller]
fn effects_for(authority: &AuthorityContext) -> Arc<AuraEffectSystem> {
    let config = AgentConfig {
        device_id: authority.device_id(),
        ..Default::default()
    };
    let effects = Arc::new(
        AuraEffectSystem::simulation_for_test_for_authority(&config, authority.authority_id())
            .unwrap(),
    );
    install_full_invitation_biscuit_cache(&effects, authority.authority_id());
    effects
}

#[track_caller]
fn production_effects_for(authority: &AuthorityContext) -> Arc<AuraEffectSystem> {
    let config = AgentConfig {
        storage: StorageConfig {
            base_path: tempfile::Builder::new()
                .prefix("aura-agent-invitation-prod-")
                .tempdir()
                .expect("production invitation root should build")
                .keep()
                .join("aura"),
            secure_storage_backend: crate::core::config::SecureStorageBackend::FilesystemFallback,
            ..Default::default()
        },
        device_id: authority.device_id(),
        ..Default::default()
    };
    let effects = Arc::new(
        AuraEffectSystem::production_for_test_for_authority(config, authority.authority_id())
            .unwrap(),
    );
    install_full_invitation_biscuit_cache(&effects, authority.authority_id());
    effects
}

async fn bootstrap_test_signing_authority(
    effects: &Arc<AuraEffectSystem>,
    authority_id: AuthorityId,
) {
    effects
        .bootstrap_authority(&authority_id)
        .await
        .expect("test signing authority should bootstrap");
}

async fn sign_test_channel_acceptance(
    effects: &Arc<AuraEffectSystem>,
    invitation: &Invitation,
    acceptor_id: AuthorityId,
    context_id: ContextId,
    channel_id: ChannelId,
    channel_name: Option<String>,
) -> ThresholdSignature {
    bootstrap_test_signing_authority(effects, acceptor_id).await;
    sign_invitation_acceptance_transcript(
        effects.as_ref(),
        acceptor_id,
        &channel_invitation_acceptance_transcript(
            invitation,
            acceptor_id,
            context_id,
            channel_id,
            channel_name,
        ),
    )
    .await
    .expect("channel acceptance transcript should sign")
}

async fn sign_test_contact_acceptance(
    effects: &Arc<AuraEffectSystem>,
    invitation: &Invitation,
    acceptor_id: AuthorityId,
) -> ThresholdSignature {
    bootstrap_test_signing_authority(effects, acceptor_id).await;
    sign_invitation_acceptance_transcript(
        effects.as_ref(),
        acceptor_id,
        &contact_invitation_acceptance_transcript(invitation, acceptor_id, None),
    )
    .await
    .expect("contact acceptance transcript should sign")
}

fn canonical_home_id(seed: u8) -> ChannelId {
    ChannelId::from_bytes([seed; 32])
}

async fn register_test_app_signals(effects: &AuraEffectSystem) {
    register_app_signals(&effects.reactive_handler())
        .await
        .unwrap();
}

async fn attach_test_rendezvous_manager(
    effects: &AuraEffectSystem,
    authority_id: AuthorityId,
) -> Arc<crate::runtime::TaskSupervisor> {
    let manager = crate::runtime::services::RendezvousManager::new_with_default_udp(
        authority_id,
        crate::runtime::services::RendezvousManagerConfig::default(),
        Arc::new(effects.time_effects().clone()),
    );
    effects.attach_rendezvous_manager(manager.clone());
    let tasks = Arc::new(crate::runtime::TaskSupervisor::new());
    let service_context = crate::runtime::services::RuntimeServiceContext::new(
        tasks.clone(),
        Arc::new(effects.time_effects().clone()),
    );
    crate::runtime::services::RuntimeService::start(&manager, &service_context)
        .await
        .unwrap();
    tasks
}

async fn cache_test_peer_descriptor(
    effects: &AuraEffectSystem,
    local_authority: AuthorityId,
    peer: AuthorityId,
    addr: &str,
    now_ms: u64,
) {
    let manager = effects
        .rendezvous_manager()
        .expect("test rendezvous manager should be attached");
    let hint = TransportHint::tcp_direct(addr.trim_start_matches("tcp://")).unwrap();
    let peer_context_id = default_context_id_for_authority(peer);
    manager
        .cache_descriptor(RendezvousDescriptor {
            authority_id: peer,
            device_id: None,
            context_id: peer_context_id,
            transport_hints: vec![hint.clone()],
            handshake_psk_commitment: [7u8; 32],
            public_key: [8u8; 32],
            valid_from: now_ms.saturating_sub(1),
            valid_until: now_ms.saturating_add(86_400_000),
            nonce: [0u8; 32],
            nickname_suggestion: None,
        })
        .await
        .unwrap();

    let local_context_id = default_context_id_for_authority(local_authority);
    if local_context_id != peer_context_id {
        manager
            .cache_descriptor(RendezvousDescriptor {
                authority_id: peer,
                device_id: None,
                context_id: local_context_id,
                transport_hints: vec![hint],
                handshake_psk_commitment: [7u8; 32],
                public_key: [8u8; 32],
                valid_from: now_ms.saturating_sub(1),
                valid_until: now_ms.saturating_add(86_400_000),
                nonce: [0u8; 32],
                nickname_suggestion: None,
            })
            .await
            .unwrap();
    }
}

async fn accept_invitation_without_notification(
    handler: &InvitationHandler,
    effects: Arc<AuraEffectSystem>,
    invitation_id: &InvitationId,
) {
    handler
        .accept_invitation(effects, invitation_id)
        .await
        .unwrap();
}

fn invitation_service_for(
    authority_context: AuthorityContext,
    effects: Arc<AuraEffectSystem>,
) -> InvitationServiceApi {
    let time_effects: Arc<dyn aura_core::effects::time::PhysicalTimeEffects> =
        Arc::new(effects.time_effects().clone());
    let ceremony_runner = CeremonyRunner::new(CeremonyTracker::new(time_effects));
    InvitationServiceApi::new_with_runner(
        effects,
        authority_context,
        ceremony_runner,
        Arc::new(TaskSupervisor::new()),
    )
    .unwrap()
}

fn unsigned_test_code_for_invitation(invitation: &Invitation) -> String {
    ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: invitation.invitation_id.clone(),
        sender_id: invitation.sender_id,
        context_id: Some(invitation.context_id),
        invitation_type: invitation.invitation_type.clone(),
        expires_at: invitation.expires_at,
        message: invitation.message.clone(),
    }
    .to_code()
    .expect("test shareable invitation should serialize")
}

/// An inviter and an invitee on one shared transport, each with signing
/// keys and a descriptor for the other, as a contact link needs.
pub(crate) struct ContactPair {
    transport: crate::runtime::SharedTransport,
    pub(crate) sender_id: AuthorityId,
    pub(crate) receiver_id: AuthorityId,
    pub(crate) sender_effects: Arc<AuraEffectSystem>,
    pub(crate) receiver_effects: Arc<AuraEffectSystem>,
    pub(crate) sender_handler: InvitationHandler,
    pub(crate) receiver_handler: InvitationHandler,
    _tasks: (Arc<TaskSupervisor>, Arc<TaskSupervisor>),
}

/// Effects for one side of a contact pair. Every pair is built here, so the
/// authority names the test seed (registered, so reused seeds fail fast).
#[track_caller]
fn contact_pair_effects(
    authority_id: AuthorityId,
    transport: crate::runtime::SharedTransport,
) -> Arc<AuraEffectSystem> {
    Arc::new(
        AuraEffectSystem::simulation_for_named_test_with_shared_transport_for_authority(
            &AgentConfig::default(),
            &format!("contact-pair:{authority_id}"),
            authority_id,
            transport,
        )
        .expect("contact pair effects should build"),
    )
}

pub(crate) async fn contact_pair(seed: u8) -> ContactPair {
    let shared_transport = crate::runtime::SharedTransport::new();
    let sender_id = AuthorityId::new_from_entropy([seed; 32]);
    let receiver_id = AuthorityId::new_from_entropy([seed.wrapping_add(1); 32]);
    let transport = shared_transport.clone();
    let sender_effects = contact_pair_effects(sender_id, shared_transport.clone());
    let receiver_effects = contact_pair_effects(receiver_id, shared_transport);
    let sender_tasks = attach_test_rendezvous_manager(sender_effects.as_ref(), sender_id).await;
    let receiver_tasks =
        attach_test_rendezvous_manager(receiver_effects.as_ref(), receiver_id).await;
    let now_ms = 1_700_000_000_000;
    cache_test_peer_descriptor(
        sender_effects.as_ref(),
        sender_id,
        receiver_id,
        "tcp://receiver.test:1",
        now_ms,
    )
    .await;
    cache_test_peer_descriptor(
        receiver_effects.as_ref(),
        receiver_id,
        sender_id,
        "tcp://sender.test:1",
        now_ms,
    )
    .await;
    bootstrap_test_signing_authority(&sender_effects, sender_id).await;
    bootstrap_test_signing_authority(&receiver_effects, receiver_id).await;
    ContactPair {
        transport,
        sender_id,
        receiver_id,
        sender_effects,
        receiver_effects,
        sender_handler: handler_for_id(sender_id),
        receiver_handler: handler_for_id(receiver_id),
        _tasks: (sender_tasks, receiver_tasks),
    }
}

impl ContactPair {
    /// Another inviter, on the same transport, for the same invitee.
    async fn with_new_inviter(&self, seed: u8) -> ContactPair {
        let sender_id = AuthorityId::new_from_entropy([seed; 32]);
        let sender_effects = contact_pair_effects(sender_id, self.transport.clone());
        let sender_tasks =
            attach_test_rendezvous_manager(sender_effects.as_ref(), sender_id).await;
        let now_ms = 1_700_000_000_000;
        cache_test_peer_descriptor(
            sender_effects.as_ref(),
            sender_id,
            self.receiver_id,
            "tcp://receiver.test:1",
            now_ms,
        )
        .await;
        cache_test_peer_descriptor(
            self.receiver_effects.as_ref(),
            self.receiver_id,
            sender_id,
            "tcp://sender.test:2",
            now_ms,
        )
        .await;
        bootstrap_test_signing_authority(&sender_effects, sender_id).await;
        ContactPair {
            transport: self.transport.clone(),
            sender_id,
            receiver_id: self.receiver_id,
            sender_effects,
            receiver_effects: self.receiver_effects.clone(),
            sender_handler: handler_for_id(sender_id),
            receiver_handler: self.receiver_handler.clone(),
            _tasks: (sender_tasks, self._tasks.1.clone()),
        }
    }

    async fn create_contact_invitation(&self) -> Invitation {
        self.sender_handler
            .create_invitation(
                self.sender_effects.clone(),
                self.receiver_id,
                InvitationType::Contact { nickname: None },
                None,
                None,
            )
            .await
            .expect("contact invitation should be created")
    }

    /// A code signed by the inviter, so the invitee can authenticate the
    /// inviter's response.
    async fn signed_code(&self, invitation: &Invitation) -> String {
        crate::handlers::invitation_service::InvitationServiceApi::export_signed_invitation_with_transport(
            self.sender_effects.as_ref(),
            invitation,
            // Real exports name the sender device, as known-sender trust needs.
            &ShareableInvitationTransportMetadata {
                sender_device_id: Some(self.sender_effects.device_id()),
                ..ShareableInvitationTransportMetadata::default()
            },
            false,
        )
        .await
        .expect("signed invitation code should export")
    }

    async fn import(&self, code: &str) -> Invitation {
        self.receiver_handler
            .import_invitation_code(&self.receiver_effects, code)
            .await
            .expect("invitation code should import")
    }

    /// Accepts while the inviter processes acceptances, as its runtime would.
    async fn accept_with_responding_inviter(
        &self,
        invitation_id: &InvitationId,
    ) -> AgentResult<InvitationResult> {
        self.accept_via(&self.receiver_handler, invitation_id).await
    }

    /// Accepts through `handler` (a receiver handler instance) while the
    /// inviter responds.
    async fn accept_via(
        &self,
        handler: &InvitationHandler,
        invitation_id: &InvitationId,
    ) -> AgentResult<InvitationResult> {
        self.respond_while(Box::pin(
            handler.accept_invitation(self.receiver_effects.clone(), invitation_id),
        ))
            .await
    }

    /// Runs `work` while the inviter processes acceptances, as its runtime
    /// would.
    /// Callers box large futures so test futures stay small.
    pub(crate) async fn respond_while<F: Future>(&self, work: F) -> F::Output {
        let respond = async {
            loop {
                let _ = self
                    .sender_handler
                    .process_contact_invitation_acceptances(self.sender_effects.clone())
                    .await;
                sleep(Duration::from_millis(20)).await;
            }
        };
        tokio::select! {
            result = work => result,
            () = respond => unreachable!("the inviter loop never ends"),
        }
    }
}

#[track_caller]
fn run_async_test_on_large_stack<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    std::thread::Builder::new()
        .name("invitation-test-large-stack".to_string())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("test runtime should build");
            runtime.block_on(future);
        })
        .expect("large-stack test thread should spawn")
        .join()
        .expect("large-stack test thread should complete");
}

macro_rules! large_stack_async_test {
    ($name:ident, $body:block) => {
        #[test]
        fn $name() {
            run_async_test_on_large_stack(async move $body);
        }
    };
}

struct EnvRestore {
    values: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl EnvRestore {
    fn capture(keys: &[&'static str]) -> Self {
        Self {
            values: keys
                .iter()
                .map(|key| (*key, std::env::var_os(key)))
                .collect(),
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        for (key, value) in self.values.drain(..) {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
}

#[tokio::test]
async fn channel_home_materialization_requires_registered_homes_signal() {
    let reactive = ReactiveHandler::new();

    let error = app_signal_views::materialize_home_signal_for_channel_invitation(
        &reactive,
        AuthorityId::new_from_entropy([1u8; 32]),
        canonical_home_id(1),
        "shared-parity-lab",
        AuthorityId::new_from_entropy([2u8; 32]),
        ContextId::new_from_entropy([3u8; 32]),
        0,
    )
    .await
    .unwrap_err();
    let message = error.clone();
    assert!(
        message.contains("requires registered homes signal"),
        "unexpected error: {message}"
    );
}

#[tokio::test]
async fn test_execute_allowed_outcome() {
    let authority = create_test_authority(130);
    let effects = effects_for(&authority);

    let outcome = GuardOutcome::allowed(vec![EffectCommand::ChargeFlowBudget {
        cost: FlowCost::new(1),
    }]);

    let result = execute_guard_outcome(outcome, &authority, effects.as_ref()).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_execute_denied_outcome() {
    let authority = create_test_authority(131);
    let effects = effects_for(&authority);

    let outcome = GuardOutcome::denied(aura_guards::types::GuardViolation::other(
        "Test denial reason",
    ));

    let result = execute_guard_outcome(outcome, &authority, effects.as_ref()).await;
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.to_string().contains("Test denial reason"));
}

#[tokio::test]
async fn test_execute_journal_append() {
    let authority = create_test_authority(132);
    let effects = effects_for(&authority);

    let fact = InvitationFact::sent_ms(
        ContextId::new_from_entropy([232u8; 32]),
        InvitationId::new("inv-test"),
        authority.authority_id(),
        AuthorityId::new_from_entropy([133u8; 32]),
        InvitationType::Contact { nickname: None },
        1000,
        Some(2000),
        None,
    );

    let outcome = GuardOutcome::allowed(vec![EffectCommand::JournalAppend { fact }]);

    let result = execute_guard_outcome(outcome, &authority, effects.as_ref()).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_execute_notify_peer() {
    let authority = create_test_authority(134);
    let shared_transport = crate::runtime::SharedTransport::new();
    let config = AgentConfig::default();
    let peer = AuthorityId::new_from_entropy([135u8; 32]);
    let now_ms: u64 = 1_700_000_000_000;
    let effects =
        crate::testing::simulation_effect_system_with_shared_transport_for_authority_arc(
            &config,
            authority.authority_id(),
            shared_transport.clone(),
        );
    // Materialize a destination participant on the shared transport.
    let _peer_effects =
        crate::testing::simulation_effect_system_with_shared_transport_for_authority_arc(
            &config,
            peer,
            shared_transport,
        );
    let _authority_rendezvous_tasks =
        attach_test_rendezvous_manager(effects.as_ref(), authority.authority_id()).await;
    let _peer_rendezvous_tasks =
        attach_test_rendezvous_manager(_peer_effects.as_ref(), peer).await;
    cache_test_peer_descriptor(
        effects.as_ref(),
        authority.authority_id(),
        peer,
        "tcp://127.0.0.1:55011",
        now_ms,
    )
    .await;
    cache_test_peer_descriptor(
        _peer_effects.as_ref(),
        peer,
        authority.authority_id(),
        "tcp://127.0.0.1:55012",
        now_ms,
    )
    .await;
    let handler = handler_for(authority.clone());

    let invitation = handler
        .create_invitation(
            effects.clone(),
            peer,
            InvitationType::Contact { nickname: None },
            None,
            None,
        )
        .await
        .unwrap();

    let outcome = GuardOutcome::allowed(vec![EffectCommand::NotifyPeer {
        peer,
        invitation_id: invitation.invitation_id,
    }]);

    let result = execute_guard_outcome(outcome, &authority, effects.as_ref()).await;
    assert!(result.is_ok(), "{result:?}");

    let received = timeout(Duration::from_secs(2), _peer_effects.receive_envelope())
        .await
        .expect("invitation envelope should arrive before timeout")
        .expect("invitation delivery should not fail receipt validation");
    assert_eq!(received.destination, peer);
    assert_eq!(received.source, authority.authority_id());
    assert_eq!(
        received.context,
        default_context_id_for_authority(peer)
    );
    assert_eq!(
        received.metadata.get("content-type").map(String::as_str),
        Some("application/aura-invitation")
    );
    let code = String::from_utf8(received.payload).expect("invitation payload should be utf-8");
    if effects.lan_transport().is_some() {
        assert!(
            ShareableInvitation::sender_addr_from_code(&code).is_some(),
            "invitation notify should carry a sender hint when LAN transport is attached"
        );
        assert_eq!(
            ShareableInvitation::sender_device_id_from_code(&code),
            Some(authority.device_id())
        );
    }
}

#[tokio::test]
async fn test_execute_record_receipt() {
    let authority = create_test_authority(136);
    let effects = effects_for(&authority);

    let outcome = GuardOutcome::allowed(vec![EffectCommand::RecordReceipt {
        operation: InvitationOperation::SendInvitation,
        peer: Some(AuthorityId::new_from_entropy([137u8; 32])),
    }]);

    let result = execute_guard_outcome(outcome, &authority, effects.as_ref()).await;
    assert!(result.is_ok(), "{result:?}");
}

#[tokio::test]
async fn send_invitation_records_receipt_in_peer_delivery_context() {
    let authority = create_test_authority(137);
    let effects = production_effects_for(&authority);
    let peer = AuthorityId::new_from_entropy([138u8; 32]);
    let outcome = GuardOutcome::allowed(vec![
        EffectCommand::ChargeFlowBudget {
            cost: FlowCost::new(1),
        },
        EffectCommand::RecordReceipt {
            operation: InvitationOperation::SendInvitation,
            peer: Some(peer),
        },
    ]);

    execute_guard_outcome(outcome, &authority, effects.as_ref())
        .await
        .expect("production invitation receipt record should succeed");

    let delivery_context = default_context_id_for_authority(peer);
    let key_prefix = format!(
        "invitation/receipts/{}/{}/send_invitation/",
        delivery_context, peer
    );

    let mut found = None;
    for nonce in 0..=4 {
        let key = format!("{key_prefix}{nonce}");
        if let Some(bytes) = effects
            .retrieve(&key)
            .await
            .expect("receipt lookup should succeed")
        {
            found = Some((key, bytes));
            break;
        }
    }

    let (_, bytes) = found.expect("send invitation receipt should be stored under delivery context");
    let stored: Receipt =
        serde_json::from_slice(&bytes).expect("stored invitation receipt should deserialize");
    assert_eq!(stored.ctx, delivery_context);
    assert_eq!(stored.dst, peer);
    assert_eq!(stored.src, authority.authority_id());
}

#[tokio::test]
async fn test_execute_multiple_commands() {
    let authority = create_test_authority(138);
    let shared_transport = crate::runtime::SharedTransport::new();
    let config = AgentConfig::default();
    let peer = AuthorityId::new_from_entropy([139u8; 32]);
    let now_ms: u64 = 1_700_000_000_000;
    let effects =
        crate::testing::simulation_effect_system_with_shared_transport_for_authority_arc(
            &config,
            authority.authority_id(),
            shared_transport.clone(),
        );
    // Materialize a destination participant on the shared transport.
    let _peer_effects =
        crate::testing::simulation_effect_system_with_shared_transport_for_authority_arc(
            &config,
            peer,
            shared_transport,
        );
    let _authority_rendezvous_tasks =
        attach_test_rendezvous_manager(effects.as_ref(), authority.authority_id()).await;
    let _peer_rendezvous_tasks =
        attach_test_rendezvous_manager(_peer_effects.as_ref(), peer).await;
    cache_test_peer_descriptor(
        effects.as_ref(),
        authority.authority_id(),
        peer,
        "tcp://127.0.0.1:55021",
        now_ms,
    )
    .await;
    cache_test_peer_descriptor(
        _peer_effects.as_ref(),
        peer,
        authority.authority_id(),
        "tcp://127.0.0.1:55022",
        now_ms,
    )
    .await;
    let handler = handler_for(authority.clone());

    let invitation = handler
        .create_invitation(
            effects.clone(),
            peer,
            InvitationType::Contact { nickname: None },
            None,
            None,
        )
        .await
        .unwrap();
    let outcome = GuardOutcome::allowed(vec![
        EffectCommand::ChargeFlowBudget {
            cost: FlowCost::new(1),
        },
        EffectCommand::NotifyPeer {
            peer,
            invitation_id: invitation.invitation_id,
        },
        EffectCommand::RecordReceipt {
            operation: InvitationOperation::SendInvitation,
            peer: Some(peer),
        },
    ]);

    let result = execute_guard_outcome(outcome, &authority, effects.as_ref()).await;
    assert!(result.is_ok(), "{result:?}");

    let received = timeout(Duration::from_secs(2), _peer_effects.receive_envelope())
        .await
        .expect("invitation envelope should arrive before timeout")
        .expect("invitation delivery should not fail receipt validation");
    assert_eq!(received.destination, peer);
    assert_eq!(received.source, authority.authority_id());
    assert_eq!(
        received.context,
        default_context_id_for_authority(peer)
    );
    assert_eq!(
        received.metadata.get("content-type").map(String::as_str),
        Some("application/aura-invitation")
    );
}

#[tokio::test]
async fn invitation_can_be_created() {
    let authority_context = create_test_authority(91);
    let effects = effects_for(&authority_context);
    let handler = handler_for(authority_context.clone());

    let receiver_id = AuthorityId::new_from_entropy([92u8; 32]);

    let invitation = handler
        .create_invitation(
            effects.clone(),
            receiver_id,
            InvitationType::Contact {
                nickname: Some("alice".to_string()),
            },
            Some("Let's connect!".to_string()),
            Some(86400000), // 1 day
        )
        .await
        .unwrap();

    assert!(invitation.invitation_id.as_str().starts_with("inv-"));
    assert_eq!(invitation.sender_id, authority_context.authority_id());
    assert_eq!(invitation.receiver_id, receiver_id);
    assert_eq!(invitation.status, InvitationStatus::Pending);
    assert!(invitation.expires_at.is_some());
}

large_stack_async_test!(invitation_can_be_accepted, {
    let pair = contact_pair(93).await;
    let invitation = pair.create_contact_invitation().await;
    let imported = pair.import(&pair.signed_code(&invitation).await).await;

    let result = pair
        .accept_with_responding_inviter(&imported.invitation_id)
        .await
        .unwrap();

    assert_eq!(result.new_status, InvitationStatus::Accepted);
});

#[tokio::test]
async fn invitation_can_be_declined() {
    let authority_context = create_test_authority(96);
    let effects = effects_for(&authority_context);
    let handler = InvitationHandler::new(authority_context).unwrap();

    let receiver_id = AuthorityId::new_from_entropy([97u8; 32]);
    let context_id = ContextId::new_from_entropy([98u8; 32]);
    let home_id = canonical_home_id(11);

    effects
        .create_channel(ChannelCreateParams {
            context: context_id,
            channel: Some(home_id),
            skip_window: None,
            topic: None,
        })
        .await
        .unwrap();

    let invitation = handler
        .create_invitation_with_context(
            effects.clone(),
            receiver_id,
            InvitationType::Channel {
                home_id,
                nickname_suggestion: None,
                bootstrap: None,
                home: false,
            },
            None,
            Some(context_id),
            None,
            None,
        )
        .await
        .unwrap();

    let result = handler
        .decline_invitation(effects.clone(), &invitation.invitation_id)
        .await
        .unwrap();

    assert_eq!(result.new_status, InvitationStatus::Declined);
}

#[tokio::test]
async fn importing_channel_invitation_without_context_rejects_before_persist() {
    let authority_context = create_test_authority(101);
    let effects = effects_for(&authority_context);
    let handler = handler_for(authority_context.clone());

    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-channel-missing-context"),
        sender_id: AuthorityId::new_from_entropy([102u8; 32]),
        context_id: None,
        invitation_type: InvitationType::Channel {
            home_id: canonical_home_id(17),
            nickname_suggestion: Some("shared-parity-lab".to_string()),
            bootstrap: None,
            home: false,
        },
        expires_at: None,
        message: None,
    };
    let code = shareable
        .to_code()
        .expect("shareable invitation should serialize");

    let error = handler
        .import_invitation_code(effects.as_ref(), &code)
        .await
        .expect_err("channel invitation without authoritative context should fail");
    assert!(error.to_string().contains("missing authoritative context"));

    let persisted = InvitationHandler::load_imported_invitation(
        effects.as_ref(),
        authority_context.authority_id(),
        &shareable.invitation_id,
        None,
    )
    .await;
    assert!(persisted.is_none());
}

#[tokio::test]
async fn accepting_guardian_invitation_surfaces_choreography_failure() {
    let authority_context = create_test_authority(103);
    let effects = effects_for(&authority_context);
    let handler = InvitationHandler::new(authority_context).unwrap();
    let sender_id = AuthorityId::new_from_entropy([104u8; 32]);
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-guardian-missing-ceremony"),
        sender_id,
        context_id: None,
        invitation_type: InvitationType::Guardian {
            subject_authority: sender_id,
        },
        expires_at: None,
        message: None,
    };
    let code = shareable
        .to_code()
        .expect("shareable invitation should serialize");
    let imported = handler
        .import_invitation_code(effects.as_ref(), &code)
        .await
        .expect("guardian invitation should import");

    let error = timeout(
        Duration::from_secs(5),
        handler.accept_invitation(effects.clone(), &imported.invitation_id),
    )
    .await
    .expect("guardian accept should terminate")
    .expect_err("guardian choreography failure should surface");
    // With no principal online the signed acceptance cannot be delivered;
    // the failure must still surface to the caller.
    assert!(
        error.to_string().contains("accept_invitation_choreography"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn declining_contact_invitation_succeeds_locally_when_exchange_failure_occurs() {
    let authority_context = create_test_authority(105);
    let effects = effects_for(&authority_context);
    let handler = InvitationHandler::new(authority_context).unwrap();
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-contact-missing-decline-exchange"),
        sender_id: AuthorityId::new_from_entropy([106u8; 32]),
        context_id: None,
        invitation_type: InvitationType::Contact {
            nickname: Some("Alice".to_string()),
        },
        expires_at: None,
        message: None,
    };
    let code = shareable
        .to_code()
        .expect("shareable invitation should serialize");
    let imported = handler
        .import_invitation_code(effects.as_ref(), &code)
        .await
        .expect("contact invitation should import");

    let result = timeout(
        Duration::from_secs(5),
        handler.decline_invitation(effects.clone(), &imported.invitation_id),
    )
    .await
    .expect("decline should terminate")
    .expect("decline should settle locally even if follow-up exchange fails");
    assert_eq!(result.new_status, InvitationStatus::Declined);

    let stored = handler
        .get_invitation_with_storage(effects.as_ref(), &imported.invitation_id)
        .await
        .expect("declined invitation should remain queryable");
    assert_eq!(stored.status, InvitationStatus::Declined);
}

#[tokio::test]
async fn build_snapshot_uses_authoritative_flow_budget_state() {
    let authority_context = create_test_authority(115);
    let effects = effects_for(&authority_context);
    let handler = InvitationHandler::new(authority_context.clone()).unwrap();
    let context_id = authority_context.default_context_id();

    aura_core::effects::JournalEffects::update_flow_budget(
        effects.as_ref(),
        &context_id,
        &authority_context.authority_id(),
        &aura_core::FlowBudget {
            limit: 50,
            spent: 27,
            epoch: aura_core::Epoch::new(7),
        },
    )
    .await
    .unwrap();

    let snapshot = handler
        .build_snapshot_for_context(effects.as_ref(), context_id)
        .await;
    assert_eq!(snapshot.flow_budget_remaining, FlowCost::new(23));
    assert_eq!(snapshot.epoch, 7);
}

#[tokio::test]
async fn build_snapshot_without_biscuit_frontier_has_empty_capability_frontier() {
    let authority_context = create_test_authority(140);
    let config = AgentConfig::default();
    let effects = crate::testing::simulation_effect_system_arc(&config);
    let handler = InvitationHandler::new(authority_context.clone()).unwrap();
    effects.clear_biscuit_cache();

    let snapshot = handler
        .build_snapshot_for_context(effects.as_ref(), authority_context.default_context_id())
        .await;

    assert!(snapshot.capabilities.is_empty());
}

#[tokio::test]
async fn creating_invitation_is_denied_when_biscuit_lacks_invitation_send_capability() {
    let authority_context = create_test_authority(116);
    let effects = effects_for(&authority_context);
    let handler = InvitationHandler::new(authority_context.clone()).unwrap();
    let keypair = aura_authorization::KeyPair::new();
    let authority = authority_context.authority_id().to_string();
    let token = biscuit_auth::macros::biscuit!(
        r#"
        authority({authority});
        role("member");
        capability("read");
        capability("write");
    "#
    )
    .build(&keypair)
    .expect("capability-limited biscuit should build");
    let token_bytes = token.to_vec().expect("token should serialize");
    let engine = base64::engine::general_purpose::STANDARD;
    effects.set_biscuit_cache(crate::runtime::effects::BiscuitCache {
        token_b64: engine.encode(&token_bytes),
        issuer_authority: authority_context.authority_id(),
        root_pk_b64: engine.encode(keypair.public().to_bytes()),
    });

    let error = handler
        .create_invitation(
            effects.clone(),
            AuthorityId::new_from_entropy([117u8; 32]),
            InvitationType::Contact { nickname: None },
            None,
            None,
        )
        .await
        .expect_err("missing invitation:send capability should deny invitation creation");
    assert!(error.to_string().contains("Guard denied operation"));
}

#[tokio::test]
async fn accepting_unknown_invitation_is_rejected() {
    let authority_context = create_test_authority(118);
    let effects = effects_for(&authority_context);
    let handler = InvitationHandler::new(authority_context).unwrap();

    let error = handler
        .accept_invitation(effects, &InvitationId::new("invitation-does-not-exist"))
        .await
        .expect_err("unknown invitation should be rejected");
    assert!(error.to_string().contains("not found"));
}

#[tokio::test]
async fn accept_guard_outcome_continues_after_deferred_network_failures() {
    let authority = create_test_authority(107);
    let effects = production_effects_for(&authority);
    let peer = AuthorityId::new_from_entropy([108u8; 32]);
    let outcome = aura_invitation::guards::GuardOutcome::allowed(vec![
        aura_invitation::guards::EffectCommand::ChargeFlowBudget {
            cost: FlowCost::new(1),
        },
        aura_invitation::guards::EffectCommand::NotifyPeer {
            peer,
            invitation_id: InvitationId::new("inv-missing-notify"),
        },
        aura_invitation::guards::EffectCommand::RecordReceipt {
            operation: InvitationOperation::AcceptInvitation,
            peer: Some(peer),
        },
    ]);

    execute_guard_outcome_for_accept(outcome, &authority, effects.as_ref())
        .await
        .expect("deferred network failures should not block accept settlement");
}

#[test]
fn accept_guard_outcome_only_defers_peer_notification() {
    let authority = create_test_authority(109);
    let peer = AuthorityId::new_from_entropy([110u8; 32]);
    let invitation_id = InvitationId::new("inv-accept-split");
    let outcome = aura_invitation::guards::GuardOutcome::allowed(vec![
        aura_invitation::guards::EffectCommand::ChargeFlowBudget {
            cost: FlowCost::new(1),
        },
        aura_invitation::guards::EffectCommand::JournalAppend {
            fact: InvitationFact::Accepted {
                context_id: Some(authority.default_context_id()),
                invitation_id: invitation_id.clone(),
                acceptor_id: authority.authority_id(),
                accepted_at: PhysicalTime {
                    ts_ms: 1,
                    uncertainty: None,
                },
            },
        },
        aura_invitation::guards::EffectCommand::NotifyPeer {
            peer,
            invitation_id: invitation_id.clone(),
        },
        aura_invitation::guards::EffectCommand::RecordReceipt {
            operation: InvitationOperation::AcceptInvitation,
            peer: Some(peer),
        },
    ]);

    let execution_plan = aura_invitation::guards::plan_accept_execution(outcome)
        .expect("accept split should succeed");

    assert_eq!(execution_plan.local_effects.len(), 3);
    assert_eq!(execution_plan.deferred_network_effects.len(), 1);
    assert!(matches!(
        execution_plan.deferred_network_effects.first(),
        Some(aura_invitation::guards::EffectCommand::NotifyPeer { .. })
    ));
}

#[test]
fn malformed_home_id_rejected_at_string_boundary() {
    let err =
        channel_id_from_home_id("oak-house").expect_err("malformed home id should be rejected");
    assert!(matches!(err, AgentError::Config(_)));
}

large_stack_async_test!(importing_and_accepting_contact_invitation_commits_contact_fact, {
    let pair = contact_pair(120).await;
    let own_authority = pair.receiver_id;
    let sender_id = pair.sender_id;
    let invitation = pair.create_contact_invitation().await;
    let imported = pair.import(&pair.signed_code(&invitation).await).await;
    assert_eq!(imported.sender_id, sender_id);
    assert_eq!(imported.receiver_id, own_authority);

    pair.accept_with_responding_inviter(&imported.invitation_id)
        .await
        .unwrap();

    let committed = pair
        .receiver_effects
        .load_committed_facts(own_authority)
        .await
        .unwrap();

    let mut found = None::<ContactFact>;
    let mut seen_binding_types: Vec<String> = Vec::new();
    for fact in committed {
        let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = fact.content
        else {
            continue;
        };

        seen_binding_types.push(envelope.type_id.as_str().to_string());
        if envelope.type_id.as_str() != CONTACT_FACT_TYPE_ID {
            continue;
        }

        found = ContactFact::from_envelope(&envelope);
    }

    if found.is_none() {
        panic!(
            "Expected a committed ContactFact, saw bindings: {:?}",
            seen_binding_types
        );
    }
    let fact = found.unwrap();
    match fact {
        ContactFact::Added {
            owner_id,
            contact_id,
            ..
        } => {
            assert_eq!(owner_id, own_authority);
            assert_eq!(contact_id, sender_id);
        }
        other => panic!("Expected ContactFact::Added, got {:?}", other),
    }
});

large_stack_async_test!(accepting_contact_invitation_notifies_sender_and_adds_contact, {
    let pair = contact_pair(124).await;
    let (sender_id, receiver_id) = (pair.sender_id, pair.receiver_id);
    let invitation = pair.create_contact_invitation().await;
    let imported = pair.import(&pair.signed_code(&invitation).await).await;

    pair.accept_with_responding_inviter(&imported.invitation_id)
        .await
        .unwrap();

    let committed = pair
        .sender_effects
        .load_committed_facts(sender_id)
        .await
        .unwrap();

    let mut found = false;
    for fact in committed {
        let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = fact.content
        else {
            continue;
        };

        if envelope.type_id.as_str() != CONTACT_FACT_TYPE_ID {
            continue;
        }

        let Some(ContactFact::Added {
            owner_id,
            contact_id,
            nickname,
            ..
        }) = ContactFact::from_envelope(&envelope)
        else {
            continue;
        };
        if owner_id == sender_id
            && contact_id == receiver_id
            && nickname == receiver_id.to_string()
        {
            found = true;
            break;
        }
    }
    assert!(
        found,
        "expected sender-side ContactFact::Added for receiver"
    );
});

#[tokio::test]
async fn creating_contact_invitation_materializes_sender_contact() {
    let sender_id = AuthorityId::new_from_entropy([128u8; 32]);
    let receiver_id = AuthorityId::new_from_entropy([129u8; 32]);
    let config = AgentConfig::default();
    let effects = Arc::new(
        AuraEffectSystem::simulation_for_test_for_authority(&config, sender_id).unwrap(),
    );
    let handler = handler_for_id(sender_id);

    handler
        .create_invitation(
            effects.clone(),
            receiver_id,
            InvitationType::Contact { nickname: None },
            Some("Contact invitation".to_string()),
            None,
        )
        .await
        .unwrap();

    let committed = effects.load_committed_facts(sender_id).await.unwrap();
    let mut found = false;
    for fact in committed {
        let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = fact.content
        else {
            continue;
        };
        if envelope.type_id.as_str() != CONTACT_FACT_TYPE_ID {
            continue;
        }
        let Some(ContactFact::Added {
            owner_id,
            contact_id,
            ..
        }) = ContactFact::from_envelope(&envelope)
        else {
            continue;
        };
        if owner_id == sender_id && contact_id == receiver_id {
            found = true;
            break;
        }
    }

    assert!(
        found,
        "expected ContactFact::Added for sender invitation recipient"
    );
}

large_stack_async_test!(contact_acceptance_processing_skips_unrelated_envelopes, {
    let pair = contact_pair(126).await;
    let (sender_id, receiver_id) = (pair.sender_id, pair.receiver_id);
    let invitation = pair.create_contact_invitation().await;

    // Queue a large unrelated backlog ahead of the acceptance. This guards
    // against starvation when inbox scanning encounters many unknown
    // content-types before actionable invitation envelopes.
    for _ in 0..300 {
        let mut metadata = HashMap::new();
        metadata.insert(
            "content-type".to_string(),
            "application/aura-unrelated".to_string(),
        );
        send_invitation_test_raw_envelope(
            &pair.receiver_effects,
            TransportEnvelope {
                destination: sender_id,
                source: receiver_id,
                context: default_context_id_for_authority(sender_id),
                payload: b"noop".to_vec(),
                metadata,
                receipt: None,
            },
        )
        .await
        .unwrap();
    }

    let imported = pair.import(&pair.signed_code(&invitation).await).await;
    let result = pair
        .accept_with_responding_inviter(&imported.invitation_id)
        .await
        .unwrap();
    assert_eq!(result.new_status, InvitationStatus::Accepted);
});

large_stack_async_test!(contact_acceptance_processing_seeds_peer_default_descriptor_from_local_context, {
    let shared_transport = crate::runtime::SharedTransport::new();
    let config = AgentConfig::default();

    let sender_id = AuthorityId::new_from_entropy([198u8; 32]);
    let receiver_id = AuthorityId::new_from_entropy([199u8; 32]);

    let sender_effects =
        crate::testing::simulation_effect_system_with_shared_transport_for_authority_arc(
            &config,
            sender_id,
            shared_transport.clone(),
        );
    let receiver_effects =
        crate::testing::simulation_effect_system_with_shared_transport_for_authority_arc(
            &config,
            receiver_id,
            shared_transport,
        );

    let sender_handler = handler_for_id(sender_id);
    let _sender_rendezvous_tasks =
        attach_test_rendezvous_manager(sender_effects.as_ref(), sender_id).await;
    let sender_manager = sender_effects
        .rendezvous_manager()
        .expect("sender rendezvous manager should be attached");
    let now_ms: u64 = 1_700_000_000_000;
    let sender_local_context = default_context_id_for_authority(sender_id);
    let receiver_peer_context = default_context_id_for_authority(receiver_id);
    sender_manager
        .cache_descriptor(RendezvousDescriptor {
            authority_id: receiver_id,
            device_id: None,
            context_id: sender_local_context,
            transport_hints: vec![TransportHint::tcp_direct("127.0.0.1:55041").unwrap()],
            handshake_psk_commitment: [41u8; 32],
            public_key: [42u8; 32],
            valid_from: now_ms.saturating_sub(1),
            valid_until: now_ms.saturating_add(86_400_000),
            nonce: [43u8; 32],
            nickname_suggestion: None,
        })
        .await
        .unwrap();
    assert!(
        sender_manager
            .get_descriptor(receiver_peer_context, receiver_id)
            .await
            .is_none(),
        "peer-default descriptor should start unset"
    );

    let invitation = sender_handler
        .create_invitation(
            sender_effects.clone(),
            receiver_id,
            InvitationType::Contact { nickname: None },
            Some("Contact invitation from sender".to_string()),
            None,
        )
        .await
        .unwrap();
    let acceptance = ContactInvitationAcceptance {
        invitation_id: invitation.invitation_id.clone(),
        acceptor_id: receiver_id,
        signature: sign_test_contact_acceptance(&receiver_effects, &invitation, receiver_id).await,
        nickname_suggestion: None,
    };
    let payload = serde_json::to_vec(&acceptance).unwrap();
    let mut metadata = HashMap::new();
    metadata.insert(
        "content-type".to_string(),
        CONTACT_INVITATION_ACCEPTANCE_CONTENT_TYPE.to_string(),
    );
    metadata.insert(
        "invitation-id".to_string(),
        invitation.invitation_id.to_string(),
    );
    metadata.insert("acceptor-id".to_string(), receiver_id.to_string());

    send_invitation_test_verified_envelope(
        &sender_effects,
        TransportEnvelope {
            destination: sender_id,
            source: receiver_id,
            context: default_context_id_for_authority(sender_id),
            payload,
            metadata,
            receipt: None,
        },
    )
    .await
    .unwrap();

    let processed = sender_handler
        .process_contact_invitation_acceptances(sender_effects.clone())
        .await
        .unwrap();
    assert!(processed >= 1);

    let descriptor = sender_manager
        .get_descriptor(receiver_peer_context, receiver_id)
        .await
        .expect("processed contact acceptance should seed peer-default descriptor");
    assert!(matches!(
        descriptor.transport_hints.as_slice(),
        [TransportHint::TcpDirect { addr, .. }] if addr.to_string() == "127.0.0.1:55041"
    ));
    assert_eq!(descriptor.handshake_psk_commitment, [41u8; 32]);
    assert_eq!(descriptor.public_key, [42u8; 32]);
    assert_eq!(descriptor.nonce, [43u8; 32]);
});

large_stack_async_test!(invite_to_channel_imports_pending_invitation_in_harness_mode, {
    let harness_mode_env = crate::runtime_bridge::harness_mode_env_key_for_tests();
    let _env_restore = EnvRestore::capture(&[harness_mode_env]);
    std::env::set_var(harness_mode_env, "1");

    let shared_transport = crate::runtime::SharedTransport::new();
    let config = AgentConfig::default();
    let sender_id = AuthorityId::new_from_entropy([200u8; 32]);
    let receiver_id = AuthorityId::new_from_entropy([201u8; 32]);
    let sender_effects = Arc::new(
        AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
            &config,
            sender_id,
            shared_transport.clone(),
        )
        .unwrap(),
    );
    let receiver_effects = Arc::new(
        AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
            &config,
            receiver_id,
            shared_transport,
        )
        .unwrap(),
    );

    let sender_service =
        invitation_service_for(AuthorityContext::new(sender_id), sender_effects.clone());
    let receiver_handler = handler_for_id(receiver_id);

    let context_id = ContextId::new_from_entropy([202u8; 32]);
    let channel_id = ChannelId::from_bytes(hash(b"harness-mode-channel-invite-import"));
    sender_effects
        .create_channel(ChannelCreateParams {
            context: context_id,
            channel: Some(channel_id),
            skip_window: None,
            topic: None,
        })
        .await
        .unwrap();
    sender_effects
        .join_channel(ChannelJoinParams {
            context: context_id,
            channel: channel_id,
            participant: sender_id,
        })
        .await
        .unwrap();

    let invitation = sender_service
        .invite_to_channel(
            receiver_id,
            channel_id.to_string(),
            Some(context_id),
            Some("shared-parity-lab".to_string()),
            None,
            None,
            None,
        )
        .await
        .unwrap();

    let mut imported = None;
    for _ in 0..20 {
        let _ = receiver_handler
            .process_contact_invitation_acceptances(receiver_effects.clone())
            .await
            .unwrap();
        let pending = receiver_handler
            .list_pending_with_storage(receiver_effects.as_ref())
            .await;
        if let Some(found) = pending
            .into_iter()
            .find(|candidate| candidate.invitation_id == invitation.invitation_id)
        {
            imported = Some(found);
            break;
        }
        sleep(Duration::from_millis(100)).await;
    }

    let imported = imported.expect("receiver should import the pending channel invitation");
    assert!(matches!(imported.invitation_type, InvitationType::Channel { .. }));
    assert_eq!(imported.status, InvitationStatus::Pending);
    assert_eq!(imported.sender_id, sender_id);
    assert_eq!(imported.receiver_id, receiver_id);
});

large_stack_async_test!(contact_acceptance_processing_commits_chat_fact_envelopes, {
    let authority = AuthorityId::new_from_entropy([201u8; 32]);
    let peer = AuthorityId::new_from_entropy([202u8; 32]);
    let config = AgentConfig::default();
    let effects = Arc::new(
        AuraEffectSystem::simulation_for_test_for_authority(&config, authority).unwrap(),
    );
    let handler = InvitationHandler::new(AuthorityContext::new(authority)).unwrap();

    let context_id = ContextId::new_from_entropy([203u8; 32]);
    let channel_id = ChannelId::from_bytes([204u8; 32]);
    let chat_fact = ChatFact::channel_created_ms(
        context_id,
        channel_id,
        "dm".to_string(),
        Some("Direct messages".to_string()),
        true,
        1_700_000_000_000,
        peer,
    )
    .to_generic();

    let payload = aura_core::util::serialization::to_vec(&chat_fact).unwrap();
    let mut metadata = HashMap::new();
    metadata.insert(
        "content-type".to_string(),
        CHAT_FACT_CONTENT_TYPE.to_string(),
    );

    send_invitation_test_verified_envelope(
        &effects,
        TransportEnvelope {
            destination: authority,
            source: peer,
            context: context_id,
            payload,
            metadata,
            receipt: None,
        },
    )
    .await
    .unwrap();

    let processed = handler
        .process_contact_invitation_acceptances(effects.clone())
        .await
        .unwrap();
    assert_eq!(processed, 1);

    let committed = effects.load_committed_facts(authority).await.unwrap();
    let mut found = false;
    for fact in committed {
        let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = fact.content
        else {
            continue;
        };
        if envelope.type_id.as_str() != CHAT_FACT_TYPE_ID {
            continue;
        }
        let Some(ChatFact::ChannelCreated {
            channel_id: seen, ..
        }) = ChatFact::from_envelope(&envelope)
        else {
            continue;
        };
        if seen == channel_id {
            found = true;
            break;
        }
    }

    assert!(found, "expected committed chat fact from inbound envelope");
});

large_stack_async_test!(contact_acceptance_processing_commits_non_chat_relational_fact_envelopes, {
    let authority = AuthorityId::new_from_entropy([205u8; 32]);
    let peer = AuthorityId::new_from_entropy([206u8; 32]);
    let config = AgentConfig::default();
    let effects = Arc::new(
        AuraEffectSystem::simulation_for_test_for_authority(&config, authority).unwrap(),
    );
    let handler = InvitationHandler::new(AuthorityContext::new(authority)).unwrap();

    let context_id = ContextId::new_from_entropy([207u8; 32]);
    let grant = HomeGrantModeratorFact::new_ms(context_id, authority, peer, 1_700_000_000_001)
        .to_generic();

    let payload = aura_core::util::serialization::to_vec(&grant).unwrap();
    let mut metadata = HashMap::new();
    metadata.insert(
        "content-type".to_string(),
        CHAT_FACT_CONTENT_TYPE.to_string(),
    );

    send_invitation_test_verified_envelope(
        &effects,
        TransportEnvelope {
            destination: authority,
            source: peer,
            context: context_id,
            payload,
            metadata,
            receipt: None,
        },
    )
    .await
    .unwrap();

    let processed = handler
        .process_contact_invitation_acceptances(effects.clone())
        .await
        .unwrap();
    assert_eq!(processed, 1);

    let committed = effects.load_committed_facts(authority).await.unwrap();
    let mut found = false;
    for fact in committed {
        let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = fact.content
        else {
            continue;
        };
        let Some(grant_fact) = HomeGrantModeratorFact::from_envelope(&envelope) else {
            continue;
        };
        if grant_fact.target_authority == authority && grant_fact.actor_authority == peer {
            found = true;
            break;
        }
    }

    assert!(
        found,
        "expected committed non-chat relational fact from inbound envelope"
    );
});

large_stack_async_test!(channel_acceptance_processing_marks_created_invitation_accepted_for_sender, {
    let sender_id = AuthorityId::new_from_entropy([207u8; 32]);
    let receiver_id = AuthorityId::new_from_entropy([208u8; 32]);
    let config = AgentConfig::default();
    let shared_transport = crate::runtime::SharedTransport::new();
    let sender_effects = Arc::new(
        AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
            &config,
            sender_id,
            shared_transport.clone(),
        )
        .unwrap(),
    );
    let receiver_effects = Arc::new(
        AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
            &config,
            receiver_id,
            shared_transport,
        )
        .unwrap(),
    );
    let sender_context = AuthorityContext::new(sender_id);
    let sender_handler = handler_for(sender_context.clone());
    let receiver_handler = handler_for_id(receiver_id);
    let sender_service = invitation_service_for(sender_context, sender_effects.clone());

    let sender_manager = crate::runtime::services::RendezvousManager::new_with_default_udp(
        sender_id,
        crate::runtime::services::RendezvousManagerConfig::default(),
        Arc::new(sender_effects.time_effects().clone()),
    );
    sender_effects.attach_rendezvous_manager(sender_manager.clone());
    let sender_service_context = crate::runtime::services::RuntimeServiceContext::new(
        Arc::new(crate::runtime::TaskSupervisor::new()),
        Arc::new(sender_effects.time_effects().clone()),
    );
    crate::runtime::services::RuntimeService::start(&sender_manager, &sender_service_context)
        .await
        .unwrap();

    let receiver_manager = crate::runtime::services::RendezvousManager::new_with_default_udp(
        receiver_id,
        crate::runtime::services::RendezvousManagerConfig::default(),
        Arc::new(receiver_effects.time_effects().clone()),
    );
    receiver_effects.attach_rendezvous_manager(receiver_manager.clone());
    let receiver_service_context = crate::runtime::services::RuntimeServiceContext::new(
        Arc::new(crate::runtime::TaskSupervisor::new()),
        Arc::new(receiver_effects.time_effects().clone()),
    );
    crate::runtime::services::RuntimeService::start(
        &receiver_manager,
        &receiver_service_context,
    )
    .await
    .unwrap();

    register_test_app_signals(sender_effects.as_ref()).await;
    register_test_app_signals(receiver_effects.as_ref()).await;

    let now_ms = 1_700_000_000_000;
    sender_handler
        .cache_verified_peer_descriptor_for_peer(
            sender_effects.as_ref(),
            receiver_id,
            None,
            Some("tcp://127.0.0.1:55021"),
            now_ms,
        )
        .await;
    receiver_handler
        .cache_verified_peer_descriptor_for_peer(
            receiver_effects.as_ref(),
            sender_id,
            None,
            Some("tcp://127.0.0.1:55022"),
            now_ms,
        )
        .await;

    let context_id = ContextId::new_from_entropy([209u8; 32]);
    let channel_id = ChannelId::from_bytes(hash(b"channel-acceptance-sender-propagation"));
    sender_effects
        .create_channel(ChannelCreateParams {
            context: context_id,
            channel: Some(channel_id),
            skip_window: None,
            topic: None,
        })
        .await
        .unwrap();
    sender_effects
        .join_channel(ChannelJoinParams {
            context: context_id,
            channel: channel_id,
            participant: sender_id,
        })
        .await
        .unwrap();

    let invitation = sender_service
        .invite_to_channel(
            receiver_id,
            channel_id.to_string(),
            Some(context_id),
            Some("shared-parity-lab".to_string()),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    let code = unsigned_test_code_for_invitation(&invitation);
    let imported = receiver_handler
        .import_invitation_code(&receiver_effects, &code)
        .await
        .unwrap();

    receiver_handler
        .accept_invitation(receiver_effects.clone(), &imported.invitation_id)
        .await
        .unwrap();
    let acceptance = ChannelInvitationAcceptance {
        invitation_id: imported.invitation_id.clone(),
        acceptor_id: receiver_id,
        context_id,
        channel_id,
        channel_name: Some("shared-parity-lab".to_string()),
        signature: sign_test_channel_acceptance(
            &receiver_effects,
            &invitation,
            receiver_id,
            context_id,
            channel_id,
            Some("shared-parity-lab".to_string()),
        )
        .await,
    };
    let payload = serde_json::to_vec(&acceptance).unwrap();
    let mut metadata = HashMap::new();
    metadata.insert(
        "content-type".to_string(),
        CHANNEL_INVITATION_ACCEPTANCE_CONTENT_TYPE.to_string(),
    );
    metadata.insert(
        "invitation-id".to_string(),
        imported.invitation_id.to_string(),
    );
    metadata.insert("acceptor-id".to_string(), receiver_id.to_string());
    metadata.insert("channel-id".to_string(), channel_id.to_string());
    send_invitation_test_verified_envelope(
        &sender_effects,
        TransportEnvelope {
            destination: sender_id,
            source: receiver_id,
            context: default_context_id_for_authority(sender_id),
            payload,
            metadata,
            receipt: None,
        },
    )
    .await
    .unwrap();

    let processed = sender_handler
        .process_contact_invitation_acceptances(sender_effects.clone())
        .await
        .unwrap();
    assert!(processed >= 1);

    let stored = InvitationHandler::load_created_invitation(
        sender_effects.as_ref(),
        sender_id,
        &invitation.invitation_id,
    )
    .await
    .expect("created invitation should remain accessible");
    assert_eq!(stored.status, InvitationStatus::Accepted);
});

large_stack_async_test!(channel_acceptance_notification_transports_and_updates_sender_state, {
    let sender_id = AuthorityId::new_from_entropy([221u8; 32]);
    let receiver_id = AuthorityId::new_from_entropy([222u8; 32]);
    let config = AgentConfig::default();
    let shared_transport = crate::runtime::SharedTransport::new();
    let sender_effects = Arc::new(
        AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
            &config,
            sender_id,
            shared_transport.clone(),
        )
        .unwrap(),
    );
    let receiver_effects = Arc::new(
        AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
            &config,
            receiver_id,
            shared_transport,
        )
        .unwrap(),
    );
    let sender_context = AuthorityContext::new(sender_id);
    let sender_handler = handler_for(sender_context.clone());
    let receiver_handler = handler_for_id(receiver_id);
    let sender_service = invitation_service_for(sender_context, sender_effects.clone());

    let sender_manager = crate::runtime::services::RendezvousManager::new_with_default_udp(
        sender_id,
        crate::runtime::services::RendezvousManagerConfig::default(),
        Arc::new(sender_effects.time_effects().clone()),
    );
    sender_effects.attach_rendezvous_manager(sender_manager.clone());
    let sender_service_context = crate::runtime::services::RuntimeServiceContext::new(
        Arc::new(crate::runtime::TaskSupervisor::new()),
        Arc::new(sender_effects.time_effects().clone()),
    );
    crate::runtime::services::RuntimeService::start(&sender_manager, &sender_service_context)
        .await
        .unwrap();

    let receiver_manager = crate::runtime::services::RendezvousManager::new_with_default_udp(
        receiver_id,
        crate::runtime::services::RendezvousManagerConfig::default(),
        Arc::new(receiver_effects.time_effects().clone()),
    );
    receiver_effects.attach_rendezvous_manager(receiver_manager.clone());
    let receiver_service_context = crate::runtime::services::RuntimeServiceContext::new(
        Arc::new(crate::runtime::TaskSupervisor::new()),
        Arc::new(receiver_effects.time_effects().clone()),
    );
    crate::runtime::services::RuntimeService::start(
        &receiver_manager,
        &receiver_service_context,
    )
    .await
    .unwrap();

    register_test_app_signals(sender_effects.as_ref()).await;
    register_test_app_signals(receiver_effects.as_ref()).await;

    let now_ms = 1_700_000_000_000;
    sender_handler
        .cache_verified_peer_descriptor_for_peer(
            sender_effects.as_ref(),
            receiver_id,
            None,
            Some("tcp://127.0.0.1:55002"),
            now_ms,
        )
        .await;
    receiver_handler
        .cache_verified_peer_descriptor_for_peer(
            receiver_effects.as_ref(),
            sender_id,
            None,
            Some("tcp://127.0.0.1:55001"),
            now_ms,
        )
        .await;

    let context_id = ContextId::new_from_entropy([223u8; 32]);
    let channel_id = ChannelId::from_bytes(hash(b"channel-acceptance-real-transport"));
    sender_effects
        .create_channel(ChannelCreateParams {
            context: context_id,
            channel: Some(channel_id),
            skip_window: None,
            topic: None,
        })
        .await
        .unwrap();
    sender_effects
        .join_channel(ChannelJoinParams {
            context: context_id,
            channel: channel_id,
            participant: sender_id,
        })
        .await
        .unwrap();

    // The channel is the sender's own home, so this is a home invitation and
    // the acceptance materializes home membership.
    sender_effects
        .commit_generic_fact_bytes(
            context_id,
            aura_social::SOCIAL_FACT_TYPE_ID.into(),
            aura_social::SocialFact::home_created_ms(
                aura_social::HomeId::from_bytes(*channel_id.as_bytes()),
                context_id,
                1,
                sender_id,
                "shared-parity-lab".to_string(),
            )
            .to_bytes(),
        )
        .await
        .unwrap();
    let invitation = sender_service
        .invite_to_channel(
            receiver_id,
            channel_id.to_string(),
            Some(context_id),
            Some("shared-parity-lab".to_string()),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    let code = unsigned_test_code_for_invitation(&invitation);
    let imported = receiver_handler
        .import_invitation_code(&receiver_effects, &code)
        .await
        .unwrap();
    receiver_handler
        .accept_invitation(receiver_effects.clone(), &imported.invitation_id)
        .await
        .unwrap();
    bootstrap_test_signing_authority(&receiver_effects, receiver_id).await;
    receiver_handler
        .notify_channel_invitation_acceptance(
            receiver_effects.as_ref(),
            &imported.invitation_id,
        )
        .await
        .unwrap();

    let processed = sender_handler
        .process_contact_invitation_acceptances(sender_effects.clone())
        .await
        .unwrap();
    assert!(processed >= 1);

    let stored = InvitationHandler::load_created_invitation(
        sender_effects.as_ref(),
        sender_id,
        &invitation.invitation_id,
    )
    .await
    .expect("created invitation should remain accessible");
    assert_eq!(stored.status, InvitationStatus::Accepted);

    use aura_effects::ReactiveEffects;
    let homes: HomesState = sender_effects
        .reactive_handler()
        .read(&*HOMES_SIGNAL)
        .await
        .unwrap();
    let home = homes
        .home_state(&channel_id)
        .expect("sender should materialize channel acceptance home state");
    assert_eq!(home.context_id, Some(context_id));
    assert!(
        home.member(&receiver_id).is_some(),
        "sender home state should include receiver after transported acceptance"
    );

    let committed = sender_effects
        .load_committed_facts(sender_id)
        .await
        .unwrap();
    let updated_channel_projection = committed.iter().any(|fact| {
        let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = &fact.content
        else {
            return false;
        };
        matches!(
            ChatFact::from_envelope(envelope),
            Some(ChatFact::ChannelUpdated {
                context_id: seen_context,
                channel_id: seen_channel,
                name: Some(name),
                member_count: Some(2),
                member_ids: Some(member_ids),
                ..
            }) if seen_context == context_id
                && seen_channel == channel_id
                && name == "shared-parity-lab"
                && member_ids.as_slice() == [receiver_id]
        )
    });
    assert!(
        updated_channel_projection,
        "sender should publish a canonical ChannelUpdated projection after transported acceptance"
    );
});

#[tokio::test]
async fn cache_peer_descriptor_promotes_fresh_explicit_transport_hints() {
    let authority_id = AuthorityId::new_from_entropy([225u8; 32]);
    let peer_id = AuthorityId::new_from_entropy([226u8; 32]);
    let config = AgentConfig::default();
    let effects = Arc::new(
        AuraEffectSystem::simulation_for_test_for_authority(&config, authority_id).unwrap(),
    );
    let handler = InvitationHandler::new(AuthorityContext::new(authority_id)).unwrap();
    let manager = crate::runtime::services::RendezvousManager::new_with_default_udp(
        authority_id,
        crate::runtime::services::RendezvousManagerConfig::default(),
        Arc::new(effects.time_effects().clone()),
    );
    effects.attach_rendezvous_manager(manager.clone());

    let now_ms = 1_700_000_000_000;
    handler
        .cache_peer_descriptor_for_peer(
            effects.as_ref(),
            peer_id,
            None,
            Some("ws://127.0.0.1:4173"),
            now_ms,
        )
        .await;
    handler
        .cache_peer_descriptor_for_peer(
            effects.as_ref(),
            peer_id,
            None,
            Some("ws://127.0.0.1:43011"),
            now_ms + 1,
        )
        .await;

    let peer_descriptor = manager
        .get_descriptor(default_context_id_for_authority(peer_id), peer_id)
        .await;
    assert!(
        peer_descriptor.is_none(),
        "unauthenticated invitation sender hints must not seed peer-context routing"
    );

    let local_descriptor = manager
        .get_descriptor(default_context_id_for_authority(authority_id), peer_id)
        .await;
    assert!(
        local_descriptor.is_none(),
        "unauthenticated invitation sender hints must not seed local-context routing"
    );
}

#[tokio::test]
async fn cache_peer_descriptor_ignores_unauthenticated_hints_in_harness_mode() {
    let harness_mode_env = crate::runtime_bridge::harness_mode_env_key_for_tests();
    let _env_restore = EnvRestore::capture(&[harness_mode_env]);
    std::env::set_var(harness_mode_env, "1");

    let authority_id = AuthorityId::new_from_entropy([227u8; 32]);
    let peer_id = AuthorityId::new_from_entropy([228u8; 32]);
    let config = AgentConfig::default();
    let effects = Arc::new(
        AuraEffectSystem::simulation_for_test_for_authority(&config, authority_id).unwrap(),
    );
    let handler = InvitationHandler::new(AuthorityContext::new(authority_id)).unwrap();
    let manager = crate::runtime::services::RendezvousManager::new_with_default_udp(
        authority_id,
        crate::runtime::services::RendezvousManagerConfig::default(),
        Arc::new(effects.time_effects().clone()),
    );
    effects.attach_rendezvous_manager(manager.clone());

    let now_ms = 1_700_000_000_000;
    handler
        .cache_peer_descriptor_for_peer(
            effects.as_ref(),
            peer_id,
            None,
            Some("ws://127.0.0.1:43011"),
            now_ms,
        )
        .await;

    let peer_descriptor = manager
        .get_descriptor(default_context_id_for_authority(peer_id), peer_id)
        .await;
    assert!(
        peer_descriptor.is_none(),
        "harness mode must not turn unauthenticated sender hints into routing descriptors"
    );

    let local_descriptor = manager
        .get_descriptor(default_context_id_for_authority(authority_id), peer_id)
        .await;
    assert!(
        local_descriptor.is_none(),
        "harness mode must also avoid seeding local-context routing"
    );
}

#[tokio::test]
async fn import_channel_invitation_requires_authoritative_context() {
    let receiver_id = AuthorityId::new_from_entropy([217u8; 32]);
    let config = AgentConfig::default();
    let effects = Arc::new(
        AuraEffectSystem::simulation_for_test_for_authority(&config, receiver_id).unwrap(),
    );
    let handler = InvitationHandler::new(AuthorityContext::new(receiver_id)).unwrap();

    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-missing-channel-context"),
        sender_id: AuthorityId::new_from_entropy([218u8; 32]),
        context_id: None,
        invitation_type: InvitationType::Channel {
            home_id: canonical_home_id(18),
            nickname_suggestion: Some("No Context House".to_string()),
            bootstrap: None,
            home: false,
        },
        expires_at: None,
        message: Some("Join No Context House".to_string()),
    };

    let error = handler
        .import_invitation_code(
            effects.as_ref(),
            &shareable
                .to_code()
                .expect("shareable invitation should serialize"),
        )
        .await
        .expect_err("channel invitation import must require authoritative context");

    assert!(error.to_string().contains("missing authoritative context"));
}

#[tokio::test]
async fn channel_acceptance_notification_surfaces_peer_channel_establishment_failure() {
    let sender_id = AuthorityId::new_from_entropy([219u8; 32]);
    let receiver_id = AuthorityId::new_from_entropy([220u8; 32]);
    let config = AgentConfig::default();
    let effects = Arc::new(
        AuraEffectSystem::simulation_for_test_for_authority(&config, receiver_id).unwrap(),
    );
    let handler = InvitationHandler::new(AuthorityContext::new(receiver_id)).unwrap();
    register_test_app_signals(effects.as_ref()).await;
    let _rendezvous_tasks = attach_test_rendezvous_manager(effects.as_ref(), receiver_id).await;
    cache_test_peer_descriptor(
        effects.as_ref(),
        receiver_id,
        sender_id,
        "tcp://127.0.0.1:55118",
        1_700_000_000_000,
    )
    .await;

    let invitation_context = ContextId::new_from_entropy([56u8; 32]);
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-channel-context-strict"),
        sender_id,
        context_id: Some(invitation_context),
        invitation_type: InvitationType::Channel {
            home_id: canonical_home_id(19),
            nickname_suggestion: Some("Context Strict House".to_string()),
            bootstrap: None,
            home: false,
        },
        expires_at: None,
        message: Some("Join Context Strict House".to_string()),
    };

    let imported = handler
        .import_invitation_code(
            effects.as_ref(),
            &shareable
                .to_code()
                .expect("shareable invitation should serialize"),
        )
        .await
        .expect("channel invitation import should succeed");
    let channel_invite = handler
        .resolve_channel_invitation(effects.as_ref(), &imported.invitation_id)
        .await
        .expect("channel invitation resolution should succeed")
        .expect("channel invitation should remain available");
    handler
        .materialize_channel_invitation_acceptance(effects.as_ref(), &channel_invite)
        .await
        .expect("channel invitation accept should succeed locally");
    {
        use aura_effects::ReactiveEffects;
        let homes: HomesState = effects
            .reactive_handler()
            .read(&*HOMES_SIGNAL)
            .await
            .unwrap();
        assert!(
            homes.home_state(&canonical_home_id(19)).is_none(),
            "accepting a plain channel invitation must not create a home"
        );
    }
    bootstrap_test_signing_authority(&effects, receiver_id).await;

    let error = handler
        .notify_channel_invitation_acceptance(effects.as_ref(), &imported.invitation_id)
        .await
        .expect_err("notification must not fall back to sender default context");

    assert!(matches!(
        error,
        AgentError::Runtime(_) | AgentError::Effects(_)
    ));
}

#[tokio::test]
async fn channel_acceptance_notification_uses_materialized_channel_context() {
    let sender_id = AuthorityId::new_from_entropy([221u8; 32]);
    let receiver_id = AuthorityId::new_from_entropy([222u8; 32]);
    let config = AgentConfig::default();
    let shared_transport = crate::runtime::SharedTransport::new();
    let sender_effects = Arc::new(
        AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
            &config,
            sender_id,
            shared_transport.clone(),
        )
        .unwrap(),
    );
    let receiver_effects = Arc::new(
        AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
            &config,
            receiver_id,
            shared_transport,
        )
        .unwrap(),
    );
    register_test_app_signals(sender_effects.as_ref()).await;
    register_test_app_signals(receiver_effects.as_ref()).await;
    let _sender_rendezvous =
        attach_test_rendezvous_manager(sender_effects.as_ref(), sender_id).await;
    let _receiver_rendezvous =
        attach_test_rendezvous_manager(receiver_effects.as_ref(), receiver_id).await;
    cache_test_peer_descriptor(
        sender_effects.as_ref(),
        sender_id,
        receiver_id,
        "tcp://127.0.0.1:55119",
        1_700_000_000_000,
    )
    .await;
    cache_test_peer_descriptor(
        receiver_effects.as_ref(),
        receiver_id,
        sender_id,
        "tcp://127.0.0.1:55120",
        1_700_000_000_000,
    )
    .await;

    let handler = InvitationHandler::new(AuthorityContext::new(receiver_id)).unwrap();
    let invitation_context = ContextId::new_from_entropy([57u8; 32]);
    let materialized_context = default_context_id_for_authority(sender_id);
    let home_id = canonical_home_id(20);
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-channel-context-materialized"),
        sender_id,
        context_id: Some(invitation_context),
        invitation_type: InvitationType::Channel {
            home_id,
            nickname_suggestion: Some("Materialized Context House".to_string()),
            bootstrap: None,
            home: false,
        },
        expires_at: None,
        message: Some("Join Materialized Context House".to_string()),
    };

    receiver_effects
        .commit_relational_facts(vec![ChatFact::channel_created_ms(
            materialized_context,
            home_id,
            "Materialized Context House".to_string(),
            Some(format!("Home channel {}", home_id)),
            false,
            1_700_000_000_100,
            sender_id,
        )
        .to_generic()])
        .await
        .unwrap();

    let imported = handler
        .import_invitation_code(
            receiver_effects.as_ref(),
            &shareable
                .to_code()
                .expect("shareable invitation should serialize"),
        )
        .await
        .expect("channel invitation import should succeed");
    bootstrap_test_signing_authority(&receiver_effects, receiver_id).await;

    handler
        .notify_channel_invitation_acceptance(
            receiver_effects.as_ref(),
            &imported.invitation_id,
        )
        .await
        .expect("notification should use the materialized channel context");

    let received = timeout(Duration::from_secs(5), async {
        loop {
            let envelope = sender_effects
                .receive_envelope()
                .await
                .expect("receiver notification should arrive");
            if envelope.metadata.get("content-type").map(String::as_str)
                == Some(CHANNEL_INVITATION_ACCEPTANCE_CONTENT_TYPE)
            {
                break envelope;
            }
        }
    })
    .await
    .expect("timed out waiting for channel acceptance envelope");

    assert_eq!(received.context, materialized_context);
}

large_stack_async_test!(contact_acceptance_processing_provisions_amp_state_for_channel_created_facts, {
    let authority = AuthorityId::new_from_entropy([208u8; 32]);
    let peer = AuthorityId::new_from_entropy([209u8; 32]);
    let config = AgentConfig::default();
    let effects = Arc::new(
        AuraEffectSystem::simulation_for_test_for_authority(&config, authority).unwrap(),
    );
    let handler = InvitationHandler::new(AuthorityContext::new(authority)).unwrap();

    let context_id = ContextId::new_from_entropy([210u8; 32]);
    let channel_id = ChannelId::from_bytes([211u8; 32]);
    let chat_fact = ChatFact::channel_created_ms(
        context_id,
        channel_id,
        "provisioned".to_string(),
        Some("Provisioned channel".to_string()),
        false,
        1_700_000_000_100,
        peer,
    )
    .to_generic();

    let payload = aura_core::util::serialization::to_vec(&chat_fact).unwrap();
    let mut metadata = HashMap::new();
    metadata.insert(
        "content-type".to_string(),
        CHAT_FACT_CONTENT_TYPE.to_string(),
    );

    send_invitation_test_verified_envelope(
        &effects,
        TransportEnvelope {
            destination: authority,
            source: peer,
            context: context_id,
            payload,
            metadata,
            receipt: None,
        },
    )
    .await
    .unwrap();

    let processed = handler
        .process_contact_invitation_acceptances(effects.clone())
        .await
        .unwrap();
    assert_eq!(processed, 1);

    timeout(Duration::from_secs(5), async {
        loop {
            if aura_protocol::amp::get_channel_state(effects.as_ref(), context_id, channel_id)
                .await
                .is_ok()
            {
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("timed out waiting for provisioned AMP channel state");
});

#[tokio::test]
async fn invitation_envelope_processing_imports_pending_channel_invites() {
    let sender_id = AuthorityId::new_from_entropy([211u8; 32]);
    let receiver_id = AuthorityId::new_from_entropy([212u8; 32]);
    let config = AgentConfig::default();
    let effects = Arc::new(
        AuraEffectSystem::simulation_for_test_for_authority(&config, receiver_id).unwrap(),
    );
    register_app_signals(&effects.reactive_handler())
        .await
        .expect("app signals should register");

    let receiver_handler = InvitationHandler::new(AuthorityContext::new(receiver_id)).unwrap();

    let invitation_id = InvitationId::new("inv-envelope-home-1");
    let home_id = canonical_home_id(12);
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: invitation_id.clone(),
        sender_id,
        context_id: Some(default_context_id_for_authority(sender_id)),
        invitation_type: InvitationType::Channel {
            home_id,
            nickname_suggestion: Some("Maple House".to_string()),
            bootstrap: None,
            home: false,
        },
        expires_at: None,
        message: Some("Join Maple House".to_string()),
    };

    let mut metadata = HashMap::new();
    metadata.insert(
        "content-type".to_string(),
        INVITATION_CONTENT_TYPE.to_string(),
    );
    metadata.insert("invitation-id".to_string(), invitation_id.to_string());
    metadata.insert(
        "invitation-context".to_string(),
        default_context_id_for_authority(sender_id).to_string(),
    );

    send_invitation_test_verified_envelope(
        &effects,
        TransportEnvelope {
            destination: receiver_id,
            source: sender_id,
            context: default_context_id_for_authority(sender_id),
            payload: shareable
                .to_code()
                .expect("shareable invitation should serialize")
                .into_bytes(),
            metadata,
            receipt: None,
        },
    )
    .await
    .unwrap();

    let processed = receiver_handler
        .process_contact_invitation_acceptances(effects.clone())
        .await
        .unwrap();
    assert_eq!(processed, 1);

    let fresh_handler = InvitationHandler::new(AuthorityContext::new(receiver_id)).unwrap();
    let pending = fresh_handler
        .list_pending_with_storage(effects.as_ref())
        .await;
    let found = pending.iter().any(|inv| {
        inv.invitation_id == invitation_id
            && matches!(inv.invitation_type, InvitationType::Channel { .. })
            && inv.status == InvitationStatus::Pending
            && inv.sender_id == sender_id
            && inv.receiver_id == receiver_id
    });
    assert!(
        found,
        "expected imported channel invitation to appear in pending list"
    );

    let invitations = effects
        .reactive_handler()
        .read(&*INVITATIONS_SIGNAL)
        .await
        .expect("invitation signal should be registered");
    assert!(invitations.all_pending().iter().any(|inv| {
        inv.id == invitation_id.to_string()
            && inv.direction == aura_app::views::invitations::InvitationDirection::Received
            && inv.status == aura_app::views::invitations::InvitationStatus::Pending
    }));
}

large_stack_async_test!(accepting_channel_invitation_materializes_home_and_channel_state, {
    let sender_id = AuthorityId::new_from_entropy([213u8; 32]);
    let receiver_id = AuthorityId::new_from_entropy([214u8; 32]);
    let config = AgentConfig::default();
    let shared_transport = crate::runtime::SharedTransport::new();
    let _sender_effects = Arc::new(
        AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
            &config,
            sender_id,
            shared_transport.clone(),
        )
        .unwrap(),
    );
    let effects = Arc::new(
        AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
            &config,
            receiver_id,
            shared_transport,
        )
        .unwrap(),
    );
    let handler = InvitationHandler::new(AuthorityContext::new(receiver_id)).unwrap();
    register_test_app_signals(effects.as_ref()).await;
    let _rendezvous_tasks = attach_test_rendezvous_manager(effects.as_ref(), receiver_id).await;
    cache_test_peer_descriptor(
        effects.as_ref(),
        receiver_id,
        sender_id,
        "tcp://127.0.0.1:55113",
        1_700_000_000_000,
    )
    .await;

    let invitation_id = InvitationId::new("inv-materialize-home-1");
    let home_id = canonical_home_id(13);
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: invitation_id.clone(),
        sender_id,
        context_id: Some(default_context_id_for_authority(sender_id)),
        invitation_type: InvitationType::Channel {
            home_id,
            nickname_suggestion: Some("Oak House".to_string()),
            bootstrap: None,
            home: true,
        },
        expires_at: None,
        message: Some("Join Oak House".to_string()),
    };

    let imported = handler
        .import_invitation_code(
            effects.as_ref(),
            &shareable
                .to_code()
                .expect("shareable invitation should serialize"),
        )
        .await
        .unwrap();

    handler
        .accept_invitation(effects.clone(), &imported.invitation_id)
        .await
        .unwrap();

    let expected_context = default_context_id_for_authority(sender_id);
    let expected_channel = home_id;

    let committed = effects.load_committed_facts(receiver_id).await.unwrap();
    let found_channel_fact = committed.iter().any(|fact| {
        let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = &fact.content
        else {
            return false;
        };
        if envelope.type_id.as_str() != CHAT_FACT_TYPE_ID {
            return false;
        }
        matches!(
            ChatFact::from_envelope(envelope),
            Some(ChatFact::ChannelCreated {
                context_id,
                channel_id,
                ..
            }) if context_id == expected_context && channel_id == expected_channel
        )
    });
    assert!(
        found_channel_fact,
        "expected ChannelCreated fact for accepted channel invitation"
    );

    use aura_effects::ReactiveEffects;
    let homes: HomesState = effects
        .reactive_handler()
        .read(&*HOMES_SIGNAL)
        .await
        .unwrap();
    let home = homes
        .home_state(&expected_channel)
        .expect("accepted invitation should materialize home state");
    assert_eq!(home.context_id, Some(expected_context));
    assert!(home.member(&receiver_id).is_some());
    assert_eq!(home.my_role, HomeRole::Participant);
});

#[test]
fn accepting_channel_invitation_corrects_preexisting_raw_channel_name() {
    run_async_test_on_large_stack(async move {
        let sender_id = AuthorityId::new_from_entropy([219u8; 32]);
        let receiver_id = AuthorityId::new_from_entropy([220u8; 32]);
        let config = AgentConfig::default();
        let shared_transport = crate::runtime::SharedTransport::new();
        let _sender_effects = Arc::new(
            AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
                &config,
                sender_id,
                shared_transport.clone(),
            )
            .unwrap(),
        );
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
                &config,
                receiver_id,
                shared_transport,
            )
            .unwrap(),
        );
        let handler = InvitationHandler::new(AuthorityContext::new(receiver_id)).unwrap();
        register_test_app_signals(effects.as_ref()).await;
        let _rendezvous_tasks =
            attach_test_rendezvous_manager(effects.as_ref(), receiver_id).await;
        cache_test_peer_descriptor(
            effects.as_ref(),
            receiver_id,
            sender_id,
            "tcp://127.0.0.1:55116",
            1_700_000_000_000,
        )
        .await;

        let invitation_id = InvitationId::new("inv-materialize-home-raw-name");
        let home_id = canonical_home_id(16);
        let expected_context = default_context_id_for_authority(sender_id);

        effects
            .commit_relational_facts(vec![ChatFact::channel_created_ms(
                expected_context,
                home_id,
                home_id.to_string(),
                Some(format!("Home channel {}", home_id)),
                false,
                1_700_000_000_000,
                sender_id,
            )
            .to_generic()])
            .await
            .unwrap();

        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: invitation_id.clone(),
            sender_id,
            context_id: Some(expected_context),
            invitation_type: InvitationType::Channel {
                home_id,
                nickname_suggestion: Some("Maple House".to_string()),
                bootstrap: None,
                home: false,
            },
            expires_at: None,
            message: Some("Join Maple House".to_string()),
        };

        let imported = handler
            .import_invitation_code(
                effects.as_ref(),
                &shareable
                    .to_code()
                    .expect("shareable invitation should serialize"),
            )
            .await
            .unwrap();

        accept_invitation_without_notification(&handler, effects.clone(), &imported.invitation_id)
            .await;

        let committed = effects.load_committed_facts(receiver_id).await.unwrap();
        let found_named_update = committed.iter().any(|fact| {
            let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = &fact.content
            else {
                return false;
            };
            if envelope.type_id.as_str() != CHAT_FACT_TYPE_ID {
                return false;
            }
            matches!(
                ChatFact::from_envelope(envelope),
                Some(ChatFact::ChannelUpdated {
                    context_id,
                    channel_id,
                    name: Some(name),
                    ..
                }) if context_id == expected_context
                    && channel_id == home_id
                    && name == "Maple House"
            )
        });
        assert!(
            found_named_update,
            "accepted invitation should correct preexisting raw-id channel metadata"
        );
    });
}

large_stack_async_test!(accepting_channel_invitation_materializes_amp_bootstrap_state, {
    let sender_id = AuthorityId::new_from_entropy([217u8; 32]);
    let receiver_id = AuthorityId::new_from_entropy([218u8; 32]);
    let config = AgentConfig::default();
    let shared_transport = crate::runtime::SharedTransport::new();
    let _sender_effects = Arc::new(
        AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
            &config,
            sender_id,
            shared_transport.clone(),
        )
        .unwrap(),
    );
    let effects = Arc::new(
        AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
            &config,
            receiver_id,
            shared_transport,
        )
        .unwrap(),
    );
    let handler = InvitationHandler::new(AuthorityContext::new(receiver_id)).unwrap();
    register_test_app_signals(effects.as_ref()).await;
    let _rendezvous_tasks = attach_test_rendezvous_manager(effects.as_ref(), receiver_id).await;
    cache_test_peer_descriptor(
        effects.as_ref(),
        receiver_id,
        sender_id,
        "tcp://127.0.0.1:55114",
        1_700_000_000_000,
    )
    .await;

    let invitation_id = InvitationId::new("inv-materialize-bootstrap-1");
    let home_id = canonical_home_id(14);
    let bootstrap_key = [7u8; 32];
    let bootstrap_id = Hash32::from_bytes(&bootstrap_key);
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: invitation_id.clone(),
        sender_id,
        context_id: Some(default_context_id_for_authority(sender_id)),
        invitation_type: InvitationType::Channel {
            home_id,
            nickname_suggestion: Some("Elm House".to_string()),
            bootstrap: Some(ChannelBootstrapPackage {
                bootstrap_id,
                key: bootstrap_key.to_vec(),
            }),
            home: false,
        },
        expires_at: None,
        message: Some("Join Elm House".to_string()),
    };

    let imported = handler
        .import_invitation_code(
            effects.as_ref(),
            &shareable
                .to_code()
                .expect("shareable invitation should serialize"),
        )
        .await
        .unwrap();

    handler
        .accept_invitation(effects.clone(), &imported.invitation_id)
        .await
        .unwrap();

    let expected_context = default_context_id_for_authority(sender_id);
    let expected_channel = home_id;

    let state = aura_protocol::amp::get_channel_state(
        effects.as_ref(),
        expected_context,
        expected_channel,
    )
    .await
    .expect("accepted invitation should materialize AMP channel state");
    let bootstrap = state
        .bootstrap
        .expect("accepted invitation should materialize bootstrap metadata");
    assert_eq!(bootstrap.bootstrap_id, bootstrap_id);
    assert_eq!(bootstrap.dealer, sender_id);
    assert!(bootstrap.recipients.contains(&sender_id));
    assert!(bootstrap.recipients.contains(&receiver_id));

    let location = SecureStorageLocation::amp_bootstrap_key(
        &expected_context,
        &expected_channel,
        &bootstrap_id,
    );
    let stored_key = effects
        .secure_retrieve(&location, &[SecureStorageCapability::Read])
        .await
        .expect("bootstrap key should be persisted");
    assert_eq!(stored_key, bootstrap_key.to_vec());
});

large_stack_async_test!(accepting_channel_invitation_uses_shareable_context_when_present, {
    let sender_id = AuthorityId::new_from_entropy([215u8; 32]);
    let receiver_id = AuthorityId::new_from_entropy([216u8; 32]);
    let config = AgentConfig::default();
    let shared_transport = crate::runtime::SharedTransport::new();
    let _sender_effects = Arc::new(
        AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
            &config,
            sender_id,
            shared_transport.clone(),
        )
        .unwrap(),
    );
    let effects = Arc::new(
        AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
            &config,
            receiver_id,
            shared_transport,
        )
        .unwrap(),
    );
    let handler = InvitationHandler::new(AuthorityContext::new(receiver_id)).unwrap();
    register_test_app_signals(effects.as_ref()).await;
    let _rendezvous_tasks = attach_test_rendezvous_manager(effects.as_ref(), receiver_id).await;
    cache_test_peer_descriptor(
        effects.as_ref(),
        receiver_id,
        sender_id,
        "tcp://127.0.0.1:55115",
        1_700_000_000_000,
    )
    .await;

    let invitation_id = InvitationId::new("inv-materialize-home-context");
    let custom_context = ContextId::new_from_entropy([55u8; 32]);
    let home_id = canonical_home_id(15);
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: invitation_id.clone(),
        sender_id,
        context_id: Some(custom_context),
        invitation_type: InvitationType::Channel {
            home_id,
            nickname_suggestion: Some("Birch House".to_string()),
            bootstrap: None,
            home: true,
        },
        expires_at: None,
        message: Some("Join Birch House".to_string()),
    };

    let imported = handler
        .import_invitation_code(
            effects.as_ref(),
            &shareable
                .to_code()
                .expect("shareable invitation should serialize"),
        )
        .await
        .unwrap();
    assert_eq!(imported.context_id, custom_context);
    assert_ne!(
        imported.context_id,
        default_context_id_for_authority(sender_id),
        "custom context must override sender default context"
    );

    handler
        .accept_invitation(effects.clone(), &imported.invitation_id)
        .await
        .unwrap();

    let expected_channel = home_id;
    let committed = effects.load_committed_facts(receiver_id).await.unwrap();
    let found_channel_fact = committed.iter().any(|fact| {
        let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = &fact.content
        else {
            return false;
        };
        if envelope.type_id.as_str() != CHAT_FACT_TYPE_ID {
            return false;
        }
        matches!(
            ChatFact::from_envelope(envelope),
            Some(ChatFact::ChannelCreated {
                context_id,
                channel_id,
                ..
            }) if context_id == custom_context && channel_id == expected_channel
        )
    });
    assert!(
        found_channel_fact,
        "expected ChannelCreated fact to use shareable context id"
    );

    use aura_effects::ReactiveEffects;
    let homes: HomesState = effects
        .reactive_handler()
        .read(&*HOMES_SIGNAL)
        .await
        .unwrap();
    let home = homes
        .home_state(&expected_channel)
        .expect("accepted invitation should materialize home state");
    assert_eq!(home.context_id, Some(custom_context));
});

large_stack_async_test!(imported_invitation_is_resolvable_across_handler_instances, {
    let pair = contact_pair(122).await;
    let (own_authority, sender_id) = (pair.receiver_id, pair.sender_id);
    let invitation = pair.create_contact_invitation().await;
    let imported = pair.import(&pair.signed_code(&invitation).await).await;

    // Accept using a separate handler instance to ensure we don't rely on in-memory caches.
    pair.accept_via(&handler_for_id(own_authority), &imported.invitation_id)
        .await
        .unwrap();

    let committed = pair
        .receiver_effects
        .load_committed_facts(own_authority)
        .await
        .unwrap();

    let mut found = None::<ContactFact>;
    for fact in committed {
        let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = fact.content
        else {
            continue;
        };

        if envelope.type_id.as_str() != CONTACT_FACT_TYPE_ID {
            continue;
        }

        found = ContactFact::from_envelope(&envelope);
    }

    let fact = found.expect("expected a committed ContactFact");
    match fact {
        ContactFact::Added { contact_id, .. } => {
            assert_eq!(contact_id, sender_id);
        }
        other => panic!("Expected ContactFact::Added, got {:?}", other),
    }
});

#[tokio::test]
async fn imported_channel_invitation_preserves_authoritative_context_for_choreography() {
    let own_authority = AuthorityId::new_from_entropy([211u8; 32]);
    let sender_id = AuthorityId::new_from_entropy([212u8; 32]);
    let invitation_context = ContextId::new_from_entropy([213u8; 32]);
    let channel_id = canonical_home_id(214);
    let config = AgentConfig::default();
    let effects = Arc::new(
        AuraEffectSystem::simulation_for_test_for_authority(&config, own_authority).unwrap(),
    );

    let authority_context = AuthorityContext::new(own_authority);
    let handler = InvitationHandler::new(authority_context).unwrap();

    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-demo-channel-context"),
        sender_id,
        context_id: Some(invitation_context),
        invitation_type: InvitationType::Channel {
            home_id: channel_id,
            nickname_suggestion: Some("shared-parity-lab".to_string()),
            bootstrap: None,
            home: false,
        },
        expires_at: None,
        message: Some("Channel invitation".to_string()),
    };
    let code = shareable
        .to_code()
        .expect("shareable invitation should serialize");

    let imported = handler
        .import_invitation_code(&effects, &code)
        .await
        .expect("channel import should succeed");

    let choreography_invitation = handler
        .load_invitation_for_choreography(effects.as_ref(), &imported.invitation_id)
        .await
        .expect("imported invitation should be resolvable for choreography");

    assert_eq!(
        choreography_invitation.context_id, invitation_context,
        "channel invitation choreography must preserve the authoritative invitation context"
    );
}

#[tokio::test]
async fn created_invitation_is_retrievable_across_handler_instances() {
    // This test verifies that created invitations are persisted to storage
    // and can be retrieved by a different handler instance (fixing the
    // "failed to export" bug where each agent.invitations() call creates
    // a new handler with an empty in-memory cache).
    let own_authority = AuthorityId::new_from_entropy([124u8; 32]);
    let config = AgentConfig::default();
    let effects = Arc::new(
        AuraEffectSystem::simulation_for_test_for_authority(&config, own_authority).unwrap(),
    );

    let authority_context = AuthorityContext::new(own_authority);

    // Handler 1: Create an invitation
    let handler_create = handler_for(authority_context.clone());
    let receiver_id = AuthorityId::new_from_entropy([125u8; 32]);
    let invitation = handler_create
        .create_invitation(
            effects.clone(),
            receiver_id,
            InvitationType::Contact {
                nickname: Some("Bob".to_string()),
            },
            Some("Hello Bob!".to_string()),
            None,
        )
        .await
        .unwrap();

    // Handler 2: Retrieve the invitation (simulates new service instance)
    let handler_retrieve = handler_for(authority_context);
    let retrieved = handler_retrieve
        .get_invitation_with_storage(&effects, &invitation.invitation_id)
        .await;

    assert!(
        retrieved.is_some(),
        "Created invitation should be retrievable across handler instances"
    );
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.invitation_id, invitation.invitation_id);
    assert_eq!(retrieved.receiver_id, receiver_id);
    assert_eq!(retrieved.sender_id, own_authority);
}

large_stack_async_test!(accepted_imported_invitation_persists_status_across_handler_instances, {
    let pair = contact_pair(252).await;
    let (own_authority, sender_id) = (pair.receiver_id, pair.sender_id);
    let invitation = pair.create_contact_invitation().await;
    let imported = pair.import(&pair.signed_code(&invitation).await).await;
    pair.accept_with_responding_inviter(&imported.invitation_id)
        .await
        .expect("confirmed contact invitation accept should persist imported status");

    let retrieved = handler_for_id(own_authority)
        .get_invitation_with_storage(pair.receiver_effects.as_ref(), &imported.invitation_id)
        .await
        .expect("accepted imported invitation should remain available");
    assert_eq!(retrieved.status, InvitationStatus::Accepted);
    assert_eq!(retrieved.sender_id, sender_id);
    assert_eq!(retrieved.receiver_id, own_authority);
});

large_stack_async_test!(invitation_can_be_cancelled, {
    let authority_context = create_test_authority(98);
    let effects = effects_for(&authority_context);
    let handler = InvitationHandler::new(authority_context).unwrap();

    let receiver_id = AuthorityId::new_from_entropy([99u8; 32]);

    let invitation = handler
        .create_invitation(
            effects.clone(),
            receiver_id,
            InvitationType::Contact { nickname: None },
            None,
            None,
        )
        .await
        .unwrap();

    let result = handler
        .cancel_invitation(effects.clone(), &invitation.invitation_id)
        .await
        .unwrap();

    assert_eq!(result.new_status, InvitationStatus::Cancelled);

    // Verify it was removed from pending
    let pending = handler.list_pending().await;
    assert!(pending.is_empty());
});

large_stack_async_test!(list_pending_shows_only_pending, {
    let authority_context = create_test_authority(100);
    let effects = effects_for(&authority_context);
    let handler = InvitationHandler::new(authority_context).unwrap();

    // Create 3 invitations
    let inv1 = handler
        .create_invitation(
            effects.clone(),
            AuthorityId::new_from_entropy([101u8; 32]),
            InvitationType::Contact { nickname: None },
            None,
            None,
        )
        .await
        .unwrap();

    let inv2 = handler
        .create_invitation(
            effects.clone(),
            AuthorityId::new_from_entropy([102u8; 32]),
            InvitationType::Contact { nickname: None },
            None,
            None,
        )
        .await
        .unwrap();

    let _inv3 = handler
        .create_invitation(
            effects.clone(),
            AuthorityId::new_from_entropy([103u8; 32]),
            InvitationType::Contact { nickname: None },
            None,
            None,
        )
        .await
        .unwrap();

    // Accept one, cancel another
    handler
        .accept_invitation(effects.clone(), &inv1.invitation_id)
        .await
        .unwrap();
    handler
        .cancel_invitation(effects.clone(), &inv2.invitation_id)
        .await
        .unwrap();

    // Only inv3 should be pending
    let pending = handler.list_pending().await;
    assert_eq!(pending.len(), 1);
});

// =========================================================================
// ShareableInvitation Tests
// =========================================================================

#[test]
fn shareable_invitation_roundtrip_contact() {
    let sender_id = AuthorityId::new_from_entropy([42u8; 32]);
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-test-123"),
        sender_id,
        context_id: None,
        invitation_type: InvitationType::Contact {
            nickname: Some("alice".to_string()),
        },
        expires_at: Some(1700000000000),
        message: Some("Hello!".to_string()),
    };

    let code = shareable
        .to_code()
        .expect("shareable invitation should serialize");
    assert!(code.starts_with("aura:v1:"));

    let decoded = ShareableInvitation::from_code(&code).unwrap();
    assert_eq!(decoded.version, shareable.version);
    assert_eq!(decoded.invitation_id, shareable.invitation_id);
    assert_eq!(decoded.sender_id, shareable.sender_id);
    assert_eq!(decoded.expires_at, shareable.expires_at);
    assert_eq!(decoded.message, shareable.message);
}

#[test]
fn shareable_invitation_roundtrip_guardian() {
    let sender_id = AuthorityId::new_from_entropy([43u8; 32]);
    let subject_authority = AuthorityId::new_from_entropy([44u8; 32]);
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-guardian-456"),
        sender_id,
        context_id: None,
        invitation_type: InvitationType::Guardian { subject_authority },
        expires_at: None,
        message: None,
    };

    let code = shareable
        .to_code()
        .expect("shareable invitation should serialize");
    let decoded = ShareableInvitation::from_code(&code).unwrap();

    match decoded.invitation_type {
        InvitationType::Guardian {
            subject_authority: sa,
        } => {
            assert_eq!(sa, subject_authority);
        }
        _ => panic!("wrong invitation type"),
    }
}

#[test]
fn shareable_invitation_roundtrip_channel() {
    let sender_id = AuthorityId::new_from_entropy([45u8; 32]);
    let context_id = ContextId::new_from_entropy([56u8; 32]);
    let home_id = ChannelId::from_bytes([21u8; 32]);
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-channel-789"),
        sender_id,
        context_id: Some(context_id),
        invitation_type: InvitationType::Channel {
            home_id,
            nickname_suggestion: None,
            bootstrap: None,
            home: false,
        },
        expires_at: Some(1800000000000),
        message: Some("Join my channel!".to_string()),
    };

    let code = shareable
        .to_code()
        .expect("shareable invitation should serialize");
    let decoded = ShareableInvitation::from_code(&code).unwrap();
    assert_eq!(decoded.context_id, Some(context_id));

    match decoded.invitation_type {
        InvitationType::Channel {
            home_id,
            nickname_suggestion: _,
            bootstrap: _,
            home: _,
        } => {
            assert_eq!(home_id, ChannelId::from_bytes([21u8; 32]));
        }
        _ => panic!("wrong invitation type"),
    }
}

#[test]
fn shareable_invitation_roundtrip_device_enrollment_preserves_baseline_tree_ops() {
    let sender_id = AuthorityId::new_from_entropy([145u8; 32]);
    let subject_authority = AuthorityId::new_from_entropy([146u8; 32]);
    let context_id = ContextId::new_from_entropy([147u8; 32]);
    let initiator_device_id = DeviceId::new_from_entropy([148u8; 32]);
    let device_id = DeviceId::new_from_entropy([149u8; 32]);
    let ceremony_id = CeremonyId::new("ceremony:test-device-enrollment");
    let baseline_tree_ops = vec![vec![1, 2, 3], vec![4, 5, 6, 7]];
    let threshold_config = vec![9, 8, 7];
    let public_key_package = vec![6, 5, 4, 3];
    let key_package = vec![3, 4, 5];

    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-device-enrollment"),
        sender_id,
        context_id: Some(context_id),
        invitation_type: InvitationType::DeviceEnrollment {
            subject_authority,
            invitee_authority: None,
            initiator_device_id,
            device_id,
            nickname_suggestion: Some("WebApp".to_string()),
            ceremony_id: ceremony_id.clone(),
            pending_epoch: 1,
            key_package: key_package.clone(),
            threshold_config: threshold_config.clone(),
            public_key_package: public_key_package.clone(),
            baseline_tree_ops: baseline_tree_ops.clone(),
        },
        expires_at: None,
        message: None,
    };

    let code = shareable
        .to_code()
        .expect("shareable invitation should serialize");
    let decoded = ShareableInvitation::from_code(&code).unwrap();

    match decoded.invitation_type {
        InvitationType::DeviceEnrollment {
            invitee_authority: _,
            subject_authority: decoded_subject_authority,
            initiator_device_id: decoded_initiator_device_id,
            device_id: decoded_device_id,
            nickname_suggestion,
            ceremony_id: decoded_ceremony_id,
            pending_epoch,
            key_package: decoded_key_package,
            threshold_config: decoded_threshold_config,
            public_key_package: decoded_public_key_package,
            baseline_tree_ops: decoded_baseline_tree_ops,
        } => {
            assert_eq!(decoded_subject_authority, subject_authority);
            assert_eq!(decoded_initiator_device_id, initiator_device_id);
            assert_eq!(decoded_device_id, device_id);
            assert_eq!(nickname_suggestion.as_deref(), Some("WebApp"));
            assert_eq!(decoded_ceremony_id, ceremony_id);
            assert_eq!(pending_epoch, 1);
            assert_eq!(decoded_key_package, key_package);
            assert_eq!(decoded_threshold_config, threshold_config);
            assert_eq!(decoded_public_key_package, public_key_package);
            assert_eq!(decoded_baseline_tree_ops, baseline_tree_ops);
        }
        _ => panic!("wrong invitation type"),
    }
}

fn test_device_enrollment_invitation(invitation_id: &str) -> Invitation {
    let sender_id = AuthorityId::new_from_entropy([150u8; 32]);
    Invitation {
        invitation_id: InvitationId::new(invitation_id),
        sender_id,
        receiver_id: AuthorityId::new_from_entropy([151u8; 32]),
        context_id: default_context_id_for_authority(sender_id),
        invitation_type: InvitationType::DeviceEnrollment {
            subject_authority: sender_id,
            invitee_authority: None,
            initiator_device_id: DeviceId::new_from_entropy([152u8; 32]),
            device_id: DeviceId::new_from_entropy([153u8; 32]),
            nickname_suggestion: Some("Tablet".to_string()),
            ceremony_id: CeremonyId::new(format!("ceremony:{invitation_id}")),
            pending_epoch: 7,
            key_package: vec![17, 18, 19],
            threshold_config: vec![27, 28, 29],
            public_key_package: vec![37, 38, 39],
            baseline_tree_ops: vec![vec![47, 48, 49]],
        },
        created_at: 1_700_000_000_000,
        expires_at: None,
        message: Some("enroll this device".to_string()),
        receiver_nickname: None,
        status: InvitationStatus::Pending,
    }
}

fn assert_device_enrollment_payload_empty(invitation_type: &InvitationType) {
    match invitation_type {
        InvitationType::DeviceEnrollment {
            key_package,
            threshold_config,
            public_key_package,
            baseline_tree_ops,
            ..
        } => {
            assert!(key_package.is_empty(), "regular cache leaked key package");
            assert!(
                threshold_config.is_empty(),
                "regular cache leaked threshold config"
            );
            assert!(
                public_key_package.is_empty(),
                "regular cache leaked public key package"
            );
            assert!(
                baseline_tree_ops.is_empty(),
                "regular cache leaked baseline tree ops"
            );
        }
        _ => panic!("expected device enrollment invitation"),
    }
}

fn assert_device_enrollment_payload_restored(invitation_type: &InvitationType) {
    match invitation_type {
        InvitationType::DeviceEnrollment {
            key_package,
            threshold_config,
            public_key_package,
            baseline_tree_ops,
            ..
        } => {
            assert_eq!(key_package, &vec![17, 18, 19]);
            assert_eq!(threshold_config, &vec![27, 28, 29]);
            assert_eq!(public_key_package, &vec![37, 38, 39]);
            assert_eq!(baseline_tree_ops, &vec![vec![47, 48, 49]]);
        }
        _ => panic!("expected device enrollment invitation"),
    }
}

#[tokio::test]
async fn device_enrollment_created_cache_redacts_regular_storage_and_restores_secure_payload() {
    let authority = create_test_authority(154);
    let effects = effects_for(&authority);
    let invitation = test_device_enrollment_invitation("created-device-secret-cache");

    InvitationCacheHandler::persist_created_invitation(
        effects.as_ref(),
        authority.authority_id(),
        &invitation,
    )
    .await
    .expect("device invitation should persist");

    let key = InvitationCacheHandler::created_invitation_key(
        authority.authority_id(),
        &invitation.invitation_id,
    );
    let regular_bytes = effects
        .retrieve(&key)
        .await
        .expect("regular storage should be readable")
        .expect("regular cache record should exist");
    let regular_invitation: Invitation =
        serde_json::from_slice(&regular_bytes).expect("regular cache should parse");
    assert_device_enrollment_payload_empty(&regular_invitation.invitation_type);

    let restored = InvitationCacheHandler::load_created_invitation(
        effects.as_ref(),
        authority.authority_id(),
        &invitation.invitation_id,
    )
    .await
    .expect("secure payload should restore created invitation");
    assert_device_enrollment_payload_restored(&restored.invitation_type);
}

#[tokio::test]
async fn device_enrollment_imported_cache_redacts_regular_storage_and_restores_secure_payload() {
    let authority = create_test_authority(155);
    let effects = effects_for(&authority);
    let invitation = test_device_enrollment_invitation("imported-device-secret-cache");
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: invitation.invitation_id.clone(),
        sender_id: invitation.sender_id,
        context_id: Some(invitation.context_id),
        invitation_type: invitation.invitation_type.clone(),
        expires_at: invitation.expires_at,
        message: invitation.message.clone(),
    };
    let imported =
        StoredImportedInvitation::pending(shareable, invitation.created_at, ImportedSenderTrust::SelfCertified);

    InvitationCacheHandler::persist_imported_invitation(
        effects.as_ref(),
        authority.authority_id(),
        &imported,
    )
    .await
    .expect("imported device invitation should persist");

    let key = InvitationCacheHandler::imported_invitation_key(
        authority.authority_id(),
        &invitation.invitation_id,
    );
    let regular_bytes = effects
        .retrieve(&key)
        .await
        .expect("regular storage should be readable")
        .expect("regular imported cache record should exist");
    let regular_imported =
        InvitationCacheHandler::parse_imported_invitation_bytes(&regular_bytes, None)
            .expect("regular imported cache should parse");
    assert_device_enrollment_payload_empty(&regular_imported.shareable.invitation_type);

    let restored = InvitationCacheHandler::load_imported_invitation(
        effects.as_ref(),
        authority.authority_id(),
        &invitation.invitation_id,
        None,
    )
    .await
    .expect("secure payload should restore imported invitation");
    assert_device_enrollment_payload_restored(&restored.shareable.invitation_type);
}

fn device_enrollment_test_invitation(
    invitation_id: &str,
    sender_id: AuthorityId,
    receiver_id: AuthorityId,
    device_id: DeviceId,
) -> Invitation {
    Invitation {
        invitation_id: InvitationId::new(invitation_id),
        sender_id,
        receiver_id,
        context_id: default_context_id_for_authority(sender_id),
        invitation_type: InvitationType::DeviceEnrollment {
            subject_authority: sender_id,
            invitee_authority: None,
            initiator_device_id: DeviceId::new_from_entropy([153u8; 32]),
            device_id,
            nickname_suggestion: Some("Tablet".to_string()),
            ceremony_id: CeremonyId::new("ceremony:device-enrollment-signed"),
            pending_epoch: 1,
            key_package: vec![1, 2, 3],
            threshold_config: vec![4, 5, 6],
            public_key_package: vec![7, 8, 9],
            baseline_tree_ops: vec![vec![10, 11, 12]],
        },
        created_at: 1_700_000_000_000,
        expires_at: Some(1_700_000_600_000),
        message: None,
        receiver_nickname: None,
        status: InvitationStatus::Pending,
    }
}

async fn signed_device_enrollment_accept(
    invitee_effects: &Arc<AuraEffectSystem>,
    invitation: &Invitation,
    acceptor_id: AuthorityId,
    device_id: DeviceId,
) -> DeviceEnrollmentAccept {
    bootstrap_test_signing_authority(invitee_effects, acceptor_id).await;
    let ceremony_id = CeremonyId::new("ceremony:device-enrollment-signed");
    let transcript = DeviceEnrollmentAcceptanceTranscript {
        invitation,
        acceptor_id,
        subject_authority: invitation.sender_id,
        ceremony_id: ceremony_id.clone(),
        device_id,
    };
    let signature =
        sign_invitation_acceptance_transcript(invitee_effects.as_ref(), acceptor_id, &transcript)
            .await
            .expect("device enrollment acceptance should sign");
    DeviceEnrollmentAccept {
        invitation_id: invitation.invitation_id.clone(),
        ceremony_id,
        device_id,
        acceptor_id,
        signature,
    }
}

/// Regression (work/8.md task 7): device enrollment is accepted only with a
/// valid signature from the invited authority over this exact enrollment.
#[tokio::test]
async fn device_enrollment_acceptance_requires_signed_transcript_from_invitee() {
    let initiator = create_test_authority(151);
    let invitee = create_test_authority(154);
    let initiator_effects = effects_for(&initiator);
    let invitee_effects = effects_for(&invitee);
    let device_id = invitee.device_id();
    let ceremony_id = CeremonyId::new("ceremony:device-enrollment-signed");
    let invitation = device_enrollment_test_invitation(
        "inv-device-enrollment-signed",
        initiator.authority_id(),
        invitee.authority_id(),
        device_id,
    );
    let accept = signed_device_enrollment_accept(
        &invitee_effects,
        &invitation,
        invitee.authority_id(),
        device_id,
    )
    .await;

    // Valid signed acceptance verifies.
    super::device_enrollment::verify_device_enrollment_acceptance(
        initiator_effects.as_ref(),
        &invitation,
        initiator.authority_id(),
        &ceremony_id,
        device_id,
        &accept,
    )
    .await
    .expect("valid signed acceptance should verify");

    // Forged: a different authority claims the acceptance.
    let mut forged = accept.clone();
    forged.acceptor_id = AuthorityId::new_from_entropy([200u8; 32]);
    assert!(super::device_enrollment::verify_device_enrollment_acceptance(
        initiator_effects.as_ref(),
        &invitation,
        initiator.authority_id(),
        &ceremony_id,
        device_id,
        &forged,
    )
    .await
    .is_err());

    // Tampered: acceptance names a different device.
    let mut tampered = accept.clone();
    tampered.device_id = DeviceId::new_from_entropy([201u8; 32]);
    assert!(super::device_enrollment::verify_device_enrollment_acceptance(
        initiator_effects.as_ref(),
        &invitation,
        initiator.authority_id(),
        &ceremony_id,
        device_id,
        &tampered,
    )
    .await
    .is_err());

    // Replay: the same acceptance presented for a different invitation.
    let other_invitation = device_enrollment_test_invitation(
        "inv-device-enrollment-other",
        initiator.authority_id(),
        invitee.authority_id(),
        device_id,
    );
    let mut replayed = accept.clone();
    replayed.invitation_id = other_invitation.invitation_id.clone();
    assert!(super::device_enrollment::verify_device_enrollment_acceptance(
        initiator_effects.as_ref(),
        &other_invitation,
        initiator.authority_id(),
        &ceremony_id,
        device_id,
        &replayed,
    )
    .await
    .is_err());

    // Tampered signature bytes.
    let mut bad_sig = accept;
    if let Some(byte) = bad_sig.signature.signature.first_mut() {
        *byte ^= 0x01;
    }
    assert!(super::device_enrollment::verify_device_enrollment_acceptance(
        initiator_effects.as_ref(),
        &invitation,
        initiator.authority_id(),
        &ceremony_id,
        device_id,
        &bad_sig,
    )
    .await
    .is_err());
}

#[test]
fn shareable_invitation_parses_optional_sender_addr_and_device_segments() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

    let sender_id = AuthorityId::new_from_entropy([46u8; 32]);
    let sender_device_id = DeviceId::new_from_entropy([47u8; 32]);
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-addr-001"),
        sender_id,
        context_id: None,
        invitation_type: InvitationType::Contact { nickname: None },
        expires_at: None,
        message: None,
    };
    let base = shareable
        .to_code()
        .expect("shareable invitation should serialize");
    let code = format!(
        "{base}:{}:{}",
        URL_SAFE_NO_PAD.encode("127.0.0.1:43501".as_bytes()),
        URL_SAFE_NO_PAD.encode(sender_device_id.to_string().as_bytes())
    );

    let decoded = ShareableInvitation::from_code(&code).unwrap();
    assert_eq!(decoded.invitation_id, shareable.invitation_id);
    assert_eq!(decoded.sender_id, shareable.sender_id);
    assert_eq!(
        ShareableInvitation::sender_addr_from_code(&code),
        Some("127.0.0.1:43501".to_string())
    );
    assert_eq!(
        ShareableInvitation::sender_device_id_from_code(&code),
        Some(sender_device_id)
    );
}

#[tokio::test]
async fn shareable_invitation_signed_envelope_roundtrips_sender_proof() {
    let authority = create_test_authority(240);
    let effects = effects_for(&authority);
    let (private_key, public_key) = effects.ed25519_generate_keypair().await.unwrap();
    let sender_id = AuthorityId::new_from_entropy(hash(&public_key));
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-signed-proof"),
        sender_id,
        context_id: None,
        invitation_type: InvitationType::Contact {
            nickname: Some("Signed".to_string()),
        },
        expires_at: Some(1_800_000_000_000),
        message: Some("signed contact invite".to_string()),
    };
    let signature = aura_signature::sign_ed25519_transcript(
        effects.as_ref(),
        &shareable.signing_transcript(),
        &private_key,
    )
    .await
    .unwrap();
    let code = shareable
        .to_signed_code(ShareableInvitationSenderProof {
            scheme: ShareableInvitation::SENDER_PROOF_SCHEME.to_string(),
            public_key: public_key.clone(),
            signature,
            sender_device_id: Some(authority.device_id()),
            key_epoch: Some(1),
        })
        .unwrap();

    let (decoded, proof) = ShareableInvitation::from_code_with_proof(&code).unwrap();
    let proof = proof.expect("signed envelope should carry proof");
    assert_eq!(decoded.sender_id, sender_id);
    assert_eq!(proof.public_key, public_key);
    assert!(decoded.sender_id_bound_to_public_key(&proof.public_key));
}

#[tokio::test]
async fn shareable_invitation_signed_envelope_binds_transport_metadata() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

    let authority = create_test_authority(239);
    let effects = effects_for(&authority);
    let (private_key, public_key) = effects.ed25519_generate_keypair().await.unwrap();
    let sender_id = AuthorityId::new_from_entropy(hash(&public_key));
    let sender_device_id = DeviceId::new_from_entropy([238u8; 32]);
    let transport = ShareableInvitationTransportMetadata {
        sender_hint: Some("tcp://203.0.113.10:45555".to_string()),
        sender_device_id: Some(sender_device_id),
    };
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-signed-transport"),
        sender_id,
        context_id: None,
        invitation_type: InvitationType::Contact { nickname: None },
        expires_at: Some(4_102_444_800_000),
        message: None,
    };
    let signature = aura_signature::sign_ed25519_transcript(
        effects.as_ref(),
        &shareable.signing_transcript_with_transport(&transport),
        &private_key,
    )
    .await
    .unwrap();
    let code = shareable
        .to_signed_code_with_transport(
            ShareableInvitationSenderProof {
                scheme: ShareableInvitation::SENDER_PROOF_SCHEME.to_string(),
                public_key: public_key.clone(),
                signature,
                sender_device_id: Some(sender_device_id),
                key_epoch: Some(1),
            },
            transport.clone(),
        )
        .unwrap();

    let (decoded, proof, decoded_transport) =
        ShareableInvitation::from_code_with_proof_and_transport(&code).unwrap();
    assert_eq!(decoded.sender_id, sender_id);
    assert_eq!(proof.unwrap().public_key, public_key);
    assert_eq!(decoded_transport, transport);

    let mut parts: Vec<&str> = code.split(':').collect();
    let mut envelope: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[2]).unwrap()).unwrap();
    envelope["transport"]["sender_hint"] =
        serde_json::Value::String("tcp://203.0.113.11:45555".to_string());
    let tampered_json = serde_json::to_vec(&envelope).unwrap();
    let tampered_payload = URL_SAFE_NO_PAD.encode(tampered_json);
    parts[2] = &tampered_payload;
    let tampered_code = parts.join(":");
    let (tampered, proof, tampered_transport) =
        ShareableInvitation::from_code_with_proof_and_transport(&tampered_code).unwrap();
    let verified = aura_signature::verify_ed25519_transcript(
        effects.as_ref(),
        &tampered.signing_transcript_with_transport(&tampered_transport),
        &proof.unwrap().signature,
        &public_key,
    )
    .await
    .unwrap();
    assert!(!verified);
}

#[tokio::test]
async fn production_import_rejects_unsigned_shareable_invitation() {
    let authority = create_test_authority(241);
    let temp = tempfile::tempdir().unwrap();
    let config = AgentConfig {
        device_id: authority.device_id(),
        storage: StorageConfig {
            base_path: temp.path().join("aura"),
            ..Default::default()
        },
        ..Default::default()
    };
    let effects = AuraEffectSystem::production(config, authority.authority_id()).unwrap();
    let handler = handler_for(authority);
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-unsigned-prod"),
        sender_id: AuthorityId::new_from_entropy([242u8; 32]),
        context_id: None,
        invitation_type: InvitationType::Contact { nickname: None },
        expires_at: None,
        message: None,
    };
    let code = shareable.to_code().unwrap();

    let err = handler
        .import_invitation_code(&effects, &code)
        .await
        .expect_err("production import must reject unsigned codes");
    assert!(err.to_string().contains("missing sender proof"));
}

#[tokio::test]
async fn production_harness_mode_still_rejects_invalid_sender_proof() {
    let harness_mode_env = crate::runtime_bridge::harness_mode_env_key_for_tests();
    let _env_restore = EnvRestore::capture(&[harness_mode_env]);
    std::env::set_var(harness_mode_env, "1");

    let authority = create_test_authority(237);
    let effects = production_effects_for(&authority);
    let handler = handler_for(authority.clone());
    assert!(effects.harness_mode_enabled());
    let (_, public_key) = effects.ed25519_generate_keypair().await.unwrap();
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-harness-invalid-proof"),
        sender_id: AuthorityId::new_from_entropy(hash(&public_key)),
        context_id: None,
        invitation_type: InvitationType::Contact { nickname: None },
        expires_at: Some(4_102_444_800_000),
        message: None,
    };
    let transport = ShareableInvitationTransportMetadata::default();
    let invalid_proof = ShareableInvitationSenderProof {
        scheme: ShareableInvitation::SENDER_PROOF_SCHEME.to_string(),
        public_key,
        signature: vec![0; ShareableInvitation::SENDER_PROOF_SIGNATURE_BYTES],
        sender_device_id: Some(authority.device_id()),
        key_epoch: Some(1),
    };

    let missing = handler
        .validate_importable_shareable_invitation(&effects, &shareable, None, &transport)
    .await
    .unwrap_err();
    assert!(
        missing.to_string().contains("missing sender proof"),
        "unexpected missing-proof error: {missing}"
    );

    let invalid = handler
        .validate_importable_shareable_invitation(
            &effects,
            &shareable,
            Some(&invalid_proof),
            &transport,
        )
    .await
    .unwrap_err();
    assert!(
        invalid.to_string().contains("sender proof is invalid"),
        "unexpected invalid-proof error: {invalid}"
    );
}

#[tokio::test]
async fn production_sender_proof_validation_is_harness_mode_neutral() {
    let harness_mode_env = crate::runtime_bridge::harness_mode_env_key_for_tests();
    let _env_restore = EnvRestore::capture(&[harness_mode_env]);
    std::env::remove_var(harness_mode_env);

    let authority = create_test_authority(236);
    let handler = handler_for(authority.clone());
    let baseline_effects = production_effects_for(&authority);
    let (private_key, public_key) = baseline_effects.ed25519_generate_keypair().await.unwrap();
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-harness-valid-proof"),
        sender_id: AuthorityId::new_from_entropy(hash(&public_key)),
        context_id: None,
        invitation_type: InvitationType::Contact { nickname: None },
        expires_at: Some(4_102_444_800_000),
        message: None,
    };
    let transport = ShareableInvitationTransportMetadata {
        sender_hint: Some("tcp://203.0.113.20:45555".to_string()),
        sender_device_id: Some(authority.device_id()),
    };
    let signature = aura_signature::sign_ed25519_transcript(
        baseline_effects.as_ref(),
        &shareable.signing_transcript_with_transport(&transport),
        &private_key,
    )
    .await
    .unwrap();
    let proof = ShareableInvitationSenderProof {
        scheme: ShareableInvitation::SENDER_PROOF_SCHEME.to_string(),
        public_key,
        signature,
        sender_device_id: Some(authority.device_id()),
        key_epoch: Some(1),
    };

    handler
        .validate_importable_shareable_invitation(
            &baseline_effects,
            &shareable,
            Some(&proof),
            &transport,
        )
    .await
    .unwrap();

    std::env::set_var(harness_mode_env, "1");
    let harness_effects = production_effects_for(&authority);
    assert!(harness_effects.harness_mode_enabled());
    handler
        .validate_importable_shareable_invitation(
            &harness_effects,
            &shareable,
            Some(&proof),
            &transport,
        )
    .await
    .unwrap();
}

#[tokio::test]
async fn production_import_accepts_signed_self_certified_opaque_sender() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

    let authority = create_test_authority(243);
    let temp = tempfile::tempdir().unwrap();
    let config = AgentConfig {
        device_id: authority.device_id(),
        storage: StorageConfig {
            base_path: temp.path().join("aura"),
            ..Default::default()
        },
        ..Default::default()
    };
    let effects = AuraEffectSystem::production(config, authority.authority_id()).unwrap();
    let sender_device_id = authority.device_id();
    let handler = handler_for(authority);
    let (private_key, public_key) = effects.ed25519_generate_keypair().await.unwrap();
    let sender_id = AuthorityId::new_from_entropy([244u8; 32]);
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-opaque-sender-prod"),
        sender_id,
        context_id: None,
        invitation_type: InvitationType::Contact { nickname: None },
        expires_at: None,
        message: None,
    };
    assert!(!shareable.sender_id_bound_to_public_key(&public_key));
    let signature = aura_signature::sign_ed25519_transcript(
        &effects,
        &shareable.signing_transcript(),
        &private_key,
    )
    .await
    .unwrap();
    let code = shareable
        .to_signed_code(ShareableInvitationSenderProof {
            scheme: ShareableInvitation::SENDER_PROOF_SCHEME.to_string(),
            public_key,
            signature,
            sender_device_id: Some(sender_device_id),
            key_epoch: Some(1),
        })
        .unwrap();

    let mut parts: Vec<&str> = code.split(':').collect();
    let mut envelope: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[2]).unwrap()).unwrap();
    envelope["payload"]["sender_id"] =
        serde_json::to_value(AuthorityId::new_from_entropy([245u8; 32])).unwrap();
    let tampered_json = serde_json::to_vec(&envelope).unwrap();
    let tampered_payload = URL_SAFE_NO_PAD.encode(tampered_json);
    parts[2] = &tampered_payload;
    let tampered_code = parts.join(":");

    let err = handler
        .import_invitation_code(&effects, &tampered_code)
        .await
        .expect_err("tampered sender id must invalidate the signed proof");
    assert!(
        err.to_string().contains("sender proof is invalid"),
        "unexpected tampered sender error: {err}"
    );

    let imported = handler
        .import_invitation_code(&effects, &code)
        .await
        .expect("signed self-certified opaque sender should import");
    assert_eq!(imported.sender_id, sender_id);
}

#[tokio::test]
async fn production_import_rejects_self_certified_key_for_known_sender() {
    let receiver = create_test_authority(232);
    let effects = production_effects_for(&receiver);
    let handler = handler_for(receiver.clone());
    let (private_key, public_key) = effects.ed25519_generate_keypair().await.unwrap();
    let sender_id = AuthorityId::new_from_entropy(hash(&public_key));
    handler
        .invitation_cache
        .record_contact_fact(&ContactFact::added_with_timestamp_ms(
            default_context_id_for_authority(sender_id),
            receiver.authority_id(),
            sender_id,
            "Known sender".to_string(),
            1,
        ))
        .await;

    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-known-sender-self-certified"),
        sender_id,
        context_id: None,
        invitation_type: InvitationType::Contact { nickname: None },
        expires_at: Some(4_102_444_800_000),
        message: None,
    };
    let signature = aura_signature::sign_ed25519_transcript(
        effects.as_ref(),
        &shareable.signing_transcript(),
        &private_key,
    )
    .await
    .unwrap();
    let code = shareable
        .to_signed_code(ShareableInvitationSenderProof {
            scheme: ShareableInvitation::SENDER_PROOF_SCHEME.to_string(),
            public_key,
            signature,
            sender_device_id: Some(receiver.device_id()),
            key_epoch: Some(1),
        })
        .unwrap();

    let err = handler
        .import_invitation_code(effects.as_ref(), &code)
        .await
        .expect_err("known sender must not be accepted with self-certified proof key");
    assert!(
        err.to_string()
            .contains("known sender invitation requires trusted sender key resolution"),
        "unexpected known-sender trust error: {err}"
    );
}

#[tokio::test]
async fn production_import_rejects_stale_self_certified_sender_key_for_known_contact() {
    let receiver = create_test_authority(231);
    let effects = production_effects_for(&receiver);
    let handler = handler_for(receiver.clone());
    let (stale_private_key, stale_public_key) = effects.ed25519_generate_keypair().await.unwrap();
    let (_, current_public_key) = effects.ed25519_generate_keypair().await.unwrap();
    let sender_id = AuthorityId::new_from_entropy(hash(&stale_public_key));
    let sender_device_id = DeviceId::new_from_entropy([231u8; 32]);
    handler
        .trusted_key_resolver
        .register_device_key(sender_device_id, current_public_key)
        .unwrap();
    handler
        .invitation_cache
        .record_contact_fact(&ContactFact::added_with_timestamp_ms(
            default_context_id_for_authority(sender_id),
            receiver.authority_id(),
            sender_id,
            "Rotated sender".to_string(),
            1,
        ))
        .await;

    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-known-sender-stale-key"),
        sender_id,
        context_id: None,
        invitation_type: InvitationType::Contact { nickname: None },
        expires_at: Some(4_102_444_800_000),
        message: None,
    };
    let signature = aura_signature::sign_ed25519_transcript(
        effects.as_ref(),
        &shareable.signing_transcript(),
        &stale_private_key,
    )
    .await
    .unwrap();
    let code = shareable
        .to_signed_code(ShareableInvitationSenderProof {
            scheme: ShareableInvitation::SENDER_PROOF_SCHEME.to_string(),
            public_key: stale_public_key,
            signature,
            sender_device_id: Some(sender_device_id),
            key_epoch: Some(0),
        })
        .unwrap();

    let err = handler
        .import_invitation_code(effects.as_ref(), &code)
        .await
        .expect_err("known sender stale key must fail closed without trusted resolver");
    assert!(
        err.to_string()
            .contains("known sender invitation proof key does not match trusted device key"),
        "unexpected stale-key error: {err}"
    );
}

#[tokio::test]
async fn production_import_accepts_known_sender_with_trusted_device_key() {
    let receiver = create_test_authority(230);
    let effects = production_effects_for(&receiver);
    let handler = handler_for(receiver.clone());
    let (private_key, public_key) = effects.ed25519_generate_keypair().await.unwrap();
    let sender_id = AuthorityId::new_from_entropy(hash(&public_key));
    let sender_device_id = DeviceId::new_from_entropy([230u8; 32]);
    handler
        .trusted_key_resolver
        .register_device_key(sender_device_id, public_key.clone())
        .unwrap();
    handler
        .invitation_cache
        .record_contact_fact(&ContactFact::added_with_timestamp_ms(
            default_context_id_for_authority(sender_id),
            receiver.authority_id(),
            sender_id,
            "Trusted sender".to_string(),
            1,
        ))
        .await;

    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-known-sender-trusted-key"),
        sender_id,
        context_id: None,
        invitation_type: InvitationType::Contact { nickname: None },
        expires_at: Some(4_102_444_800_000),
        message: None,
    };
    let signature = aura_signature::sign_ed25519_transcript(
        effects.as_ref(),
        &shareable.signing_transcript(),
        &private_key,
    )
    .await
    .unwrap();
    let code = shareable
        .to_signed_code(ShareableInvitationSenderProof {
            scheme: ShareableInvitation::SENDER_PROOF_SCHEME.to_string(),
            public_key,
            signature,
            sender_device_id: Some(sender_device_id),
            key_epoch: Some(7),
        })
        .unwrap();

    let imported = handler
        .import_invitation_code(effects.as_ref(), &code)
        .await
        .expect("known sender should import with matching trusted device key");
    assert_eq!(imported.sender_id, sender_id);
}

#[tokio::test]
async fn production_import_rejects_expired_signed_invitation_code() {
    let authority = create_test_authority(245);
    let temp = tempfile::tempdir().unwrap();
    let config = AgentConfig {
        device_id: authority.device_id(),
        storage: StorageConfig {
            base_path: temp.path().join("aura"),
            ..Default::default()
        },
        ..Default::default()
    };
    let effects = AuraEffectSystem::production(config, authority.authority_id()).unwrap();
    let handler = handler_for(authority);
    let (private_key, public_key) = effects.ed25519_generate_keypair().await.unwrap();
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-expired-signed-prod"),
        sender_id: AuthorityId::new_from_entropy(hash(&public_key)),
        context_id: None,
        invitation_type: InvitationType::Contact { nickname: None },
        expires_at: Some(1),
        message: None,
    };
    let signature = aura_signature::sign_ed25519_transcript(
        &effects,
        &shareable.signing_transcript(),
        &private_key,
    )
    .await
    .unwrap();
    let code = shareable
        .to_signed_code(ShareableInvitationSenderProof {
            scheme: ShareableInvitation::SENDER_PROOF_SCHEME.to_string(),
            public_key,
            signature,
            sender_device_id: None,
            key_epoch: Some(1),
        })
        .unwrap();

    let err = handler
        .import_invitation_code(&effects, &code)
        .await
        .expect_err("expired signed invite code must be rejected");
    assert!(err.to_string().contains("invite code expired"));
}

#[tokio::test]
async fn production_import_rejects_tampered_signed_invitation_type() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

    let authority = create_test_authority(246);
    let temp = tempfile::tempdir().unwrap();
    let config = AgentConfig {
        device_id: authority.device_id(),
        storage: StorageConfig {
            base_path: temp.path().join("aura"),
            ..Default::default()
        },
        ..Default::default()
    };
    let effects = AuraEffectSystem::production(config, authority.authority_id()).unwrap();
    let handler = handler_for(authority);
    let (private_key, public_key) = effects.ed25519_generate_keypair().await.unwrap();
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-tampered-type-prod"),
        sender_id: AuthorityId::new_from_entropy(hash(&public_key)),
        context_id: None,
        invitation_type: InvitationType::Contact { nickname: None },
        expires_at: Some(4_102_444_800_000),
        message: None,
    };
    let signature = aura_signature::sign_ed25519_transcript(
        &effects,
        &shareable.signing_transcript(),
        &private_key,
    )
    .await
    .unwrap();
    let code = shareable
        .to_signed_code(ShareableInvitationSenderProof {
            scheme: ShareableInvitation::SENDER_PROOF_SCHEME.to_string(),
            public_key,
            signature,
            sender_device_id: None,
            key_epoch: Some(1),
        })
        .unwrap();
    let mut parts: Vec<&str> = code.split(':').collect();
    let mut envelope: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[2]).unwrap()).unwrap();
    envelope["payload"]["invitation_type"] =
        serde_json::to_value(InvitationType::Guardian {
            subject_authority: AuthorityId::new_from_entropy([247u8; 32]),
        })
        .unwrap();
    let tampered_json = serde_json::to_vec(&envelope).unwrap();
    let tampered_payload = URL_SAFE_NO_PAD.encode(tampered_json);
    parts[2] = &tampered_payload;
    let tampered_code = parts.join(":");

    let err = handler
        .import_invitation_code(&effects, &tampered_code)
        .await
        .expect_err("tampered signed invite type must be rejected");
    assert!(err.to_string().contains("sender proof is invalid"));
}

#[tokio::test]
async fn production_import_rejects_signed_channel_replay_against_another_context() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

    let authority = create_test_authority(248);
    let temp = tempfile::tempdir().unwrap();
    let config = AgentConfig {
        device_id: authority.device_id(),
        storage: StorageConfig {
            base_path: temp.path().join("aura"),
            ..Default::default()
        },
        ..Default::default()
    };
    let effects = AuraEffectSystem::production(config, authority.authority_id()).unwrap();
    let handler = handler_for(authority);
    let (private_key, public_key) = effects.ed25519_generate_keypair().await.unwrap();
    let original_context = ContextId::new_from_entropy([249u8; 32]);
    let replay_context = ContextId::new_from_entropy([250u8; 32]);
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-channel-replay-prod"),
        sender_id: AuthorityId::new_from_entropy(hash(&public_key)),
        context_id: Some(original_context),
        invitation_type: InvitationType::Channel {
            home_id: ChannelId::from_bytes([251u8; 32]),
            nickname_suggestion: None,
            bootstrap: None,
            home: false,
        },
        expires_at: Some(4_102_444_800_000),
        message: None,
    };
    let signature = aura_signature::sign_ed25519_transcript(
        &effects,
        &shareable.signing_transcript(),
        &private_key,
    )
    .await
    .unwrap();
    let code = shareable
        .to_signed_code(ShareableInvitationSenderProof {
            scheme: ShareableInvitation::SENDER_PROOF_SCHEME.to_string(),
            public_key,
            signature,
            sender_device_id: None,
            key_epoch: Some(1),
        })
        .unwrap();
    let mut parts: Vec<&str> = code.split(':').collect();
    let mut envelope: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[2]).unwrap()).unwrap();
    envelope["payload"]["context_id"] = serde_json::to_value(replay_context).unwrap();
    let tampered_json = serde_json::to_vec(&envelope).unwrap();
    let tampered_payload = URL_SAFE_NO_PAD.encode(tampered_json);
    parts[2] = &tampered_payload;
    let tampered_code = parts.join(":");

    let err = handler
        .import_invitation_code(&effects, &tampered_code)
        .await
        .expect_err("replayed channel invite context must be rejected");
    assert!(err.to_string().contains("sender proof is invalid"));
}

#[test]
fn sender_hint_list_yields_every_transport_type() {
    let hints = InvitationHandler::transport_hints_from_sender_hint(
        "tcp://127.0.0.1:1,ws://127.0.0.1:2",
    );
    assert_eq!(hints.len(), 2);
    assert!(matches!(hints[0], TransportHint::TcpDirect { .. }));
    assert!(matches!(hints[1], TransportHint::WebSocketDirect { .. }));
}

#[tokio::test]
async fn sender_hint_suffix_does_not_overwrite_trusted_descriptor_route() {
    let authority = create_test_authority(252);
    let effects = effects_for(&authority);
    let manager = RendezvousManager::new_with_default_udp(
        authority.authority_id(),
        RendezvousManagerConfig::default(),
        Arc::new(effects.time_effects().clone()),
    );
    effects.attach_rendezvous_manager(manager.clone());
    let peer = AuthorityId::new_from_entropy([253u8; 32]);
    let peer_context = default_context_id_for_authority(peer);
    manager
        .cache_descriptor(RendezvousDescriptor {
            authority_id: peer,
            device_id: None,
            context_id: peer_context,
            transport_hints: vec![TransportHint::tcp_direct("127.0.0.1:55001").unwrap()],
            handshake_psk_commitment: [7u8; 32],
            public_key: [8u8; 32],
            valid_from: 1,
            valid_until: u64::MAX,
            nonce: [9u8; 32],
            nickname_suggestion: None,
        })
        .await
        .unwrap();

    let handler = handler_for(authority);
    handler
        .cache_verified_peer_descriptor_for_peer(
            effects.as_ref(),
            peer,
            Some(DeviceId::new_from_entropy([254u8; 32])),
            Some("tcp://127.0.0.1:55002"),
            10,
        )
        .await;

    let descriptor = manager.get_descriptor(peer_context, peer).await.unwrap();
    assert!(matches!(
        descriptor.transport_hints.as_slice(),
        [TransportHint::TcpDirect { addr, .. }] if addr.to_string() == "127.0.0.1:55001"
    ));
}

#[test]
fn shareable_invitation_invalid_format() {
    // Missing parts
    assert_eq!(
        ShareableInvitation::from_code("aura:v1").unwrap_err(),
        ShareableInvitationError::InvalidFormat
    );

    // Wrong prefix
    assert_eq!(
        ShareableInvitation::from_code("badprefix:v1:abc").unwrap_err(),
        ShareableInvitationError::InvalidFormat
    );

    // Invalid version format
    assert_eq!(
        ShareableInvitation::from_code("aura:1:abc").unwrap_err(),
        ShareableInvitationError::InvalidFormat
    );
}

#[test]
fn shareable_invitation_unsupported_version() {
    // Version 99 doesn't exist
    assert_eq!(
        ShareableInvitation::from_code("aura:v99:abc").unwrap_err(),
        ShareableInvitationError::UnsupportedVersion(99)
    );
}

#[test]
fn shareable_invitation_decoding_failed() {
    // Not valid base64
    assert_eq!(
        ShareableInvitation::from_code("aura:v1:!!!invalid!!!").unwrap_err(),
        ShareableInvitationError::DecodingFailed
    );
}

#[test]
fn shareable_invitation_parsing_failed() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    // Valid base64 but not valid JSON
    let bad_json = URL_SAFE_NO_PAD.encode("not json");
    let code = format!("aura:v1:{}", bad_json);
    assert_eq!(
        ShareableInvitation::from_code(&code).unwrap_err(),
        ShareableInvitationError::ParsingFailed
    );
}

#[test]
fn shareable_invitation_rejects_oversized_payload_before_decode() {
    let payload = "A".repeat(ShareableInvitation::MAX_PAYLOAD_BASE64_CHARS + 1);
    let code = format!("aura:v1:{payload}");
    assert_eq!(
        ShareableInvitation::from_code(&code).unwrap_err(),
        ShareableInvitationError::SizeLimitExceeded("payload")
    );
}

#[test]
fn shareable_invitation_rejects_oversized_json_fields() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-large-message"),
        sender_id: AuthorityId::new_from_entropy([53u8; 32]),
        context_id: None,
        invitation_type: InvitationType::Contact { nickname: None },
        expires_at: None,
        message: Some("x".repeat(ShareableInvitation::MAX_MESSAGE_BYTES + 1)),
    };
    let json = serde_json::to_vec(&shareable).expect("json");
    let code = format!("aura:v1:{}", URL_SAFE_NO_PAD.encode(json));

    assert_eq!(
        ShareableInvitation::from_code(&code).unwrap_err(),
        ShareableInvitationError::SizeLimitExceeded("message")
    );
}

#[test]
fn shareable_invitation_rejects_many_colon_segments() {
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-extra-segments"),
        sender_id: AuthorityId::new_from_entropy([54u8; 32]),
        context_id: None,
        invitation_type: InvitationType::Contact { nickname: None },
        expires_at: None,
        message: None,
    };
    let code = format!(
        "{}:too:many:segments",
        shareable.to_code().expect("shareable invitation should serialize")
    );

    assert_eq!(
        ShareableInvitation::from_code(&code).unwrap_err(),
        ShareableInvitationError::InvalidFormat
    );
}

#[test]
fn shareable_invitation_rejects_oversized_sender_hint_segment() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

    let sender_device_id = DeviceId::new_from_entropy([55u8; 32]);
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-large-hint"),
        sender_id: AuthorityId::new_from_entropy([56u8; 32]),
        context_id: None,
        invitation_type: InvitationType::Contact { nickname: None },
        expires_at: None,
        message: None,
    };
    let code = format!(
        "{}:{}:{}",
        shareable.to_code().expect("shareable invitation should serialize"),
        URL_SAFE_NO_PAD.encode("x".repeat(ShareableInvitation::MAX_SENDER_HINT_BYTES + 1)),
        URL_SAFE_NO_PAD.encode(sender_device_id.to_string())
    );

    assert_eq!(
        ShareableInvitation::from_code(&code).unwrap_err(),
        ShareableInvitationError::SizeLimitExceeded("sender_hint")
    );
}

#[test]
fn shareable_invitation_accepts_max_size_message() {
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-max-message"),
        sender_id: AuthorityId::new_from_entropy([57u8; 32]),
        context_id: None,
        invitation_type: InvitationType::Contact {
            nickname: Some("n".repeat(ShareableInvitation::MAX_NICKNAME_BYTES)),
        },
        expires_at: None,
        message: Some("m".repeat(ShareableInvitation::MAX_MESSAGE_BYTES)),
    };
    let code = shareable
        .to_code()
        .expect("max-size shareable invitation should serialize");
    let decoded = ShareableInvitation::from_code(&code)
        .expect("max-size shareable invitation should parse");

    assert_eq!(decoded.message, shareable.message);
    assert_eq!(decoded.invitation_id, shareable.invitation_id);
}

#[test]
fn shareable_invitation_from_invitation() {
    let invitation = Invitation {
        invitation_id: InvitationId::new("inv-from-full"),
        context_id: ContextId::new_from_entropy([50u8; 32]),
        sender_id: AuthorityId::new_from_entropy([51u8; 32]),
        receiver_id: AuthorityId::new_from_entropy([52u8; 32]),
        invitation_type: InvitationType::Contact {
            nickname: Some("bob".to_string()),
        },
        status: InvitationStatus::Pending,
        created_at: 1600000000000,
        expires_at: Some(1700000000000),
        receiver_nickname: None,
        message: Some("Hi Bob!".to_string()),
    };

    let shareable = ShareableInvitation::from(&invitation);
    assert_eq!(shareable.invitation_id, invitation.invitation_id);
    assert_eq!(shareable.sender_id, invitation.sender_id);
    assert_eq!(shareable.expires_at, invitation.expires_at);
    assert_eq!(shareable.message, invitation.message);

    // Round-trip via code
    let code = shareable
        .to_code()
        .expect("shareable invitation should serialize");
    let decoded = ShareableInvitation::from_code(&code).unwrap();
    assert_eq!(decoded.invitation_id, invitation.invitation_id);
}

// Test that importing and accepting multiple contact invitations works
// sequentially. This mimics the TUI demo-mode flow where Alice's invitation is
// imported and accepted before Carol's, and both should succeed without
// interfering with each other.
large_stack_async_test!(importing_multiple_contact_invitations_sequentially, {
    let alice = contact_pair(150).await;
    let carol = alice.with_new_inviter(152).await;
    let own_authority = alice.receiver_id;
    let (alice_sender_id, carol_sender_id) = (alice.sender_id, carol.sender_id);

    // Import and accept Alice's invitation
    let alice_invitation = alice.create_contact_invitation().await;
    let alice_imported = alice.import(&alice.signed_code(&alice_invitation).await).await;
    assert_eq!(alice_imported.sender_id, alice_sender_id);
    alice
        .accept_with_responding_inviter(&alice_imported.invitation_id)
        .await
        .expect("Alice accept should succeed");

    // Import and accept Carol's invitation (this is the step that was failing in TUI)
    let carol_invitation = carol.create_contact_invitation().await;
    let carol_imported = carol.import(&carol.signed_code(&carol_invitation).await).await;
    assert_eq!(carol_imported.sender_id, carol_sender_id);

    // This is the critical assertion - Carol's accept should work after Alice's
    carol
        .accept_with_responding_inviter(&carol_imported.invitation_id)
        .await
        .expect("Carol accept should succeed after Alice");

    // Verify both contacts were added
    let committed = alice
        .receiver_effects
        .load_committed_facts(own_authority)
        .await
        .unwrap();

    let mut contact_facts: Vec<ContactFact> = Vec::new();
    for fact in committed {
        let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = fact.content
        else {
            continue;
        };

        if envelope.type_id.as_str() != CONTACT_FACT_TYPE_ID {
            continue;
        }

        if let Some(contact_fact) = ContactFact::from_envelope(&envelope) {
            contact_facts.push(contact_fact);
        }
    }

    // Verify we have both Alice and Carol as contacts
    // (other tests may add additional contact facts, so we just verify these two are present)
    let contact_ids: Vec<AuthorityId> = contact_facts
        .iter()
        .filter_map(|f| match f {
            ContactFact::Added { contact_id, .. } => Some(*contact_id),
            _ => None,
        })
        .collect();

    assert!(
        contact_ids.contains(&alice_sender_id),
        "Alice should be in contacts, found: {:?}",
        contact_ids
    );
    assert!(
        contact_ids.contains(&carol_sender_id),
        "Carol should be in contacts, found: {:?}",
        contact_ids
    );
});

/// Regression (work/8.md task 15): the accepter's nickname travels with the
/// signed acceptance, and relabelling it invalidates the signature.
#[tokio::test]
async fn contact_acceptance_signature_binds_accepter_nickname() {
    let sender = create_test_authority(171);
    let receiver = create_test_authority(172);
    let receiver_effects = effects_for(&receiver);
    let invitation = device_enrollment_test_invitation(
        "inv-contact-nickname-binding",
        sender.authority_id(),
        receiver.authority_id(),
        receiver.device_id(),
    );
    bootstrap_test_signing_authority(&receiver_effects, receiver.authority_id()).await;
    let signed = contact_invitation_acceptance_transcript(
        &invitation,
        receiver.authority_id(),
        Some("Barbara".to_string()),
    );
    let signature = sign_invitation_acceptance_transcript(
        receiver_effects.as_ref(),
        receiver.authority_id(),
        &signed,
    )
    .await
    .expect("acceptance should sign");

    verify_invitation_acceptance_signature(
        receiver_effects.as_ref(),
        receiver.authority_id(),
        &contact_invitation_acceptance_transcript(
            &invitation,
            receiver.authority_id(),
            Some("Barbara".to_string()),
        ),
        &signature,
    )
    .await
    .expect("matching nickname should verify");

    assert!(verify_invitation_acceptance_signature(
        receiver_effects.as_ref(),
        receiver.authority_id(),
        &contact_invitation_acceptance_transcript(
            &invitation,
            receiver.authority_id(),
            Some("Mallory".to_string()),
        ),
        &signature,
    )
    .await
    .is_err());
}

/// Regression (work/8.md task 2 / M1): a guardian accepts with a signed
/// recovery key, and the principal stores it only if the signature verifies.
#[tokio::test]
async fn guardian_acceptance_records_verified_recovery_key() {
    use aura_core::effects::{CryptoCoreEffects, StorageCoreEffects};

    let principal = create_test_authority(181);
    let guardian = create_test_authority(182);
    let principal_effects = effects_for(&principal);
    let guardian_effects = effects_for(&guardian);
    let mut invitation = device_enrollment_test_invitation(
        "inv-guardian-signed",
        principal.authority_id(),
        guardian.authority_id(),
        guardian.device_id(),
    );
    invitation.invitation_type = InvitationType::Guardian {
        subject_authority: principal.authority_id(),
    };

    let (private_key, public_key) = guardian_effects.ed25519_generate_keypair().await.unwrap();
    let transcript = super::guardian::GuardianInvitationAcceptanceTranscript {
        invitation: &invitation,
        guardian: guardian.authority_id(),
        recovery_public_key: &public_key,
    };
    let signature =
        aura_signature::sign_ed25519_transcript(guardian_effects.as_ref(), &transcript, &private_key)
            .await
            .unwrap();
    let accept = GuardianAccept {
        invitation_id: invitation.invitation_id.clone(),
        signature,
        recovery_public_key: public_key.clone(),
    };

    // A substituted key is rejected and nothing is stored.
    let (_, other_key) = guardian_effects.ed25519_generate_keypair().await.unwrap();
    let mut swapped = accept.clone();
    swapped.recovery_public_key = other_key;
    assert!(super::guardian::verify_and_record_guardian_acceptance(
        principal_effects.as_ref(),
        &invitation,
        &swapped,
    )
    .await
    .is_err());
    let key_path = crate::handlers::recovery_guardian_public_key_storage_key(guardian.authority_id());
    assert!(principal_effects.retrieve(&key_path).await.unwrap().is_none());

    // The genuine acceptance is recorded for guardian setup.
    super::guardian::verify_and_record_guardian_acceptance(
        principal_effects.as_ref(),
        &invitation,
        &accept,
    )
    .await
    .expect("valid guardian acceptance should verify");
    assert_eq!(
        principal_effects.retrieve(&key_path).await.unwrap(),
        Some(public_key)
    );
}
/// Run the guardian choreography between two runtimes with real device ids,
/// starting the guardian after `guardian_delay`.
async fn run_guardian_choreography(seed: u8, guardian_delay: std::time::Duration) {
    let shared_transport = crate::runtime::SharedTransport::new();
    let principal_id = AuthorityId::new_from_entropy([seed; 32]);
    let guardian_id = AuthorityId::new_from_entropy([seed + 1; 32]);
    let principal_device = DeviceId::new_from_entropy([seed + 2; 32]);
    let guardian_device = DeviceId::new_from_entropy([seed + 3; 32]);
    let principal_effects =
        crate::testing::simulation_effect_system_with_shared_transport_for_authority_arc(
            &AgentConfig {
                device_id: principal_device,
                ..Default::default()
            },
            principal_id,
            shared_transport.clone(),
        );
    let guardian_effects =
        crate::testing::simulation_effect_system_with_shared_transport_for_authority_arc(
            &AgentConfig {
                device_id: guardian_device,
                ..Default::default()
            },
            guardian_id,
            shared_transport.clone(),
        );
    install_full_invitation_biscuit_cache(&principal_effects, principal_id);
    install_full_invitation_biscuit_cache(&guardian_effects, guardian_id);
    let principal_handler = handler_for(AuthorityContext::new_with_device(
        principal_id,
        principal_device,
    ));
    let guardian_handler =
        handler_for(AuthorityContext::new_with_device(guardian_id, guardian_device));

    let mut invitation = device_enrollment_test_invitation(
        "inv-guardian-cross-runtime",
        principal_id,
        guardian_id,
        guardian_device,
    );
    invitation.invitation_type = InvitationType::Guardian {
        subject_authority: principal_id,
    };

    let (principal_result, guardian_result) = tokio::join!(
        principal_handler.execute_guardian_invitation_principal(principal_effects.clone(), &invitation),
        async {
            tokio::time::sleep(guardian_delay).await;
            guardian_handler
                .execute_guardian_invitation_guardian(guardian_effects.clone(), &invitation)
                .await
        },
    );
    guardian_result.expect("guardian side completes");
    principal_result.expect("principal side completes");

    let key_path = crate::handlers::recovery_guardian_public_key_storage_key(guardian_id);
    assert!(
        principal_effects.retrieve(&key_path).await.unwrap().is_some(),
        "principal records the guardian recovery key"
    );
    let bound = principal_effects
        .load_committed_facts(principal_id)
        .await
        .unwrap()
        .into_iter()
        .any(|fact| {
            matches!(
                fact.content,
                FactContent::Relational(RelationalFact::Protocol(
                    aura_journal::ProtocolRelationalFact::GuardianBinding { guardian_id: bound, .. }
                )) if bound == guardian_id
            )
        });
    assert!(bound, "a verified acceptance commits the guardian binding");
}

large_stack_async_test!(guardian_choreography_completes_when_guardian_accepts_late, {
    // Longer than one principal receive window, as when a person accepts later.
    run_guardian_choreography(191, std::time::Duration::from_millis(6_000)).await;
});

// Regression (work/8.md task 7): the new device re-imports the enrollment code
// after its runtime switches to the subject authority; the invitation must
// still name the authority that was invited, or the initiator rejects the
// signed acceptance.
large_stack_async_test!(reimported_device_enrollment_keeps_invited_authority, {
    let mut invitation = test_device_enrollment_invitation("reimport-after-switch");
    let invited = invitation.receiver_id;
    if let InvitationType::DeviceEnrollment {
        invitee_authority, ..
    } = &mut invitation.invitation_type
    {
        *invitee_authority = Some(invited);
    }
    // The importing runtime already runs as the subject authority.
    let subject_context = AuthorityContext::new(invitation.sender_id);
    let effects = effects_for(&subject_context);
    let handler = handler_for(subject_context);

    let imported = handler
        .import_invitation_code(&effects, &unsigned_test_code_for_invitation(&invitation))
        .await
        .expect("device enrollment code should import");

    assert_eq!(imported.receiver_id, invited);
});

// Regression (work/8.md task 32): listing invitations re-caches persisted
// records; a device enrollment must be cached with its secure payload
// restored, or accepting it skips the baseline tree and key package.
large_stack_async_test!(listing_restores_device_enrollment_payload_before_caching, {
    let authority = create_test_authority(156);
    let effects = effects_for(&authority);
    let handler = handler_for(authority.clone());
    let invitation = test_device_enrollment_invitation("listed-device-enrollment");
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: invitation.invitation_id.clone(),
        sender_id: invitation.sender_id,
        context_id: Some(invitation.context_id),
        invitation_type: invitation.invitation_type.clone(),
        expires_at: invitation.expires_at,
        message: invitation.message.clone(),
    };
    InvitationCacheHandler::persist_imported_invitation(
        effects.as_ref(),
        authority.authority_id(),
        &StoredImportedInvitation::pending(
            shareable,
            invitation.created_at,
            ImportedSenderTrust::SelfCertified,
        ),
    )
    .await
    .expect("persist imported device enrollment");

    let _ = handler.list_with_storage(effects.as_ref()).await;

    let cached = handler
        .invitation_cache
        .get_invitation(&invitation.invitation_id)
        .await
        .expect("listing caches the invitation");
    assert_device_enrollment_payload_restored(&cached.invitation_type);
});

// Regression (work/8.md task 20, D10): a revoked contact invitation's code no
// longer creates a contact when the invitee accepts it.
large_stack_async_test!(revoked_contact_invitation_acceptance_adds_no_contact, {
    let pair = contact_pair(130).await;
    let (sender_id, receiver_id) = (pair.sender_id, pair.receiver_id);
    let invitation = pair.create_contact_invitation().await;
    let imported = pair.import(&pair.signed_code(&invitation).await).await;

    // Simulation storage persists across runs, so compare against what each
    // side already held before this acceptance.
    let contacts_added = |facts: &[aura_journal::fact::Fact], owner: AuthorityId| {
        facts
            .iter()
            .filter(|fact| {
                let FactContent::Relational(RelationalFact::Generic { envelope, .. }) =
                    &fact.content
                else {
                    return false;
                };
                envelope.type_id.as_str() == CONTACT_FACT_TYPE_ID
                    && matches!(
                        ContactFact::from_envelope(envelope),
                        Some(ContactFact::Added { owner_id, .. }) if owner_id == owner
                    )
            })
            .count()
    };
    let sender_before = contacts_added(
        &pair.sender_effects.load_committed_facts(sender_id).await.unwrap(),
        sender_id,
    );
    let receiver_before = contacts_added(
        &pair.receiver_effects.load_committed_facts(receiver_id).await.unwrap(),
        receiver_id,
    );

    // The sender revokes before the acceptance reaches it.
    pair.sender_handler
        .cancel_invitation(pair.sender_effects.clone(), &invitation.invitation_id)
        .await
        .unwrap();
    let error = pair
        .accept_with_responding_inviter(&imported.invitation_id)
        .await
        .expect_err("accepting a revoked invitation must fail");
    assert!(
        error.to_string().contains("revoked this contact invitation"),
        "expected a typed revocation failure, got: {error}"
    );

    let sender_after = contacts_added(
        &pair.sender_effects.load_committed_facts(sender_id).await.unwrap(),
        sender_id,
    );
    let receiver_after = contacts_added(
        &pair.receiver_effects.load_committed_facts(receiver_id).await.unwrap(),
        receiver_id,
    );
    assert_eq!(sender_after, sender_before, "a revoked invitation must not add a contact for the sender");
    assert_eq!(receiver_after, receiver_before, "a revoked invitation must not add a contact for the invitee");
    let settled = pair
        .receiver_handler
        .get_invitation_with_storage(pair.receiver_effects.as_ref(), &imported.invitation_id)
        .await
        .expect("imported invitation should remain readable");
    assert_eq!(settled.status, InvitationStatus::Cancelled);
});

large_stack_async_test!(unanswered_contact_acceptance_fails_typed_and_stays_pending, {
    let pair = contact_pair(54).await;
    let invitation = pair.create_contact_invitation().await;
    let imported = pair.import(&pair.signed_code(&invitation).await).await;

    // No inviter processes the acceptance.
    let error = timeout(
        Duration::from_secs(90),
        Box::pin(
            pair.receiver_handler
                .accept_invitation(pair.receiver_effects.clone(), &imported.invitation_id),
        ),
    )
    .await
    .expect("the confirmation wait must be bounded")
    .expect_err("an unanswered acceptance must not succeed");
    assert!(
        error.to_string().contains("did not confirm this contact invitation"),
        "expected a typed unconfirmed failure, got: {error}"
    );

    let stored = pair
        .receiver_handler
        .get_invitation_with_storage(pair.receiver_effects.as_ref(), &imported.invitation_id)
        .await
        .expect("imported invitation should remain readable");
    assert_eq!(stored.status, InvitationStatus::Pending);
});

large_stack_async_test!(invitee_applies_only_authentic_responses_to_its_pending_acceptance, {
    use super::contact_confirmation::{
        contact_acceptance_digest, ContactInvitationDecision, ContactInvitationResponse,
        ContactInvitationResponseTranscript, CONTACT_INVITATION_RESPONSE_CONTENT_TYPE,
    };

    let pair = contact_pair(56).await;
    let invitation = pair.create_contact_invitation().await;
    let imported = pair.import(&pair.signed_code(&invitation).await).await;

    // The invitee is awaiting a response to this acceptance.
    let acceptance_digest = contact_acceptance_digest(b"the acceptance we sent");
    let mut stored = InvitationHandler::load_imported_invitation(
        pair.receiver_effects.as_ref(),
        pair.receiver_id,
        &imported.invitation_id,
        None,
    )
    .await
    .expect("imported invitation should be stored");
    stored.pending_acceptance_digest = Some(acceptance_digest);
    InvitationHandler::persist_imported_invitation(
        pair.receiver_effects.as_ref(),
        pair.receiver_id,
        &stored,
    )
    .await
    .unwrap();

    let (inviter_key, _) = crate::handlers::rendezvous_identity::retrieve_identity_keys(
        pair.sender_effects.as_ref(),
        &pair.sender_id,
    )
    .await
    .expect("inviter identity keys should exist");
    let (forger_key, _) = pair.sender_effects.ed25519_generate_keypair().await.unwrap();
    let respond = |digest: [u8; 32], invitation_id: InvitationId, key: Vec<u8>, source| {
        let effects = pair.sender_effects.clone();
        let (inviter_id, acceptor_id) = (pair.sender_id, pair.receiver_id);
        async move {
            let mut response = ContactInvitationResponse {
                invitation_id: invitation_id.clone(),
                inviter_id,
                acceptor_id,
                decision: ContactInvitationDecision::Confirmed,
                acceptance_digest: digest,
                signature: Vec::new(),
            };
            response.signature = aura_signature::sign_ed25519_transcript(
                effects.as_ref(),
                &ContactInvitationResponseTranscript(&response),
                &key,
            )
            .await
            .unwrap();
            let mut metadata = HashMap::new();
            metadata.insert(
                "content-type".to_string(),
                CONTACT_INVITATION_RESPONSE_CONTENT_TYPE.to_string(),
            );
            metadata.insert("invitation-id".to_string(), invitation_id.to_string());
            TransportEnvelope {
                destination: acceptor_id,
                source,
                context: default_context_id_for_authority(acceptor_id),
                payload: serde_json::to_vec(&response).unwrap(),
                metadata,
                receipt: None,
            }
        }
    };
    let apply = |envelope: TransportEnvelope| {
        let handler = &pair.receiver_handler;
        let effects = pair.receiver_effects.clone();
        async move {
            handler
                .apply_contact_invitation_response(effects.as_ref(), &envelope)
                .await
                .unwrap()
        }
    };
    let status = || async {
        pair.receiver_handler
            .get_invitation_with_storage(pair.receiver_effects.as_ref(), &imported.invitation_id)
            .await
            .unwrap()
            .status
    };
    let id = imported.invitation_id.clone();

    // Forged: signed by a key other than the code's sender proof key.
    let forged = respond(acceptance_digest, id.clone(), forger_key.clone(), pair.sender_id).await;
    assert_eq!(apply(forged).await, None);
    // Replayed: authentic, but answering a different acceptance.
    let replayed = respond([9; 32], id.clone(), inviter_key.to_vec(), pair.sender_id).await;
    assert_eq!(apply(replayed).await, None);
    // Wrong source: an authentic response relayed by another authority.
    let relayed = respond(acceptance_digest, id.clone(), inviter_key.to_vec(), pair.receiver_id).await;
    assert_eq!(apply(relayed).await, None);
    // Unrelated invitation id.
    let unrelated = respond(
        acceptance_digest,
        InvitationId::new("not-our-invitation"),
        inviter_key.to_vec(),
        pair.sender_id,
    )
    .await;
    assert_eq!(apply(unrelated).await, None);
    assert_eq!(status().await, InvitationStatus::Pending);

    // Authentic and answering our acceptance: applied once.
    let authentic = respond(acceptance_digest, id.clone(), inviter_key.to_vec(), pair.sender_id).await;
    assert_eq!(apply(authentic.clone()).await, Some(ContactInvitationDecision::Confirmed));
    assert_eq!(status().await, InvitationStatus::Accepted);
    // A duplicate of it is ignored (idempotent).
    assert_eq!(apply(authentic).await, None);
});

#[test]
fn inviter_answers_each_settled_status_with_a_typed_decision() {
    use super::contact_confirmation::{settled_contact_invitation_decision, ContactInvitationDecision};
    use ContactInvitationDecision::*;
    let decide = settled_contact_invitation_decision;
    assert_eq!(decide(&InvitationStatus::Pending, false, false), None);
    assert_eq!(decide(&InvitationStatus::Pending, true, false), Some(Expired));
    assert_eq!(decide(&InvitationStatus::Expired, false, false), Some(Expired));
    assert_eq!(decide(&InvitationStatus::Cancelled, false, false), Some(Revoked));
    assert_eq!(decide(&InvitationStatus::Declined, false, false), Some(AlreadySettled));
    assert_eq!(decide(&InvitationStatus::Accepted, false, false), Some(AlreadySettled));
    // A duplicate from the acceptor who already accepted is re-confirmed.
    assert_eq!(decide(&InvitationStatus::Accepted, false, true), Some(Confirmed));
}

#[tokio::test]
async fn home_invitation_acceptance_commits_durable_home_membership() {
    let own = AuthorityId::new_from_entropy([61u8; 32]);
    let inviter = AuthorityId::new_from_entropy([62u8; 32]);
    let effects = Arc::new(
        AuraEffectSystem::simulation_for_test_for_authority(&AgentConfig::default(), own).unwrap(),
    );
    let handler = handler_for_id(own);
    let invite = ChannelInviteDetails {
        context_id: ContextId::new_from_entropy([63u8; 32]),
        channel_id: ChannelId::from_bytes([64u8; 32]),
        home_name: "Den".to_string(),
        sender_id: inviter,
        bootstrap: None,
        home: true,
    };
    handler
        .commit_home_membership(effects.as_ref(), &invite, own, true)
        .await
        .unwrap();

    let social: Vec<aura_social::SocialFact> = effects
        .load_committed_facts(own)
        .await
        .unwrap()
        .into_iter()
        .filter_map(|fact| match fact.content {
            FactContent::Relational(RelationalFact::Generic { envelope, .. })
                if envelope.type_id.as_str() == aura_social::SOCIAL_FACT_TYPE_ID =>
            {
                aura_social::SocialFact::from_envelope(&envelope)
            }
            _ => None,
        })
        .collect();
    assert!(social.iter().any(|fact| matches!(
        fact,
        aura_social::SocialFact::HomeCreated { home_id, creator_id, name, .. }
            if home_id.as_bytes() == &[64u8; 32] && *creator_id == inviter && name == "Den"
    )));
    assert!(social.iter().any(|fact| matches!(
        fact,
        aura_social::SocialFact::MemberJoined { authority_id, home_id, .. }
            if *authority_id == own && home_id.as_bytes() == &[64u8; 32]
    )));
}

large_stack_async_test!(existing_contact_can_import_a_new_code_signed_by_its_confirmed_key, {
    let pair = contact_pair(66).await;
    let first = pair.create_contact_invitation().await;
    let imported = pair.import(&pair.signed_code(&first).await).await;
    pair.accept_with_responding_inviter(&imported.invitation_id)
        .await
        .expect("first contact invitation should be confirmed");

    // The sender is now a contact; a second code signed by the same key imports.
    let second = pair.create_contact_invitation().await;
    let reimported = pair
        .receiver_handler
        .import_invitation_code(&pair.receiver_effects, &pair.signed_code(&second).await)
        .await
        .expect("a confirmed contact's new code should import");
    assert_eq!(reimported.sender_id, pair.sender_id);
});

#[tokio::test]
async fn verified_peer_descriptors_survive_a_runtime_restart() {
    let own = AuthorityId::new_from_entropy([231u8; 32]);
    let peer = AuthorityId::new_from_entropy([232u8; 32]);
    let effects = Arc::new(
        AuraEffectSystem::simulation_for_test_for_authority(&AgentConfig::default(), own).unwrap(),
    );
    let handler = handler_for_id(own);
    let _first = attach_test_rendezvous_manager(effects.as_ref(), own).await;
    handler
        .cache_verified_peer_descriptor_for_peer(
            effects.as_ref(),
            peer,
            None,
            Some("tcp://192.168.1.30:55031"),
            1_700_000_000_000,
        )
        .await;

    // A restart: a fresh rendezvous manager has no descriptors in memory.
    let _second = attach_test_rendezvous_manager(effects.as_ref(), own).await;
    let manager = effects.rendezvous_manager().unwrap();
    assert!(manager.get_any_descriptor_for_authority(peer).await.is_none());

    handler.restore_verified_peer_descriptors(effects.as_ref()).await;
    assert!(
        manager.get_any_descriptor_for_authority(peer).await.is_some(),
        "the persisted verified hint is re-cached"
    );
}

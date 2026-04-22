use super::*;
use crate::core::AgentConfig;
use crate::reactive::app_signal_views;
use crate::runtime::effects::AuraEffectSystem;
use crate::runtime::services::ceremony_runner::CeremonyRunner;
use crate::runtime::services::CeremonyTracker;
use crate::runtime::TaskSupervisor;
use aura_app::signal_defs::{register_app_signals, HOMES_SIGNAL, INVITATIONS_SIGNAL};
use aura_app::views::home::{HomeRole, HomesState};
use aura_chat::{ChatFact, CHAT_FACT_TYPE_ID};
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::types::identifiers::{
    AuthorityId, CeremonyId, ChannelId, ContextId, InvitationId,
};
use aura_core::DeviceId;
use aura_effects::reactive::ReactiveHandler;
use aura_invitation::guards::{EffectCommand, GuardOutcome};
use aura_journal::fact::{FactContent, RelationalFact};
use aura_journal::DomainFact;
use aura_relational::{ContactFact, CONTACT_FACT_TYPE_ID};
use aura_social::moderation::facts::HomeGrantModeratorFact;
use std::collections::HashMap;
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
        device_id: authority.device_id(),
        ..Default::default()
    };
    let effects = Arc::new(
        AuraEffectSystem::production_for_authority(config, authority.authority_id()).unwrap(),
    );
    install_full_invitation_biscuit_cache(&effects, authority.authority_id());
    effects
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
            handshake_psk_commitment: [0u8; 32],
            public_key: [0u8; 32],
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
                handshake_psk_commitment: [0u8; 32],
                public_key: [0u8; 32],
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
    assert!(result.is_ok());
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
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_execute_multiple_commands() {
    let authority = create_test_authority(138);
    let shared_transport = crate::runtime::SharedTransport::new();
    let config = AgentConfig::default();
    let peer = AuthorityId::new_from_entropy([139u8; 32]);
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
    assert!(result.is_ok());

    let received = timeout(Duration::from_secs(2), _peer_effects.receive_envelope())
        .await
        .expect("invitation envelope should arrive before timeout")
        .expect("invitation delivery should not fail receipt validation");
    assert_eq!(received.destination, peer);
    assert_eq!(received.source, authority.authority_id());
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

#[tokio::test]
async fn invitation_can_be_accepted() {
    let sender_context = create_test_authority(93);
    let receiver_id = AuthorityId::new_from_entropy([94u8; 32]);
    let receiver_context = AuthorityContext::new(receiver_id);

    let sender_effects = effects_for(&sender_context);
    let receiver_effects = effects_for(&receiver_context);
    let sender_handler = handler_for(sender_context);
    let receiver_handler = handler_for(receiver_context);

    let invitation = sender_handler
        .create_invitation(
            sender_effects,
            receiver_id,
            InvitationType::Contact {
                nickname: Some("receiver".to_string()),
            },
            None,
            None,
        )
        .await
        .unwrap();
    let code = InvitationServiceApi::export_invitation(&invitation)
        .expect("shareable invitation should serialize");
    let imported = receiver_handler
        .import_invitation_code(&receiver_effects, &code)
        .await
        .unwrap();

    let result = receiver_handler
        .accept_invitation(receiver_effects, &imported.invitation_id)
        .await
        .unwrap();

    assert_eq!(result.new_status, InvitationStatus::Accepted);
}

#[tokio::test]
async fn invitation_can_be_declined() {
    let authority_context = create_test_authority(96);
    let effects = effects_for(&authority_context);
    let handler = InvitationHandler::new(authority_context).unwrap();

    let receiver_id = handler.authority_context().authority_id();
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
    assert!(error
        .to_string()
        .contains("guardian invitation accept follow-up failed"));
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
async fn old_format_imported_invitation_preserves_cached_terminal_status() {
    let authority_context = create_test_authority(111);
    let effects = effects_for(&authority_context);
    let handler = handler_for(authority_context.clone());
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("legacy-imported-status-preserved"),
        sender_id: AuthorityId::new_from_entropy([112u8; 32]),
        context_id: None,
        invitation_type: InvitationType::Contact {
            nickname: Some("Alice".to_string()),
        },
        expires_at: None,
        message: Some("legacy invite".to_string()),
    };
    let code = shareable
        .to_code()
        .expect("shareable invitation should serialize");
    let imported = handler
        .import_invitation_code(effects.as_ref(), &code)
        .await
        .expect("legacy invite import should succeed");

    handler
        .invitation_cache
        .update_invitation(&imported.invitation_id, |invitation| {
            invitation.status = InvitationStatus::Accepted;
            invitation.created_at = 123;
        })
        .await;

    let legacy_key = InvitationCacheHandler::imported_invitation_key(
        authority_context.authority_id(),
        &imported.invitation_id,
    );
    effects
        .store(&legacy_key, serde_json::to_vec(&shareable).unwrap())
        .await
        .unwrap();

    let listed = handler.list_with_storage(effects.as_ref()).await;
    let invitation = listed
        .into_iter()
        .find(|invitation| invitation.invitation_id == imported.invitation_id)
        .expect("legacy imported invitation should remain listable");
    assert_eq!(invitation.status, InvitationStatus::Accepted);
    assert_eq!(invitation.created_at, 123);
}

#[tokio::test]
async fn choreography_load_defaults_terminal_status_for_legacy_imports() {
    let authority_context = create_test_authority(113);
    let effects = effects_for(&authority_context);
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("legacy-imported-choreo-status"),
        sender_id: AuthorityId::new_from_entropy([114u8; 32]),
        context_id: None,
        invitation_type: InvitationType::Contact {
            nickname: Some("Alice".to_string()),
        },
        expires_at: None,
        message: Some("legacy invite".to_string()),
    };
    let legacy_key = InvitationCacheHandler::imported_invitation_key(
        authority_context.authority_id(),
        &shareable.invitation_id,
    );
    effects
        .store(&legacy_key, serde_json::to_vec(&shareable).unwrap())
        .await
        .unwrap();

    let preserved = Invitation {
        invitation_id: shareable.invitation_id.clone(),
        context_id: authority_context.default_context_id(),
        sender_id: shareable.sender_id,
        receiver_id: authority_context.authority_id(),
        invitation_type: shareable.invitation_type.clone(),
        status: InvitationStatus::Declined,
        created_at: 456,
        expires_at: shareable.expires_at,
        receiver_nickname: None,
        message: shareable.message.clone(),
    };
    let stored = InvitationHandler::load_imported_invitation(
        effects.as_ref(),
        authority_context.authority_id(),
        &shareable.invitation_id,
        Some(&preserved),
    )
    .await
    .expect("legacy imported invitation should remain loadable");
    assert_eq!(stored.status, InvitationStatus::Pending);
    assert_eq!(stored.created_at, 0);
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

#[tokio::test]
async fn importing_and_accepting_contact_invitation_commits_contact_fact() {
    let own_authority = AuthorityId::new_from_entropy([120u8; 32]);
    let config = AgentConfig::default();
    let effects =
        crate::testing::simulation_effect_system_for_authority_arc(&config, own_authority);

    let authority_context = AuthorityContext::new(own_authority);

    let handler = InvitationHandler::new(authority_context).unwrap();

    let sender_id = AuthorityId::new_from_entropy([121u8; 32]);
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-demo-contact-1"),
        sender_id,
        context_id: None,
        invitation_type: InvitationType::Contact {
            nickname: Some("Alice".to_string()),
        },
        expires_at: None,
        message: Some("Contact invitation from Alice (demo)".to_string()),
    };
    let code = shareable
        .to_code()
        .expect("shareable invitation should serialize");

    let imported = handler
        .import_invitation_code(&effects, &code)
        .await
        .unwrap();
    assert_eq!(imported.sender_id, sender_id);
    assert_eq!(imported.receiver_id, own_authority);

    accept_invitation_without_notification(&handler, effects.clone(), &imported.invitation_id)
        .await;

    let committed = effects.load_committed_facts(own_authority).await.unwrap();

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

        let contact_fact = ContactFact::from_envelope(&envelope);
        if matches!(
            contact_fact,
            Some(ContactFact::Added {
                owner_id,
                contact_id,
                ..
            }) if owner_id == own_authority && contact_id == sender_id
        ) {
            found = contact_fact;
        }
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
}

#[tokio::test]
async fn accepting_contact_invitation_notifies_sender_and_adds_contact() {
    let shared_transport = crate::runtime::SharedTransport::new();
    let config = AgentConfig::default();

    let sender_id = AuthorityId::new_from_entropy([124u8; 32]);
    let receiver_id = AuthorityId::new_from_entropy([125u8; 32]);

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
            shared_transport.clone(),
        );

    let sender_handler = handler_for_id(sender_id);
    let receiver_handler = handler_for_id(receiver_id);

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

    let code = InvitationServiceApi::export_invitation(&invitation)
        .expect("shareable invitation should serialize");
    let imported = receiver_handler
        .import_invitation_code(&receiver_effects, &code)
        .await
        .unwrap();

    receiver_handler
        .accept_invitation(receiver_effects.clone(), &imported.invitation_id)
        .await
        .unwrap();
    receiver_handler
        .notify_contact_invitation_acceptance(
            receiver_effects.as_ref(),
            &imported.invitation_id,
        )
        .await
        .unwrap();
    let processed = sender_handler
        .process_contact_invitation_acceptances(sender_effects.clone())
        .await
        .unwrap();
    assert!(
        processed >= 1,
        "expected at least one transported acceptance envelope to be processed"
    );

    let committed = sender_effects
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
}

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

#[tokio::test]
async fn creating_contact_invitation_reissues_fact_with_current_code() {
    // Reissuance semantic: when a sender calls
    // create_invitation for a target that already has a pending
    // contact record, a new ContactFact::Added is emitted so the
    // recorded invitation_code reflects the latest reissued code. The
    // signal-view reducer preserves user-set nickname on re-emits;
    // the journal-level assertion here is that reissuance does
    // produce a fact (count increases) and that the latest fact
    // carries an invitation_code.
    let sender_id = AuthorityId::new_from_entropy([130u8; 32]);
    let receiver_id = AuthorityId::new_from_entropy([131u8; 32]);
    let config = AgentConfig::default();
    let effects = Arc::new(
        AuraEffectSystem::simulation_for_test_for_authority(&config, sender_id).unwrap(),
    );
    let handler = handler_for_id(sender_id);

    let context_id = default_context_id_for_authority(sender_id);
    let existing_contact = ContactFact::Added {
        context_id,
        owner_id: sender_id,
        contact_id: receiver_id,
        nickname: "Alice-Maple".to_string(),
        added_at: PhysicalTime {
            ts_ms: 1,
            uncertainty: None,
        },
        invitation_code: None,
    };
    effects
        .commit_generic_fact_bytes(
            context_id,
            CONTACT_FACT_TYPE_ID.into(),
            existing_contact.to_bytes(),
        )
        .await
        .unwrap();
    effects.await_next_view_update().await;

    let collect_added_facts = |effects: Arc<AuraEffectSystem>| async move {
        effects
            .load_committed_facts(sender_id)
            .await
            .unwrap()
            .into_iter()
            .filter_map(|fact| match fact.content {
                FactContent::Relational(RelationalFact::Generic { envelope, .. })
                    if envelope.type_id.as_str() == CONTACT_FACT_TYPE_ID =>
                {
                    ContactFact::from_envelope(&envelope)
                }
                _ => None,
            })
            .filter(|fact| {
                matches!(
                    fact,
                    ContactFact::Added {
                        owner_id,
                        contact_id,
                        ..
                    } if *owner_id == sender_id && *contact_id == receiver_id
                )
            })
            .collect::<Vec<_>>()
    };

    let before = collect_added_facts(effects.clone()).await;
    let before_count = before.len();

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

    let after = collect_added_facts(effects.clone()).await;
    assert_eq!(
        after.len(),
        before_count + 1,
        "reissuance should emit a new ContactFact::Added with the updated invitation code"
    );

    // The reissue should produce at least one Added fact that carries
    // a derived invitation code. (The signal-view reducer treats
    // later Addeds as last-writer-wins for the code field.)
    let has_code = after.iter().any(|fact| {
        matches!(
            fact,
            ContactFact::Added {
                invitation_code: Some(_),
                ..
            }
        )
    });
    assert!(
        has_code,
        "reissuance emission should carry a derived invitation code \
         (none of {} Added facts had a code)",
        after.len()
    );
}

#[tokio::test]
async fn contact_acceptance_processing_skips_unrelated_envelopes() {
    let shared_transport = crate::runtime::SharedTransport::new();
    let config = AgentConfig::default();

    let sender_id = AuthorityId::new_from_entropy([126u8; 32]);
    let receiver_id = AuthorityId::new_from_entropy([127u8; 32]);

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
            shared_transport.clone(),
        )
        .unwrap(),
    );

    let sender_handler = handler_for_id(sender_id);
    let receiver_handler = handler_for_id(receiver_id);

    let invitation = sender_handler
        .create_invitation(
            sender_effects.clone(),
            receiver_id,
            InvitationType::Contact { nickname: None },
            Some("Contact invitation".to_string()),
            None,
        )
        .await
        .unwrap();

    // Queue a large unrelated backlog ahead of the acceptance notification.
    // This guards against starvation when inbox scanning encounters many
    // unknown content-types before actionable invitation/chat envelopes.
    for _ in 0..300 {
        let mut metadata = HashMap::new();
        metadata.insert(
            "content-type".to_string(),
            "application/aura-unrelated".to_string(),
        );
        receiver_effects
            .send_envelope(TransportEnvelope {
                destination: sender_id,
                source: receiver_id,
                context: default_context_id_for_authority(sender_id),
                payload: b"noop".to_vec(),
                metadata,
                receipt: None,
            })
            .await
            .unwrap();
    }

    let code = InvitationServiceApi::export_invitation(&invitation)
        .expect("shareable invitation should serialize");
    let imported = receiver_handler
        .import_invitation_code(&receiver_effects, &code)
        .await
        .unwrap();
    receiver_handler
        .accept_invitation(receiver_effects.clone(), &imported.invitation_id)
        .await
        .unwrap();
    receiver_handler
        .notify_contact_invitation_acceptance(
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
}

#[tokio::test]
async fn contact_acceptance_processing_commits_chat_fact_envelopes() {
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

    effects
        .send_envelope(TransportEnvelope {
            destination: authority,
            source: peer,
            context: context_id,
            payload,
            metadata,
            receipt: None,
        })
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
}

#[tokio::test]
async fn contact_acceptance_processing_commits_non_chat_relational_fact_envelopes() {
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

    effects
        .send_envelope(TransportEnvelope {
            destination: authority,
            source: peer,
            context: context_id,
            payload,
            metadata,
            receipt: None,
        })
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
}

#[tokio::test]
async fn channel_acceptance_processing_marks_created_invitation_accepted_for_sender() {
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
        .cache_peer_descriptor_for_peer(
            sender_effects.as_ref(),
            receiver_id,
            None,
            Some("tcp://127.0.0.1:55021"),
            now_ms,
        )
        .await;
    receiver_handler
        .cache_peer_descriptor_for_peer(
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
    let code = InvitationServiceApi::export_invitation(&invitation)
        .expect("shareable invitation should serialize");
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
    sender_effects
        .send_envelope(TransportEnvelope {
            destination: sender_id,
            source: receiver_id,
            context: default_context_id_for_authority(sender_id),
            payload,
            metadata,
            receipt: None,
        })
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
}

#[tokio::test]
async fn channel_acceptance_notification_transports_and_updates_sender_state() {
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
        .cache_peer_descriptor_for_peer(
            sender_effects.as_ref(),
            receiver_id,
            None,
            Some("tcp://127.0.0.1:55002"),
            now_ms,
        )
        .await;
    receiver_handler
        .cache_peer_descriptor_for_peer(
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
    let code = InvitationServiceApi::export_invitation(&invitation)
        .expect("shareable invitation should serialize");
    let imported = receiver_handler
        .import_invitation_code(&receiver_effects, &code)
        .await
        .unwrap();
    receiver_handler
        .accept_invitation(receiver_effects.clone(), &imported.invitation_id)
        .await
        .unwrap();
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
}

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

    let fresh_hint = TransportHint::websocket_direct("127.0.0.1:43011").unwrap();
    let peer_descriptor = manager
        .get_descriptor(default_context_id_for_authority(peer_id), peer_id)
        .await
        .expect("peer default-context descriptor should exist");
    assert_eq!(peer_descriptor.transport_hints.first(), Some(&fresh_hint));

    let local_descriptor = manager
        .get_descriptor(default_context_id_for_authority(authority_id), peer_id)
        .await
        .expect("local-context descriptor should exist");
    assert_eq!(local_descriptor.transport_hints.first(), Some(&fresh_hint));
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

#[tokio::test]
async fn contact_acceptance_processing_provisions_amp_state_for_channel_created_facts() {
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

    effects
        .send_envelope(TransportEnvelope {
            destination: authority,
            source: peer,
            context: context_id,
            payload,
            metadata,
            receipt: None,
        })
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
}

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

    effects
        .send_envelope(TransportEnvelope {
            destination: receiver_id,
            source: sender_id,
            context: default_context_id_for_authority(sender_id),
            payload: shareable
                .to_code()
                .expect("shareable invitation should serialize")
                .into_bytes(),
            metadata,
            receipt: None,
        })
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

#[tokio::test]
async fn accepting_channel_invitation_materializes_home_and_channel_state() {
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
}

#[tokio::test]
async fn accepting_channel_invitation_corrects_preexisting_raw_channel_name() {
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
    let _rendezvous_tasks = attach_test_rendezvous_manager(effects.as_ref(), receiver_id).await;
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
}

#[tokio::test]
async fn accepting_channel_invitation_materializes_amp_bootstrap_state() {
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
}

#[tokio::test]
async fn accepting_channel_invitation_uses_shareable_context_when_present() {
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
}

#[tokio::test]
async fn imported_invitation_is_resolvable_across_handler_instances() {
    let own_authority = AuthorityId::new_from_entropy([122u8; 32]);
    let config = AgentConfig::default();
    let effects = Arc::new(
        AuraEffectSystem::simulation_for_test_for_authority(&config, own_authority).unwrap(),
    );

    let authority_context = AuthorityContext::new(own_authority);

    let handler_import = handler_for(authority_context.clone());
    let handler_accept = handler_for(authority_context);

    let sender_id = AuthorityId::new_from_entropy([123u8; 32]);
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-demo-contact-2"),
        sender_id,
        context_id: None,
        invitation_type: InvitationType::Contact {
            nickname: Some("Alice".to_string()),
        },
        expires_at: None,
        message: Some("Contact invitation from Alice (demo)".to_string()),
    };
    let code = shareable
        .to_code()
        .expect("shareable invitation should serialize");

    let imported = handler_import
        .import_invitation_code(&effects, &code)
        .await
        .unwrap();

    // Accept using a separate handler instance to ensure we don't rely on in-memory caches.
    handler_accept
        .accept_invitation(effects.clone(), &imported.invitation_id)
        .await
        .unwrap();

    let committed = effects.load_committed_facts(own_authority).await.unwrap();

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
}

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

#[tokio::test]
async fn accepted_imported_invitation_persists_status_across_handler_instances() {
    let own_authority = AuthorityId::new_from_entropy([126u8; 32]);
    let sender_id = AuthorityId::new_from_entropy([127u8; 32]);
    let config = AgentConfig::default();
    let effects = Arc::new(
        AuraEffectSystem::simulation_for_test_for_authority(&config, own_authority).unwrap(),
    );
    let authority_context = AuthorityContext::new(own_authority);
    let handler = InvitationHandler::new(authority_context.clone()).unwrap();
    let shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("imported-contact-persists-accepted"),
        sender_id,
        context_id: None,
        invitation_type: InvitationType::Contact {
            nickname: Some("Alice".to_string()),
        },
        expires_at: None,
        message: Some("hello".to_string()),
    };

    let imported = handler
        .import_invitation_code(
            effects.as_ref(),
            &shareable
                .to_code()
                .expect("shareable invitation should serialize"),
        )
        .await
        .expect("contact invitation import should succeed");
    handler
        .accept_invitation(effects.clone(), &imported.invitation_id)
        .await
        .expect("contact invitation accept should persist imported status");

    let retrieved = InvitationHandler::new(authority_context)
        .unwrap()
        .get_invitation_with_storage(effects.as_ref(), &imported.invitation_id)
        .await
        .expect("accepted imported invitation should remain available");
    assert_eq!(retrieved.status, InvitationStatus::Accepted);
    assert_eq!(retrieved.sender_id, sender_id);
    assert_eq!(retrieved.receiver_id, own_authority);
}

#[tokio::test]
async fn invitation_can_be_cancelled() {
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
}

#[tokio::test]
async fn list_pending_shows_only_pending() {
    let authority_context = create_test_authority(100);
    let effects = effects_for(&authority_context);
    let handler = InvitationHandler::new(authority_context).unwrap();

    // Create 3 invitations
    let own_id = handler.authority_context().authority_id();
    let inv1 = handler
        .create_invitation(
            effects.clone(),
            own_id,
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
}

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

/// Test that importing and accepting multiple contact invitations works sequentially.
///
/// This test mimics the TUI demo mode flow where:
/// 1. Alice's invitation is imported and accepted
/// 2. Carol's invitation is imported and accepted
///
/// Both should succeed without interfering with each other.
#[tokio::test]
async fn importing_multiple_contact_invitations_sequentially() {
    let own_authority = AuthorityId::new_from_entropy([150u8; 32]);
    let config = AgentConfig::default();
    let effects = Arc::new(
        AuraEffectSystem::simulation_for_test_for_authority(&config, own_authority).unwrap(),
    );

    let authority_context = AuthorityContext::new(own_authority);
    let handler = InvitationHandler::new(authority_context).unwrap();

    // Create Alice's invitation (matching DemoHints pattern)
    let alice_sender_id = AuthorityId::new_from_entropy([151u8; 32]);
    let alice_shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-demo-alice-sequential"),
        sender_id: alice_sender_id,
        context_id: None,
        invitation_type: InvitationType::Contact {
            nickname: Some("Alice".to_string()),
        },
        expires_at: None,
        message: Some("Contact invitation from Alice (demo)".to_string()),
    };
    let alice_code = alice_shareable
        .to_code()
        .expect("shareable invitation should serialize");

    // Create Carol's invitation (matching DemoHints pattern - different seed)
    let carol_sender_id = AuthorityId::new_from_entropy([152u8; 32]);
    let carol_shareable = ShareableInvitation {
        version: ShareableInvitation::CURRENT_VERSION,
        invitation_id: InvitationId::new("inv-demo-carol-sequential"),
        sender_id: carol_sender_id,
        context_id: None,
        invitation_type: InvitationType::Contact {
            nickname: Some("Carol".to_string()),
        },
        expires_at: None,
        message: Some("Contact invitation from Carol (demo)".to_string()),
    };
    let carol_code = carol_shareable
        .to_code()
        .expect("shareable invitation should serialize");

    // Import and accept Alice's invitation
    let alice_imported = handler
        .import_invitation_code(&effects, &alice_code)
        .await
        .expect("Alice import should succeed");
    assert_eq!(alice_imported.sender_id, alice_sender_id);
    assert_eq!(
        alice_imported.invitation_id.as_str(),
        "inv-demo-alice-sequential"
    );

    handler
        .accept_invitation(effects.clone(), &alice_imported.invitation_id)
        .await
        .expect("Alice accept should succeed");

    // Import and accept Carol's invitation (this is the step that was failing in TUI)
    let carol_imported = handler
        .import_invitation_code(&effects, &carol_code)
        .await
        .expect("Carol import should succeed");
    assert_eq!(carol_imported.sender_id, carol_sender_id);
    assert_eq!(
        carol_imported.invitation_id.as_str(),
        "inv-demo-carol-sequential"
    );

    // This is the critical assertion - Carol's accept should work after Alice's
    handler
        .accept_invitation(effects.clone(), &carol_imported.invitation_id)
        .await
        .expect("Carol accept should succeed after Alice");

    // Verify both contacts were added
    let committed = effects.load_committed_facts(own_authority).await.unwrap();

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
}

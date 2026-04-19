use super::*;
use crate::core::AgentConfig;
use crate::AgentBuilder;
use async_lock::Mutex;
use aura_core::context::EffectContext;
use aura_core::effects::ExecutionMode;
use aura_core::effects::TransportEffects;
use aura_core::hash::hash;
use aura_journal::commitment_tree::storage::TREE_OPS_INDEX_KEY;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

fn env_lock() -> &'static Mutex<()> {
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    ENV_LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvRestore {
    saved: Vec<(&'static str, Option<OsString>)>,
}

impl EnvRestore {
    fn capture(keys: &[&'static str]) -> Self {
        Self {
            saved: keys
                .iter()
                .map(|key| (*key, std::env::var_os(key)))
                .collect(),
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        for (key, value) in &self.saved {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

fn unique_test_path(label: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "aura-agent-runtime-bridge-{label}-{}",
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ))
}

// Note: Full tests would require mock infrastructure which is in aura-testkit
// These are placeholder tests showing the API usage

#[test]
fn test_sync_status_default() {
    let status = SyncStatus::default();
    assert!(!status.is_running);
    assert_eq!(status.connected_peers, 0);
}

#[test]
fn test_rendezvous_status_default() {
    let status = RendezvousStatus::default();
    assert!(!status.is_running);
    assert_eq!(status.cached_peers, 0);
}

#[test]
fn harness_sync_policy_defaults_when_env_missing() {
    let _guard = env_lock().lock_blocking();
    std::env::remove_var("AURA_HARNESS_MODE");
    std::env::remove_var("AURA_HARNESS_SYNC_ROUNDS");
    std::env::remove_var("AURA_HARNESS_SYNC_BACKOFF_MS");

    assert!(!harness_mode_enabled());
    assert_eq!(harness_sync_rounds(), DEFAULT_HARNESS_SYNC_ROUNDS);
    assert_eq!(harness_sync_backoff_ms(), DEFAULT_HARNESS_SYNC_BACKOFF_MS);
}

#[test]
fn harness_sync_policy_honors_explicit_env_values() {
    let _guard = env_lock().lock_blocking();
    std::env::set_var("AURA_HARNESS_MODE", "1");
    std::env::set_var("AURA_HARNESS_SYNC_ROUNDS", "5");
    std::env::set_var("AURA_HARNESS_SYNC_BACKOFF_MS", "125");

    assert!(harness_mode_enabled());
    assert_eq!(harness_sync_rounds(), 5);
    assert_eq!(harness_sync_backoff_ms(), 125);

    std::env::remove_var("AURA_HARNESS_MODE");
    std::env::remove_var("AURA_HARNESS_SYNC_ROUNDS");
    std::env::remove_var("AURA_HARNESS_SYNC_BACKOFF_MS");
}

#[tokio::test]
async fn ensure_peer_channel_requires_sync_peers_after_established_channel() {
    let authority = AuthorityId::new_from_entropy([74u8; 32]);
    let peer = AuthorityId::new_from_entropy([75u8; 32]);
    let context = ContextId::new_from_entropy([76u8; 32]);
    let build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([77u8; 32]),
        ExecutionMode::Testing,
    );
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .with_rendezvous()
            .with_sync()
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    let manager = agent
        .runtime()
        .rendezvous()
        .expect("runtime rendezvous service");
    manager
        .cache_descriptor(aura_rendezvous::facts::RendezvousDescriptor {
            authority_id: peer,
            device_id: None,
            context_id: context,
            transport_hints: vec![aura_rendezvous::facts::TransportHint::tcp_direct(
                "127.0.0.1:6555",
            )
            .expect("tcp hint")],
            handshake_psk_commitment: [0u8; 32],
            public_key: [0u8; 32],
            valid_from: 0,
            valid_until: u64::MAX,
            nonce: [0u8; 32],
            nickname_suggestion: None,
        })
        .await
        .expect("cache current-context descriptor");

    let bridge = AgentRuntimeBridge::new(agent);
    let error = bridge
        .ensure_peer_channel(context, peer)
        .await
        .expect_err("established peer channel should still fail when sync cannot run");
    assert!(
        error
            .to_string()
            .contains("No sync peers are available for synchronization"),
        "expected no-peers sync validation error, got: {error}"
    );
}

#[tokio::test]
async fn ensure_peer_channel_surfaces_service_unavailability_before_descriptor_fallback() {
    let _guard = env_lock().lock().await;
    let _env_restore = EnvRestore::capture(&[
        "AURA_HARNESS_MODE",
        "AURA_HARNESS_SYNC_ROUNDS",
        "AURA_HARNESS_SYNC_BACKOFF_MS",
    ]);
    std::env::set_var("AURA_HARNESS_MODE", "1");
    std::env::set_var("AURA_HARNESS_SYNC_ROUNDS", "2");
    std::env::set_var("AURA_HARNESS_SYNC_BACKOFF_MS", "50");

    let authority = AuthorityId::new_from_entropy([78u8; 32]);
    let peer = AuthorityId::new_from_entropy([79u8; 32]);
    let context = ContextId::new_from_entropy([80u8; 32]);
    let fallback_context = default_context_id_for_authority(peer);
    let build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([81u8; 32]),
        ExecutionMode::Testing,
    );
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .with_rendezvous()
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    let manager = agent
        .runtime()
        .rendezvous()
        .expect("runtime rendezvous service")
        .clone();

    let make_descriptor =
        move |descriptor_context| aura_rendezvous::facts::RendezvousDescriptor {
            authority_id: peer,
            device_id: None,
            context_id: descriptor_context,
            transport_hints: vec![aura_rendezvous::facts::TransportHint::tcp_direct(
                "127.0.0.1:6556",
            )
            .expect("tcp hint")],
            handshake_psk_commitment: [0u8; 32],
            public_key: [0u8; 32],
            valid_from: 0,
            valid_until: u64::MAX,
            nonce: [0u8; 32],
            nickname_suggestion: None,
        };

    manager
        .cache_descriptor(make_descriptor(fallback_context))
        .await
        .expect("cache fallback descriptor for initiation");

    let bridge = AgentRuntimeBridge::new(agent);
    let error = bridge.ensure_peer_channel(context, peer).await.expect_err(
        "peer channel initiation should fail explicitly when prerequisites are unavailable",
    );
    assert!(
        error.to_string().contains("service unavailable"),
        "expected service-unavailable boundary, got: {error}"
    );
}

#[tokio::test]
async fn resolve_amp_channel_context_finds_registered_amp_checkpoint_context() {
    let authority = AuthorityId::new_from_entropy([7u8; 32]);
    let build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([9u8; 32]),
        ExecutionMode::Testing,
    );
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    let bridge = AgentRuntimeBridge::new(agent);
    let context = bridge
        .agent
        .runtime()
        .contexts()
        .create_context(authority, 42)
        .await
        .expect("register context");
    let channel = ChannelId::from_bytes(hash(b"resolve-amp-channel-context"));

    bridge
        .amp_create_channel(ChannelCreateParams {
            context,
            channel: Some(channel),
            skip_window: None,
            topic: None,
        })
        .await
        .expect("create channel");
    bridge
        .amp_join_channel(ChannelJoinParams {
            context,
            channel,
            participant: authority,
        })
        .await
        .expect("join channel");

    let resolved = bridge
        .resolve_amp_channel_context(channel)
        .await
        .expect("resolve channel context");

    assert_eq!(resolved, Some(context));
}

#[tokio::test]
async fn amp_list_channel_participants_includes_accepted_channel_invitees() {
    let authority = AuthorityId::new_from_entropy([10u8; 32]);
    let receiver = AuthorityId::new_from_entropy([11u8; 32]);
    let build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([12u8; 32]),
        ExecutionMode::Testing,
    );
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    let bridge = AgentRuntimeBridge::new(agent.clone());
    let context = ContextId::new_from_entropy([13u8; 32]);
    let channel = ChannelId::from_bytes(hash(b"accepted-channel-invitee-visible"));

    bridge
        .amp_create_channel(ChannelCreateParams {
            context,
            channel: Some(channel),
            skip_window: None,
            topic: None,
        })
        .await
        .expect("create channel");
    bridge
        .amp_join_channel(ChannelJoinParams {
            context,
            channel,
            participant: authority,
        })
        .await
        .expect("join channel");

    let invitations = agent.invitations().expect("invitation service");
    let invitation = invitations
        .invite_to_channel(
            receiver,
            channel.to_string(),
            Some(context),
            Some("shared-parity-lab".to_string()),
            None,
            None,
            None,
        )
        .await
        .expect("create channel invitation");
    invitations
        .accept(&invitation.invitation_id)
        .await
        .expect("mark invitation accepted");

    let participants = bridge
        .amp_list_channel_participants(context, channel)
        .await
        .expect("list authoritative participants");

    assert!(participants.contains(&authority));
    assert!(
        participants.contains(&receiver),
        "accepted invitee should appear in authoritative participant set"
    );
}

#[tokio::test]
async fn amp_list_channel_participants_includes_transported_channel_acceptance() {
    let authority = AuthorityId::new_from_entropy([42u8; 32]);
    let receiver = AuthorityId::new_from_entropy([43u8; 32]);
    let sender_build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([44u8; 32]),
        ExecutionMode::Testing,
    );
    let receiver_build_context = EffectContext::new(
        receiver,
        ContextId::new_from_entropy([45u8; 32]),
        ExecutionMode::Testing,
    );
    let shared_transport = crate::SharedTransport::new();
    let sender_agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .build_simulation_async_with_shared_transport(
                1001,
                &sender_build_context,
                shared_transport.clone(),
            )
            .await
            .expect("build sender simulation agent"),
    );
    let receiver_agent = Arc::new(
        AgentBuilder::new()
            .with_authority(receiver)
            .build_simulation_async_with_shared_transport(
                1002,
                &receiver_build_context,
                shared_transport,
            )
            .await
            .expect("build receiver simulation agent"),
    );
    let sender_effects = sender_agent.runtime().effects();
    crate::handlers::invitation::InvitationHandler::new(crate::core::AuthorityContext::new(
        authority,
    ))
    .expect("sender invitation handler")
    .cache_peer_descriptor_for_peer(
        sender_effects.as_ref(),
        receiver,
        None,
        Some("tcp://127.0.0.1:55012"),
        1_700_000_000_000,
    )
    .await;
    let sender_manager = crate::runtime::services::RendezvousManager::new_with_default_udp(
        authority,
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
        .expect("start sender rendezvous manager");

    let receiver_effects = receiver_agent.runtime().effects();
    crate::handlers::invitation::InvitationHandler::new(crate::core::AuthorityContext::new(
        receiver,
    ))
    .expect("receiver invitation handler")
    .cache_peer_descriptor_for_peer(
        receiver_effects.as_ref(),
        authority,
        None,
        Some("tcp://127.0.0.1:55011"),
        1_700_000_000_000,
    )
    .await;
    let receiver_manager = crate::runtime::services::RendezvousManager::new_with_default_udp(
        receiver,
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
    .expect("start receiver rendezvous manager");

    let sender_bridge = AgentRuntimeBridge::new(sender_agent.clone());
    let receiver_bridge = AgentRuntimeBridge::new(receiver_agent.clone());
    let context = ContextId::new_from_entropy([46u8; 32]);
    let channel = ChannelId::from_bytes(hash(b"transported-channel-acceptance-visible"));

    sender_bridge
        .amp_create_channel(ChannelCreateParams {
            context,
            channel: Some(channel),
            skip_window: None,
            topic: None,
        })
        .await
        .expect("create channel");
    sender_bridge
        .amp_join_channel(ChannelJoinParams {
            context,
            channel,
            participant: authority,
        })
        .await
        .expect("join channel");

    let sender_invitations = sender_agent
        .invitations()
        .expect("sender invitation service");
    let receiver_invitations = receiver_agent
        .invitations()
        .expect("receiver invitation service");
    let invitation = sender_invitations
        .invite_to_channel(
            receiver,
            channel.to_string(),
            Some(context),
            Some("shared-parity-lab".to_string()),
            None,
            None,
            None,
        )
        .await
        .expect("create channel invitation");
    let imported = receiver_invitations
        .import_and_cache(
            &crate::handlers::invitation_service::InvitationServiceApi::export_invitation(
                &invitation,
            )
            .expect("shareable invitation should serialize"),
        )
        .await
        .expect("import channel invitation");
    receiver_invitations
        .accept(&imported.invitation_id)
        .await
        .expect("accept channel invitation");
    let receiver_participants = receiver_bridge
        .amp_list_channel_participants(context, channel)
        .await
        .expect("receiver should list authoritative participants after accepting invite");
    assert!(receiver_participants.contains(&receiver));
    assert!(
        receiver_participants.contains(&authority),
        "receiver authoritative participant set should include inviter after accepting channel invitation; participants={receiver_participants:?}"
    );
    crate::handlers::invitation::InvitationHandler::new(crate::core::AuthorityContext::new(
        receiver,
    ))
    .expect("receiver invitation handler")
    .notify_channel_invitation_acceptance(receiver_effects.as_ref(), &imported.invitation_id)
    .await
    .expect("resend channel invitation acceptance");
    let acceptance_envelope = sender_effects
        .receive_envelope()
        .await
        .expect("sender should receive transported channel acceptance envelope");
    assert_eq!(
        acceptance_envelope
            .metadata
            .get("content-type")
            .map(String::as_str),
        Some("application/aura-channel-invitation-acceptance"),
    );
    let acceptance: serde_json::Value = serde_json::from_slice(&acceptance_envelope.payload)
        .expect("parse channel acceptance payload");
    assert_eq!(
        acceptance
            .get("invitation_id")
            .and_then(serde_json::Value::as_str),
        Some(invitation.invitation_id.as_str()),
    );
    sender_effects.requeue_envelope(acceptance_envelope);
    let first_outcome = sender_bridge
        .process_ceremony_messages()
        .await
        .expect("process transported channel acceptance");
    match first_outcome {
        CeremonyProcessingOutcome::Processed {
            counts,
            reachability_refresh,
        } => {
            assert!(
                counts.contact_messages >= 1,
                "expected channel acceptance transport to count as processed contact/channel traffic: {counts:?}"
            );
            assert!(
                matches!(
                    reachability_refresh,
                    ReachabilityRefreshOutcome::Degraded { .. }
                ),
                "missing sync service should surface an explicit degraded refresh outcome"
            );
        }
        CeremonyProcessingOutcome::NoProgress => {
            panic!(
                "transported channel acceptance should not collapse to a no-progress outcome"
            );
        }
    }

    for _ in 0..7 {
        let _ = sender_bridge
            .process_ceremony_messages()
            .await
            .expect("continue processing transported channel acceptance");
        let participants = sender_bridge
            .amp_list_channel_participants(context, channel)
            .await
            .expect("list authoritative participants");
        if participants.contains(&receiver) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    let participants = sender_bridge
        .amp_list_channel_participants(context, channel)
        .await
        .expect("list authoritative participants");
    let invitations = sender_agent
        .invitations()
        .expect("sender invitation service")
        .list_with_storage()
        .await;
    assert!(participants.contains(&authority));
    assert!(
        participants.contains(&receiver),
        "transported accepted invitee should appear in authoritative participant set; participants={participants:?} invitations={invitations:?}"
    );
}

#[tokio::test]
async fn identify_materialized_channel_ids_by_name_requires_materialized_runtime_context() {
    let authority = AuthorityId::new_from_entropy([14u8; 32]);
    let sender = AuthorityId::new_from_entropy([15u8; 32]);
    let build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([16u8; 32]),
        ExecutionMode::Testing,
    );
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    let bridge = AgentRuntimeBridge::new(agent.clone());
    let context = ContextId::new_from_entropy([17u8; 32]);
    let channel = ChannelId::from_bytes(hash(b"resolve-channel-name-from-imported-invite"));
    let invitations = agent.invitations().expect("invitation service");
    let shareable = crate::handlers::invitation::ShareableInvitation {
        version: crate::handlers::invitation::ShareableInvitation::CURRENT_VERSION,
        invitation_id: aura_core::InvitationId::new("inv-imported-channel-runtime-bridge"),
        sender_id: sender,
        context_id: Some(context),
        invitation_type: aura_invitation::InvitationType::Channel {
            home_id: channel,
            nickname_suggestion: Some("shared-parity-lab".to_string()),
            bootstrap: None,
        },
        expires_at: None,
        message: Some("Join shared-parity-lab".to_string()),
    };
    let code = shareable
        .to_code()
        .expect("shareable invitation should serialize");

    let imported = invitations
        .import_and_cache(&code)
        .await
        .expect("import channel invitation");
    assert_eq!(imported.invitation_id, shareable.invitation_id);

    let resolved = bridge
        .identify_materialized_channel_ids_by_name("shared-parity-lab")
        .await
        .expect("identify imported channel name");

    assert!(
        resolved.is_empty(),
        "imported channel invitation must not become an authoritative channel resolution result"
    );
}

#[tokio::test]
async fn try_get_sync_peers_requires_sync_service() {
    let authority = AuthorityId::new_from_entropy([18u8; 32]);
    let build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([19u8; 32]),
        ExecutionMode::Testing,
    );
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    let bridge = AgentRuntimeBridge::new(agent);

    let error = bridge
        .try_get_sync_peers()
        .await
        .expect_err("missing sync service should be explicit");
    assert!(
        error.to_string().contains("sync_service"),
        "expected sync service error, got: {error}"
    );
}

#[tokio::test]
async fn trigger_sync_without_peers_is_a_noop() {
    let authority = AuthorityId::new_from_entropy([26u8; 32]);
    let build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([27u8; 32]),
        ExecutionMode::Testing,
    );
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .with_sync()
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    let bridge = AgentRuntimeBridge::new(agent);

    bridge
        .trigger_sync()
        .await
        .expect("sync with no peers should remain a no-op");
}

#[tokio::test]
async fn try_get_sync_status_requires_sync_service() {
    let authority = AuthorityId::new_from_entropy([28u8; 32]);
    let build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([29u8; 32]),
        ExecutionMode::Testing,
    );
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    let bridge = AgentRuntimeBridge::new(agent);

    let error = bridge
        .try_get_sync_status()
        .await
        .expect_err("missing sync service should be explicit");
    assert!(
        error.to_string().contains("sync_service"),
        "expected sync service error, got: {error}"
    );
}

#[tokio::test]
async fn try_get_discovered_peers_requires_rendezvous_service() {
    let authority = AuthorityId::new_from_entropy([20u8; 32]);
    let build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([21u8; 32]),
        ExecutionMode::Testing,
    );
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    let bridge = AgentRuntimeBridge::new(agent);

    let error = bridge
        .try_get_discovered_peers()
        .await
        .expect_err("missing rendezvous service should be explicit");
    assert!(
        error.to_string().contains("rendezvous_service"),
        "expected rendezvous service error, got: {error}"
    );
}

#[tokio::test]
async fn try_get_bootstrap_candidates_requires_rendezvous_service() {
    let authority = AuthorityId::new_from_entropy([22u8; 32]);
    let build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([23u8; 32]),
        ExecutionMode::Testing,
    );
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    let bridge = AgentRuntimeBridge::new(agent);

    let error = bridge
        .try_get_bootstrap_candidates()
        .await
        .expect_err("missing rendezvous service should be explicit");
    assert!(
        error.to_string().contains("rendezvous_service"),
        "expected rendezvous service error, got: {error}"
    );
}

#[tokio::test]
async fn try_get_rendezvous_status_requires_rendezvous_service() {
    let authority = AuthorityId::new_from_entropy([24u8; 32]);
    let build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([25u8; 32]),
        ExecutionMode::Testing,
    );
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    let bridge = AgentRuntimeBridge::new(agent);

    let error = bridge
        .try_get_rendezvous_status()
        .await
        .expect_err("missing rendezvous service should be explicit");
    assert!(
        error.to_string().contains("rendezvous_service"),
        "expected rendezvous service error, got: {error}"
    );
}

#[tokio::test]
async fn trigger_discovery_returns_typed_noop_when_lan_discovery_is_disabled() {
    let authority = AuthorityId::new_from_entropy([80u8; 32]);
    let build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([81u8; 32]),
        ExecutionMode::Testing,
    );
    let config = AgentConfig::default().with_lan_discovery_enabled(false);
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .with_config(config.clone())
            .with_rendezvous_config(config.rendezvous_config())
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    let bridge = AgentRuntimeBridge::new(agent);

    let outcome = bridge
        .trigger_discovery()
        .await
        .expect("discovery trigger should return a typed outcome");
    assert_eq!(outcome, DiscoveryTriggerOutcome::AlreadyRunning);
}

#[tokio::test]
async fn process_ceremony_messages_returns_no_progress_when_nothing_is_pending() {
    let authority = AuthorityId::new_from_entropy([82u8; 32]);
    let build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([83u8; 32]),
        ExecutionMode::Testing,
    );
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    let bridge = AgentRuntimeBridge::new(agent);

    let outcome = bridge
        .process_ceremony_messages()
        .await
        .expect("empty inbox should be a typed no-progress outcome");
    assert_eq!(outcome, CeremonyProcessingOutcome::NoProgress);
}

#[tokio::test]
async fn try_list_devices_requires_readable_tree_state() {
    let authority = AuthorityId::new_from_entropy([30u8; 32]);
    let build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([31u8; 32]),
        ExecutionMode::Testing,
    );
    let storage_root = unique_test_path("device-list-read-error");
    fs::create_dir_all(&storage_root).expect("create storage root");
    fs::create_dir_all(storage_root.join(format!("{TREE_OPS_INDEX_KEY}.dat")))
        .expect("create unreadable tree index directory");

    let mut config = AgentConfig::default();
    config.storage.base_path = storage_root.clone();

    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .with_config(config)
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    let bridge = AgentRuntimeBridge::new(agent);

    let error = bridge
        .try_list_devices()
        .await
        .expect_err("missing tree readability should be explicit");
    let message = error.to_string();
    assert!(
        message.contains("Failed to read current device list")
            || message.contains("Read current device list failed"),
        "device-list failure should stay explicit: {message}"
    );

    let _ = fs::remove_dir_all(storage_root);
}

#[tokio::test]
async fn try_list_authorities_requires_readable_storage_listing() {
    let authority = AuthorityId::new_from_entropy([32u8; 32]);
    let build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([33u8; 32]),
        ExecutionMode::Testing,
    );
    let storage_root = unique_test_path("authority-list-read-error");
    fs::create_dir_all(&storage_root).expect("create storage root");

    let mut config = AgentConfig::default();
    config.storage.base_path = storage_root.clone();

    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .with_config(config)
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    let bridge = AgentRuntimeBridge::new(agent);

    fs::remove_dir_all(&storage_root).expect("remove storage root directory");
    fs::write(&storage_root, b"not-a-directory").expect("create invalid storage root file");

    let error = bridge
        .try_list_authorities()
        .await
        .expect_err("missing authority storage listing should be explicit");
    let message = error.to_string();
    assert!(
        message.contains("Failed to list stored authorities")
            || message.contains("List stored authorities failed"),
        "authority-list failure should stay explicit: {message}"
    );

    let _ = fs::remove_file(storage_root);
}

#[tokio::test]
async fn try_list_authorities_requires_readable_account_config() {
    let authority = AuthorityId::new_from_entropy([45u8; 32]);
    let build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([46u8; 32]),
        ExecutionMode::Testing,
    );
    let storage_root = unique_test_path("authority-list-account-config-read-error");
    fs::create_dir_all(&storage_root).expect("create storage root");

    let mut config = AgentConfig::default();
    config.storage.base_path = storage_root.clone();

    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .with_config(config)
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    let bridge = AgentRuntimeBridge::new(agent);

    fs::create_dir_all(storage_root.join("account.json.dat"))
        .expect("create unreadable account config directory");

    let error = bridge
        .try_list_authorities()
        .await
        .expect_err("account config read failure should be explicit");
    let message = error.to_string();
    assert!(
        message.contains("Failed to read account.json")
            || message.contains("Read account config failed"),
        "authority-list failure should surface the account config read error: {message}"
    );

    let _ = fs::remove_dir_all(storage_root);
}

#[tokio::test]
async fn try_list_authorities_rejects_corrupt_authority_records() {
    let authority = AuthorityId::new_from_entropy([47u8; 32]);
    let build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([48u8; 32]),
        ExecutionMode::Testing,
    );
    let storage_root = unique_test_path("authority-list-corrupt-record");
    fs::create_dir_all(&storage_root).expect("create storage root");

    let mut config = AgentConfig::default();
    config.storage.base_path = storage_root.clone();

    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .with_config(config)
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    let bridge = AgentRuntimeBridge::new(agent);
    let other_authority = AuthorityId::new_from_entropy([49u8; 32]);
    let record_key = aura_app::ui::prelude::authority_storage_key(&other_authority);
    fs::write(storage_root.join(format!("{record_key}.dat")), b"not-json")
        .expect("write corrupt authority record");

    let error = bridge
        .try_list_authorities()
        .await
        .expect_err("corrupt authority record should be explicit");
    let message = error.to_string();
    assert!(
        message.contains("Failed to read authority record")
            || message.contains("Read authority record failed")
            || message.contains("Failed to decode authority record"),
        "authority-list failure should reject corrupt records explicitly: {message}"
    );

    let _ = fs::remove_dir_all(storage_root);
}

#[tokio::test]
async fn try_get_settings_requires_readable_account_config() {
    let authority = AuthorityId::new_from_entropy([34u8; 32]);
    let build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([35u8; 32]),
        ExecutionMode::Testing,
    );
    let storage_root = unique_test_path("settings-account-config-read-error");
    fs::create_dir_all(&storage_root).expect("create storage root");

    let mut config = AgentConfig::default();
    config.storage.base_path = storage_root.clone();

    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .with_config(config)
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    let bridge = AgentRuntimeBridge::new(agent);

    fs::create_dir_all(storage_root.join("account.json.dat"))
        .expect("create unreadable account config directory");

    let error = bridge
        .try_get_settings()
        .await
        .expect_err("account config read failure should be explicit");
    let message = error.to_string();
    assert!(
        message.contains("Failed to read account.json")
            || message.contains("Read account config failed"),
        "settings failure should surface the account config read error: {message}"
    );

    let _ = fs::remove_dir_all(storage_root);
}

#[tokio::test]
async fn try_list_pending_invitations_requires_accepting_invitation_service() {
    let authority = AuthorityId::new_from_entropy([36u8; 32]);
    let build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([37u8; 32]),
        ExecutionMode::Testing,
    );
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    agent.runtime().activity_gate().begin_shutdown();
    let bridge = AgentRuntimeBridge::new(agent);

    let error = bridge
        .try_list_pending_invitations()
        .await
        .expect_err("stopping runtime should reject invitation queries");
    assert!(
        error.to_string().contains("invitation_service"),
        "expected invitation service error, got: {error}"
    );
}

#[tokio::test]
async fn try_get_invited_peer_ids_requires_accepting_invitation_service() {
    let authority = AuthorityId::new_from_entropy([38u8; 32]);
    let build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([39u8; 32]),
        ExecutionMode::Testing,
    );
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    agent.runtime().activity_gate().begin_shutdown();
    let bridge = AgentRuntimeBridge::new(agent);

    let error = bridge
        .try_get_invited_peer_ids()
        .await
        .expect_err("stopping runtime should reject invited-peer queries");
    assert!(
        error.to_string().contains("invitation_service"),
        "expected invitation service error, got: {error}"
    );
}

#[tokio::test]
async fn try_get_invited_peer_ids_skips_generic_contact_invites() {
    let authority = AuthorityId::new_from_entropy([52u8; 32]);
    let receiver = AuthorityId::new_from_entropy([53u8; 32]);
    let build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([54u8; 32]),
        ExecutionMode::Testing,
    );
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    let bridge = AgentRuntimeBridge::new(agent);

    bridge
        .create_contact_invitation(authority, None, Some("generic".to_string()), None, None)
        .await
        .expect("generic contact invitation should succeed");
    bridge
        .create_contact_invitation(receiver, None, Some("direct".to_string()), None, None)
        .await
        .expect("direct contact invitation should succeed");

    let invited = bridge
        .try_get_invited_peer_ids()
        .await
        .expect("read invited peer ids");

    assert_eq!(invited, vec![receiver]);
}

#[tokio::test]
async fn amp_list_channel_participants_requires_accepting_invitation_service() {
    let authority = AuthorityId::new_from_entropy([40u8; 32]);
    let build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([41u8; 32]),
        ExecutionMode::Testing,
    );
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    let bridge = AgentRuntimeBridge::new(agent.clone());
    let context = ContextId::new_from_entropy([42u8; 32]);
    let channel = ChannelId::from_bytes(hash(b"participants-require-invitation-service"));

    bridge
        .amp_create_channel(ChannelCreateParams {
            context,
            channel: Some(channel),
            skip_window: None,
            topic: None,
        })
        .await
        .expect("create channel");
    bridge
        .amp_join_channel(ChannelJoinParams {
            context,
            channel,
            participant: authority,
        })
        .await
        .expect("join channel");

    agent.runtime().activity_gate().begin_shutdown();

    let error = bridge
        .amp_list_channel_participants(context, channel)
        .await
        .expect_err("stopping runtime should reject participant queries");
    assert!(
        error.to_string().contains("invitation_service"),
        "expected invitation service error, got: {error}"
    );
}

#[tokio::test]
async fn try_get_settings_requires_accepting_invitation_service() {
    let authority = AuthorityId::new_from_entropy([43u8; 32]);
    let build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([44u8; 32]),
        ExecutionMode::Testing,
    );
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    agent.runtime().activity_gate().begin_shutdown();
    let bridge = AgentRuntimeBridge::new(agent);

    let error = bridge
        .try_get_settings()
        .await
        .expect_err("stopping runtime should reject settings queries");
    assert!(
        error.to_string().contains("invitation_service"),
        "expected invitation service error, got: {error}"
    );
}

#[tokio::test]
async fn is_peer_online_requires_current_context_descriptor() {
    let authority = AuthorityId::new_from_entropy([50u8; 32]);
    let peer = AuthorityId::new_from_entropy([51u8; 32]);
    let build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([52u8; 32]),
        ExecutionMode::Testing,
    );
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    let effects = agent.runtime().effects();
    let manager = crate::runtime::services::RendezvousManager::new_with_default_udp(
        authority,
        crate::runtime::services::RendezvousManagerConfig::default(),
        Arc::new(effects.time_effects().clone()),
    );
    effects.attach_rendezvous_manager(manager.clone());
    let service_context = crate::runtime::services::RuntimeServiceContext::new(
        Arc::new(crate::runtime::TaskSupervisor::new()),
        Arc::new(effects.time_effects().clone()),
    );
    crate::runtime::services::RuntimeService::start(&manager, &service_context)
        .await
        .expect("start rendezvous manager");

    manager
        .cache_descriptor(aura_rendezvous::facts::RendezvousDescriptor {
            authority_id: peer,
            device_id: None,
            context_id: default_context_id_for_authority(peer),
            transport_hints: vec![aura_rendezvous::facts::TransportHint::tcp_direct(
                "127.0.0.1:6553",
            )
            .expect("tcp hint")],
            handshake_psk_commitment: [0u8; 32],
            public_key: [0u8; 32],
            valid_from: 0,
            valid_until: u64::MAX,
            nonce: [0u8; 32],
            nickname_suggestion: None,
        })
        .await
        .expect("cache peer-default-context descriptor");

    let bridge = AgentRuntimeBridge::new(agent);
    assert!(
        !bridge.is_peer_online(peer).await,
        "peer online checks must not promote peer-default-context descriptors into current-context reachability"
    );

    crate::runtime::services::RuntimeService::stop(&manager)
        .await
        .expect("stop rendezvous manager");
}

#[tokio::test]
async fn pull_remote_relational_facts_requires_rendezvous_service() {
    let authority = AuthorityId::new_from_entropy([53u8; 32]);
    let peer = AuthorityId::new_from_entropy([54u8; 32]);
    let build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([55u8; 32]),
        ExecutionMode::Testing,
    );
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    let bridge = AgentRuntimeBridge::new(agent);

    let error = bridge
        .pull_remote_relational_facts(peer)
        .await
        .expect_err("missing rendezvous service should be explicit");
    assert!(
        error.to_string().contains("rendezvous_service"),
        "expected rendezvous service error, got: {error}"
    );
}

#[tokio::test]
async fn pull_remote_relational_facts_requires_websocket_direct_hint() {
    let authority = AuthorityId::new_from_entropy([56u8; 32]);
    let peer = AuthorityId::new_from_entropy([57u8; 32]);
    let build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([58u8; 32]),
        ExecutionMode::Testing,
    );
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .with_rendezvous()
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    let manager = agent
        .runtime()
        .rendezvous()
        .expect("runtime rendezvous service");
    manager
        .cache_descriptor(aura_rendezvous::facts::RendezvousDescriptor {
            authority_id: peer,
            device_id: None,
            context_id: default_context_id_for_authority(peer),
            transport_hints: vec![aura_rendezvous::facts::TransportHint::tcp_direct(
                "127.0.0.1:6554",
            )
            .expect("tcp hint")],
            handshake_psk_commitment: [0u8; 32],
            public_key: [0u8; 32],
            valid_from: 0,
            valid_until: u64::MAX,
            nonce: [0u8; 32],
            nickname_suggestion: None,
        })
        .await
        .expect("cache non-websocket descriptor");

    let bridge = AgentRuntimeBridge::new(agent);
    let error = bridge
        .pull_remote_relational_facts(peer)
        .await
        .expect_err("missing websocket hint should be explicit");
    assert!(
        error
            .to_string()
            .contains("No websocket direct transport hint available"),
        "expected websocket-hint error, got: {error}"
    );
}

#[tokio::test]
async fn sync_seeded_peers_requires_sync_service() {
    let authority = AuthorityId::new_from_entropy([59u8; 32]);
    let build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([60u8; 32]),
        ExecutionMode::Testing,
    );
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    let bridge = AgentRuntimeBridge::new(agent);

    let error = bridge
        .sync_seeded_peers()
        .await
        .expect_err("missing sync service should be explicit");
    assert!(
        error.to_string().contains("sync_service"),
        "expected sync service error, got: {error}"
    );
}

#[tokio::test]
async fn sync_seeded_peers_requires_seeded_peer_set() {
    let authority = AuthorityId::new_from_entropy([61u8; 32]);
    let build_context = EffectContext::new(
        authority,
        ContextId::new_from_entropy([62u8; 32]),
        ExecutionMode::Testing,
    );
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(authority)
            .with_sync()
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    let bridge = AgentRuntimeBridge::new(agent);

    let error = bridge
        .sync_seeded_peers()
        .await
        .expect_err("empty sync peer set should be explicit");
    assert!(
        error
            .to_string()
            .contains("No sync peers are available for synchronization"),
        "expected empty-peer sync error, got: {error}"
    );
}

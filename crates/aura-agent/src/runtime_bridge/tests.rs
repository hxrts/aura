use super::*;
use crate::core::AgentConfig;
use crate::AgentBuilder;
use crate::AuraEffectSystem;
use async_lock::Mutex;
use aura_core::context::EffectContext;
use aura_core::effects::storage::StorageCoreEffects;
use aura_core::effects::{CryptoCoreEffects, ExecutionMode};
use aura_core::hash::hash;
use aura_journal::commitment_tree::storage::TREE_OPS_INDEX_KEY;
use std::ffi::OsString;
use std::fs;
use std::future::Future;
use std::path::PathBuf;
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
    tempfile::Builder::new()
        .prefix(&format!("aura-agent-runtime-bridge-{label}-"))
        .tempdir()
        .expect("independent runtime bridge profile")
        .keep()
}

async fn generated_test_public_key(effects: &AuraEffectSystem) -> [u8; 32] {
    let (_, verifying_key) = effects
        .ed25519_generate_keypair()
        .await
        .expect("test peer keypair should generate");
    verifying_key
        .try_into()
        .expect("test verifying key should be 32 bytes")
}

#[track_caller]
fn run_async_test_on_large_stack<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    std::thread::Builder::new()
        .name("runtime-bridge-test-large-stack".to_string())
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
    std::env::remove_var(HARNESS_MODE_ENV_VAR);
    std::env::remove_var(HARNESS_SYNC_ROUNDS_ENV_VAR);
    std::env::remove_var(HARNESS_SYNC_BACKOFF_MS_ENV_VAR);

    assert!(!harness_mode_enabled());
    assert_eq!(harness_sync_rounds(), DEFAULT_HARNESS_SYNC_ROUNDS);
    assert_eq!(harness_sync_backoff_ms(), DEFAULT_HARNESS_SYNC_BACKOFF_MS);
}

#[test]
fn harness_sync_policy_honors_explicit_env_values() {
    let _guard = env_lock().lock_blocking();
    std::env::set_var(HARNESS_MODE_ENV_VAR, "1");
    std::env::set_var(HARNESS_SYNC_ROUNDS_ENV_VAR, "5");
    std::env::set_var(HARNESS_SYNC_BACKOFF_MS_ENV_VAR, "125");

    assert!(harness_mode_enabled());
    assert_eq!(harness_sync_rounds(), 5);
    assert_eq!(harness_sync_backoff_ms(), 125);

    std::env::remove_var(HARNESS_MODE_ENV_VAR);
    std::env::remove_var(HARNESS_SYNC_ROUNDS_ENV_VAR);
    std::env::remove_var(HARNESS_SYNC_BACKOFF_MS_ENV_VAR);
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
    let peer_public_key = generated_test_public_key(agent.runtime().effects().as_ref()).await;
    manager
        .cache_descriptor(aura_rendezvous::facts::RendezvousDescriptor {
            authority_id: peer,
            device_id: None,
            context_id: context,
            transport_hints: vec![aura_rendezvous::facts::TransportHint::tcp_direct(
                "127.0.0.1:6555",
            )
            .expect("tcp hint")],
            handshake_psk_commitment: [7u8; 32],
            public_key: peer_public_key,
            valid_from: 0,
            valid_until: u64::MAX,
            nonce: [9u8; 32],
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
        HARNESS_MODE_ENV_VAR,
        HARNESS_SYNC_ROUNDS_ENV_VAR,
        HARNESS_SYNC_BACKOFF_MS_ENV_VAR,
    ]);
    std::env::set_var(HARNESS_MODE_ENV_VAR, "1");
    std::env::set_var(HARNESS_SYNC_ROUNDS_ENV_VAR, "2");
    std::env::set_var(HARNESS_SYNC_BACKOFF_MS_ENV_VAR, "50");

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
    let peer_public_key = generated_test_public_key(agent.runtime().effects().as_ref()).await;

    let make_descriptor = move |descriptor_context| aura_rendezvous::facts::RendezvousDescriptor {
        authority_id: peer,
        device_id: None,
        context_id: descriptor_context,
        transport_hints: vec![
            aura_rendezvous::facts::TransportHint::tcp_direct("127.0.0.1:6556").expect("tcp hint"),
        ],
        handshake_psk_commitment: [7u8; 32],
        public_key: peer_public_key,
        valid_from: 0,
        valid_until: u64::MAX,
        nonce: [9u8; 32],
        nickname_suggestion: None,
    };

    manager
        .cache_descriptor(make_descriptor(fallback_context))
        .await
        .expect("cache fallback descriptor for initiation");

    let bridge = AgentRuntimeBridge::new(agent);
    bridge
        .bootstrap_signing_keys()
        .await
        .expect("bootstrap local identity keys");
    let error = bridge.ensure_peer_channel(context, peer).await.expect_err(
        "peer channel initiation should fail explicitly when prerequisites are unavailable",
    );
    assert!(
        error.to_string().contains("service unavailable"),
        "expected service-unavailable boundary, got: {error}"
    );
}

#[tokio::test]
async fn seed_authority_route_descriptor_repairs_placeholder_from_other_cached_context() {
    let local_authority = AuthorityId::new_from_entropy([82u8; 32]);
    let peer = AuthorityId::new_from_entropy([83u8; 32]);
    let peer_authority_context = default_context_id_for_authority(peer);
    let other_cached_context = ContextId::new_from_entropy([84u8; 32]);
    let build_context = EffectContext::new(
        local_authority,
        ContextId::new_from_entropy([85u8; 32]),
        ExecutionMode::Testing,
    );
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(local_authority)
            .with_rendezvous()
            .build_testing_async(&build_context)
            .await
            .expect("build testing agent"),
    );
    let manager = agent
        .runtime()
        .rendezvous()
        .expect("runtime rendezvous service");
    let peer_public_key = generated_test_public_key(agent.runtime().effects().as_ref()).await;

    let placeholder_descriptor = aura_rendezvous::facts::RendezvousDescriptor {
        authority_id: peer,
        device_id: None,
        context_id: peer_authority_context,
        transport_hints: vec![
            aura_rendezvous::facts::TransportHint::tcp_direct("127.0.0.1:6557").expect("tcp hint"),
        ],
        handshake_psk_commitment: [0u8; 32],
        public_key: [0u8; 32],
        valid_from: 0,
        valid_until: u64::MAX,
        nonce: [0u8; 32],
        nickname_suggestion: None,
    };
    manager
        .registry()
        .cache_descriptor(placeholder_descriptor)
        .await;

    let mut non_placeholder_descriptor = aura_rendezvous::facts::RendezvousDescriptor {
        authority_id: peer,
        device_id: None,
        context_id: other_cached_context,
        transport_hints: vec![
            aura_rendezvous::facts::TransportHint::tcp_direct("127.0.0.1:6558").expect("tcp hint"),
        ],
        handshake_psk_commitment: [7u8; 32],
        public_key: peer_public_key,
        valid_from: 0,
        valid_until: u64::MAX,
        nonce: [9u8; 32],
        nickname_suggestion: None,
    };
    manager
        .cache_descriptor(non_placeholder_descriptor.clone())
        .await
        .expect("cache non-placeholder descriptor");

    seed_authority_route_descriptor_if_needed(
        agent.runtime().effects().as_ref(),
        local_authority,
        peer,
    )
    .await;

    let repaired_descriptor = manager
        .get_descriptor(peer_authority_context, peer)
        .await
        .expect("authority route descriptor should be present");
    assert!(!descriptor_has_placeholder_crypto(&repaired_descriptor));
    non_placeholder_descriptor.context_id = peer_authority_context;
    assert_eq!(
        repaired_descriptor.public_key,
        non_placeholder_descriptor.public_key
    );
    assert_eq!(
        repaired_descriptor.handshake_psk_commitment,
        non_placeholder_descriptor.handshake_psk_commitment
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

#[test]
fn amp_list_channel_participants_includes_accepted_channel_invitees() {
    run_async_test_on_large_stack(async move {
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
        bridge
            .bootstrap_signing_keys()
            .await
            .expect("bootstrap canonical signing keys as a real account does");
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
    });
}

#[test]
fn amp_list_channel_participants_includes_transported_channel_acceptance() {
    run_async_test_on_large_stack(async move {
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
        for agent in [&sender_agent, &receiver_agent] {
            AgentRuntimeBridge::new(agent.clone())
                .bootstrap_signing_keys()
                .await
                .expect("bootstrap canonical signing keys as a real account does");
        }
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
        let sender_service_context =
            crate::runtime::services::RuntimeServiceContext::test_original(
                Arc::new(crate::runtime::TaskSupervisor::new()),
                Arc::new(sender_effects.time_effects().clone()),
            )
            .await;
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
        let receiver_service_context =
            crate::runtime::services::RuntimeServiceContext::test_original(
                Arc::new(crate::runtime::TaskSupervisor::new()),
                Arc::new(receiver_effects.time_effects().clone()),
            )
            .await;
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
        let exported = crate::handlers::invitation::ShareableInvitation {
            version: crate::handlers::invitation::ShareableInvitation::CURRENT_VERSION,
            invitation_id: invitation.invitation_id.clone(),
            sender_id: invitation.sender_id,
            context_id: Some(invitation.context_id),
            invitation_type: invitation.invitation_type.clone(),
            expires_at: invitation.expires_at,
            message: invitation.message.clone(),
        }
        .to_code()
        .expect("shareable invitation should serialize");
        let imported = receiver_invitations
            .import_and_cache(&exported)
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
    });
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
            home: false,
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

#[test]
fn channel_name_lookup_requires_matching_creation_fact() {
    let context = ContextId::new_from_entropy([201u8; 32]);
    let other_context = ContextId::new_from_entropy([202u8; 32]);
    let channel = ChannelId::from_bytes(hash(b"created-name-lookup"));
    let actor = AuthorityId::new_from_entropy([203u8; 32]);
    let update = ChatFact::channel_updated_ms(
        context,
        channel,
        Some("renamed".to_string()),
        None,
        None,
        None,
        30,
        actor,
    );
    assert!(resolve_created_channel_ids_by_name([update.clone()], "renamed").is_empty());

    let creation = ChatFact::channel_created_ms(
        context,
        channel,
        "original".to_string(),
        None,
        false,
        10,
        actor,
    );
    assert_eq!(
        resolve_created_channel_ids_by_name([update, creation], "renamed"),
        vec![channel]
    );
    let wrong_context_update = ChatFact::channel_updated_ms(
        other_context,
        channel,
        Some("wrong context".to_string()),
        None,
        None,
        None,
        40,
        actor,
    );
    let creation = ChatFact::channel_created_ms(
        context,
        channel,
        "original".to_string(),
        None,
        false,
        10,
        actor,
    );
    assert!(
        resolve_created_channel_ids_by_name([wrong_context_update, creation], "wrong context",)
            .is_empty()
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
    assert_eq!(
        error.kind(),
        aura_app::runtime_bridge::RuntimeBridgeErrorKind::Storage
    );
    assert!(native_identity_has_source::<aura_core::effects::StorageError>(&error));
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
    assert_eq!(
        error.kind(),
        aura_app::runtime_bridge::RuntimeBridgeErrorKind::Storage
    );
    assert!(native_identity_has_source::<aura_core::effects::StorageError>(&error));
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
    assert_eq!(
        error.kind(),
        aura_app::runtime_bridge::RuntimeBridgeErrorKind::Storage
    );
    assert!(native_identity_has_source::<aura_core::effects::StorageError>(&error));
    let message = error.to_string();
    assert!(
        message.contains("Failed to read account.json")
            || message.contains("Read account config failed"),
        "authority-list failure should surface the account config read error: {message}"
    );

    for failure in [
        bridge
            .has_account_config()
            .await
            .expect_err("required bootstrap read rejects IO fault"),
        bridge
            .initialize_account("new nickname")
            .await
            .expect_err("initialization rejects IO fault"),
    ] {
        assert_eq!(
            failure.kind(),
            aura_app::runtime_bridge::RuntimeBridgeErrorKind::Storage
        );
        assert!(native_identity_has_source::<aura_core::effects::StorageError>(&failure));
    }
    assert!(
        storage_root.join("account.json.dat").is_dir(),
        "bootstrap must not replace failed config read"
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
    bridge
        .agent
        .runtime()
        .effects()
        .store(&record_key, b"not-json".to_vec())
        .await
        .expect("write corrupt authority record");

    let error = bridge
        .try_list_authorities()
        .await
        .expect_err("corrupt authority record should be explicit");
    assert_eq!(
        error.kind(),
        aura_app::runtime_bridge::RuntimeBridgeErrorKind::Serialization
    );
    assert!(native_identity_has_source::<serde_json::Error>(&error));
    let message = error.to_string();
    assert!(
        message.contains("Failed to read authority record")
            || message.contains("Read authority record failed")
            || message.contains("Failed to decode authority record")
            || message.contains("Decode authority record failed"),
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
    assert_eq!(
        error.kind(),
        aura_app::runtime_bridge::RuntimeBridgeErrorKind::Storage
    );
    assert!(native_identity_has_source::<aura_core::effects::StorageError>(&error));
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
        .bootstrap_signing_keys()
        .await
        .expect("bootstrap canonical signing keys as a real account does");

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
        error
            .to_string()
            .to_ascii_lowercase()
            .replace('_', " ")
            .contains("invitation service"),
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
    let service_context = crate::runtime::services::RuntimeServiceContext::test_original(
        Arc::new(crate::runtime::TaskSupervisor::new()),
        Arc::new(effects.time_effects().clone()),
    )
    .await;
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
            handshake_psk_commitment: [7u8; 32],
            public_key: [8u8; 32],
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
async fn pull_remote_relational_facts_is_disabled_without_authenticated_sync_service() {
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
        .expect_err("direct LAN fact pull must fail closed");
    assert!(
        error
            .to_string()
            .contains("Direct LAN relational fact pull has been removed"),
        "expected direct-LAN-removal error, got: {error}"
    );
}

#[tokio::test]
async fn pull_remote_relational_facts_stays_disabled_even_with_rendezvous_hints() {
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
            handshake_psk_commitment: [7u8; 32],
            public_key: [8u8; 32],
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
        .expect_err("direct LAN fact pull must remain unavailable");
    assert!(
        error
            .to_string()
            .contains("Direct LAN relational fact pull has been removed"),
        "expected direct-LAN-removal error, got: {error}"
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

#[test]
fn leaving_a_channel_removes_it_from_the_chat_projection() {
    run_async_test_on_large_stack(async move {
        use aura_journal::DomainFact as _;
        let authority = AuthorityId::new_from_entropy([70u8; 32]);
        let build_context = EffectContext::new(
            authority,
            ContextId::new_from_entropy([71u8; 32]),
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
        let effects = agent.runtime().effects();
        let started = aura_core::effects::PhysicalTimeEffects::physical_time(effects.as_ref())
            .await
            .expect("actual original projection test observation");
        let original = aura_core::TimeoutBudget::from_start_and_timeout(
            &started,
            std::time::Duration::from_secs(30),
        )
        .expect("original bounded projection scenario");
        let context = ContextId::new_from_entropy([72u8; 32]);
        let channel = ChannelId::from_bytes(hash(b"leave-removes-channel"));
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
        let created = aura_chat::ChatFact::channel_created_ms(
            context,
            channel,
            "lab".to_string(),
            None,
            false,
            1,
            authority,
        )
        .to_generic();
        bridge
            .commit_relational_facts(std::slice::from_ref(&created))
            .await
            .expect("commit channel");
        effects
            .await_reactive_publications_in_original_window(&original)
            .await
            .expect("exact original processing barrier");
        let listed = |chat: aura_app::views::chat::ChatState| chat.channel(&channel).is_some();
        let chat = effects
            .reactive_handler()
            .read(&*aura_app::signal_defs::CHAT_SIGNAL)
            .await
            .expect("chat signal");
        assert!(listed(chat), "channel listed after creation");

        bridge
            .amp_leave_channel(ChannelLeaveParams {
                context,
                channel,
                participant: authority,
            })
            .await
            .expect("leave channel");
        effects
            .await_reactive_publications_in_original_window(&original)
            .await
            .expect("exact original processing barrier");
        let chat = effects
            .reactive_handler()
            .read(&*aura_app::signal_defs::CHAT_SIGNAL)
            .await
            .expect("chat signal");
        assert!(!listed(chat), "channel removed after leaving");
    });
}

/// Regression (work/8.md task 7): with one existing device, the initiator
/// finalizes the enrollment itself once the new device accepts. Cross-machine,
/// the invitee reported success while the initiator failed to sign the commit.
#[test]
fn enrollment_receipt_store_failure_keeps_acceptance_invisible() {
    run_async_test_on_large_stack(async move {
        let (issuer, _invitee, _invitation, start, _accept, verified) =
            crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                "receipt-store-fault",
            )
            .await;
        crate::handlers::invitation::enrollment_trust::fail_next_receipt_store_for_test(
            issuer.runtime().effects().as_ref(),
            start.ceremony_id.clone(),
        )
        .await;
        let error = issuer
            .runtime()
            .ceremony_runner()
            .record_verified_enrollment_response(verified.clone())
            .await
            .expect_err("required receipt write must fail");
        assert!(
            std::error::Error::source(&error).is_some(),
            "fault source must survive recorder"
        );
        let state = issuer
            .runtime()
            .ceremony_tracker()
            .get(&start.ceremony_id)
            .await
            .unwrap();
        assert!(state.accepted_participants.is_empty());
        assert!(state.terminal_outcome.is_none());
        assert!(issuer
            .runtime()
            .ceremony_tracker()
            .require_verified_enrollment_response(&start.ceremony_id)
            .await
            .is_err());
        issuer
            .runtime()
            .ceremony_runner()
            .record_verified_enrollment_response(verified)
            .await
            .unwrap();
        assert_eq!(
            issuer
                .runtime()
                .ceremony_tracker()
                .get(&start.ceremony_id)
                .await
                .unwrap()
                .accepted_participants
                .len(),
            1
        );
    });
}

#[test]
fn enrollment_pending_registration_and_signing_generation_resume_on_runtime_restart() {
    run_async_test_on_large_stack(async move {
        let (issuer, invitee, invitation, start, _accept, _old_witness) =
            crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                "runtime-restart",
            )
            .await;
        let authority = issuer.authority_id();
        let config = issuer.runtime().effects().config().clone();
        let context = EffectContext::new(
            authority,
            issuer.context().default_context_id(),
            ExecutionMode::Testing,
        );
        let before = issuer
            .runtime()
            .ceremony_tracker()
            .get(&start.ceremony_id)
            .await
            .unwrap();
        let original_budget = issuer
            .runtime()
            .ceremony_runner()
            .enrollment_window_budget(&start.ceremony_id)
            .await
            .unwrap();
        assert!(before.accepted_participants.is_empty());
        issuer
            .runtime()
            .tasks()
            .shutdown_with_timeout(std::time::Duration::from_secs(2))
            .await
            .unwrap();
        drop(issuer);
        let restarted = Arc::new(
            AgentBuilder::new()
                .with_authority(authority)
                .with_config(config)
                .build_testing_async(&context)
                .await
                .expect("reopen actual persisted runtime"),
        );
        let bridge = AgentRuntimeBridge::new(restarted.clone());
        bridge
            .bootstrap_signing_keys()
            .await
            .expect("restore active signing then pending ownership");
        let after = restarted
            .runtime()
            .ceremony_tracker()
            .get(&start.ceremony_id)
            .await
            .unwrap();
        let restored_budget = restarted
            .runtime()
            .ceremony_runner()
            .enrollment_window_budget(&start.ceremony_id)
            .await
            .unwrap();
        assert_eq!(
            restored_budget.started_at_ms(),
            original_budget.started_at_ms()
        );
        assert_eq!(
            restored_budget.deadline_at_ms(),
            original_budget.deadline_at_ms()
        );
        assert_eq!(after.started_at, before.started_at);
        assert_eq!(after.prestate_hash, before.prestate_hash);
        assert_eq!(after.enrollment_device_id, before.enrollment_device_id);
        assert!(
            after.accepted_participants.is_empty(),
            "restart does not manufacture acceptance"
        );
        let verified = crate::handlers::invitation::enrollment_trust::verify_actual_invitee_acceptance_for_test(
            restarted.runtime().effects().as_ref(), invitee.runtime().effects().as_ref(), &invitation,
            authority, &start.ceremony_id, start.device_id, start.pending_epoch.value(),
        ).await.unwrap();
        restarted
            .runtime()
            .ceremony_runner()
            .record_verified_enrollment_response(verified)
            .await
            .unwrap();
        let service = crate::handlers::device_epoch_rotation::DeviceEpochRotationService::new(
            authority,
            restarted.runtime().effects(),
            restarted.runtime().ceremony_tracker().clone(),
            restarted.runtime().ceremony_runner().clone(),
            restarted.runtime().threshold_signing(),
            restarted.runtime().reconfiguration().clone(),
        );
        service
            .finalize_sole_device_enrollment(&start.ceremony_id)
            .await
            .expect("activate recovered exact generation");
        let tree = restarted
            .runtime()
            .effects()
            .get_current_state()
            .await
            .unwrap();
        assert_eq!(
            tree.leaves
                .values()
                .filter(|leaf| leaf.device_id == start.device_id)
                .count(),
            1
        );
        assert_eq!(
            restarted
                .runtime()
                .ceremony_runner()
                .terminal_outcome(&start.ceremony_id)
                .await
                .unwrap(),
            Some(aura_app::runtime_bridge::CeremonyTerminalOutcome::Committed)
        );
        service
            .finalize_sole_device_enrollment(&start.ceremony_id)
            .await
            .unwrap();
        let tree = restarted
            .runtime()
            .effects()
            .get_current_state()
            .await
            .unwrap();
        assert_eq!(
            tree.leaves
                .values()
                .filter(|leaf| leaf.device_id == start.device_id)
                .count(),
            1
        );
    });
}

#[test]
fn enrollment_prepared_tree_operation_reconciles_before_and_after_tree_apply() {
    run_async_test_on_large_stack(async move {
        for after_tree in [false, true] {
            let label = if after_tree {
                "activation-after-tree"
            } else {
                "activation-before-tree"
            };
            let (issuer, _invitee, _invitation, start, _accept, witness) =
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(label)
                    .await;
            issuer
                .runtime()
                .tasks()
                .shutdown_with_timeout(std::time::Duration::from_secs(2))
                .await
                .unwrap();
            issuer
                .runtime()
                .ceremony_runner()
                .record_verified_enrollment_response(witness)
                .await
                .unwrap();
            crate::handlers::device_epoch_rotation::fail_activation_for_test(
                issuer.runtime().effects().as_ref(),
                start.ceremony_id.clone(),
                after_tree,
            )
            .await;
            let authority = issuer.authority_id();
            let config = issuer.runtime().effects().config().clone();
            let context = EffectContext::new(
                authority,
                issuer.context().default_context_id(),
                ExecutionMode::Testing,
            );
            let service = crate::handlers::device_epoch_rotation::DeviceEpochRotationService::new(
                authority,
                issuer.runtime().effects(),
                issuer.runtime().ceremony_tracker().clone(),
                issuer.runtime().ceremony_runner().clone(),
                issuer.runtime().threshold_signing(),
                issuer.runtime().reconfiguration().clone(),
            );
            service
                .finalize_sole_device_enrollment(&start.ceremony_id)
                .await
                .expect_err("stop at exact preparation boundary");
            let tree = issuer
                .runtime()
                .effects()
                .get_current_state()
                .await
                .unwrap();
            assert_eq!(
                tree.leaves
                    .values()
                    .any(|leaf| leaf.device_id == start.device_id),
                after_tree
            );
            assert_eq!(
                issuer
                    .runtime()
                    .ceremony_runner()
                    .terminal_outcome(&start.ceremony_id)
                    .await
                    .unwrap(),
                None
            );
            assert!(
                issuer
                    .runtime()
                    .ceremony_runner()
                    .abort(&start.ceremony_id, None)
                    .await
                    .is_err(),
                "cancellation cannot replace a durably prepared activation owner"
            );
            drop(service);
            drop(issuer);
            let restarted = Arc::new(
                AgentBuilder::new()
                    .with_authority(authority)
                    .with_config(config)
                    .build_testing_async(&context)
                    .await
                    .unwrap(),
            );
            AgentRuntimeBridge::new(restarted.clone())
                .bootstrap_signing_keys()
                .await
                .unwrap();
            let resumed = crate::handlers::device_epoch_rotation::DeviceEpochRotationService::new(
                authority,
                restarted.runtime().effects(),
                restarted.runtime().ceremony_tracker().clone(),
                restarted.runtime().ceremony_runner().clone(),
                restarted.runtime().threshold_signing(),
                restarted.runtime().reconfiguration().clone(),
            );
            resumed
                .finalize_sole_device_enrollment(&start.ceremony_id)
                .await
                .expect("reconcile exact retained operation");
            let tree = restarted
                .runtime()
                .effects()
                .get_current_state()
                .await
                .unwrap();
            assert_eq!(
                tree.leaves
                    .values()
                    .filter(|leaf| leaf.device_id == start.device_id)
                    .count(),
                1
            );
            assert_eq!(
                restarted
                    .runtime()
                    .ceremony_runner()
                    .terminal_outcome(&start.ceremony_id)
                    .await
                    .unwrap(),
                Some(aura_app::runtime_bridge::CeremonyTerminalOutcome::Committed)
            );
        }
    });
}

#[test]
fn sole_device_enrollment_commits_after_new_device_accepts() {
    run_async_test_on_large_stack(async move {
        let authority = AuthorityId::new_from_entropy([73u8; 32]);
        // Fresh storage: a previous run's enrolled device would otherwise make
        // this a multi-device account.
        let storage_root = unique_test_path("sole-device-enrollment");
        let _ = fs::remove_dir_all(&storage_root);
        let mut config = AgentConfig::default();
        config.storage.base_path = storage_root;
        let build_context = EffectContext::new(
            authority,
            ContextId::new_from_entropy([74u8; 32]),
            ExecutionMode::Testing,
        );
        let agent = Arc::new(
            AgentBuilder::new()
                .with_authority(authority)
                .with_config(config)
                .build_testing_async(&build_context)
                .await
                .expect("build testing agent"),
        );
        let bridge = AgentRuntimeBridge::new(agent.clone());
        bridge
            .bootstrap_signing_keys()
            .await
            .expect("bootstrap local identity keys");

        let baseline = agent
            .runtime()
            .effects()
            .export_tree_ops()
            .await
            .expect("baseline");

        let invitee_authority = AuthorityId::new_from_entropy([75u8; 32]);
        let invitee_context = EffectContext::new(
            invitee_authority,
            ContextId::new_from_entropy([76u8; 32]),
            ExecutionMode::Testing,
        );
        let invitee_config = AgentConfig {
            device_id: aura_core::DeviceId::new_from_entropy([77u8; 32]),
            storage: crate::core::config::StorageConfig {
                base_path: unique_test_path("sole-device-enrollment-invitee"),
                ..Default::default()
            },
            ..Default::default()
        };
        let invitee = Arc::new(
            AgentBuilder::new()
                .with_authority(invitee_authority)
                .with_config(invitee_config)
                .build_testing_async(&invitee_context)
                .await
                .expect("build actual invitee runtime"),
        );
        let invitee_bridge = AgentRuntimeBridge::new(invitee.clone());
        invitee_bridge
            .bootstrap_signing_keys()
            .await
            .expect("invitee signing ready");
        let setup_code = invitee_bridge
            .export_device_enrollment_setup_request()
            .await
            .expect("export actual invitee setup");
        let initiator_app = Arc::new(async_lock::RwLock::new(
            aura_app::AppCore::with_runtime(
                aura_app::AppConfig::default(),
                Arc::new(AgentRuntimeBridge::new(agent.clone())),
            )
            .expect("initiator app"),
        ));
        let setup =
            aura_app::ui::workflows::ceremonies::pin_user_transferred_device_enrollment_setup(
                &initiator_app,
                setup_code,
            )
            .await
            .expect("verify explicitly transferred invitee setup");

        let start = bridge
            .initiate_device_enrollment_ceremony("Tablet".to_string(), setup)
            .await
            .expect("start device enrollment");
        assert_eq!(start.device_id, invitee.context().device_id());
        let runner = agent.runtime().ceremony_runner().clone();
        let decoded =
            aura_invitation::shareable::ShareableInvitation::from_code(&start.enrollment_code)
                .expect("actual issued enrollment code");
        let invitation = agent
            .invitations()
            .expect("issuer invitation service")
            .get(&decoded.invitation_id)
            .await
            .expect("actual issued invitation");
        let transfer = start
            .manifest_transfer
            .as_ref()
            .expect("actual issuer manifest transfer");
        let invitee_app = Arc::new(async_lock::RwLock::new(
            aura_app::AppCore::with_runtime(
                aura_app::AppConfig::default(),
                Arc::new(AgentRuntimeBridge::new(invitee.clone())),
            )
            .expect("actual invitee app"),
        ));
        let manifest_pin =
            aura_app::ui::workflows::ceremonies::pin_user_transferred_enrollment_manifest(
                &invitee_app,
                transfer.manifest_code.clone(),
                transfer.initiator_verifier_code.clone(),
            )
            .await
            .expect("independent actual manifest and verifier transfer");
        invitee_bridge
            .import_enrollment_invitation(&start.enrollment_code, manifest_pin)
            .await
            .expect("actual invitee admission before canonical acceptance");
        let verified = crate::handlers::invitation::enrollment_trust::verify_actual_invitee_acceptance_for_test(
            agent.runtime().effects().as_ref(), invitee.runtime().effects().as_ref(),
            &invitation, authority, &start.ceremony_id, start.device_id,
            start.pending_epoch.value(),
        ).await.expect("verify canonical provisional acceptance under retained setup key");
        let duplicate = crate::handlers::invitation::enrollment_trust::verify_actual_invitee_acceptance_for_test(
            agent.runtime().effects().as_ref(), invitee.runtime().effects().as_ref(),
            &invitation, authority, &start.ceremony_id, start.device_id,
            start.pending_epoch.value(),
        ).await.expect("independently signed duplicate canonical decision");
        runner
            .record_verified_enrollment_response(verified)
            .await
            .expect("record actual verified device acceptance");
        runner
            .record_verified_enrollment_response(duplicate)
            .await
            .expect("duplicate proof is idempotent");
        assert_eq!(
            agent
                .runtime()
                .ceremony_tracker()
                .get(&start.ceremony_id)
                .await
                .unwrap()
                .accepted_participants
                .len(),
            1
        );

        let service = crate::handlers::device_epoch_rotation::DeviceEpochRotationService::new(
            authority,
            agent.runtime().effects(),
            agent.runtime().ceremony_tracker().clone(),
            runner.clone(),
            agent.runtime().threshold_signing(),
            agent.runtime().reconfiguration().clone(),
        );
        service
            .finalize_sole_device_enrollment(&start.ceremony_id)
            .await
            .expect("initiator should finalize the enrollment");
        let effects = agent.runtime().effects();
        let ceremony = agent
            .runtime()
            .ceremony_tracker()
            .get(&start.ceremony_id)
            .await
            .expect("ceremony");
        assert!(
            ceremony.is_committed,
            "enrollment should commit, got error {:?}",
            ceremony.error_message
        );
        let tree = aura_protocol::effects::TreeEffects::get_current_state(effects.as_ref())
            .await
            .expect("tree state");
        assert!(tree
            .leaves
            .values()
            .any(|leaf| leaf.device_id == start.device_id));
        let devices = super::identity::list_devices(&bridge)
            .await
            .expect("list devices");
        assert_eq!(
            devices.len(),
            2,
            "initiator should list itself and the new device"
        );

        // The joining device holds the pre-enrollment baseline; replicating the
        // initiator's verified ops gives it its own leaf (work/8.md task 32).
        let joiner =
            crate::testing::simulation_effect_system_with_shared_transport_for_authority_arc(
                &crate::core::AgentConfig {
                    device_id: start.device_id,
                    storage: crate::core::config::StorageConfig {
                        base_path: unique_test_path("device-enrollment-joiner"),
                        ..Default::default()
                    },
                    ..crate::core::AgentConfig::default()
                },
                authority,
                crate::SharedTransport::new(),
            );
        joiner
            .replace_tree_ops(&baseline)
            .await
            .expect("adopt baseline");
        let all_ops = effects.export_tree_ops().await.expect("initiator ops");
        // A tree frame cannot supply its own trust anchor. The fixture's
        // secure-store copy stands in for an independently authenticated
        // enrollment handoff of the parent's verifier and policy.
        let missing = joiner.import_verified_tree_ops(&all_ops).await;
        assert!(missing.is_err(), "an empty verifier store must fail closed");
        let parent_key = aura_core::effects::SecureStorageLocation::with_sub_key(
            "threshold_pubkey",
            authority.to_string(),
            "0",
        );
        let parent_policy = aura_core::effects::SecureStorageLocation::with_sub_key(
            "threshold_config",
            authority.to_string(),
            "0",
        );
        for location in [&parent_key, &parent_policy] {
            let package = aura_core::effects::SecureStorageEffects::secure_retrieve(
                effects.as_ref(),
                location,
                &[aura_core::effects::SecureStorageCapability::Read],
            )
            .await
            .expect("initiator parent verifier");
            aura_core::effects::SecureStorageEffects::secure_store(
                joiner.as_ref(),
                location,
                &package,
                &[
                    aura_core::effects::SecureStorageCapability::Read,
                    aura_core::effects::SecureStorageCapability::Write,
                ],
            )
            .await
            .expect("authenticated parent verifier fixture");
        }
        let mut forged = all_ops.clone();
        forged.last_mut().expect("enrollment extension").agg_sig[0] ^= 0x01;
        assert!(
            joiner.import_verified_tree_ops(&forged).await.is_err(),
            "forged extending operation must fail with a trusted parent verifier"
        );
        assert_eq!(
            joiner.export_tree_ops().await.expect("after forgery"),
            baseline
        );
        let resulting_state =
            aura_journal::commitment_tree::reduce(&all_ops).expect("initiator tree should reduce");
        let mut forged_tail = all_ops.last().expect("enrollment extension").clone();
        forged_tail.op.parent_epoch = resulting_state.epoch;
        forged_tail.op.parent_commitment = resulting_state.root_commitment;
        let mut mixed_batch = all_ops.clone();
        mixed_batch.push(forged_tail);
        assert!(joiner.import_verified_tree_ops(&mixed_batch).await.is_err());
        assert_eq!(
            joiner.export_tree_ops().await.expect("after mixed batch"),
            baseline,
            "a bad later operation must not partially persist a valid prefix"
        );
        assert!(
            joiner
                .import_verified_tree_ops(&all_ops)
                .await
                .expect("import")
                > 0
        );
        let joiner_tree = aura_protocol::effects::TreeEffects::get_current_state(joiner.as_ref())
            .await
            .expect("joiner tree");
        assert!(joiner_tree
            .leaves
            .values()
            .any(|leaf| leaf.device_id == start.device_id));

        // A provisional op a joining device made for itself does not extend the
        // account tree and is not applied by the initiator.
        let provisional =
            crate::testing::simulation_effect_system_with_shared_transport_for_authority_arc(
                &crate::core::AgentConfig {
                    device_id: aura_core::DeviceId::new_from_entropy([0x7E; 32]),
                    storage: crate::core::config::StorageConfig {
                        base_path: unique_test_path("device-enrollment-provisional"),
                        ..Default::default()
                    },
                    ..crate::core::AgentConfig::default()
                },
                authority,
                crate::SharedTransport::new(),
            );
        crate::runtime::services::ThresholdSigningService::new(provisional.clone())
            .bootstrap_authority(&authority)
            .await
            .expect("provisional bootstrap");
        let before = effects.export_tree_ops().await.expect("before");
        let provisional_ops = provisional
            .export_tree_ops()
            .await
            .expect("provisional ops");
        assert!(effects
            .import_verified_tree_ops(&provisional_ops)
            .await
            .is_err());
        assert_eq!(effects.export_tree_ops().await.expect("after"), before);
    });
}

#[test]
fn enrollment_cancelled_generation_deletion_failure_restarts_and_reissues() {
    run_async_test_on_large_stack(async move {
        let (issuer, invitee, _invitation, start, _accept, _witness) =
            crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                "retirement-reissue",
            )
            .await;
        let authority = issuer.authority_id();
        let config = issuer.runtime().effects().config().clone();
        let context = EffectContext::new(
            authority,
            issuer.context().default_context_id(),
            ExecutionMode::Testing,
        );
        AgentRuntimeBridge::new(issuer.clone())
            .cancel_key_rotation_ceremony(&start.ceremony_id)
            .await
            .expect("genuine cancellation publishes before shutting down its sink");
        issuer
            .runtime()
            .tasks()
            .shutdown_with_timeout(std::time::Duration::from_secs(2))
            .await
            .unwrap();
        issuer
            .runtime()
            .effects()
            .fail_next_enrollment_retirement_for_test(start.pending_epoch.value());
        let bridge = AgentRuntimeBridge::new(issuer.clone());
        let error = issuer
            .runtime()
            .ceremony_tracker()
            .retire_failed_enrollment_generation(&start.ceremony_id)
            .await
            .expect_err("required secure deletion must fail");
        assert!(std::error::Error::source(&error).is_some());
        let original = issuer
            .runtime()
            .ceremony_runner()
            .terminal_outcome(&start.ceremony_id)
            .await
            .unwrap();
        assert!(matches!(
            original,
            Some(aura_app::runtime_bridge::CeremonyTerminalOutcome::Failed(_))
        ));
        let profile = crate::runtime::effects::enrollment_generation_profile_location(
            &authority,
            start.pending_epoch.value(),
        );
        assert!(
            issuer
                .runtime()
                .effects()
                .secure_exists(&profile)
                .await
                .unwrap(),
            "interrupted cleanup retains activation fence"
        );
        drop(bridge);
        drop(issuer);
        let restarted = Arc::new(
            AgentBuilder::new()
                .with_authority(authority)
                .with_config(config)
                .build_testing_async(&context)
                .await
                .expect("reopen failed generation"),
        );
        let bridge = AgentRuntimeBridge::new(restarted.clone());
        bridge
            .bootstrap_signing_keys()
            .await
            .expect("retry original failed cleanup under owned bootstrap");
        assert_eq!(
            restarted
                .runtime()
                .ceremony_runner()
                .terminal_outcome(&start.ceremony_id)
                .await
                .unwrap(),
            original
        );
        assert!(!restarted
            .runtime()
            .effects()
            .secure_exists(&profile)
            .await
            .unwrap());
        let setup_code = AgentRuntimeBridge::new(invitee.clone())
            .export_device_enrollment_setup_request()
            .await
            .unwrap();
        let app = Arc::new(async_lock::RwLock::new(
            aura_app::AppCore::with_runtime(
                aura_app::AppConfig::default(),
                Arc::new(AgentRuntimeBridge::new(restarted.clone())),
            )
            .unwrap(),
        ));
        let setup =
            aura_app::ui::workflows::ceremonies::pin_user_transferred_device_enrollment_setup(
                &app, setup_code,
            )
            .await
            .unwrap();
        let replacement = bridge
            .initiate_device_enrollment_ceremony("reissued".to_string(), setup)
            .await
            .expect("fresh owned generation after strict retirement");
        assert_ne!(replacement.ceremony_id, start.ceremony_id);
        assert_eq!(replacement.pending_epoch, start.pending_epoch);
        assert_eq!(
            restarted
                .runtime()
                .ceremony_runner()
                .terminal_outcome(&start.ceremony_id)
                .await
                .unwrap(),
            original
        );
    });
}

#[test]
fn enrollment_unissued_allocation_preserves_first_retirement_and_releases_after_restart() {
    run_async_test_on_large_stack(async move {
        let (issuer, invitee, _invitation, first, _accept, _witness) =
            crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                "orphan-retirement",
            )
            .await;
        AgentRuntimeBridge::new(issuer.clone())
            .cancel_key_rotation_ceremony(&first.ceremony_id)
            .await
            .expect("genuine cancellation publishes before shutting down its sink");
        issuer
            .runtime()
            .tasks()
            .shutdown_with_timeout(std::time::Duration::from_secs(2))
            .await
            .unwrap();
        let authority = issuer.authority_id();
        let config = issuer.runtime().effects().config().clone();
        let context = EffectContext::new(
            authority,
            issuer.context().default_context_id(),
            ExecutionMode::Testing,
        );
        let setup_code = AgentRuntimeBridge::new(invitee.clone())
            .export_device_enrollment_setup_request()
            .await
            .unwrap();
        let app = Arc::new(async_lock::RwLock::new(
            aura_app::AppCore::with_runtime(
                aura_app::AppConfig::default(),
                Arc::new(AgentRuntimeBridge::new(issuer.clone())),
            )
            .unwrap(),
        ));
        let setup =
            aura_app::ui::workflows::ceremonies::pin_user_transferred_device_enrollment_setup(
                &app, setup_code,
            )
            .await
            .unwrap();
        let service = issuer.invitations().unwrap();
        let reserved = service
            .reserve_device_enrollment_invitation()
            .await
            .unwrap();
        let ceremony = aura_core::CeremonyId::new(format!("unissued:{}", reserved.invitation_id()));
        let effects = issuer.runtime().effects();
        let plan = effects
            .prepare_authenticated_enrollment_rotation(&setup, issuer.runtime().ceremony_tracker())
            .await
            .expect("actual authenticated current roster");
        let (epoch, _packages, _public, allocation) = effects
            .prepare_pinned_enrollment_rotation(&setup, &reserved, &ceremony, plan)
            .await
            .unwrap();
        drop(allocation); // Crash before invitation fact commit or registration.
        effects.fail_next_enrollment_retirement_for_test(epoch);
        let error = effects
            .retire_unissued_enrollment_allocation()
            .await
            .expect_err("release fault retains original orphan decision");
        assert!(std::error::Error::source(&error).is_some());
        let decision = aura_core::effects::SecureStorageLocation::new(
            "device_enrollment_orphan_retirement_v1",
            ceremony.to_string(),
        );
        let original = effects
            .secure_retrieve(
                &decision,
                &[aura_core::effects::SecureStorageCapability::Read],
            )
            .await
            .unwrap();
        let profile =
            crate::runtime::effects::enrollment_generation_profile_location(&authority, epoch);
        assert!(effects.secure_exists(&profile).await.unwrap());
        drop(effects);
        drop(service);
        drop(app);
        drop(issuer);
        let restarted = Arc::new(
            AgentBuilder::new()
                .with_authority(authority)
                .with_config(config)
                .build_testing_async(&context)
                .await
                .expect("reopen actual orphan allocation"),
        );
        AgentRuntimeBridge::new(restarted.clone())
            .bootstrap_signing_keys()
            .await
            .expect("finish original orphan retirement");
        assert!(!restarted
            .runtime()
            .effects()
            .secure_exists(&profile)
            .await
            .unwrap());
        assert_eq!(
            restarted
                .runtime()
                .effects()
                .secure_retrieve(
                    &decision,
                    &[aura_core::effects::SecureStorageCapability::Read]
                )
                .await
                .unwrap(),
            original
        );
        assert!(!restarted
            .runtime()
            .ceremony_tracker()
            .list_device_enrollment_ceremonies()
            .await
            .unwrap()
            .contains(&ceremony));
    });
}

fn native_identity_has_source<E: std::error::Error + 'static>(
    error: &(dyn std::error::Error + 'static),
) -> bool {
    let mut current = Some(error);
    while let Some(error) = current {
        if error.is::<E>() {
            return true;
        }
        current = error.source();
    }
    false
}

#[tokio::test]
async fn required_moderation_rejects_corrupt_committed_ban_with_native_codec_source() {
    use aura_journal::DomainFact as _;
    let authority = AuthorityId::new_from_entropy([235; 32]);
    let context = ContextId::new_from_entropy([236; 32]);
    let channel = ChannelId::from_bytes([237; 32]);
    let root = tempfile::tempdir().expect("actual isolated storage profile");
    let config = AgentConfig {
        storage: crate::core::config::StorageConfig {
            base_path: root.path().to_path_buf(),
            ..Default::default()
        },
        ..Default::default()
    };
    let build_context = EffectContext::new(authority, context, ExecutionMode::Testing);
    let agent = Arc::new(
        Box::pin(
            AgentBuilder::new()
                .with_authority(authority)
                .with_config(config)
                .build_testing_async(&build_context),
        )
        .await
        .expect("actual runtime"),
    );
    let bridge = AgentRuntimeBridge::new(agent);
    let mut envelope = aura_social::HomeBanFact {
        context_id: context,
        channel_id: Some(channel),
        banned_authority: authority,
        actor_authority: authority,
        reason: "actual committed corrupt test payload".into(),
        banned_at: aura_core::PhysicalTime {
            ts_ms: 1,
            uncertainty: None,
        },
        expires_at: None,
    }
    .to_envelope();
    envelope.encoding = aura_core::types::facts::FactEncoding::Json;
    envelope.payload = b"not-json".to_vec();
    bridge
        .agent
        .runtime()
        .effects()
        .commit_relational_facts(vec![RelationalFact::Generic {
            context_id: context,
            envelope,
        }])
        .await
        .expect("persist actual journal wrapper with corrupt domain payload");
    let error = bridge
        .moderation_status(context, channel, authority, 2)
        .await
        .expect_err("corrupt ban cannot authorize absence");
    assert_eq!(
        error.kind(),
        aura_app::runtime_bridge::RuntimeBridgeErrorKind::Serialization
    );
    assert!(native_identity_has_source::<serde_json::Error>(&error));
    assert!(native_identity_has_source::<
        aura_social::RequiredModerationQueryError,
    >(&error));
}

#[test]
fn runtime_enrollment_cancellation_uses_original_issued_owner_and_window() {
    run_async_test_on_large_stack(async move {
        let (issuer, _invitee, invitation, start, _accept, _witness) =
            crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                "runtime-issued-cancellation-owner",
            )
            .await;
        // Required fact publication must retain its live service owner. The
        // invitee has not accepted, so this real live initiator cannot commit.
        let tracker = issuer.ceremony_tracker().await;
        let before = tracker
            .get(&start.ceremony_id)
            .await
            .expect("original registration");
        AgentRuntimeBridge::new(issuer.clone())
            .cancel_key_rotation_ceremony(&start.ceremony_id)
            .await
            .expect("public selector acquires original protected issuer control");
        let after = tracker
            .get(&start.ceremony_id)
            .await
            .expect("cancelled registration");
        assert_eq!(
            after.terminal_outcome,
            Some(aura_app::runtime_bridge::CeremonyTerminalOutcome::Failed(
                aura_app::runtime_bridge::CeremonyFailureReason::Cancelled,
            ),)
        );
        assert_eq!(after.started_at, before.started_at);
        assert_eq!(after.timeout, before.timeout);
        assert_eq!(
            after.timeout_budget.deadline_at_ms(),
            before.timeout_budget.deadline_at_ms()
        );
        assert!(
            issuer
                .runtime()
                .effects()
                .secure_exists(
                    &crate::runtime::effects::enrollment_generation_profile_location(
                        &issuer.authority_id(),
                        start.pending_epoch.value(),
                    ),
                )
                .await
                .expect("required pending signing profile"),
            "cancellation does not delete the signer before signed terminal notification"
        );
        issuer
            .invitations()
            .expect("original invitation owner")
            .cancel(&invitation.invitation_id)
            .await
            .expect("same genuine cancellation is idempotent");
    });
}

#[test]
fn runtime_enrollment_selector_rejects_foreign_runtime_before_terminal_mutation() {
    run_async_test_on_large_stack(async move {
        let (issuer, invitee, _invitation, start, _accept, _witness) =
            crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                "runtime-foreign-cancellation-selector",
            )
            .await;
        issuer
            .runtime()
            .tasks()
            .shutdown_with_timeout(std::time::Duration::from_secs(2))
            .await
            .expect("drain original execution owner");
        let before = issuer
            .ceremony_tracker()
            .await
            .get(&start.ceremony_id)
            .await
            .expect("original registration");
        let failure = invitee
            .invitations()
            .expect("foreign invitation service")
            .cancel_original_device_enrollment_ceremony(&start.ceremony_id)
            .await
            .expect_err("foreign runtime has no original protected issuer artifact");
        assert!(
            std::error::Error::source(&failure).is_some(),
            "required secure read failure retains its original source"
        );
        let after = issuer
            .ceremony_tracker()
            .await
            .get(&start.ceremony_id)
            .await
            .expect("original registration remains readable");
        assert_eq!(after.terminal_outcome, before.terminal_outcome);
        assert_eq!(
            after.timeout_budget.deadline_at_ms(),
            before.timeout_budget.deadline_at_ms()
        );
    });
}

#[test]
fn runtime_enrollment_closed_sink_retains_native_cause_after_cancelled_decision() {
    run_async_test_on_large_stack(async move {
        let (issuer, _invitee, _invitation, start, _accept, _witness) =
            crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                "runtime-closed-cancellation-sink",
            )
            .await;
        issuer
            .runtime()
            .tasks()
            .shutdown_with_timeout(std::time::Duration::from_secs(2))
            .await
            .expect("deliberately close the actual publication owner");
        let tracker = issuer.ceremony_tracker().await;
        let before = tracker
            .get(&start.ceremony_id)
            .await
            .expect("original window");
        let failure = AgentRuntimeBridge::new(issuer.clone())
            .cancel_key_rotation_ceremony(&start.ceremony_id)
            .await
            .expect_err("a closed publication sink cannot report full cancellation success");
        let mut current: Option<&(dyn std::error::Error + 'static)> = Some(&failure);
        let mut found = false;
        while let Some(error) = current {
            if matches!(
                error.downcast_ref::<crate::runtime::subsystems::journal::JournalSubsystemError>(),
                Some(crate::runtime::subsystems::journal::JournalSubsystemError::SinkClosed { .. })
            ) {
                found = true;
                break;
            }
            current = error.source();
        }
        assert!(
            found,
            "native bridge retains the actual journal sink cause end to end"
        );
        let after = tracker
            .get(&start.ceremony_id)
            .await
            .expect("durable terminal decision");
        assert_eq!(
            after.terminal_outcome,
            Some(aura_app::runtime_bridge::CeremonyTerminalOutcome::Failed(
                aura_app::runtime_bridge::CeremonyFailureReason::Cancelled,
            ),)
        );
        assert_eq!(
            after.timeout_budget.deadline_at_ms(),
            before.timeout_budget.deadline_at_ms()
        );
        assert_eq!(after.started_at, before.started_at);
        let repeated = AgentRuntimeBridge::new(issuer.clone())
            .cancel_key_rotation_ceremony(&start.ceremony_id)
            .await
            .expect_err("same closed owner still prevents required publication");
        assert!(std::error::Error::source(&repeated).is_some());
        assert_eq!(
            tracker
                .get(&start.ceremony_id)
                .await
                .expect("unchanged terminal")
                .terminal_outcome,
            after.terminal_outcome,
            "publication failure does not overwrite the first real decision"
        );
    });
}

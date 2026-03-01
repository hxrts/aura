//! LAN integration tests (UDP discovery + TCP envelope + sync roundtrip).

use async_lock::RwLock;
use aura_agent::{
    AgentBuilder, AgentConfig, AuraAgent, EffectContext, ExecutionMode, RendezvousManagerConfig,
    SyncManagerConfig,
};
use aura_app::ui::signals::CHAT_SIGNAL;
use aura_app::ui::workflows::{invitation as invitation_workflow, messaging as messaging_workflow};
use aura_app::{AppConfig, AppCore};
use aura_chat::{ChatFact, CHAT_FACT_TYPE_ID};
use aura_core::domain::journal::FactValue;
use aura_core::effects::{
    JournalEffects, ReactiveEffects, ThresholdSigningEffects, TransportEffects,
};
use aura_core::hash::hash;
use aura_core::identifiers::{AuthorityId, ContextId, DeviceId};
use aura_core::threshold::ParticipantIdentity;
use aura_journal::fact::{Fact, FactContent, RelationalFact};
use aura_journal::DomainFact;
use aura_rendezvous::LanDiscoveryConfig;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, timeout};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

static NEXT_LAN_PORT: AtomicU16 = AtomicU16::new(22000);

fn next_lan_port() -> u16 {
    NEXT_LAN_PORT.fetch_add(1, Ordering::Relaxed)
}

fn test_context(authority_id: AuthorityId) -> EffectContext {
    let context_entropy = hash(&authority_id.to_bytes());
    EffectContext::new(
        authority_id,
        ContextId::new_from_entropy(context_entropy),
        ExecutionMode::Testing,
    )
}

async fn bootstrap_agent(agent: &AuraAgent, authority_id: AuthorityId) -> TestResult {
    let effects = agent.runtime().effects();
    effects.bootstrap_authority(&authority_id).await?;
    let participants = vec![ParticipantIdentity::guardian(authority_id)];
    let (epoch, _, _) = effects
        .rotate_keys(&authority_id, 1, 1, &participants)
        .await?;
    effects.commit_key_rotation(&authority_id, epoch).await?;
    Ok(())
}

async fn create_lan_agent(seed: u8, lan_port: u16) -> TestResult<Arc<AuraAgent>> {
    let authority_id = AuthorityId::new_from_entropy([seed; 32]);
    let ctx = test_context(authority_id);

    let mut config = AgentConfig {
        device_id: DeviceId::from_uuid(authority_id.uuid()),
        ..Default::default()
    };
    config.network.bind_address = "0.0.0.0:0".to_string();
    config.lan_discovery = LanDiscoveryConfig {
        port: lan_port,
        announce_interval_ms: 200,
        enabled: true,
        bind_addr: "0.0.0.0".to_string(),
        broadcast_addr: "255.255.255.255".to_string(),
    };

    let rendezvous_config =
        RendezvousManagerConfig::default().with_lan_discovery(config.lan_discovery.clone());
    let sync_config = SyncManagerConfig::manual_only();

    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .with_config(config)
        .with_rendezvous_config(rendezvous_config)
        .with_sync_config(sync_config)
        .build_testing_async(&ctx)
        .await?;

    bootstrap_agent(&agent, authority_id).await?;
    agent.runtime().start_services().await?;

    Ok(Arc::new(agent))
}

async fn create_production_lan_agent_with_bind(
    seed: u8,
    lan_port: u16,
    bind_port: u16,
) -> TestResult<Arc<AuraAgent>> {
    let authority_id = AuthorityId::new_from_entropy([seed; 32]);
    let context_entropy = hash(&authority_id.to_bytes());
    let ctx = EffectContext::new(
        authority_id,
        ContextId::new_from_entropy(context_entropy),
        ExecutionMode::Production,
    );

    let temp_dir = std::env::temp_dir().join(format!(
        "aura-prod-lan-bind-test-{seed}-{lan_port}-{bind_port}"
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let _ = std::fs::create_dir_all(&temp_dir);

    let mut config = AgentConfig {
        device_id: DeviceId::from_uuid(authority_id.uuid()),
        ..Default::default()
    };
    config.network.bind_address = format!("127.0.0.1:{bind_port}");
    config.storage.base_path = temp_dir;
    config.lan_discovery = LanDiscoveryConfig {
        port: lan_port,
        announce_interval_ms: 200,
        enabled: true,
        bind_addr: "0.0.0.0".to_string(),
        broadcast_addr: "255.255.255.255".to_string(),
    };

    let rendezvous_config =
        RendezvousManagerConfig::default().with_lan_discovery(config.lan_discovery.clone());
    let sync_config = SyncManagerConfig::manual_only();

    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .with_config(config)
        .with_rendezvous_config(rendezvous_config)
        .with_sync_config(sync_config)
        .build_production(&ctx)
        .await?;

    bootstrap_agent(&agent, authority_id).await?;
    agent.runtime().start_services().await?;

    Ok(Arc::new(agent))
}

async fn create_runtime_app(agent: Arc<AuraAgent>) -> TestResult<Arc<RwLock<AppCore>>> {
    let mut app = AppCore::with_runtime(AppConfig::default(), agent.as_runtime_bridge())?;
    app.init_signals().await?;
    Ok(Arc::new(RwLock::new(app)))
}

async fn wait_for_lan_peer(agent: &AuraAgent, peer_id: AuthorityId) -> TestResult {
    let rendezvous = agent
        .runtime()
        .rendezvous()
        .ok_or("rendezvous service not enabled")?;

    timeout(Duration::from_secs(5), async {
        loop {
            let peers = rendezvous.list_lan_discovered_peers().await;
            if peers.iter().any(|peer| peer.authority_id == peer_id) {
                break;
            }
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .map_err(|_| "timed out waiting for LAN peer discovery".into())
}

async fn wait_for_envelope(
    effects: &Arc<aura_agent::AuraEffectSystem>,
) -> Result<aura_core::effects::transport::TransportEnvelope, aura_core::effects::TransportError> {
    timeout(Duration::from_secs(5), async {
        loop {
            match effects.receive_envelope().await {
                Ok(env) => return Ok(env),
                Err(aura_core::effects::TransportError::NoMessage) => {
                    sleep(Duration::from_millis(50)).await;
                }
                Err(err) => return Err(err),
            }
        }
    })
    .await
    .map_err(|_| aura_core::effects::TransportError::NoMessage)?
}

async fn wait_for_chat_fact(
    effects: &Arc<aura_agent::AuraEffectSystem>,
    authority_id: AuthorityId,
    channel_id: aura_core::identifiers::ChannelId,
) -> TestResult {
    timeout(Duration::from_secs(5), async {
        loop {
            let committed = effects
                .load_committed_facts(authority_id)
                .await
                .unwrap_or_default();
            let found = committed.into_iter().any(|fact| {
                let FactContent::Relational(RelationalFact::Generic { envelope, .. }) =
                    fact.content
                else {
                    return false;
                };
                if envelope.type_id.as_str() != CHAT_FACT_TYPE_ID {
                    return false;
                }
                matches!(
                    ChatFact::from_envelope(&envelope),
                    Some(ChatFact::ChannelCreated { channel_id: seen, .. }) if seen == channel_id
                )
            });

            if found {
                break;
            }

            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .map_err(|_| "timed out waiting for committed chat fact".into())
}

async fn wait_for_matching_chat_fact(
    effects: &Arc<aura_agent::AuraEffectSystem>,
    authority_id: AuthorityId,
    predicate: impl Fn(&ChatFact) -> bool,
) -> TestResult<Fact> {
    timeout(Duration::from_secs(8), async {
        loop {
            let committed = effects
                .load_committed_facts(authority_id)
                .await
                .unwrap_or_default();
            for fact in committed {
                let FactContent::Relational(RelationalFact::Generic { envelope, .. }) =
                    &fact.content
                else {
                    continue;
                };
                if envelope.type_id.as_str() != CHAT_FACT_TYPE_ID {
                    continue;
                }
                let Some(chat_fact) = ChatFact::from_envelope(envelope) else {
                    continue;
                };
                if predicate(&chat_fact) {
                    return fact;
                }
            }

            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .map_err(|_| "timed out waiting for matching chat fact".into())
}

async fn wait_for_chat_signal_message(
    app: &Arc<RwLock<AppCore>>,
    sender_id: AuthorityId,
    expected_content: &str,
) -> TestResult {
    timeout(Duration::from_secs(8), async {
        loop {
            let state: aura_app::views::ChatState = {
                let core = app.read().await;
                core.read(&*CHAT_SIGNAL).await.unwrap_or_default()
            };

            let mut found = false;
            for message in state.all_messages() {
                if message.sender_id == sender_id && message.content == expected_content {
                    found = true;
                    break;
                }
            }

            if found {
                break;
            }

            sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .map_err(|_| "timed out waiting for chat signal message".into())
}

#[tokio::test]
async fn test_lan_discovery_and_tcp_envelope() -> TestResult {
    let port = next_lan_port();
    let agent_a = create_lan_agent(1, port).await?;
    let agent_b = create_lan_agent(2, port).await?;

    wait_for_lan_peer(&agent_a, agent_b.authority_id()).await?;
    wait_for_lan_peer(&agent_b, agent_a.authority_id()).await?;

    let effects_a = agent_a.runtime().effects();
    let effects_b = agent_b.runtime().effects();

    let payload = b"lan-envelope-test".to_vec();
    let envelope = aura_core::effects::transport::TransportEnvelope {
        source: agent_a.authority_id(),
        destination: agent_b.authority_id(),
        context: ContextId::new_from_entropy(hash(&agent_b.authority_id().to_bytes())),
        payload: payload.clone(),
        metadata: std::collections::HashMap::new(),
        receipt: None,
    };

    effects_a.send_envelope(envelope).await?;
    let received = wait_for_envelope(&effects_b).await?;

    assert_eq!(received.payload, payload);
    assert_eq!(received.source, agent_a.authority_id());
    assert_eq!(received.destination, agent_b.authority_id());

    Ok(())
}

#[tokio::test]
async fn test_lan_chat_fact_ingress_commits_without_manual_inbox_poll() -> TestResult {
    let port = next_lan_port();
    let agent_a = create_lan_agent(9, port).await?;
    let agent_b = create_lan_agent(10, port).await?;

    wait_for_lan_peer(&agent_a, agent_b.authority_id()).await?;
    wait_for_lan_peer(&agent_b, agent_a.authority_id()).await?;

    let effects_a = agent_a.runtime().effects();
    let effects_b = agent_b.runtime().effects();

    let context_id = ContextId::new_from_entropy([42u8; 32]);
    let channel_id = aura_core::identifiers::ChannelId::from_bytes([43u8; 32]);
    let fact = ChatFact::channel_created_ms(
        context_id,
        channel_id,
        "dm".to_string(),
        Some("LAN ingress test".to_string()),
        true,
        1_700_000_000_000,
        agent_a.authority_id(),
    )
    .to_generic();

    let payload = aura_core::util::serialization::to_vec(&fact)?;
    let mut metadata = std::collections::HashMap::new();
    metadata.insert(
        "content-type".to_string(),
        "application/aura-chat-fact".to_string(),
    );

    let envelope = aura_core::effects::transport::TransportEnvelope {
        source: agent_a.authority_id(),
        destination: agent_b.authority_id(),
        context: context_id,
        payload,
        metadata,
        receipt: None,
    };

    effects_a.send_envelope(envelope).await?;
    wait_for_chat_fact(&effects_b, agent_b.authority_id(), channel_id).await?;

    Ok(())
}

#[tokio::test]
async fn test_lan_invitation_dm_message_e2e() -> TestResult {
    let discovery_port = next_lan_port();
    let bind_port_a = next_lan_port();
    let bind_port_b = next_lan_port();

    let agent_a = create_production_lan_agent_with_bind(51, discovery_port, bind_port_a).await?;
    let agent_b = create_production_lan_agent_with_bind(52, discovery_port, bind_port_b).await?;

    wait_for_lan_peer(&agent_a, agent_b.authority_id()).await?;
    wait_for_lan_peer(&agent_b, agent_a.authority_id()).await?;

    let app_a = create_runtime_app(agent_a.clone()).await?;
    let app_b = create_runtime_app(agent_b.clone()).await?;

    // Alice creates a contact invitation for Bob and exports the shareable code.
    let invite = invitation_workflow::create_contact_invitation(
        &app_a,
        agent_b.authority_id(),
        None,
        Some("e2e lan invite".to_string()),
        None,
    )
    .await?;
    let invite_code = invitation_workflow::export_invitation(&app_a, &invite.invitation_id).await?;

    // Bob imports and accepts the invitation code.
    invitation_workflow::import_invitation(&app_b, &invite_code).await?;
    invitation_workflow::accept_invitation(&app_b, &invite.invitation_id).await?;

    // Wait for Alice to receive Bob's acceptance and cache Bob's descriptor.
    let effects_a = agent_a.runtime().effects();
    timeout(Duration::from_secs(8), async {
        loop {
            if effects_a
                .is_channel_established(
                    aura_agent::core::default_context_id_for_authority(agent_b.authority_id()),
                    agent_b.authority_id(),
                )
                .await
            {
                break;
            }
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .map_err(|_| "timed out waiting for sender-side descriptor cache")?;

    // Both sides start the same deterministic DM channel.
    let dm_a = messaging_workflow::start_direct_chat(
        &app_a,
        &agent_b.authority_id().to_string(),
        1_700_000_000_001,
    )
    .await?;
    let dm_channel_id: aura_core::identifiers::ChannelId = dm_a
        .parse()
        .map_err(|_| "failed to parse dm channel id from start_direct_chat")?;

    // Ensure Bob has received the direct-channel creation fact before messaging.
    let effects_b = agent_b.runtime().effects();
    let channel_created_fact =
        wait_for_matching_chat_fact(&effects_b, agent_b.authority_id(), |fact| {
            matches!(
                fact,
                ChatFact::ChannelCreated {
                    channel_id,
                    creator_id,
                    ..
                } if *channel_id == dm_channel_id && *creator_id == agent_a.authority_id()
            )
        })
        .await?;
    let dm_context_id = match &channel_created_fact.content {
        FactContent::Relational(RelationalFact::Generic { envelope, .. }) => {
            match ChatFact::from_envelope(envelope) {
                Some(ChatFact::ChannelCreated { context_id, .. }) => context_id,
                _ => return Err("expected ChannelCreated chat fact".into()),
            }
        }
        _ => return Err("expected relational generic fact for channel create".into()),
    };

    let msg_text = "lan-e2e-message";
    let _message_id =
        messaging_workflow::send_message_by_name(&app_a, &dm_a, msg_text, 1_700_000_000_010)
            .await?;

    wait_for_matching_chat_fact(&effects_b, agent_b.authority_id(), |fact| {
        matches!(
            fact,
            ChatFact::MessageSentSealed {
                sender_id,
                channel_id: _,
                ..
            } if *sender_id == agent_a.authority_id()
        )
    })
    .await?;

    // Bob must observe a plaintext payload in CHAT_SIGNAL, not sealed fallback.
    wait_for_chat_signal_message(&app_b, agent_a.authority_id(), msg_text).await?;

    // Ensure Bob's chat state and AMP state agree on context/channel before reply send.
    let state_b: aura_app::views::ChatState = {
        let core = app_b.read().await;
        core.read(&*CHAT_SIGNAL).await.unwrap_or_default()
    };
    let Some(dm_channel_b) = state_b.channel(&dm_channel_id) else {
        return Err("recipient missing DM channel in CHAT_SIGNAL".into());
    };
    if dm_channel_b.context_id != Some(dm_context_id) {
        return Err("recipient DM channel context mismatch".into());
    }
    aura_protocol::amp::get_channel_state(&*effects_b, dm_context_id, dm_channel_id)
        .await
        .map_err(|e| format!("recipient AMP channel state missing before reply: {e}"))?;

    // Bob replies on the same channel provisioned by Alice's create flow.
    let reply_text = "lan-e2e-reply";
    let reply_result =
        messaging_workflow::send_message_by_name(&app_b, &dm_a, reply_text, 1_700_000_000_020)
            .await;
    if let Err(err) = reply_result {
        let post_send_state =
            aura_protocol::amp::get_channel_state(&*effects_b, dm_context_id, dm_channel_id).await;
        return Err(format!(
            "reply send failed; dm_context={dm_context_id} channel={dm_channel_id} \
             post_send_channel_state_present={} err={err}",
            post_send_state.is_ok()
        )
        .into());
    }

    wait_for_matching_chat_fact(&effects_a, agent_a.authority_id(), |fact| {
        matches!(
            fact,
            ChatFact::MessageSentSealed {
                sender_id,
                channel_id: _,
                ..
            } if *sender_id == agent_b.authority_id()
        )
    })
    .await?;
    wait_for_chat_signal_message(&app_a, agent_b.authority_id(), reply_text).await?;

    Ok(())
}

#[tokio::test]
async fn test_lan_group_channel_invitation_roundtrip_plaintext() -> TestResult {
    let discovery_port = next_lan_port();
    let bind_port_a = next_lan_port();
    let bind_port_b = next_lan_port();

    let agent_a = create_production_lan_agent_with_bind(53, discovery_port, bind_port_a).await?;
    let agent_b = create_production_lan_agent_with_bind(54, discovery_port, bind_port_b).await?;

    wait_for_lan_peer(&agent_a, agent_b.authority_id()).await?;
    wait_for_lan_peer(&agent_b, agent_a.authority_id()).await?;

    let app_a = create_runtime_app(agent_a.clone()).await?;
    let app_b = create_runtime_app(agent_b.clone()).await?;

    // Establish direct contact first.
    let invite = invitation_workflow::create_contact_invitation(
        &app_a,
        agent_b.authority_id(),
        None,
        Some("group channel bootstrap".to_string()),
        None,
    )
    .await?;
    let invite_code = invitation_workflow::export_invitation(&app_a, &invite.invitation_id).await?;
    invitation_workflow::import_invitation(&app_b, &invite_code).await?;
    invitation_workflow::accept_invitation(&app_b, &invite.invitation_id).await?;

    let effects_a = agent_a.runtime().effects();
    timeout(Duration::from_secs(8), async {
        loop {
            if effects_a
                .is_channel_established(
                    aura_agent::core::default_context_id_for_authority(agent_b.authority_id()),
                    agent_b.authority_id(),
                )
                .await
            {
                break;
            }
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .map_err(|_| "timed out waiting for sender-side descriptor cache")?;

    // Alice creates a group channel and invites Bob.
    let members = vec![agent_b.authority_id().to_string()];
    let channel_id = messaging_workflow::create_channel(
        &app_a,
        "lan-group",
        None,
        &members,
        1,
        1_700_000_100_000,
    )
    .await?;
    let effects_b = agent_b.runtime().effects();
    let channel_created_fact =
        wait_for_matching_chat_fact(&effects_b, agent_b.authority_id(), |fact| {
            matches!(
                fact,
                ChatFact::ChannelCreated {
                    channel_id: seen,
                    creator_id,
                    ..
                } if *seen == channel_id && *creator_id == agent_a.authority_id()
            )
        })
        .await?;
    let channel_context = match &channel_created_fact.content {
        FactContent::Relational(RelationalFact::Generic { envelope, .. }) => {
            match ChatFact::from_envelope(envelope) {
                Some(ChatFact::ChannelCreated { context_id, .. }) => context_id,
                _ => return Err("expected ChannelCreated chat fact".into()),
            }
        }
        _ => return Err("expected relational generic fact for channel create".into()),
    };

    aura_protocol::amp::get_channel_state(&*effects_b, channel_context, channel_id)
        .await
        .map_err(|e| format!("recipient AMP channel state missing before send: {e}"))?;

    let msg_text = "lan-group-a1";
    messaging_workflow::send_message(&app_a, channel_id, msg_text, 1_700_000_100_010).await?;
    wait_for_chat_signal_message(&app_b, agent_a.authority_id(), msg_text).await?;

    let reply_text = "lan-group-b1";
    let reply_result =
        messaging_workflow::send_message(&app_b, channel_id, reply_text, 1_700_000_100_020).await;
    if let Err(err) = reply_result {
        let post_send_state =
            aura_protocol::amp::get_channel_state(&*effects_b, channel_context, channel_id).await;
        return Err(format!(
            "group reply send failed; context={channel_context} channel={channel_id} \
             post_send_channel_state_present={} err={err}",
            post_send_state.is_ok()
        )
        .into());
    }
    wait_for_chat_signal_message(&app_a, agent_b.authority_id(), reply_text).await?;

    Ok(())
}

/// Create a LAN agent in **Production** execution mode.
///
/// Unlike `create_lan_agent` (Testing mode), this exercises the real guard
/// chain path. Biscuit tokens are created during `bootstrap_authority()` and
/// cached in `AuraEffectSystem`, so `publish_descriptor()` passes the guard
/// chain in production mode.
async fn create_production_lan_agent(seed: u8, lan_port: u16) -> TestResult<Arc<AuraAgent>> {
    let authority_id = AuthorityId::new_from_entropy([seed; 32]);
    let context_entropy = hash(&authority_id.to_bytes());
    let ctx = EffectContext::new(
        authority_id,
        ContextId::new_from_entropy(context_entropy),
        ExecutionMode::Production,
    );

    let temp_dir = std::env::temp_dir().join(format!("aura-prod-lan-test-{seed}-{lan_port}"));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let _ = std::fs::create_dir_all(&temp_dir);

    let mut config = AgentConfig {
        device_id: DeviceId::from_uuid(authority_id.uuid()),
        ..Default::default()
    };
    config.network.bind_address = "0.0.0.0:0".to_string();
    config.storage.base_path = temp_dir;
    config.lan_discovery = LanDiscoveryConfig {
        port: lan_port,
        announce_interval_ms: 200,
        enabled: true,
        bind_addr: "0.0.0.0".to_string(),
        broadcast_addr: "255.255.255.255".to_string(),
    };

    let rendezvous_config =
        RendezvousManagerConfig::default().with_lan_discovery(config.lan_discovery.clone());
    let sync_config = SyncManagerConfig::manual_only();

    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .with_config(config)
        .with_rendezvous_config(rendezvous_config)
        .with_sync_config(sync_config)
        .build_production(&ctx)
        .await?;

    bootstrap_agent(&agent, authority_id).await?;
    // Re-publish LAN descriptor now that signing keys and Biscuit tokens are bootstrapped
    agent.runtime().start_services().await?;

    Ok(Arc::new(agent))
}

/// Regression test: LAN discovery must work in Production execution mode.
///
/// Verifies that Biscuit tokens bootstrapped during account creation enable
/// the guard chain to authorize `publish_descriptor()` in production mode.
/// Without proper Biscuit bootstrap, the guard chain denies authorization
/// and the LAN announcer never receives a descriptor to broadcast.
#[tokio::test]
async fn test_production_lan_discovery() -> TestResult {
    let port = next_lan_port();
    let agent_a = create_production_lan_agent(20, port).await?;
    let agent_b = create_production_lan_agent(21, port).await?;

    // Both agents must discover each other. This would fail without
    // Biscuit token bootstrap because the guard chain denies authorization
    // for publish_descriptor() when no tokens are available.
    wait_for_lan_peer(&agent_a, agent_b.authority_id()).await?;
    wait_for_lan_peer(&agent_b, agent_a.authority_id()).await?;

    Ok(())
}

#[tokio::test]
async fn test_lan_sync_roundtrip() -> TestResult {
    let port = next_lan_port();
    let agent_a = create_lan_agent(3, port).await?;
    let agent_b = create_lan_agent(4, port).await?;

    wait_for_lan_peer(&agent_a, agent_b.authority_id()).await?;
    wait_for_lan_peer(&agent_b, agent_a.authority_id()).await?;

    let effects_a = agent_a.runtime().effects();
    let effects_b = agent_b.runtime().effects();

    let mut journal = effects_a.get_journal().await?;
    journal.facts.insert(
        "lan_sync_key",
        FactValue::String("lan_sync_value".to_string()),
    )?;
    effects_a.persist_journal(&journal).await?;

    let peer_device_id = DeviceId::from_uuid(agent_b.authority_id().uuid());
    let sync_a = agent_a.runtime().sync().ok_or("sync service not enabled")?;
    let peer_device_id_b = DeviceId::from_uuid(agent_a.authority_id().uuid());
    let sync_b = agent_b.runtime().sync().ok_or("sync service not enabled")?;

    sync_a.add_peer(peer_device_id).await;
    sync_b.add_peer(peer_device_id_b).await;

    let (res_a, res_b) = tokio::join!(
        sync_a.sync_with_peers(&*effects_a, vec![peer_device_id]),
        sync_b.sync_with_peers(&*effects_b, vec![peer_device_id_b]),
    );
    res_a?;
    res_b?;

    let health_a = sync_a.health().await.ok_or("missing sync health")?;
    let health_b = sync_b.health().await.ok_or("missing sync health")?;
    assert!(health_a.last_sync.is_some());
    assert!(health_b.last_sync.is_some());

    Ok(())
}

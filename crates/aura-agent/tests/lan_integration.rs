//! LAN integration tests (UDP discovery + TCP envelope + sync roundtrip).

use anyhow::anyhow;
use async_lock::RwLock;
use aura_agent::{
    AgentBuilder, AgentConfig, AuraAgent, EffectContext, ExecutionMode, RendezvousManagerConfig,
    SyncManagerConfig,
};
use aura_app::runtime_bridge::InvitationBridgeType;
use aura_app::ui::signals::{CHAT_SIGNAL, CONTACTS_SIGNAL};
use aura_app::ui::workflows::{
    context as context_workflow, invitation as invitation_workflow,
    messaging as messaging_workflow, strong_command as strong_command_workflow,
};
use aura_app::{AppConfig, AppCore};
use aura_chat::{ChatFact, CHAT_FACT_TYPE_ID};
use aura_core::domain::journal::FactValue;
use aura_core::effects::{
    JournalEffects, ReactiveEffects, ThresholdSigningEffects, TransportEffects,
};
use aura_core::hash::hash;
use aura_core::identifiers::{AuthorityId, ChannelId, ContextId, DeviceId};
use aura_core::threshold::ParticipantIdentity;
use aura_journal::fact::{Fact, FactContent, RelationalFact};
use aura_journal::DomainFact;
use aura_rendezvous::LanDiscoveryConfig;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, timeout};

type TestResult<T = ()> = anyhow::Result<T>;

static NEXT_LAN_PORT: AtomicU16 = AtomicU16::new(22000);
static LAN_TEST_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn next_lan_port() -> u16 {
    NEXT_LAN_PORT.fetch_add(1, Ordering::Relaxed)
}

async fn lock_lan_test() -> tokio::sync::MutexGuard<'static, ()> {
    LAN_TEST_MUTEX.lock().await
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
        .ok_or_else(|| anyhow!("rendezvous service not enabled"))?;

    timeout(Duration::from_secs(15), async {
        loop {
            let peers = rendezvous.list_lan_discovered_peers().await;
            if peers.iter().any(|peer| peer.authority_id == peer_id) {
                break;
            }
            sleep(Duration::from_millis(150)).await;
        }
    })
    .await
    .map_err(|_| anyhow!("timed out waiting for LAN peer discovery"))
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
    .map_err(|_| anyhow!("timed out waiting for committed chat fact"))
}

async fn wait_for_matching_chat_fact(
    effects: &Arc<aura_agent::AuraEffectSystem>,
    authority_id: AuthorityId,
    predicate: impl Fn(&ChatFact) -> bool,
) -> TestResult<Fact> {
    timeout(Duration::from_secs(12), async {
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
    .map_err(|_| anyhow!("timed out waiting for matching chat fact"))
}

async fn wait_for_chat_signal_message(
    app: &Arc<RwLock<AppCore>>,
    sender_id: AuthorityId,
    expected_content: &str,
) -> TestResult {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);

    loop {
        let state: aura_app::views::ChatState = {
            let core = app.read().await;
            core.read(&*CHAT_SIGNAL).await.unwrap_or_default()
        };

        let observed: Vec<String> = state
            .all_messages()
            .into_iter()
            .map(|message| {
                format!(
                    "sender={} content={}",
                    message.sender_id,
                    message.content.replace('\n', "\\n")
                )
            })
            .take(8)
            .collect();

        if state
            .all_messages()
            .into_iter()
            .any(|message| message.sender_id == sender_id && message.content == expected_content)
        {
            return Ok(());
        }

        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow!(
                "timed out waiting for chat signal message; expected sender={sender_id} content={expected_content}; observed=[{}]",
                observed.join(" | ")
            ));
        }

        sleep(Duration::from_millis(100)).await;
    }
}
async fn ensure_chat_signal_message_absent(
    app: &Arc<RwLock<AppCore>>,
    sender_id: AuthorityId,
    forbidden_content: &str,
    duration: Duration,
) -> TestResult {
    let deadline = tokio::time::Instant::now() + duration;
    loop {
        let state: aura_app::views::ChatState = {
            let core = app.read().await;
            core.read(&*CHAT_SIGNAL).await.unwrap_or_default()
        };

        if state
            .all_messages()
            .into_iter()
            .any(|message| message.sender_id == sender_id && message.content == forbidden_content)
        {
            return Err(anyhow!(
                "unexpected message delivery for muted sender: '{forbidden_content}'"
            ));
        }

        if tokio::time::Instant::now() >= deadline {
            return Ok(());
        }

        sleep(Duration::from_millis(50)).await;
    }
}

async fn wait_for_contact_signal(app: &Arc<RwLock<AppCore>>, target: AuthorityId) -> TestResult {
    timeout(Duration::from_secs(8), async {
        loop {
            let state: aura_app::views::ContactsState = {
                let core = app.read().await;
                core.read(&*CONTACTS_SIGNAL).await.unwrap_or_default()
            };

            if state.all_contacts().any(|contact| contact.id == target) {
                break;
            }

            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .map_err(|_| anyhow!("timed out waiting for contact signal"))
}

async fn wait_for_channel_signal(app: &Arc<RwLock<AppCore>>, channel_id: ChannelId) -> TestResult {
    timeout(Duration::from_secs(8), async {
        loop {
            let state: aura_app::views::ChatState = {
                let core = app.read().await;
                core.read(&*CHAT_SIGNAL).await.unwrap_or_default()
            };

            if state.channel(&channel_id).is_some() {
                break;
            }

            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .map_err(|_| anyhow!("timed out waiting for channel in chat signal"))
}

async fn accept_pending_channel_invitation(
    app: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
) -> TestResult<bool> {
    timeout(Duration::from_secs(4), async {
        loop {
            let pending = invitation_workflow::list_pending_invitations(app).await;
            if let Some(invitation) = pending.into_iter().find(|invitation| {
                matches!(
                    &invitation.invitation_type,
                    InvitationBridgeType::Channel { home_id, .. }
                        if home_id.parse::<ChannelId>().ok() == Some(channel_id)
                )
            }) {
                invitation_workflow::accept_invitation(app, &invitation.invitation_id).await?;
                return Ok(true);
            }

            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or(Ok(false))
}

async fn ensure_current_home(app: &Arc<RwLock<AppCore>>) -> TestResult {
    if context_workflow::current_home_context_or_fallback(app)
        .await
        .is_ok()
    {
        return Ok(());
    }

    let _ = context_workflow::create_home(app, Some("lan-home".to_string()), None).await?;
    Ok(())
}

async fn setup_lan_group_channel_pair(
    seed_a: u8,
    seed_b: u8,
    channel_name: &str,
) -> TestResult<(
    Arc<AuraAgent>,
    Arc<AuraAgent>,
    Arc<RwLock<AppCore>>,
    Arc<RwLock<AppCore>>,
    ChannelId,
)> {
    let discovery_port = next_lan_port();
    let bind_port_a = next_lan_port();
    let bind_port_b = next_lan_port();

    let agent_a =
        create_production_lan_agent_with_bind(seed_a, discovery_port, bind_port_a).await?;
    let agent_b =
        create_production_lan_agent_with_bind(seed_b, discovery_port, bind_port_b).await?;

    wait_for_lan_peer(&agent_a, agent_b.authority_id()).await?;
    wait_for_lan_peer(&agent_b, agent_a.authority_id()).await?;

    let app_a = create_runtime_app(agent_a.clone()).await?;
    let app_b = create_runtime_app(agent_b.clone()).await?;
    ensure_current_home(&app_a).await?;
    context_workflow::move_position(&app_a, "home", "partial").await?;
    context_workflow::move_position(&app_a, "home", "partial").await?;

    let invite = invitation_workflow::create_contact_invitation(
        &app_a,
        agent_b.authority_id(),
        None,
        Some("lan strong-command coverage".to_string()),
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
    .map_err(|_| anyhow!("timed out waiting for descriptor cache"))?;

    wait_for_contact_signal(&app_a, agent_b.authority_id()).await?;
    wait_for_contact_signal(&app_b, agent_a.authority_id()).await?;
    context_workflow::move_position(&app_a, "home", "partial").await?;

    let members = vec![agent_b.authority_id().to_string()];
    let channel_id = messaging_workflow::create_channel(
        &app_a,
        channel_name,
        None,
        &members,
        1,
        1_700_000_300_000,
    )
    .await?;

    let effects_b = agent_b.runtime().effects();
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
    let _accepted_channel_invite = accept_pending_channel_invitation(&app_b, channel_id).await?;
    messaging_workflow::join_channel(&app_a, channel_id).await?;
    messaging_workflow::join_channel(&app_b, channel_id).await?;

    wait_for_channel_signal(&app_a, channel_id).await?;
    wait_for_channel_signal(&app_b, channel_id).await?;

    Ok((agent_a, agent_b, app_a, app_b, channel_id))
}

async fn execute_strong_command(
    app: &Arc<RwLock<AppCore>>,
    actor: AuthorityId,
    channel_id: ChannelId,
    parsed: strong_command_workflow::ParsedCommand,
) -> TestResult<strong_command_workflow::CommandExecutionResult> {
    let resolver = strong_command_workflow::CommandResolver::default();
    let snapshot = resolver.capture_snapshot(app).await;
    let resolved = resolver
        .resolve(parsed, &snapshot)
        .map_err(|error| anyhow!("resolve failed: {error}"))?;
    let channel_hint = channel_id.to_string();
    let plan = resolver
        .plan(
            resolved,
            &snapshot,
            Some(channel_hint.as_str()),
            Some(actor),
        )
        .map_err(|error| anyhow!("plan failed: {error}"))?;
    strong_command_workflow::execute_planned(app, plan)
        .await
        .map_err(|error| anyhow!("execute failed: {error}"))
}

#[tokio::test]
async fn test_lan_discovery_and_tcp_envelope() -> TestResult {
    let _lan_lock = lock_lan_test().await;
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
    let _lan_lock = lock_lan_test().await;
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
    let _lan_lock = lock_lan_test().await;
    let discovery_port = next_lan_port();
    let bind_port_a = next_lan_port();
    let bind_port_b = next_lan_port();

    let agent_a = create_production_lan_agent_with_bind(51, discovery_port, bind_port_a).await?;
    let agent_b = create_production_lan_agent_with_bind(52, discovery_port, bind_port_b).await?;

    wait_for_lan_peer(&agent_a, agent_b.authority_id()).await?;
    wait_for_lan_peer(&agent_b, agent_a.authority_id()).await?;

    let app_a = create_runtime_app(agent_a.clone()).await?;
    let app_b = create_runtime_app(agent_b.clone()).await?;
    ensure_current_home(&app_a).await?;

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
    .map_err(|_| anyhow!("timed out waiting for sender-side descriptor cache"))?;

    // Both sides start the same deterministic DM channel.
    let dm_a = messaging_workflow::start_direct_chat(
        &app_a,
        &agent_b.authority_id().to_string(),
        1_700_000_000_001,
    )
    .await?;
    let dm_channel_id: aura_core::identifiers::ChannelId = dm_a
        .parse()
        .map_err(|_| anyhow!("failed to parse dm channel id from start_direct_chat"))?;

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
                _ => return Err(anyhow!("expected ChannelCreated chat fact")),
            }
        }
        _ => {
            return Err(anyhow!(
                "expected relational generic fact for channel create"
            ))
        }
    };

    // Ensure both sides are explicitly joined before message exchange so moderation
    // membership checks converge deterministically for both directions.
    messaging_workflow::join_channel(&app_a, dm_channel_id).await?;
    messaging_workflow::join_channel(&app_b, dm_channel_id).await?;
    wait_for_channel_signal(&app_a, dm_channel_id).await?;
    wait_for_channel_signal(&app_b, dm_channel_id).await?;

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
        return Err(anyhow!("recipient missing DM channel in CHAT_SIGNAL"));
    };
    if dm_channel_b.context_id != Some(dm_context_id) {
        return Err(anyhow!("recipient DM channel context mismatch"));
    }
    aura_protocol::amp::get_channel_state(&*effects_b, dm_context_id, dm_channel_id)
        .await
        .map_err(|e| anyhow!("recipient AMP channel state missing before reply: {e}"))?;

    // Bob replies on the same channel provisioned by Alice's create flow.
    let reply_text = "lan-e2e-reply";
    let reply_result =
        messaging_workflow::send_message_by_name(&app_b, &dm_a, reply_text, 1_700_000_000_020)
            .await;
    if let Err(err) = reply_result {
        let post_send_state =
            aura_protocol::amp::get_channel_state(&*effects_b, dm_context_id, dm_channel_id).await;
        return Err(anyhow!(
            "reply send failed; dm_context={dm_context_id} channel={dm_channel_id} \
             post_send_channel_state_present={} err={err}",
            post_send_state.is_ok()
        ));
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

    Ok(())
}

#[tokio::test]
async fn test_lan_invitation_dm_message_e2e_without_descriptor_wait() -> TestResult {
    let _lan_lock = lock_lan_test().await;
    let discovery_port = next_lan_port();
    let bind_port_a = next_lan_port();
    let bind_port_b = next_lan_port();

    let agent_a = create_production_lan_agent_with_bind(61, discovery_port, bind_port_a).await?;
    let agent_b = create_production_lan_agent_with_bind(62, discovery_port, bind_port_b).await?;

    wait_for_lan_peer(&agent_a, agent_b.authority_id()).await?;
    wait_for_lan_peer(&agent_b, agent_a.authority_id()).await?;

    let app_a = create_runtime_app(agent_a.clone()).await?;
    let app_b = create_runtime_app(agent_b.clone()).await?;
    let effects_a = agent_a.runtime().effects();
    ensure_current_home(&app_a).await?;
    context_workflow::move_position(&app_a, "home", "partial").await?;

    let invite = invitation_workflow::create_contact_invitation(
        &app_a,
        agent_b.authority_id(),
        None,
        Some("e2e lan invite without descriptor wait".to_string()),
        None,
    )
    .await?;
    let invite_code = invitation_workflow::export_invitation(&app_a, &invite.invitation_id).await?;

    invitation_workflow::import_invitation(&app_b, &invite_code).await?;
    invitation_workflow::accept_invitation(&app_b, &invite.invitation_id).await?;

    // Intentionally do not wait for sender-side descriptor cache.
    // The workflow should still bootstrap DM channel delivery robustly.
    let dm_name = messaging_workflow::start_direct_chat(
        &app_a,
        &agent_b.authority_id().to_string(),
        1_700_000_100_001,
    )
    .await?;
    let dm_channel_id: aura_core::identifiers::ChannelId = dm_name
        .parse()
        .map_err(|_| anyhow!("failed to parse dm channel id from start_direct_chat"))?;

    let effects_b = agent_b.runtime().effects();
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
    messaging_workflow::join_channel(&app_a, dm_channel_id).await?;
    messaging_workflow::join_channel(&app_b, dm_channel_id).await?;
    wait_for_channel_signal(&app_a, dm_channel_id).await?;
    wait_for_channel_signal(&app_b, dm_channel_id).await?;

    let msg_text = "lan-e2e-no-wait-message";
    let _message_id =
        messaging_workflow::send_message_by_name(&app_a, &dm_name, msg_text, 1_700_000_100_010)
            .await?;

    wait_for_chat_signal_message(&app_b, agent_a.authority_id(), msg_text).await?;

    let reply_text = "lan-e2e-no-wait-reply";
    let _reply_id =
        messaging_workflow::send_message_by_name(&app_b, &dm_name, reply_text, 1_700_000_100_020)
            .await?;

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

    Ok(())
}

#[tokio::test]
async fn test_lan_group_channel_invitation_roundtrip_plaintext() -> TestResult {
    let _lan_lock = lock_lan_test().await;
    let (agent_a, agent_b, app_a, app_b, channel_id) =
        setup_lan_group_channel_pair(53, 54, "lan-group").await?;
    let effects_a = agent_a.runtime().effects();
    let effects_b = agent_b.runtime().effects();

    let msg_text = "lan-group-a1";
    messaging_workflow::send_message(&app_a, channel_id, msg_text, 1_700_000_100_010).await?;
    wait_for_matching_chat_fact(&effects_b, agent_b.authority_id(), |fact| {
        matches!(
            fact,
            ChatFact::MessageSentSealed {
                sender_id,
                channel_id: seen_channel_id,
                ..
            } if *sender_id == agent_a.authority_id() && *seen_channel_id == channel_id
        )
    })
    .await?;
    wait_for_chat_signal_message(&app_b, agent_a.authority_id(), msg_text).await?;

    let reply_text = "lan-group-b1";
    messaging_workflow::send_message(&app_b, channel_id, reply_text, 1_700_000_100_020).await?;
    wait_for_matching_chat_fact(&effects_a, agent_a.authority_id(), |fact| {
        matches!(
            fact,
            ChatFact::MessageSentSealed {
                sender_id,
                channel_id: seen_channel_id,
                ..
            } if *sender_id == agent_b.authority_id() && *seen_channel_id == channel_id
        )
    })
    .await?;
    wait_for_chat_signal_message(&app_a, agent_b.authority_id(), reply_text).await?;

    Ok(())
}

#[tokio::test]
async fn test_lan_strong_command_mute_blocks_cross_instance_delivery_until_unmute() -> TestResult {
    let _lan_lock = lock_lan_test().await;
    let (agent_a, agent_b, app_a, app_b, channel_id) =
        setup_lan_group_channel_pair(70, 71, "lan-strong").await?;
    // Under the Member+Moderator model, moderation actions require moderator designation.
    let promote_self = execute_strong_command(
        &app_a,
        agent_a.authority_id(),
        channel_id,
        strong_command_workflow::ParsedCommand::Op {
            target: agent_a.authority_id().to_string(),
        },
    )
    .await?;
    assert_eq!(
        promote_self.consistency_state,
        strong_command_workflow::ConsistencyState::Enforced
    );

    let bob_target = {
        let contacts: aura_app::views::ContactsState = {
            let core = app_a.read().await;
            core.read(&*CONTACTS_SIGNAL).await.unwrap_or_default()
        };
        let Some(contact) = contacts.all_contacts().next() else {
            return Err(anyhow!(
                "expected at least one contact for strong-command target resolution"
            ));
        };
        contact.id.to_string()
    };

    let mute = execute_strong_command(
        &app_a,
        agent_a.authority_id(),
        channel_id,
        strong_command_workflow::ParsedCommand::Mute {
            target: bob_target.clone(),
            duration: Some(Duration::from_secs(300)),
        },
    )
    .await?;
    assert_eq!(
        mute.consistency_state,
        strong_command_workflow::ConsistencyState::Enforced
    );

    let muted_payload = "blocked-by-mute";
    let _ = messaging_workflow::send_message(&app_b, channel_id, muted_payload, 1_700_000_300_010)
        .await;
    ensure_chat_signal_message_absent(
        &app_a,
        agent_b.authority_id(),
        muted_payload,
        Duration::from_secs(2),
    )
    .await?;

    let unmute = execute_strong_command(
        &app_a,
        agent_a.authority_id(),
        channel_id,
        strong_command_workflow::ParsedCommand::Unmute { target: bob_target },
    )
    .await?;
    assert_eq!(
        unmute.consistency_state,
        strong_command_workflow::ConsistencyState::Enforced
    );

    let after = "after-unmute-cross-instance";
    messaging_workflow::send_message(&app_b, channel_id, after, 1_700_000_300_020).await?;
    wait_for_chat_signal_message(&app_a, agent_b.authority_id(), after).await?;

    Ok(())
}

#[tokio::test]
async fn test_lan_leave_then_join_reuses_channel_id_cross_instance() -> TestResult {
    let _lan_lock = lock_lan_test().await;
    let (_agent_a, agent_b, app_a, app_b, channel_id) =
        setup_lan_group_channel_pair(72, 73, "lan-rejoin").await?;

    messaging_workflow::leave_channel(&app_b, channel_id).await?;
    messaging_workflow::join_channel_by_name(&app_b, "#lan-rejoin").await?;
    wait_for_channel_signal(&app_b, channel_id).await?;

    let state_b: aura_app::views::ChatState = {
        let core = app_b.read().await;
        core.read(&*CHAT_SIGNAL).await.unwrap_or_default()
    };
    assert!(
        state_b.channel(&channel_id).is_some(),
        "leave/join must preserve canonical channel id visibility"
    );

    let message = "rejoin-channel-id-stable";
    messaging_workflow::send_message(&app_b, channel_id, message, 1_700_000_300_030).await?;
    wait_for_chat_signal_message(&app_a, agent_b.authority_id(), message).await?;

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
    let _lan_lock = lock_lan_test().await;
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
    let _lan_lock = lock_lan_test().await;
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
    let sync_a = agent_a
        .runtime()
        .sync()
        .ok_or_else(|| anyhow!("sync service not enabled"))?;
    let peer_device_id_b = DeviceId::from_uuid(agent_a.authority_id().uuid());
    let sync_b = agent_b
        .runtime()
        .sync()
        .ok_or_else(|| anyhow!("sync service not enabled"))?;

    sync_a.add_peer(peer_device_id).await;
    sync_b.add_peer(peer_device_id_b).await;

    let (res_a, res_b) = tokio::join!(
        sync_a.sync_with_peers(&*effects_a, vec![peer_device_id]),
        sync_b.sync_with_peers(&*effects_b, vec![peer_device_id_b]),
    );
    res_a.map_err(anyhow::Error::msg)?;
    res_b.map_err(anyhow::Error::msg)?;

    let health_a = sync_a
        .health()
        .await
        .ok_or_else(|| anyhow!("missing sync health"))?;
    let health_b = sync_b
        .health()
        .await
        .ok_or_else(|| anyhow!("missing sync health"))?;
    assert!(health_a.last_sync.is_some());
    assert!(health_b.last_sync.is_some());

    Ok(())
}

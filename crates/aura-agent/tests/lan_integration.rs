//! LAN integration tests (UDP discovery + TCP envelope + sync roundtrip).

use aura_agent::{
    AgentBuilder, AgentConfig, AuraAgent, EffectContext, ExecutionMode, RendezvousManagerConfig,
    SyncManagerConfig,
};
use aura_core::domain::journal::FactValue;
use aura_core::effects::{JournalEffects, ThresholdSigningEffects, TransportEffects};
use aura_core::hash::hash;
use aura_core::identifiers::{AuthorityId, ContextId, DeviceId};
use aura_core::threshold::ParticipantIdentity;
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

    let mut config = AgentConfig::default();
    config.device_id = DeviceId::from_uuid(authority_id.uuid());
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

/// Create a LAN agent in **Production** execution mode.
///
/// Unlike `create_lan_agent` (Testing mode), this exercises the real guard
/// chain path. Before the `publish_descriptor_local()` fix, this would fail
/// because the handler-level Biscuit guard denied descriptor publication
/// when Biscuit tokens weren't bootstrapped.
async fn create_production_lan_agent(seed: u8, lan_port: u16) -> TestResult<Arc<AuraAgent>> {
    let authority_id = AuthorityId::new_from_entropy([seed; 32]);
    let context_entropy = hash(&authority_id.to_bytes());
    let ctx = EffectContext::new(
        authority_id,
        ContextId::new_from_entropy(context_entropy),
        ExecutionMode::Production,
    );

    let temp_dir =
        std::env::temp_dir().join(format!("aura-prod-lan-test-{}-{}", seed, lan_port));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let _ = std::fs::create_dir_all(&temp_dir);

    let mut config = AgentConfig::default();
    config.device_id = DeviceId::from_uuid(authority_id.uuid());
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
    // Re-publish LAN descriptor now that keys are bootstrapped
    agent.runtime().start_services().await?;

    Ok(Arc::new(agent))
}

/// Regression test: LAN discovery must work in Production execution mode.
///
/// Before the `publish_descriptor_local()` fix, the handler-level Biscuit
/// guard in `publish_descriptor()` always denied authorization for fresh
/// production accounts (no Biscuit tokens configured). This meant the LAN
/// announcer never received a descriptor and never broadcast, so peer
/// discovery was completely broken in production.
#[tokio::test]
async fn test_production_lan_discovery() -> TestResult {
    let port = next_lan_port();
    let agent_a = create_production_lan_agent(20, port).await?;
    let agent_b = create_production_lan_agent(21, port).await?;

    // Both agents must discover each other. This would fail without the
    // publish_descriptor_local() bypass because the announcer had no
    // descriptor to broadcast in production mode.
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
    let sync_a = agent_a
        .runtime()
        .sync()
        .ok_or("sync service not enabled")?;
    let peer_device_id_b = DeviceId::from_uuid(agent_a.authority_id().uuid());
    let sync_b = agent_b
        .runtime()
        .sync()
        .ok_or("sync service not enabled")?;

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

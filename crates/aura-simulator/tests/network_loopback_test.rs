//! Network loopback tests for two-agent communication via shared transport.
//!
//! These tests verify that two agents can discover each other and exchange
//! messages over the simulator's shared transport infrastructure. This validates
//! the full Biscuit authorization flow end-to-end in simulation mode.

#![allow(clippy::expect_used, clippy::disallowed_methods)]

use aura_agent::{
    AgentBuilder, AgentConfig, AuraAgent, EffectContext, ExecutionMode, SharedTransport,
};
use aura_core::effects::transport::TransportEnvelope;
use aura_core::effects::{ThresholdSigningEffects, TransportEffects};
use aura_core::hash::hash;
use aura_core::identifiers::{AuthorityId, ContextId, DeviceId};
use aura_core::threshold::ParticipantIdentity;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, timeout};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn test_context(authority_id: AuthorityId, mode: ExecutionMode) -> EffectContext {
    let context_entropy = hash(&authority_id.to_bytes());
    EffectContext::new(
        authority_id,
        ContextId::new_from_entropy(context_entropy),
        mode,
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

async fn create_simulation_agent(
    seed: u8,
    shared_transport: SharedTransport,
) -> TestResult<Arc<AuraAgent>> {
    let authority_id = AuthorityId::new_from_entropy([seed; 32]);
    let ctx = test_context(
        authority_id,
        ExecutionMode::Simulation { seed: seed as u64 },
    );

    let config = AgentConfig {
        device_id: DeviceId::from_uuid(authority_id.uuid()),
        ..AgentConfig::default()
    };

    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .with_config(config)
        .build_simulation_async_with_shared_transport(seed as u64, &ctx, shared_transport)
        .await?;

    bootstrap_agent(&agent, authority_id).await?;

    Ok(Arc::new(agent))
}

/// Wait for an envelope to arrive in the agent's inbox.
async fn wait_for_envelope(
    effects: &Arc<aura_agent::AuraEffectSystem>,
    timeout_secs: u64,
) -> Result<TransportEnvelope, aura_core::effects::TransportError> {
    timeout(Duration::from_secs(timeout_secs), async {
        loop {
            match effects.receive_envelope().await {
                Ok(env) => return Ok(env),
                Err(aura_core::effects::TransportError::NoMessage) => {
                    sleep(Duration::from_millis(10)).await;
                }
                Err(err) => return Err(err),
            }
        }
    })
    .await
    .map_err(|_| aura_core::effects::TransportError::NoMessage)?
}

/// Test that two agents can communicate via shared transport in simulation mode.
///
/// This verifies that:
/// 1. Agents can be created with shared transport
/// 2. Biscuit tokens are properly bootstrapped in simulation mode
/// 3. Envelopes route correctly between agents
#[tokio::test]
async fn test_two_agent_loopback_communication() -> TestResult {
    // Create shared transport that both agents will use
    let shared_transport = SharedTransport::new();

    // Create two agents on the shared transport (they auto-register during construction)
    let agent_a = create_simulation_agent(10, shared_transport.clone()).await?;
    let agent_b = create_simulation_agent(20, shared_transport.clone()).await?;

    let effects_a = agent_a.runtime().effects();
    let effects_b = agent_b.runtime().effects();

    // Verify both agents are visible in the shared transport
    let online_peers = shared_transport.online_peers();
    assert!(online_peers.contains(&agent_a.authority_id()));
    assert!(online_peers.contains(&agent_b.authority_id()));

    // Agent A sends envelope to Agent B
    let payload = b"hello from agent A".to_vec();
    let envelope = TransportEnvelope {
        source: agent_a.authority_id(),
        destination: agent_b.authority_id(),
        context: ContextId::new_from_entropy(hash(&agent_b.authority_id().to_bytes())),
        payload: payload.clone(),
        metadata: {
            let mut m = HashMap::new();
            m.insert("test".to_string(), "loopback".to_string());
            m
        },
        receipt: None,
    };

    effects_a.send_envelope(envelope).await?;

    // Agent B receives the envelope
    let received = wait_for_envelope(&effects_b, 5).await?;

    assert_eq!(received.payload, payload);
    assert_eq!(received.source, agent_a.authority_id());
    assert_eq!(received.destination, agent_b.authority_id());
    assert_eq!(received.metadata.get("test"), Some(&"loopback".to_string()));

    Ok(())
}

/// Test bidirectional communication between two agents.
#[tokio::test]
async fn test_bidirectional_agent_communication() -> TestResult {
    let shared_transport = SharedTransport::new();

    let agent_a = create_simulation_agent(30, shared_transport.clone()).await?;
    let agent_b = create_simulation_agent(40, shared_transport.clone()).await?;

    // Agents auto-register during construction

    let effects_a = agent_a.runtime().effects();
    let effects_b = agent_b.runtime().effects();

    // A -> B
    let payload_ab = b"message A to B".to_vec();
    let envelope_ab = TransportEnvelope {
        source: agent_a.authority_id(),
        destination: agent_b.authority_id(),
        context: ContextId::new_from_entropy(hash(&[1u8; 32])),
        payload: payload_ab.clone(),
        metadata: HashMap::new(),
        receipt: None,
    };
    effects_a.send_envelope(envelope_ab).await?;

    // B -> A
    let payload_ba = b"message B to A".to_vec();
    let envelope_ba = TransportEnvelope {
        source: agent_b.authority_id(),
        destination: agent_a.authority_id(),
        context: ContextId::new_from_entropy(hash(&[2u8; 32])),
        payload: payload_ba.clone(),
        metadata: HashMap::new(),
        receipt: None,
    };
    effects_b.send_envelope(envelope_ba).await?;

    // Verify both received correctly
    let received_at_b = wait_for_envelope(&effects_b, 5).await?;
    assert_eq!(received_at_b.payload, payload_ab);
    assert_eq!(received_at_b.source, agent_a.authority_id());

    let received_at_a = wait_for_envelope(&effects_a, 5).await?;
    assert_eq!(received_at_a.payload, payload_ba);
    assert_eq!(received_at_a.source, agent_b.authority_id());

    Ok(())
}

/// Test multiple sequential messages between agents.
#[tokio::test]
async fn test_sequential_message_exchange() -> TestResult {
    let shared_transport = SharedTransport::new();

    let agent_a = create_simulation_agent(50, shared_transport.clone()).await?;
    let agent_b = create_simulation_agent(60, shared_transport.clone()).await?;

    // Agents auto-register during construction

    let effects_a = agent_a.runtime().effects();
    let effects_b = agent_b.runtime().effects();

    // Send multiple messages from A to B
    for i in 0..5 {
        let payload = format!("message {i}").into_bytes();
        let envelope = TransportEnvelope {
            source: agent_a.authority_id(),
            destination: agent_b.authority_id(),
            context: ContextId::new_from_entropy(hash(&[i as u8; 32])),
            payload,
            metadata: HashMap::new(),
            receipt: None,
        };
        effects_a.send_envelope(envelope).await?;
    }

    // B should receive all messages in order (FIFO)
    for i in 0..5 {
        let received = wait_for_envelope(&effects_b, 5).await?;
        let expected_payload = format!("message {i}").into_bytes();
        assert_eq!(received.payload, expected_payload, "message {i} mismatch");
    }

    Ok(())
}

/// Test that agents can query connected peer count.
///
/// Note: Agents are auto-registered with the shared transport during construction.
#[tokio::test]
async fn test_shared_transport_peer_awareness() -> TestResult {
    let shared_transport = SharedTransport::new();

    // Create agent A - it auto-registers during construction
    let agent_a = create_simulation_agent(70, shared_transport.clone()).await?;

    // After A is created, it should see 0 other peers (only itself is registered)
    assert_eq!(
        shared_transport.connected_peer_count(agent_a.authority_id()),
        0
    );
    assert!(shared_transport.is_peer_online(agent_a.authority_id()));

    // Create agent B - it also auto-registers during construction
    let agent_b = create_simulation_agent(80, shared_transport.clone()).await?;

    // Now both agents see each other as peers
    assert_eq!(
        shared_transport.connected_peer_count(agent_a.authority_id()),
        1
    );
    assert_eq!(
        shared_transport.connected_peer_count(agent_b.authority_id()),
        1
    );

    // Verify peer visibility
    assert!(shared_transport.is_peer_online(agent_a.authority_id()));
    assert!(shared_transport.is_peer_online(agent_b.authority_id()));

    // Verify online peers list
    let online = shared_transport.online_peers();
    assert_eq!(online.len(), 2);
    assert!(online.contains(&agent_a.authority_id()));
    assert!(online.contains(&agent_b.authority_id()));

    Ok(())
}

/// Test three-agent communication topology.
#[tokio::test]
async fn test_three_agent_mesh_communication() -> TestResult {
    let shared_transport = SharedTransport::new();

    let agent_a = create_simulation_agent(90, shared_transport.clone()).await?;
    let agent_b = create_simulation_agent(91, shared_transport.clone()).await?;
    let agent_c = create_simulation_agent(92, shared_transport.clone()).await?;

    // Agents auto-register during construction

    let effects_a = agent_a.runtime().effects();
    let effects_b = agent_b.runtime().effects();
    let effects_c = agent_c.runtime().effects();

    // A -> B
    effects_a
        .send_envelope(TransportEnvelope {
            source: agent_a.authority_id(),
            destination: agent_b.authority_id(),
            context: ContextId::new_from_entropy(hash(&[1u8; 32])),
            payload: b"A to B".to_vec(),
            metadata: HashMap::new(),
            receipt: None,
        })
        .await?;

    // B -> C
    effects_b
        .send_envelope(TransportEnvelope {
            source: agent_b.authority_id(),
            destination: agent_c.authority_id(),
            context: ContextId::new_from_entropy(hash(&[2u8; 32])),
            payload: b"B to C".to_vec(),
            metadata: HashMap::new(),
            receipt: None,
        })
        .await?;

    // C -> A
    effects_c
        .send_envelope(TransportEnvelope {
            source: agent_c.authority_id(),
            destination: agent_a.authority_id(),
            context: ContextId::new_from_entropy(hash(&[3u8; 32])),
            payload: b"C to A".to_vec(),
            metadata: HashMap::new(),
            receipt: None,
        })
        .await?;

    // Verify all messages received
    let recv_b = wait_for_envelope(&effects_b, 5).await?;
    assert_eq!(recv_b.payload, b"A to B");

    let recv_c = wait_for_envelope(&effects_c, 5).await?;
    assert_eq!(recv_c.payload, b"B to C");

    let recv_a = wait_for_envelope(&effects_a, 5).await?;
    assert_eq!(recv_a.payload, b"C to A");

    // Verify 2 peers visible to each agent
    assert_eq!(
        shared_transport.connected_peer_count(agent_a.authority_id()),
        2
    );
    assert_eq!(
        shared_transport.connected_peer_count(agent_b.authority_id()),
        2
    );
    assert_eq!(
        shared_transport.connected_peer_count(agent_c.authority_id()),
        2
    );

    Ok(())
}

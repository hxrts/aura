use aura_agent::runtime::choreography_adapter::AuraProtocolAdapter;
use aura_consensus::messages::ConsensusMessage;
use aura_consensus::protocol::runners::{execute_as, AuraConsensusRole};
use aura_consensus::types::{CommitFact, ConsensusId};
use aura_core::crypto::tree_signing::{NonceCommitment, PartialSignature, MAX_COMMITMENT_BYTES};
use aura_core::epochs::Epoch;
use aura_core::frost::ThresholdSignature;
use aura_core::time::{PhysicalTime, ProvenancedTime, TimeStamp};
use aura_core::{AuthorityId, DeviceId, Hash32};
use aura_simulator::{SimulatedMessageBus, SimulatedTransport};
use aura_testkit::ProtocolTest;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

fn dummy_nonce_commitment(signer: u16) -> NonceCommitment {
    NonceCommitment {
        signer,
        commitment: vec![0u8; MAX_COMMITMENT_BYTES],
    }
}

fn dummy_partial_signature(signer: u16) -> PartialSignature {
    PartialSignature {
        signer,
        signature: vec![0u8; 32],
    }
}

fn dummy_commit_fact(
    consensus_id: ConsensusId,
    prestate_hash: Hash32,
    operation_hash: Hash32,
    participants: Vec<AuthorityId>,
    threshold: u16,
) -> CommitFact {
    let timestamp = ProvenancedTime {
        stamp: TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 0,
            uncertainty: None,
        }),
        proofs: Vec::new(),
        origin: None,
    };
    CommitFact::new(
        consensus_id,
        prestate_hash,
        operation_hash,
        vec![1, 2, 3],
        ThresholdSignature::new(vec![0u8; 64], vec![1, 2]),
        None,
        participants,
        threshold,
        false,
        timestamp,
    )
}

#[tokio::test]
async fn consensus_choreography_executes_with_simulated_transport() {
    let coordinator_device = DeviceId::from_uuid(Uuid::new_v4());
    let witness_a = DeviceId::from_uuid(Uuid::new_v4());
    let witness_b = DeviceId::from_uuid(Uuid::new_v4());

    let test = ProtocolTest::new("AuraConsensus")
        .bind_role("Coordinator", coordinator_device)
        .bind_roles("Witness", &[witness_a, witness_b])
        .expect_success();
    let _harness = test.build_harness().expect("protocol harness should build");

    let coordinator_auth = AuthorityId::from_uuid(coordinator_device.uuid());
    let witness_auths = vec![
        AuthorityId::from_uuid(witness_a.uuid()),
        AuthorityId::from_uuid(witness_b.uuid()),
    ];

    let prestate_hash = Hash32::default();
    let operation_hash = Hash32([1u8; 32]);
    let consensus_id = ConsensusId::new(prestate_hash, operation_hash, 1);

    let mut role_map = HashMap::new();
    role_map.insert(AuraConsensusRole::Coordinator, coordinator_auth);
    role_map.insert(AuraConsensusRole::Witness(0), witness_auths[0]);
    role_map.insert(AuraConsensusRole::Witness(1), witness_auths[1]);

    let witness_roles = vec![AuraConsensusRole::Witness(0), AuraConsensusRole::Witness(1)];

    let bus = Arc::new(SimulatedMessageBus::new());
    let session_id = Uuid::new_v4();

    let coordinator_transport = SimulatedTransport::new(
        bus.clone(),
        coordinator_device,
        AuraConsensusRole::Coordinator.role_index().unwrap_or(0),
    )
    .expect("coordinator transport");
    let witness_a_transport = SimulatedTransport::new(
        bus.clone(),
        witness_a,
        AuraConsensusRole::Witness(0).role_index().unwrap_or(0),
    )
    .expect("witness transport");
    let witness_b_transport = SimulatedTransport::new(
        bus.clone(),
        witness_b,
        AuraConsensusRole::Witness(1).role_index().unwrap_or(0),
    )
    .expect("witness transport");

    let coordinator_queue = {
        let mut queue = std::collections::VecDeque::new();
        for _ in &witness_roles {
            queue.push_back(ConsensusMessage::Execute {
                consensus_id,
                prestate_hash,
                operation_hash,
                operation_bytes: vec![1, 2, 3],
                cached_commitments: None,
            });
        }
        for _ in &witness_roles {
            queue.push_back(ConsensusMessage::SignRequest {
                consensus_id,
                aggregated_nonces: vec![dummy_nonce_commitment(1), dummy_nonce_commitment(2)],
            });
        }
        for _ in &witness_roles {
            queue.push_back(ConsensusMessage::ConsensusResult {
                commit_fact: dummy_commit_fact(
                    consensus_id,
                    prestate_hash,
                    operation_hash,
                    witness_auths.clone(),
                    2,
                ),
            });
        }
        queue
    };

    let witness_queue = std::collections::VecDeque::from([
        ConsensusMessage::NonceCommit {
            consensus_id,
            commitment: dummy_nonce_commitment(1),
        },
        ConsensusMessage::SignShare {
            consensus_id,
            share: dummy_partial_signature(1),
            next_commitment: None,
            epoch: Epoch::new(0),
        },
    ]);

    let coordinator_task = {
        let mut outbound = coordinator_queue;
        let mut adapter = AuraProtocolAdapter::new(
            Arc::new(coordinator_transport),
            coordinator_auth,
            AuraConsensusRole::Coordinator,
            role_map.clone(),
        )
        .with_role_family("Witness", witness_roles.clone())
        .with_message_provider(move |_req, _received| {
            outbound
                .pop_front()
                .map(|msg| Box::new(msg) as Box<dyn std::any::Any + Send>)
        });
        async move {
            adapter.start_session(session_id).await?;
            let result = execute_as(AuraConsensusRole::Coordinator, &mut adapter).await;
            let _ = adapter.end_session().await;
            result
        }
    };

    let witness_task = |role: AuraConsensusRole, authority: AuthorityId, transport: SimulatedTransport| {
        let mut outbound = witness_queue.clone();
        let mut adapter =
            AuraProtocolAdapter::new(Arc::new(transport), authority, role, role_map.clone())
                .with_role_family("Witness", witness_roles.clone())
                .with_message_provider(move |_req, _received| {
                    outbound
                        .pop_front()
                        .map(|msg| Box::new(msg) as Box<dyn std::any::Any + Send>)
                });
        async move {
            adapter.start_session(session_id).await?;
            let result = execute_as(role, &mut adapter).await;
            let _ = adapter.end_session().await;
            result
        }
    };

    let (coord_res, witness_a_res, witness_b_res) = tokio::join!(
        coordinator_task,
        witness_task(AuraConsensusRole::Witness(0), witness_auths[0], witness_a_transport),
        witness_task(AuraConsensusRole::Witness(1), witness_auths[1], witness_b_transport)
    );

    coord_res.expect("coordinator execution failed");
    witness_a_res.expect("witness 0 execution failed");
    witness_b_res.expect("witness 1 execution failed");
}

//! Golden baseline traces for core choreographies before Telltale migration.
//!
//! These traces intentionally capture deterministic session lifecycle behavior:
//! role bindings, role-family mappings, and start/end ordering for a fixed
//! session id and fixed authority/device assignments.
//!
//! Update fixtures only when behavior changes are intentional:
//! `AURA_UPDATE_BASELINES=1 cargo test -p aura-simulator choreography_baselines_match_golden`

#![allow(clippy::expect_used, clippy::disallowed_methods)]

use aura_agent::AuraProtocolAdapter;
use aura_consensus::protocol::runners::AuraConsensusRole;
use aura_core::{AuthorityId, DeviceId};
use aura_invitation::protocol::exchange_runners::InvitationExchangeRole;
use aura_mpst::upstream::runtime::RoleId;
use aura_recovery::recovery_runners::RecoveryProtocolRole;
use aura_rendezvous::protocol::exchange_runners::RendezvousExchangeRole;
use aura_simulator::{SimulatedMessageBus, TestEffectSystem};
use aura_sync::protocols::epoch_runners::EpochRotationProtocolRole;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct RoleBindingBaseline {
    role: String,
    authority: String,
    role_index: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct LifecycleEvent {
    order: usize,
    role: String,
    action: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct ProtocolBaseline {
    protocol: String,
    session_id: String,
    role_bindings: Vec<RoleBindingBaseline>,
    role_families: BTreeMap<String, Vec<String>>,
    lifecycle: Vec<LifecycleEvent>,
}

fn baseline_path(protocol: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("baselines")
        .join(format!("{protocol}.json"))
}

fn authority_label(authority: AuthorityId) -> String {
    authority.to_string()
}

fn authority(seed: u8) -> AuthorityId {
    AuthorityId::from_uuid(Uuid::from_bytes([seed; 16]))
}

fn sorted_role_bindings(mut bindings: Vec<RoleBindingBaseline>) -> Vec<RoleBindingBaseline> {
    bindings.sort_by(|a, b| a.role.cmp(&b.role));
    bindings
}

fn push_event(lifecycle: &mut Vec<LifecycleEvent>, role: &str, action: &str) {
    lifecycle.push(LifecycleEvent {
        order: lifecycle.len(),
        role: role.to_string(),
        action: action.to_string(),
    });
}

fn assert_or_update(baseline: &ProtocolBaseline) {
    let path = baseline_path(&baseline.protocol);
    let serialized = serde_json::to_string_pretty(baseline).expect("serialize baseline") + "\n";

    if std::env::var_os("AURA_UPDATE_BASELINES").is_some() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create baseline dir");
        }
        fs::write(&path, &serialized).expect("write baseline file");
    }

    let expected = fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing baseline file {} (set AURA_UPDATE_BASELINES=1 and rerun test)",
            path.display()
        )
    });

    assert_eq!(
        expected, serialized,
        "baseline drift for {} (update only if intentional)",
        baseline.protocol
    );
}

async fn consensus_baseline() -> ProtocolBaseline {
    let coordinator_device = DeviceId::from_uuid(Uuid::from_bytes([11; 16]));
    let witness_a = DeviceId::from_uuid(Uuid::from_bytes([12; 16]));
    let witness_b = DeviceId::from_uuid(Uuid::from_bytes([13; 16]));

    let coordinator_auth = authority(0x61);
    let witness_auth_a = authority(0x62);
    let witness_auth_b = authority(0x63);

    let mut role_map = HashMap::new();
    role_map.insert(AuraConsensusRole::Coordinator, coordinator_auth);
    role_map.insert(AuraConsensusRole::Witness(0), witness_auth_a);
    role_map.insert(AuraConsensusRole::Witness(1), witness_auth_b);

    let mut role_families = BTreeMap::new();
    role_families.insert(
        "Witness".to_string(),
        vec!["Witness[0]".to_string(), "Witness[1]".to_string()],
    );
    let witness_roles = vec![AuraConsensusRole::Witness(0), AuraConsensusRole::Witness(1)];

    let bus = Arc::new(SimulatedMessageBus::new());
    let session_id = Uuid::from_bytes([14; 16]);

    let coordinator_effects = TestEffectSystem::new(
        bus.clone(),
        coordinator_device,
        AuraConsensusRole::Coordinator.role_index().unwrap_or(0),
    )
    .expect("coordinator effects");
    let witness_a_effects = TestEffectSystem::new(
        bus.clone(),
        witness_a,
        AuraConsensusRole::Witness(0).role_index().unwrap_or(0),
    )
    .expect("witness a effects");
    let witness_b_effects = TestEffectSystem::new(
        bus,
        witness_b,
        AuraConsensusRole::Witness(1).role_index().unwrap_or(1),
    )
    .expect("witness b effects");

    let mut coordinator_adapter = AuraProtocolAdapter::new(
        Arc::new(coordinator_effects),
        coordinator_auth,
        AuraConsensusRole::Coordinator,
        role_map.clone(),
    )
    .with_role_family("Witness", witness_roles.clone());
    let mut witness_a_adapter = AuraProtocolAdapter::new(
        Arc::new(witness_a_effects),
        witness_auth_a,
        AuraConsensusRole::Witness(0),
        role_map.clone(),
    )
    .with_role_family("Witness", witness_roles.clone());
    let mut witness_b_adapter = AuraProtocolAdapter::new(
        Arc::new(witness_b_effects),
        witness_auth_b,
        AuraConsensusRole::Witness(1),
        role_map,
    )
    .with_role_family("Witness", witness_roles);

    let mut lifecycle = Vec::new();
    coordinator_adapter
        .start_session(session_id)
        .await
        .expect("coordinator start");
    push_event(&mut lifecycle, "Coordinator", "start");
    witness_a_adapter
        .start_session(session_id)
        .await
        .expect("witness[0] start");
    push_event(&mut lifecycle, "Witness[0]", "start");
    witness_b_adapter
        .start_session(session_id)
        .await
        .expect("witness[1] start");
    push_event(&mut lifecycle, "Witness[1]", "start");

    coordinator_adapter
        .end_session()
        .await
        .expect("coordinator end");
    push_event(&mut lifecycle, "Coordinator", "end");
    witness_a_adapter
        .end_session()
        .await
        .expect("witness[0] end");
    push_event(&mut lifecycle, "Witness[0]", "end");
    witness_b_adapter
        .end_session()
        .await
        .expect("witness[1] end");
    push_event(&mut lifecycle, "Witness[1]", "end");

    ProtocolBaseline {
        protocol: "consensus".to_string(),
        session_id: session_id.to_string(),
        role_bindings: sorted_role_bindings(vec![
            RoleBindingBaseline {
                role: "Coordinator".to_string(),
                authority: authority_label(coordinator_auth),
                role_index: AuraConsensusRole::Coordinator.role_index().unwrap_or(0),
            },
            RoleBindingBaseline {
                role: "Witness[0]".to_string(),
                authority: authority_label(witness_auth_a),
                role_index: AuraConsensusRole::Witness(0).role_index().unwrap_or(0),
            },
            RoleBindingBaseline {
                role: "Witness[1]".to_string(),
                authority: authority_label(witness_auth_b),
                role_index: AuraConsensusRole::Witness(1).role_index().unwrap_or(1),
            },
        ]),
        role_families,
        lifecycle,
    }
}

async fn invitation_baseline() -> ProtocolBaseline {
    let sender_device = DeviceId::from_uuid(Uuid::from_bytes([21; 16]));
    let receiver_device = DeviceId::from_uuid(Uuid::from_bytes([22; 16]));
    let sender_auth = authority(0x71);
    let receiver_auth = authority(0x72);

    let mut role_map = HashMap::new();
    role_map.insert(InvitationExchangeRole::Sender, sender_auth);
    role_map.insert(InvitationExchangeRole::Receiver, receiver_auth);

    let bus = Arc::new(SimulatedMessageBus::new());
    let session_id = Uuid::from_bytes([23; 16]);

    let sender_effects = TestEffectSystem::new(
        bus.clone(),
        sender_device,
        InvitationExchangeRole::Sender.role_index().unwrap_or(0),
    )
    .expect("sender effects");
    let receiver_effects = TestEffectSystem::new(
        bus,
        receiver_device,
        InvitationExchangeRole::Receiver.role_index().unwrap_or(1),
    )
    .expect("receiver effects");

    let mut sender_adapter = AuraProtocolAdapter::new(
        Arc::new(sender_effects),
        sender_auth,
        InvitationExchangeRole::Sender,
        role_map.clone(),
    );
    let mut receiver_adapter = AuraProtocolAdapter::new(
        Arc::new(receiver_effects),
        receiver_auth,
        InvitationExchangeRole::Receiver,
        role_map,
    );

    let mut lifecycle = Vec::new();
    sender_adapter
        .start_session(session_id)
        .await
        .expect("sender start");
    push_event(&mut lifecycle, "Sender", "start");
    receiver_adapter
        .start_session(session_id)
        .await
        .expect("receiver start");
    push_event(&mut lifecycle, "Receiver", "start");

    sender_adapter.end_session().await.expect("sender end");
    push_event(&mut lifecycle, "Sender", "end");
    receiver_adapter.end_session().await.expect("receiver end");
    push_event(&mut lifecycle, "Receiver", "end");

    ProtocolBaseline {
        protocol: "invitation".to_string(),
        session_id: session_id.to_string(),
        role_bindings: sorted_role_bindings(vec![
            RoleBindingBaseline {
                role: "Sender".to_string(),
                authority: authority_label(sender_auth),
                role_index: InvitationExchangeRole::Sender.role_index().unwrap_or(0),
            },
            RoleBindingBaseline {
                role: "Receiver".to_string(),
                authority: authority_label(receiver_auth),
                role_index: InvitationExchangeRole::Receiver.role_index().unwrap_or(1),
            },
        ]),
        role_families: BTreeMap::new(),
        lifecycle,
    }
}

async fn recovery_baseline() -> ProtocolBaseline {
    let account_device = DeviceId::from_uuid(Uuid::from_bytes([31; 16]));
    let guardian_device = DeviceId::from_uuid(Uuid::from_bytes([32; 16]));
    let coordinator_device = DeviceId::from_uuid(Uuid::from_bytes([33; 16]));
    let account_auth = authority(0x81);
    let guardian_auth = authority(0x82);
    let coordinator_auth = authority(0x83);

    let mut role_map = HashMap::new();
    role_map.insert(RecoveryProtocolRole::Account, account_auth);
    role_map.insert(RecoveryProtocolRole::Guardian, guardian_auth);
    role_map.insert(RecoveryProtocolRole::Coordinator, coordinator_auth);

    let bus = Arc::new(SimulatedMessageBus::new());
    let session_id = Uuid::from_bytes([34; 16]);

    let account_effects = TestEffectSystem::new(
        bus.clone(),
        account_device,
        RecoveryProtocolRole::Account.role_index().unwrap_or(0),
    )
    .expect("account effects");
    let guardian_effects = TestEffectSystem::new(
        bus.clone(),
        guardian_device,
        RecoveryProtocolRole::Guardian.role_index().unwrap_or(1),
    )
    .expect("guardian effects");
    let coordinator_effects = TestEffectSystem::new(
        bus,
        coordinator_device,
        RecoveryProtocolRole::Coordinator.role_index().unwrap_or(2),
    )
    .expect("coordinator effects");

    let mut account_adapter = AuraProtocolAdapter::new(
        Arc::new(account_effects),
        account_auth,
        RecoveryProtocolRole::Account,
        role_map.clone(),
    );
    let mut guardian_adapter = AuraProtocolAdapter::new(
        Arc::new(guardian_effects),
        guardian_auth,
        RecoveryProtocolRole::Guardian,
        role_map.clone(),
    );
    let mut coordinator_adapter = AuraProtocolAdapter::new(
        Arc::new(coordinator_effects),
        coordinator_auth,
        RecoveryProtocolRole::Coordinator,
        role_map,
    );

    let mut lifecycle = Vec::new();
    account_adapter
        .start_session(session_id)
        .await
        .expect("account start");
    push_event(&mut lifecycle, "Account", "start");
    guardian_adapter
        .start_session(session_id)
        .await
        .expect("guardian start");
    push_event(&mut lifecycle, "Guardian", "start");
    coordinator_adapter
        .start_session(session_id)
        .await
        .expect("coordinator start");
    push_event(&mut lifecycle, "Coordinator", "start");

    account_adapter.end_session().await.expect("account end");
    push_event(&mut lifecycle, "Account", "end");
    guardian_adapter.end_session().await.expect("guardian end");
    push_event(&mut lifecycle, "Guardian", "end");
    coordinator_adapter
        .end_session()
        .await
        .expect("coordinator end");
    push_event(&mut lifecycle, "Coordinator", "end");

    ProtocolBaseline {
        protocol: "recovery".to_string(),
        session_id: session_id.to_string(),
        role_bindings: sorted_role_bindings(vec![
            RoleBindingBaseline {
                role: "Account".to_string(),
                authority: authority_label(account_auth),
                role_index: RecoveryProtocolRole::Account.role_index().unwrap_or(0),
            },
            RoleBindingBaseline {
                role: "Coordinator".to_string(),
                authority: authority_label(coordinator_auth),
                role_index: RecoveryProtocolRole::Coordinator.role_index().unwrap_or(2),
            },
            RoleBindingBaseline {
                role: "Guardian".to_string(),
                authority: authority_label(guardian_auth),
                role_index: RecoveryProtocolRole::Guardian.role_index().unwrap_or(1),
            },
        ]),
        role_families: BTreeMap::new(),
        lifecycle,
    }
}

async fn rendezvous_baseline() -> ProtocolBaseline {
    let initiator_device = DeviceId::from_uuid(Uuid::from_bytes([41; 16]));
    let responder_device = DeviceId::from_uuid(Uuid::from_bytes([42; 16]));
    let initiator_auth = authority(0x91);
    let responder_auth = authority(0x92);

    let mut role_map = HashMap::new();
    role_map.insert(RendezvousExchangeRole::Initiator, initiator_auth);
    role_map.insert(RendezvousExchangeRole::Responder, responder_auth);

    let bus = Arc::new(SimulatedMessageBus::new());
    let session_id = Uuid::from_bytes([43; 16]);

    let initiator_effects = TestEffectSystem::new(
        bus.clone(),
        initiator_device,
        RendezvousExchangeRole::Initiator.role_index().unwrap_or(0),
    )
    .expect("initiator effects");
    let responder_effects = TestEffectSystem::new(
        bus,
        responder_device,
        RendezvousExchangeRole::Responder.role_index().unwrap_or(1),
    )
    .expect("responder effects");

    let mut initiator_adapter = AuraProtocolAdapter::new(
        Arc::new(initiator_effects),
        initiator_auth,
        RendezvousExchangeRole::Initiator,
        role_map.clone(),
    );
    let mut responder_adapter = AuraProtocolAdapter::new(
        Arc::new(responder_effects),
        responder_auth,
        RendezvousExchangeRole::Responder,
        role_map,
    );

    let mut lifecycle = Vec::new();
    initiator_adapter
        .start_session(session_id)
        .await
        .expect("initiator start");
    push_event(&mut lifecycle, "Initiator", "start");
    responder_adapter
        .start_session(session_id)
        .await
        .expect("responder start");
    push_event(&mut lifecycle, "Responder", "start");

    initiator_adapter
        .end_session()
        .await
        .expect("initiator end");
    push_event(&mut lifecycle, "Initiator", "end");
    responder_adapter
        .end_session()
        .await
        .expect("responder end");
    push_event(&mut lifecycle, "Responder", "end");

    ProtocolBaseline {
        protocol: "rendezvous".to_string(),
        session_id: session_id.to_string(),
        role_bindings: sorted_role_bindings(vec![
            RoleBindingBaseline {
                role: "Initiator".to_string(),
                authority: authority_label(initiator_auth),
                role_index: RendezvousExchangeRole::Initiator.role_index().unwrap_or(0),
            },
            RoleBindingBaseline {
                role: "Responder".to_string(),
                authority: authority_label(responder_auth),
                role_index: RendezvousExchangeRole::Responder.role_index().unwrap_or(1),
            },
        ]),
        role_families: BTreeMap::new(),
        lifecycle,
    }
}

async fn epoch_rotation_baseline() -> ProtocolBaseline {
    let coordinator_device = DeviceId::from_uuid(Uuid::from_bytes([51; 16]));
    let participant1_device = DeviceId::from_uuid(Uuid::from_bytes([52; 16]));
    let participant2_device = DeviceId::from_uuid(Uuid::from_bytes([53; 16]));
    let coordinator_auth = authority(0xa1);
    let participant1_auth = authority(0xa2);
    let participant2_auth = authority(0xa3);

    let mut role_map = HashMap::new();
    role_map.insert(EpochRotationProtocolRole::Coordinator, coordinator_auth);
    role_map.insert(EpochRotationProtocolRole::Participant1, participant1_auth);
    role_map.insert(EpochRotationProtocolRole::Participant2, participant2_auth);

    let participant_roles = vec![
        EpochRotationProtocolRole::Participant1,
        EpochRotationProtocolRole::Participant2,
    ];
    let mut role_families = BTreeMap::new();
    role_families.insert(
        "Participant".to_string(),
        vec!["Participant1".to_string(), "Participant2".to_string()],
    );

    let bus = Arc::new(SimulatedMessageBus::new());
    let session_id = Uuid::from_bytes([54; 16]);

    let coordinator_effects = TestEffectSystem::new(
        bus.clone(),
        coordinator_device,
        EpochRotationProtocolRole::Coordinator
            .role_index()
            .unwrap_or(0),
    )
    .expect("coordinator effects");
    let participant1_effects = TestEffectSystem::new(
        bus.clone(),
        participant1_device,
        EpochRotationProtocolRole::Participant1
            .role_index()
            .unwrap_or(1),
    )
    .expect("participant1 effects");
    let participant2_effects = TestEffectSystem::new(
        bus,
        participant2_device,
        EpochRotationProtocolRole::Participant2
            .role_index()
            .unwrap_or(2),
    )
    .expect("participant2 effects");

    let mut coordinator_adapter = AuraProtocolAdapter::new(
        Arc::new(coordinator_effects),
        coordinator_auth,
        EpochRotationProtocolRole::Coordinator,
        role_map.clone(),
    )
    .with_role_family("Participant", participant_roles.clone());
    let mut participant1_adapter = AuraProtocolAdapter::new(
        Arc::new(participant1_effects),
        participant1_auth,
        EpochRotationProtocolRole::Participant1,
        role_map.clone(),
    )
    .with_role_family("Participant", participant_roles.clone());
    let mut participant2_adapter = AuraProtocolAdapter::new(
        Arc::new(participant2_effects),
        participant2_auth,
        EpochRotationProtocolRole::Participant2,
        role_map,
    )
    .with_role_family("Participant", participant_roles);

    let mut lifecycle = Vec::new();
    coordinator_adapter
        .start_session(session_id)
        .await
        .expect("coordinator start");
    push_event(&mut lifecycle, "Coordinator", "start");
    participant1_adapter
        .start_session(session_id)
        .await
        .expect("participant1 start");
    push_event(&mut lifecycle, "Participant1", "start");
    participant2_adapter
        .start_session(session_id)
        .await
        .expect("participant2 start");
    push_event(&mut lifecycle, "Participant2", "start");

    coordinator_adapter
        .end_session()
        .await
        .expect("coordinator end");
    push_event(&mut lifecycle, "Coordinator", "end");
    participant1_adapter
        .end_session()
        .await
        .expect("participant1 end");
    push_event(&mut lifecycle, "Participant1", "end");
    participant2_adapter
        .end_session()
        .await
        .expect("participant2 end");
    push_event(&mut lifecycle, "Participant2", "end");

    ProtocolBaseline {
        protocol: "epoch_rotation".to_string(),
        session_id: session_id.to_string(),
        role_bindings: sorted_role_bindings(vec![
            RoleBindingBaseline {
                role: "Coordinator".to_string(),
                authority: authority_label(coordinator_auth),
                role_index: EpochRotationProtocolRole::Coordinator
                    .role_index()
                    .unwrap_or(0),
            },
            RoleBindingBaseline {
                role: "Participant1".to_string(),
                authority: authority_label(participant1_auth),
                role_index: EpochRotationProtocolRole::Participant1
                    .role_index()
                    .unwrap_or(1),
            },
            RoleBindingBaseline {
                role: "Participant2".to_string(),
                authority: authority_label(participant2_auth),
                role_index: EpochRotationProtocolRole::Participant2
                    .role_index()
                    .unwrap_or(2),
            },
        ]),
        role_families,
        lifecycle,
    }
}

#[tokio::test]
async fn choreography_baselines_match_golden() {
    let baselines = vec![
        consensus_baseline().await,
        invitation_baseline().await,
        recovery_baseline().await,
        rendezvous_baseline().await,
        epoch_rotation_baseline().await,
    ];

    for baseline in &baselines {
        assert_or_update(baseline);
    }
}

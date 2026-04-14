use super::*;
use async_trait::async_trait;
use aura_core::effects::{JournalEffects, PhysicalTimeEffects, ThresholdSigningEffects};
use aura_core::threshold::{
    policy_for, CeremonyFlow, ParticipantIdentity, SigningContext, ThresholdConfig, ThresholdState,
};
use aura_core::time::PhysicalTime;
use aura_core::types::epochs::Epoch;
use aura_core::{AuraError, ContextId, FlowBudget, FlowCost, Journal};
use std::sync::{Arc, Mutex};

fn test_prestate() -> Hash32 {
    Hash32([1u8; 32])
}

fn test_upgrade_hash() -> Hash32 {
    Hash32([2u8; 32])
}

fn test_authority(seed: u8) -> AuthorityId {
    AuthorityId::new_from_entropy([seed; 32])
}

#[derive(Clone)]
struct TestEffects {
    journal: Arc<Mutex<Journal>>,
    time_ms: Arc<Mutex<u64>>,
}

impl TestEffects {
    fn new() -> Self {
        Self {
            journal: Arc::new(Mutex::new(Journal::new())),
            time_ms: Arc::new(Mutex::new(1_700_000_000_000)),
        }
    }

    fn snapshot_journal(&self) -> Journal {
        let journal = self.journal.lock().unwrap();
        let mut copy = Journal::new();
        copy.facts = journal.facts.clone();
        copy.caps = journal.caps.clone();
        copy
    }
}

#[async_trait]
impl JournalEffects for TestEffects {
    async fn merge_facts(&self, target: Journal, delta: Journal) -> Result<Journal, AuraError> {
        let mut merged = Journal::new();
        merged.facts = target.facts.clone();
        merged.caps = target.caps;
        merged.merge_facts(delta.read_facts().clone());
        Ok(merged)
    }

    async fn refine_caps(
        &self,
        target: Journal,
        refinement: Journal,
    ) -> Result<Journal, AuraError> {
        let mut refined = Journal::new();
        refined.facts = target.facts.clone();
        refined.caps = target.caps;
        refined.refine_caps(refinement.read_caps().clone());
        Ok(refined)
    }

    async fn get_journal(&self) -> Result<Journal, AuraError> {
        Ok(self.snapshot_journal())
    }

    async fn persist_journal(&self, journal: &Journal) -> Result<(), AuraError> {
        let mut stored = self.journal.lock().unwrap();
        stored.facts = journal.facts.clone();
        stored.caps = journal.caps.clone();
        Ok(())
    }

    async fn get_flow_budget(
        &self,
        _context: &ContextId,
        _peer: &AuthorityId,
    ) -> Result<FlowBudget, AuraError> {
        Ok(FlowBudget::new(1_000, Epoch::new(0)))
    }

    async fn update_flow_budget(
        &self,
        _context: &ContextId,
        _peer: &AuthorityId,
        budget: &FlowBudget,
    ) -> Result<FlowBudget, AuraError> {
        Ok(*budget)
    }

    async fn charge_flow_budget(
        &self,
        _context: &ContextId,
        _peer: &AuthorityId,
        cost: FlowCost,
    ) -> Result<FlowBudget, AuraError> {
        let mut budget = FlowBudget::new(1_000, Epoch::new(0));
        budget.spent = u64::from(cost);
        Ok(budget)
    }
}

#[async_trait]
impl PhysicalTimeEffects for TestEffects {
    async fn physical_time(&self) -> Result<PhysicalTime, aura_core::effects::time::TimeError> {
        let mut time = self.time_ms.lock().unwrap();
        *time += 1;
        Ok(PhysicalTime {
            ts_ms: *time,
            uncertainty: None,
        })
    }

    async fn sleep_ms(&self, _ms: u64) -> Result<(), aura_core::effects::time::TimeError> {
        Ok(())
    }
}

#[async_trait]
impl ThresholdSigningEffects for TestEffects {
    async fn bootstrap_authority(&self, _authority: &AuthorityId) -> Result<Vec<u8>, AuraError> {
        Ok(vec![0u8; 32])
    }

    async fn sign(&self, _context: SigningContext) -> Result<ThresholdSignature, AuraError> {
        Ok(ThresholdSignature::single_signer(
            vec![0u8; 64],
            vec![0u8; 32],
            0,
        ))
    }

    async fn threshold_config(&self, _authority: &AuthorityId) -> Option<ThresholdConfig> {
        Some(ThresholdConfig {
            threshold: 1,
            total_participants: 1,
        })
    }

    async fn threshold_state(&self, authority: &AuthorityId) -> Option<ThresholdState> {
        Some(ThresholdState {
            epoch: 0,
            threshold: 1,
            total_participants: 1,
            participants: vec![ParticipantIdentity::guardian(*authority)],
            agreement_mode: AgreementMode::Provisional,
        })
    }

    async fn has_signing_capability(&self, _authority: &AuthorityId) -> bool {
        true
    }

    async fn public_key_package(&self, _authority: &AuthorityId) -> Option<Vec<u8>> {
        Some(vec![0u8; 32])
    }

    async fn rotate_keys(
        &self,
        _authority: &AuthorityId,
        _new_threshold: u16,
        _new_total_participants: u16,
        _participants: &[ParticipantIdentity],
    ) -> Result<(u64, Vec<Vec<u8>>, Vec<u8>), AuraError> {
        Ok((1, vec![vec![0u8; 32]], vec![0u8; 32]))
    }

    async fn commit_key_rotation(
        &self,
        _authority: &AuthorityId,
        _new_epoch: u64,
    ) -> Result<(), AuraError> {
        Ok(())
    }

    async fn rollback_key_rotation(
        &self,
        _authority: &AuthorityId,
        _failed_epoch: u64,
    ) -> Result<(), AuraError> {
        Ok(())
    }
}

#[test]
fn test_ceremony_id_determinism() {
    let id1 = OTACeremonyId::new(&test_prestate(), &test_upgrade_hash(), 12345);
    let id2 = OTACeremonyId::new(&test_prestate(), &test_upgrade_hash(), 12345);
    assert_eq!(id1, id2);
}

#[test]
fn test_ceremony_id_uniqueness_with_nonce() {
    let id1 = OTACeremonyId::new(&test_prestate(), &test_upgrade_hash(), 12345);
    let id2 = OTACeremonyId::new(&test_prestate(), &test_upgrade_hash(), 12346);
    assert_ne!(id1, id2);
}

#[test]
fn test_ceremony_id_uniqueness_with_prestate() {
    let prestate1 = Hash32([1u8; 32]);
    let prestate2 = Hash32([3u8; 32]);
    let id1 = OTACeremonyId::new(&prestate1, &test_upgrade_hash(), 12345);
    let id2 = OTACeremonyId::new(&prestate2, &test_upgrade_hash(), 12345);
    assert_ne!(id1, id2);
}

#[test]
fn test_ceremony_status_transitions() {
    let status = OTACeremonyStatus::CollectingCommitments;
    assert!(matches!(status, OTACeremonyStatus::CollectingCommitments));

    let status = OTACeremonyStatus::AwaitingConsensus;
    assert!(matches!(status, OTACeremonyStatus::AwaitingConsensus));

    let status = OTACeremonyStatus::Committed;
    assert!(matches!(status, OTACeremonyStatus::Committed));

    let status = OTACeremonyStatus::Aborted {
        reason: OTACeremonyAbortReason::TimedOut,
    };
    assert!(matches!(
        status,
        OTACeremonyStatus::Aborted {
            reason: OTACeremonyAbortReason::TimedOut
        }
    ));
}

#[test]
fn test_ceremony_state_threshold_check() {
    let mut state = OTACeremonyState {
        ceremony_id: OTACeremonyId::new(&test_prestate(), &test_upgrade_hash(), 1),
        proposal: UpgradeProposal {
            proposal_id: Uuid::from_bytes(1u128.to_be_bytes()),
            package_id: Uuid::from_bytes(2u128.to_be_bytes()),
            version: SemanticVersion::new(2, 0, 0),
            kind: UpgradeKind::HardFork,
            package_hash: Hash32([0u8; 32]),
            activation_epoch: Epoch::new(200),
            coordinator: DeviceId::from_bytes([1; 32]),
        },
        status: OTACeremonyStatus::CollectingCommitments,
        agreement_mode: policy_for(CeremonyFlow::OtaActivation).initial_mode(),
        commitments: HashMap::new(),
        threshold: 2,
        quorum_size: 3,
        started_at_ms: 0,
        timeout_ms: 1000,
    };

    assert!(!state.threshold_met());
    assert_eq!(state.ready_count(), 0);

    state.commitments.insert(
        DeviceId::from_bytes([2; 32]),
        ReadinessCommitment {
            ceremony_id: state.ceremony_id,
            device: DeviceId::from_bytes([2; 32]),
            authority: test_authority(2),
            prestate_hash: test_prestate(),
            ready: true,
            reason: None,
            signature: ThresholdSignature::single_signer(vec![], vec![], 0),
            committed_at_ms: 0,
        },
    );
    assert!(!state.threshold_met());
    assert_eq!(state.ready_count(), 1);

    state.commitments.insert(
        DeviceId::from_bytes([3; 32]),
        ReadinessCommitment {
            ceremony_id: state.ceremony_id,
            device: DeviceId::from_bytes([3; 32]),
            authority: test_authority(3),
            prestate_hash: test_prestate(),
            ready: true,
            reason: None,
            signature: ThresholdSignature::single_signer(vec![], vec![], 0),
            committed_at_ms: 0,
        },
    );
    assert!(state.threshold_met());
    assert_eq!(state.ready_count(), 2);

    let ready = state.ready_devices();
    assert_eq!(ready.len(), 2);
}

#[test]
fn test_readiness_commitment_serialization() {
    let commitment = ReadinessCommitment {
        ceremony_id: OTACeremonyId::new(&test_prestate(), &test_upgrade_hash(), 1),
        device: DeviceId::from_bytes([42u8; 32]),
        authority: test_authority(42),
        prestate_hash: Hash32([0u8; 32]),
        ready: true,
        reason: None,
        signature: ThresholdSignature::single_signer(vec![1, 2, 3], vec![4, 5, 6], 0),
        committed_at_ms: 12345,
    };

    let bytes = serde_json::to_vec(&commitment).unwrap();
    let restored: ReadinessCommitment = serde_json::from_slice(&bytes).unwrap();

    assert!(restored.ready);
    assert_eq!(restored.committed_at_ms, 12345);
    assert_eq!(restored.authority, test_authority(42));
}

#[test]
fn test_ota_ceremony_fact_serialization() {
    let fact = OTACeremonyFact::CeremonyInitiated {
        ceremony_id: "abc123".to_string(),
        trace_id: None,
        proposal_id: "prop-1".to_string(),
        package_id: "pkg-1".to_string(),
        version: "2.0.0".to_string(),
        activation_epoch: Epoch::new(200),
        coordinator: "coord-1".to_string(),
        threshold: 2,
        quorum_size: 3,
        timestamp_ms: 12345,
    };

    let bytes = serde_json::to_vec(&fact).unwrap();
    let restored: OTACeremonyFact = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(restored.ceremony_id(), "abc123");
    assert_eq!(restored.timestamp_ms(), 12345);
}

#[test]
fn test_upgrade_proposal_hash() {
    let proposal1 = UpgradeProposal {
        proposal_id: Uuid::from_bytes(1u128.to_be_bytes()),
        package_id: Uuid::from_bytes(2u128.to_be_bytes()),
        version: SemanticVersion::new(2, 0, 0),
        kind: UpgradeKind::HardFork,
        package_hash: Hash32([0u8; 32]),
        activation_epoch: Epoch::new(200),
        coordinator: DeviceId::from_bytes([1; 32]),
    };

    let proposal2 = UpgradeProposal {
        proposal_id: Uuid::from_bytes(1u128.to_be_bytes()),
        package_id: Uuid::from_bytes(2u128.to_be_bytes()),
        version: SemanticVersion::new(2, 0, 0),
        kind: UpgradeKind::HardFork,
        package_hash: Hash32([0u8; 32]),
        activation_epoch: Epoch::new(200),
        coordinator: DeviceId::from_bytes([1; 32]),
    };

    assert_eq!(proposal1.compute_hash(), proposal2.compute_hash());

    let proposal1_hash = proposal1.compute_hash();
    let proposal3 = UpgradeProposal {
        activation_epoch: Epoch::new(300),
        ..proposal1
    };
    assert_ne!(proposal1_hash, proposal3.compute_hash());
}

#[test]
fn test_ota_commit_helper_emits_fact() {
    let mut pool = futures::executor::LocalPool::new();
    pool.run_until(async {
        let effects = TestEffects::new();
        let ceremony_id = OTACeremonyId::new(&test_prestate(), &test_upgrade_hash(), 1);
        let ready_devices = vec![DeviceId::from_bytes([1u8; 32])];
        let commitment = ReadinessCommitment {
            ceremony_id,
            device: ready_devices[0],
            authority: test_authority(9),
            prestate_hash: test_prestate(),
            ready: true,
            reason: None,
            signature: ThresholdSignature::single_signer(vec![7u8; 64], vec![8u8; 32], 0),
            committed_at_ms: 42,
        };
        let signature = create_ota_activation_signature(ceremony_id, &[commitment]).unwrap();

        emit_ota_ceremony_committed_fact(
            &effects,
            ceremony_id,
            Epoch::new(10),
            &ready_devices,
            &signature,
        )
        .await
        .unwrap();

        let journal = effects.snapshot_journal();
        let key = format!("ota:committed:{}", hex::encode(ceremony_id.0.as_bytes()));
        assert!(
            journal.facts.contains_key(&key),
            "Expected committed fact in journal"
        );
    });
}

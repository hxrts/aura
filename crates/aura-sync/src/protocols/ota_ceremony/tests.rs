use super::*;
use async_trait::async_trait;
use aura_core::crypto::SingleSignerPublicKeyPackage;
use aura_core::effects::CryptoCoreEffects;
use aura_core::effects::{JournalEffects, PhysicalTimeEffects, ThresholdSigningEffects};
use aura_core::threshold::{
    policy_for, CeremonyFlow, ParticipantIdentity, SigningContext, ThresholdConfig, ThresholdState,
};
use aura_core::time::PhysicalTime;
use aura_core::types::epochs::Epoch;
use aura_core::{AuraError, ContextId, FlowBudget, FlowCost, Journal};
use aura_effects::RealCryptoHandler;
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

async fn signed_readiness_commitment(
    seed: u8,
    ceremony_id: OTACeremonyId,
    proposal: &UpgradeProposal,
    prestate_hash: Hash32,
    device: DeviceId,
    authority: AuthorityId,
    ready: bool,
    committed_at_ms: u64,
) -> (ReadinessCommitment, Vec<u8>) {
    let crypto = RealCryptoHandler::for_simulation_seed([seed; 32]);
    let (private_key, public_key) = crypto
        .ed25519_generate_keypair()
        .await
        .expect("ed25519 keypair");
    let trusted_public_key_package = SingleSignerPublicKeyPackage::new(public_key)
        .to_bytes()
        .expect("serialize single-signer package");
    let context = ota_readiness_signing_context(
        &ReadinessCommitment {
            ceremony_id,
            device,
            authority,
            prestate_hash,
            ready,
            reason: None,
            signature: ThresholdSignature::single_signer(
                vec![],
                trusted_public_key_package.clone(),
                0,
            ),
            committed_at_ms,
        },
        proposal.compute_hash(),
        proposal.activation_epoch,
    );
    let transcript = aura_signature::threshold_signing_context_transcript_bytes(&context, 0)
        .expect("ota readiness transcript");
    let signature = crypto
        .ed25519_sign(&transcript, &private_key)
        .await
        .expect("sign ota readiness transcript");
    (
        ReadinessCommitment {
            ceremony_id,
            device,
            authority,
            prestate_hash,
            ready,
            reason: None,
            signature: ThresholdSignature::single_signer(
                signature,
                trusted_public_key_package.clone(),
                0,
            ),
            committed_at_ms,
        },
        trusted_public_key_package,
    )
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
        proposal_hash: test_upgrade_hash(),
        prestate_hash: test_prestate(),
        status: OTACeremonyStatus::CollectingCommitments,
        agreement_mode: policy_for(CeremonyFlow::OtaActivation).initial_mode(),
        quorum_members: HashMap::new(),
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
        let certificate = create_ota_activation_certificate(&[commitment.clone()]).unwrap();

        emit_ota_ceremony_committed_fact(
            &effects,
            ceremony_id,
            Epoch::new(10),
            &test_upgrade_hash(),
            &test_prestate(),
            &ready_devices,
            &certificate,
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

#[tokio::test]
async fn verify_ota_readiness_commitment_rejects_forged_signature() {
    let proposal = UpgradeProposal {
        proposal_id: Uuid::from_bytes(1u128.to_be_bytes()),
        package_id: Uuid::from_bytes(2u128.to_be_bytes()),
        version: SemanticVersion::new(2, 0, 0),
        kind: UpgradeKind::HardFork,
        package_hash: Hash32([9u8; 32]),
        activation_epoch: Epoch::new(200),
        coordinator: DeviceId::from_bytes([1; 32]),
    };
    let ceremony_id = OTACeremonyId::new(&test_prestate(), &proposal.compute_hash(), 1);
    let authority = test_authority(7);
    let device = DeviceId::from_bytes([7u8; 32]);
    let (mut commitment, trusted_public_key_package) = signed_readiness_commitment(
        7,
        ceremony_id,
        &proposal,
        test_prestate(),
        device,
        authority,
        true,
        55,
    )
    .await;
    commitment.signature.signature[0] ^= 0xFF;

    assert!(verify_ota_readiness_commitment(
        &RealCryptoHandler::for_simulation_seed([7; 32]),
        &commitment,
        proposal.compute_hash(),
        proposal.activation_epoch,
        &trusted_public_key_package,
    )
    .await
    .is_err());
}

#[tokio::test]
async fn verify_ota_readiness_commitment_rejects_wrong_proposal_binding() {
    let proposal = UpgradeProposal {
        proposal_id: Uuid::from_bytes(3u128.to_be_bytes()),
        package_id: Uuid::from_bytes(4u128.to_be_bytes()),
        version: SemanticVersion::new(2, 1, 0),
        kind: UpgradeKind::HardFork,
        package_hash: Hash32([4u8; 32]),
        activation_epoch: Epoch::new(220),
        coordinator: DeviceId::from_bytes([2; 32]),
    };
    let ceremony_id = OTACeremonyId::new(&test_prestate(), &proposal.compute_hash(), 2);
    let authority = test_authority(8);
    let device = DeviceId::from_bytes([8u8; 32]);
    let (commitment, trusted_public_key_package) = signed_readiness_commitment(
        8,
        ceremony_id,
        &proposal,
        test_prestate(),
        device,
        authority,
        true,
        77,
    )
    .await;

    assert!(verify_ota_readiness_commitment(
        &RealCryptoHandler::for_simulation_seed([8; 32]),
        &commitment,
        Hash32([5u8; 32]),
        proposal.activation_epoch,
        &trusted_public_key_package,
    )
    .await
    .is_err());
}

#[test]
fn ota_ready_count_counts_unique_authorities() {
    let authority = test_authority(9);
    let mut state = OTACeremonyState {
        ceremony_id: OTACeremonyId::new(&test_prestate(), &test_upgrade_hash(), 9),
        proposal: UpgradeProposal {
            proposal_id: Uuid::from_bytes(9u128.to_be_bytes()),
            package_id: Uuid::from_bytes(10u128.to_be_bytes()),
            version: SemanticVersion::new(3, 0, 0),
            kind: UpgradeKind::HardFork,
            package_hash: Hash32([6u8; 32]),
            activation_epoch: Epoch::new(300),
            coordinator: DeviceId::from_bytes([9; 32]),
        },
        proposal_hash: test_upgrade_hash(),
        prestate_hash: test_prestate(),
        status: OTACeremonyStatus::CollectingCommitments,
        agreement_mode: policy_for(CeremonyFlow::OtaActivation).initial_mode(),
        quorum_members: HashMap::new(),
        commitments: HashMap::new(),
        threshold: 2,
        quorum_size: 3,
        started_at_ms: 0,
        timeout_ms: 1000,
    };
    state.commitments.insert(
        DeviceId::from_bytes([1u8; 32]),
        ReadinessCommitment {
            ceremony_id: state.ceremony_id,
            device: DeviceId::from_bytes([1u8; 32]),
            authority,
            prestate_hash: test_prestate(),
            ready: true,
            reason: None,
            signature: ThresholdSignature::single_signer(vec![1; 64], vec![2; 32], 0),
            committed_at_ms: 1,
        },
    );
    state.commitments.insert(
        DeviceId::from_bytes([2u8; 32]),
        ReadinessCommitment {
            ceremony_id: state.ceremony_id,
            device: DeviceId::from_bytes([2u8; 32]),
            authority,
            prestate_hash: test_prestate(),
            ready: true,
            reason: None,
            signature: ThresholdSignature::single_signer(vec![3; 64], vec![4; 32], 0),
            committed_at_ms: 2,
        },
    );

    assert_eq!(state.ready_count(), 1);
    assert!(!state.threshold_met());
}

#[test]
fn validate_ota_quorum_commitment_rejects_wrong_ceremony() {
    let proposal = UpgradeProposal {
        proposal_id: Uuid::from_bytes(11u128.to_be_bytes()),
        package_id: Uuid::from_bytes(12u128.to_be_bytes()),
        version: SemanticVersion::new(3, 1, 0),
        kind: UpgradeKind::HardFork,
        package_hash: Hash32([7u8; 32]),
        activation_epoch: Epoch::new(320),
        coordinator: DeviceId::from_bytes([3; 32]),
    };
    let member = OTAQuorumMember {
        device: DeviceId::from_bytes([3u8; 32]),
        authority: test_authority(10),
        public_key_package: vec![5u8; 32],
    };
    let state = OTACeremonyState {
        ceremony_id: OTACeremonyId::new(&test_prestate(), &proposal.compute_hash(), 3),
        proposal_hash: proposal.compute_hash(),
        prestate_hash: test_prestate(),
        proposal,
        status: OTACeremonyStatus::CollectingCommitments,
        agreement_mode: policy_for(CeremonyFlow::OtaActivation).initial_mode(),
        quorum_members: HashMap::from([(member.device, member.clone())]),
        commitments: HashMap::new(),
        threshold: 2,
        quorum_size: 3,
        started_at_ms: 0,
        timeout_ms: 1000,
    };
    let commitment = ReadinessCommitment {
        ceremony_id: OTACeremonyId::new(&test_prestate(), &test_upgrade_hash(), 99),
        device: member.device,
        authority: member.authority,
        prestate_hash: test_prestate(),
        ready: true,
        reason: None,
        signature: ThresholdSignature::single_signer(vec![1; 64], vec![2; 32], 0),
        committed_at_ms: 1,
    };

    assert!(validate_ota_quorum_commitment(&state, &commitment).is_err());
}

#[test]
fn validate_ota_quorum_commitment_rejects_wrong_prestate_and_non_quorum_device() {
    let proposal = UpgradeProposal {
        proposal_id: Uuid::from_bytes(13u128.to_be_bytes()),
        package_id: Uuid::from_bytes(14u128.to_be_bytes()),
        version: SemanticVersion::new(3, 2, 0),
        kind: UpgradeKind::HardFork,
        package_hash: Hash32([8u8; 32]),
        activation_epoch: Epoch::new(330),
        coordinator: DeviceId::from_bytes([4; 32]),
    };
    let member = OTAQuorumMember {
        device: DeviceId::from_bytes([4u8; 32]),
        authority: test_authority(11),
        public_key_package: vec![6u8; 32],
    };
    let state = OTACeremonyState {
        ceremony_id: OTACeremonyId::new(&test_prestate(), &proposal.compute_hash(), 4),
        proposal_hash: proposal.compute_hash(),
        prestate_hash: test_prestate(),
        proposal,
        status: OTACeremonyStatus::CollectingCommitments,
        agreement_mode: policy_for(CeremonyFlow::OtaActivation).initial_mode(),
        quorum_members: HashMap::from([(member.device, member.clone())]),
        commitments: HashMap::new(),
        threshold: 2,
        quorum_size: 3,
        started_at_ms: 0,
        timeout_ms: 1000,
    };
    let wrong_prestate = ReadinessCommitment {
        ceremony_id: state.ceremony_id,
        device: member.device,
        authority: member.authority,
        prestate_hash: Hash32([0u8; 32]),
        ready: true,
        reason: None,
        signature: ThresholdSignature::single_signer(vec![1; 64], vec![2; 32], 0),
        committed_at_ms: 1,
    };
    let non_quorum = ReadinessCommitment {
        ceremony_id: state.ceremony_id,
        device: DeviceId::from_bytes([99u8; 32]),
        authority: member.authority,
        prestate_hash: test_prestate(),
        ready: true,
        reason: None,
        signature: ThresholdSignature::single_signer(vec![1; 64], vec![2; 32], 0),
        committed_at_ms: 1,
    };

    assert!(validate_ota_quorum_commitment(&state, &wrong_prestate).is_err());
    assert!(validate_ota_quorum_commitment(&state, &non_quorum).is_err());
}

#[test]
fn validate_ota_quorum_commitment_rejects_duplicate_authority_and_accepts_unique_quorum() {
    let proposal = UpgradeProposal {
        proposal_id: Uuid::from_bytes(15u128.to_be_bytes()),
        package_id: Uuid::from_bytes(16u128.to_be_bytes()),
        version: SemanticVersion::new(4, 0, 0),
        kind: UpgradeKind::HardFork,
        package_hash: Hash32([9u8; 32]),
        activation_epoch: Epoch::new(340),
        coordinator: DeviceId::from_bytes([5; 32]),
    };
    let authority_a = test_authority(12);
    let authority_b = test_authority(13);
    let member_a = OTAQuorumMember {
        device: DeviceId::from_bytes([5u8; 32]),
        authority: authority_a,
        public_key_package: vec![7u8; 32],
    };
    let member_b = OTAQuorumMember {
        device: DeviceId::from_bytes([6u8; 32]),
        authority: authority_b,
        public_key_package: vec![8u8; 32],
    };
    let mut state = OTACeremonyState {
        ceremony_id: OTACeremonyId::new(&test_prestate(), &proposal.compute_hash(), 5),
        proposal_hash: proposal.compute_hash(),
        prestate_hash: test_prestate(),
        proposal,
        status: OTACeremonyStatus::CollectingCommitments,
        agreement_mode: policy_for(CeremonyFlow::OtaActivation).initial_mode(),
        quorum_members: HashMap::from([
            (member_a.device, member_a.clone()),
            (member_b.device, member_b.clone()),
        ]),
        commitments: HashMap::new(),
        threshold: 2,
        quorum_size: 2,
        started_at_ms: 0,
        timeout_ms: 1000,
    };
    state.commitments.insert(
        member_a.device,
        ReadinessCommitment {
            ceremony_id: state.ceremony_id,
            device: member_a.device,
            authority: authority_a,
            prestate_hash: test_prestate(),
            ready: true,
            reason: None,
            signature: ThresholdSignature::single_signer(vec![1; 64], vec![2; 32], 0),
            committed_at_ms: 1,
        },
    );
    let duplicate_authority = ReadinessCommitment {
        ceremony_id: state.ceremony_id,
        device: member_b.device,
        authority: authority_a,
        prestate_hash: test_prestate(),
        ready: true,
        reason: None,
        signature: ThresholdSignature::single_signer(vec![1; 64], vec![2; 32], 0),
        committed_at_ms: 2,
    };
    let unique_authority = ReadinessCommitment {
        authority: authority_b,
        ..duplicate_authority.clone()
    };

    assert!(validate_ota_quorum_commitment(&state, &duplicate_authority).is_err());
    assert!(validate_ota_quorum_commitment(&state, &unique_authority).is_ok());
}

#[tokio::test]
async fn ota_threshold_completion_accepts_verified_unique_commitments() {
    let proposal = UpgradeProposal {
        proposal_id: Uuid::from_bytes(17u128.to_be_bytes()),
        package_id: Uuid::from_bytes(18u128.to_be_bytes()),
        version: SemanticVersion::new(4, 1, 0),
        kind: UpgradeKind::HardFork,
        package_hash: Hash32([10u8; 32]),
        activation_epoch: Epoch::new(350),
        coordinator: DeviceId::from_bytes([6; 32]),
    };
    let ceremony_id = OTACeremonyId::new(&test_prestate(), &proposal.compute_hash(), 6);
    let (commitment_a, trusted_a) = signed_readiness_commitment(
        21,
        ceremony_id,
        &proposal,
        test_prestate(),
        DeviceId::from_bytes([21u8; 32]),
        test_authority(21),
        true,
        101,
    )
    .await;
    let (commitment_b, trusted_b) = signed_readiness_commitment(
        22,
        ceremony_id,
        &proposal,
        test_prestate(),
        DeviceId::from_bytes([22u8; 32]),
        test_authority(22),
        true,
        102,
    )
    .await;

    verify_ota_readiness_commitment(
        &RealCryptoHandler::for_simulation_seed([21; 32]),
        &commitment_a,
        proposal.compute_hash(),
        proposal.activation_epoch,
        &trusted_a,
    )
    .await
    .expect("first verified readiness commitment");
    verify_ota_readiness_commitment(
        &RealCryptoHandler::for_simulation_seed([22; 32]),
        &commitment_b,
        proposal.compute_hash(),
        proposal.activation_epoch,
        &trusted_b,
    )
    .await
    .expect("second verified readiness commitment");

    let mut state = OTACeremonyState {
        ceremony_id,
        proposal_hash: proposal.compute_hash(),
        prestate_hash: test_prestate(),
        proposal,
        status: OTACeremonyStatus::CollectingCommitments,
        agreement_mode: policy_for(CeremonyFlow::OtaActivation).initial_mode(),
        quorum_members: HashMap::from([
            (
                commitment_a.device,
                OTAQuorumMember {
                    device: commitment_a.device,
                    authority: commitment_a.authority,
                    public_key_package: trusted_a,
                },
            ),
            (
                commitment_b.device,
                OTAQuorumMember {
                    device: commitment_b.device,
                    authority: commitment_b.authority,
                    public_key_package: trusted_b,
                },
            ),
        ]),
        commitments: HashMap::new(),
        threshold: 2,
        quorum_size: 2,
        started_at_ms: 0,
        timeout_ms: 1000,
    };
    state
        .commitments
        .insert(commitment_a.device, commitment_a.clone());
    state
        .commitments
        .insert(commitment_b.device, commitment_b.clone());

    assert!(state.threshold_met());
    let certificate = create_ota_activation_certificate(&state.ready_commitments())
        .expect("certificate should be created from verified commitments");
    assert_eq!(certificate.len(), 2);
    assert_eq!(certificate[0].device, commitment_a.device);
    assert_eq!(certificate[1].device, commitment_b.device);
}

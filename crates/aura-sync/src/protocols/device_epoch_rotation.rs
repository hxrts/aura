//! Device-scoped epoch rotation choreography surface.
//!
//! This protocol covers the device-specific share distribution / acceptance /
//! commit handshake used by enrollment and removal ceremonies once the local
//! initiator has already prepared the pending epoch.

use aura_core::types::identifiers::CeremonyId;
use aura_core::{AttestedOp, AuthorityId, DeviceId};
use aura_macros::tell;
use serde::{Deserialize, Serialize};

/// The initiating ceremony type for one device-scoped epoch rotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceEpochRotationKind {
    Rotation,
    Enrollment,
    Removal,
}

/// Proposal sent from the initiating device to one participant device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceEpochProposal {
    pub ceremony_id: CeremonyId,
    pub kind: DeviceEpochRotationKind,
    pub subject_authority: AuthorityId,
    pub pending_epoch: u64,
    pub initiator_device_id: DeviceId,
    pub participant_device_id: DeviceId,
    // aura-security: raw-secret-field-justified owner=security-refactor expires=before-release remediation=work/2.md choreography wire payload; producer must wrap/export with key-wrapping context before serialization.
    pub key_package: Vec<u8>,
    // aura-security: raw-secret-field-justified owner=security-refactor expires=before-release remediation=work/2.md choreography wire payload; producer must wrap/export with key-wrapping context before serialization.
    pub threshold_config: Vec<u8>,
    /// Untrusted key material: pending-epoch ceremony payload; authentication must resolve expected keys from trusted authority/device state.
    pub public_key_package: Vec<u8>,
}

/// Acceptance issued by one participant device after locally staging the share.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceEpochAcceptance {
    pub ceremony_id: CeremonyId,
    pub acceptor_device_id: DeviceId,
}

/// Commit sent by the initiator once the ceremony threshold is satisfied.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceEpochCommit {
    pub ceremony_id: CeremonyId,
    pub new_epoch: u64,
    pub attested_leaf_op: Option<AttestedOp>,
}

tell!(include_str!("src/protocols/device_epoch_rotation.tell"));

#[cfg(test)]
mod tests {
    use super::*;
    use aura_protocol::admission::{
        CAPABILITY_PROTOCOL_ENVELOPE_BRIDGE, CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADHERENCE,
        CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADMISSION, CAPABILITY_RECONFIGURATION_SAFETY,
        THEOREM_PACK_AURA_TRANSITION_SAFETY,
    };

    #[test]
    fn proof_status_exposes_required_transition_pack() {
        assert_eq!(
            telltale_session_types_device_epoch_rotation::proof_status::REQUIRED_THEOREM_PACKS,
            &[THEOREM_PACK_AURA_TRANSITION_SAFETY]
        );
    }

    #[test]
    fn manifest_emits_transition_safety_pack_metadata() {
        let manifest =
            telltale_session_types_device_epoch_rotation::vm_artifacts::composition_manifest();
        assert_eq!(
            manifest.required_theorem_packs,
            vec![THEOREM_PACK_AURA_TRANSITION_SAFETY.to_string()]
        );
        assert_eq!(
            manifest.required_theorem_pack_capabilities,
            vec![
                CAPABILITY_PROTOCOL_ENVELOPE_BRIDGE.to_string(),
                CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADHERENCE.to_string(),
                CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADMISSION.to_string(),
                CAPABILITY_RECONFIGURATION_SAFETY.to_string(),
            ]
        );
    }

    #[test]
    fn device_epoch_rotation_transcripts_bind_epoch_and_participant() {
        let proposal = DeviceEpochProposal {
            ceremony_id: CeremonyId::new("device-epoch-1"),
            kind: DeviceEpochRotationKind::Rotation,
            subject_authority: AuthorityId::new_from_entropy([2u8; 32]),
            pending_epoch: 10,
            initiator_device_id: DeviceId::new_from_entropy([3u8; 32]),
            participant_device_id: DeviceId::new_from_entropy([4u8; 32]),
            key_package: vec![5],
            threshold_config: vec![6],
            public_key_package: vec![7],
        };
        let mut different_epoch = proposal.clone();
        different_epoch.pending_epoch = 11;
        let mut different_participant = proposal.clone();
        different_participant.participant_device_id = DeviceId::new_from_entropy([8u8; 32]);

        let base =
            aura_signature::encode_transcript("aura.device-epoch.proposal", 1, &proposal).unwrap();
        let epoch =
            aura_signature::encode_transcript("aura.device-epoch.proposal", 1, &different_epoch)
                .unwrap();
        let participant = aura_signature::encode_transcript(
            "aura.device-epoch.proposal",
            1,
            &different_participant,
        )
        .unwrap();

        assert_ne!(base, epoch);
        assert_ne!(base, participant);
    }
}

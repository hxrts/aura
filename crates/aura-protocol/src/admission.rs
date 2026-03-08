//! Protocol runtime-capability admission requirements.
//!
//! This module declares theorem-pack/runtime capability requirements for
//! choreography bundles before execution.

use crate::termination::TerminationProtocolClass;
use aura_core::effects::{AdmissionError, CapabilityKey};

/// Capability key for Byzantine-envelope/runtime BFT admission.
pub const CAPABILITY_BYZANTINE_ENVELOPE: &str = "byzantine_envelope";
/// Capability key for bounded-termination runtime admission.
pub const CAPABILITY_TERMINATION_BOUNDED: &str = "termination_bounded";
/// Capability key for protocol reconfiguration admission.
pub const CAPABILITY_RECONFIGURATION: &str = "reconfiguration";
/// Capability key for mixed-determinism lane admission.
pub const CAPABILITY_MIXED_DETERMINISM: &str = "mixed_determinism";

/// Aura protocol id for consensus choreography admission.
pub const PROTOCOL_AURA_CONSENSUS: &str = "aura.consensus";
/// Aura protocol id for sync epoch-rotation choreography admission.
pub const PROTOCOL_SYNC_EPOCH_ROTATION: &str = "aura.sync.epoch_rotation";
/// Aura protocol id for DKG ceremony execution.
pub const PROTOCOL_DKG_CEREMONY: &str = "aura.dkg.ceremony";
/// Aura protocol id for guardian recovery-grant choreography.
pub const PROTOCOL_RECOVERY_GRANT: &str = "aura.recovery.grant";
/// Aura protocol id for guardian-auth relational choreography.
pub const PROTOCOL_GUARDIAN_AUTH_RELATIONAL: &str = "aura.authentication.guardian_auth_relational";
/// Aura protocol id for AMP transport choreography.
pub const PROTOCOL_AMP_TRANSPORT: &str = "aura.amp.transport";
/// Aura protocol id for invitation exchange choreography.
pub const PROTOCOL_INVITATION_EXCHANGE: &str = "aura.invitation.exchange";
/// Aura protocol id for guardian invitation choreography.
pub const PROTOCOL_GUARDIAN_INVITATION: &str = "aura.invitation.guardian";
/// Aura protocol id for device enrollment invitation choreography.
pub const PROTOCOL_DEVICE_ENROLLMENT: &str = "aura.invitation.device_enrollment";
/// Aura protocol id for direct rendezvous choreography.
pub const PROTOCOL_RENDEZVOUS_EXCHANGE: &str = "aura.rendezvous.exchange";
/// Aura protocol id for relayed rendezvous choreography.
pub const PROTOCOL_RENDEZVOUS_RELAY: &str = "aura.rendezvous.relay";
/// Aura protocol id for guardian ceremony choreography.
pub const PROTOCOL_GUARDIAN_CEREMONY: &str = "aura.recovery.guardian_ceremony";
/// Aura protocol id for guardian setup choreography.
pub const PROTOCOL_GUARDIAN_SETUP: &str = "aura.recovery.guardian_setup";
/// Aura protocol id for guardian membership change choreography.
pub const PROTOCOL_GUARDIAN_MEMBERSHIP_CHANGE: &str = "aura.recovery.guardian_membership_change";
/// Aura protocol id for session coordination choreography.
pub const PROTOCOL_SESSION_COORDINATION: &str = "aura.session.coordination";

/// Admission metadata for one production protocol id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtocolAdmissionProfile {
    /// Runtime capability artifacts required before execution.
    pub required_artifacts: &'static [&'static str],
}

/// Resolve explicit admission metadata for one production protocol id.
#[must_use]
pub fn protocol_admission_profile(protocol_id: &str) -> Option<ProtocolAdmissionProfile> {
    match protocol_id {
        PROTOCOL_AURA_CONSENSUS => Some(ProtocolAdmissionProfile {
            required_artifacts: &[CAPABILITY_BYZANTINE_ENVELOPE],
        }),
        PROTOCOL_SYNC_EPOCH_ROTATION => Some(ProtocolAdmissionProfile {
            required_artifacts: &[CAPABILITY_TERMINATION_BOUNDED],
        }),
        PROTOCOL_DKG_CEREMONY => Some(ProtocolAdmissionProfile {
            required_artifacts: &[
                CAPABILITY_BYZANTINE_ENVELOPE,
                CAPABILITY_TERMINATION_BOUNDED,
            ],
        }),
        PROTOCOL_RECOVERY_GRANT => Some(ProtocolAdmissionProfile {
            required_artifacts: &[CAPABILITY_TERMINATION_BOUNDED],
        }),
        PROTOCOL_GUARDIAN_AUTH_RELATIONAL
        | PROTOCOL_AMP_TRANSPORT
        | PROTOCOL_INVITATION_EXCHANGE
        | PROTOCOL_GUARDIAN_INVITATION
        | PROTOCOL_DEVICE_ENROLLMENT
        | PROTOCOL_RENDEZVOUS_EXCHANGE
        | PROTOCOL_RENDEZVOUS_RELAY
        | PROTOCOL_GUARDIAN_CEREMONY
        | PROTOCOL_GUARDIAN_SETUP
        | PROTOCOL_GUARDIAN_MEMBERSHIP_CHANGE
        | PROTOCOL_SESSION_COORDINATION => Some(ProtocolAdmissionProfile {
            required_artifacts: &[],
        }),
        _ => None,
    }
}

/// Consensus runtime-capability profiles used for theorem-pack admission checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsensusCapabilityProfile {
    /// Fast-path consensus profile.
    FastPath,
    /// Fallback/gossip recovery profile.
    FallbackPath,
    /// Threshold-signing profile (DKG + commit signing).
    ThresholdSigning,
}

/// Required capability/artifact identifiers for a protocol execution.
#[must_use]
pub fn required_artifacts(protocol_id: &str) -> &'static [&'static str] {
    protocol_admission_profile(protocol_id)
        .map(|profile| profile.required_artifacts)
        .unwrap_or(&[])
}

/// Required capability keys converted to core admission types.
#[must_use]
pub fn required_capability_keys(protocol_id: &str) -> Vec<CapabilityKey> {
    required_artifacts(protocol_id)
        .iter()
        .map(|capability| CapabilityKey::new(*capability))
        .collect()
}

/// Reject long-running protocols if bounded-termination evidence is unavailable.
pub fn validate_termination_artifact_requirement(
    protocol_id: &str,
    capability_inventory: &[(CapabilityKey, bool)],
) -> Result<(), AdmissionError> {
    let Some(class) = TerminationProtocolClass::from_protocol_id(protocol_id) else {
        return Ok(());
    };
    if !class.requires_termination_artifact() {
        return Ok(());
    }

    let required = CapabilityKey::new(CAPABILITY_TERMINATION_BOUNDED);
    let admitted = capability_inventory
        .iter()
        .find(|(key, _)| key == &required)
        .is_some_and(|(_, admitted)| *admitted);
    if admitted {
        Ok(())
    } else {
        Err(AdmissionError::MissingCapability {
            capability: required,
        })
    }
}

/// Required capability keys for one consensus profile.
#[must_use]
pub fn required_consensus_profile_capabilities(
    profile: ConsensusCapabilityProfile,
) -> Vec<CapabilityKey> {
    match profile {
        ConsensusCapabilityProfile::FastPath => vec![
            CapabilityKey::new(CAPABILITY_BYZANTINE_ENVELOPE),
            CapabilityKey::new(CAPABILITY_MIXED_DETERMINISM),
        ],
        ConsensusCapabilityProfile::FallbackPath => vec![
            CapabilityKey::new(CAPABILITY_BYZANTINE_ENVELOPE),
            CapabilityKey::new(CAPABILITY_TERMINATION_BOUNDED),
        ],
        ConsensusCapabilityProfile::ThresholdSigning => {
            vec![CapabilityKey::new(CAPABILITY_BYZANTINE_ENVELOPE)]
        }
    }
}

/// Validate that a capability inventory satisfies a consensus runtime profile.
pub fn validate_consensus_profile_capabilities(
    profile: ConsensusCapabilityProfile,
    capability_inventory: &[(CapabilityKey, bool)],
) -> Result<(), AdmissionError> {
    for required in required_consensus_profile_capabilities(profile) {
        let admitted = capability_inventory
            .iter()
            .find(|(key, _)| key == &required)
            .is_some_and(|(_, admitted)| *admitted);
        if !admitted {
            return Err(AdmissionError::MissingCapability {
                capability: required,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consensus_profile_validation_fails_when_required_capability_missing() {
        let inventory = vec![
            (CapabilityKey::new(CAPABILITY_BYZANTINE_ENVELOPE), true),
            (CapabilityKey::new(CAPABILITY_MIXED_DETERMINISM), false),
        ];
        let result = validate_consensus_profile_capabilities(
            ConsensusCapabilityProfile::FastPath,
            &inventory,
        );
        assert!(matches!(
            result,
            Err(AdmissionError::MissingCapability { capability })
                if capability == CapabilityKey::new(CAPABILITY_MIXED_DETERMINISM)
        ));
    }

    #[test]
    fn long_running_protocol_requires_termination_artifact() {
        let inventory = vec![(CapabilityKey::new(CAPABILITY_TERMINATION_BOUNDED), false)];
        let result =
            validate_termination_artifact_requirement(PROTOCOL_SYNC_EPOCH_ROTATION, &inventory);
        assert!(matches!(
            result,
            Err(AdmissionError::MissingCapability { capability })
                if capability == CapabilityKey::new(CAPABILITY_TERMINATION_BOUNDED)
        ));
    }

    #[test]
    fn admission_profiles_cover_all_production_protocol_ids() {
        let known = [
            PROTOCOL_AURA_CONSENSUS,
            PROTOCOL_AMP_TRANSPORT,
            PROTOCOL_DKG_CEREMONY,
            PROTOCOL_GUARDIAN_AUTH_RELATIONAL,
            PROTOCOL_INVITATION_EXCHANGE,
            PROTOCOL_GUARDIAN_INVITATION,
            PROTOCOL_DEVICE_ENROLLMENT,
            PROTOCOL_RECOVERY_GRANT,
            PROTOCOL_GUARDIAN_CEREMONY,
            PROTOCOL_GUARDIAN_SETUP,
            PROTOCOL_GUARDIAN_MEMBERSHIP_CHANGE,
            PROTOCOL_RENDEZVOUS_EXCHANGE,
            PROTOCOL_RENDEZVOUS_RELAY,
            PROTOCOL_SESSION_COORDINATION,
            PROTOCOL_SYNC_EPOCH_ROTATION,
        ];

        for protocol_id in known {
            assert!(
                protocol_admission_profile(protocol_id).is_some(),
                "missing explicit admission profile for {protocol_id}"
            );
        }

        assert!(protocol_admission_profile("aura.unknown").is_none());
    }
}

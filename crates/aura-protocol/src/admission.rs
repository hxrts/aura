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
    match protocol_id {
        PROTOCOL_AURA_CONSENSUS => &[CAPABILITY_BYZANTINE_ENVELOPE],
        PROTOCOL_SYNC_EPOCH_ROTATION => &[CAPABILITY_TERMINATION_BOUNDED],
        PROTOCOL_DKG_CEREMONY => &[
            CAPABILITY_BYZANTINE_ENVELOPE,
            CAPABILITY_TERMINATION_BOUNDED,
        ],
        PROTOCOL_RECOVERY_GRANT => &[CAPABILITY_TERMINATION_BOUNDED],
        _ => &[],
    }
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
}

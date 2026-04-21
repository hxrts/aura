//! Protocol runtime-capability admission requirements.
//!
//! This module declares theorem-pack/runtime capability requirements for
//! choreography bundles before execution.

use crate::termination::TerminationProtocolClass;
use aura_core::effects::{AdmissionError, CapabilityKey};
use aura_mpst::CompositionManifest;
use std::collections::BTreeSet;

/// Capability key for Byzantine-envelope/runtime BFT admission.
pub const CAPABILITY_BYZANTINE_ENVELOPE: &str = "byzantine_envelope";
/// Capability key for bounded-termination runtime admission.
pub const CAPABILITY_TERMINATION_BOUNDED: &str = "termination_bounded";
/// Capability key for protocol reconfiguration admission.
pub const CAPABILITY_RECONFIGURATION: &str = "reconfiguration";
/// Capability key for mixed-determinism lane admission.
pub const CAPABILITY_MIXED_DETERMINISM: &str = "mixed_determinism";
/// Protocol-critical surface proving theorem-pack capability admission is active.
pub const CAPABILITY_THEOREM_PACK_CAPABILITIES: &str = "theorem_pack_capabilities";
/// Protocol-critical surface for authoritative-read support.
pub const CAPABILITY_AUTHORITATIVE_READ: &str = "authoritative_read";
/// Protocol-critical surface for materialization-proof support.
pub const CAPABILITY_MATERIALIZATION_PROOF: &str = "materialization_proof";
/// Protocol-critical surface for canonical-handle support.
pub const CAPABILITY_CANONICAL_HANDLE: &str = "canonical_handle";
/// Protocol-critical surface for ownership-receipt support.
pub const CAPABILITY_OWNERSHIP_RECEIPT: &str = "ownership_receipt";
/// Protocol-critical surface for semantic-handoff support.
pub const CAPABILITY_SEMANTIC_HANDOFF: &str = "semantic_handoff";
/// Protocol-critical surface for reconfiguration-transition support.
pub const CAPABILITY_RECONFIGURATION_TRANSITION: &str = "reconfiguration_transition";
/// Telltale theorem-pack capability for protocol-machine envelope adherence.
pub const CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADHERENCE: &str =
    "protocol_machine_envelope_adherence";
/// Telltale theorem-pack capability for protocol-machine admission.
pub const CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADMISSION: &str =
    "protocol_machine_envelope_admission";
/// Telltale theorem-pack capability for envelope bridge support.
pub const CAPABILITY_PROTOCOL_ENVELOPE_BRIDGE: &str = "protocol_envelope_bridge";
/// Telltale theorem-pack capability for reconfiguration safety.
pub const CAPABILITY_RECONFIGURATION_SAFETY: &str = "reconfiguration_safety";

/// Aura theorem pack for transition/reconfiguration safety.
pub const THEOREM_PACK_AURA_TRANSITION_SAFETY: &str = "AuraTransitionSafety";
/// Aura theorem pack for authority/evidence-backed admission.
pub const THEOREM_PACK_AURA_AUTHORITY_EVIDENCE: &str = "AuraAuthorityEvidence";
/// Aura theorem pack reserved for future consensus deployment gating.
pub const THEOREM_PACK_AURA_CONSENSUS_DEPLOYMENT: &str = "AuraConsensusDeployment";

/// Aura protocol id for consensus choreography admission.
pub const PROTOCOL_AURA_CONSENSUS: &str = "aura.consensus";
/// Aura protocol id for sync epoch-rotation choreography admission.
pub const PROTOCOL_SYNC_EPOCH_ROTATION: &str = "aura.sync.epoch_rotation";
/// Aura protocol id for OTA activation choreography admission.
pub const PROTOCOL_SYNC_OTA_ACTIVATION: &str = "aura.sync.ota_activation";
/// Aura protocol id for device-scoped epoch rotation choreography.
pub const PROTOCOL_SYNC_DEVICE_EPOCH_ROTATION: &str = "aura.sync.device_epoch_rotation";
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

/// One Aura-owned theorem-pack admission policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TheoremPackAdmissionProfile {
    /// Capability names that must appear in the choreography's proof-bundle declaration.
    pub declared_capabilities: &'static [&'static str],
    /// Runtime-admission capabilities Aura requires before launch.
    pub required_runtime_capabilities: &'static [&'static str],
}

/// One resolved theorem-pack admission requirement for a concrete manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTheoremPackRequirement {
    /// Required theorem-pack name.
    pub theorem_pack: String,
    /// Capability names declared in choreography metadata for this pack.
    pub declared_capabilities: Vec<String>,
    /// Aura runtime capabilities that must be present before launch.
    pub required_runtime_capabilities: Vec<CapabilityKey>,
}

/// Resolved manifest admission requirements including theorem-pack gating.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestAdmissionRequirements {
    /// Plain protocol-level runtime requirements independent of theorem packs.
    pub required_runtime_capabilities: Vec<CapabilityKey>,
    /// Per-pack admission requirements derived from theorem-pack metadata.
    pub theorem_pack_requirements: Vec<ResolvedTheoremPackRequirement>,
}

fn capability_key(capability: &str) -> CapabilityKey {
    CapabilityKey::new(capability)
}

fn capability_keys<'a>(capabilities: impl IntoIterator<Item = &'a str>) -> Vec<CapabilityKey> {
    capabilities.into_iter().map(capability_key).collect()
}

fn capability_name_set<'a>(capabilities: impl IntoIterator<Item = &'a str>) -> BTreeSet<String> {
    capabilities.into_iter().map(str::to_string).collect()
}

fn has_admitted_capability(
    capability_inventory: &[(CapabilityKey, bool)],
    required: &CapabilityKey,
) -> bool {
    capability_inventory
        .iter()
        .find(|(key, _)| key == required)
        .is_some_and(|(_, admitted)| *admitted)
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
        PROTOCOL_SYNC_OTA_ACTIVATION => Some(ProtocolAdmissionProfile {
            required_artifacts: &[],
        }),
        PROTOCOL_SYNC_DEVICE_EPOCH_ROTATION => Some(ProtocolAdmissionProfile {
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

/// Resolve one Aura-owned theorem-pack admission profile.
#[must_use]
pub fn theorem_pack_admission_profile(theorem_pack: &str) -> Option<TheoremPackAdmissionProfile> {
    match theorem_pack {
        THEOREM_PACK_AURA_TRANSITION_SAFETY => Some(TheoremPackAdmissionProfile {
            declared_capabilities: &[
                CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADHERENCE,
                CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADMISSION,
                CAPABILITY_PROTOCOL_ENVELOPE_BRIDGE,
                CAPABILITY_RECONFIGURATION_SAFETY,
            ],
            required_runtime_capabilities: &[
                CAPABILITY_THEOREM_PACK_CAPABILITIES,
                CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADHERENCE,
                CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADMISSION,
                CAPABILITY_PROTOCOL_ENVELOPE_BRIDGE,
                CAPABILITY_RECONFIGURATION_SAFETY,
                CAPABILITY_OWNERSHIP_RECEIPT,
                CAPABILITY_SEMANTIC_HANDOFF,
                CAPABILITY_RECONFIGURATION_TRANSITION,
            ],
        }),
        THEOREM_PACK_AURA_AUTHORITY_EVIDENCE => Some(TheoremPackAdmissionProfile {
            declared_capabilities: &[
                CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADHERENCE,
                CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADMISSION,
                CAPABILITY_PROTOCOL_ENVELOPE_BRIDGE,
            ],
            required_runtime_capabilities: &[
                CAPABILITY_THEOREM_PACK_CAPABILITIES,
                CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADHERENCE,
                CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADMISSION,
                CAPABILITY_PROTOCOL_ENVELOPE_BRIDGE,
                CAPABILITY_AUTHORITATIVE_READ,
                CAPABILITY_MATERIALIZATION_PROOF,
                CAPABILITY_CANONICAL_HANDLE,
            ],
        }),
        THEOREM_PACK_AURA_CONSENSUS_DEPLOYMENT => None,
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
    capability_keys(required_artifacts(protocol_id).iter().copied())
}

/// Resolve combined runtime admission requirements for one generated manifest.
pub fn manifest_admission_requirements(
    manifest: &CompositionManifest,
) -> Result<ManifestAdmissionRequirements, AdmissionError> {
    let required_runtime_capabilities = manifest
        .required_capabilities
        .iter()
        .map(|capability| capability_key(capability.as_str()))
        .collect::<BTreeSet<_>>();
    let mut theorem_pack_requirements = Vec::new();
    let mut resolved_required_theorem_pack_capabilities = BTreeSet::new();

    for required_theorem_pack in &manifest.required_theorem_packs {
        let declaration = manifest
            .theorem_packs
            .iter()
            .find(|pack| pack.name == *required_theorem_pack)
            .ok_or_else(|| AdmissionError::MissingTheoremPack {
                theorem_pack: required_theorem_pack.clone(),
            })?;
        let profile = theorem_pack_admission_profile(required_theorem_pack).ok_or_else(|| {
            AdmissionError::MissingTheoremPack {
                theorem_pack: required_theorem_pack.clone(),
            }
        })?;

        let declared_capabilities = declaration
            .capabilities
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let expected_declared_capabilities = profile.declared_capabilities.iter().copied();
        let expected_declared_capabilities = capability_name_set(expected_declared_capabilities);
        if declared_capabilities != expected_declared_capabilities {
            return Err(AdmissionError::Internal {
                reason: format!(
                    "theorem-pack declaration `{required_theorem_pack}` drifted from Aura admission policy"
                ),
            });
        }

        resolved_required_theorem_pack_capabilities.extend(declared_capabilities.iter().cloned());

        let pack_runtime_capabilities = profile.required_runtime_capabilities.iter().copied();
        let pack_runtime_capabilities = capability_keys(pack_runtime_capabilities);
        theorem_pack_requirements.push(ResolvedTheoremPackRequirement {
            theorem_pack: required_theorem_pack.clone(),
            declared_capabilities: declaration.capabilities.clone(),
            required_runtime_capabilities: pack_runtime_capabilities,
        });
    }

    let declared_required_theorem_pack_capabilities = manifest
        .required_theorem_pack_capabilities
        .iter()
        .map(String::as_str);
    let declared_required_theorem_pack_capabilities =
        capability_name_set(declared_required_theorem_pack_capabilities);
    if declared_required_theorem_pack_capabilities != resolved_required_theorem_pack_capabilities {
        return Err(AdmissionError::Internal {
            reason: format!(
                "required theorem-pack capability metadata drifted for protocol {}",
                manifest.protocol_id
            ),
        });
    }

    Ok(ManifestAdmissionRequirements {
        required_runtime_capabilities: required_runtime_capabilities.into_iter().collect(),
        theorem_pack_requirements,
    })
}

/// Resolve the flattened runtime capability set for one generated manifest.
pub fn required_capability_keys_for_manifest(
    manifest: &CompositionManifest,
) -> Result<Vec<CapabilityKey>, AdmissionError> {
    let requirements = manifest_admission_requirements(manifest)?;
    let mut combined = requirements
        .required_runtime_capabilities
        .into_iter()
        .collect::<BTreeSet<_>>();
    for theorem_pack in requirements.theorem_pack_requirements {
        combined.extend(theorem_pack.required_runtime_capabilities);
    }
    Ok(combined.into_iter().collect())
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

    let required = capability_key(CAPABILITY_TERMINATION_BOUNDED);
    let admitted = has_admitted_capability(capability_inventory, &required);
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
        ConsensusCapabilityProfile::FastPath => {
            capability_keys([CAPABILITY_BYZANTINE_ENVELOPE, CAPABILITY_MIXED_DETERMINISM])
        }
        ConsensusCapabilityProfile::FallbackPath => capability_keys([
            CAPABILITY_BYZANTINE_ENVELOPE,
            CAPABILITY_TERMINATION_BOUNDED,
        ]),
        ConsensusCapabilityProfile::ThresholdSigning => {
            capability_keys([CAPABILITY_BYZANTINE_ENVELOPE])
        }
    }
}

/// Validate that a capability inventory satisfies a consensus runtime profile.
pub fn validate_consensus_profile_capabilities(
    profile: ConsensusCapabilityProfile,
    capability_inventory: &[(CapabilityKey, bool)],
) -> Result<(), AdmissionError> {
    for required in required_consensus_profile_capabilities(profile) {
        if !has_admitted_capability(capability_inventory, &required) {
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
    use aura_mpst::{CompositionManifest, CompositionTheoremPack};

    fn manifest_with_theorem_packs(
        theorem_packs: Vec<CompositionTheoremPack>,
        required_theorem_packs: Vec<&str>,
        required_theorem_pack_capabilities: Vec<&str>,
    ) -> CompositionManifest {
        CompositionManifest {
            protocol_name: "Test".to_string(),
            protocol_namespace: Some("test".to_string()),
            protocol_qualified_name: "test.Test".to_string(),
            protocol_id: "test.protocol".to_string(),
            role_names: vec!["Alice".to_string(), "Bob".to_string()],
            required_capabilities: Vec::new(),
            theorem_packs,
            required_theorem_packs: required_theorem_packs
                .into_iter()
                .map(str::to_string)
                .collect(),
            required_theorem_pack_capabilities: required_theorem_pack_capabilities
                .into_iter()
                .map(str::to_string)
                .collect(),
            guard_capabilities: Vec::new(),
            determinism_policy_ref: Some("aura.vm.prod.default".to_string()),
            link_specs: Vec::new(),
            delegation_constraints: Vec::new(),
        }
    }

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
            PROTOCOL_SYNC_DEVICE_EPOCH_ROTATION,
        ];

        for protocol_id in known {
            assert!(
                protocol_admission_profile(protocol_id).is_some(),
                "missing explicit admission profile for {protocol_id}"
            );
        }

        assert!(protocol_admission_profile("aura.unknown").is_none());
    }

    #[test]
    fn manifest_admission_fails_when_required_theorem_pack_is_missing() {
        let manifest = manifest_with_theorem_packs(
            Vec::new(),
            vec![THEOREM_PACK_AURA_TRANSITION_SAFETY],
            Vec::new(),
        );
        let result = manifest_admission_requirements(&manifest);
        assert!(matches!(
            result,
            Err(AdmissionError::MissingTheoremPack { theorem_pack })
                if theorem_pack == THEOREM_PACK_AURA_TRANSITION_SAFETY
        ));
    }

    #[test]
    fn manifest_admission_resolves_transition_safety_requirements() {
        let manifest = manifest_with_theorem_packs(
            vec![CompositionTheoremPack {
                name: THEOREM_PACK_AURA_TRANSITION_SAFETY.to_string(),
                capabilities: vec![
                    CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADHERENCE.to_string(),
                    CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADMISSION.to_string(),
                    CAPABILITY_PROTOCOL_ENVELOPE_BRIDGE.to_string(),
                    CAPABILITY_RECONFIGURATION_SAFETY.to_string(),
                ],
                version: Some("1.0.0".to_string()),
                issuer: None,
                constraints: vec!["fresh_nonce".to_string()],
            }],
            vec![THEOREM_PACK_AURA_TRANSITION_SAFETY],
            vec![
                CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADHERENCE,
                CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADMISSION,
                CAPABILITY_PROTOCOL_ENVELOPE_BRIDGE,
                CAPABILITY_RECONFIGURATION_SAFETY,
            ],
        );
        let resolved = match required_capability_keys_for_manifest(&manifest) {
            Ok(resolved) => resolved
                .into_iter()
                .map(|capability| capability.to_string())
                .collect::<BTreeSet<_>>(),
            Err(error) => panic!("transition pack must resolve: {error:?}"),
        };
        let expected = [
            CAPABILITY_THEOREM_PACK_CAPABILITIES,
            CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADHERENCE,
            CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADMISSION,
            CAPABILITY_PROTOCOL_ENVELOPE_BRIDGE,
            CAPABILITY_RECONFIGURATION_SAFETY,
            CAPABILITY_OWNERSHIP_RECEIPT,
            CAPABILITY_SEMANTIC_HANDOFF,
            CAPABILITY_RECONFIGURATION_TRANSITION,
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
        assert_eq!(resolved, expected);
    }
}

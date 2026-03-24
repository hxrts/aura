//! Typed runtime contracts for concurrency admission, link boundaries, and delegation witnesses.

use super::subsystems::choreography::SessionOwnerCapabilityScope;
use aura_core::{AuthorityId, ComposedBundle, ContextId, SessionId};
use aura_mpst::CompositionManifest;
use std::collections::BTreeSet;

#[cfg(feature = "choreo-backend-telltale-vm")]
use super::vm_hardening::{
    policy_requires_envelope_artifact, required_runtime_capabilities_for_policy,
    AuraVmConcurrencyProfile, AuraVmProtocolExecutionPolicy, AuraVmRuntimeMode,
};

/// Typed evidence kinds used to justify one runtime envelope admission decision.
#[cfg(feature = "choreo-backend-telltale-vm")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuraRuntimeAdmissionEvidenceKind {
    RuntimeCapability,
    EnvelopeArtifact,
    DeterminismProfile,
    RuntimeContract,
    CanonicalFallback,
}

/// One proof-facing evidence item attached to one envelope admission decision.
#[cfg(feature = "choreo-backend-telltale-vm")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuraRuntimeAdmissionEvidence {
    pub kind: AuraRuntimeAdmissionEvidenceKind,
    pub ref_id: String,
    pub details: String,
}

#[cfg(feature = "choreo-backend-telltale-vm")]
impl AuraRuntimeAdmissionEvidence {
    pub fn new(
        kind: AuraRuntimeAdmissionEvidenceKind,
        ref_id: impl Into<String>,
        details: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            ref_id: ref_id.into(),
            details: details.into(),
        }
    }
}

/// Typed runtime admission contract for one VM execution profile.
#[cfg(feature = "choreo-backend-telltale-vm")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuraRuntimeEnvelopeAdmission {
    pub protocol_id: String,
    pub requested_policy_ref: String,
    pub effective_policy_ref: String,
    pub requested_profile: AuraVmConcurrencyProfile,
    pub effective_profile: AuraVmConcurrencyProfile,
    pub requested_runtime_mode: AuraVmRuntimeMode,
    pub effective_runtime_mode: AuraVmRuntimeMode,
    pub evidence: Vec<AuraRuntimeAdmissionEvidence>,
    pub canonical_fallback_reason: Option<&'static str>,
}

#[cfg(feature = "choreo-backend-telltale-vm")]
impl AuraRuntimeEnvelopeAdmission {
    pub fn from_policy(
        protocol_id: impl Into<String>,
        requested_policy: AuraVmProtocolExecutionPolicy,
        effective_policy: AuraVmProtocolExecutionPolicy,
    ) -> Self {
        let mut evidence = vec![
            AuraRuntimeAdmissionEvidence::new(
                AuraRuntimeAdmissionEvidenceKind::DeterminismProfile,
                requested_policy.policy_ref,
                format!(
                    "requested profile={} runtime_mode={}",
                    requested_policy.concurrency_profile().as_ref(),
                    requested_policy.runtime_mode.as_ref()
                ),
            ),
            AuraRuntimeAdmissionEvidence::new(
                AuraRuntimeAdmissionEvidenceKind::RuntimeContract,
                effective_policy.policy_ref,
                format!(
                    "effective profile={} runtime_mode={}",
                    effective_policy.concurrency_profile().as_ref(),
                    effective_policy.runtime_mode.as_ref()
                ),
            ),
        ];

        for capability in required_runtime_capabilities_for_policy(requested_policy) {
            evidence.push(AuraRuntimeAdmissionEvidence::new(
                AuraRuntimeAdmissionEvidenceKind::RuntimeCapability,
                *capability,
                "required runtime capability for requested profile",
            ));
        }

        if policy_requires_envelope_artifact(requested_policy) {
            evidence.push(AuraRuntimeAdmissionEvidence::new(
                AuraRuntimeAdmissionEvidenceKind::EnvelopeArtifact,
                requested_policy.policy_ref,
                "requested policy requires an envelope artifact witness",
            ));
        }

        let canonical_fallback_reason =
            (requested_policy != effective_policy).then_some("host_bridge_canonical_only");
        if let Some(reason) = canonical_fallback_reason {
            evidence.push(AuraRuntimeAdmissionEvidence::new(
                AuraRuntimeAdmissionEvidenceKind::CanonicalFallback,
                effective_policy.policy_ref,
                reason,
            ));
        }

        Self {
            protocol_id: protocol_id.into(),
            requested_policy_ref: requested_policy.policy_ref.to_string(),
            effective_policy_ref: effective_policy.policy_ref.to_string(),
            requested_profile: requested_policy.concurrency_profile(),
            effective_profile: effective_policy.concurrency_profile(),
            requested_runtime_mode: requested_policy.runtime_mode,
            effective_runtime_mode: effective_policy.runtime_mode,
            evidence,
            canonical_fallback_reason,
        }
    }

    pub fn effective_policy_ref(&self) -> &str {
        &self.effective_policy_ref
    }

    pub fn activated_fallback(&self) -> bool {
        self.canonical_fallback_reason.is_some()
    }
}

/// Explicit runtime routing/capability boundary for one linked protocol/bundle fragment set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuraLinkBoundary {
    pub bundle_id: Option<String>,
    pub protocol_ids: BTreeSet<String>,
    pub fragment_keys: BTreeSet<String>,
    pub capability_scope: SessionOwnerCapabilityScope,
}

impl AuraLinkBoundary {
    pub fn for_scope(scope: SessionOwnerCapabilityScope) -> Self {
        match scope {
            SessionOwnerCapabilityScope::Session => Self {
                bundle_id: None,
                protocol_ids: BTreeSet::new(),
                fragment_keys: BTreeSet::new(),
                capability_scope: SessionOwnerCapabilityScope::Session,
            },
            SessionOwnerCapabilityScope::Fragments(fragment_keys) => Self {
                bundle_id: None,
                protocol_ids: BTreeSet::new(),
                fragment_keys: fragment_keys.clone(),
                capability_scope: SessionOwnerCapabilityScope::Fragments(fragment_keys),
            },
        }
    }

    pub fn for_protocol(protocol_id: impl Into<String>) -> Self {
        let protocol_id = protocol_id.into();
        let fragment_keys = BTreeSet::from([format!("protocol:{protocol_id}")]);
        Self {
            bundle_id: None,
            protocol_ids: BTreeSet::from([protocol_id]),
            fragment_keys,
            capability_scope: SessionOwnerCapabilityScope::Session,
        }
    }

    pub fn for_bundle(bundle: &ComposedBundle) -> Self {
        let fragment_keys = BTreeSet::from([format!("bundle:{}", bundle.bundle_id)]);
        Self {
            bundle_id: Some(bundle.bundle_id.clone()),
            protocol_ids: bundle.protocol_ids.iter().cloned().collect(),
            fragment_keys: fragment_keys.clone(),
            capability_scope: SessionOwnerCapabilityScope::Fragments(fragment_keys),
        }
    }

    pub fn for_bundle_id(bundle_id: impl Into<String>) -> Self {
        let bundle_id = bundle_id.into();
        let fragment_keys = BTreeSet::from([format!("bundle:{bundle_id}")]);
        Self {
            bundle_id: Some(bundle_id),
            protocol_ids: BTreeSet::new(),
            fragment_keys: fragment_keys.clone(),
            capability_scope: SessionOwnerCapabilityScope::Fragments(fragment_keys),
        }
    }

    pub fn from_manifest(manifest: &CompositionManifest) -> Vec<Self> {
        let bundle_ids = manifest
            .link_specs
            .iter()
            .map(|spec| spec.bundle_id.clone())
            .collect::<BTreeSet<_>>();
        if bundle_ids.is_empty() {
            return vec![Self::for_protocol(manifest.protocol_id.clone())];
        }

        bundle_ids
            .into_iter()
            .map(|bundle_id| {
                let fragment_keys = BTreeSet::from([format!("bundle:{bundle_id}")]);
                Self {
                    bundle_id: Some(bundle_id),
                    protocol_ids: BTreeSet::from([manifest.protocol_id.clone()]),
                    fragment_keys: fragment_keys.clone(),
                    capability_scope: SessionOwnerCapabilityScope::Fragments(fragment_keys),
                }
            })
            .collect()
    }

    pub fn for_manifest(manifest: &CompositionManifest) -> Self {
        let mut boundaries = Self::from_manifest(manifest);
        if boundaries.len() == 1 {
            return boundaries.pop().expect("single boundary");
        }
        if boundaries.is_empty() {
            return Self::for_protocol(manifest.protocol_id.clone());
        }

        let fragment_keys = boundaries
            .iter()
            .flat_map(|boundary| boundary.fragment_keys.iter().cloned())
            .collect::<BTreeSet<_>>();
        let protocol_ids = boundaries
            .iter()
            .flat_map(|boundary| boundary.protocol_ids.iter().cloned())
            .collect::<BTreeSet<_>>();

        Self {
            bundle_id: None,
            protocol_ids,
            fragment_keys: fragment_keys.clone(),
            capability_scope: SessionOwnerCapabilityScope::Fragments(fragment_keys),
        }
    }

    pub fn matches_fragment_key(&self, fragment_key: &str) -> bool {
        matches!(&self.capability_scope, SessionOwnerCapabilityScope::Session)
            || self.fragment_keys.contains(fragment_key)
    }

    pub fn is_allowed_by(&self, scope: &SessionOwnerCapabilityScope) -> bool {
        match scope {
            SessionOwnerCapabilityScope::Session => true,
            SessionOwnerCapabilityScope::Fragments(allowed) => {
                !self.fragment_keys.is_empty()
                    && self
                        .fragment_keys
                        .iter()
                        .all(|fragment_key| allowed.contains(fragment_key))
            }
        }
    }
}

/// Coherence/harmony status captured with one delegation witness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuraDelegationCoherence {
    Pending,
    Preserved,
    Violations(Vec<String>),
}

/// Auditable witness for one delegation operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuraDelegationWitness {
    pub context_id: ContextId,
    pub session_id: SessionId,
    pub from_authority: AuthorityId,
    pub to_authority: AuthorityId,
    pub bundle_id: String,
    pub link_boundary: AuraLinkBoundary,
    pub capability_scope: SessionOwnerCapabilityScope,
    pub moved_fragment_keys: BTreeSet<String>,
    pub coherence: AuraDelegationCoherence,
}

impl AuraDelegationWitness {
    pub fn new(
        context_id: ContextId,
        session_id: SessionId,
        from_authority: AuthorityId,
        to_authority: AuthorityId,
        bundle_id: impl Into<String>,
        link_boundary: AuraLinkBoundary,
        capability_scope: SessionOwnerCapabilityScope,
    ) -> Self {
        let moved_fragment_keys = link_boundary.fragment_keys.clone();
        Self {
            context_id,
            session_id,
            from_authority,
            to_authority,
            bundle_id: bundle_id.into(),
            link_boundary,
            capability_scope,
            moved_fragment_keys,
            coherence: AuraDelegationCoherence::Pending,
        }
    }

    pub fn with_coherence(mut self, coherence: AuraDelegationCoherence) -> Self {
        self.coherence = coherence;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_mpst::CompositionLinkSpec;
    use uuid::Uuid;

    #[cfg(feature = "choreo-backend-telltale-vm")]
    #[test]
    fn envelope_admission_records_fallback_evidence() {
        let requested =
            super::super::vm_hardening::policy_for_protocol("aura.sync.epoch_rotation", None)
                .expect("policy resolves");
        let effective = requested.canonical_fallback_policy();
        let admission =
            AuraRuntimeEnvelopeAdmission::from_policy("aura.test.protocol", requested, effective);

        assert!(admission.activated_fallback());
        assert!(admission
            .evidence
            .iter()
            .any(|e| e.kind == AuraRuntimeAdmissionEvidenceKind::CanonicalFallback));
    }

    #[test]
    fn manifest_with_no_links_yields_protocol_boundary() {
        let manifest = CompositionManifest {
            protocol_name: "aura.test.protocol".to_string(),
            protocol_namespace: None,
            protocol_qualified_name: "aura.test.protocol".to_string(),
            protocol_id: "aura.test.protocol".to_string(),
            role_names: vec!["A".to_string()],
            required_capabilities: Vec::new(),
            guard_capabilities: Vec::new(),
            determinism_policy_ref: None,
            delegation_constraints: Vec::new(),
            link_specs: vec![],
        };

        let boundaries = AuraLinkBoundary::from_manifest(&manifest);
        assert_eq!(boundaries.len(), 1);
        assert!(matches!(
            boundaries[0].capability_scope,
            SessionOwnerCapabilityScope::Session
        ));
    }

    #[test]
    fn manifest_with_links_yields_fragment_scoped_boundaries() {
        let manifest = CompositionManifest {
            protocol_name: "aura.test.protocol".to_string(),
            protocol_namespace: None,
            protocol_qualified_name: "aura.test.protocol".to_string(),
            protocol_id: "aura.test.protocol".to_string(),
            role_names: vec!["A".to_string()],
            required_capabilities: Vec::new(),
            guard_capabilities: Vec::new(),
            determinism_policy_ref: None,
            delegation_constraints: Vec::new(),
            link_specs: vec![
                CompositionLinkSpec {
                    role: "A".to_string(),
                    bundle_id: "bundle-a".to_string(),
                    exports: Vec::new(),
                    imports: Vec::new(),
                },
                CompositionLinkSpec {
                    role: "A".to_string(),
                    bundle_id: "bundle-b".to_string(),
                    exports: Vec::new(),
                    imports: Vec::new(),
                },
            ],
        };

        let boundaries = AuraLinkBoundary::from_manifest(&manifest);
        assert_eq!(boundaries.len(), 2);
        assert!(boundaries.iter().all(|boundary| matches!(
            boundary.capability_scope,
            SessionOwnerCapabilityScope::Fragments(_)
        )));
    }

    #[test]
    fn delegation_witness_captures_boundary_and_scope() {
        let boundary = AuraLinkBoundary::for_protocol("aura.test.protocol");
        let witness = AuraDelegationWitness::new(
            ContextId::new_from_entropy([7; 32]),
            SessionId::from_uuid(Uuid::from_bytes([5; 16])),
            AuthorityId::from_uuid(Uuid::from_bytes([1; 16])),
            AuthorityId::from_uuid(Uuid::from_bytes([2; 16])),
            "bundle-a",
            boundary.clone(),
            SessionOwnerCapabilityScope::Session,
        );

        assert_eq!(witness.link_boundary, boundary);
        assert_eq!(witness.moved_fragment_keys, boundary.fragment_keys);
    }

    #[test]
    fn manifest_boundary_unions_multiple_link_scopes() {
        let manifest = CompositionManifest {
            protocol_name: "aura.test.protocol".to_string(),
            protocol_namespace: None,
            protocol_qualified_name: "aura.test.protocol".to_string(),
            protocol_id: "aura.test.protocol".to_string(),
            role_names: vec!["A".to_string()],
            required_capabilities: Vec::new(),
            guard_capabilities: Vec::new(),
            determinism_policy_ref: None,
            delegation_constraints: Vec::new(),
            link_specs: vec![
                CompositionLinkSpec {
                    role: "A".to_string(),
                    bundle_id: "bundle-a".to_string(),
                    exports: Vec::new(),
                    imports: Vec::new(),
                },
                CompositionLinkSpec {
                    role: "A".to_string(),
                    bundle_id: "bundle-b".to_string(),
                    exports: Vec::new(),
                    imports: Vec::new(),
                },
            ],
        };

        let boundary = AuraLinkBoundary::for_manifest(&manifest);
        assert!(matches!(
            boundary.capability_scope,
            SessionOwnerCapabilityScope::Fragments(_)
        ));
        assert!(boundary.fragment_keys.contains("bundle:bundle-a"));
        assert!(boundary.fragment_keys.contains("bundle:bundle-b"));
    }

    #[test]
    fn fragment_boundary_requires_matching_fragment_scope() {
        let boundary =
            AuraLinkBoundary::for_scope(SessionOwnerCapabilityScope::Fragments(BTreeSet::from([
                "bundle:bundle-a".to_string(),
            ])));
        let allowed_scope = SessionOwnerCapabilityScope::Fragments(BTreeSet::from([
            "bundle:bundle-a".to_string(),
            "bundle:bundle-b".to_string(),
        ]));
        let rejected_scope =
            SessionOwnerCapabilityScope::Fragments(BTreeSet::from(["bundle:bundle-b".to_string()]));

        assert!(boundary.is_allowed_by(&allowed_scope));
        assert!(!boundary.is_allowed_by(&rejected_scope));
        assert!(boundary.is_allowed_by(&SessionOwnerCapabilityScope::Session));
    }
}

//! Stateless OTA policy evaluation surfaces.

use aura_core::AuthorityId;
use aura_maintenance::{
    AuraActivationScope, AuraCompatibilityClass, AuraDeterministicBuildCertificate, AuraHealthGate,
    AuraPolicyScope, AuraReleaseActivationPolicy, AuraReleaseDiscoveryPolicy, AuraReleaseManifest,
    AuraReleaseSharingPolicy, PinPolicy, RecommendationPolicy,
};
use std::collections::BTreeSet;

/// Input to discovery-policy evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryCandidate {
    /// Release authority publishing the candidate.
    pub release_authority: AuthorityId,
    /// Builders that have certified the candidate so far.
    pub builder_authorities: BTreeSet<AuthorityId>,
    /// Policy scope through which the candidate was discovered.
    pub scope: AuraPolicyScope,
}

/// Decision produced by discovery-policy evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryDecision {
    /// Whether the candidate may be discovered at all.
    pub allow_discovery: bool,
    /// Whether metadata should be fetched automatically.
    pub fetch_metadata: bool,
    /// Whether artifacts should be fetched automatically.
    pub fetch_artifacts: bool,
    /// Optional denial reason.
    pub reason: Option<String>,
}

/// Input to sharing-policy evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharingCandidate {
    /// Scope in which redistribution is being considered.
    pub scope: AuraPolicyScope,
    /// Whether a manifest is present to forward.
    pub includes_manifest: bool,
    /// Whether artifacts are present to forward.
    pub includes_artifacts: bool,
    /// Whether certificates are present to forward.
    pub includes_certificates: bool,
    /// Whether the action would publish a recommendation.
    pub recommendation_target: Option<AuthorityId>,
}

/// Decision produced by sharing-policy evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharingDecision {
    /// Whether local pinning is desired.
    pub pin_bundle: bool,
    /// Whether manifests may be forwarded.
    pub forward_manifest: bool,
    /// Whether artifacts may be forwarded.
    pub forward_artifacts: bool,
    /// Whether certificates may be forwarded.
    pub forward_certificates: bool,
    /// Whether a recommendation may be published.
    pub publish_recommendation: bool,
}

/// Input to activation-policy evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationCandidate {
    /// Manifest under evaluation.
    pub manifest: AuraReleaseManifest,
    /// Build certificates currently available.
    pub certificates: Vec<AuraDeterministicBuildCertificate>,
    /// Scope in which activation is being considered.
    pub scope: AuraActivationScope,
    /// Mixed-version compatibility metadata for the target release.
    pub compatibility: AuraCompatibilityClass,
    /// Whether all required artifacts are staged locally.
    pub artifacts_staged: bool,
    /// Whether required threshold approval exists for this scope.
    pub threshold_approved: bool,
    /// Whether the required epoch fence has been satisfied for this scope.
    pub epoch_fence_satisfied: bool,
    /// Health gate outcomes collected for the scope.
    pub health_gates: Vec<AuraHealthGate>,
    /// Local Unix wall-clock time in milliseconds used for advisory-time evaluation.
    pub local_unix_time_ms: u64,
    /// Whether a local rebuild result is available.
    pub local_rebuild_available: bool,
}

/// Decision produced by activation-policy evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationDecision {
    /// Whether the candidate may be staged.
    pub allow_stage: bool,
    /// Whether the candidate may be activated immediately.
    pub allow_activate: bool,
    /// Optional denial reason.
    pub reason: Option<String>,
}

/// Stateless OTA policy evaluator.
#[derive(Debug, Clone, Copy, Default)]
pub struct OtaPolicyEvaluator;

impl OtaPolicyEvaluator {
    /// Create a new OTA policy evaluator.
    pub fn new() -> Self {
        Self
    }

    /// Evaluate discovery policy independently from sharing or activation.
    pub fn evaluate_discovery(
        &self,
        policy: &AuraReleaseDiscoveryPolicy,
        candidate: &DiscoveryCandidate,
    ) -> DiscoveryDecision {
        if !policy
            .allowed_release_sources
            .matches(&candidate.release_authority)
        {
            return DiscoveryDecision {
                allow_discovery: false,
                fetch_metadata: false,
                fetch_artifacts: false,
                reason: Some("release authority not allowed by discovery policy".to_string()),
            };
        }
        if !policy
            .allowed_contexts
            .matches_policy_scope(&candidate.scope)
        {
            return DiscoveryDecision {
                allow_discovery: false,
                fetch_metadata: false,
                fetch_artifacts: false,
                reason: Some("discovery scope not allowed by discovery policy".to_string()),
            };
        }
        if !candidate.builder_authorities.is_empty()
            && !candidate
                .builder_authorities
                .iter()
                .all(|builder| policy.allowed_builder_sources.matches(builder))
        {
            return DiscoveryDecision {
                allow_discovery: false,
                fetch_metadata: false,
                fetch_artifacts: false,
                reason: Some("builder authority not allowed by discovery policy".to_string()),
            };
        }

        DiscoveryDecision {
            allow_discovery: true,
            fetch_metadata: policy.auto_fetch_metadata,
            fetch_artifacts: policy.auto_fetch_artifacts,
            reason: None,
        }
    }

    /// Evaluate sharing policy independently from discovery or activation.
    pub fn evaluate_sharing(
        &self,
        policy: &AuraReleaseSharingPolicy,
        candidate: &SharingCandidate,
    ) -> SharingDecision {
        let _ = candidate.scope.clone();
        SharingDecision {
            pin_bundle: matches!(
                policy.pin_policy,
                PinPolicy::MetadataOnly | PinPolicy::RequiredArtifacts | PinPolicy::FullBundle
            ),
            forward_manifest: candidate.includes_manifest && policy.allow_manifest_forwarding,
            forward_artifacts: candidate.includes_artifacts && policy.allow_artifact_forwarding,
            forward_certificates: candidate.includes_certificates
                && policy.allow_certificate_forwarding,
            publish_recommendation: match (
                policy.recommendation_policy,
                candidate.recommendation_target,
            ) {
                (RecommendationPolicy::Never, _) => false,
                (RecommendationPolicy::TrustedAuthoritiesOnly, Some(_)) => true,
                (RecommendationPolicy::ScopeWide, Some(_)) => true,
                (_, None) => false,
            },
        }
    }

    /// Evaluate activation policy independently from discovery or sharing.
    pub fn evaluate_activation(
        &self,
        policy: &AuraReleaseActivationPolicy,
        candidate: &ActivationCandidate,
    ) -> ActivationDecision {
        if candidate.scope != policy.activation_scope {
            return ActivationDecision {
                allow_stage: false,
                allow_activate: false,
                reason: Some("activation scope does not match activation policy".to_string()),
            };
        }
        if !policy
            .trust_policy
            .allowed_release_sources
            .matches(&candidate.manifest.author)
        {
            return ActivationDecision {
                allow_stage: false,
                allow_activate: false,
                reason: Some("release authority not allowed by activation policy".to_string()),
            };
        }
        if !candidate.certificates.iter().all(|certificate| {
            policy
                .trust_policy
                .allowed_builder_sources
                .matches(&certificate.builder)
        }) {
            return ActivationDecision {
                allow_stage: false,
                allow_activate: false,
                reason: Some("builder authority not allowed by activation policy".to_string()),
            };
        }
        if let Some(required) = policy.trust_policy.required_builder_threshold {
            let distinct_builders = candidate
                .certificates
                .iter()
                .map(|certificate| certificate.builder)
                .collect::<BTreeSet<_>>()
                .len() as u16;
            if distinct_builders < required {
                return ActivationDecision {
                    allow_stage: false,
                    allow_activate: false,
                    reason: Some("builder threshold not satisfied".to_string()),
                };
            }
        }
        match candidate.compatibility {
            AuraCompatibilityClass::BackwardCompatible
            | AuraCompatibilityClass::MixedCoexistenceAllowed => {}
            AuraCompatibilityClass::ScopedHardFork => {
                if !candidate.threshold_approved {
                    return ActivationDecision {
                        allow_stage: false,
                        allow_activate: false,
                        reason: Some("scoped hard fork requires threshold approval".to_string()),
                    };
                }
                if !candidate.epoch_fence_satisfied {
                    return ActivationDecision {
                        allow_stage: false,
                        allow_activate: false,
                        reason: Some(
                            "scoped hard fork requires a satisfied epoch fence".to_string(),
                        ),
                    };
                }
            }
            AuraCompatibilityClass::IncompatibleWithoutPartition => {
                return ActivationDecision {
                    allow_stage: false,
                    allow_activate: false,
                    reason: Some(
                        "incompatible release requires explicit partition handling".to_string(),
                    ),
                };
            }
        }
        if !candidate.artifacts_staged {
            return ActivationDecision {
                allow_stage: false,
                allow_activate: false,
                reason: Some("required artifacts are not staged".to_string()),
            };
        }
        if policy.trust_policy.require_local_rebuild && !candidate.local_rebuild_available {
            return ActivationDecision {
                allow_stage: false,
                allow_activate: false,
                reason: Some("local rebuild evidence is required".to_string()),
            };
        }
        if policy.respect_suggested_activation_time {
            if let Some(suggested_time) = candidate.manifest.suggested_activation_time_unix_ms {
                if candidate.local_unix_time_ms < suggested_time {
                    return ActivationDecision {
                        allow_stage: policy.auto_stage,
                        allow_activate: false,
                        reason: Some(
                            "local policy is waiting for suggested activation time".to_string(),
                        ),
                    };
                }
            }
        }
        if candidate.health_gates.iter().any(|gate| !gate.passed) {
            return ActivationDecision {
                allow_stage: false,
                allow_activate: false,
                reason: Some("one or more health gates failed".to_string()),
            };
        }

        ActivationDecision {
            allow_stage: policy.auto_stage,
            allow_activate: policy.auto_stage && policy.auto_activate,
            reason: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::crypto::Ed25519SigningKey;
    use aura_core::time::{PhysicalTime, TimeStamp};
    use aura_core::{Hash32, SemanticVersion};
    use aura_maintenance::{
        AuraActivationTrustPolicy, AuraArtifactDescriptor, AuraArtifactKind,
        AuraCompatibilityClass, AuraHealthGate, AuraReleaseProvenance, AuraReleaseSeriesId,
        AuraTeeAttestation, AuthoritySelector, ContextSelector,
    };
    use std::collections::BTreeMap;

    fn authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn hash(seed: u8) -> Hash32 {
        Hash32([seed; 32])
    }

    fn ts(ms: u64) -> TimeStamp {
        TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: ms,
            uncertainty: Some(5),
        })
    }

    fn provenance(seed: u8) -> AuraReleaseProvenance {
        AuraReleaseProvenance::new(
            format!("https://example.invalid/policy-{seed}.git"),
            hash(seed),
            hash(seed.wrapping_add(1)),
            hash(seed.wrapping_add(2)),
            hash(seed.wrapping_add(3)),
            hash(seed.wrapping_add(4)),
        )
    }

    fn manifest_and_certificate() -> (AuraReleaseManifest, AuraDeterministicBuildCertificate) {
        let author = authority(1);
        let series_id = AuraReleaseSeriesId::new(hash(9));
        let signing_key = Ed25519SigningKey::from_bytes([2; 32]);
        let manifest = AuraReleaseManifest::new(
            series_id,
            SemanticVersion::new(3, 0, 0),
            author,
            provenance(20),
            vec![AuraArtifactDescriptor::new(
                AuraArtifactKind::Binary,
                "aura-agent",
                Some("x86_64-linux".to_string()),
                hash(44),
                1024,
            )],
            BTreeMap::new(),
            None,
            Some(1_800_000_000_000),
            signing_key.verifying_key().unwrap(),
            signing_key.sign(b"manifest").unwrap(),
        )
        .unwrap();
        let builder_key = Ed25519SigningKey::from_bytes([3; 32]);
        let certificate = AuraDeterministicBuildCertificate::new(
            series_id,
            authority(4),
            manifest.provenance.clone(),
            hash(55),
            ts(5),
            Some(AuraTeeAttestation {
                attestor_device: aura_core::DeviceId::new_from_entropy([7; 32]),
                measurement_hash: hash(56),
                evidence_hash: hash(57),
            }),
            builder_key.verifying_key().unwrap(),
            builder_key.sign(b"certificate").unwrap(),
        )
        .unwrap();
        (manifest, certificate)
    }

    #[test]
    fn discovery_policy_does_not_imply_activation() {
        let evaluator = OtaPolicyEvaluator::new();
        let discovery = evaluator.evaluate_discovery(
            &AuraReleaseDiscoveryPolicy {
                allowed_release_sources: AuthoritySelector::Exact(BTreeSet::from([authority(1)])),
                allowed_builder_sources: AuthoritySelector::Any,
                allowed_contexts: ContextSelector::Any,
                auto_fetch_metadata: true,
                auto_fetch_artifacts: false,
            },
            &DiscoveryCandidate {
                release_authority: authority(1),
                builder_authorities: BTreeSet::from([authority(4)]),
                scope: AuraPolicyScope::Authority {
                    authority_id: authority(1),
                },
            },
        );

        assert!(discovery.allow_discovery);
        assert!(discovery.fetch_metadata);
        assert!(!discovery.fetch_artifacts);
    }

    #[test]
    fn sharing_policy_only_controls_forwarding_surface() {
        let evaluator = OtaPolicyEvaluator::new();
        let decision = evaluator.evaluate_sharing(
            &AuraReleaseSharingPolicy {
                pin_policy: PinPolicy::MetadataOnly,
                allow_manifest_forwarding: true,
                allow_artifact_forwarding: false,
                allow_certificate_forwarding: true,
                recommendation_policy: RecommendationPolicy::TrustedAuthoritiesOnly,
            },
            &SharingCandidate {
                scope: AuraPolicyScope::Authority {
                    authority_id: authority(1),
                },
                includes_manifest: true,
                includes_artifacts: true,
                includes_certificates: true,
                recommendation_target: Some(authority(2)),
            },
        );

        assert!(decision.pin_bundle);
        assert!(decision.forward_manifest);
        assert!(!decision.forward_artifacts);
        assert!(decision.forward_certificates);
        assert!(decision.publish_recommendation);
    }

    #[test]
    fn activation_policy_requires_separate_trust_checks() {
        let evaluator = OtaPolicyEvaluator::new();
        let (manifest, certificate) = manifest_and_certificate();
        let decision = evaluator.evaluate_activation(
            &AuraReleaseActivationPolicy {
                trust_policy: AuraActivationTrustPolicy {
                    allowed_release_sources: AuthoritySelector::Exact(BTreeSet::from([
                        manifest.author
                    ])),
                    allowed_builder_sources: AuthoritySelector::Exact(BTreeSet::from([
                        certificate.builder,
                    ])),
                    required_builder_threshold: Some(1),
                    require_local_rebuild: true,
                },
                auto_stage: true,
                auto_activate: false,
                respect_suggested_activation_time: true,
                activation_scope: AuraActivationScope::AuthorityLocal {
                    authority_id: manifest.author,
                },
            },
            &ActivationCandidate {
                manifest,
                certificates: vec![certificate],
                scope: AuraActivationScope::AuthorityLocal {
                    authority_id: authority(1),
                },
                compatibility: AuraCompatibilityClass::ScopedHardFork,
                artifacts_staged: true,
                threshold_approved: true,
                epoch_fence_satisfied: true,
                health_gates: vec![AuraHealthGate {
                    gate_name: "post-stage-smoke".to_string(),
                    passed: true,
                }],
                local_unix_time_ms: 1_800_000_000_001,
                local_rebuild_available: true,
            },
        );

        assert!(decision.allow_stage);
        assert!(!decision.allow_activate);
        assert_eq!(decision.reason, None);
    }

    #[test]
    fn activation_policy_can_wait_for_suggested_activation_time() {
        let evaluator = OtaPolicyEvaluator::new();
        let (manifest, certificate) = manifest_and_certificate();
        let decision = evaluator.evaluate_activation(
            &AuraReleaseActivationPolicy {
                trust_policy: AuraActivationTrustPolicy {
                    allowed_release_sources: AuthoritySelector::Exact(BTreeSet::from([
                        manifest.author
                    ])),
                    allowed_builder_sources: AuthoritySelector::Exact(BTreeSet::from([
                        certificate.builder,
                    ])),
                    required_builder_threshold: Some(1),
                    require_local_rebuild: false,
                },
                auto_stage: true,
                auto_activate: true,
                respect_suggested_activation_time: true,
                activation_scope: AuraActivationScope::AuthorityLocal {
                    authority_id: manifest.author,
                },
            },
            &ActivationCandidate {
                manifest,
                certificates: vec![certificate],
                scope: AuraActivationScope::AuthorityLocal {
                    authority_id: authority(1),
                },
                compatibility: AuraCompatibilityClass::BackwardCompatible,
                artifacts_staged: true,
                threshold_approved: true,
                epoch_fence_satisfied: true,
                health_gates: vec![AuraHealthGate {
                    gate_name: "post-stage-smoke".to_string(),
                    passed: true,
                }],
                local_unix_time_ms: 1_700_000_000_000,
                local_rebuild_available: true,
            },
        );

        assert!(decision.allow_stage);
        assert!(!decision.allow_activate);
        assert_eq!(
            decision.reason,
            Some("local policy is waiting for suggested activation time".to_string())
        );
    }
}

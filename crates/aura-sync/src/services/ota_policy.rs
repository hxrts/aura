//! Stateless OTA policy evaluation surfaces.

use aura_core::AuthorityId;
use aura_maintenance::{
    AuraActivationScope, AuraActivationWindow, AuraCompatibilityClass,
    AuraDeterministicBuildCertificate, AuraHealthGate, AuraPolicyScope,
    AuraReleaseActivationPolicy, AuraReleaseDiscoveryPolicy, AuraReleaseId, AuraReleaseManifest,
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
    /// Local rollout cohort label, if the scope is part of one.
    pub rollout_cohort: Option<String>,
    /// Trusted authorities that have revoked the target release.
    pub revoked_by: BTreeSet<AuthorityId>,
    /// Trusted superseding release, if local evidence considers this release superseded.
    pub superseded_by: Option<AuraReleaseId>,
}

/// Decision produced by activation-policy evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationDecision {
    /// Whether the candidate may be staged.
    pub allow_stage: bool,
    /// Whether the candidate may be activated immediately.
    pub allow_activate: bool,
    /// Optional structured activation blocker or wait condition.
    pub reason: Option<ActivationDecisionReason>,
}

/// Structured activation blocker or wait condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationDecisionReason {
    /// The evaluated scope does not match the policy scope.
    ActivationScopeMismatch,
    /// The manifest author is not trusted for activation.
    ReleaseAuthorityNotAllowed,
    /// At least one builder certificate came from an untrusted authority.
    BuilderAuthorityNotAllowed,
    /// The trusted builder threshold is not satisfied.
    BuilderThresholdNotSatisfied {
        /// Minimum number of distinct trusted builders required.
        required: u16,
        /// Number of distinct trusted builders observed.
        observed: u16,
    },
    /// The trusted TEE-backed builder threshold is not satisfied.
    TeeBuilderThresholdNotSatisfied {
        /// Minimum number of distinct trusted builders with TEE evidence required.
        required: u16,
        /// Number of distinct trusted builders with TEE evidence observed.
        observed: u16,
    },
    /// A scoped hard fork is missing explicit approval.
    ThresholdApprovalRequired,
    /// A scoped hard fork is missing its required epoch fence.
    EpochFenceRequired,
    /// The release cannot activate until partition behavior is handled explicitly.
    PartitionHandlingRequired,
    /// Required artifacts are not staged locally.
    ArtifactsNotStaged,
    /// Local rebuild evidence is required by policy but absent.
    LocalRebuildRequired,
    /// Automatic activation is gated on rollout cohort eligibility.
    RolloutCohortMismatch {
        /// Cohort label required by policy.
        required: String,
        /// Cohort label observed for this scope, if any.
        observed: Option<String>,
    },
    /// Trusted revocation evidence blocks activation.
    RevokedByTrustedAuthorities {
        /// Trusted authorities that revoked the release.
        authorities: Vec<AuthorityId>,
    },
    /// Trusted supersession evidence blocks activation.
    SupersededByRelease {
        /// Trusted target release that supersedes the current candidate.
        superseding_release_id: AuraReleaseId,
    },
    /// The activation window has not opened yet for the local clock.
    ActivationWindowNotOpen {
        /// Earliest local activation time.
        not_before_unix_ms: u64,
        /// Local wall-clock time used for evaluation.
        local_unix_time_ms: u64,
    },
    /// The activation window has already closed for the local clock.
    ActivationWindowClosed {
        /// Latest local activation time.
        not_after_unix_ms: u64,
        /// Local wall-clock time used for evaluation.
        local_unix_time_ms: u64,
    },
    /// Activation is waiting for the local clock to pass the advisory time hint.
    SuggestedActivationTimePending {
        /// Advisory activation time from the manifest.
        suggested_time_unix_ms: u64,
        /// Local wall-clock time used for evaluation.
        local_unix_time_ms: u64,
    },
    /// One or more named health gates failed.
    HealthGateFailed {
        /// Gate names that reported failure.
        failed_gates: Vec<String>,
    },
}

impl ActivationDecision {
    fn denied(reason: ActivationDecisionReason) -> Self {
        Self {
            allow_stage: false,
            allow_activate: false,
            reason: Some(reason),
        }
    }

    fn deferred(allow_stage: bool, reason: ActivationDecisionReason) -> Self {
        Self {
            allow_stage,
            allow_activate: false,
            reason: Some(reason),
        }
    }

    fn allowed(allow_stage: bool, allow_activate: bool) -> Self {
        Self {
            allow_stage,
            allow_activate,
            reason: None,
        }
    }
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
            return ActivationDecision::denied(ActivationDecisionReason::ActivationScopeMismatch);
        }
        if !policy
            .trust_policy
            .allowed_release_sources
            .matches(&candidate.manifest.author)
        {
            return ActivationDecision::denied(
                ActivationDecisionReason::ReleaseAuthorityNotAllowed,
            );
        }
        if !candidate.certificates.iter().all(|certificate| {
            policy
                .trust_policy
                .allowed_builder_sources
                .matches(&certificate.builder)
        }) {
            return ActivationDecision::denied(
                ActivationDecisionReason::BuilderAuthorityNotAllowed,
            );
        }
        if let Some(required) = policy.trust_policy.required_builder_threshold {
            let observed = candidate
                .certificates
                .iter()
                .map(|certificate| certificate.builder)
                .collect::<BTreeSet<_>>()
                .len() as u16;
            if observed < required {
                return ActivationDecision::denied(
                    ActivationDecisionReason::BuilderThresholdNotSatisfied { required, observed },
                );
            }
        }
        if let Some(required) = policy.trust_policy.required_tee_builder_threshold {
            let observed = candidate
                .certificates
                .iter()
                .filter(|certificate| certificate.tee_attestation.is_some())
                .map(|certificate| certificate.builder)
                .collect::<BTreeSet<_>>()
                .len() as u16;
            if observed < required {
                return ActivationDecision::denied(
                    ActivationDecisionReason::TeeBuilderThresholdNotSatisfied {
                        required,
                        observed,
                    },
                );
            }
        }
        match candidate.compatibility {
            AuraCompatibilityClass::BackwardCompatible
            | AuraCompatibilityClass::MixedCoexistenceAllowed => {}
            AuraCompatibilityClass::ScopedHardFork => {
                if !candidate.threshold_approved {
                    return ActivationDecision::denied(
                        ActivationDecisionReason::ThresholdApprovalRequired,
                    );
                }
                if !candidate.epoch_fence_satisfied {
                    return ActivationDecision::denied(
                        ActivationDecisionReason::EpochFenceRequired,
                    );
                }
            }
            AuraCompatibilityClass::IncompatibleWithoutPartition => {
                return ActivationDecision::denied(
                    ActivationDecisionReason::PartitionHandlingRequired,
                );
            }
        }
        if !candidate.artifacts_staged {
            return ActivationDecision::denied(ActivationDecisionReason::ArtifactsNotStaged);
        }
        if policy.trust_policy.require_local_rebuild && !candidate.local_rebuild_available {
            return ActivationDecision::denied(ActivationDecisionReason::LocalRebuildRequired);
        }
        if let Some(required_cohort) = &policy.required_rollout_cohort {
            if candidate.rollout_cohort.as_ref() != Some(required_cohort) {
                return ActivationDecision::deferred(
                    policy.auto_stage,
                    ActivationDecisionReason::RolloutCohortMismatch {
                        required: required_cohort.clone(),
                        observed: candidate.rollout_cohort.clone(),
                    },
                );
            }
        }
        if policy.trust_policy.block_on_trusted_revocation && !candidate.revoked_by.is_empty() {
            return ActivationDecision::denied(
                ActivationDecisionReason::RevokedByTrustedAuthorities {
                    authorities: candidate.revoked_by.iter().copied().collect(),
                },
            );
        }
        if policy.trust_policy.block_on_supersession {
            if let Some(superseding_release_id) = candidate.superseded_by {
                return ActivationDecision::denied(ActivationDecisionReason::SupersededByRelease {
                    superseding_release_id,
                });
            }
        }
        if let Some(window) = &policy.activation_window {
            if let Some(decision) = Self::evaluate_activation_window(
                window,
                candidate.local_unix_time_ms,
                policy.auto_stage,
            ) {
                return decision;
            }
        }
        if policy.respect_suggested_activation_time {
            if let Some(suggested_time) = candidate.manifest.suggested_activation_time_unix_ms {
                if candidate.local_unix_time_ms < suggested_time {
                    return ActivationDecision::deferred(
                        policy.auto_stage,
                        ActivationDecisionReason::SuggestedActivationTimePending {
                            suggested_time_unix_ms: suggested_time,
                            local_unix_time_ms: candidate.local_unix_time_ms,
                        },
                    );
                }
            }
        }
        let failed_gates = candidate
            .health_gates
            .iter()
            .filter(|gate| !gate.passed)
            .map(|gate| gate.gate_name.clone())
            .collect::<Vec<_>>();
        if !failed_gates.is_empty() {
            return ActivationDecision::denied(ActivationDecisionReason::HealthGateFailed {
                failed_gates,
            });
        }

        ActivationDecision::allowed(policy.auto_stage, policy.auto_stage && policy.auto_activate)
    }

    fn evaluate_activation_window(
        window: &AuraActivationWindow,
        local_unix_time_ms: u64,
        allow_stage: bool,
    ) -> Option<ActivationDecision> {
        if let Some(not_before_unix_ms) = window.not_before_unix_ms {
            if local_unix_time_ms < not_before_unix_ms {
                return Some(ActivationDecision::deferred(
                    allow_stage,
                    ActivationDecisionReason::ActivationWindowNotOpen {
                        not_before_unix_ms,
                        local_unix_time_ms,
                    },
                ));
            }
        }
        if let Some(not_after_unix_ms) = window.not_after_unix_ms {
            if local_unix_time_ms > not_after_unix_ms {
                return Some(ActivationDecision::denied(
                    ActivationDecisionReason::ActivationWindowClosed {
                        not_after_unix_ms,
                        local_unix_time_ms,
                    },
                ));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::crypto::Ed25519SigningKey;
    use aura_core::time::{PhysicalTime, TimeStamp};
    use aura_core::{Hash32, SemanticVersion};
    use aura_maintenance::{
        AuraActivationProfile, AuraActivationTrustPolicy, AuraActivationWindow,
        AuraArtifactDescriptor, AuraArtifactKind, AuraArtifactPackaging, AuraCompatibilityClass,
        AuraCompatibilityManifest, AuraDataMigration, AuraHealthGate, AuraLauncherEntrypoint,
        AuraReleaseProvenance, AuraReleaseSeriesId, AuraRollbackPreference,
        AuraRollbackRequirement, AuraTargetPlatform, AuraTeeAttestation, AuthoritySelector,
        ContextSelector,
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
                Some(AuraTargetPlatform::new("x86_64-linux")),
                AuraArtifactPackaging::TarZst,
                "bin/aura-agent",
                Some(AuraLauncherEntrypoint::new(
                    "bin/aura-agent",
                    vec!["--serve".to_string()],
                )),
                AuraRollbackRequirement::KeepPriorReleaseStaged,
                hash(44),
                1024,
            )],
            AuraCompatibilityManifest::new(
                AuraCompatibilityClass::MixedCoexistenceAllowed,
                None,
                BTreeMap::new(),
                BTreeMap::new(),
            ),
            vec![AuraDataMigration::new(
                "journal-v3",
                "Upgrade journal metadata encoding",
                true,
            )],
            AuraActivationProfile::new(false, false, vec!["post-stage-smoke".to_string()]),
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

    fn certificate_for(
        manifest: &AuraReleaseManifest,
        builder_seed: u8,
        signer_seed: u8,
    ) -> AuraDeterministicBuildCertificate {
        let builder_key = Ed25519SigningKey::from_bytes([signer_seed; 32]);
        AuraDeterministicBuildCertificate::new(
            manifest.series_id,
            authority(builder_seed),
            manifest.provenance.clone(),
            hash(signer_seed.wrapping_add(40)),
            ts(5 + signer_seed as u64),
            Some(AuraTeeAttestation {
                attestor_device: aura_core::DeviceId::new_from_entropy([builder_seed; 32]),
                measurement_hash: hash(signer_seed.wrapping_add(41)),
                evidence_hash: hash(signer_seed.wrapping_add(42)),
            }),
            builder_key.verifying_key().unwrap(),
            builder_key.sign(b"certificate").unwrap(),
        )
        .unwrap()
    }

    fn certificate_without_tee(
        manifest: &AuraReleaseManifest,
        builder_seed: u8,
        signer_seed: u8,
    ) -> AuraDeterministicBuildCertificate {
        let builder_key = Ed25519SigningKey::from_bytes([signer_seed; 32]);
        AuraDeterministicBuildCertificate::new(
            manifest.series_id,
            authority(builder_seed),
            manifest.provenance.clone(),
            hash(signer_seed.wrapping_add(50)),
            ts(10 + signer_seed as u64),
            None,
            builder_key.verifying_key().unwrap(),
            builder_key.sign(b"certificate").unwrap(),
        )
        .unwrap()
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
    fn discovery_policy_is_scope_aware() {
        let evaluator = OtaPolicyEvaluator::new();
        let home_id = aura_core::HomeId::from_bytes([8; 32]);
        let decision = evaluator.evaluate_discovery(
            &AuraReleaseDiscoveryPolicy {
                allowed_release_sources: AuthoritySelector::Exact(BTreeSet::from([authority(1)])),
                allowed_builder_sources: AuthoritySelector::Any,
                allowed_contexts: ContextSelector::Home(BTreeSet::from([home_id])),
                auto_fetch_metadata: true,
                auto_fetch_artifacts: true,
            },
            &DiscoveryCandidate {
                release_authority: authority(1),
                builder_authorities: BTreeSet::new(),
                scope: AuraPolicyScope::Authority {
                    authority_id: authority(1),
                },
            },
        );

        assert!(!decision.allow_discovery);
        assert_eq!(
            decision.reason,
            Some("discovery scope not allowed by discovery policy".to_string())
        );
    }

    #[test]
    fn sharing_policy_selectively_forwards_available_components() {
        let evaluator = OtaPolicyEvaluator::new();
        let decision = evaluator.evaluate_sharing(
            &AuraReleaseSharingPolicy {
                pin_policy: PinPolicy::RequiredArtifacts,
                allow_manifest_forwarding: true,
                allow_artifact_forwarding: true,
                allow_certificate_forwarding: false,
                recommendation_policy: RecommendationPolicy::Never,
            },
            &SharingCandidate {
                scope: AuraPolicyScope::Authority {
                    authority_id: authority(1),
                },
                includes_manifest: true,
                includes_artifacts: false,
                includes_certificates: true,
                recommendation_target: Some(authority(2)),
            },
        );

        assert!(decision.pin_bundle);
        assert!(decision.forward_manifest);
        assert!(!decision.forward_artifacts);
        assert!(!decision.forward_certificates);
        assert!(!decision.publish_recommendation);
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
                    required_tee_builder_threshold: None,
                    require_local_rebuild: true,
                    block_on_trusted_revocation: true,
                    block_on_supersession: false,
                },
                auto_stage: true,
                auto_activate: false,
                required_rollout_cohort: None,
                activation_window: None,
                respect_suggested_activation_time: true,
                rollback_preference: AuraRollbackPreference::Automatic,
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
                rollout_cohort: None,
                revoked_by: BTreeSet::new(),
                superseded_by: None,
            },
        );

        assert!(decision.allow_stage);
        assert!(!decision.allow_activate);
        assert_eq!(decision.reason, None);
    }

    #[test]
    fn activation_policy_is_local_to_its_scope() {
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
                    required_tee_builder_threshold: None,
                    require_local_rebuild: false,
                    block_on_trusted_revocation: true,
                    block_on_supersession: false,
                },
                auto_stage: true,
                auto_activate: true,
                required_rollout_cohort: None,
                activation_window: None,
                respect_suggested_activation_time: false,
                rollback_preference: AuraRollbackPreference::Automatic,
                activation_scope: AuraActivationScope::AuthorityLocal {
                    authority_id: manifest.author,
                },
            },
            &ActivationCandidate {
                manifest,
                certificates: vec![certificate],
                scope: AuraActivationScope::AuthorityLocal {
                    authority_id: authority(99),
                },
                compatibility: AuraCompatibilityClass::BackwardCompatible,
                artifacts_staged: true,
                threshold_approved: true,
                epoch_fence_satisfied: true,
                health_gates: vec![AuraHealthGate {
                    gate_name: "post-stage-smoke".to_string(),
                    passed: true,
                }],
                local_unix_time_ms: 1_900_000_000_000,
                local_rebuild_available: true,
                rollout_cohort: None,
                revoked_by: BTreeSet::new(),
                superseded_by: None,
            },
        );

        assert_eq!(
            decision.reason,
            Some(ActivationDecisionReason::ActivationScopeMismatch)
        );
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
                    required_tee_builder_threshold: None,
                    require_local_rebuild: false,
                    block_on_trusted_revocation: true,
                    block_on_supersession: false,
                },
                auto_stage: true,
                auto_activate: true,
                required_rollout_cohort: None,
                activation_window: None,
                respect_suggested_activation_time: true,
                rollback_preference: AuraRollbackPreference::Automatic,
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
                rollout_cohort: None,
                revoked_by: BTreeSet::new(),
                superseded_by: None,
            },
        );

        assert!(decision.allow_stage);
        assert!(!decision.allow_activate);
        assert_eq!(
            decision.reason,
            Some(ActivationDecisionReason::SuggestedActivationTimePending {
                suggested_time_unix_ms: 1_800_000_000_000,
                local_unix_time_ms: 1_700_000_000_000,
            })
        );
    }

    #[test]
    fn activation_policy_reports_builder_threshold_shortfall() {
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
                    required_builder_threshold: Some(2),
                    required_tee_builder_threshold: None,
                    require_local_rebuild: false,
                    block_on_trusted_revocation: true,
                    block_on_supersession: false,
                },
                auto_stage: true,
                auto_activate: true,
                required_rollout_cohort: None,
                activation_window: None,
                respect_suggested_activation_time: false,
                rollback_preference: AuraRollbackPreference::Automatic,
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
                local_unix_time_ms: 1_900_000_000_000,
                local_rebuild_available: true,
                rollout_cohort: None,
                revoked_by: BTreeSet::new(),
                superseded_by: None,
            },
        );

        assert_eq!(
            decision.reason,
            Some(ActivationDecisionReason::BuilderThresholdNotSatisfied {
                required: 2,
                observed: 1,
            })
        );
    }

    #[test]
    fn activation_policy_blocks_trusted_revocation() {
        let evaluator = OtaPolicyEvaluator::new();
        let (manifest, certificate) = manifest_and_certificate();
        let revoker = authority(9);
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
                    required_tee_builder_threshold: None,
                    require_local_rebuild: false,
                    block_on_trusted_revocation: true,
                    block_on_supersession: false,
                },
                auto_stage: true,
                auto_activate: true,
                required_rollout_cohort: None,
                activation_window: None,
                respect_suggested_activation_time: false,
                rollback_preference: AuraRollbackPreference::Automatic,
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
                local_unix_time_ms: 1_900_000_000_000,
                local_rebuild_available: true,
                rollout_cohort: None,
                revoked_by: BTreeSet::from([revoker]),
                superseded_by: None,
            },
        );

        assert_eq!(
            decision.reason,
            Some(ActivationDecisionReason::RevokedByTrustedAuthorities {
                authorities: vec![revoker],
            })
        );
    }

    #[test]
    fn activation_policy_can_require_not_superseded() {
        let evaluator = OtaPolicyEvaluator::new();
        let (manifest, certificate) = manifest_and_certificate();
        let superseding_release_id = AuraReleaseId::new(hash(90));
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
                    required_tee_builder_threshold: None,
                    require_local_rebuild: false,
                    block_on_trusted_revocation: true,
                    block_on_supersession: true,
                },
                auto_stage: true,
                auto_activate: true,
                required_rollout_cohort: None,
                activation_window: None,
                respect_suggested_activation_time: false,
                rollback_preference: AuraRollbackPreference::Automatic,
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
                local_unix_time_ms: 1_900_000_000_000,
                local_rebuild_available: true,
                rollout_cohort: None,
                revoked_by: BTreeSet::new(),
                superseded_by: Some(superseding_release_id),
            },
        );

        assert_eq!(
            decision.reason,
            Some(ActivationDecisionReason::SupersededByRelease {
                superseding_release_id,
            })
        );
    }

    #[test]
    fn activation_policy_accumulates_distinct_builder_evidence() {
        let evaluator = OtaPolicyEvaluator::new();
        let (manifest, first_certificate) = manifest_and_certificate();
        let second_certificate = certificate_for(&manifest, 5, 6);
        let decision = evaluator.evaluate_activation(
            &AuraReleaseActivationPolicy {
                trust_policy: AuraActivationTrustPolicy {
                    allowed_release_sources: AuthoritySelector::Exact(BTreeSet::from([
                        manifest.author
                    ])),
                    allowed_builder_sources: AuthoritySelector::Exact(BTreeSet::from([
                        first_certificate.builder,
                        second_certificate.builder,
                    ])),
                    required_builder_threshold: Some(2),
                    required_tee_builder_threshold: None,
                    require_local_rebuild: false,
                    block_on_trusted_revocation: true,
                    block_on_supersession: false,
                },
                auto_stage: true,
                auto_activate: true,
                required_rollout_cohort: None,
                activation_window: None,
                respect_suggested_activation_time: false,
                rollback_preference: AuraRollbackPreference::Automatic,
                activation_scope: AuraActivationScope::AuthorityLocal {
                    authority_id: manifest.author,
                },
            },
            &ActivationCandidate {
                manifest,
                certificates: vec![first_certificate, second_certificate],
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
                local_unix_time_ms: 1_900_000_000_000,
                local_rebuild_available: true,
                rollout_cohort: None,
                revoked_by: BTreeSet::new(),
                superseded_by: None,
            },
        );

        assert!(decision.allow_stage);
        assert!(decision.allow_activate);
        assert_eq!(decision.reason, None);
    }

    #[test]
    fn activation_policy_counts_duplicate_builder_certificates_once() {
        let evaluator = OtaPolicyEvaluator::new();
        let (manifest, certificate) = manifest_and_certificate();
        let duplicate_certificate = certificate.clone();
        let decision = evaluator.evaluate_activation(
            &AuraReleaseActivationPolicy {
                trust_policy: AuraActivationTrustPolicy {
                    allowed_release_sources: AuthoritySelector::Exact(BTreeSet::from([
                        manifest.author
                    ])),
                    allowed_builder_sources: AuthoritySelector::Exact(BTreeSet::from([
                        certificate.builder,
                    ])),
                    required_builder_threshold: Some(2),
                    required_tee_builder_threshold: None,
                    require_local_rebuild: false,
                    block_on_trusted_revocation: true,
                    block_on_supersession: false,
                },
                auto_stage: true,
                auto_activate: true,
                required_rollout_cohort: None,
                activation_window: None,
                respect_suggested_activation_time: false,
                rollback_preference: AuraRollbackPreference::Automatic,
                activation_scope: AuraActivationScope::AuthorityLocal {
                    authority_id: manifest.author,
                },
            },
            &ActivationCandidate {
                manifest,
                certificates: vec![certificate, duplicate_certificate],
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
                local_unix_time_ms: 1_900_000_000_000,
                local_rebuild_available: true,
                rollout_cohort: None,
                revoked_by: BTreeSet::new(),
                superseded_by: None,
            },
        );

        assert_eq!(
            decision.reason,
            Some(ActivationDecisionReason::BuilderThresholdNotSatisfied {
                required: 2,
                observed: 1,
            })
        );
    }

    #[test]
    fn activation_policy_can_gate_on_rollout_cohort() {
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
                    required_tee_builder_threshold: None,
                    require_local_rebuild: false,
                    block_on_trusted_revocation: true,
                    block_on_supersession: false,
                },
                auto_stage: true,
                auto_activate: true,
                required_rollout_cohort: Some("canary".to_string()),
                activation_window: None,
                respect_suggested_activation_time: false,
                rollback_preference: AuraRollbackPreference::Automatic,
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
                local_unix_time_ms: 1_900_000_000_000,
                local_rebuild_available: true,
                rollout_cohort: Some("general".to_string()),
                revoked_by: BTreeSet::new(),
                superseded_by: None,
            },
        );

        assert_eq!(
            decision.reason,
            Some(ActivationDecisionReason::RolloutCohortMismatch {
                required: "canary".to_string(),
                observed: Some("general".to_string()),
            })
        );
    }

    #[test]
    fn activation_policy_respects_local_activation_window() {
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
                    required_tee_builder_threshold: None,
                    require_local_rebuild: false,
                    block_on_trusted_revocation: true,
                    block_on_supersession: false,
                },
                auto_stage: true,
                auto_activate: true,
                required_rollout_cohort: None,
                activation_window: Some(AuraActivationWindow {
                    not_before_unix_ms: Some(1_900_000_000_000),
                    not_after_unix_ms: Some(1_950_000_000_000),
                }),
                respect_suggested_activation_time: false,
                rollback_preference: AuraRollbackPreference::Automatic,
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
                local_unix_time_ms: 1_800_000_000_000,
                local_rebuild_available: true,
                rollout_cohort: None,
                revoked_by: BTreeSet::new(),
                superseded_by: None,
            },
        );

        assert_eq!(
            decision.reason,
            Some(ActivationDecisionReason::ActivationWindowNotOpen {
                not_before_unix_ms: 1_900_000_000_000,
                local_unix_time_ms: 1_800_000_000_000,
            })
        );
    }

    #[test]
    fn activation_policy_can_require_tee_backed_builder_quorum() {
        let evaluator = OtaPolicyEvaluator::new();
        let (manifest, tee_certificate) = manifest_and_certificate();
        let non_tee_certificate = certificate_without_tee(&manifest, 5, 8);
        let decision = evaluator.evaluate_activation(
            &AuraReleaseActivationPolicy {
                trust_policy: AuraActivationTrustPolicy {
                    allowed_release_sources: AuthoritySelector::Exact(BTreeSet::from([
                        manifest.author
                    ])),
                    allowed_builder_sources: AuthoritySelector::Exact(BTreeSet::from([
                        tee_certificate.builder,
                        non_tee_certificate.builder,
                    ])),
                    required_builder_threshold: Some(2),
                    required_tee_builder_threshold: Some(2),
                    require_local_rebuild: false,
                    block_on_trusted_revocation: true,
                    block_on_supersession: false,
                },
                auto_stage: true,
                auto_activate: true,
                required_rollout_cohort: None,
                activation_window: None,
                respect_suggested_activation_time: false,
                rollback_preference: AuraRollbackPreference::Automatic,
                activation_scope: AuraActivationScope::AuthorityLocal {
                    authority_id: manifest.author,
                },
            },
            &ActivationCandidate {
                manifest,
                certificates: vec![tee_certificate, non_tee_certificate],
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
                local_unix_time_ms: 1_900_000_000_000,
                local_rebuild_available: true,
                rollout_cohort: None,
                revoked_by: BTreeSet::new(),
                superseded_by: None,
            },
        );

        assert_eq!(
            decision.reason,
            Some(ActivationDecisionReason::TeeBuilderThresholdNotSatisfied {
                required: 2,
                observed: 1,
            })
        );
    }
}

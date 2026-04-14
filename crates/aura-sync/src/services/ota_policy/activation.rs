use super::*;

fn distinct_builder_count(candidate: &ActivationCandidate) -> u16 {
    candidate
        .certificates
        .iter()
        .map(|certificate| certificate.builder)
        .collect::<BTreeSet<_>>()
        .len() as u16
}

fn distinct_tee_builder_count(candidate: &ActivationCandidate) -> u16 {
    candidate
        .certificates
        .iter()
        .filter(|certificate| certificate.tee_attestation.is_some())
        .map(|certificate| certificate.builder)
        .collect::<BTreeSet<_>>()
        .len() as u16
}

fn failed_gate_names(candidate: &ActivationCandidate) -> Vec<String> {
    candidate
        .health_gates
        .iter()
        .filter(|gate| !gate.passed)
        .map(|gate| gate.gate_name.clone())
        .collect()
}

fn require_builder_threshold(
    required: Option<u16>,
    observed: u16,
    reason: impl FnOnce(u16, u16) -> ActivationDecisionReason,
) -> Option<ActivationDecision> {
    required.and_then(|required| {
        (observed < required).then(|| ActivationDecision::denied(reason(required, observed)))
    })
}

fn require_rollout_cohort(
    policy: &AuraReleaseActivationPolicy,
    candidate: &ActivationCandidate,
) -> Option<ActivationDecision> {
    policy
        .required_rollout_cohort
        .as_ref()
        .and_then(|required_cohort| {
            (candidate.rollout_cohort.as_ref() != Some(required_cohort)).then(|| {
                ActivationDecision::deferred(
                    policy.auto_stage,
                    ActivationDecisionReason::RolloutCohortMismatch {
                        required: required_cohort.clone(),
                        observed: candidate.rollout_cohort.clone(),
                    },
                )
            })
        })
}

fn require_suggested_activation_time(
    policy: &AuraReleaseActivationPolicy,
    candidate: &ActivationCandidate,
) -> Option<ActivationDecision> {
    if !policy.respect_suggested_activation_time {
        return None;
    }

    candidate
        .manifest
        .suggested_activation_time_unix_ms
        .and_then(|suggested_time| {
            (candidate.local_unix_time_ms < suggested_time).then(|| {
                ActivationDecision::deferred(
                    policy.auto_stage,
                    ActivationDecisionReason::SuggestedActivationTimePending {
                        suggested_time_unix_ms: suggested_time,
                        local_unix_time_ms: candidate.local_unix_time_ms,
                    },
                )
            })
        })
}

pub(super) fn evaluate_activation(
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
        return ActivationDecision::denied(ActivationDecisionReason::ReleaseAuthorityNotAllowed);
    }
    if !candidate.certificates.iter().all(|certificate| {
        policy
            .trust_policy
            .allowed_builder_sources
            .matches(&certificate.builder)
    }) {
        return ActivationDecision::denied(ActivationDecisionReason::BuilderAuthorityNotAllowed);
    }
    if let Some(decision) = require_builder_threshold(
        policy.trust_policy.required_builder_threshold,
        distinct_builder_count(candidate),
        |required, observed| ActivationDecisionReason::BuilderThresholdNotSatisfied {
            required,
            observed,
        },
    ) {
        return decision;
    }
    if let Some(decision) = require_builder_threshold(
        policy.trust_policy.required_tee_builder_threshold,
        distinct_tee_builder_count(candidate),
        |required, observed| ActivationDecisionReason::TeeBuilderThresholdNotSatisfied {
            required,
            observed,
        },
    ) {
        return decision;
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
                return ActivationDecision::denied(ActivationDecisionReason::EpochFenceRequired);
            }
        }
        AuraCompatibilityClass::IncompatibleWithoutPartition => {
            return ActivationDecision::denied(ActivationDecisionReason::PartitionHandlingRequired);
        }
    }
    if !candidate.artifacts_staged {
        return ActivationDecision::denied(ActivationDecisionReason::ArtifactsNotStaged);
    }
    if policy.trust_policy.require_local_rebuild && !candidate.local_rebuild_available {
        return ActivationDecision::denied(ActivationDecisionReason::LocalRebuildRequired);
    }
    if let Some(decision) = require_rollout_cohort(policy, candidate) {
        return decision;
    }
    if policy.trust_policy.block_on_trusted_revocation && !candidate.revoked_by.is_empty() {
        return ActivationDecision::denied(ActivationDecisionReason::RevokedByTrustedAuthorities {
            authorities: candidate.revoked_by.iter().copied().collect(),
        });
    }
    if policy.trust_policy.block_on_supersession {
        if let Some(superseding_release_id) = candidate.superseded_by {
            return ActivationDecision::denied(ActivationDecisionReason::SupersededByRelease {
                superseding_release_id,
            });
        }
    }
    if let Some(window) = &policy.activation_window {
        if let Some(decision) =
            evaluate_activation_window(window, candidate.local_unix_time_ms, policy.auto_stage)
        {
            return decision;
        }
    }
    if let Some(decision) = require_suggested_activation_time(policy, candidate) {
        return decision;
    }
    let failed_gates = failed_gate_names(candidate);
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

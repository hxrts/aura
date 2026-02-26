//! Envelope divergence policy for cross-runtime parity gates.

use telltale_vm::{
    EffectDeterminismTier, EffectOrderingClass, EnvelopeDiff, FailureVisibleDiffClass,
    SchedulerPermutationClass,
};

/// Aura policy for admissible cross-runtime envelope differences.
///
/// This policy allows only commutative/algebraic differences:
/// - scheduler: exact or session-normalized permutation
/// - effect ordering: exact or replay-deterministic
/// - failure-visible state: exact only
/// - determinism tier: strict or replay-deterministic
#[derive(Debug, Clone, Copy, Default)]
pub struct AuraEnvelopeParityPolicy;

/// Validation error for [`AuraEnvelopeParityPolicy`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AuraEnvelopeParityError {
    /// Candidate run exceeded the declared wave-width bound.
    #[error(
        "candidate wave width {candidate} exceeds declared bound {declared} (baseline {baseline})"
    )]
    WaveWidthExceeded {
        /// Observed baseline max wave width.
        baseline: usize,
        /// Observed candidate max wave width.
        candidate: usize,
        /// Declared admissible upper bound.
        declared: usize,
    },
    /// Scheduler divergence was outside commutative/algebraic policy.
    #[error("scheduler permutation class {class:?} is outside commutative policy")]
    SchedulerClassRejected {
        /// Rejected scheduler permutation class.
        class: SchedulerPermutationClass,
    },
    /// Effect ordering divergence was outside commutative/algebraic policy.
    #[error("effect ordering class {class:?} is outside commutative policy")]
    EffectOrderingClassRejected {
        /// Rejected effect ordering class.
        class: EffectOrderingClass,
    },
    /// Failure-visible divergence is not admissible.
    #[error("failure-visible divergence class {class:?} is not allowed")]
    FailureVisibleClassRejected {
        /// Rejected failure-visible class.
        class: FailureVisibleDiffClass,
    },
    /// Effect determinism tier is too weak for Aura parity gates.
    #[error("effect determinism tier {tier:?} is not allowed")]
    EffectTierRejected {
        /// Rejected effect determinism tier.
        tier: EffectDeterminismTier,
    },
}

impl AuraEnvelopeParityPolicy {
    /// Commutative/algebraic policy used by Aura parity gates.
    #[must_use]
    pub fn commutative_algebraic_only() -> Self {
        Self
    }

    /// Validate an envelope diff against Aura parity policy.
    ///
    /// # Errors
    ///
    /// Returns [`AuraEnvelopeParityError`] when the diff exceeds policy.
    pub fn validate(&self, diff: &EnvelopeDiff) -> Result<(), AuraEnvelopeParityError> {
        let wave = &diff.wave_width_bound;
        if !wave.within_declared_bound() {
            return Err(AuraEnvelopeParityError::WaveWidthExceeded {
                baseline: wave.baseline_max_wave_width,
                candidate: wave.candidate_max_wave_width,
                declared: wave.declared_upper_bound,
            });
        }

        if !matches!(
            diff.scheduler_permutation_class,
            SchedulerPermutationClass::Exact
                | SchedulerPermutationClass::SessionNormalizedPermutation
        ) {
            return Err(AuraEnvelopeParityError::SchedulerClassRejected {
                class: diff.scheduler_permutation_class,
            });
        }

        if !matches!(
            diff.effect_ordering_class,
            EffectOrderingClass::Exact | EffectOrderingClass::ReplayDeterministic
        ) {
            return Err(AuraEnvelopeParityError::EffectOrderingClassRejected {
                class: diff.effect_ordering_class,
            });
        }

        if !matches!(
            diff.failure_visible_diff_class,
            FailureVisibleDiffClass::Exact
        ) {
            return Err(AuraEnvelopeParityError::FailureVisibleClassRejected {
                class: diff.failure_visible_diff_class,
            });
        }

        if !matches!(
            diff.effect_determinism_tier,
            EffectDeterminismTier::StrictDeterministic | EffectDeterminismTier::ReplayDeterministic
        ) {
            return Err(AuraEnvelopeParityError::EffectTierRejected {
                tier: diff.effect_determinism_tier,
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use telltale_vm::envelope_diff::ENVELOPE_DIFF_SCHEMA_VERSION;
    use telltale_vm::WaveWidthBound;

    fn diff_fixture() -> EnvelopeDiff {
        EnvelopeDiff {
            schema_version: ENVELOPE_DIFF_SCHEMA_VERSION.to_string(),
            baseline_engine: "native_cooperative".to_string(),
            candidate_engine: "native_threaded".to_string(),
            scheduler_permutation_class: SchedulerPermutationClass::Exact,
            wave_width_bound: WaveWidthBound {
                baseline_max_wave_width: 1,
                candidate_max_wave_width: 1,
                declared_upper_bound: 1,
            },
            effect_ordering_class: EffectOrderingClass::Exact,
            failure_visible_diff_class: FailureVisibleDiffClass::Exact,
            effect_determinism_tier: EffectDeterminismTier::StrictDeterministic,
        }
    }

    #[test]
    fn policy_accepts_exact_diff() {
        let policy = AuraEnvelopeParityPolicy::commutative_algebraic_only();
        policy
            .validate(&diff_fixture())
            .expect("exact diff must pass");
    }

    #[test]
    fn policy_accepts_commutative_replay_diff() {
        let policy = AuraEnvelopeParityPolicy::commutative_algebraic_only();
        let mut diff = diff_fixture();
        diff.scheduler_permutation_class = SchedulerPermutationClass::SessionNormalizedPermutation;
        diff.effect_ordering_class = EffectOrderingClass::ReplayDeterministic;
        diff.effect_determinism_tier = EffectDeterminismTier::ReplayDeterministic;

        policy
            .validate(&diff)
            .expect("commutative replay diff must pass");
    }

    #[test]
    fn policy_rejects_envelope_bounded_scheduler() {
        let policy = AuraEnvelopeParityPolicy::commutative_algebraic_only();
        let mut diff = diff_fixture();
        diff.scheduler_permutation_class = SchedulerPermutationClass::EnvelopeBounded;

        let err = policy
            .validate(&diff)
            .expect_err("envelope bounded scheduler must fail");
        assert!(matches!(
            err,
            AuraEnvelopeParityError::SchedulerClassRejected { .. }
        ));
    }

    #[test]
    fn policy_rejects_failure_visible_drift() {
        let policy = AuraEnvelopeParityPolicy::commutative_algebraic_only();
        let mut diff = diff_fixture();
        diff.failure_visible_diff_class = FailureVisibleDiffClass::EnvelopeBounded;

        let err = policy
            .validate(&diff)
            .expect_err("failure-visible drift must fail");
        assert!(matches!(
            err,
            AuraEnvelopeParityError::FailureVisibleClassRejected { .. }
        ));
    }
}

//! Quantitative termination budgeting for choreography execution.
//!
//! Budgets are derived from Telltale's weighted measure:
//! `W = 2 * sum(depth(local_type)) + sum(buffer_sizes)`.

use std::fmt;

/// Protocol classes with calibrated scheduler factors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TerminationProtocolClass {
    /// Consensus fast path (short agreement rounds).
    ConsensusFastPath,
    /// Consensus fallback path (gossip/recovery).
    ConsensusFallback,
    /// Sync anti-entropy and reconciliation.
    SyncAntiEntropy,
    /// DKG/threshold key-generation ceremonies.
    DkgCeremony,
    /// Guardian recovery grant/approval flow.
    RecoveryGrant,
}

impl TerminationProtocolClass {
    /// Stable protocol identifier used in telemetry/docs.
    #[must_use]
    pub const fn protocol_id(self) -> &'static str {
        match self {
            Self::ConsensusFastPath => "aura.consensus.fast_path",
            Self::ConsensusFallback => "aura.consensus.fallback",
            Self::SyncAntiEntropy => "aura.sync.anti_entropy",
            Self::DkgCeremony => "aura.dkg.ceremony",
            Self::RecoveryGrant => "aura.recovery.grant",
        }
    }

    /// Scheduler factor `k_sigma` used for step bounds.
    #[must_use]
    pub const fn scheduler_factor(self) -> f64 {
        match self {
            Self::ConsensusFastPath => 1.5,
            Self::ConsensusFallback => 1.8,
            Self::SyncAntiEntropy => 1.8,
            Self::DkgCeremony => 1.6,
            Self::RecoveryGrant => 1.7,
        }
    }

    /// Typical weighted-measure ranges for documentation and sanity checks.
    #[must_use]
    pub const fn expected_weight_range(self) -> (u64, u64) {
        match self {
            Self::ConsensusFastPath => (16, 40),
            Self::ConsensusFallback => (80, 140),
            Self::SyncAntiEntropy => (420, 600),
            Self::DkgCeremony => (160, 260),
            Self::RecoveryGrant => (40, 70),
        }
    }

    /// Expected step-budget range derived from expected weight and scheduler factor.
    #[must_use]
    pub fn expected_step_budget_range(self) -> (u64, u64) {
        let (min_w, max_w) = self.expected_weight_range();
        (
            step_budget_from_weight(min_w, self.scheduler_factor()),
            step_budget_from_weight(max_w, self.scheduler_factor()),
        )
    }

    /// Map a stable Aura protocol identifier to a termination class.
    #[must_use]
    pub fn from_protocol_id(protocol_id: &str) -> Option<Self> {
        match protocol_id {
            "aura.consensus" | "aura.consensus.fallback" => Some(Self::ConsensusFallback),
            "aura.consensus.fast_path" => Some(Self::ConsensusFastPath),
            "aura.sync.epoch_rotation" | "aura.sync.anti_entropy" => Some(Self::SyncAntiEntropy),
            "aura.dkg.ceremony" => Some(Self::DkgCeremony),
            "aura.recovery.grant" => Some(Self::RecoveryGrant),
            _ => None,
        }
    }

    /// Long-running classes must carry termination evidence at admission time.
    #[must_use]
    pub const fn requires_termination_artifact(self) -> bool {
        matches!(
            self,
            Self::SyncAntiEntropy | Self::DkgCeremony | Self::RecoveryGrant
        )
    }
}

impl fmt::Display for TerminationProtocolClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.protocol_id())
    }
}

/// Runtime configuration for termination budgets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerminationBudgetConfig {
    /// Multiplier applied to computed budget (for controlled overrides).
    pub budget_multiplier: f64,
    /// Optional hard cap from caller/runtime config.
    pub hard_step_cap: Option<u64>,
    /// Warn when multiplier diverges from computed bound by this ratio.
    pub divergence_warn_ratio: f64,
}

impl Default for TerminationBudgetConfig {
    fn default() -> Self {
        Self {
            budget_multiplier: 1.0,
            hard_step_cap: None,
            divergence_warn_ratio: 1.5,
        }
    }
}

/// Deterministic termination budget for a protocol execution.
#[derive(Debug, Clone, PartialEq)]
pub struct TerminationBudget {
    /// Protocol class used to calibrate scheduler factor.
    pub protocol_class: TerminationProtocolClass,
    /// Initial weighted measure W(s0).
    pub initial_weight: u64,
    /// Scheduler factor `k_sigma`.
    pub scheduler_factor: f64,
    /// Applied configuration multiplier.
    pub budget_multiplier: f64,
    /// Maximum admissible steps.
    pub max_steps: u64,
    /// Steps consumed so far.
    pub steps_consumed: u64,
}

impl TerminationBudget {
    /// Build a budget from an initial weighted measure.
    pub fn from_weighted_measure(
        protocol_class: TerminationProtocolClass,
        initial_weight: u64,
        config: TerminationBudgetConfig,
    ) -> Result<Self, TerminationBudgetError> {
        if !config.budget_multiplier.is_finite() || config.budget_multiplier <= 0.0 {
            return Err(TerminationBudgetError::InvalidMultiplier {
                multiplier: config.budget_multiplier,
            });
        }
        if !config.divergence_warn_ratio.is_finite() || config.divergence_warn_ratio <= 1.0 {
            return Err(TerminationBudgetError::InvalidDivergenceRatio {
                ratio: config.divergence_warn_ratio,
            });
        }

        let scheduler_factor = protocol_class.scheduler_factor();
        let base_max_steps = step_budget_from_weight(initial_weight, scheduler_factor);
        let scaled_steps = (base_max_steps as f64 * config.budget_multiplier)
            .ceil()
            .max(1.0) as u64;
        let max_steps = config
            .hard_step_cap
            .map_or(scaled_steps, |cap| scaled_steps.min(cap.max(1)));

        Ok(Self {
            protocol_class,
            initial_weight,
            scheduler_factor,
            budget_multiplier: config.budget_multiplier,
            max_steps,
            steps_consumed: 0,
        })
    }

    /// Consume one scheduler step and enforce bound.
    pub fn check_progress(&mut self) -> Result<(), TerminationBudgetError> {
        self.steps_consumed = self.steps_consumed.saturating_add(1);
        if self.steps_consumed > self.max_steps {
            return Err(TerminationBudgetError::BoundExceeded {
                protocol_class: self.protocol_class,
                initial_weight: self.initial_weight,
                scheduler_factor: self.scheduler_factor,
                budget_multiplier: self.budget_multiplier,
                max_steps: self.max_steps,
                steps_consumed: self.steps_consumed,
            });
        }
        Ok(())
    }

    /// Fraction of budget consumed in range [0, +inf).
    #[must_use]
    pub fn utilization(&self) -> f64 {
        if self.max_steps == 0 {
            1.0
        } else {
            self.steps_consumed as f64 / self.max_steps as f64
        }
    }

    /// Whether the configured multiplier diverges from nominal bound.
    #[must_use]
    pub fn diverges_significantly(&self, config: TerminationBudgetConfig) -> bool {
        let ratio = if self.budget_multiplier >= 1.0 {
            self.budget_multiplier
        } else {
            1.0 / self.budget_multiplier
        };
        ratio >= config.divergence_warn_ratio
    }
}

/// Budgeting failures.
#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum TerminationBudgetError {
    /// Multiplier must be positive and finite.
    #[error("invalid termination budget multiplier: {multiplier}")]
    InvalidMultiplier {
        /// Invalid multiplier provided by runtime configuration.
        multiplier: f64,
    },
    /// Divergence ratio must be finite and > 1.
    #[error("invalid divergence ratio: {ratio}")]
    InvalidDivergenceRatio {
        /// Invalid divergence ratio provided by runtime configuration.
        ratio: f64,
    },
    /// Deterministic step bound exceeded.
    #[error(
        "termination bound exceeded for {protocol_class}: steps_consumed={steps_consumed}, max_steps={max_steps}, initial_weight={initial_weight}, scheduler_factor={scheduler_factor}, multiplier={budget_multiplier}"
    )]
    BoundExceeded {
        /// Protocol class used for budget derivation.
        protocol_class: TerminationProtocolClass,
        /// Initial weighted measure used for this execution.
        initial_weight: u64,
        /// Scheduler factor associated with the protocol class.
        scheduler_factor: f64,
        /// Runtime multiplier applied to the computed bound.
        budget_multiplier: f64,
        /// Maximum admissible steps.
        max_steps: u64,
        /// Steps consumed when violation was detected.
        steps_consumed: u64,
    },
}

#[must_use]
fn step_budget_from_weight(weight: u64, scheduler_factor: f64) -> u64 {
    ((weight as f64) * scheduler_factor).ceil().max(1.0) as u64
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn expected_ranges_match_design_targets() {
        assert_eq!(
            TerminationProtocolClass::ConsensusFastPath.expected_step_budget_range(),
            (24, 60)
        );
        assert_eq!(
            TerminationProtocolClass::ConsensusFallback.expected_step_budget_range(),
            (144, 252)
        );
        assert_eq!(
            TerminationProtocolClass::SyncAntiEntropy.expected_step_budget_range(),
            (756, 1080)
        );
        assert_eq!(
            TerminationProtocolClass::DkgCeremony.expected_step_budget_range(),
            (256, 416)
        );
        assert_eq!(
            TerminationProtocolClass::RecoveryGrant.expected_step_budget_range(),
            (68, 119)
        );
    }

    #[test]
    fn budget_exceeds_deterministically() {
        let mut budget = TerminationBudget::from_weighted_measure(
            TerminationProtocolClass::ConsensusFastPath,
            10,
            TerminationBudgetConfig {
                budget_multiplier: 1.0,
                hard_step_cap: Some(2),
                divergence_warn_ratio: 1.5,
            },
        )
        .expect("valid budget");

        assert!(budget.check_progress().is_ok());
        assert!(budget.check_progress().is_ok());
        assert!(matches!(
            budget.check_progress(),
            Err(TerminationBudgetError::BoundExceeded { .. })
        ));
    }

    #[test]
    fn invalid_budget_config_is_rejected() {
        let err = TerminationBudget::from_weighted_measure(
            TerminationProtocolClass::RecoveryGrant,
            10,
            TerminationBudgetConfig {
                budget_multiplier: 0.0,
                ..TerminationBudgetConfig::default()
            },
        )
        .expect_err("multiplier must be > 0");
        assert!(matches!(
            err,
            TerminationBudgetError::InvalidMultiplier { .. }
        ));
    }
}

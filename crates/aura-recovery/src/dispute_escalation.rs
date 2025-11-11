//! Dispute escalation tooling for guardian recovery workflows.
//!
//! Provides programmatic dispute handling, escalation paths, and resolution workflows.

use crate::types::{GuardianSet, RecoveryDispute};
use aura_core::{identifiers::GuardianId, AccountId, AuraError, AuraResult, DeviceId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Escalation severity levels for disputed recoveries
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EscalationLevel {
    /// Low priority - single guardian dispute
    Low = 1,
    /// Medium priority - multiple guardians or policy violations
    Medium = 2,
    /// High priority - threshold of guardians disputing
    High = 3,
    /// Critical - majority of guardians disputing or security concern
    Critical = 4,
}

/// Escalation actions available for dispute resolution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EscalationAction {
    /// Extend dispute window to allow more guardian review
    ExtendDisputeWindow { additional_secs: u64 },
    /// Require additional guardian approvals beyond threshold
    RequireAdditionalApprovals { additional_count: usize },
    /// Escalate to account administrator for review
    EscalateToAdmin { reason: String },
    /// Cancel recovery and notify all parties
    CancelRecovery { reason: String },
    /// Request guardian vote on disputed recovery
    RequestGuardianVote,
}

/// Escalation policy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationPolicy {
    /// Map from dispute count to escalation level
    pub dispute_threshold_map: HashMap<usize, EscalationLevel>,
    /// Actions to take per escalation level
    pub level_actions: HashMap<EscalationLevel, Vec<EscalationAction>>,
    /// Auto-cancel threshold (number of disputes)
    pub auto_cancel_dispute_count: usize,
}

impl Default for EscalationPolicy {
    fn default() -> Self {
        let mut dispute_threshold_map = HashMap::new();
        dispute_threshold_map.insert(1, EscalationLevel::Low);
        dispute_threshold_map.insert(2, EscalationLevel::Medium);
        dispute_threshold_map.insert(3, EscalationLevel::High);
        dispute_threshold_map.insert(5, EscalationLevel::Critical);

        let mut level_actions = HashMap::new();
        level_actions.insert(
            EscalationLevel::Low,
            vec![EscalationAction::ExtendDisputeWindow {
                additional_secs: 24 * 60 * 60,
            }],
        );
        level_actions.insert(
            EscalationLevel::Medium,
            vec![
                EscalationAction::ExtendDisputeWindow {
                    additional_secs: 48 * 60 * 60,
                },
                EscalationAction::RequireAdditionalApprovals {
                    additional_count: 1,
                },
            ],
        );
        level_actions.insert(
            EscalationLevel::High,
            vec![
                EscalationAction::RequestGuardianVote,
                EscalationAction::EscalateToAdmin {
                    reason: "High dispute threshold reached".to_string(),
                },
            ],
        );
        level_actions.insert(
            EscalationLevel::Critical,
            vec![EscalationAction::CancelRecovery {
                reason: "Critical dispute threshold - majority of guardians object".to_string(),
            }],
        );

        Self {
            dispute_threshold_map,
            level_actions,
            auto_cancel_dispute_count: 5,
        }
    }
}

/// Dispute escalation manager
#[derive(Debug, Clone)]
pub struct DisputeEscalationManager {
    policy: EscalationPolicy,
}

impl DisputeEscalationManager {
    /// Create new escalation manager with policy
    pub fn new(policy: EscalationPolicy) -> Self {
        Self { policy }
    }

    /// Create with default policy
    pub fn with_defaults() -> Self {
        Self::new(EscalationPolicy::default())
    }

    /// Evaluate disputes and determine escalation level
    pub fn evaluate_disputes(
        &self,
        disputes: &[RecoveryDispute],
        guardian_set: &GuardianSet,
    ) -> EscalationEvaluation {
        let dispute_count = disputes.len();

        // Determine escalation level
        let level = self
            .policy
            .dispute_threshold_map
            .iter()
            .filter(|(threshold, _)| dispute_count >= **threshold)
            .map(|(_, level)| *level)
            .max()
            .unwrap_or(EscalationLevel::Low);

        // Check if auto-cancel threshold reached
        let should_auto_cancel = dispute_count >= self.policy.auto_cancel_dispute_count;

        // Get recommended actions for this level
        let recommended_actions = self
            .policy
            .level_actions
            .get(&level)
            .cloned()
            .unwrap_or_default();

        // Calculate dispute ratio (guardians disputing / total guardians)
        let dispute_ratio = dispute_count as f64 / guardian_set.guardians.len().max(1) as f64;

        EscalationEvaluation {
            level,
            dispute_count,
            total_guardians: guardian_set.guardians.len(),
            dispute_ratio,
            should_auto_cancel,
            recommended_actions,
            disputing_guardians: disputes.iter().map(|d| d.guardian_id).collect(),
        }
    }

    /// Check if recovery should be blocked due to disputes
    pub fn should_block_recovery(&self, evaluation: &EscalationEvaluation) -> bool {
        evaluation.should_auto_cancel
            || evaluation.level >= EscalationLevel::Critical
            || evaluation.dispute_ratio > 0.5
    }

    /// Generate escalation notice for guardians and administrators
    pub fn generate_escalation_notice(
        &self,
        account_id: &AccountId,
        requesting_device: &DeviceId,
        evaluation: &EscalationEvaluation,
    ) -> EscalationNotice {
        EscalationNotice {
            account_id: *account_id,
            requesting_device: *requesting_device,
            level: evaluation.level,
            dispute_count: evaluation.dispute_count,
            disputing_guardians: evaluation.disputing_guardians.clone(),
            recommended_actions: evaluation.recommended_actions.clone(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }
}

/// Result of dispute escalation evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationEvaluation {
    /// Determined escalation level
    pub level: EscalationLevel,
    /// Number of disputes filed
    pub dispute_count: usize,
    /// Total number of guardians
    pub total_guardians: usize,
    /// Ratio of guardians disputing
    pub dispute_ratio: f64,
    /// Whether recovery should be auto-cancelled
    pub should_auto_cancel: bool,
    /// Recommended actions for this escalation level
    pub recommended_actions: Vec<EscalationAction>,
    /// Guardians who filed disputes
    pub disputing_guardians: Vec<GuardianId>,
}

/// Escalation notice sent to guardians and administrators
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationNotice {
    /// Account under disputed recovery
    pub account_id: AccountId,
    /// Device requesting recovery
    pub requesting_device: DeviceId,
    /// Escalation severity level
    pub level: EscalationLevel,
    /// Number of disputes
    pub dispute_count: usize,
    /// Guardians who filed disputes
    pub disputing_guardians: Vec<GuardianId>,
    /// Recommended escalation actions
    pub recommended_actions: Vec<EscalationAction>,
    /// Timestamp of escalation notice
    pub timestamp: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::GuardianId;

    #[test]
    fn test_escalation_levels() {
        let manager = DisputeEscalationManager::with_defaults();
        let guardian_set = GuardianSet {
            guardians: vec![
                GuardianId::new(),
                GuardianId::new(),
                GuardianId::new(),
                GuardianId::new(),
                GuardianId::new(),
            ]
            .into_iter()
            .map(|id| (id, Default::default()))
            .collect(),
        };

        // Single dispute = Low
        let disputes = vec![RecoveryDispute {
            guardian_id: GuardianId::new(),
            reason: "Suspicious activity".to_string(),
            filed_at: 1234567890,
        }];
        let eval = manager.evaluate_disputes(&disputes, &guardian_set);
        assert_eq!(eval.level, EscalationLevel::Low);
        assert!(!manager.should_block_recovery(&eval));

        // Three disputes = High
        let mut disputes = vec![];
        for _ in 0..3 {
            disputes.push(RecoveryDispute {
                guardian_id: GuardianId::new(),
                reason: "Suspicious".to_string(),
                filed_at: 1234567890,
            });
        }
        let eval = manager.evaluate_disputes(&disputes, &guardian_set);
        assert_eq!(eval.level, EscalationLevel::High);

        // Five disputes = Critical (auto-cancel)
        for _ in 0..2 {
            disputes.push(RecoveryDispute {
                guardian_id: GuardianId::new(),
                reason: "Suspicious".to_string(),
                filed_at: 1234567890,
            });
        }
        let eval = manager.evaluate_disputes(&disputes, &guardian_set);
        assert_eq!(eval.level, EscalationLevel::Critical);
        assert!(manager.should_block_recovery(&eval));
    }

    #[test]
    fn test_dispute_ratio() {
        let manager = DisputeEscalationManager::with_defaults();
        let guardian_set = GuardianSet {
            guardians: vec![GuardianId::new(), GuardianId::new()]
                .into_iter()
                .map(|id| (id, Default::default()))
                .collect(),
        };

        // 2 disputes out of 2 guardians = 100% ratio
        let disputes = vec![
            RecoveryDispute {
                guardian_id: GuardianId::new(),
                reason: "Test".to_string(),
                filed_at: 1234567890,
            },
            RecoveryDispute {
                guardian_id: GuardianId::new(),
                reason: "Test".to_string(),
                filed_at: 1234567890,
            },
        ];
        let eval = manager.evaluate_disputes(&disputes, &guardian_set);
        assert_eq!(eval.dispute_ratio, 1.0);
        assert!(manager.should_block_recovery(&eval)); // >50% ratio blocks
    }
}

//! Scoped OTA transition engine and mixed-version session behavior.

use aura_core::{AuthorityId, TimeStamp};
use aura_maintenance::{
    AuraActivationScope, AuraCompatibilityClass, AuraReleaseId, AuraUpgradeFailure,
    MaintenanceFact, ReleaseResidency, TransitionState, UpgradeExecutionFact,
};

/// Admission policy for new sessions during scoped rollout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewSessionAdmission {
    /// New sessions may continue as normal.
    Allow,
    /// New sessions incompatible with the target release must be rejected.
    RejectIncompatible,
}

/// Action for in-flight sessions that become incompatible during cutover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InFlightIncompatibilityAction {
    /// Let incompatible sessions drain naturally.
    Drain,
    /// Abort incompatible sessions once the local scope cuts over.
    Abort,
    /// Delegate incompatible sessions away before local cutover completes.
    Delegate,
}

/// Mixed-version and partition plan for one scoped upgrade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionCompatibilityPlan {
    /// Policy for new sessions during the current scope state.
    pub new_sessions: NewSessionAdmission,
    /// Policy for in-flight incompatible sessions.
    pub in_flight: InFlightIncompatibilityAction,
    /// Whether this scope should expect clean partitioning from incompatible peers.
    pub partition_required: bool,
}

impl SessionCompatibilityPlan {
    fn compatible_coexistence() -> Self {
        Self {
            new_sessions: NewSessionAdmission::Allow,
            in_flight: InFlightIncompatibilityAction::Drain,
            partition_required: false,
        }
    }
}

/// Scoped upgrade state tracked by the transition engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedUpgradeState {
    /// Scope owning this upgrade state machine.
    pub scope: AuraActivationScope,
    /// Release that is currently live before the target cutover.
    pub legacy_release_id: Option<AuraReleaseId>,
    /// Target release being staged or activated.
    pub target_release_id: AuraReleaseId,
    /// Compatibility class governing mixed-version behavior.
    pub compatibility: AuraCompatibilityClass,
    /// Current release residency in the scope.
    pub residency: ReleaseResidency,
    /// Current transition state in the scope.
    pub transition: TransitionState,
    /// Current session compatibility plan for the scope.
    pub session_plan: SessionCompatibilityPlan,
}

/// Deterministic rollback directive produced on activation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollbackDirective {
    /// Release being rolled back from.
    pub from_release_id: AuraReleaseId,
    /// Release being restored.
    pub to_release_id: AuraReleaseId,
    /// Structured failure that triggered rollback.
    pub failure: AuraUpgradeFailure,
}

/// Scoped OTA transition engine.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScopedOtaTransitionEngine;

impl ScopedOtaTransitionEngine {
    /// Create a new scoped OTA transition engine.
    pub fn new() -> Self {
        Self
    }

    /// Create the staged scope state for one target release.
    pub fn stage_scope(
        &self,
        scope: AuraActivationScope,
        legacy_release_id: Option<AuraReleaseId>,
        target_release_id: AuraReleaseId,
        compatibility: AuraCompatibilityClass,
    ) -> ScopedUpgradeState {
        let residency = match compatibility {
            AuraCompatibilityClass::BackwardCompatible
            | AuraCompatibilityClass::MixedCoexistenceAllowed => ReleaseResidency::Coexisting,
            AuraCompatibilityClass::ScopedHardFork
            | AuraCompatibilityClass::IncompatibleWithoutPartition => ReleaseResidency::LegacyOnly,
        };

        ScopedUpgradeState {
            scope,
            legacy_release_id,
            target_release_id,
            compatibility,
            residency,
            transition: TransitionState::AwaitingCutover,
            session_plan: SessionCompatibilityPlan::compatible_coexistence(),
        }
    }

    /// Move a staged scope into active cutover with explicit mixed-version behavior.
    pub fn begin_cutover(
        &self,
        mut state: ScopedUpgradeState,
        preferred_in_flight: InFlightIncompatibilityAction,
        delegation_supported: bool,
    ) -> ScopedUpgradeState {
        state.transition = TransitionState::CuttingOver;
        state.session_plan = self.session_plan_for_cutover(
            state.compatibility,
            preferred_in_flight,
            delegation_supported,
        );
        state
    }

    /// Commit a successful cutover.
    pub fn complete_cutover(&self, mut state: ScopedUpgradeState) -> ScopedUpgradeState {
        state.residency = ReleaseResidency::TargetOnly;
        state.transition = TransitionState::Idle;
        state
    }

    /// Start deterministic rollback after a failed cutover.
    pub fn begin_rollback(
        &self,
        mut state: ScopedUpgradeState,
        failure: AuraUpgradeFailure,
    ) -> Option<(ScopedUpgradeState, RollbackDirective)> {
        let legacy_release_id = state.legacy_release_id?;
        let directive = RollbackDirective {
            from_release_id: state.target_release_id,
            to_release_id: legacy_release_id,
            failure,
        };
        state.transition = TransitionState::RollingBack;
        state.session_plan = SessionCompatibilityPlan {
            new_sessions: NewSessionAdmission::RejectIncompatible,
            in_flight: InFlightIncompatibilityAction::Abort,
            partition_required: matches!(
                state.compatibility,
                AuraCompatibilityClass::IncompatibleWithoutPartition
            ),
        };
        Some((state, directive))
    }

    /// Commit a successful rollback to the legacy release.
    pub fn complete_rollback(&self, mut state: ScopedUpgradeState) -> ScopedUpgradeState {
        state.residency = ReleaseResidency::LegacyOnly;
        state.transition = TransitionState::Idle;
        state.session_plan = SessionCompatibilityPlan::compatible_coexistence();
        state
    }

    /// Build a rollback journal fact for a completed scoped rollback.
    pub fn rollback_executed_fact(
        &self,
        authority_id: AuthorityId,
        scope: AuraActivationScope,
        directive: &RollbackDirective,
        rolled_back_at: TimeStamp,
    ) -> MaintenanceFact {
        MaintenanceFact::UpgradeExecution(UpgradeExecutionFact::RollbackExecuted {
            authority_id,
            scope,
            from_release_id: directive.from_release_id,
            to_release_id: directive.to_release_id,
            failure: directive.failure.clone(),
            rolled_back_at,
        })
    }

    /// Build a partition observation journal fact for an incompatible scoped cutover.
    pub fn partition_observed_fact(
        &self,
        authority_id: AuthorityId,
        scope: AuraActivationScope,
        release_id: AuraReleaseId,
        failure: AuraUpgradeFailure,
        observed_at: TimeStamp,
    ) -> MaintenanceFact {
        MaintenanceFact::UpgradeExecution(UpgradeExecutionFact::PartitionObserved {
            authority_id,
            scope,
            release_id,
            failure,
            observed_at,
        })
    }

    fn session_plan_for_cutover(
        &self,
        compatibility: AuraCompatibilityClass,
        preferred_in_flight: InFlightIncompatibilityAction,
        delegation_supported: bool,
    ) -> SessionCompatibilityPlan {
        match compatibility {
            AuraCompatibilityClass::BackwardCompatible
            | AuraCompatibilityClass::MixedCoexistenceAllowed => {
                SessionCompatibilityPlan::compatible_coexistence()
            }
            AuraCompatibilityClass::ScopedHardFork => SessionCompatibilityPlan {
                new_sessions: NewSessionAdmission::RejectIncompatible,
                in_flight: self.resolve_in_flight_action(
                    preferred_in_flight,
                    delegation_supported,
                    InFlightIncompatibilityAction::Drain,
                ),
                partition_required: false,
            },
            AuraCompatibilityClass::IncompatibleWithoutPartition => SessionCompatibilityPlan {
                new_sessions: NewSessionAdmission::RejectIncompatible,
                in_flight: self.resolve_in_flight_action(
                    preferred_in_flight,
                    delegation_supported,
                    InFlightIncompatibilityAction::Abort,
                ),
                partition_required: true,
            },
        }
    }

    fn resolve_in_flight_action(
        &self,
        preferred: InFlightIncompatibilityAction,
        delegation_supported: bool,
        fallback: InFlightIncompatibilityAction,
    ) -> InFlightIncompatibilityAction {
        match preferred {
            InFlightIncompatibilityAction::Delegate if !delegation_supported => fallback,
            _ => preferred,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::time::PhysicalTime;
    use aura_core::Hash32;
    use aura_maintenance::AuraUpgradeFailureClass;

    fn release(byte: u8) -> AuraReleaseId {
        AuraReleaseId::new(Hash32([byte; 32]))
    }

    fn scope() -> AuraActivationScope {
        AuraActivationScope::AuthorityLocal {
            authority_id: aura_core::AuthorityId::new_from_entropy([7; 32]),
        }
    }

    fn ts(ms: u64) -> TimeStamp {
        TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: ms,
            uncertainty: Some(5),
        })
    }

    #[test]
    fn soft_fork_scopes_start_in_coexisting_mode() {
        let engine = ScopedOtaTransitionEngine::new();
        let state = engine.stage_scope(
            scope(),
            Some(release(1)),
            release(2),
            AuraCompatibilityClass::MixedCoexistenceAllowed,
        );

        assert_eq!(state.residency, ReleaseResidency::Coexisting);
        assert_eq!(state.transition, TransitionState::AwaitingCutover);
        assert_eq!(state.session_plan.new_sessions, NewSessionAdmission::Allow);
    }

    #[test]
    fn incompatible_cutover_rejects_new_sessions_and_can_delegate() {
        let engine = ScopedOtaTransitionEngine::new();
        let staged = engine.stage_scope(
            scope(),
            Some(release(1)),
            release(2),
            AuraCompatibilityClass::IncompatibleWithoutPartition,
        );
        let cutting_over =
            engine.begin_cutover(staged, InFlightIncompatibilityAction::Delegate, true);

        assert_eq!(cutting_over.transition, TransitionState::CuttingOver);
        assert_eq!(
            cutting_over.session_plan.new_sessions,
            NewSessionAdmission::RejectIncompatible
        );
        assert_eq!(
            cutting_over.session_plan.in_flight,
            InFlightIncompatibilityAction::Delegate
        );
        assert!(cutting_over.session_plan.partition_required);
    }

    #[test]
    fn rollback_is_deterministic_and_restores_legacy_mode() {
        let engine = ScopedOtaTransitionEngine::new();
        let staged = engine.stage_scope(
            scope(),
            Some(release(1)),
            release(2),
            AuraCompatibilityClass::ScopedHardFork,
        );
        let cutting_over =
            engine.begin_cutover(staged, InFlightIncompatibilityAction::Drain, false);
        let (rolling_back, directive) = engine
            .begin_rollback(
                cutting_over,
                AuraUpgradeFailure::new(
                    AuraUpgradeFailureClass::HealthGateFailed,
                    "health gate failed",
                ),
            )
            .unwrap_or_else(|| panic!("legacy release exists"));
        assert_eq!(rolling_back.transition, TransitionState::RollingBack);
        assert_eq!(directive.from_release_id, release(2));
        assert_eq!(directive.to_release_id, release(1));
        assert_eq!(
            directive.failure,
            AuraUpgradeFailure::new(
                AuraUpgradeFailureClass::HealthGateFailed,
                "health gate failed",
            )
        );

        let restored = engine.complete_rollback(rolling_back);
        assert_eq!(restored.residency, ReleaseResidency::LegacyOnly);
        assert_eq!(restored.transition, TransitionState::Idle);
    }

    #[test]
    fn rollback_and_partition_facts_preserve_failure_classification() {
        let engine = ScopedOtaTransitionEngine::new();
        let staged = engine.stage_scope(
            scope(),
            Some(release(1)),
            release(2),
            AuraCompatibilityClass::IncompatibleWithoutPartition,
        );
        let cutting_over =
            engine.begin_cutover(staged, InFlightIncompatibilityAction::Abort, false);
        let (rolling_back, directive) = engine
            .begin_rollback(
                cutting_over.clone(),
                AuraUpgradeFailure::new(
                    AuraUpgradeFailureClass::HealthGateFailed,
                    "post-cutover health failure",
                ),
            )
            .unwrap_or_else(|| panic!("legacy release exists"));
        let authority_id = aura_core::AuthorityId::new_from_entropy([8; 32]);
        let rollback_fact =
            engine.rollback_executed_fact(authority_id, scope(), &directive, ts(10));
        assert_eq!(
            rollback_fact,
            MaintenanceFact::UpgradeExecution(UpgradeExecutionFact::RollbackExecuted {
                authority_id,
                scope: scope(),
                from_release_id: directive.from_release_id,
                to_release_id: directive.to_release_id,
                failure: directive.failure.clone(),
                rolled_back_at: ts(10),
            })
        );

        let partition_fact = engine.partition_observed_fact(
            authority_id,
            rolling_back.scope.clone(),
            rolling_back.target_release_id,
            AuraUpgradeFailure::new(
                AuraUpgradeFailureClass::PartitionRequired,
                "incompatible peer partition required",
            ),
            ts(11),
        );
        assert_eq!(
            partition_fact,
            MaintenanceFact::UpgradeExecution(UpgradeExecutionFact::PartitionObserved {
                authority_id,
                scope: rolling_back.scope,
                release_id: rolling_back.target_release_id,
                failure: AuraUpgradeFailure::new(
                    AuraUpgradeFailureClass::PartitionRequired,
                    "incompatible peer partition required",
                ),
                observed_at: ts(11),
            })
        );
    }
}

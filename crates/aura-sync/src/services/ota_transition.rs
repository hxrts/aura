//! OTA compatibility-policy helpers and maintenance facts.

use aura_core::{AuthorityId, TimeStamp};
use aura_maintenance::{
    AuraActivationScope, AuraCompatibilityClass, AuraReleaseId, AuraUpgradeFailure,
    MaintenanceFact, UpgradeExecutionFact,
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

/// Observed scoped OTA state projected from canonical runtime-upgrade artifacts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedUpgradeState {
    /// Scope owning this upgrade state.
    pub scope: AuraActivationScope,
    /// Release that is currently live before the target cutover.
    pub legacy_release_id: Option<AuraReleaseId>,
    /// Target release being staged or activated.
    pub target_release_id: AuraReleaseId,
    /// Compatibility class governing mixed-version behavior.
    pub compatibility: AuraCompatibilityClass,
    /// Current release residency in the scope.
    pub residency: aura_maintenance::ReleaseResidency,
    /// Current transition state in the scope.
    pub transition: aura_maintenance::TransitionState,
    /// Current session compatibility plan for the scope.
    pub session_plan: SessionCompatibilityPlan,
}

impl SessionCompatibilityPlan {
    /// Compatibility plan for coexistence-safe states.
    #[must_use]
    pub fn compatible_coexistence() -> Self {
        Self {
            new_sessions: NewSessionAdmission::Allow,
            in_flight: InFlightIncompatibilityAction::Drain,
            partition_required: false,
        }
    }
}

/// Resolve the scoped mixed-version plan for an active cutover.
#[must_use]
pub fn cutover_session_plan(
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
            in_flight: resolve_in_flight_action(
                preferred_in_flight,
                delegation_supported,
                InFlightIncompatibilityAction::Drain,
            ),
            partition_required: false,
        },
        AuraCompatibilityClass::IncompatibleWithoutPartition => SessionCompatibilityPlan {
            new_sessions: NewSessionAdmission::RejectIncompatible,
            in_flight: resolve_in_flight_action(
                preferred_in_flight,
                delegation_supported,
                InFlightIncompatibilityAction::Abort,
            ),
            partition_required: true,
        },
    }
}

/// Resolve the scoped mixed-version plan for an explicit rollback.
#[must_use]
pub fn rollback_session_plan(compatibility: AuraCompatibilityClass) -> SessionCompatibilityPlan {
    SessionCompatibilityPlan {
        new_sessions: NewSessionAdmission::RejectIncompatible,
        in_flight: InFlightIncompatibilityAction::Abort,
        partition_required: matches!(
            compatibility,
            AuraCompatibilityClass::IncompatibleWithoutPartition
        ),
    }
}

/// Determine the staged residency before an OTA scope has cut over.
#[must_use]
pub fn staged_residency_for_compatibility(
    compatibility: AuraCompatibilityClass,
) -> aura_maintenance::ReleaseResidency {
    match compatibility {
        AuraCompatibilityClass::BackwardCompatible
        | AuraCompatibilityClass::MixedCoexistenceAllowed => {
            aura_maintenance::ReleaseResidency::Coexisting
        }
        AuraCompatibilityClass::ScopedHardFork
        | AuraCompatibilityClass::IncompatibleWithoutPartition => {
            aura_maintenance::ReleaseResidency::LegacyOnly
        }
    }
}

/// Build a rollback journal fact for a completed scoped rollback.
#[must_use]
pub fn rollback_executed_fact(
    authority_id: AuthorityId,
    scope: AuraActivationScope,
    from_release_id: AuraReleaseId,
    to_release_id: AuraReleaseId,
    failure: AuraUpgradeFailure,
    rolled_back_at: TimeStamp,
) -> MaintenanceFact {
    MaintenanceFact::UpgradeExecution(UpgradeExecutionFact::RollbackExecuted {
        authority_id,
        scope,
        from_release_id,
        to_release_id,
        failure,
        rolled_back_at,
    })
}

/// Build a partition observation journal fact for an incompatible scoped cutover.
#[must_use]
pub fn partition_observed_fact(
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

fn resolve_in_flight_action(
    preferred: InFlightIncompatibilityAction,
    delegation_supported: bool,
    fallback: InFlightIncompatibilityAction,
) -> InFlightIncompatibilityAction {
    match preferred {
        InFlightIncompatibilityAction::Delegate if !delegation_supported => fallback,
        _ => preferred,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::time::PhysicalTime;
    use aura_core::Hash32;
    use aura_maintenance::{
        AuraUpgradeFailureClass, ReleaseResidency, TransitionState, UpgradeExecutionFact,
    };

    fn release(byte: u8) -> AuraReleaseId {
        AuraReleaseId::new(Hash32([byte; 32]))
    }

    fn scope() -> AuraActivationScope {
        AuraActivationScope::AuthorityLocal {
            authority_id: AuthorityId::new_from_entropy([1; 32]),
        }
    }

    fn ts(ms: u64) -> TimeStamp {
        TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: ms,
            uncertainty: None,
        })
    }

    #[test]
    fn staged_residency_matches_compatibility_contract() {
        assert_eq!(
            staged_residency_for_compatibility(AuraCompatibilityClass::BackwardCompatible),
            ReleaseResidency::Coexisting
        );
        assert_eq!(
            staged_residency_for_compatibility(
                AuraCompatibilityClass::IncompatibleWithoutPartition
            ),
            ReleaseResidency::LegacyOnly
        );
    }

    #[test]
    fn hard_fork_cutover_rejects_incompatible_sessions() {
        let plan = cutover_session_plan(
            AuraCompatibilityClass::ScopedHardFork,
            InFlightIncompatibilityAction::Delegate,
            true,
        );
        assert_eq!(plan.new_sessions, NewSessionAdmission::RejectIncompatible);
        assert_eq!(plan.in_flight, InFlightIncompatibilityAction::Delegate);
        assert!(!plan.partition_required);
    }

    #[test]
    fn incompatible_partition_cutover_requires_partition_and_abort_fallback() {
        let plan = cutover_session_plan(
            AuraCompatibilityClass::IncompatibleWithoutPartition,
            InFlightIncompatibilityAction::Delegate,
            false,
        );
        assert_eq!(plan.new_sessions, NewSessionAdmission::RejectIncompatible);
        assert_eq!(plan.in_flight, InFlightIncompatibilityAction::Abort);
        assert!(plan.partition_required);
    }

    #[test]
    fn rollback_plan_is_fail_closed() {
        let plan = rollback_session_plan(AuraCompatibilityClass::IncompatibleWithoutPartition);
        assert_eq!(plan.new_sessions, NewSessionAdmission::RejectIncompatible);
        assert_eq!(plan.in_flight, InFlightIncompatibilityAction::Abort);
        assert!(plan.partition_required);
    }

    #[test]
    fn rollback_fact_records_structured_failure() {
        let fact = rollback_executed_fact(
            AuthorityId::new_from_entropy([2; 32]),
            scope(),
            release(9),
            release(8),
            AuraUpgradeFailure::new(AuraUpgradeFailureClass::HealthGateFailed, "health gate"),
            ts(10),
        );
        assert!(matches!(
            fact,
            MaintenanceFact::UpgradeExecution(UpgradeExecutionFact::RollbackExecuted {
                from_release_id,
                to_release_id,
                ..
            }) if from_release_id == release(9) && to_release_id == release(8)
        ));
    }

    #[test]
    fn partition_fact_records_incompatible_observation() {
        let fact = partition_observed_fact(
            AuthorityId::new_from_entropy([3; 32]),
            scope(),
            release(7),
            AuraUpgradeFailure::new(AuraUpgradeFailureClass::PartitionRequired, "partition"),
            ts(20),
        );
        assert!(matches!(
            fact,
            MaintenanceFact::UpgradeExecution(UpgradeExecutionFact::PartitionObserved {
                release_id,
                ..
            }) if release_id == release(7)
        ));
    }

    #[test]
    fn scoped_upgrade_state_is_an_observed_projection_shape() {
        let state = ScopedUpgradeState {
            scope: scope(),
            legacy_release_id: Some(release(1)),
            target_release_id: release(2),
            compatibility: AuraCompatibilityClass::BackwardCompatible,
            residency: ReleaseResidency::TargetOnly,
            transition: TransitionState::Idle,
            session_plan: SessionCompatibilityPlan::compatible_coexistence(),
        };
        assert_eq!(state.transition, TransitionState::Idle);
        assert_eq!(state.residency, ReleaseResidency::TargetOnly);
    }
}

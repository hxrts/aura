//! Integration test for guard effect types

use aura_core::{
    effects::guard::{EffectCommand, GuardOutcome},
    identifiers::{AuthorityId, ContextId},
    FlowCost,
};

#[test]
fn test_guard_outcome_creation() {
    // Test pure guard outcome creation
    let effects = vec![
        EffectCommand::ChargeBudget {
            context: ContextId::new_from_entropy([1u8; 32]),
            authority: AuthorityId::new_from_entropy([2u8; 32]),
            peer: AuthorityId::new_from_entropy([3u8; 32]),
            amount: FlowCost::new(100),
        },
        EffectCommand::RecordLeakage { bits: 64 },
    ];

    let outcome = GuardOutcome::authorized(effects);
    assert!(outcome.is_authorized());
    assert_eq!(outcome.effects.len(), 2);

    let denied = GuardOutcome::denied("Test denial");
    assert!(!denied.is_authorized());
    assert_eq!(denied.effects.len(), 0);
}

//! Golden test: GuardPlan fields match a choreography-generated GuardRequest.
//!
//! If this test breaks, the guard plan construction has drifted from
//! the choreography annotation contract.

#![allow(missing_docs)]
use super::support::{test_authority, test_context};
use aura_core::FlowCost;
use aura_guards::chain::create_send_guard_op;
use aura_guards::executor::GuardPlan;
use aura_guards::guards::pure::GuardRequest;
use aura_guards::GuardOperation;

#[test]
fn guard_plan_matches_choreography_request() {
    let authority = test_authority(1);
    let peer = test_authority(2);
    let context = test_context(3);

    let send_guard = create_send_guard_op(GuardOperation::AmpSend, context, peer, FlowCost::new(1));
    let plan = match GuardPlan::from_send_guard(&send_guard, authority) {
        Ok(plan) => plan,
        Err(err) => panic!("plan: {err}"),
    };

    let request = GuardRequest::new(authority, "amp:send".to_string(), FlowCost::new(1))
        .with_context_id(context)
        .with_peer(peer)
        .with_context(context.to_bytes().to_vec());

    assert_eq!(plan.request().operation, request.operation);
    assert_eq!(plan.request().cost, request.cost);
    assert_eq!(plan.request().context, request.context);
    assert_eq!(plan.request().peer, request.peer);
    assert_eq!(plan.request().authority, request.authority);
    assert!(plan.additional_commands().is_empty());
}

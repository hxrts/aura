#![allow(missing_docs)]
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_guards::chain::create_send_guard_op;
use aura_guards::executor::GuardPlan;
use aura_guards::guards::pure::GuardRequest;
use aura_guards::GuardOperation;

#[test]
fn guard_plan_matches_choreography_request() {
    let authority = AuthorityId::new_from_entropy([1u8; 32]);
    let peer = AuthorityId::new_from_entropy([2u8; 32]);
    let context = ContextId::new_from_entropy([3u8; 32]);

    let send_guard = create_send_guard_op(GuardOperation::AmpSend, context, peer, 1);
    let plan = match GuardPlan::from_send_guard(&send_guard, authority) {
        Ok(plan) => plan,
        Err(err) => panic!("plan: {err}"),
    };

    let request = GuardRequest::new(authority, "amp:send".to_string(), 1)
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

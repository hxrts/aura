//! Integration tests for guard enforcement in consensus protocol
//!
//! These tests verify that guard chains are properly configured with
//! correct capabilities, flow costs, and operation IDs.

use aura_consensus::protocol::{
    ConsensusResultGuard, ExecuteGuard, NonceCommitGuard, SignRequestGuard, SignShareGuard,
};
use aura_core::{identifiers::{AuthorityId, ContextId}, FlowCost};

#[test]
fn test_execute_guard_configuration() {
    let context = ContextId::new_from_entropy([1u8; 32]);
    let peer = AuthorityId::new_from_entropy([2u8; 32]);

    let guard = ExecuteGuard::new(context, peer);
    let chain = guard.create_guard_chain();

    assert_eq!(chain.authorization_requirement(), "consensus:initiate");
    assert_eq!(chain.cost(), FlowCost::from(100u32));
    assert_eq!(chain.context(), context);
    assert_eq!(chain.peer(), peer);
}

#[test]
fn test_nonce_commit_guard_configuration() {
    let context = ContextId::new_from_entropy([1u8; 32]);
    let coordinator = AuthorityId::new_from_entropy([2u8; 32]);

    let guard = NonceCommitGuard::new(context, coordinator);
    let chain = guard.create_guard_chain();

    assert_eq!(chain.authorization_requirement(), "consensus:witness_nonce");
    assert_eq!(chain.cost(), FlowCost::from(50u32));
    assert_eq!(chain.context(), context);
    assert_eq!(chain.peer(), coordinator);
}

#[test]
fn test_sign_request_guard_configuration() {
    let context = ContextId::new_from_entropy([1u8; 32]);
    let witness = AuthorityId::new_from_entropy([3u8; 32]);

    let guard = SignRequestGuard::new(context, witness);
    let chain = guard.create_guard_chain();

    assert_eq!(chain.authorization_requirement(), "consensus:aggregate_nonces");
    assert_eq!(chain.cost(), FlowCost::from(75u32));
    assert_eq!(chain.context(), context);
    assert_eq!(chain.peer(), witness);
}

#[test]
fn test_sign_share_guard_configuration() {
    let context = ContextId::new_from_entropy([1u8; 32]);
    let coordinator = AuthorityId::new_from_entropy([2u8; 32]);

    let guard = SignShareGuard::new(context, coordinator);
    let chain = guard.create_guard_chain();

    assert_eq!(chain.authorization_requirement(), "consensus:witness_sign");
    assert_eq!(chain.cost(), FlowCost::from(50u32));
    assert_eq!(chain.context(), context);
    assert_eq!(chain.peer(), coordinator);
}

#[test]
fn test_consensus_result_guard_configuration() {
    let context = ContextId::new_from_entropy([1u8; 32]);
    let witness = AuthorityId::new_from_entropy([3u8; 32]);

    let guard = ConsensusResultGuard::new(context, witness);
    let chain = guard.create_guard_chain();

    assert_eq!(chain.authorization_requirement(), "consensus:finalize");
    assert_eq!(chain.cost(), FlowCost::from(100u32));
    assert_eq!(chain.context(), context);
    assert_eq!(chain.peer(), witness);
}

/// Test that all guards have the correct flow costs as specified in choreography annotations
#[test]
fn test_all_guards_have_correct_flow_costs() {
    let context = ContextId::new_from_entropy([1u8; 32]);
    let peer = AuthorityId::new_from_entropy([2u8; 32]);

    // Execute: 100 (annotation: flow_cost=100)
    let execute = ExecuteGuard::new(context, peer);
    let chain = execute.create_guard_chain();
    assert_eq!(chain.cost(), FlowCost::from(100u32));

    // NonceCommit: 50 (annotation: flow_cost=50)
    let nonce = NonceCommitGuard::new(context, peer);
    let chain = nonce.create_guard_chain();
    assert_eq!(chain.cost(), FlowCost::from(50u32));

    // SignRequest: 75 (annotation: flow_cost=75)
    let sign_req = SignRequestGuard::new(context, peer);
    let chain = sign_req.create_guard_chain();
    assert_eq!(chain.cost(), FlowCost::from(75u32));

    // SignShare: 50 (annotation: flow_cost=50)
    let sign_share = SignShareGuard::new(context, peer);
    let chain = sign_share.create_guard_chain();
    assert_eq!(chain.cost(), FlowCost::from(50u32));

    // ConsensusResult: 100 (annotation: flow_cost=100)
    let result = ConsensusResultGuard::new(context, peer);
    let chain = result.create_guard_chain();
    assert_eq!(chain.cost(), FlowCost::from(100u32));
}


/// Test that guard chains are properly configured with context and peer
#[test]
fn test_guard_chain_context_and_peer() {
    let context = ContextId::new_from_entropy([1u8; 32]);
    let peer = AuthorityId::new_from_entropy([2u8; 32]);

    let execute = ExecuteGuard::new(context, peer);
    let chain = execute.create_guard_chain();

    assert_eq!(chain.context(), context, "Guard should preserve context");
    assert_eq!(chain.peer(), peer, "Guard should preserve peer");
}

/// Test that authorization requirements match choreography annotations
#[test]
fn test_authorization_requirements() {
    let context = ContextId::new_from_entropy([1u8; 32]);
    let peer = AuthorityId::new_from_entropy([2u8; 32]);

    // From choreography annotations:
    // Execute: guard_capability="initiate_consensus"
    let execute = ExecuteGuard::new(context, peer);
    assert_eq!(
        execute.create_guard_chain().authorization_requirement(),
        "consensus:initiate"
    );

    // NonceCommit: guard_capability="witness_nonce"
    let nonce = NonceCommitGuard::new(context, peer);
    assert_eq!(
        nonce.create_guard_chain().authorization_requirement(),
        "consensus:witness_nonce"
    );

    // SignRequest: guard_capability="aggregate_nonces"
    let sign_req = SignRequestGuard::new(context, peer);
    assert_eq!(
        sign_req.create_guard_chain().authorization_requirement(),
        "consensus:aggregate_nonces"
    );

    // SignShare: guard_capability="witness_sign"
    let sign_share = SignShareGuard::new(context, peer);
    assert_eq!(
        sign_share.create_guard_chain().authorization_requirement(),
        "consensus:witness_sign"
    );

    // ConsensusResult: guard_capability="finalize_consensus"
    let result = ConsensusResultGuard::new(context, peer);
    assert_eq!(
        result.create_guard_chain().authorization_requirement(),
        "consensus:finalize"
    );
}

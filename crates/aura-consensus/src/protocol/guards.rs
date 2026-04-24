// Guard helpers for consensus protocol choreography annotations
//!
//! This module provides guard chain integration for the consensus protocol,
//! mapping choreography annotations to runtime guard enforcement.
//!
//! # Choreography Annotations
//!
//! From `choreography.tell`:
//! - Execute: guard_capability="consensus:initiate", flow_cost=100
//! - NonceCommit: guard_capability="consensus:witness_nonce", flow_cost=50
//! - SignRequest: guard_capability="consensus:aggregate_nonces", flow_cost=75
//! - SignShare: guard_capability="consensus:witness_sign", flow_cost=50, leak="pipelined_commitment"
//! - ConsensusResult: guard_capability="consensus:finalize", flow_cost=100, journal_facts="consensus_complete"

use crate::capabilities::ConsensusCapability;
use aura_core::{AuraResult, AuthorityId, CapabilityName, ContextId, FlowCost};
use aura_guards::{
    GuardContextProvider, GuardEffects, GuardOperationId, LeakageBudget, SendGuardChain,
    SendGuardResult,
};

fn build_guard_chain(
    capability: CapabilityName,
    context: ContextId,
    peer: AuthorityId,
    cost: FlowCost,
    operation_id: &'static str,
    leakage_budget: Option<LeakageBudget>,
) -> SendGuardChain {
    let operation_id =
        GuardOperationId::custom(operation_id).expect("consensus guard operations are valid");
    let chain =
        SendGuardChain::new(capability, context, peer, cost).with_operation_id(operation_id);

    if let Some(leakage_budget) = leakage_budget {
        chain.with_leakage_budget(leakage_budget)
    } else {
        chain
    }
}

async fn evaluate_guard_chain<E>(chain: SendGuardChain, effects: &E) -> AuraResult<SendGuardResult>
where
    E: GuardEffects + GuardContextProvider + aura_core::PhysicalTimeEffects,
{
    chain.evaluate(effects).await
}

fn pipelined_commitment_leakage() -> LeakageBudget {
    LeakageBudget::new(0, 32, 0)
}

/// Guard configuration for Execute message (Coordinator -> Witness)
pub struct ExecuteGuard {
    context: ContextId,
    peer: AuthorityId,
}

impl ExecuteGuard {
    pub fn new(context: ContextId, peer: AuthorityId) -> Self {
        Self { context, peer }
    }

    /// Create guard chain for Execute message
    /// Annotations: guard_capability="consensus:initiate", flow_cost=100
    pub fn create_guard_chain(&self) -> SendGuardChain {
        build_guard_chain(
            ConsensusCapability::Initiate.as_name(),
            self.context,
            self.peer,
            FlowCost::from(100u32),
            "consensus_execute",
            None,
        )
    }

    /// Evaluate guard chain before sending Execute message
    pub async fn evaluate<E>(&self, effects: &E) -> AuraResult<SendGuardResult>
    where
        E: GuardEffects + GuardContextProvider + aura_core::PhysicalTimeEffects,
    {
        evaluate_guard_chain(self.create_guard_chain(), effects).await
    }
}

/// Guard configuration for NonceCommit message (Witness -> Coordinator)
pub struct NonceCommitGuard {
    context: ContextId,
    peer: AuthorityId,
}

impl NonceCommitGuard {
    pub fn new(context: ContextId, peer: AuthorityId) -> Self {
        Self { context, peer }
    }

    /// Create guard chain for NonceCommit message
    /// Annotations: guard_capability="consensus:witness_nonce", flow_cost=50
    pub fn create_guard_chain(&self) -> SendGuardChain {
        build_guard_chain(
            ConsensusCapability::WitnessNonce.as_name(),
            self.context,
            self.peer,
            FlowCost::from(50u32),
            "consensus_nonce_commit",
            None,
        )
    }

    /// Evaluate guard chain before sending NonceCommit message
    pub async fn evaluate<E>(&self, effects: &E) -> AuraResult<SendGuardResult>
    where
        E: GuardEffects + GuardContextProvider + aura_core::PhysicalTimeEffects,
    {
        evaluate_guard_chain(self.create_guard_chain(), effects).await
    }
}

/// Guard configuration for SignRequest message (Coordinator -> Witness)
pub struct SignRequestGuard {
    context: ContextId,
    peer: AuthorityId,
}

impl SignRequestGuard {
    pub fn new(context: ContextId, peer: AuthorityId) -> Self {
        Self { context, peer }
    }

    /// Create guard chain for SignRequest message
    /// Annotations: guard_capability="consensus:aggregate_nonces", flow_cost=75
    pub fn create_guard_chain(&self) -> SendGuardChain {
        build_guard_chain(
            ConsensusCapability::AggregateNonces.as_name(),
            self.context,
            self.peer,
            FlowCost::from(75u32),
            "consensus_sign_request",
            None,
        )
    }

    /// Evaluate guard chain before sending SignRequest message
    pub async fn evaluate<E>(&self, effects: &E) -> AuraResult<SendGuardResult>
    where
        E: GuardEffects + GuardContextProvider + aura_core::PhysicalTimeEffects,
    {
        evaluate_guard_chain(self.create_guard_chain(), effects).await
    }
}

/// Guard configuration for SignShare message (Witness -> Coordinator)
pub struct SignShareGuard {
    context: ContextId,
    peer: AuthorityId,
}

impl SignShareGuard {
    pub fn new(context: ContextId, peer: AuthorityId) -> Self {
        Self { context, peer }
    }

    /// Create guard chain for SignShare message
    /// Annotations: guard_capability="consensus:witness_sign", flow_cost=50, leak="pipelined_commitment"
    pub fn create_guard_chain(&self) -> SendGuardChain {
        build_guard_chain(
            ConsensusCapability::WitnessSign.as_name(),
            self.context,
            self.peer,
            FlowCost::from(50u32),
            "consensus_sign_share",
            Some(pipelined_commitment_leakage()),
        )
    }

    /// Evaluate guard chain before sending SignShare message
    pub async fn evaluate<E>(&self, effects: &E) -> AuraResult<SendGuardResult>
    where
        E: GuardEffects + GuardContextProvider + aura_core::PhysicalTimeEffects,
    {
        evaluate_guard_chain(self.create_guard_chain(), effects).await
    }
}

/// Guard configuration for ConsensusResult message (Coordinator -> Witness)
pub struct ConsensusResultGuard {
    context: ContextId,
    peer: AuthorityId,
}

impl ConsensusResultGuard {
    pub fn new(context: ContextId, peer: AuthorityId) -> Self {
        Self { context, peer }
    }

    /// Create guard chain for ConsensusResult message
    /// Annotations: guard_capability="consensus:finalize", flow_cost=100, journal_facts="consensus_complete"
    ///
    /// # Journal Coupling Pattern
    ///
    /// The journal_facts annotation indicates that consensus completion should be recorded
    /// in the journal. This is enforced at the runtime bridge layer (aura-agent), not in
    /// the protocol layer:
    ///
    /// 1. Protocol (`coordinator::finalize_consensus`) creates CommitFact
    /// 2. Runtime bridge commits CommitFact via `commit_relational_facts()`
    /// 3. Runtime bridge broadcasts ConsensusResult message
    ///
    /// This pattern enforces charge-before-send at the correct architectural layer where
    /// both journal and transport effects are available.
    ///
    /// See: `aura-agent/src/runtime_bridge/consensus.rs` for the implementation.
    pub fn create_guard_chain(&self) -> SendGuardChain {
        build_guard_chain(
            ConsensusCapability::Finalize.as_name(),
            self.context,
            self.peer,
            FlowCost::from(100u32),
            "consensus_result",
            None,
        )
    }

    /// Evaluate guard chain before sending ConsensusResult message
    pub async fn evaluate<E>(&self, effects: &E) -> AuraResult<SendGuardResult>
    where
        E: GuardEffects + GuardContextProvider + aura_core::PhysicalTimeEffects,
    {
        evaluate_guard_chain(self.create_guard_chain(), effects).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::ConsensusCapability;

    fn test_context() -> ContextId {
        ContextId::new_from_entropy([1u8; 32])
    }

    fn test_authority() -> AuthorityId {
        AuthorityId::new_from_entropy([2u8; 32])
    }

    fn assert_guard_annotations(
        chain: &SendGuardChain,
        capability: CapabilityName,
        cost: FlowCost,
    ) {
        assert_eq!(chain.authorization_requirement(), &capability);
        assert_eq!(chain.cost(), cost);
        assert_eq!(chain.context(), test_context());
        assert_eq!(chain.peer(), test_authority());
    }

    #[test]
    fn test_execute_guard_has_correct_annotations() {
        let guard = ExecuteGuard::new(test_context(), test_authority());
        let chain = guard.create_guard_chain();

        assert_guard_annotations(
            &chain,
            ConsensusCapability::Initiate.as_name(),
            FlowCost::from(100u32),
        );
    }

    #[test]
    fn test_nonce_commit_guard_has_correct_annotations() {
        let guard = NonceCommitGuard::new(test_context(), test_authority());
        let chain = guard.create_guard_chain();

        assert_guard_annotations(
            &chain,
            ConsensusCapability::WitnessNonce.as_name(),
            FlowCost::from(50u32),
        );
    }

    #[test]
    fn test_sign_request_guard_has_correct_annotations() {
        let guard = SignRequestGuard::new(test_context(), test_authority());
        let chain = guard.create_guard_chain();

        assert_guard_annotations(
            &chain,
            ConsensusCapability::AggregateNonces.as_name(),
            FlowCost::from(75u32),
        );
    }

    #[test]
    fn test_sign_share_guard_has_correct_annotations() {
        let guard = SignShareGuard::new(test_context(), test_authority());
        let chain = guard.create_guard_chain();

        assert_guard_annotations(
            &chain,
            ConsensusCapability::WitnessSign.as_name(),
            FlowCost::from(50u32),
        );
    }

    #[test]
    fn test_consensus_result_guard_has_correct_annotations() {
        let guard = ConsensusResultGuard::new(test_context(), test_authority());
        let chain = guard.create_guard_chain();

        assert_guard_annotations(
            &chain,
            ConsensusCapability::Finalize.as_name(),
            FlowCost::from(100u32),
        );
    }
}

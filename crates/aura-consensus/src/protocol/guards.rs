// Guard helpers for consensus protocol choreography annotations
//!
//! This module provides guard chain integration for the consensus protocol,
//! mapping choreography annotations to runtime guard enforcement.
//!
//! # Choreography Annotations
//!
//! From `choreography.choreo`:
//! - Execute: guard_capability="initiate_consensus", flow_cost=100
//! - NonceCommit: guard_capability="witness_nonce", flow_cost=50
//! - SignRequest: guard_capability="aggregate_nonces", flow_cost=75
//! - SignShare: guard_capability="witness_sign", flow_cost=50, leak="pipelined_commitment"
//! - ConsensusResult: guard_capability="finalize_consensus", flow_cost=100, journal_facts="consensus_complete"

use aura_core::{AuraResult, AuthorityId, ContextId, FlowCost};
use aura_guards::{
    CapabilityId, GuardContextProvider, GuardEffects, LeakageBudget, SendGuardChain,
    SendGuardResult,
};

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
    /// Annotations: guard_capability="initiate_consensus", flow_cost=100
    pub fn create_guard_chain(&self) -> SendGuardChain {
        SendGuardChain::new(
            CapabilityId::from("consensus:initiate"),
            self.context,
            self.peer,
            FlowCost::from(100u32),
        )
        .with_operation_id("consensus_execute")
    }

    /// Evaluate guard chain before sending Execute message
    pub async fn evaluate<E>(&self, effects: &E) -> AuraResult<SendGuardResult>
    where
        E: GuardEffects + GuardContextProvider + aura_core::PhysicalTimeEffects,
    {
        let chain = self.create_guard_chain();
        chain.evaluate(effects).await
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
    /// Annotations: guard_capability="witness_nonce", flow_cost=50
    pub fn create_guard_chain(&self) -> SendGuardChain {
        SendGuardChain::new(
            CapabilityId::from("consensus:witness_nonce"),
            self.context,
            self.peer,
            FlowCost::from(50u32),
        )
        .with_operation_id("consensus_nonce_commit")
    }

    /// Evaluate guard chain before sending NonceCommit message
    pub async fn evaluate<E>(&self, effects: &E) -> AuraResult<SendGuardResult>
    where
        E: GuardEffects + GuardContextProvider + aura_core::PhysicalTimeEffects,
    {
        let chain = self.create_guard_chain();
        chain.evaluate(effects).await
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
    /// Annotations: guard_capability="aggregate_nonces", flow_cost=75
    pub fn create_guard_chain(&self) -> SendGuardChain {
        SendGuardChain::new(
            CapabilityId::from("consensus:aggregate_nonces"),
            self.context,
            self.peer,
            FlowCost::from(75u32),
        )
        .with_operation_id("consensus_sign_request")
    }

    /// Evaluate guard chain before sending SignRequest message
    pub async fn evaluate<E>(&self, effects: &E) -> AuraResult<SendGuardResult>
    where
        E: GuardEffects + GuardContextProvider + aura_core::PhysicalTimeEffects,
    {
        let chain = self.create_guard_chain();
        chain.evaluate(effects).await
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
    /// Annotations: guard_capability="witness_sign", flow_cost=50, leak="pipelined_commitment"
    pub fn create_guard_chain(&self) -> SendGuardChain {
        // Leakage budget for pipelined commitment metadata
        // External: 0 bits (no external leakage)
        // Neighbor: 32 bits (commitment hash visible to coordinator)
        // In-group: 0 bits (group members share this info anyway)
        let leakage = LeakageBudget::new(0, 32, 0);

        SendGuardChain::new(
            CapabilityId::from("consensus:witness_sign"),
            self.context,
            self.peer,
            FlowCost::from(50u32),
        )
        .with_leakage_budget(leakage)
        .with_operation_id("consensus_sign_share")
    }

    /// Evaluate guard chain before sending SignShare message
    pub async fn evaluate<E>(&self, effects: &E) -> AuraResult<SendGuardResult>
    where
        E: GuardEffects + GuardContextProvider + aura_core::PhysicalTimeEffects,
    {
        let chain = self.create_guard_chain();
        chain.evaluate(effects).await
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
    /// Annotations: guard_capability="finalize_consensus", flow_cost=100, journal_facts="consensus_complete"
    pub fn create_guard_chain(&self) -> SendGuardChain {
        // TODO: Add journal coupler for "consensus_complete" fact
        // This would require the consensus completion fact to be passed in
        SendGuardChain::new(
            CapabilityId::from("consensus:finalize"),
            self.context,
            self.peer,
            FlowCost::from(100u32),
        )
        .with_operation_id("consensus_result")
    }

    /// Evaluate guard chain before sending ConsensusResult message
    pub async fn evaluate<E>(&self, effects: &E) -> AuraResult<SendGuardResult>
    where
        E: GuardEffects + GuardContextProvider + aura_core::PhysicalTimeEffects,
    {
        let chain = self.create_guard_chain();
        chain.evaluate(effects).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_context() -> ContextId {
        ContextId::new_from_entropy([1u8; 32])
    }

    fn test_authority() -> AuthorityId {
        AuthorityId::new_from_entropy([2u8; 32])
    }

    #[test]
    fn test_execute_guard_has_correct_annotations() {
        let guard = ExecuteGuard::new(test_context(), test_authority());
        let chain = guard.create_guard_chain();

        assert_eq!(chain.authorization_requirement(), "consensus:initiate");
        assert_eq!(chain.cost(), FlowCost::from(100u32));
        assert_eq!(chain.context(), test_context());
        assert_eq!(chain.peer(), test_authority());
    }

    #[test]
    fn test_nonce_commit_guard_has_correct_annotations() {
        let guard = NonceCommitGuard::new(test_context(), test_authority());
        let chain = guard.create_guard_chain();

        assert_eq!(chain.authorization_requirement(), "consensus:witness_nonce");
        assert_eq!(chain.cost(), FlowCost::from(50u32));
    }

    #[test]
    fn test_sign_request_guard_has_correct_annotations() {
        let guard = SignRequestGuard::new(test_context(), test_authority());
        let chain = guard.create_guard_chain();

        assert_eq!(
            chain.authorization_requirement(),
            "consensus:aggregate_nonces"
        );
        assert_eq!(chain.cost(), FlowCost::from(75u32));
    }

    #[test]
    fn test_sign_share_guard_has_correct_annotations() {
        let guard = SignShareGuard::new(test_context(), test_authority());
        let chain = guard.create_guard_chain();

        assert_eq!(chain.authorization_requirement(), "consensus:witness_sign");
        assert_eq!(chain.cost(), FlowCost::from(50u32));
    }

    #[test]
    fn test_consensus_result_guard_has_correct_annotations() {
        let guard = ConsensusResultGuard::new(test_context(), test_authority());
        let chain = guard.create_guard_chain();

        assert_eq!(chain.authorization_requirement(), "consensus:finalize");
        assert_eq!(chain.cost(), FlowCost::from(100u32));
    }
}

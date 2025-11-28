//! Layer 1: Flow Budget Effect Trait Definitions
//!
//! This module defines the pure effect trait interface for flow budget management.
//! Flow budgets are used to track privacy leakage and information flow costs
//! in distributed protocols.
//!
//! **Effect Classification**: Application Effect
//! - Implemented by domain crates (aura-journal provides CRDT-based implementation)
//! - Used by orchestration layer (aura-protocol) for guard chain integration
//! - Core trait definition belongs in Layer 1 (foundation)

use crate::{
    types::flow::Receipt,
    types::identifiers::{AuthorityId, ContextId},
    AuraResult,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Hint describing which flow bucket should be charged before a send.
///
/// This is a pure data structure that carries information about flow budget
/// charging requirements. It contains no orchestration logic.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlowHint {
    /// The context in which the flow is being charged
    pub context: ContextId,
    /// The peer authority that will receive the information
    pub peer: AuthorityId,
    /// The cost in flow budget units
    pub cost: u32,
}

impl FlowHint {
    /// Create a new flow hint
    pub fn new(context: ContextId, peer: AuthorityId, cost: u32) -> Self {
        Self {
            context,
            peer,
            cost,
        }
    }

    /// Get the context ID
    pub fn context(&self) -> &ContextId {
        &self.context
    }

    /// Get the peer authority ID
    pub fn peer(&self) -> &AuthorityId {
        &self.peer
    }

    /// Get the cost
    pub fn cost(&self) -> u32 {
        self.cost
    }
}

/// Effect trait for flow budget management operations.
///
/// This trait defines the interface for charging flow budgets in distributed
/// protocols. Flow budgets track information leakage and ensure privacy
/// constraints are respected.
///
/// **Implementation Note**: This trait is typically implemented by:
/// - Journal effects (CRDT-based flow budget tracking)
/// - Mock handlers (testing with configurable budgets)
///
/// The trait itself is pure and stateless - all state management is handled
/// by the implementing effect handlers.
#[async_trait]
pub trait FlowBudgetEffects: Send + Sync {
    /// Charge a flow budget for information sent to a peer.
    ///
    /// This operation should:
    /// 1. Check if sufficient budget exists for the context/peer combination
    /// 2. Deduct the cost from the available budget
    /// 3. Return a receipt proving the charge was successful
    /// 4. Fail if insufficient budget is available
    ///
    /// # Arguments
    /// * `context` - The context in which flow is being charged
    /// * `peer` - The authority receiving the information  
    /// * `cost` - The cost in flow budget units
    ///
    /// # Returns
    /// A `Receipt` proving the flow budget was successfully charged.
    ///
    /// # Errors
    /// Returns an error if:
    /// - Insufficient budget is available
    /// - The context is invalid
    /// - The peer authority is unknown
    /// - Internal storage/journal errors occur
    async fn charge_flow(
        &self,
        context: &ContextId,
        peer: &AuthorityId,
        cost: u32,
    ) -> AuraResult<Receipt>;
}

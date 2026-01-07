//! Pure guard implementations following ADR-014
//!
//! This module implements guards as pure functions that:
//! - Take immutable snapshots as input
//! - Return decisions and effect commands as output
//! - Perform no I/O operations
//! - Are fully deterministic and testable

use super::types::GuardOperationId;
use aura_core::{
    effects::{EffectCommand, GuardOutcome, GuardSnapshot, JournalEntry},
    identifiers::AuthorityId,
    Cap, Fact, FlowCost,
};
use std::fmt::Debug;

/// Request to be evaluated by guards
#[derive(Debug, Clone)]
pub struct GuardRequest {
    /// Authority making the request
    pub authority: AuthorityId,
    /// Context in which the operation occurs
    pub context: aura_core::ContextId,
    /// Peer the operation targets (if applicable)
    pub peer: AuthorityId,
    /// Operation being requested
    pub operation: GuardOperationId,
    /// Cost in flow budget units
    pub cost: FlowCost,
    /// Required capability
    pub capability: Cap,
    /// Whether this operation reveals metadata
    pub reveals_metadata: bool,
    /// Number of metadata bits revealed (if any)
    pub metadata_bits: u32,
    /// Additional context data
    pub context_data: Vec<u8>,
}

impl GuardRequest {
    /// Create a new guard request
    pub fn new(
        authority: AuthorityId,
        operation: impl Into<GuardOperationId>,
        cost: FlowCost,
    ) -> Self {
        Self {
            authority,
            context: aura_core::ContextId::new_from_entropy([2u8; 32]),
            peer: AuthorityId::new_from_entropy([1u8; 32]),
            operation: operation.into(),
            cost,
            capability: Cap::default(),
            reveals_metadata: false,
            metadata_bits: 0,
            context_data: Vec::new(),
        }
    }

    /// Set required capability
    pub fn with_capability(mut self, cap: Cap) -> Self {
        self.capability = cap;
        self
    }

    /// Mark as revealing metadata
    pub fn with_metadata_leakage(mut self, bits: u32) -> Self {
        self.reveals_metadata = true;
        self.metadata_bits = bits;
        self
    }

    /// Add context data
    pub fn with_context(mut self, context: Vec<u8>) -> Self {
        self.context_data = context;
        self
    }

    /// Set the context identifier
    pub fn with_context_id(mut self, context_id: aura_core::ContextId) -> Self {
        self.context = context_id;
        self
    }

    /// Set peer for the request
    pub fn with_peer(mut self, peer: AuthorityId) -> Self {
        self.peer = peer;
        self
    }
}

/// Pure guard trait
///
/// Guards are pure functions that evaluate requests against snapshots
/// and return outcomes containing decisions and effect commands.
pub trait Guard: Send + Sync + Debug {
    /// Evaluate a request against a snapshot
    ///
    /// This method must be pure:
    /// - No I/O operations
    /// - No side effects
    /// - Deterministic output for same inputs
    fn evaluate(&self, snapshot: &GuardSnapshot, request: &GuardRequest) -> GuardOutcome;

    /// Get guard name for debugging
    fn name(&self) -> &'static str;
}

/// Capability guard - checks authorization
#[derive(Debug)]
pub struct CapabilityGuard;

impl Guard for CapabilityGuard {
    fn evaluate(&self, snapshot: &GuardSnapshot, request: &GuardRequest) -> GuardOutcome {
        if !request.operation.is_empty() {
            let authz_key = format!("authz:{}", request.operation);
            match snapshot.metadata.get(&authz_key) {
                Some("allow") => {}
                Some("deny") => {
                    return GuardOutcome::denied("Authorization denied");
                }
                Some(_) | None => {
                    return GuardOutcome::denied("Missing authorization decision");
                }
            }
        }

        // Basic check: if a capability is required, ensure the snapshot carries one.
        let capability_ok = request.capability.is_empty()
            || (!snapshot.caps.is_empty() && snapshot.caps == request.capability);

        if !capability_ok {
            return GuardOutcome::denied("Capability check failed");
        }

        GuardOutcome::authorized(vec![])
    }

    fn name(&self) -> &'static str {
        "CapabilityGuard"
    }
}

/// Flow budget guard - enforces rate limiting
#[derive(Debug)]
pub struct FlowBudgetGuard;

impl Guard for FlowBudgetGuard {
    fn evaluate(&self, snapshot: &GuardSnapshot, request: &GuardRequest) -> GuardOutcome {
        // Check if the sender has sufficient budget for the target peer.
        if !snapshot
            .budgets
            .has_budget(&request.context, &request.peer, request.cost)
        {
            return GuardOutcome::denied("Insufficient flow budget");
        }

        // Return effect to charge budget
        GuardOutcome::authorized(vec![EffectCommand::ChargeBudget {
            context: request.context,
            authority: request.authority,
            peer: request.peer,
            amount: request.cost,
        }])
    }

    fn name(&self) -> &'static str {
        "FlowBudgetGuard"
    }
}

/// Journal coupling guard - ensures facts are recorded
#[derive(Debug)]
pub struct JournalCouplingGuard;

impl Guard for JournalCouplingGuard {
    fn evaluate(&self, snapshot: &GuardSnapshot, request: &GuardRequest) -> GuardOutcome {
        // Create journal entry for this operation
        let mut fact = Fact::new();
        let _ = fact.insert(
            "operation_executed",
            aura_core::journal::FactValue::String(format!(
                "{}:{}",
                request.authority, request.operation
            )),
        );

        let entry = JournalEntry {
            fact,
            authority: request.authority,
            timestamp: snapshot.now.clone(),
        };

        GuardOutcome::authorized(vec![EffectCommand::AppendJournal { entry }])
    }

    fn name(&self) -> &'static str {
        "JournalCouplingGuard"
    }
}

/// Leakage tracking guard - records metadata leakage
#[derive(Debug)]
pub struct LeakageTrackingGuard;

impl Guard for LeakageTrackingGuard {
    fn evaluate(&self, _snapshot: &GuardSnapshot, request: &GuardRequest) -> GuardOutcome {
        let mut effects = vec![];

        // Record leakage if operation reveals metadata
        if request.reveals_metadata {
            effects.push(EffectCommand::RecordLeakage {
                bits: request.metadata_bits,
            });
        }

        GuardOutcome::authorized(effects)
    }

    fn name(&self) -> &'static str {
        "LeakageTrackingGuard"
    }
}

/// Guard chain - composes multiple guards in sequence
#[derive(Debug)]
pub struct GuardChain {
    guards: Vec<Box<dyn Guard>>,
}

impl GuardChain {
    /// Create a new guard chain
    pub fn new() -> Self {
        Self { guards: Vec::new() }
    }

    /// Add a guard to the chain
    pub fn with(mut self, guard: Box<dyn Guard>) -> Self {
        self.guards.push(guard);
        self
    }

    /// Create standard guard chain (Cap → Budget → Journal → Leakage)
    pub fn standard() -> Self {
        Self::new()
            .with(Box::new(CapabilityGuard))
            .with(Box::new(FlowBudgetGuard))
            .with(Box::new(JournalCouplingGuard))
            .with(Box::new(LeakageTrackingGuard))
    }
}

impl Default for GuardChain {
    fn default() -> Self {
        Self::new()
    }
}

impl Guard for GuardChain {
    fn evaluate(&self, snapshot: &GuardSnapshot, request: &GuardRequest) -> GuardOutcome {
        let mut all_effects = Vec::new();

        // Evaluate each guard in sequence
        for guard in &self.guards {
            let outcome = guard.evaluate(snapshot, request);

            // If any guard denies, return immediately
            if !outcome.is_authorized() {
                tracing::debug!(
                    "Guard {} denied request: {:?}",
                    guard.name(),
                    outcome.decision.denial_reason()
                );
                return outcome;
            }

            // Accumulate effects from authorized guards
            all_effects.extend(outcome.effects);
        }

        // All guards passed - return accumulated effects
        GuardOutcome::authorized(all_effects)
    }

    fn name(&self) -> &'static str {
        "GuardChain"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{
        effects::{FlowBudgetView, MetadataView},
        time::{PhysicalTime, TimeStamp},
    };
    use std::collections::HashMap;

    fn test_authority() -> AuthorityId {
        AuthorityId::new_from_entropy([72u8; 32])
    }

    fn test_peer() -> AuthorityId {
        AuthorityId::new_from_entropy([1u8; 32])
    }

    fn test_context() -> aura_core::ContextId {
        aura_core::ContextId::new_from_entropy([73u8; 32])
    }

    fn test_snapshot() -> GuardSnapshot {
        let mut budgets = HashMap::new();
        budgets.insert((test_context(), test_peer()), FlowCost::new(1000));

        let mut metadata = HashMap::new();
        metadata.insert("authz:test_op".to_string(), "allow".to_string());

        GuardSnapshot {
            now: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            }),
            caps: Cap::default(),
            budgets: FlowBudgetView::new(budgets),
            metadata: MetadataView::new(metadata),
            rng_seed: [0u8; 32],
        }
    }

    #[test]
    fn test_capability_guard() {
        let guard = CapabilityGuard;
        let snapshot = test_snapshot();
        let request = GuardRequest::new(test_authority(), "test_op", FlowCost::new(100));

        let outcome = guard.evaluate(&snapshot, &request);
        assert!(outcome.is_authorized());
        assert!(outcome.effects.is_empty());
    }

    #[test]
    fn test_flow_budget_guard_success() {
        let guard = FlowBudgetGuard;
        let authority = test_authority();
        let context = test_context();
        let peer = test_peer();
        let mut snapshot = test_snapshot();

        // Set up budget
        let mut budgets = HashMap::new();
        budgets.insert((context, peer), FlowCost::new(1000));
        snapshot.budgets = FlowBudgetView::new(budgets);

        let request = GuardRequest::new(authority, "test_op", FlowCost::new(100))
            .with_context_id(context)
            .with_peer(peer);
        let outcome = guard.evaluate(&snapshot, &request);

        assert!(outcome.is_authorized());
        assert_eq!(outcome.effects.len(), 1);

        match &outcome.effects[0] {
            EffectCommand::ChargeBudget { amount, .. } => {
                assert_eq!(*amount, FlowCost::new(100));
            }
            _ => panic!("Expected ChargeBudget effect"),
        }
    }

    #[test]
    fn test_flow_budget_guard_insufficient() {
        let guard = FlowBudgetGuard;
        let snapshot = test_snapshot();
        let request = GuardRequest::new(test_authority(), "test_op", FlowCost::new(2000));

        let outcome = guard.evaluate(&snapshot, &request);
        assert!(!outcome.is_authorized());
        assert_eq!(
            outcome.decision.denial_reason(),
            Some("Insufficient flow budget")
        );
    }

    #[test]
    fn test_journal_coupling_guard() {
        let guard = JournalCouplingGuard;
        let snapshot = test_snapshot();
        let request = GuardRequest::new(test_authority(), "test_op", FlowCost::new(100));

        let outcome = guard.evaluate(&snapshot, &request);
        assert!(outcome.is_authorized());
        assert_eq!(outcome.effects.len(), 1);

        assert!(matches!(
            &outcome.effects[0],
            EffectCommand::AppendJournal { .. }
        ));
    }

    #[test]
    fn test_leakage_tracking_guard() {
        let guard = LeakageTrackingGuard;
        let snapshot = test_snapshot();

        // Request without metadata leakage
        let request1 = GuardRequest::new(test_authority(), "test_op", FlowCost::new(100));
        let outcome1 = guard.evaluate(&snapshot, &request1);
        assert!(outcome1.is_authorized());
        assert!(outcome1.effects.is_empty());

        // Request with metadata leakage
        let request2 = GuardRequest::new(test_authority(), "test_op", FlowCost::new(100))
            .with_metadata_leakage(32);
        let outcome2 = guard.evaluate(&snapshot, &request2);
        assert!(outcome2.is_authorized());
        assert_eq!(outcome2.effects.len(), 1);

        match &outcome2.effects[0] {
            EffectCommand::RecordLeakage { bits } => assert_eq!(*bits, 32),
            _ => panic!("Expected RecordLeakage effect"),
        }
    }

    #[test]
    fn test_guard_chain() {
        let chain = GuardChain::standard();
        let mut snapshot = test_snapshot();
        let authority = test_authority();
        let context = test_context();
        let peer = test_peer();

        // Set up budget
        let mut budgets = HashMap::new();
        budgets.insert((context, peer), FlowCost::new(1000));
        snapshot.budgets = FlowBudgetView::new(budgets);

        let request = GuardRequest::new(authority, "test_op", FlowCost::new(100))
            .with_context_id(context)
            .with_peer(peer)
            .with_metadata_leakage(16);

        let outcome = chain.evaluate(&snapshot, &request);
        assert!(outcome.is_authorized());

        // Should have effects from multiple guards
        assert!(outcome.effects.len() >= 3); // Budget, Journal, Leakage

        // Verify effect types
        let has_budget = outcome
            .effects
            .iter()
            .any(|e| matches!(e, EffectCommand::ChargeBudget { .. }));
        let has_journal = outcome
            .effects
            .iter()
            .any(|e| matches!(e, EffectCommand::AppendJournal { .. }));
        let has_leakage = outcome
            .effects
            .iter()
            .any(|e| matches!(e, EffectCommand::RecordLeakage { .. }));

        assert!(has_budget);
        assert!(has_journal);
        assert!(has_leakage);
    }

    #[test]
    fn test_guard_chain_early_denial() {
        let chain = GuardChain::standard();
        let snapshot = test_snapshot(); // No budget for unknown authority
        let request = GuardRequest::new(test_authority(), "test_op", FlowCost::new(100));

        let outcome = chain.evaluate(&snapshot, &request);
        assert!(!outcome.is_authorized());
        assert_eq!(
            outcome.decision.denial_reason(),
            Some("Insufficient flow budget")
        );
        assert!(outcome.effects.is_empty()); // No effects on denial
    }
}

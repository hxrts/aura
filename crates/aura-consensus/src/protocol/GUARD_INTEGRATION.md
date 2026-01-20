# Guard Chain Integration for Consensus Protocol

## Overview

This document describes the guard chain integration for the aura-consensus protocol, mapping choreography annotations to runtime guard enforcement.

## Current State (2026-01-18)

### ✅ Completed

1. **Guard Helper Infrastructure** (`protocol/guards.rs`):
   - Created typed guard helpers for all 5 message types
   - Each helper maps choreography annotations to `SendGuardChain` configuration
   - Flow costs match `choreography.choreo` annotations exactly
   - Leakage budgets implemented for privacy-sensitive operations

2. **Message Coverage**:
   - ✅ Execute (Coordinator → Witness): `guard_capability="initiate_consensus"`, `flow_cost=100`
   - ✅ NonceCommit (Witness → Coordinator): `guard_capability="witness_nonce"`, `flow_cost=50`
   - ✅ SignRequest (Coordinator → Witness): `guard_capability="aggregate_nonces"`, `flow_cost=75`
   - ✅ SignShare (Witness → Coordinator): `guard_capability="witness_sign"`, `flow_cost=50`, `leak="pipelined_commitment"`
   - ✅ ConsensusResult (Coordinator → Witness): `guard_capability="finalize_consensus"`, `flow_cost=100`, `journal_facts="consensus_complete"`

3. **Testing**:
   - ✅ Unit tests validate guard configurations match annotations
   - ✅ All 5 guard types have passing tests
   - ✅ Flow cost conversion tests pass

### ⏳ Remaining Work

1. **Send Site Integration**:
   - Update `coordinator.rs` broadcast sites to call guard helpers before sending
   - Update `witness.rs` send sites to call guard helpers before responding
   - Handle guard denial (log and return error)
   - Attach receipts to messages for relay verification

2. **Journal Coupling**:
   - ConsensusResult guard needs `JournalCoupler` for "consensus_complete" fact
   - Requires consensus completion fact to be defined and attached

3. **Integration Testing**:
   - End-to-end test showing guard evaluation before send
   - Test guard denial path (insufficient budget, missing capability)
   - Test leakage tracking for SignShare
   - Test journal fact commitment for ConsensusResult

4. **Effect System Wiring**:
   - Guards require access to effect system (FlowBudgetEffects, AuthorizationEffects, etc.)
   - Current `ConsensusProtocol` doesn't have direct effect system access
   - May need to thread effects through or use effect context pattern

## Usage Example (When Integrated)

```rust
// In coordinator.rs, before broadcasting Execute message:
use crate::protocol::guards::ExecuteGuard;

pub async fn broadcast_execute<E>(
    &mut self,
    cid: ConsensusId,
    effects: &E,
) -> Result<()>
where
    E: GuardEffects + GuardContextProvider + PhysicalTimeEffects,
{
    let msg = ConsensusMessage::Execute { /* ... */ };

    // Evaluate guard chain before send
    for witness in &self.config.witness_set {
        let guard = ExecuteGuard::new(self.context_id, *witness);
        let guard_result = guard.evaluate(effects).await?;

        if !guard_result.authorized {
            warn!(
                witness = ?witness,
                reason = ?guard_result.denial_reason,
                "Guard denied Execute send"
            );
            continue; // Skip this witness
        }

        // Send with receipt
        self.transport.send_with_receipt(witness, msg.clone(), guard_result.receipt).await?;
    }

    Ok(())
}
```

## Choreography Annotations Reference

From `choreography.choreo`:

```purescript
@parallel
Coordinator[guard_capability = "initiate_consensus", flow_cost = 100]
  -> Witness[*] : Execute(crate::ConsensusMessage)

@parallel
Witness[*][guard_capability = "witness_nonce", flow_cost = 50]
  -> Coordinator : NonceCommit(crate::ConsensusMessage)

@parallel
Coordinator[guard_capability = "aggregate_nonces", flow_cost = 75]
  -> Witness[*] : SignRequest(crate::ConsensusMessage)

@parallel
Witness[*][guard_capability = "witness_sign", flow_cost = 50, leak = "pipelined_commitment"]
  -> Coordinator : SignShare(crate::ConsensusMessage)

@parallel
Coordinator[guard_capability = "finalize_consensus", flow_cost = 100, journal_facts = "consensus_complete"]
  -> Witness[*] : ConsensusResult(crate::ConsensusMessage)
```

## Guard Chain Sequence

Per `docs/001_system_architecture.md`:

1. **CapGuard**: Verify `need(m) ≤ Auth(ctx)` using Biscuit tokens
2. **FlowGuard**: Check `headroom(ctx, cost)` and atomically charge budget
3. **JournalCoupler**: Commit delta facts atomically with send
4. **LeakageTracker**: Validate privacy budget per observer class
5. **Transport**: Actual message send with receipt

## Invariants Enforced

- **Charge-Before-Send**: No transport operation without prior budget charge
- **Authorization-Gated**: All sends require appropriate capability
- **Atomic-Journal-Coupling**: Facts committed atomically with send
- **Privacy-Bounded**: Leakage stays within budget limits

## Architecture Compliance

- ✅ Layer 4 (Orchestration): Guards belong here
- ✅ No direct runtime coupling: Uses effect traits, not tokio directly
- ✅ Pure guard chain: `SendGuardChain` is pure, execution via interpreter
- ✅ Annotation-driven: Guards derived from choreography annotations

## Next Steps

1. Thread effect system through `ConsensusProtocol` struct
2. Update send sites in coordinator.rs and witness.rs
3. Add integration tests for guard enforcement
4. Implement journal coupler for consensus completion fact
5. Consider macro-generated guard helpers (future enhancement)

## References

- `docs/001_system_architecture.md` - Guard chain architecture
- `docs/003_information_flow_contract.md` - Privacy and flow budgets
- `crates/aura-guards/src/guards/chain.rs` - SendGuardChain implementation
- `crates/aura-consensus/src/protocol/choreography.choreo` - Source annotations

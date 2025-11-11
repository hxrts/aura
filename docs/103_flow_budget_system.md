# Information Flow Budget

The information-flow budget system simultaneously enforces privacy leakage limits and prevents spam using semilattice primitives. The goal is to express "who may talk, how often, and with how much metadata" using the same primitives as the rest of Aura ensuring the feature is simple, monotone, and deterministic. For the theoretical foundation of this privacy model, see [Privacy and Information Flow Model](004_information_flow_model.md).

Budgets are per-context and per-peer without automatic cover traffic. Handlers block when a budget is exhausted. Later versions can layer padding and cover traffic on top of the same ledger without changing the core math.

## Data Model

Each context-peer pair stores a flow budget fact in the journal:

```rust
struct FlowBudget {
    limit: u64,    // meet-semilattice: merge = min
    spent: u64,    // join-semilattice: merge = max
    epoch: Epoch,  // ties fact to active key epoch
}
```

The `spent` field counts cumulative cost of emitted messages. The `limit` field is the maximum cost before throttling. Budgets live in the same CRDT namespace as `JournalCaps` inheriting monotonicity. Joins increase `spent` while meets decrease `limit`.

Entries scope to contexts so replicas outside that context or lacking relevant capability never see counters, preventing leakage through the ledger itself. For details on capability-based access control, see [Capability System](202_capability_system.md). Every send primitive carries `flow_cost` metadata derived from the choreography. Defaults include `DirectSend` at 1 unit, `RelayForward` at 1 unit, `LargePayload` proportional to size, and `QueryRequest` and `Reply` at 2 units each.

## Charging and Receipts

Before calling `TransportEffects::send`, a choreography charges the budget. The charging algorithm compares the sum of current spend plus requested cost against the limit. If the charge fails the choreography emits an error and no network observable is produced.

Relays require proof that the upstream hop already charged its budget. Each `Receipt` contains context identifier, source and destination device identifiers, epoch number, flow cost charged, nonce for uniqueness, hash of previous receipt for anti-replay chaining, and signature using the context key. Receipts provide cryptographic proof that a sender legitimately charged their budget allowing relays to verify upstream compliance without trusting the sender.

The protocol proceeds as follows. The sender charges its context-receiver budget and emits the payload plus receipt. The relay validates the signature and checks remaining budget. The relay charges its own context-next hop budget before forwarding. If any step fails the message is dropped locally. Receipts never leave the context as they are encrypted with relationship keys preventing budget value leakage.

## Implementation

The `FlowBudget` struct in `aura-core/src/flow.rs` provides the concrete realization of the budget data model. The implementation uses hybrid semilattice semantics where the limit field follows meet-semilattice ordering and the spent field follows join-semilattice ordering. This asymmetry ensures concurrent updates only tighten limits or remember higher expenditures.

The `new()` constructor creates a budget with specified limit and epoch initializing spent to zero. The `headroom()` method computes available budget as the saturating difference between limit and spent returning zero when exhausted. The `can_charge()` method performs a non-mutating check whether a given cost fits. The `record_charge()` method atomically updates spent and returns error if the charge exceeds the limit.

The `merge()` method combines two budgets using meet for limit and join for spent preserving the newer epoch. The `rotate_epoch()` method advances the epoch counter and resets spent to zero implementing the periodic replenishment cycle.

`FlowBudgetKey` provides journal storage addressing for budget facts by combining a context identifier with peer device identifier. This ensures budgets remain scoped to specific relationships and prevents cross-context leakage. Only devices with appropriate capabilities can observe or modify budget facts for a given context.

The implementation integrates with the journal CRDT through semilattice interfaces. `FlowBudget` implements `JoinSemilattice` delegating to hybrid merge semantics and implements `CvState` to participate in causal consistency protocols. This allows budget facts to propagate through anti-entropy mechanisms as other journal data ensuring eventual consistency while maintaining monotonicity guarantees.

## Replenishment

Budgets replenish when epochs rotate or when explicit `BudgetUpdate` facts are merged. Updates are deterministic functions of journal data so every replica derives the same limit.

Input signals come from the journal. The `w` parameter is Web-of-Trust edge weight between 0 and 1. The `recip` parameter is the minimum of outbound and inbound messages. The `abuse` parameter is the count of abuse flags for the peer and context. The `tier` parameter is a manual override multiplier.

All inputs are facts already present in the journal. The deterministic formula computes base limit as the product of baseline multiplied by tier. Trust boost multiplies the base by the edge weight. Reciprocity boost caps the minimum of reciprocal messages over the window. Penalty subtracts abuse count multiplied by penalty unit per message. The new limit is the maximum of minimum limit and the sum of trust boost plus reciprocity boost minus penalty.

Constants like `BASE_LIMIT` and `RECIP_UNIT` live in the capability policy configuration so all devices share them. During epoch rotation each participant computes the new limit and gossips `BudgetProposal` facts. The merge rule takes the minimum proposed limit to remain conservative. The `spent` field resets to zero with the new epoch fact. Because limits only move via facts, forks cannot accidentally grant extra budget.

## Enforcement Pipeline

Every choreography annotation adds both leakage and flow cost to send nodes. Generated role code calls `charge()` using the current `AuraContext`. Failure routes to an error branch for retry or user notification. Once charge succeeds the actual network packet is sent with the receipt attached for multi-hop sends. `FlowBudget` updates enter the journal as CRDT ops so all devices converge.

High-level analytics can read bucketed per-epoch spent values while raw counters remain scoped to their contexts. Error surface area is intentionally small with only the handler performing the send able to fail and the failure being deterministic.

The `FlowGuard` interface runs before any transport effect. The guard enforces both flow budgets and leakage annotations. This pattern separates budget validation from actual transport execution ensuring consistent enforcement across all choreographies.

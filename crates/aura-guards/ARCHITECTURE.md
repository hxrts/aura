# Aura Guards (Layer 4)

## Purpose

Provide the guard chain that enforces authorization, flow budgets, leakage budgets, and journal coupling for every network-visible send. Guards are pure evaluators that return EffectCommands; no guard performs I/O directly.

## Scope

| Belongs here | Does not belong here |
|--------------|----------------------|
| Pure guard evaluation and chain ordering | Direct transport or storage calls inside guards |
| GuardSnapshot preparation and decision types | Effect execution (happens via interpreters) |
| EffectCommands describing required side effects | Time/random access (only via effect traits in interpreter layer) |
| GuardDecision with optional receipt metadata | Runtime composition or lifecycle management |
| Verified remote-ingress typestate boundary | Decoded peer messages mutating state directly |

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| Down | `aura-core` | Effect trait definitions, domain types |
| In | GuardSnapshot | Prepared from effect system state |
| In | GuardChain configuration | Capability requirement, flow cost, leakage policy, journal facts |
| In | EffectContext metadata | Authority/context/session |
| Out | EffectCommands | Budget charge, journal commit, etc. |
| Out | GuardDecision | With optional receipt metadata |

## Key Modules

- `guards/pure.rs`: Pure guard chain ordering and evaluation.
- `guards/chain.rs`: Guard chain composition.
- `guards/types.rs`: Guard types and decision structures.
- `guards/policy.rs`: Policy defaults and configuration.
- `guards/biscuit_evaluator.rs`: Biscuit-based authorization evaluation.
- `guards/capability_guard.rs`: Capability requirement checks.
- `guards/flow.rs`: Flow budget guard.
- `guards/journal.rs`: Journal coupling guard.
- `guards/executor.rs`: Effectful execution of guard decisions.

## Invariants

- Charge-before-send: no transport side effects without successful guard evaluation.
- Peer-originated data must carry verified ingress evidence before it can be
  accepted by state-mutating APIs.
- Authorization precedes budgeting: CapGuard runs before FlowGuard.
- Journal coupling is atomic with budget charge.
- Leakage accounting is recorded as journal facts (RelationalFact::LeakageEvent).
- Guards are pure and deterministic given the snapshot.
- Guard operation ids are validated typed values; empty or whitespace-only custom
  operations must be rejected before guard evaluation.
- Flow budget lookup errors or missing budget state fail closed. Zero limits do
  not imply unlimited headroom.

### InvariantSentMessagesHaveFacts

No observable network behavior may occur before capability validation, flow budget charging, and journal coupling succeed.

Enforcement locus:
- `src/guards/pure.rs`: `GuardChain` defines pure decision order.
- `src/guards/executor.rs`: `GuardChainExecutor` applies effect commands before transport send.
- `src/guards/capability_guard.rs`: authorization checks run before budget logic.
- `src/guards/flow.rs` and `src/guards/journal.rs`: flow charge and journal coupling are linked.

Failure mode:
- Unauthorized packets reach the network.
- Budget exhaustion fails open.
- Journal and transport diverge on send accounting.

Verification hooks:
- `just check-arch`
- `cargo test -p aura-protocol guard_chain_invariant`
- `cargo test -p aura-simulator invariant_tests`

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) defines guard-mediated observability.
- [Privacy and Information Flow Contract](../../docs/003_information_flow_contract.md) defines charge-before-send and flow budget invariants.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines `InvariantSentMessagesHaveFacts` and `InvariantFlowBudgetNonNegative`.

## Ownership Model

> Taxonomy: [Ownership Model](../../docs/122_ownership_model.md)

`aura-guards` is primarily `Pure` guard evaluation. Effectful execution around guards remains outside the pure decision core and does not turn guards into `ActorOwned` semantic owners. Guard execution outcomes participate in typed terminal failure rather than hidden fail-open or silent blocking behavior.

See [System Internals Guide](../../docs/807_system_internals_guide.md) §Core + Orchestrator Rule.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| `guards/pure.rs`, `guards/chain.rs`, `guards/types.rs`, `guards/policy.rs` | `Pure` | Canonical guard ordering, policy, and typed guard results. |
| `ingress.rs` verified remote-ingress boundary | `MoveOwned`, capability-gated | Sealed typestate wrapper for peer-originated data admitted by protocol/runtime verifiers. |
| `guards/biscuit_evaluator.rs`, `guards/capability_guard.rs`, `guards/flow.rs`, `guards/journal.rs` | `Pure`, `MoveOwned` | Guard inputs/results remain explicit values; no hidden ownership transfer or fail-open mutation. |
| `guards/executor.rs` | effectful orchestrator | Applies effect commands without becoming a long-lived semantic owner. |
| Actor-owned runtime state | none | Guard chain ownership stays outside this crate. |
| Observed-only surfaces | none | Observation of decisions/receipts belongs downstream. |

### Capability-Gated Points

- Guard capability requirements and Biscuit-based authorization input.
- Typed guard outcomes consumed by higher-layer send/journal/flow execution.

## Testing

### Strategy

Guard chain ordering and the charge-before-send invariant are the primary testing concerns. Integration tests in `tests/chain/` verify end-to-end chain behavior; inline tests verify individual guard correctness.

### Commands

```
cargo test -p aura-guards
just check-arch
```

### Coverage matrix

| What breaks if wrong | Test location | Status |
|---------------------|--------------|--------|
| Guard chain effect ordering wrong | `src/guards/pure.rs` `test_guard_chain_effect_ordering` | Covered |
| Empty operation id bypasses authorization | `tests/compile_fail.rs` and guard inline tests | Covered |
| Budget lookup failure authorizes traffic | `tests/chain/guard_chain_transport.rs` | Covered |
| CapGuard denial leaks downstream effects | `src/guards/pure.rs` `test_guard_chain_capguard_denial_stops_chain` | Covered |
| FlowGuard denial leaks downstream effects | `src/guards/pure.rs` `test_guard_chain_early_denial` | Covered |
| Transport without guard evaluation | `tests/chain/guard_chain_transport.rs` | Covered |
| CapGuard accepts missing capability | `src/guards/capability_guard.rs` (inline) | Covered |
| FlowGuard accepts exhausted budget | `src/guards/pure.rs` (inline) | Covered |
| Charge-before-send property | `tests/chain/guard_chain_properties.rs` (proptest) | Covered |
| Choreography guard integration | `tests/chain/choreography_guards.rs` | Covered |
| GuardPlan drifts from choreography | `tests/chain/guard_plan_golden.rs` | Covered |
| Policy defaults incorrect | `src/guards/policy.rs` (inline) | Covered |
| Decoded peer data is treated as verified ingress | `src/ingress.rs` (inline) | Initial typestate coverage |

## References

- [Theoretical Model](../../docs/002_theoretical_model.md)
- [Privacy and Information Flow Contract](../../docs/003_information_flow_contract.md)
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md)
- [Ownership Model](../../docs/122_ownership_model.md)
- [System Internals Guide](../../docs/807_system_internals_guide.md)

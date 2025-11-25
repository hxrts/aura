# Aura System Invariants

This document serves as an index to invariant documentation colocated with enforcement code throughout the Aura codebase. Each invariant is documented where it is enforced, ensuring documentation stays synchronized with implementation.

## What Are System Invariants?

System invariants are properties that must always hold true for Aura to maintain its security, consistency, and correctness guarantees. Violating an invariant indicates a critical bug that could compromise the entire system.

## Core System Invariants

### 1. **Charge-Before-Send**
**Location**: [`crates/aura-protocol/src/guards/INVARIANTS.md`](../crates/aura-protocol/src/guards/INVARIANTS.md)

No observable network behavior may occur without successful authorization and flow budget charging. All messages must pass through the guard chain before reaching the network.

**Key Properties**:
- Authorization via capabilities must succeed
- Flow budget must have sufficient headroom
- Journal facts must be atomically coupled
- No bypass paths to `TransportEffects::send()`

### 2. **CRDT Convergence** 
**Location**: [`crates/aura-journal/src/reduce/INVARIANTS.md`](../crates/aura-journal/src/reduce/INVARIANTS.md)

Identical sets of facts must always produce identical reduced state, ensuring eventual consistency across all nodes.

**Key Properties**:
- Deterministic reduction functions
- Commutative and associative fact operations
- No external dependencies during reduction
- Pure functions only (no side effects)

### 3. **Context Isolation**
**Location**: [`crates/aura-core/src/relational/INVARIANTS.md`](../crates/aura-core/src/relational/INVARIANTS.md)

Information must not flow across relational context boundaries without explicit authorization. Each context maintains isolated state.

**Key Properties**:
- Separate journal namespaces per context
- Facts cannot cross context boundaries
- Channels bound to single context
- No cross-context state reduction

### 4. **Secure Channel Lifecycle**
**Location**: [`crates/aura-transport/src/channel/INVARIANTS.md`](../crates/aura-transport/src/channel/INVARIANTS.md)

Secure channels are strictly bound to epochs and follow a defined state machine. Messages on stale channels must be rejected.

**Key Properties**:
- Channels bound to specific epoch
- State transitions follow FSM rules
- Epoch rotation forces channel closure
- No messages accepted from wrong epoch

## Derived Invariants

These invariants follow from the core invariants:

- **Budget Monotonicity**: Flow budget spent counters only increase
- **Fact Immutability**: Facts never change after creation  
- **Epoch Monotonicity**: Epoch numbers only increase
- **Snapshot Determinism**: Snapshots taken at same facts produce same state

## Validation and Testing

### Automated Validation

Run architectural validation:
```bash
just arch-check
```

This validates:
- Invariant documentation follows required schema
- No guard chain bypasses exist
- Effect placement is correct
- Impure functions are properly isolated

### Testing Invariants

Each invariant has associated tests:

```bash
# Test charge-before-send
cargo test -p aura-protocol guard_chain_invariant

# Test CRDT convergence  
cargo test -p aura-journal convergence

# Test context isolation
cargo test -p aura-core context_isolation

# Test channel lifecycle
cargo test -p aura-transport channel_lifecycle
```

### Simulator Scenarios

The simulator includes specific scenarios to stress-test invariants:

```bash
# Run all invariant scenarios
cargo test -p aura-simulator invariant_tests
```

## Adding New Invariants

When adding a new invariant:

1. Create `INVARIANTS.md` in the module that enforces it
2. Follow the schema:
   - Invariant name
   - Enforcement locus (module/function)
   - Failure mode (observable consequences)
   - Detection method (test/sim/arch-check)
3. Add tests that verify the invariant
4. Update this index with a link

## Invariant Violations

If you discover an invariant violation:

1. **STOP** - Do not deploy code with known violations
2. File a critical bug with:
   - Which invariant was violated
   - Steps to reproduce
   - Potential security impact
3. Add regression test before fixing
4. Document the fix in the invariant file

## Related Documentation

- [System Architecture](001_system_architecture.md) - Architectural context for invariants
- [Distributed Systems Contract](004_distributed_systems_contract.md) - Distributed system guarantees
- [Information Flow Contract](003_information_flow_contract.md) - Privacy and flow invariants
- [Effect System and Runtime](106_effect_system_and_runtime.md) - Effect system design
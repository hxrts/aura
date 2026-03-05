# Aura Formal Verification

Formal verification artifacts using two complementary systems:

| System | Purpose | Strength |
|--------|---------|----------|
| **Lean 4** | Mathematical theorem proofs | Correctness guarantees via formal proof |
| **Quint** | State machine model checking | Exhaustive state exploration via Apalache |

## Quick Start

```bash
nix develop              # Enter development environment
just verify-lean         # Build Lean proofs
just verify-quint        # Check Quint specs
just verify-all          # Run all verification
```

## Directory Structure

```
verification/
├── lean/                # Lean 4 theorem proofs
│   ├── Aura/
│   │   ├── Assumptions.lean    # Cryptographic axioms
│   │   ├── Types.lean          # Core type definitions
│   │   ├── Domain/             # Domain types (no proofs)
│   │   └── Proofs/             # All proofs centralized
│   └── lakefile.lean
└── quint/               # Quint state machine specs
    ├── consensus/       # Consensus protocol specs
    ├── journal/         # Journal and CRDT specs
    ├── keys/            # Key management specs
    ├── sessions/        # Session and group specs
    ├── liveness/        # Liveness analysis
    ├── amp/             # AMP channel specs
    ├── harness/         # Simulator harness modules
    ├── tui/             # TUI state machine specs
    └── *.qnt            # Core protocol specs
```

## Lean 4 Proofs

Mathematical proofs of safety properties. Key areas:

- **Consensus**: Agreement, validity, equivocation detection, FROST integration
- **Journal CRDT**: Commutativity, associativity, idempotence
- **Context Isolation**: No cross-context merge, namespace isolation
- **Flow Budget**: Monotonic decrease, exact charge
- **Time System**: Reflexivity, transitivity, privacy preservation

### Cryptographic Axioms

Documented in `Aura/Assumptions.lean`:

| Axiom | Purpose |
|-------|---------|
| `frost_threshold_unforgeability` | FROST k-of-n security |
| `frost_uniqueness` | Same shares produce same signature |
| `hash_collision_resistance` | Prestate binding |
| `byzantine_threshold` | k > f (threshold > Byzantine) |

## Quint Specifications

Executable state machine specifications based on TLA. Run with:

```bash
quint typecheck <spec>.qnt    # Check syntax and types
quint run <spec>.qnt          # Generate random traces
quint verify <spec>.qnt       # Model checking (requires Apalache)
```

### Core Specifications

| Spec | Description |
|------|-------------|
| `consensus/core.qnt` | Fast-path/fallback consensus |
| `consensus/frost.qnt` | FROST threshold signatures |
| `journal/core.qnt` | CRDT journal operations |
| `transport.qnt` | Transport layer, guard chain |
| `recovery.qnt` | Guardian-based recovery |
| `invitation.qnt` | Invitation lifecycle |

### Verified Invariants

All core specs verified with Apalache. Examples:

- `journal/core.qnt`: Nonce uniqueness, Lamport monotonicity, deterministic reduce
- `consensus/core.qnt`: Unique commit per instance, threshold requirement, path convergence
- `transport.qnt`: Context isolation, flow budget non-negative, sequence monotonic

## Verified Properties Summary

### Safety
- Guard chain order (CapGuard → FlowGuard → JournalCoupler → TransportSend)
- Budget invariants (`spent ≤ limit`)
- Unique commits per consensus instance
- Session isolation

### Liveness
- Protocol completion with honest threshold
- Timeout handling within TTL
- Eventual convergence via anti-entropy

### Security
- M-of-N threshold signatures for critical operations
- Epoch-bounded receipt validity
- Cross-protocol safety (Recovery∥Consensus never deadlocks)

## Integration

### Simulator Integration

The `aura-simulator` crate provides generative testing:

```
Quint Spec → JSON IR → ActionRegistry → Effect Handlers → Property Evaluation
```

### Differential Testing

```bash
just lean-oracle-build    # Build Lean oracle
just test-differential    # Rust vs Lean tests
```

### Telltale Lean Bridge

`aura-quint` now depends on and re-exports upstream `telltale-lean-bridge` to align Aura's bridge workflows with Telltale's Lean integration surface.

Use:

```bash
just ci-lean-quint-bridge
just ci-simulator-telltale-parity
```

## Resources

- [Verification Guide](../docs/806_verification_guide.md) — Detailed workflows and Quint-Lean correspondence
- [Simulation Guide](../docs/805_simulation_guide.md) — Trace replay and conformance
- [Quint Documentation](https://quint-lang.org/docs)
- [System Architecture](../docs/001_system_architecture.md)

# Aura Formal Verification

This directory contains formal verification artifacts for the Aura protocol.

## Structure

```
verification/
├── README.md           # This file
├── CORRESPONDENCE.md   # Quint-Lean correspondence map
├── lean/               # Lean 4 theorem proofs
│   ├── STYLE.md        # Lean coding conventions
│   ├── lakefile.lean   # Build configuration
│   └── Aura/           # Proof modules
└── quint/              # Quint state machine specs
    ├── STYLE.md        # Quint coding conventions
    ├── README.md       # Detailed Quint documentation
    └── *.qnt           # Protocol specifications
```

## Verification Approach

Aura uses two complementary verification systems:

| System | Purpose | Strength |
|--------|---------|----------|
| **Lean 4** | Mathematical theorem proofs | Correctness guarantees |
| **Quint** | State machine model checking | Exhaustive state exploration |

### Lean 4 Proofs

Mathematical proofs of safety properties:
- CRDT semilattice properties (commutativity, associativity, idempotence)
- Threshold signature correctness
- Consensus agreement and validity
- Equivocation detection soundness

### Quint Models

Executable state machine specifications:
- Protocol state transitions
- Invariant checking via model checking
- Liveness and termination properties
- Byzantine fault tolerance

## Quick Start

```bash
# Enter development environment
nix develop

# Build Lean proofs
cd verification/lean && lake build

# Check Quint specs
cd verification/quint && quint typecheck protocol_consensus.qnt

# Run model checking
quint run --invariant=InvariantUniqueCommitPerInstance protocol_consensus.qnt
```

## Consensus Verification

The consensus protocol has comprehensive verification coverage:

| Property | Quint Invariant | Lean Theorem |
|----------|-----------------|--------------|
| Unique commit | `InvariantUniqueCommitPerInstance` | `Agreement.agreement` |
| Threshold requirement | `InvariantCommitRequiresThreshold` | `Validity.commit_has_threshold` |
| Equivocation exclusion | `InvariantEquivocatorsExcluded` | `Equivocation.exclusion_correctness` |
| Signature binding | `InvariantSignatureBindsToCommitFact` | `Frost.share_binding` |

See [CORRESPONDENCE.md](./CORRESPONDENCE.md) for the complete mapping.

## Key Files

### Lean Modules

| Module | Purpose |
|--------|---------|
| `Aura/Core/Assumptions.lean` | Cryptographic axioms |
| `Aura/Consensus/Types.lean` | Domain types |
| `Aura/Consensus/Agreement.lean` | Agreement proofs |
| `Aura/Consensus/Validity.lean` | Validity proofs |
| `Aura/Consensus/Evidence.lean` | CRDT proofs |
| `Aura/Consensus/Equivocation.lean` | Detection proofs |
| `Aura/Consensus/Frost.lean` | FROST integration |
| `Aura/Consensus/Proofs.lean` | Claims bundle summary |

### Quint Specifications

| Specification | Purpose |
|---------------|---------|
| `protocol_consensus.qnt` | Core consensus model |
| `protocol_consensus_adversary.qnt` | Byzantine behavior |
| `protocol_consensus_liveness.qnt` | Termination properties |

## Documentation

- [Lean Style Guide](./lean/STYLE.md)
- [Quint Style Guide](./quint/STYLE.md)
- [Quint README](./quint/README.md)
- [Correspondence Map](./CORRESPONDENCE.md)
- [Verification Guide](../docs/807_verification_guide.md)

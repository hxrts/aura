# Consensus FROST Architecture

This document explains the architectural relationship between the consensus FROST implementation and the `ThresholdSigningService`.

## Module Structure

```
consensus/
├── core/                    # Pure effect-free state machine for verification
│   ├── state.rs             # ConsensusState, WitnessParticipation, PathSelection
│   ├── transitions.rs       # Pure state transitions (start, apply_share, trigger_fallback)
│   ├── validation.rs        # Invariant checking, equivocation detection
│   └── verification/        # Verification infrastructure (not compiled in production)
│       ├── quint_mapping.rs # Quint ITF trace correspondence (simulation feature)
│       └── kani_proofs.rs   # Bounded model checking proofs
├── frost.rs        # FrostConsensusOrchestrator - crypto integration with pipelining
├── protocol.rs     # ConsensusProtocol - main coordination and execution
├── messages.rs     # Protocol messages and choreography definitions
├── witness.rs      # WitnessSet, WitnessTracker, WitnessState
├── types.rs        # ConsensusId, CommitFact, ConsensusConfig
└── relational.rs   # Cross-authority consensus adapter
```

## Why Consensus Has Its Own FROST Implementation

The `FrostConsensusOrchestrator` in this module maintains a separate FROST implementation from `ThresholdSigningService` (in `aura-agent`). This is intentional.

### Different Purposes

| Component | Purpose | Scope |
|-----------|---------|-------|
| `ThresholdSigningService` | "How does my authority sign?" | Single authority, device-to-device |
| `FrostConsensusOrchestrator` | "How do witnesses agree on state?" | Multi-witness, network consensus |

### Consensus-Specific Features

The consensus orchestrator has requirements that don't apply to general signing:

1. **Pipelining Optimization (1 RTT)**
   ```rust
   // Cache next-round nonces while signing current round
   let (next_commitment, next_token) = self.generate_nonce(share, random).await?;
   witness_state.set_next_nonce(next_commitment, next_token, self.config.epoch);
   ```
   This enables 1 RTT consensus by pre-generating nonces for round N+1 during round N.

2. **Witness State Tracking**
   - Manages concurrent consensus instances (`HashMap<ConsensusId, ConsensusInstance>`)
   - Tracks per-witness nonce commitments and partial signatures
   - Coordinates threshold quorum across network

3. **Epoch-Aware Cache Invalidation**
   - Cached nonce commitments are tied to epochs
   - Epoch changes invalidate all caches (security requirement)

4. **Output Difference**
   - `ThresholdSigningService` outputs: `ThresholdSignature`
   - `FrostConsensusOrchestrator` outputs: `CommitFact` (signature + prestate hash + participants + proof)

### Security Boundary

- **Signing Service**: Manages *your* keys (devices in your authority)
- **Consensus Orchestrator**: Coordinates *witnesses* (potentially different authorities, adversarial model)

## Pure Core State Machine

The `core/` module contains a pure, effect-free state machine that:

1. **Maps to Quint specifications** (`verification/quint/protocol_consensus.qnt`)
2. **Corresponds to Lean proofs** (`verification/lean/Aura/Consensus/`)
3. **Enables bounded model checking** via Kani proofs

This separation allows formal verification of consensus correctness independent of I/O effects.

## What IS Shared

Both implementations share:

1. **Primitives Layer** (`aura-core::crypto::tree_signing`)
   - `frost_aggregate`, `frost_verify_aggregate`, `NonceToken`, etc.
   - Single auditable surface for FROST cryptography

2. **Type Definitions** (`aura-core::frost`)
   - `Share`, `PublicKeyPackage`, `ThresholdSignature`, `NonceCommitment`, `PartialSignature`

3. **Key Material Source**
   - Both fetch keys from `SecureStorageEffects` (Keychain/TPM/Keystore)
   - Prevents key material duplication/divergence

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────┐
│          aura-core::crypto::tree_signing                    │
│    frost_aggregate, frost_verify_aggregate, NonceToken      │
│                    (primitives layer)                       │
└────────────────────────────┬────────────────────────────────┘
                             │
         ┌───────────────────┼───────────────────┐
         │                   │                   │
         ▼                   ▼                   ▼
┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
│   Threshold     │  │   Consensus     │  │  SecureStorage  │
│   Signing       │  │   Orchestrator  │  │    Effects      │
│   Service       │  │                 │  │   (key store)   │
│                 │  │  ┌───────────┐  │  │                 │
│   aura-agent    │  │  │ core/ SM  │  │  │   aura-core     │
│                 │  │  │ (verif.)  │  │  │                 │
│                 │  │  └───────────┘  │  │                 │
│                 │  │   aura-proto    │  │                 │
└─────────────────┘  └─────────────────┘  └─────────────────┘
```

## When to Use Which

- **Use `ThresholdSigningService`** for:
  - Signing tree operations (personal authority)
  - Guardian recovery approvals
  - Group decisions
  - Any "sign this message" operation

- **Use `FrostConsensusOrchestrator`** for:
  - Network consensus on state transitions
  - Multi-witness agreement
  - Operations requiring `CommitFact` with proof

## References

- `docs/104_consensus.md` - Consensus protocol design
- `docs/116_crypto.md` - Cryptography architecture
- `verification/quint/protocol_consensus.qnt` - Quint specification
- `verification/lean/Aura/Consensus/` - Lean proofs

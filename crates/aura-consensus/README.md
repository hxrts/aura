# aura-consensus

Layer 4 strong-agreement consensus implementation for Aura. Provides the sole mechanism for distributed agreement using FROST threshold signatures.

## Module Structure

```
src/
├── core/                    # Pure effect-free state machine for verification
│   ├── state.rs             # ConsensusState, WitnessParticipation, PathSelection
│   ├── transitions.rs       # Pure state transitions (start, apply_share, trigger_fallback)
│   ├── validation.rs        # Invariant checking, equivocation detection
│   └── verification/        # Verification infrastructure
│       ├── quint_mapping.rs # Quint ITF trace correspondence
│       └── kani_proofs.rs   # Bounded model checking proofs
├── dkg/           # Quorum-driven DKG orchestration + transcript helpers
├── frost.rs        # FrostConsensusOrchestrator - crypto integration with pipelining
├── protocol.rs     # ConsensusProtocol - main coordination and execution
├── messages.rs     # Protocol messages and choreography definitions
├── witness.rs      # WitnessSet, WitnessTracker, WitnessState
├── types.rs        # ConsensusId, CommitFact, ConsensusConfig
├── relational.rs   # Cross-authority consensus adapter
└── lib.rs          # Public API and re-exports
```

## Why Consensus Has Its Own FROST Implementation

The `FrostConsensusOrchestrator` maintains a separate FROST implementation from `ThresholdSigningService` (in `aura-agent`). This is intentional:

| Component | Purpose | Scope |
|-----------|---------|-------|
| `ThresholdSigningService` | "How does my authority sign?" | Single authority, device-to-device |
| `FrostConsensusOrchestrator` | "How do witnesses agree on state?" | Multi-witness, network consensus |

### Consensus-Specific Features

1. **Pipelining Optimization (1 RTT)** - Pre-generates nonces for round N+1 during round N
2. **Witness State Tracking** - Manages concurrent consensus instances and per-witness nonce commitments
3. **Epoch-Aware Cache Invalidation** - Epoch changes invalidate all caches (security requirement)
4. **Output Difference** - Outputs `CommitFact` (signature + prestate hash + participants + proof) rather than raw signatures

### Security Boundary

- **Signing Service**: Manages *your* keys (devices in your authority)
- **Consensus Orchestrator**: Coordinates *witnesses* (potentially different authorities, adversarial model)

## Shared Components

Both implementations share:

1. **Primitives** (`aura-core::crypto::tree_signing`) - `frost_aggregate`, `frost_verify_aggregate`, `NonceToken`
2. **Type Definitions** (`aura-core::frost`) - `Share`, `PublicKeyPackage`, `ThresholdSignature`
3. **Key Material Source** - Both fetch keys from `SecureStorageEffects` (Keychain/TPM/Keystore)

## DKG Transcript Storage

The `dkg` module provides a storage abstraction for finalized transcripts:

- `MemoryTranscriptStore` for tests/in-memory usage
- `StorageTranscriptStore` for production adapters (backed by `StorageEffects`)

Transcripts are serialized using canonical DAG-CBOR and stored under a deterministic hash key.

## Architecture

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
│                 │  │ aura-consensus  │  │                 │
└─────────────────┘  └─────────────────┘  └─────────────────┘
```

## When to Use Which

- **Use `ThresholdSigningService`** for: Signing tree operations, guardian recovery approvals, group decisions
- **Use `FrostConsensusOrchestrator`** for: Network consensus, multi-witness agreement, operations requiring `CommitFact`

## Tests

```bash
cargo test -p aura-consensus                                    # Unit + property tests
cargo test -p aura-consensus --test consensus_itf_conformance   # ITF conformance
```

Reference-model and Lean correspondence tests are in `tests/`.

## References

- `docs/104_consensus.md` - Consensus protocol design
- `docs/116_crypto.md` - Cryptography architecture
- `verification/quint/protocol_consensus.qnt` - Quint specification
- `verification/lean/Aura/Consensus/` - Lean proofs

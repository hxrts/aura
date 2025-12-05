# Consensus FROST Architecture

This document explains the architectural relationship between the consensus FROST implementation and the `ThresholdSigningService`.

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

## What IS Shared

Both implementations share:

1. **Primitives Layer** (`CryptoEffects` trait in `aura-core`)
   - `frost_generate_keys`, `frost_sign_share`, `frost_aggregate_signatures`, etc.
   - Single auditable surface for FROST cryptography

2. **Type Definitions** (`aura-core::frost`)
   - `Share`, `PublicKeyPackage`, `ThresholdSignature`, `NonceCommitment`, `PartialSignature`

3. **Key Material Source** (TODO: `KeyMaterialService`)
   - Both should fetch keys from the same underlying storage
   - Prevents key material duplication/divergence

## Architecture Diagram

```
┌─────────────────────────────────────────┐
│         CryptoEffects trait             │
│   frost_generate_keys, frost_sign, etc  │
│       (aura-core - primitives)          │
└────────────────────┬────────────────────┘
                     │
     ┌───────────────┼───────────────┐
     │               │               │
     ▼               ▼               ▼
┌────────────┐  ┌────────────┐  ┌────────────┐
│ Threshold  │  │ Consensus  │  │ KeyMaterial│
│ Signing    │  │ FROST      │  │ Service    │
│ Service    │  │ Orchestr.  │  │ (shared)   │
│            │  │            │  │            │
│ aura-agent │  │ aura-proto │  │ aura-core  │
└────────────┘  └────────────┘  └────────────┘
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
- `work/multi_device_frost.md` - Implementation work plan

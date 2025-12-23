# Aura Quint Specifications

Formal specifications of the Aura protocol using Quint 0.25.x, an executable specification language based on the Temporal Logic of Actions (TLA).

## Related Documentation

- **[STYLE.md](./STYLE.md)** - Quint coding conventions for this project
- **[../CORRESPONDENCE.md](../CORRESPONDENCE.md)** - Mapping between Quint invariants and Lean theorems
- **[../lean/STYLE.md](../lean/STYLE.md)** - Lean 4 coding conventions

## Getting Started

Enter the Nix development environment:

```bash
nix develop
```

Verify the Quint setup:

```bash
just verify-quint
```

Common Quint commands:

```bash
# Check syntax and types
quint typecheck <spec>.qnt

# Run the REPL
quint repl <spec>.qnt

# Generate random traces
quint run <spec>.qnt

# Verify properties with model checking (requires Apalache)
quint verify <spec>.qnt
```

## Specification Structure

### Protocol Specifications (18 specs)

Core protocol state machines modeling Aura's distributed protocols:

| Specification | Description | Documentation |
|---------------|-------------|---------------|
| `protocol_core.qnt` | Shared runtime utilities, protocol lifecycle, effects, timers | [System Architecture](../../docs/001_system_architecture.md) |
| `protocol_dkg.qnt` | FROST Distributed Key Generation ceremony | [Crypto Guide](../../docs/116_crypto.md) |
| `protocol_dkd.qnt` | Deterministic Key Derivation for context keys | [Crypto Guide](../../docs/116_crypto.md) |
| `protocol_resharing.qnt` | Threshold key resharing protocol | [Crypto Guide](../../docs/116_crypto.md) |
| `protocol_recovery.qnt` | Guardian-based recovery flows | [Relational Contexts](../../docs/103_relational_contexts.md) |
| `protocol_locking.qnt` | Distributed locking protocol | - |
| `protocol_counter.qnt` | Lamport clock counter coordination | - |
| `protocol_groups.qnt` | Group membership management | [Social Architecture](../../docs/114_social_architecture.md) |
| `protocol_sessions.qnt` | Session lifecycle and presence | [MPST Guide](../../docs/107_mpst_and_choreography.md) |
| `protocol_sbb.qnt` | Social Bulletin Board gossip | [Rendezvous](../../docs/110_rendezvous.md) |
| `protocol_journal.qnt` | CRDT journal operations | [Journal Guide](../../docs/102_journal.md) |
| `protocol_signals.qnt` | Protocol signaling utilities | - |
| `protocol_consensus.qnt` | Fast-path/fallback consensus with threshold signatures | [Consensus](../../docs/104_consensus.md), [Lean Proofs](../lean/Aura/Consensus/) |
| `protocol_consensus_adversary.qnt` | Byzantine adversary models for consensus | [Distributed Contract](../../docs/004_distributed_systems_contract.md) |
| `protocol_consensus_liveness.qnt` | Liveness and termination properties | [Distributed Contract](../../docs/004_distributed_systems_contract.md) |
| `protocol_cross_interaction.qnt` | Recovery∥Consensus concurrent execution safety | [Distributed Contract](../../docs/004_distributed_systems_contract.md) |
| `protocol_anti_entropy.qnt` | CRDT delta sync and eventual convergence | [Maintenance](../../docs/111_maintenance.md) |
| `protocol_epochs.qnt` | Epoch transitions and receipt validity windows | [Transport](../../docs/108_transport_and_information_flow.md) |
| `protocol_frost.qnt` | FROST threshold signature protocol model | [Crypto Guide](../../docs/116_crypto.md) |
| `protocol_capability_properties.qnt` | Guard chain authorization, budget, and integrity verification | [Information Flow](../../docs/003_information_flow_contract.md) |

### Harness Modules (6 specs)

Standard entry points for simulator integration:

| Harness | Protocol | Entry Points |
|---------|----------|--------------|
| `harness_dkg.qnt` | DKG | `register`, `submitCommitment`, `complete`, `abort` |
| `harness_resharing.qnt` | Resharing | `register`, `approve`, `moveToDistribution`, `complete`, `abort` |
| `harness_recovery.qnt` | Recovery | `register`, `submitShare`, `complete`, `abort` |
| `harness_locking.qnt` | Locking | `register`, `requestLock`, `complete`, `abort` |
| `harness_counter.qnt` | Counter | `register`, `increment`, `complete`, `abort` |
| `harness_groups.qnt` | Groups | `register`, `addMember`, `removeMember`, `complete`, `abort` |

### Test Specifications

Located in `crates/aura-simulator/tests/quint_specs/`:

| Specification | Purpose |
|---------------|---------|
| `dkd_minimal.qnt` | Minimal DKD protocol test |

## Design Principles

### Authority Model

All specifications use the authority model with opaque identifiers:

```quint
type AuthorityId = str   // Opaque authority identifier
type ContextId = str     // Relational context identifier
type ProtocolId = str    // Protocol instance identifier
```

Specifications avoid exposing device-level details. Use `AuthorityId` instead of `DeviceId`.

### Protocol Lifecycle

Protocols follow a standard lifecycle with typestate transitions:

```
Initialized → Active → AwaitingEvidence → Completed
                   ↘                    ↗
                     → Failed/Cancelled
```

### Effect System

Protocol actions emit effects that the simulator executes:

```quint
type ProtocolEffect =
    | EffectSend           // P2P message
    | EffectBroadcast      // Multi-party broadcast
    | EffectAppendJournal  // Fact commitment
    | EffectScheduleTimer  // Timer scheduling
    | EffectCancelTimer    // Timer cancellation
    | EffectTrace          // Debug tracing
```

## Verification Status

All core protocol specifications have been verified with Apalache model checking:

| Specification | Verified Invariants |
|---------------|---------------------|
| `protocol_journal.qnt` | `InvariantNonceUnique`, `InvariantEventsOrdered`, `InvariantLamportMonotonic`, `InvariantReduceDeterministic` |
| `protocol_consensus.qnt` | `InvariantUniqueCommitPerInstance`, `InvariantCommitRequiresThreshold`, `InvariantPathConvergence` |
| `protocol_anti_entropy.qnt` | `InvariantFactsMonotonic`, `InvariantVectorClockConsistent`, `InvariantEventualConvergence` |
| `protocol_recovery.qnt` | `InvariantThresholdWithinBounds`, `InvariantApprovalsSubsetGuardians`, `InvariantPhaseConsistency` |
| `protocol_sessions.qnt` | `InvariantAuthoritiesRegisteredSessions`, `InvariantRevokedInactive` |
| `protocol_cross_interaction.qnt` | `InvariantNoDeadlock`, `InvariantRevokedDevicesExcluded` |
| `protocol_epochs.qnt` | `InvariantReceiptValidityWindow`, `InvariantCrossEpochReplayPrevention` |
| `protocol_dkg.qnt` | `InvariantThresholdBounds`, `InvariantPhaseCommitmentCounts`, `InvariantSharesOnlyAfterVerification` |
| `protocol_capability_properties.qnt` | `guardChainOrder`, `chargeBeforeSend`, `spentWithinLimit`, `attenuationOnlyNarrows` |
| `protocol_frost.qnt` | `thresholdInvariant`, `commitmentBeforeSigning`, `sharesFromCommitted`, `validSignatureInvariant` |

## Verified Properties

### Safety

- **Guard Chain Order**: Operations follow CapGuard → FlowGuard → JournalCoupler → TransportSend
- **Budget Invariants**: `spent ≤ limit` for all flow budgets
- **Capability Attenuation**: Capabilities only narrow, never widen
- **Unique Commits**: At most one CommitFact per consensus instance
- **Session Isolation**: Compromised sessions don't affect others

### Liveness

- **Protocol Completion**: Honest threshold can complete any protocol
- **Timeout Handling**: Sessions terminate within TTL
- **Eventual Convergence**: Anti-entropy ensures all nodes converge

### Security

- **Threshold Security**: M-of-N signatures required for critical operations
- **Counter Uniqueness**: No duplicate counter values
- **Epoch Validity**: Receipts only valid within their epoch window
- **Cross-Protocol Safety**: Recovery∥Consensus never deadlocks

## Integration with Simulator

The `aura-simulator` crate provides generative testing integration:

```
Quint Spec (.qnt)
      │
      ▼ quint parse
   JSON IR
      │
      ▼ ActionRegistry
   Aura Effect Handlers
      │
      ▼ StateMapper
   Property Evaluation
```

### Generated ITF Traces

Model-based testing traces are generated in `traces/`:

| Trace File | Source Spec | Description |
|------------|-------------|-------------|
| `cap_props.itf.json` | `protocol_capability_properties.qnt` | Guard chain and budget verification |
| `frost.itf.json` | `protocol_frost.qnt` | FROST threshold signature protocol |
| `dkg.itf.json` | `protocol_dkg.qnt` | DKG ceremony execution |
| `consensus.itf.json` | `protocol_consensus.qnt` | Fast-path/fallback consensus |
| `cross_interaction.itf.json` | `protocol_cross_interaction.qnt` | Concurrent protocol safety |
| `anti_entropy.itf.json` | `protocol_anti_entropy.qnt` | CRDT synchronization |
| `epochs.itf.json` | `protocol_epochs.qnt` | Epoch transitions and receipts |

Generate traces with:

```bash
quint run verification/quint/protocol_consensus.qnt \
  --main=protocol_consensus \
  --max-samples=5 --max-steps=20 \
  --out-itf=traces/consensus.itf.json
```

## Resources

- [Quint Documentation](https://quint-lang.org/docs)
- [Verification Guide](../../docs/807_verification_guide.md)
- [Simulation Guide](../../docs/806_simulation_guide.md)
- [Generative Testing Guide](../../docs/809_generative_testing_guide.md)
- [System Architecture](../../docs/001_system_architecture.md)

# Aura Quint Specifications

Formal specifications of the Aura protocol using Quint 0.25.x, an executable specification language based on the Temporal Logic of Actions (TLA).

## Related Documentation

- **[../README.md](../README.md)** - Verification overview and Quint-Lean correspondence map
- **[../lean/README.md](../lean/README.md)** - Lean 4 module documentation

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

### Directory Layout

```
verification/quint/
├── core.qnt                    # Shared runtime utilities, lifecycle, effects
├── recovery.qnt                # Guardian-based recovery flows
├── authorization.qnt           # Guard chain authorization properties
├── epochs.qnt                  # Epoch transitions and receipt windows
├── transport.qnt               # Transport layer, sessions, guard chain
├── sbb.qnt                     # Social Bulletin Board gossip
├── interaction.qnt             # Recovery∥Consensus concurrent safety
├── consensus/                  # Consensus protocol specs
│   ├── core.qnt                # Fast-path/fallback consensus
│   ├── frost.qnt               # FROST threshold signatures
│   ├── adversary.qnt           # Byzantine adversary models
│   └── liveness.qnt            # Liveness and termination
├── journal/                    # Journal and CRDT specs
│   ├── core.qnt                # CRDT journal operations
│   ├── anti_entropy.qnt        # Delta sync and convergence
│   └── counter.qnt             # Lamport clock coordination
├── keys/                       # Key management specs
│   ├── dkg.qnt                 # Distributed Key Generation
│   ├── dkd.qnt                 # Deterministic Key Derivation
│   └── resharing.qnt           # Threshold key resharing
├── sessions/                   # Session and group specs
│   ├── core.qnt                # Session lifecycle
│   ├── groups.qnt              # Group membership management
│   └── locking.qnt             # Distributed locking
├── amp/                        # AMP channel lifecycle specs
│   └── channel.qnt             # Channel invites, membership, messaging, epoch bump
├── liveness/                   # Liveness analysis specs
│   ├── timing.qnt              # Synchrony model and timing
│   ├── connectivity.qnt        # Gossip graph connectivity
│   └── properties.qnt          # Liveness properties
├── harness/                    # Simulator harness modules
│   ├── dkg.qnt, resharing.qnt, recovery.qnt
│   ├── locking.qnt, counter.qnt, groups.qnt
│   ├── flows.qnt               # TUI flow harness
│   └── amp_channel.qnt         # AMP channel lifecycle harness
└── tui/                        # TUI state machine specs
    ├── flows.qnt               # TUI flow specifications
    └── cli_recovery_demo.qnt   # CLI recovery demo
```

### Protocol Specifications

Core protocol state machines modeling Aura's distributed protocols:

| Directory | Specification | Description | Documentation |
|-----------|---------------|-------------|---------------|
| `.` | `core.qnt` | Shared runtime utilities, protocol lifecycle, effects, timers | [System Architecture](../../docs/001_system_architecture.md) |
| `consensus/` | `core.qnt` | Fast-path/fallback consensus with threshold signatures | [Consensus](../../docs/104_consensus.md) |
| `consensus/` | `frost.qnt` | FROST threshold signature protocol model | [Crypto Guide](../../docs/116_crypto.md) |
| `consensus/` | `adversary.qnt` | Byzantine adversary models for consensus | [Distributed Contract](../../docs/004_distributed_systems_contract.md) |
| `consensus/` | `liveness.qnt` | Liveness and termination properties | [Distributed Contract](../../docs/004_distributed_systems_contract.md) |
| `journal/` | `core.qnt` | CRDT journal operations | [Journal Guide](../../docs/102_journal.md) |
| `journal/` | `anti_entropy.qnt` | CRDT delta sync and eventual convergence | [Maintenance](../../docs/111_maintenance.md) |
| `journal/` | `counter.qnt` | Lamport clock counter coordination | - |
| `keys/` | `dkg.qnt` | FROST Distributed Key Generation ceremony | [Crypto Guide](../../docs/116_crypto.md) |
| `keys/` | `dkd.qnt` | Deterministic Key Derivation for context keys | [Crypto Guide](../../docs/116_crypto.md) |
| `keys/` | `resharing.qnt` | Threshold key resharing protocol | [Crypto Guide](../../docs/116_crypto.md) |
| `sessions/` | `core.qnt` | Session lifecycle and presence | [MPST Guide](../../docs/107_mpst_and_choreography.md) |
| `sessions/` | `groups.qnt` | Group membership management | [Social Architecture](../../docs/114_social_architecture.md) |
| `sessions/` | `locking.qnt` | Distributed locking protocol | - |
| `amp/` | `channel.qnt` | AMP channel lifecycle (invite/join/send/leave/rotate) | [AMP](../../docs/112_amp.md) |
| `.` | `recovery.qnt` | Guardian-based recovery flows | [Relational Contexts](../../docs/103_relational_contexts.md) |
| `.` | `authorization.qnt` | Guard chain authorization, budget verification | [Information Flow](../../docs/003_information_flow_contract.md) |
| `.` | `epochs.qnt` | Epoch transitions and receipt validity windows | [Transport](../../docs/108_transport_and_information_flow.md) |
| `.` | `transport.qnt` | Transport layer: connections, sessions, guard chain, message ordering | [Transport](../../docs/108_transport_and_information_flow.md) |
| `.` | `invitation.qnt` | Invitation lifecycle, ceremonies, authorization invariants | [Relational Contexts](../../docs/103_relational_contexts.md) |
| `.` | `sbb.qnt` | Social Bulletin Board gossip | [Rendezvous](../../docs/110_rendezvous.md) |
| `.` | `interaction.qnt` | Recovery∥Consensus concurrent execution safety | [Distributed Contract](../../docs/004_distributed_systems_contract.md) |

### Harness Modules

Standard entry points for simulator integration (in `harness/`):

| Harness | Protocol | Entry Points |
|---------|----------|--------------|
| `dkg.qnt` | DKG | `register`, `submitCommitment`, `complete`, `abort` |
| `resharing.qnt` | Resharing | `register`, `approve`, `moveToDistribution`, `complete`, `abort` |
| `recovery.qnt` | Recovery | `register`, `submitShare`, `complete`, `abort` |
| `locking.qnt` | Locking | `register`, `requestLock`, `complete`, `abort` |
| `counter.qnt` | Counter | `register`, `increment`, `complete`, `abort` |
| `groups.qnt` | Groups | `register`, `addMember`, `removeMember`, `complete`, `abort` |
| `amp_channel.qnt` | AMP Channel | `ampChannelLifecycle` |

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
| `journal/core.qnt` | `InvariantNonceUnique`, `InvariantEventsOrdered`, `InvariantLamportMonotonic`, `InvariantReduceDeterministic` |
| `consensus/core.qnt` | `InvariantUniqueCommitPerInstance`, `InvariantCommitRequiresThreshold`, `InvariantPathConvergence` |
| `journal/anti_entropy.qnt` | `InvariantFactsMonotonic`, `InvariantVectorClockConsistent`, `InvariantEventualConvergence` |
| `recovery.qnt` | `InvariantThresholdWithinBounds`, `InvariantApprovalsSubsetGuardians`, `InvariantPhaseConsistency` |
| `sessions/core.qnt` | `InvariantAuthoritiesRegisteredSessions`, `InvariantRevokedInactive` |
| `interaction.qnt` | `InvariantNoDeadlock`, `InvariantRevokedDevicesExcluded` |
| `epochs.qnt` | `InvariantReceiptValidityWindow`, `InvariantCrossEpochReplayPrevention` |
| `keys/dkg.qnt` | `InvariantThresholdBounds`, `InvariantPhaseCommitmentCounts`, `InvariantSharesOnlyAfterVerification` |
| `authorization.qnt` | `guardChainOrder`, `chargeBeforeSend`, `spentWithinLimit`, `attenuationOnlyNarrows` |
| `transport.qnt` | `InvariantContextIsolation`, `InvariantFlowBudgetNonNegative`, `InvariantSequenceMonotonic`, `InvariantSentMessagesHaveFacts` |
| `invitation.qnt` | `InvariantOnlySenderCancels`, `InvariantOnlyReceiverAcceptsOrDeclines`, `InvariantNoDoubleResolution`, `InvariantTerminalStatusImmutable`, `InvariantAcceptedHasFact`, `InvariantCeremonyForAcceptedOnly` |
| `consensus/frost.qnt` | `thresholdInvariant`, `commitmentBeforeSigning`, `sharesFromCommitted`, `validSignatureInvariant` |

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
| `cap_props.itf.json` | `authorization.qnt` | Guard chain and budget verification |
| `frost.itf.json` | `consensus/frost.qnt` | FROST threshold signature protocol |
| `dkg.itf.json` | `keys/dkg.qnt` | DKG ceremony execution |
| `consensus.itf.json` | `consensus/core.qnt` | Fast-path/fallback consensus |
| `cross_interaction.itf.json` | `interaction.qnt` | Concurrent protocol safety |
| `anti_entropy.itf.json` | `journal/anti_entropy.qnt` | CRDT synchronization |
| `epochs.itf.json` | `epochs.qnt` | Epoch transitions and receipts |
| `amp_channel.itf.json` | `harness/amp_channel.qnt` | AMP channel lifecycle (create/invite/join/send/leave/rotate) |

Generate traces with:

```bash
quint run verification/quint/consensus/core.qnt \
  --main=protocol_consensus \
  --max-samples=5 --max-steps=20 \
  --out-itf=traces/consensus.itf.json
```

AMP channel lifecycle traces require MBT metadata so the simulator can replay action names:

```bash
quint run verification/quint/harness/amp_channel.qnt \
  --main=harness_amp_channel \
  --max-samples=1 --max-steps=20 \
  --mbt \
  --out-itf=traces/amp_channel.itf.json
```

## Rust Integration

### Bounded Liveness Checking

The `aura-simulator` crate provides bounded liveness checking that integrates with Quint liveness specs:

```rust
use aura_simulator::liveness::{
    BoundedLivenessChecker, BoundedLivenessProperty, SynchronyAssumption
};

// Check that consensus terminates within 20 steps under partial synchrony
let mut checker = BoundedLivenessChecker::with_synchrony(
    SynchronyAssumption::PartialSynchrony { gst: 5, delta: 3 }
);
checker.add_property(BoundedLivenessProperty {
    name: "consensus_terminates".to_string(),
    precondition: "gstReached".to_string(),
    goal: "allInstancesTerminated(instances)".to_string(),
    step_bound: 20,
    ..Default::default()
});
```

See `crates/aura-simulator/src/liveness/mod.rs` for the full API.

### Type Drift Detection

Check correspondence between Quint types and Rust `QuintMappable` implementations:

```bash
just quint-check-types          # Summary
just quint-check-types --verbose # Detailed report
```

This detects when Quint type definitions drift from their Rust counterparts.

### Verification Coverage

Generate a verification coverage report:

```bash
just verification-coverage      # Markdown
just verification-coverage --json # JSON metrics
```

See `docs/998_verification_coverage.md` for the current coverage status.

## Resources

- [Quint Documentation](https://quint-lang.org/docs)
- [Verification Guide](../../docs/807_verification_guide.md)
- [Simulation Guide](../../docs/806_simulation_guide.md)
- [Generative Testing Guide](../../docs/809_generative_testing_guide.md)
- [System Architecture](../../docs/001_system_architecture.md)
- [Verification Coverage](../../docs/998_verification_coverage.md)

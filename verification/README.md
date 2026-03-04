# Aura Formal Verification

Formal verification artifacts for the Aura protocol using two complementary systems:

| System | Purpose | Strength |
|--------|---------|----------|
| **Lean 4** | Mathematical theorem proofs | Correctness guarantees via formal proof |
| **Quint** | State machine model checking | Exhaustive state exploration via Apalache |

## Quick Start

```bash
# Enter development environment
nix develop

# Build Lean proofs
just verify-lean

# Check Quint specs
just verify-quint

# Run all verification
just verify-all
```

## Directory Structure

```
verification/
├── README.md                     # This file
├── lean/                         # Lean 4 theorem proofs
│   ├── lakefile.lean             # Build configuration
│   └── Aura/                     # Proof modules
│       ├── Assumptions.lean      # Cryptographic axioms
│       ├── Types.lean            # Core type definitions
│       ├── Types/                # Shared type helpers
│       │   ├── AttestedOp.lean
│       │   ├── ByteArray32.lean
│       │   ├── FactContent.lean
│       │   ├── Identifiers.lean
│       │   ├── Namespace.lean
│       │   ├── OrderTime.lean
│       │   ├── ProtocolFacts.lean
│       │   ├── TimeStamp.lean
│       │   └── TreeOp.lean
│       ├── Domain/               # Domain types and operations (no proofs)
│       │   ├── Consensus/        # Consensus data structures
│       │   │   ├── Types.lean
│       │   │   └── Frost.lean
│       │   ├── Journal/          # Journal types and operations
│       │   │   ├── Types.lean
│       │   │   └── Operations.lean
│       │   ├── ContextIsolation.lean
│       │   ├── FlowBudget.lean
│       │   ├── GuardChain.lean
│       │   ├── KeyDerivation.lean
│       │   └── TimeSystem.lean
│       ├── Proofs/               # All proofs centralized
│       │   ├── Consensus/        # Consensus proofs
│       │   │   ├── Agreement.lean
│       │   │   ├── Validity.lean
│       │   │   ├── Evidence.lean
│       │   │   ├── Equivocation.lean
│       │   │   ├── Frost.lean
│       │   │   ├── Liveness.lean
│       │   │   ├── Adversary.lean
│       │   │   └── Summary.lean
│       │   ├── ContextIsolation.lean
│       │   ├── FlowBudget.lean
│       │   ├── GuardChain.lean
│       │   ├── Journal.lean
│       │   ├── KeyDerivation.lean
│       │   └── TimeSystem.lean
│       ├── Proofs.lean           # Top-level entry point for reviewers
│       └── Runner.lean           # CLI for differential testing
└── quint/                        # Quint state machine specs
    ├── core.qnt                  # Shared runtime utilities, lifecycle, effects
    ├── recovery.qnt              # Guardian-based recovery flows
    ├── authorization.qnt         # Guard chain authorization properties
    ├── epochs.qnt                # Epoch transitions and receipt windows
    ├── transport.qnt             # Transport layer, sessions, guard chain
    ├── sbb.qnt                   # Social Bulletin Board gossip
    ├── interaction.qnt           # Recovery∥Consensus concurrent safety
    ├── invitation.qnt            # Invitation lifecycle and ceremonies
    ├── leakage.qnt               # Information leakage tracking
    ├── time_system.qnt           # Time system properties
    ├── cli_recovery_demo.qnt     # CLI recovery demo
    ├── consensus/                # Consensus protocol specs
    │   ├── core.qnt              # Fast-path/fallback consensus
    │   ├── frost.qnt             # FROST threshold signatures
    │   ├── adversary.qnt         # Byzantine adversary models
    │   └── liveness.qnt          # Liveness and termination
    ├── journal/                  # Journal and CRDT specs
    │   ├── core.qnt              # CRDT journal operations
    │   ├── anti_entropy.qnt      # Delta sync and convergence
    │   └── counter.qnt           # Lamport clock coordination
    ├── keys/                     # Key management specs
    │   ├── dkg.qnt               # Distributed Key Generation
    │   ├── dkd.qnt               # Deterministic Key Derivation
    │   └── resharing.qnt         # Threshold key resharing
    ├── sessions/                 # Session and group specs
    │   ├── core.qnt              # Session lifecycle
    │   ├── groups.qnt            # Group membership management
    │   ├── locking.qnt           # Distributed locking
    │   └── choreography.qnt      # Choreography session types
    ├── amp/                      # AMP channel lifecycle specs
    │   └── channel.qnt           # Channel invites, membership, messaging
    ├── liveness/                 # Liveness analysis specs
    │   ├── timing.qnt            # Synchrony model and timing
    │   ├── connectivity.qnt      # Gossip graph connectivity
    │   └── properties.qnt        # Liveness properties
    ├── harness/                  # Simulator harness modules
    │   ├── dkg.qnt
    │   ├── resharing.qnt
    │   ├── recovery.qnt
    │   ├── locking.qnt
    │   ├── counter.qnt
    │   ├── groups.qnt
    │   ├── flows.qnt             # TUI flow harness
    │   └── amp_channel.qnt       # AMP channel lifecycle harness
    ├── tui/                      # TUI state machine specs
    │   ├── flows.qnt             # TUI flow specifications
    │   ├── state.qnt             # TUI state management
    │   ├── signals.qnt           # TUI signal handling
    │   └── demo_recovery.qnt     # Recovery demo flows
    └── traces/                   # Generated ITF traces (on-demand)
```

---

## Lean 4 Proofs

Mathematical proofs of safety properties using Lean 4.

### Key Properties Proved

#### Consensus Agreement
- **Agreement**: Valid commits for the same consensus instance have the same result
- **Unique Commit**: At most one valid CommitFact per ConsensusId
- **Commit Determinism**: Same threshold shares produce the same commit

#### Consensus Evidence (CRDT)
- **Commutativity**: `merge e1 e2 ≃ merge e2 e1`
- **Associativity**: `merge (merge e1 e2) e3 ≃ merge e1 (merge e2 e3)`
- **Idempotence**: `merge e e ≃ e`
- **Monotonicity**: Votes and equivocators only grow under merge

#### Equivocation Detection
- **Soundness**: Detection only reports actual equivocation
- **Completeness**: All equivocations are detectable
- **Honest Safety**: Honest witnesses are never falsely accused

#### FROST Integration
- **Session Consistency**: All shares in aggregation have same session
- **Threshold Requirement**: Aggregation requires at least k shares
- **Share Binding**: Shares are cryptographically bound to consensus data

#### Journal CRDT
- **Commutativity**: `merge j1 j2 ≃ merge j2 j1`
- **Associativity**: `merge (merge j1 j2) j3 ≃ merge j1 (merge j2 j3)`
- **Idempotence**: `merge j j ≃ j`

#### Context Isolation
- **No Cross-Context Merge**: Messages from different contexts cannot be combined
- **Namespace Isolation**: Incompatible namespaces cannot merge
- **Bridge Authorization**: Cross-context flow requires explicit authorization

#### Flow Budget
- **Monotonic Decrease**: Charging never increases available budget
- **Exact Charge**: Charging exact amount results in zero budget

#### Time System
- **Reflexivity**: `compare policy t t = .eq`
- **Transitivity**: Proper ordering chain preservation
- **Privacy**: Physical time hidden when `ignorePhysical = true`

### Cryptographic Axioms

Documented in `Aura/Assumptions.lean`:

| Axiom | Purpose | Used By |
|-------|---------|---------|
| `frost_threshold_unforgeability` | FROST k-of-n security | Validity, Frost |
| `frost_uniqueness` | Same shares produce same signature | Agreement |
| `hash_collision_resistance` | Prestate binding | Validity |
| `byzantine_threshold` | k > f (threshold > Byzantine) | All safety properties |

### Claims Bundles

Each module exports a Claims bundle for reviewers:

```lean
import Aura.Proofs

-- Infrastructure claims
#check Aura.Proofs.Journal.journalClaims
#check Aura.Proofs.FlowBudget.flowBudgetClaims
#check Aura.Proofs.GuardChain.guardChainClaims
#check Aura.Proofs.TimeSystem.timeSystemClaims
#check Aura.Proofs.KeyDerivation.keyDerivationClaims
#check Aura.Proofs.ContextIsolation.contextIsolationClaims

-- Consensus claims
#check Aura.Proofs.Consensus.Agreement.agreementClaims
#check Aura.Proofs.Consensus.Validity.validityClaims
#check Aura.Proofs.Consensus.Evidence.evidenceClaims
#check Aura.Proofs.Consensus.Equivocation.equivocationClaims
#check Aura.Proofs.Consensus.Frost.frostClaims
#check Aura.Proofs.Consensus.Frost.frostOrchestratorClaims
#check Aura.Proofs.Consensus.Liveness.livenessClaims
#check Aura.Proofs.Consensus.Adversary.adversaryClaims
#check Aura.Proofs.Consensus.Summary.consensusClaims  -- Main bundle
```

### Lean Commands

```bash
just verify-lean       # Build and check proofs
just lean-check        # Alias for verify-lean
just lean-clean        # Clean build artifacts
just lean-full         # Clean + build + check
just lean-status       # Per-module status
just lean-oracle-build # Build aura_verifier (Lean oracle)
just test-differential # Rust vs Lean oracle tests
```

---

## Quint Specifications

Executable state machine specifications using Quint, based on the Temporal Logic of Actions (TLA).

### Common Commands

```bash
quint typecheck <spec>.qnt              # Check syntax and types
quint repl <spec>.qnt                   # Run the REPL
quint run <spec>.qnt                    # Generate random traces
quint verify <spec>.qnt                 # Model checking (requires Apalache)
```

### Protocol Specifications

| Directory | Specification | Description | Documentation |
|-----------|---------------|-------------|---------------|
| `.` | `core.qnt` | Shared runtime utilities, protocol lifecycle, effects, timers | [System Architecture](../docs/001_system_architecture.md) |
| `consensus/` | `core.qnt` | Fast-path/fallback consensus with threshold signatures | [Consensus](../docs/106_consensus.md) |
| `consensus/` | `frost.qnt` | FROST threshold signature protocol model | [Crypto Guide](../docs/100_crypto.md) |
| `consensus/` | `adversary.qnt` | Byzantine adversary models for consensus | [Distributed Contract](../docs/004_distributed_systems_contract.md) |
| `consensus/` | `liveness.qnt` | Liveness and termination properties | [Distributed Contract](../docs/004_distributed_systems_contract.md) |
| `journal/` | `core.qnt` | CRDT journal operations | [Journal Guide](../docs/103_journal.md) |
| `journal/` | `anti_entropy.qnt` | CRDT delta sync and eventual convergence | [Maintenance](../docs/115_maintenance.md) |
| `journal/` | `counter.qnt` | Lamport clock counter coordination | - |
| `keys/` | `dkg.qnt` | FROST Distributed Key Generation ceremony | [Crypto Guide](../docs/100_crypto.md) |
| `keys/` | `dkd.qnt` | Deterministic Key Derivation for context keys | [Crypto Guide](../docs/100_crypto.md) |
| `keys/` | `resharing.qnt` | Threshold key resharing protocol | [Crypto Guide](../docs/100_crypto.md) |
| `sessions/` | `core.qnt` | Session lifecycle and presence | [MPST Guide](../docs/108_mpst_and_choreography.md) |
| `sessions/` | `groups.qnt` | Group membership management | [Social Architecture](../docs/114_social_architecture.md) |
| `sessions/` | `locking.qnt` | Distributed locking protocol | - |
| `sessions/` | `choreography.qnt` | Choreography session type specifications | [MPST Guide](../docs/108_mpst_and_choreography.md) |
| `amp/` | `channel.qnt` | AMP channel lifecycle (invite/join/send/leave/rotate) | [AMP](../docs/110_amp.md) |
| `.` | `recovery.qnt` | Guardian-based recovery flows | [Relational Contexts](../docs/112_relational_contexts.md) |
| `.` | `authorization.qnt` | Guard chain authorization, budget verification | [Information Flow](../docs/003_information_flow_contract.md) |
| `.` | `epochs.qnt` | Epoch transitions and receipt validity windows | [Transport](../docs/109_transport_and_information_flow.md) |
| `.` | `transport.qnt` | Transport layer: connections, sessions, guard chain | [Transport](../docs/109_transport_and_information_flow.md) |
| `.` | `invitation.qnt` | Invitation lifecycle, ceremonies, authorization | [Relational Contexts](../docs/112_relational_contexts.md) |
| `.` | `sbb.qnt` | Social Bulletin Board gossip | [Rendezvous](../docs/111_rendezvous.md) |
| `.` | `interaction.qnt` | Recovery∥Consensus concurrent execution safety | [Distributed Contract](../docs/004_distributed_systems_contract.md) |
| `.` | `leakage.qnt` | Information flow leakage tracking | [Information Flow](../docs/003_information_flow_contract.md) |
| `.` | `time_system.qnt` | Time system properties and constraints | [System Architecture](../docs/001_system_architecture.md) |

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
| `flows.qnt` | TUI Flows | Guardian recovery, home lifecycle, neighborhood, chat, invitation, social graph |
| `amp_channel.qnt` | AMP Channel | `ampChannelLifecycle` |

### Design Principles

#### Authority Model

All specifications use the authority model with opaque identifiers:

```quint
type AuthorityId = str   // Opaque authority identifier
type ContextId = str     // Relational context identifier
type ProtocolId = str    // Protocol instance identifier
```

#### Protocol Lifecycle

Protocols follow a standard lifecycle with typestate transitions:

```
Initialized → Active → AwaitingEvidence → Completed
                   ↘                    ↗
                     → Failed/Cancelled
```

#### Effect System

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

### Verified Invariants

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

---

## Quint-Lean Correspondence

This section maps Quint model invariants to Lean theorem proofs.

### Types Correspondence

| Quint Type | Lean Type | Rust Type |
|------------|-----------|-----------|
| `ConsensusId` | `Aura.Domain.Consensus.Types.ConsensusId` | `consensus::types::ConsensusId` |
| `ResultId` | `Aura.Domain.Consensus.Types.ResultId` | `consensus::types::ResultId` |
| `PrestateHash` | `Aura.Domain.Consensus.Types.PrestateHash` | `consensus::types::PrestateHash` |
| `AuthorityId` | `Aura.Domain.Consensus.Types.AuthorityId` | `core::AuthorityId` |
| `ShareData` | `Aura.Domain.Consensus.Types.ShareData` | `consensus::types::SignatureShare` |
| `ThresholdSignature` | `Aura.Domain.Consensus.Types.ThresholdSignature` | `consensus::types::ThresholdSignature` |
| `CommitFact` | `Aura.Domain.Consensus.Types.CommitFact` | `consensus::types::CommitFact` |
| `WitnessVote` | `Aura.Domain.Consensus.Types.WitnessVote` | `consensus::types::WitnessVote` |
| `Evidence` | `Aura.Domain.Consensus.Types.Evidence` | `consensus::types::Evidence` |

### Invariant-Theorem Correspondence

#### Agreement Properties

| Quint Invariant | Lean Theorem | Status |
|-----------------|--------------|--------|
| `InvariantUniqueCommitPerInstance` | `Aura.Proofs.Consensus.Agreement.agreement` | proven |
| `InvariantUniqueCommitPerInstance` | `Aura.Proofs.Consensus.Agreement.unique_commit` | proven |
| - | `Aura.Proofs.Consensus.Agreement.commit_determinism` | proven |

#### Validity Properties

| Quint Invariant | Lean Theorem | Status |
|-----------------|--------------|--------|
| `InvariantCommitRequiresThreshold` | `Aura.Proofs.Consensus.Validity.commit_has_threshold` | proven |
| `InvariantSignatureBindsToCommitFact` | `Aura.Proofs.Consensus.Validity.validity` | proven |
| - | `Aura.Proofs.Consensus.Validity.distinct_signers` | proven |
| - | `Aura.Proofs.Consensus.Validity.prestate_binding_unique` | proven |
| - | `Aura.Proofs.Consensus.Validity.honest_participation` | proven |
| - | `Aura.Proofs.Consensus.Validity.threshold_unforgeability` | axiom |

#### FROST Integration Properties

| Quint Invariant | Lean Theorem | Status |
|-----------------|--------------|--------|
| `InvariantSignatureThreshold` | `Aura.Proofs.Consensus.Frost.aggregation_threshold` | proven |
| - | `Aura.Proofs.Consensus.Frost.share_session_consistency` | proven |
| - | `Aura.Proofs.Consensus.Frost.share_result_consistency` | proven |
| - | `Aura.Proofs.Consensus.Frost.distinct_signers` | proven |
| - | `Aura.Proofs.Consensus.Frost.share_binding` | proven |

#### Evidence CRDT Properties

| Quint Invariant | Lean Theorem | Status |
|-----------------|--------------|--------|
| - | `Aura.Proofs.Consensus.Evidence.merge_comm_votes` | proven |
| - | `Aura.Proofs.Consensus.Evidence.merge_assoc_votes` | proven |
| - | `Aura.Proofs.Consensus.Evidence.merge_idem` | proven |
| - | `Aura.Proofs.Consensus.Evidence.merge_preserves_commit` | proven |
| - | `Aura.Proofs.Consensus.Evidence.commit_monotonic` | proven |

#### Equivocation Detection Properties

| Quint Invariant | Lean Theorem | Status |
|-----------------|--------------|--------|
| `InvariantEquivocationDetected` | `Aura.Proofs.Consensus.Equivocation.detection_soundness` | proven |
| `InvariantEquivocationDetected` | `Aura.Proofs.Consensus.Equivocation.detection_completeness` | proven |
| `InvariantEquivocatorsExcluded` | `Aura.Proofs.Consensus.Equivocation.exclusion_correctness` | proven |
| `InvariantHonestMajorityCanCommit` | `Aura.Proofs.Consensus.Equivocation.honest_never_detected` | proven |
| - | `Aura.Proofs.Consensus.Equivocation.verified_proof_sound` | proven |

#### Byzantine Tolerance (Adversary Module)

| Quint Invariant | Lean Theorem | Status |
|-----------------|--------------|--------|
| `InvariantByzantineThreshold` | `Aura.Proofs.Consensus.Adversary.adversaryClaims.byzantine_cannot_forge` | claim |
| `InvariantEquivocationDetected` | `Aura.Proofs.Consensus.Adversary.adversaryClaims.equivocation_detectable` | claim |
| `InvariantHonestMajorityCanCommit` | `Aura.Proofs.Consensus.Adversary.adversaryClaims.honest_majority_sufficient` | claim |
| `InvariantEquivocatorsExcluded` | `Aura.Proofs.Consensus.Adversary.adversaryClaims.equivocators_excluded` | claim |
| `InvariantCompromisedNoncesExcluded` | - | Quint only |

#### Liveness Properties

| Quint Property | Lean Support | Notes |
|----------------|--------------|-------|
| `InvariantProgressUnderSynchrony` | `Aura.Proofs.Consensus.Liveness.livenessClaims.terminationUnderSynchrony` | axiom |
| `InvariantByzantineTolerance` | `byzantine_threshold` | axiom |
| `FastPathProgressCheck` | `Aura.Proofs.Consensus.Liveness.livenessClaims.fastPathBound` | axiom |
| `SlowPathProgressCheck` | `Aura.Proofs.Consensus.Liveness.livenessClaims.fallbackBound` | axiom |
| `NoDeadlock` | `Aura.Proofs.Consensus.Liveness.livenessClaims.noDeadlock` | axiom |
| `InvariantRetryBound` | - | Quint model checking only |

#### Module Correspondence

| Lean Module | Quint File | What It Proves |
|-------------|------------|----------------|
| `Proofs.ContextIsolation` | `authorization.qnt`, `leakage.qnt` | Context separation and bridge authorization |
| `Proofs.Consensus.Agreement` | `consensus/core.qnt` | Agreement safety (unique commits) |
| `Proofs.Consensus.Evidence` | `consensus/core.qnt` | CRDT semilattice properties |
| `Proofs.Consensus.Frost` | `consensus/frost.qnt` | Threshold signature correctness |
| `Proofs.Consensus.Liveness` | `consensus/liveness.qnt` | Synchrony model axioms |
| `Proofs.Consensus.Adversary` | `consensus/adversary.qnt` | Byzantine tolerance bounds |
| `Proofs.Consensus.Equivocation` | `consensus/adversary.qnt` | Detection soundness/completeness |

---

## Verified Properties Summary

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

---

## Integration

### Simulator Integration

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

Model-based testing traces can be generated into `verification/quint/traces/`. The directory starts empty; traces are generated on-demand.

| Trace File | Source Spec | Description |
|------------|-------------|-------------|
| `cap_props.itf.json` | `authorization.qnt` | Guard chain and budget verification |
| `frost.itf.json` | `consensus/frost.qnt` | FROST threshold signature protocol |
| `dkg.itf.json` | `keys/dkg.qnt` | DKG ceremony execution |
| `consensus.itf.json` | `consensus/core.qnt` | Fast-path/fallback consensus |
| `cross_interaction.itf.json` | `interaction.qnt` | Concurrent protocol safety |
| `anti_entropy.itf.json` | `journal/anti_entropy.qnt` | CRDT synchronization |
| `epochs.itf.json` | `epochs.qnt` | Epoch transitions and receipts |
| `amp_channel.itf.json` | `harness/amp_channel.qnt` | AMP channel lifecycle |

Generate traces:

```bash
quint run verification/quint/consensus/core.qnt \
  --main=protocol_consensus \
  --max-samples=5 --max-steps=20 \
  --out-itf=verification/quint/traces/consensus.itf.json

# AMP channel (requires MBT metadata):
quint run verification/quint/harness/amp_channel.qnt \
  --main=harness_amp_channel \
  --max-samples=1 --max-steps=20 \
  --mbt \
  --out-itf=verification/quint/traces/amp_channel.itf.json
```

### Bounded Liveness Checking

The `aura-simulator` crate provides bounded liveness checking:

```rust
use aura_simulator::liveness::{
    BoundedLivenessChecker, BoundedLivenessProperty, SynchronyAssumption
};

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

### Differential Testing

The Lean oracle supports differential testing against Rust:

```bash
just lean-oracle-build
just test-differential
```

Oracle commands (JSON stdin/stdout):
- `aura_verifier journal-merge`
- `aura_verifier journal-reduce`
- `aura_verifier flow-charge`
- `aura_verifier timestamp-compare`

### ITF Trace Conformance

```bash
# Generate traces
quint run verification/quint/consensus/core.qnt \
  --out-itf traces/consensus/trace.itf.json --max-steps 20

# Run conformance tests
cargo test -p aura-testkit --test consensus_itf_conformance
```

---

## Commands Reference

### Lean

```bash
just verify-lean         # Build and check proofs
just lean-check          # Alias for verify-lean
just lean-clean          # Clean build artifacts
just lean-full           # Clean + build + check
just lean-status         # Per-module status
just lean-oracle-build   # Build aura_verifier (Lean oracle)
just test-differential   # Rust vs Lean oracle tests
```

### Quint

```bash
just verify-quint        # Verify Quint setup
just ci-quint-typecheck  # CI typecheck
just ci-quint-verify     # CI model checking
```

### Tooling

```bash
just quint-check-types            # Check Quint-Rust type correspondence
just quint-check-types --verbose  # Detailed report
just verification-coverage        # Markdown coverage report
just verification-coverage --json # JSON metrics
just verify-all                   # Lean + Quint + conformance tests
```

---

## Resources

- [Quint Documentation](https://quint-lang.org/docs)
- [Verification Coverage](../docs/998_verification_coverage.md)
- [Simulation Guide](../docs/805_simulation_guide.md)
- [Verification Guide](../docs/806_verification_guide.md)
- [System Architecture](../docs/001_system_architecture.md)

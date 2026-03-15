# Aura Anti-Entropy (Layer 4) - Architecture and Invariants

## Purpose
Provide digest-based reconciliation and broadcast coordination for OpLog sync,
with explicit guard chain enforcement on network operations.

## Inputs
- BloomDigest values for reconciliation.
- GuardChainEffects + TransportEffects for effectful sync paths.
- StorageEffects for persistent OpLog caching.

## Outputs
- Merged OpLog updates (pure set union semantics).
- Guarded network operations (digest requests, op requests, announcements).

## Invariants
- Reconciliation logic is pure (see `sync/pure.rs`).
- Network-visible operations must be guard-chain approved.
- Persistent storage uses shared commitment tree storage keys.
- Vector-clock metadata remains causally consistent (`InvariantVectorClockConsistent`).

## Ownership Model

- `aura-anti-entropy` keeps reconciliation logic `Pure`.
- Any exclusive sync-session or reconciliation ownership should remain
  `MoveOwned` in higher-layer coordination surfaces.
- Long-lived background reconciliation ownership belongs in explicit
  `ActorOwned` runtime services, not hidden here.
- Guarded sync publication must remain capability-aware and typed.
- `Observed` tooling may inspect reconciliation outcomes but not define them.

### Detailed Specifications

### InvariantAntiEntropyReconciliationPurity
Anti-entropy reconciliation remains pure and deterministic for identical inputs.

Enforcement locus:
- sync pure logic computes reconciliation decisions without side effects.
- Network execution applies outputs only after guard-chain approval.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just test-crate aura-anti-entropy

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) defines deterministic reduction.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines eventual convergence and vector-clock consistency.

### InvariantVectorClockConsistent
Anti-entropy reconciliation must preserve causal ordering metadata so merged views never violate vector-clock partial order.

Enforcement locus:
- sync digest exchange carries causal metadata for reconciliation decisions.
- merge path applies operations in a causally admissible order.
- guard-approved transport path preserves per-context sequencing assumptions.

Failure mode:
- Causal anomalies during replay/reconciliation.
- Divergent merged states after partition healing.

Verification hooks:
- `cargo test -p aura-anti-entropy`
- `quint run --invariant=InvariantVectorClockConsistent verification/quint/journal/anti_entropy.qnt`

Contract alignment:
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines `InvariantVectorClockConsistent`.
## Boundaries
- No guardless network sends.
- Storage helpers are shared via `aura_journal::commitment_tree::storage`.

## Core + Orchestrator Rule
- Pure reconciliation lives in `sync/pure.rs`.
- Effectful orchestration must accept explicit effect traits.

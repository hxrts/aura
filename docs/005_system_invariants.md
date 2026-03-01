# Aura System Invariants

This document indexes invariants across Aura and maps them to enforcement loci.
Invariant specifications live in crate `ARCHITECTURE.md` files.
Contracts in [Theoretical Model](002_theoretical_model.md), [Privacy and Information Flow Contract](003_information_flow_contract.md), and [Distributed Systems Contract](004_distributed_systems_contract.md) define the cross-crate safety model.

## Scope

This index tracks invariants that protect safety, consistency, and privacy.
Every invariant must include a canonical name, enforcement locus, failure mode, and verification hooks.
Standalone `INVARIANTS.md` files are not used.

## Canonical Naming

Use `InvariantXxx` names in proofs and tests.
Use prose aliases for readability when needed.
When both forms appear, introduce the alias once and then reference the canonical name.

Examples:
- `Charge-Before-Send` maps to `InvariantSentMessagesHaveFacts` and `InvariantFlowBudgetNonNegative`.
- `Context Isolation` maps to `InvariantContextIsolation`.
- `Secure Channel Lifecycle` maps to `InvariantReceiptValidityWindow` and `InvariantCrossEpochReplayPrevention`.

## Core Invariant Index

| Alias | Canonical Name(s) | Primary Enforcement | Related Contracts |
| --- | --- | --- | --- |
| Charge-Before-Send | `InvariantSentMessagesHaveFacts`, `InvariantFlowBudgetNonNegative` | [crates/aura-guards/ARCHITECTURE.md](../crates/aura-guards/ARCHITECTURE.md) | [Privacy and Information Flow Contract](003_information_flow_contract.md), [Distributed Systems Contract](004_distributed_systems_contract.md) |
| CRDT Convergence | `InvariantCRDTConvergence` | [crates/aura-journal/ARCHITECTURE.md](../crates/aura-journal/ARCHITECTURE.md) | [Theoretical Model](002_theoretical_model.md), [Distributed Systems Contract](004_distributed_systems_contract.md) |
| Context Isolation | `InvariantContextIsolation` | [crates/aura-core/ARCHITECTURE.md](../crates/aura-core/ARCHITECTURE.md) | [Theoretical Model](002_theoretical_model.md), [Privacy and Information Flow Contract](003_information_flow_contract.md), [Distributed Systems Contract](004_distributed_systems_contract.md) |
| Secure Channel Lifecycle | `InvariantSecureChannelLifecycle`, `InvariantReceiptValidityWindow`, `InvariantCrossEpochReplayPrevention` | [crates/aura-rendezvous/ARCHITECTURE.md](../crates/aura-rendezvous/ARCHITECTURE.md) | [Privacy and Information Flow Contract](003_information_flow_contract.md), [Distributed Systems Contract](004_distributed_systems_contract.md) |
| Authority Tree Topology and Commitment Coherence | `InvariantAuthorityTreeTopologyCommitmentCoherence` | [crates/aura-journal/ARCHITECTURE.md](../crates/aura-journal/ARCHITECTURE.md) | [Theoretical Model](002_theoretical_model.md), [Distributed Systems Contract](004_distributed_systems_contract.md) |

## Distributed Contract Invariant Names

The distributed and privacy contracts define additional canonical names used by proofs and conformance tests.
These include:

- `InvariantUniqueCommitPerInstance`
- `InvariantCommitRequiresThreshold`
- `InvariantEquivocatorsExcluded`
- `InvariantNonceUnique`
- `InvariantSequenceMonotonic`
- `InvariantReceiptValidityWindow`
- `InvariantCrossEpochReplayPrevention`
- `InvariantVectorClockConsistent`
- `InvariantHonestMajorityCanCommit`
- `InvariantCompromisedNoncesExcluded`

When a crate enforces one of these invariants, record the same canonical name in that crate `ARCHITECTURE.md`.

## Validation and Verification

Use `just check-arch` to validate architecture and layering constraints.
Use `just test` for workspace-wide regression checks.
Use `just test-crate <crate>` for focused enforcement checks in a crate.
Use `nix flake check` for hermetic conformance.

Formal and model checks should reference the same canonical names listed here and in contracts.

## Adding or Updating an Invariant

1. Add or update the invariant under `## Invariants` in the crate `ARCHITECTURE.md`.
2. Add a detailed specification section in the same file with invariant name, enforcement locus, failure mode, and verification hooks.
3. Use canonical `InvariantXxx` naming for traceability across docs, tests, and proofs.
4. Add or update tests and simulator scenarios that detect violations.
5. Update this index if the invariant is cross-crate or contract-level.

## Incident Handling for Invariant Violations

1. Stop release or deployment for the affected path.
2. File a critical issue with invariant name, impact, and reproduction steps.
3. Add a failing regression test that captures the violation.
4. Implement the fix and reference the canonical invariant name in the change.
5. Verify conformance and update documentation links if enforcement moved.

## Related Documentation

- [Aura System Architecture](001_system_architecture.md)
- [Theoretical Model](002_theoretical_model.md)
- [Privacy and Information Flow Contract](003_information_flow_contract.md)
- [Distributed Systems Contract](004_distributed_systems_contract.md)
- [Effect System and Runtime](105_effect_system_and_runtime.md)

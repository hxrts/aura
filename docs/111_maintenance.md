# Maintenance Guidelines

This document captures the operational practices required to keep Aura deployments healthy. It complements the architecture docs by explaining when to rotate epochs, prune journals, refresh capabilities, and upgrade handlers. The procedures below reference the canonical specs inside `docs_2/`.

## Journal Hygiene

- **Snapshots and pruning**: Follow the rules in `102_journal.md` and ``. Create `SnapshotFact` entries once the fact set grows beyond local thresholds, then garbage collect any fact dominated by the snapshot digest. Always keep the most recent two epochs of `FlowBudget` facts so charge reconciliation can survive transient partitions.
- **Receipts**: Receipts described in `003_information_flow_contract.md` and `108_transport_and_information_flow.md` must be retained for at least one epoch after issuance so downstream auditors can validate relay behavior. When pruning, ensure that any receipt referenced by an in-progress recovery or rendezvous flow remains available.

## Epoch and Key Rotation

- **Account epochs**: As defined in `101_accounts_and_ratchet_tree.md`, rotate the ratchet tree epoch whenever device membership changes, recovery completes, or stale capability exposure is suspected. Rotation invalidates derived context keys and forces FlowBudget counters to reset.
- **Context epochs**: Relational contexts from `103_relational_contexts.md` should bump epochs when guardian membership changes or when rendezvous descriptors leak. All participants must renegotiate secure channels after the bump.

## Capability and Budget Refresh

- **Biscuit cache invalidation**: CapGuard implementations (`109_authorization.md`) must flush cached capability frontiers when journal policy facts change, when Biscuit revocation lists update, or when the authority rotates its root commitment.
- **Flow budget floors**: Periodically recompute policy-derived `limit` values for dormant peers to ensure they never drop below the minimum floor required for liveness. Any manual overrides should be committed as new policy facts rather than local configuration.

## Relational Context Care

- **Guardian bindings**: Regularly audit `GuardianBinding` and `RecoveryGrant` facts per `103_relational_contexts.md` to confirm that guardian authorities remain reachable and that consensus proofs are still valid under the latest authority commitments.
- **Consensus transcripts**: Aura Consensus commits described in `104_consensus.md` should be archived along with their evidence deltas. Before deleting old commit facts, confirm that every dependent reducer (account or context) has applied the commit and that no pending recovery references it.

## Runtime and Handler Upgrades

- **Effect handlers**: When upgrading handler crates referenced in `106_effect_system_and_runtime.md`, follow the lifecycle protocol—enter `shutting_down`, drain inflight work, apply the upgrade, then re-enter `ready`. This prevents dangling FlowGuard reservations.
- **Transport services**: Rendezvous and guard-chain components (`108_transport_and_information_flow.md`, `108_rendezvous.md`) must be upgraded in lockstep so that new receipt formats or flow-cost policies remain consistent across peers. Always rotate rendezvous descriptors after a transport upgrade.

## Monitoring Checklist

Operators should monitor the following indicators:

1. **Journal growth rate** – ensure snapshots keep storage usage bounded.
2. **FlowBudget spend vs limit** – detect stuck contexts before budgets exhaust.
3. **Consensus backlog** – unexpected growth indicates witness or transport failure.
4. **Receipt validation errors** – may point to relay misbehavior or epoch drift.
5. **Capability cache hit rate** – excessively low values suggest churning policy facts.

Keeping these maintenance routines in place ensures the architecture described throughout `docs_2/` remains reliable in production deployments.

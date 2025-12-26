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

## Boundaries
- No guardless network sends.
- Storage helpers are shared via `aura_journal::commitment_tree::storage`.

## Core + Orchestrator Rule
- Pure reconciliation lives in `sync/pure.rs`.
- Effectful orchestration must accept explicit effect traits.

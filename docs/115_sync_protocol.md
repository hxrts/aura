# Sync Protocol (Anti-Entropy + Journal Sync)

This document specifies the sync protocol phases, digest format, and retry behavior
for `aura-sync` (anti-entropy + journal sync).

## Scope

- Anti-entropy protocol: digest exchange, reconciliation planning, operation transfer
- Journal sync protocol: coordination wrapper around anti-entropy with peer state tracking

## Anti-Entropy Phases

1. **Load Local State**
   - Read local `Journal` (facts + caps) and the local operation log (if present).
2. **Compute Digest**
   - Compute `JournalDigest` for local state.
3. **Digest Exchange**
   - Send local digest to peer and receive peer digest.
4. **Reconciliation Planning**
   - Compare digests and choose action:
     - Equal → no-op
     - LocalBehind → request missing ops
     - RemoteBehind → push ops
     - Diverged → push + pull
5. **Operation Transfer**
   - Pull or push operations in batches.
6. **Merge + Persist**
   - Convert applied ops to journal delta, merge with local journal, persist once per round.

## Digest Format

`JournalDigest` is:

- `operation_count`: number of operations in local op log
- `last_epoch`: max `parent_epoch` observed in the op log
- `operation_hash`: hash of ordered op fingerprints
- `fact_hash`: hash of canonical serialization of `Journal.facts`
- `caps_hash`: hash of canonical serialization of `Journal.caps`

### Hashing Rules

- `fact_hash` and `caps_hash` use `aura_core::util::serialization::to_vec` (canonical DAG-CBOR),
  then `aura_core::hash::hash`.
- `operation_hash` is computed by streaming op fingerprints in deterministic order.
  The op log must provide a stable order (e.g., op index or log order).
- Each op fingerprint uses the same canonical serialization + hash as above.

## Determinism Requirements

- `JournalDigest` is deterministic for identical journal + op log input.
- Operation deltas are built in deterministic order (sorted by fingerprint) to
  prevent non-deterministic merge behavior.

## Retry Behavior

Anti-entropy can be retried according to `AntiEntropyConfig.retry_policy`.
The default policy is exponential backoff with a bounded max attempt count.

## Failure Semantics

Failures are reported with structured phase context:

- `SyncPhase::LoadLocalState`
- `SyncPhase::ComputeDigest`
- `SyncPhase::DigestExchange`
- `SyncPhase::PlanRequest`
- `SyncPhase::ReceiveOperations`
- `SyncPhase::MergeJournal`
- `SyncPhase::PersistJournal`

This makes failures attributable to a specific phase and peer.


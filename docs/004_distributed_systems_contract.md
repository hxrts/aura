# Distributed Systems Contract

This contract specifies Aura's distributed systems model: the safety, liveness, and consistency guarantees afforded by the architecture, along with the synchrony assumptions, latency expectations, and adversarial capabilities the system tolerates. It complements `003_privacy_and_information_flow.md`, which focuses on metadata/privacy budgets. Together these contracts define the full set of invariants protocol authors must respect.

## 1. Scope

The contract applies to:

- Effect handlers and protocols that operate within the 8-layer architecture (`001_system_architecture.md`).
- Journals and reducers (`101_accounts_and_ratchet_tree.md`, `102_journal.md`, `110_state_reduction_flows.md`).
- Aura Consensus (`104_consensus.md`).
- Relational contexts and rendezvous flows (`103_relational_contexts.md`, `107_transport_and_information_flow.md`, `108_rendezvous.md`).

## 2. Safety Guarantees

1. **Journals are monotone** – Facts merge via set union, never retract (`102_journal.md`). Reduction is deterministic: identical fact sets produce identical states.
2. **Charge-before-send** – Every transport observable is preceded by CapGuard → FlowGuard → JournalCoupler (`105_effect_system_and_runtime.md`, `108_authorization_pipeline.md`). No packet is emitted without a successful charge.
3. **Consensus agreement** – For any `(cid, prestate_hash)` there is at most one commit fact (`104_consensus.md`). Fallback gossip plus FROST signatures prevent divergent commits.
4. **Context isolation** – Messages scoped to `ContextId` never leak into other contexts unless explicitly bridged through a typed protocol (`001_theoretical_model.md`).
5. **Deterministic reduction order** – Ratchet tree operations resolve conflicts using the stable ordering described in `101_accounts_and_ratchet_tree.md`.
6. **Receipts chain** – Multi-hop forwarding requires signed receipts; downstream peers reject messages lacking a chain rooted in their relational context (`107_transport_and_information_flow.md`).

## 3. Liveness Guarantees

1. **Fast-path consensus** – Completes in one RTT when all witnesses are online.
2. **Fallback consensus** – Eventually completes under partial synchrony with bounded message delays; gossip ensures progress if a majority of witnesses re-transmit proposals.
3. **Anti-entropy** – Journals converge under eventual delivery: periodic syncs (or reorder-resistant CRDT merges) reconcile fact sets even after partitions.
4. **Rendezvous** – Offer/answer envelopes flood gossip neighborhoods; as long as at least one bidirectional path remains, secure channels can be established.
5. **Flow budgets** – Provided limit > 0, FlowGuard eventually grants headroom once the epoch rotates or recipients replenish budgets.

Liveness requires that each authority eventually receives messages from its immediate neighbors (eventual delivery) and that clocks do not drift unboundedly (for epoch rotation/receipt expiry).

## 4. Synchrony and Timing Model

Aura assumes **partial synchrony**:

- There exists a (possibly unknown) bound Δ on message delay and processing time once the network stabilizes.
- `T_fallback` in consensus is configured as `2–3 × Δ`. Before stabilization, timeouts may be conservative.
- Gossip intervals target 250–500 ms; handlers must tolerate jitter but assume eventual delivery of periodic messages.
- Epoch rotation relies on loosely synchronized clocks but uses journal facts as the source of truth; authorities treat an epoch change as effective once they observe it in the journal.

## 5. Latency Expectations

| Operation                          | Typical Bound (Δ assumptions) |
|-----------------------------------|-------------------------------|
| Threshold tree update (fast path) | ≤ 2 × RTT of slowest witness  |
| Rendezvous offer propagation      | O(log N) gossip hops          |
| FlowGuard charge                  | Single local transaction (<10 ms) |
| Anti-entropy reconciliation       | k × gossip period (k depends on fanout) |

These are guidelines; actual deployments should benchmark and set alarms when latency significantly exceeds expectations.

## 6. Adversarial Model

### 6.1 Network Adversary

- Controls a subset of transport links; can delay or drop packets but cannot break cryptography.
- Cannot forge receipts without FlowGuard grants (signatures + epoch binding).
- May simultaneously compromise up to `f < t` authorities per consensus instance without violating safety (where `t` is the threshold).

### 6.2 Byzantine Witness

- May equivocate during consensus. Evidence CRDT plus equivocation detection (`HasEquivocated`, `HasEquivocatedInSet`) excludes conflicting shares.
- Cannot cause multiple commits because threshold signature verification rejects tampered results.

### 6.3 Malicious Relay

- May drop or delay envelopes but cannot forge flow receipts or read payloads thanks to context-specific encryption.
- Downstream peers detect misbehavior via missing receipts or inconsistent budget charges.

### 6.4 Device Compromise

- A compromised device reveals its share and journal copy but cannot reconstitute the account without meeting the branch policy (threshold). Recovery relies on relational contexts as described in `103_relational_contexts.md`.

## 7. Consistency Model

- **Eventual consistency** for journals: after exchanging all facts, replicas converge.
- **Strong consistency** for operations guarded by Aura Consensus: once a commit fact is accepted, all honest replicas agree on the result.
- **Causal delivery** is not enforced at the transport layer; choreographies enforce ordering via session types instead.
- **Monotonic read-after-write**: Each authority’s view of its own journal is monotone; once it observes a fact locally, it will never “un-see” it.

## 8. Failure Handling

- **Timeouts** trigger fallback consensus (see `104_consensus.md` for `T_fallback` guidelines).
- **Partition recovery** relies on anti-entropy: authorities merge fact sets when connectivity returns.
- **Budget exhaustion** causes local blocking; protocols must implement backoff or wait for epoch rotation.
- **Guard-chain failures** return local errors (AuthorizationDenied, InsufficientBudget, JournalCommitFailed). Protocol authors must handle these without leaking information.

## 9. Deployment Guidance

- Configure witness sets such that `t > f` where `f` is the maximum number of Byzantine authorities tolerated.
- Tune gossip fanout and timeout parameters based on observed RTTs and network topology.
- Monitor receipt acceptance rates, consensus backlog, and budget utilization (see `109_maintenance.md`) to detect synchrony violations early.

## 10. References

- `001_system_architecture.md` – runtime layering and guard chain.
- `001_theoretical_model.md` – formal calculus and semilattice laws.
- `101_accounts_and_ratchet_tree.md` – reduction ordering.
- `102_journal.md` & `110_state_reduction_flows.md` – fact storage and convergence.
- `103_relational_contexts.md` – cross-authority state.
- `104_consensus.md` – fast path and fallback consensus.
- `107_transport_and_information_flow.md` – transport semantics.
- `108_authorization_pipeline.md` – CapGuard/FlowGuard sequencing.

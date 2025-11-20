# Distributed Systems Contract

This contract specifies Aura's distributed systems model. It defines the safety, liveness, and consistency guarantees provided by the architecture. It also documents the synchrony assumptions, latency expectations, and adversarial capabilities the system tolerates. This contract complements [Privacy and Information Flow](003_information_flow_contract.md), which focuses on metadata and privacy budgets. Together these contracts define the full set of invariants protocol authors must respect.

## 1. Scope

The contract applies to the following aspects of the system.

Effect handlers and protocols operate within the 8-layer architecture described in [System Architecture](001_system_architecture.md). Journals and reducers are covered by this contract. The journal specification appears in [Accounts and Ratchet Tree](101_accounts_and_ratchet_tree.md) and [Journal System](102_journal.md). Aura Consensus is documented in [Consensus](104_consensus.md).

Relational contexts and rendezvous flows fall under this contract. Relational contexts are specified in [Relational Contexts](103_relational_contexts.md). Transport semantics appear in [Transport and Information Flow](108_transport_and_information_flow.md). Rendezvous flows are detailed in [Rendezvous Architecture](110_rendezvous.md).

## 2. Safety Guarantees

### Journals are monotone

Facts merge via set union and never retract. See [Journal System](102_journal.md) for details. Reduction is deterministic. Identical fact sets produce identical states.

### Charge-before-send

Every transport observable is preceded by `CapGuard`, `FlowGuard`, and `JournalCoupler`. See [Effect System and Runtime](106_effect_system_and_runtime.md) and [Authorization](109_authorization.md). No packet is emitted without a successful charge. This invariant is enforced by the guard chain.

### Consensus agreement

For any pair `(cid, prestate_hash)` there is at most one commit fact. See [Consensus](104_consensus.md). Fallback gossip plus FROST signatures prevent divergent commits. Byzantine witnesses cannot force multiple commits for the same instance.

### Context isolation

Messages scoped to `ContextId` never leak into other contexts. Contexts may be explicitly bridged through typed protocols only. See [Theoretical Model](002_theoretical_model.md). Each authority maintains separate journals per context to enforce this isolation.

### Deterministic reduction order

Ratchet tree operations resolve conflicts using the stable ordering described in [Accounts and Ratchet Tree](101_accounts_and_ratchet_tree.md). This ordering is derived from the cryptographic identifiers and facts stored in the journal. Conflicts are always resolved in the same way across all replicas.

### Receipts chain

Multi-hop forwarding requires signed receipts. Downstream peers reject messages lacking a chain rooted in their relational context. See [Transport and Information Flow](108_transport_and_information_flow.md). This prevents unauthorized message propagation.

## 3. Liveness Guarantees

### Fast-path consensus

Fast-path consensus completes in one round-trip time (RTT) when all witnesses are online. Responses are gathered synchronously before committing.

### Fallback consensus

Fallback consensus eventually completes under partial synchrony with bounded message delays. Gossip ensures progress if a majority of witnesses re-transmit proposals. See [Consensus](104_consensus.md) for timeout configuration.

### Anti-entropy

Journals converge under eventual delivery. Periodic syncs or reorder-resistant CRDT merges reconcile fact sets even after partitions. Authorities exchange their complete fact journals with neighbors to ensure diverged state is healed.

### Rendezvous

Offer and answer envelopes flood gossip neighborhoods. Secure channels can be established as long as at least one bidirectional path remains between parties. See [Rendezvous Architecture](110_rendezvous.md).

### Flow budgets

Provided the limit is greater than zero, `FlowGuard` eventually grants headroom. Headroom is available once the epoch rotates or recipients replenish budgets. Budget exhaustion is temporary and recoverable.

Liveness requires that each authority eventually receives messages from its immediate neighbors. This is the eventual delivery assumption. Liveness also requires that clocks do not drift unboundedly. This is necessary for epoch rotation and receipt expiry.

## 4. Synchrony and Timing Model

Aura assumes partial synchrony. There exists a bound Δ on message delay and processing time once the network stabilizes. This bound is possibly unknown before stabilization occurs.

`T_fallback` in consensus is configured as 2 - 3 times Δ. Before stabilization, timeouts may be conservative. Gossip intervals target 250 to 500 milliseconds. Handlers must tolerate jitter but assume eventual delivery of periodic messages.

Epoch rotation relies on loosely synchronized clocks. The journal serves as the source of truth. Authorities treat an epoch change as effective once they observe it in the journal. This design avoids hard synchronization requirements.

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

A network adversary controls a subset of transport links. It can delay or drop packets but cannot break cryptography. It cannot forge receipts without `FlowGuard` grants. Receipts are protected by signatures and epoch binding.

The network adversary may simultaneously compromise up to `f < t` authorities per consensus instance without violating safety. Here `t` is the consensus threshold. This is the standard Byzantine fault tolerance guarantee.

### 6.2 Byzantine Witness

A Byzantine witness may equivocate during consensus. The system detects equivocation via the evidence CRDT. Types like `HasEquivocated` and `HasEquivocatedInSet` exclude conflicting shares from consensus. See [Consensus](104_consensus.md).

A Byzantine witness cannot cause multiple commits. Threshold signature verification rejects tampered results. The `t` of `t`-of-`n` threshold signatures prevents this attack.

### 6.3 Malicious Relay

A malicious relay may drop or delay envelopes. It cannot forge flow receipts because receipts require cryptographic signatures. It cannot read payloads because of context-specific encryption.

Downstream peers detect misbehavior via missing receipts or inconsistent budget charges. The transport layer detects relay failures automatically.

### 6.4 Device Compromise

A compromised device reveals its share and journal copy. It cannot reconstitute the account without meeting the branch policy. Recovery relies on relational contexts as described in [Relational Contexts](103_relational_contexts.md).

Device compromise is recoverable because the threshold prevents a single device from acting unilaterally. Guardians can revoke the compromised device and issue a new one.

## 7. Consistency Model

Journals eventually converge after replicas exchange all facts. This is eventual consistency. Authorities that have seen the same fact set arrive at identical states.

Operations guarded by Aura Consensus achieve strong consistency. Once a commit fact is accepted, all honest replicas agree on the result. See [Consensus](104_consensus.md).

Causal delivery is not enforced at the transport layer. Choreographies enforce ordering via session types instead. See [MPST and Choreography](107_mpst_and_choreography.md).

Each authority's view of its own journal is monotone. Once it observes a fact locally, it will never un-see it. This is monotonic read-after-write consistency.

## 8. Failure Handling

Timeouts trigger fallback consensus. See [Consensus](104_consensus.md) for `T_fallback` guidelines. Fallback consensus allows the system to make progress during temporary network instability.

Partition recovery relies on anti-entropy. Authorities merge fact sets when connectivity returns. The journal is the single source of truth for state.

Budget exhaustion causes local blocking. Protocols must implement backoff or wait for epoch rotation. Budgets are described in [Privacy and Information Flow](003_information_flow_contract.md).

Guard-chain failures return local errors. These errors include `AuthorizationDenied`, `InsufficientBudget`, and `JournalCommitFailed`. Protocol authors must handle these errors without leaking information. Proper error handling is critical for security.

## 9. Deployment Guidance

Configure witness sets such that `t > f`. Here `t` is the consensus threshold and `f` is the maximum number of Byzantine authorities tolerated. This is the standard Byzantine fault tolerance condition.

Tune gossip fanout and timeout parameters based on observed round-trip times and network topology. Conservative parameters ensure liveness under poor conditions.

Monitor receipt acceptance rates, consensus backlog, and budget utilization. See [Maintenance](111_maintenance.md) for monitoring guidance. Early detection of synchrony violations prevents cascading failures.

## 10. References

[System Architecture](001_system_architecture.md) describes runtime layering and the guard chain.

[Theoretical Model](002_theoretical_model.md) covers the formal calculus and semilattice laws.

[Accounts and Ratchet Tree](101_accounts_and_ratchet_tree.md) documents reduction ordering.

[Journal System](102_journal.md) and [Maintenance](111_maintenance.md) cover fact storage and convergence.

[Relational Contexts](103_relational_contexts.md) documents cross-authority state.

[Consensus](104_consensus.md) describes fast path and fallback consensus.

[Transport and Information Flow](108_transport_and_information_flow.md) documents transport semantics.

[Authorization](109_authorization.md) covers `CapGuard` and `FlowGuard` sequencing.

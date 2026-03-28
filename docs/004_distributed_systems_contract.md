# Distributed Systems Contract

This contract specifies Aura's distributed systems model. It defines the safety, liveness, and consistency guarantees provided by the architecture. It also documents the synchrony assumptions and adversarial capabilities the system tolerates.

This contract complements [Privacy and Information Flow Contract](003_information_flow_contract.md), which focuses on metadata and privacy budgets. Together these contracts define the full set of invariants protocol authors must respect.

Formal verification of these properties uses Quint model checking (`verification/quint/`) and Lean 4 theorem proofs (`verification/lean/`). See [Verification Coverage Report](998_verification_coverage.md) for current status.

## 1. Scope

The contract applies to the following aspects of the system.

Effect handlers and protocols operate within the 8-layer architecture described in [Aura System Architecture](001_system_architecture.md). Journals and reducers are covered by this contract. The journal specification appears in [Authority and Identity](102_authority_and_identity.md) and [Journal](105_journal.md). Aura Consensus is documented in [Consensus](108_consensus.md).

Relational contexts and rendezvous flows fall under this contract. Relational contexts are specified in [Relational Contexts](114_relational_contexts.md). Transport semantics appear in [Transport and Information Flow](111_transport_and_information_flow.md). Rendezvous flows are detailed in [Rendezvous Architecture](113_rendezvous.md).
Shared notation appears in [Theoretical Model](002_theoretical_model.md#shared-terms-and-notation).

### 1.1 Terminology Alignment

This contract uses shared terminology from [Theoretical Model](002_theoretical_model.md#shared-terms-and-notation).

- Consensus role terms: `witness` for consensus attestation, `signer` for FROST share holders
- Social-role terms: `Member`, `Participant`, `Moderator`
- Access terms: `AccessLevel` (`Full`, `Partial`, `Limited`)
- Topology terms: `1-hop` and `n-hop` paths

### 1.2 Contract Vocabulary

- `authoritative`: part of replicated truth and subject to convergence requirements
- `protocol object`: an execution object that supports transport or coordination without becoming replicated truth
- `runtime-local`: state owned by a local runtime and not treated as authoritative
- `accountability witness`: bounded evidence that a service action occurred
- `custody failure`: loss, eviction, or retrieval miss for a non-authoritative held object

### 1.3 Assumptions

- Cryptographic primitives remain secure at configured parameters.
- Partial synchrony eventually holds after GST, with bounded `Δ_net`.
- Honest participants execute the guard chain in the required order.
- Anti-entropy exchange eventually delivers missing facts to connected peers.

### 1.4 Non-goals

- This contract does not provide global linearizability across all operations.
- This contract does not guarantee progress during permanent partitions.
- This contract does not guarantee metadata secrecy without privacy controls defined in [Privacy and Information Flow Contract](003_information_flow_contract.md).
- This contract does not guarantee that runtime-local caches reflect authoritative truth at all times.
- This contract does not guarantee durable custody from `Hold` services.

### 1.5 Service Object Classes

Distributed behavior depends on three object classes with different contracts:

- authoritative shared objects
- transport and protocol objects
- runtime-derived local state

Only authoritative shared objects participate in replicated truth. Transport and protocol objects support execution. Runtime-derived local state remains non-authoritative and local to the runtime that owns it.

## 2. Safety Guarantees

### 2.1 Journal CRDT Properties

Facts merge via set union and never retract. Journals satisfy the semilattice laws:

- Commutativity: `merge(j1, j2) ≡ merge(j2, j1)`
- Associativity: `merge(merge(j1, j2), j3) ≡ merge(j1, merge(j2, j3))`
- Idempotence: `merge(j, j) ≡ j`

Reduction is deterministic. Identical fact sets produce identical states. No two facts share the same nonce within a namespace (`InvariantNonceUnique`).

### 2.2 Charge-Before-Send

Every transport observable is preceded by local authorization, accounting, and fact-coupling checks. No packet is emitted without a successful local decision.

Flow budgets satisfy monotonicity: charging never increases available budget (`monotonic_decrease`). Charging the exact remaining amount results in zero budget (`exact_charge`).

### 2.3 Consensus Agreement

For any pair `(cid, prestate_hash)` there is at most one commit fact (`InvariantUniqueCommitPerInstance`). Fallback gossip plus FROST signatures prevent divergent commits. Byzantine witnesses cannot force multiple commits for the same instance.

Commits require threshold participation (`InvariantCommitRequiresThreshold`). Equivocating witnesses are excluded from threshold calculations (`InvariantEquivocatorsExcluded`).

### 2.3.1 Fault Assumptions

Consensus safety depends on declared threshold and fault bounds.

Threshold signatures require the configured threshold of distinct valid shares. Safety requires fault assumptions that remain within the bound declared for the active ceremony. Different ceremonies may declare different admissible fault bounds.

### 2.4 Evidence CRDT

The evidence system tracks votes and equivocations as a grow-only CRDT:

- Monotonicity: Votes and equivocator sets only grow under merge
- Commit preservation: `merge` preserves existing commit facts
- Semilattice laws: Evidence merge is commutative, associative, and idempotent

### 2.5 Equivocation Detection

The system detects witnesses who vote for conflicting results:

- Soundness: Detection only reports actual equivocation (no false positives)
- Completeness: All equivocations are detectable given sufficient evidence
- Honest safety: Honest witnesses are never falsely accused

Types like `HasEquivocated` and `HasEquivocatedInSet` exclude conflicting shares from consensus. See [Consensus](108_consensus.md).

### 2.6 FROST Threshold Signatures

Threshold signatures satisfy binding and consistency properties:

- Share binding: Shares are cryptographically bound to `(consensus_id, result_id, prestate_hash)`
- Threshold requirement: Aggregation requires at least k shares from distinct signers
- Session consistency: All shares in an aggregation have the same session
- Determinism: Same shares always produce the same signature

### 2.7 Context Isolation

Messages scoped to `ContextId` never leak into other contexts. Contexts may be explicitly bridged through typed protocols only. See [Theoretical Model](002_theoretical_model.md). Each authority maintains separate journals per context to enforce this isolation.

### 2.8 Transport Layer

Beyond context isolation, transport satisfies:

- Flow budget non-negativity: Spent never exceeds limit (`InvariantFlowBudgetNonNegative`)
- Sequence monotonicity: Message sequence numbers strictly increase (`InvariantSequenceMonotonic`)
- Fact backing: Every sent message has a corresponding journal fact (`InvariantSentMessagesHaveFacts`)

### 2.9 Deterministic Reduction Order

Commitment tree operations resolve conflicts using the stable ordering described in [Authority and Identity](102_authority_and_identity.md). This ordering is derived from the cryptographic identifiers and facts stored in the journal. Conflicts are always resolved in the same way across all replicas.

### 2.10 Receipt Chain

Multi-hop forwarding requires signed receipts. Downstream peers reject messages lacking a chain rooted in their relational context. See [Transport and Information Flow](111_transport_and_information_flow.md). This prevents unauthorized message propagation.

### 2.11 Onion Accountability Verification

Onion-routed accountability must preserve anonymous reverse delivery of bounded witnesses.

Verifier roles are explicit and local. Local runtime consequences such as scoring, reciprocal budget, and admission preference apply only after verification succeeds.

### 2.12 Hold Service Profiles

`Hold` is a shared custody service surface. Profile-specific retrieval or retention semantics are allowed.

All `Hold` services must preserve the common custody invariants. Custody remains opaque, non-authoritative, selector-driven, and best-effort.

## 3. Protocol-Specific Guarantees

### 3.1 DKG and Resharing

Distributed key generation and resharing satisfy:

- Threshold bounds: `1 ≤ t ≤ n` where t is threshold and n is participant count
- Phase consistency: Commitment counts match protocol phase
- Share timing: Shares distributed only after commitment verification

### 3.2 Invitation Flows

Invitation lifecycle satisfies authorization invariants:

- Sender authority: Only sender can cancel an invitation
- Receiver authority: Only receiver can accept or decline
- Single resolution: No invitation resolved twice
- Terminal immutability: Terminal status (accepted/declined/cancelled/expired) is permanent
- Fact backing: Accepted invitations have corresponding journal facts
- Ceremony gating: Ceremonies only initiated for accepted invitations

### 3.3 Epoch Validity

Epochs enforce temporal boundaries:

- Receipt validity window: Receipts only valid within their epoch
- Replay prevention: Old epoch receipts cannot be replayed in new epochs

### 3.4 Cross-Protocol Safety

Concurrent protocol execution (e.g., Recovery∥Consensus) satisfies:

- No deadlock: Interleaved execution always makes progress
- Revocation enforcement: Revoked devices excluded from all protocols

## 4. Liveness Guarantees

### 4.1 Fast-Path Consensus

Fast-path consensus completes within bounded delay when the fast-path witness and network assumptions hold.

### 4.2 Fallback Consensus

Fallback consensus eventually completes under partial synchrony when the fallback quorum and delivery assumptions hold.

### 4.3 Anti-Entropy

Journals converge under eventual delivery. CRDT merges reconcile fact sets even after partitions.

### 4.4 Rendezvous

Offer and answer envelopes flood gossip neighborhoods. Secure channels can be established as long as at least one bidirectional path remains between parties. See [Rendezvous Architecture](113_rendezvous.md).

### 4.5 Flow Budgets

Flow-budget progress is conditional.
If future epoch state restores positive headroom and epoch updates converge, local budget enforcement eventually grants headroom.
Budget exhaustion remains temporary only under these assumptions.

Liveness requires that each authority eventually receives messages from its immediate neighbors. This is the eventual delivery assumption. Liveness also requires that clocks do not drift unboundedly. This is necessary for epoch rotation and receipt expiry.

### 4.6 Hold Availability

`Hold` availability is neighborhood-scoped and selector-driven. It is not a guarantee that any specific holder remains available.

Liveness for `Hold` requires that the runtime can find some admissible holder within the neighborhood-scoped provider set. Retrieval miss and re-deposit are expected recovery behaviors. They are not contract violations by themselves.

## 5. Time System

Aura uses a unified `TimeStamp` with domain-specific comparison:

- Reflexivity: `compare(policy, t, t) = eq`
- Transitivity: `compare(policy, a, b) = lt ∧ compare(policy, b, c) = lt → compare(policy, a, c) = lt`
- Privacy: Physical time hidden when `ignorePhysical = true`

Time variants include `PhysicalClock` (wall time), `LogicalClock` (vector/Lamport), `OrderClock` (opaque ordering tokens), and `Range` (validity windows).

## 6. Synchrony and Timing Model

Aura assumes partial synchrony. There exists a bound `Δ_net` on message delay and processing time once the network stabilizes. This bound may be unknown before stabilization occurs.

Before stabilization, progress may stall. After stabilization, protocols that depend on eventual delivery and bounded delay may resume progress.

Epoch rotation relies on loosely synchronized clocks. The journal remains the source of truth for observed epoch state.

## 7. Adversarial Model

### 7.1 Network Adversary

A network adversary may delay or drop traffic and may control a subset of links.

- Must not: break cryptography or forge valid accountability material without the required local authorization and signatures
- Contract boundary: safety holds only within the declared fault bounds for the active ceremony

### 7.2 Byzantine Witness

A Byzantine witness may equivocate or refuse to participate.

- Must not: cause multiple commits for the same consensus instance while the declared fault assumptions hold
- Contract boundary: equivocation is detectable and excluded from valid threshold outcomes

### 7.3 Malicious Relay

A malicious relay may drop, delay, or refuse to forward envelopes.

- Must not: read protected payload content or forge valid forwarding accountability
- Contract boundary: relay failure may reduce liveness but must not violate transport safety or accountability rules

### 7.4 Malicious Hold Provider

A malicious hold provider may evict, refuse retrieval, or under-serve after accepting custody.

- Must not: turn custody objects into authoritative state or obtain legitimate service credit without successful witness verification
- Contract boundary: custody failure affects availability, not authoritative truth

### 7.5 Device Compromise

A compromised device may reveal its local share and journal copy.

- Must not: satisfy threshold requirements on its own or rewrite authoritative history unilaterally
- Contract boundary: device compromise is recoverable if the remaining threshold and recovery assumptions still hold

## 8. Consistency Model

| Surface | Required Guarantee | Explicit Non-guarantee |
|---------|--------------------|------------------------|
| Journal | Replicas that observe the same fact set converge to the same state | Immediate global agreement |
| Consensus-scoped operation | Honest replicas agree on the committed result for that operation | A single global linearizable log |
| Transport | Transport safety does not depend on causal delivery ordering | Global transport-level causal order |
| Local replica view | Locally observed facts remain monotone | Instant reflection of all remote facts |

## 9. Failure Handling

Failure classes are distinct:

- authoritative state failure
- custody failure
- runtime-local cache or selection failure

Allowed failure:
- temporary loss of progress during partition or instability
- custody miss, eviction, or re-deposit
- runtime-local cache invalidation or reselection

Forbidden failure:
- divergent authoritative truth for the same committed operation
- treating custody state as authoritative replicated truth
- exposing internal failure causes through remote error detail

Local-only failure:
- authorization failure
- budget exhaustion
- runtime-local cache or selection failure

### 9.1 Error-Channel Privacy Requirements

- Runtime errors must use bounded enums and redacted payloads.
- Error paths must not include plaintext message content, raw capability tokens, or cross-context identifiers.
- Remote peers may observe protocol-level status outcomes only, not internal guard-stage diagnostics.

## 10. References

[Aura System Architecture](001_system_architecture.md) describes runtime layering.

[Authorization](106_authorization.md) describes authorization and budgeting ordering.

[Theoretical Model](002_theoretical_model.md) covers the formal calculus and semilattice laws.

[Authority and Identity](102_authority_and_identity.md) documents reduction ordering.

[Journal](105_journal.md) and [Distributed Maintenance Architecture](116_maintenance.md) cover fact storage and convergence.

[Relational Contexts](114_relational_contexts.md) documents cross-authority state.

[Consensus](108_consensus.md) describes fast path and fallback consensus.

[Transport and Information Flow](111_transport_and_information_flow.md) documents transport semantics.

[Authorization](106_authorization.md) covers authorization and budgeting sequencing.

[Verification Coverage Report](998_verification_coverage.md) tracks formal verification status.

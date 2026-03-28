# Privacy and Information Flow Contract

This contract specifies Aura's privacy and information-flow model. It defines privacy boundaries, leakage budgets, and enforcement mechanisms. Privacy boundaries align with social relationships rather than technical perimeters.

Violations occur when information crosses trust boundaries without consent. Acceptable flows consume explicitly budgeted headroom.

This document complements [Distributed Systems Contract](004_distributed_systems_contract.md), which covers safety, liveness, and consistency. Together these contracts define the full set of invariants protocol authors must respect.

Formal verification of these properties uses Quint model checking (`verification/quint/`) and Lean 4 theorem proofs (`verification/lean/`). See [Verification Coverage Report](998_verification_coverage.md) for current status.

## 1. Scope

The contract applies to information flows across privacy boundaries:

- Flow budgets: Per-context per-peer spending limits enforced by FlowGuard
- Leakage tracking: Metadata exposure accounting by observer class
- Context isolation: Separation of identities and journals across contexts
- Receipt chains: Multi-hop forwarding accountability
- Epoch boundaries: Temporal isolation of budget and receipt state
- Service families: `Establish`, `Move`, and `Hold` as the privacy-relevant service surfaces
- Selector retrieval: Capability-derived retrieval without identity-addressed mailbox polling
- Hold custody: Neighborhood-scoped opaque retention with bounded retrieval authority

Related specifications: [Authorization](106_authorization.md), [Transport and Information Flow](111_transport_and_information_flow.md), and [Theoretical Model](002_theoretical_model.md).
Shared notation appears in [Theoretical Model](002_theoretical_model.md#shared-terms-and-notation).

### 1.1 Terminology Alignment

This contract uses shared terminology from [Theoretical Model](002_theoretical_model.md#shared-terms-and-notation).

- Home role terms: `Member`, `Participant`, `Moderator` (only members can be moderators)
- Access-level terms: `Full`, `Partial`, `Limited`
- Storage terms: `Shared Storage` and `allocation`
- Pinning term: `pinned` as a fact attribute

### 1.2 Assumptions

- Cryptographic primitives are secure at configured key sizes.
- Local runtimes enforce guard-chain ordering before transport sends.
- Epoch updates and budget facts eventually propagate through anti-entropy.
- The service-family model is always active after migration.
- Privacy-mode deployments use encrypted envelopes and the fixed adaptive policy.
- Debug or simulation modes such as `transparent_onion` are excluded from production privacy claims.

### 1.3 Non-goals

- This contract does not guarantee traffic-analysis resistance against global passive adversaries without encrypted envelopes and sufficiently regular cover behavior.
- This contract does not define social policy decisions such as who should trust whom.
- This contract does not treat `Hold` custody as authoritative durable storage.

## 2. Privacy Philosophy

Traditional privacy systems offer only complete isolation or complete exposure. Aura treats privacy as relational. Sharing information with trusted parties is a consented disclosure, not a privacy violation.

### 2.1 Core Principles

- Consensual disclosure: Joining a group or establishing a relationship implies consent to share coordination information
- Contextual identity: Deterministic Key Derivation presents different identities in different contexts, and only relationship parties can link them
- Neighborhood visibility: Gossip neighbors observe encrypted envelope metadata, bounded by flow budgets and context isolation

### 2.2 Privacy Layers

| Layer | Protection | Mechanism |
|-------|------------|-----------|
| Identity | Context-specific keys | DKD: `derive(root, app_id, context_label)` |
| Relationship | Graph opacity | No central directory, out-of-band establishment |
| Group | Membership hiding | Threshold operations, group-scoped identity |
| Content | End-to-end encryption | AES-256-GCM, HPKE, per-message keys |
| Metadata | Rate, volume, and path-shape bounds | Flow budgets, fixed-size envelopes, batching, encrypted routing |
| Retrieval | Non-semantic access intent | Capability-derived selectors, sync-blended retrieval |
| Custody | Opaque retained-object handling | `Hold` substrate, bounded capabilities, uniform retention |

Verified by: `Aura.Proofs.KeyDerivation`, `authorization.qnt`

## 3. Flow Budget System

### 3.1 Budget Structure

For each context and peer pair, the journal records charge facts that contribute to a flow budget:

```
FlowBudget {
    limit: u64,   // derived at runtime from Biscuit + policy
    spent: u64,   // replicated fact (merge = max)
    epoch: Epoch, // replicated fact
}
```

Only `spent` and `epoch` appear as journal facts. The `limit` is computed at runtime by intersecting Biscuit-derived capabilities with sovereign policy.

### 3.2 Limit Computation

The limit for a context and peer is computed as:

```
limit(ctx, peer) = base(ctx) ⊓ policy(ctx) ⊓ role(ctx, peer)
                   ⊓ relay_factor(ctx) ⊓ peer_health(peer)
```

Each term is a lattice element. Merges occur via meet (⊓), ensuring convergence and preventing widening.

| Term | Source | Purpose |
|------|--------|---------|
| `base(ctx)` | Context class | Default headroom |
| `policy(ctx)` | Sovereign settings | Account-level limits |
| `role(ctx, peer)` | Biscuit token | Per-peer role attenuation |
| `relay_factor(ctx)` | Network topology | Hub amplification mitigation |
| `peer_health(peer)` | Liveness monitoring | Overload protection |

### 3.3 Charge-Before-Send

Every transport observable is preceded by guard evaluation:

1. CapGuard: Verify Biscuit authorization
2. FlowGuard: Charge `cost` to `(context, peer)` budget
3. JournalCoupler: Atomically commit charge fact and protocol deltas
4. Transport: Emit packet only after successful charge

If `spent + cost > limit`, the send is blocked locally with no observable behavior.

Invariants:
- `spent ≤ limit` at all times (`InvariantFlowBudgetNonNegative`)
- Charging never increases available budget (`monotonic_decrease`)
- Guard chain order is fixed (`guardChainOrder`)
- Attenuation only narrows, never widens (`attenuationOnlyNarrows`)

Verified by: `Aura.Proofs.FlowBudget`, `authorization.qnt`, `transport.qnt`

### 3.4 Multi-Hop Enforcement

For forwarding, each hop independently executes the guard chain:

- Relay validates upstream receipt before forwarding
- Relay charges its own budget before emitting
- Receipt facts are scoped to `(context, epoch)` with chained hashes
- Downstream peers can audit budget usage via receipt chain

Because `spent` is monotone (merge = max), convergence holds even if later hops fail.

Verified by: `transport.qnt` (`InvariantSentMessagesHaveFacts`)

### 3.5 Receipts and Epochs

Per-hop receipts are required for forwarding and bound to the epoch:

```
Receipt { ctx, src, dst, epoch, cost, nonce, prev_hash, sig }
```

This schema is the canonical receipt shape used across core contracts.

- Acceptance window: Current epoch only
- Rotation trigger: Journal fact `Epoch(ctx)` increments
- On rotation: `spent(ctx, *)` resets, and old receipts become invalid

Invariants:
- Receipts only valid within their epoch (`InvariantReceiptValidityWindow`)
- Old epoch receipts cannot be replayed (`InvariantCrossEpochReplayPrevention`)

Verified by: `epochs.qnt`

## 4. Leakage Tracking

### 4.1 Observer Classes

Information leakage is tracked per observer class:

| Class | Visibility | Budget Scope |
|-------|------------|--------------|
| `Relationship` | Full context content | Consensual, no budget |
| `Group` | Group-scoped content | Group dimension |
| `Neighbor` | Encrypted envelope metadata | Per-hop budget |
| `Custody` | Opaque held objects, selectors, and retention behavior | Per-holder budget and leakage policy |
| `External` | Network-level patterns | Encrypted Aura onion routing plus cover traffic |

### 4.2 Leakage Budget

Each observer class has a leakage budget separate from flow budgets:

```
LeakageBudget {
    observer: DeviceId,
    leakage_type: LeakageType,  // Metadata, Timing, Participation
    limit: u64,
    spent: u64,
    refresh_interval: Duration,
}
```

Leakage is charged before any operation that exposes information to the observer class.

`Custody` observers are special. They may see that an opaque object is deposited, retained, expired, or retrieved. They must not learn mailbox identity or depositor identity from that flow. Selector design, uniform retention, and reply-path accountability all constrain this observer class.

### 4.3 Policy Modes

| Policy | Behavior | Use Case |
|--------|----------|----------|
| `Deny` | Reject if no explicit budget | Production (secure default) |
| `DefaultBudget(n)` | Fall back to n units | Transition period |
| `LegacyPermissive` | Allow unlimited | Migration only |

Verified by: `Aura.Proofs.ContextIsolation`

## 5. Privacy Boundaries

### 5.1 Relationship Boundary

Within a direct relationship, both parties have consented to share coordination information:

- Visible: Context-specific identity, online status, message content
- Hidden: Activity in other contexts, identity linkage across contexts
- Enforcement: DKD ensures unique identity per context

### 5.2 Neighborhood Boundary

Gossip neighbors forward encrypted traffic:

- Visible: Envelope size (fixed), rotating rtags, timing patterns
- Hidden: Content, ultimate sender/receiver, rtag-to-identity mapping
- Enforcement: Flow budgets, onion routing, cover traffic

### 5.3 Group Boundary

Group participants share group-scoped information:

- Visible: Member identities (within group), group content, threshold operations
- Hidden: Member identities outside group, other group memberships
- Enforcement: Group-specific DKD identity, k-anonymity for sensitive operations

### 5.4 External Boundary

External observers have no relationship with you:

- In privacy mode: Encrypted Aura onion traffic and timing patterns are visible
- In passthrough mode: Connections to Aura nodes are visible
- Enforcement: Fixed-size envelopes, no central directory, encrypted envelopes in privacy mode, and flow budgets

### 5.5 Retrieval Boundary

Retrieval is not identity-addressed at the network boundary.

- Visible to intermediaries: selector traffic shape and reply-path usage
- Hidden from intermediaries: mailbox identity, semantic object meaning, and direct reverse identity
- Enforcement: retrieval capabilities, capability-derived selectors, sync-blended retrieval, and typed single-use reply blocks

### 5.6 Custody Boundary

`Hold` providers operate on opaque custody objects rather than authoritative truth.

- Visible to the holder: bounded retention requests, opaque held objects, selector usage, and storage pressure
- Hidden from the holder under onion routing: specific depositor identity and mailbox identity
- Enforcement: neighborhood-scoped provider set, uniform retention policy, bounded rotating holder subsets, and best-effort custody semantics

## 6. Time Domain Semantics

Time handling affects privacy through leakage:

| Variant | Purpose | Privacy Impact |
|---------|---------|----------------|
| `PhysicalClock` | Guard charging, receipts, cooldowns | Leaks wall-clock via receipts |
| `LogicalClock` | CRDT causality, journal ordering | No wall-clock leakage |
| `OrderClock` | Privacy-preserving total order | Opaque tokens (no temporal meaning) |
| `Range` | Validity windows, disputes | Bounded uncertainty from physical |

- Cross-domain comparisons require explicit `TimeStamp::compare(policy)`
- Physical time obtained only through `PhysicalTimeEffects` (never direct `SystemTime::now()`)
- Privacy mode (`ignorePhysical = true`) hides physical timestamps

Verified by: `Aura.Proofs.TimeSystem`

## 7. Adversarial Model

### 7.1 Direct Relationship

A party in a direct relationship sees everything within that context by consent.

- Cannot: Link identity across contexts, access undisclosed contexts
- Attack vector: Social engineering, context correlation
- Mitigation: UI clearly indicates active context

### 7.2 Group Insider

A group member sees all group activity by consent.

- Cannot: Determine member identities outside group, access other groups
- Attack vector: Threshold signing timing correlation
- Mitigation: k-anonymity, random delays in signing rounds

### 7.3 Gossip Neighbor

Devices forwarding your traffic observe encrypted metadata.

- Cannot: Decrypt content, identify ultimate sender/receiver, link rtags to identities
- Attack vector: Traffic correlation through sustained observation
- Mitigation: Onion routing, cover traffic, batching, rtag rotation

### 7.4 Network Observer

An ISP-level adversary sees IP connections and packet timing.

- In privacy mode: Connections to Aura nodes and encrypted onion traffic shape are visible
- In passthrough mode: Connections to known Aura nodes are visible
- Attack vector: Confirmation attacks, traffic correlation
- Mitigation: Encrypted envelopes, fixed-size envelopes, and cover traffic

### 7.5 Compromised Device

A single compromised device reveals its key share and synced journal state.

- Cannot: Perform threshold operations alone, derive account root key
- Attack vector: Compromise M-of-N devices for full control
- Mitigation: Threshold cryptography, device revocation via resharing, epoch invalidation

## 8. Privacy Metrics

| Metric | Target | Measurement |
|--------|--------|-------------|
| Identity linkability | < 5% confidence | `identity_linkability_score(ctx_a, ctx_b)` |
| Relationship inference (neighbor) | < 10% confidence | `relationship_inference_confidence` |
| Relationship inference (external) | < 1% confidence | `relationship_inference_confidence` |
| Group membership inference | ≤ 1/k (k-anonymity) | `group_membership_inference` |
| Timing entropy | > 4 bits | `H(actual_send_time \| observed_traffic)` |
| Activity detection | ± 10% of base rate | `P(user_active \| traffic)` |

Tests instantiate adversary observers and measure inference confidence against these bounds.

## 9. Cover Traffic

Cover traffic is part of privacy-mode deployment. It is not required for passthrough availability deployment:

- Adaptive: Matches real usage patterns (e.g., 20 messages/hour during work hours)
- Group-leveraged: Groups naturally provide steady traffic rates
- Scheduled slots: Real messages inserted into fixed intervals
- Indistinguishable: Only recipient can distinguish real from cover by decryption attempt

Target: `P(real | observed) ≈ 0.5`

Privacy claims for global or strong partial observers rely on encrypted envelopes plus sufficiently regular cover behavior. Passthrough deployment does not claim those stronger bounds.

## 10. Hub Node Mitigation

Hub nodes with high connectivity observe metadata for many relationships:

| Mitigation | Mechanism |
|------------|-----------|
| Route selection | Minimize fraction observed by any single node |
| Hub tracking | System identifies high-degree nodes |
| Privacy routing | Users can avoid hubs at cost of longer routes |
| Per-hop budgets | Bound forwarding rate per context |
| Decoy envelopes | Optional dummy traffic |

## 11. Implementation Requirements

### 11.1 Key Derivation

- Use HKDF with domain separation
- Path: `(account_root, app_id, context_label, "aura.key.derive.v1")`
- Never reuse keys across contexts

Verified by: `Aura.Proofs.KeyDerivation`

### 11.2 Envelope Format

- Fixed-size with random padding
- Encrypted and authenticated
- Onion-routed through multiple hops
- Rtags rotate on negotiated schedule
- Accountability callbacks use typed single-use reply blocks

Typed single-use reply blocks carry accountability witnesses back through anonymous reply paths. They are pre-committed return capabilities. They are not open-ended reverse channels.

### 11.3 Guard Chain

- All transport calls pass through FlowGuard
- Charge failure branches locally with no packet emitted
- Multi-hop forwarding attaches and validates per-hop receipts

Verified by: `authorization.qnt` (`chargeBeforeSend`, `spentWithinLimit`)

### 11.4 Retrieval and Custody Requirements

- Parity-critical retrieval must use capability-derived selectors
- Parity-critical retrieval must not use identity-addressed mailbox polling
- `Hold` custody must remain opaque and non-authoritative
- Uniform retention policy must not vary by social distance
- Applications that require guaranteed durability must use authoritative replicated state rather than `Hold`

### 11.5 Accountability Verification Requirements

- Adjacent peers verify hop-local `Move` witnesses
- Retrievers, holders, or bounded auditors verify `Hold` witnesses relevant to deposit, retrieval, or custody checks
- Local runtime scoring, reciprocal budget, and admission effects apply only after witness verification succeeds
- Raw accountability proof traffic must not become a global visibility layer

### 11.6 Secure Storage

- Use platform secure storage (Keychain, Secret Service, Keystore)
- Never store keys in plaintext files
- Audit logs for security-critical operations in journal

### 11.7 Error-Channel Privacy Requirements

- Guard failures must return bounded, typed errors only.
- Error payloads must not include raw context payload, peer identity material, or decrypted content.
- Remote peers must not infer whether failure came from capability checks, budget checks, or local storage faults beyond allowed protocol-level status codes.

## 12. Verification Coverage

This contract's guarantees are formally verified:
Canonical invariant names are indexed in [Project Structure](999_project_structure.md#invariant-traceability).
Canonical proof and coverage status are indexed in [Verification Coverage Report](998_verification_coverage.md).

| Property | Tool | Location |
|----------|------|----------|
| Flow budget monotonicity | Lean | `Aura.Proofs.FlowBudget` |
| Key derivation isolation | Lean | `Aura.Proofs.KeyDerivation` |
| Context isolation | Lean | `Aura.Proofs.ContextIsolation` |
| Guard chain ordering | Quint | `authorization.qnt` |
| Budget invariants | Quint | `authorization.qnt`, `transport.qnt` |
| Epoch validity | Quint | `epochs.qnt` |

See [Verification Coverage Report](998_verification_coverage.md) for metrics and [Aura Formal Verification](../verification/README.md) for the Quint-Lean correspondence map.

## 13. References

[Distributed Systems Contract](004_distributed_systems_contract.md) covers safety, liveness, and consistency.

[Theoretical Model](002_theoretical_model.md) covers the formal calculus and semilattice laws.

[Aura System Architecture](001_system_architecture.md) describes runtime layering and the guard chain.

[Authorization](106_authorization.md) covers CapGuard, FlowGuard, and Biscuit integration.

[Transport and Information Flow](111_transport_and_information_flow.md) documents transport semantics and receipts.

[Relational Contexts](114_relational_contexts.md) documents cross-authority state and context isolation.

[Verification Coverage Report](998_verification_coverage.md) tracks formal verification status.

## 14. Implementation References

| Component | Location |
|-----------|----------|
| Guard chain | `crates/aura-guards/src/guards/` |
| Flow budget | `crates/aura-protocol/src/flow_budget/` |
| Context isolation | `crates/aura-relational/src/privacy/` |
| Privacy testing | `crates/aura-testkit/src/privacy/` |
| Transport patterns | `crates/aura-transport/` |

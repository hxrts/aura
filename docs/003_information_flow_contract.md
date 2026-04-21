# Privacy and Information Flow Contract

This contract specifies Aura's privacy and information-flow model. It defines privacy boundaries, leakage budgets, and required privacy properties. Privacy boundaries align with social relationships rather than technical perimeters.

Violations occur when information crosses trust boundaries without consent. Acceptable flows consume explicitly budgeted headroom.

This document complements [Distributed Systems Contract](004_distributed_systems_contract.md), which covers safety, liveness, and consistency. Together these contracts define the full set of invariants protocol authors must respect.

Verification of these properties uses Quint model checking (`verification/quint/`) and Lean 4 theorem proofs (`verification/lean/`). See [Verification Coverage Report](998_verification_coverage.md) for current status.

## 1. Scope

The contract applies to information flows across privacy boundaries:

- Flow budgets: Per-context per-peer spending limits enforced before transport
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

### 1.2 Contract Vocabulary

- `observer`: any party that can learn information from traffic, custody, or local state exposure
- `authoritative`: part of replicated truth rather than local runtime interpretation
- `retrieval`: recovery of an object through bounded authority rather than identity-addressed delivery
- `custody`: opaque best-effort retention of a non-authoritative object
- `accountability evidence`: bounded evidence used to verify that a service action occurred

### 1.3 Assumptions

- Cryptographic primitives are secure at configured key sizes.
- Local runtimes enforce guard-chain ordering before transport sends.
- Epoch updates and budget facts eventually propagate through anti-entropy.
- The service-family model is part of the active privacy contract.
- Privacy-mode deployments use encrypted envelopes and the fixed adaptive policy.
- Debug and simulation modes are excluded from production privacy claims.

### 1.4 Non-goals

- This contract does not guarantee traffic-analysis resistance against global passive adversaries without encrypted envelopes and sufficiently regular cover behavior.
- This contract does not define social policy decisions such as who should trust whom.
- This contract does not treat `Hold` custody as authoritative durable storage.
- This contract does not guarantee durable delivery from custody services.
- This contract does not guarantee that debug or simulation modes preserve production privacy properties.

## 2. Privacy Philosophy

Traditional privacy systems offer only complete isolation or complete exposure. Aura treats privacy as relational. Sharing information with trusted parties is a consented disclosure, not a privacy violation.

### 2.1 Core Principles

- Consensual disclosure: Joining a group or establishing a relationship implies consent to share coordination information
- Contextual identity: Deterministic Key Derivation presents different identities in different contexts, and only relationship parties can link them
- Neighborhood visibility: Gossip neighbors observe encrypted envelope metadata, bounded by flow budgets and context isolation
- Service trust is social: social planes may admit or weight providers, but provider trust must not become visible service shape
- Communication privacy is envelope-level: descriptors, routes, retrieval, and retention behavior must remain socially neutral at the network boundary

### 2.2 Privacy Layers

| Boundary | Required Property | Forbidden Outcome |
|----------|-------------------|-------------------|
| Identity | Contexts use distinct identity material | Cross-context identity reuse |
| Relationship | Relationship discovery stays decentralized | Global directory disclosure |
| Group | Group participation remains group-scoped | Cross-group membership disclosure |
| Content | Unauthorized observers cannot learn plaintext | Plaintext disclosure outside consented boundaries |
| Metadata | Exposure stays budgeted by observer class | Unbounded metadata leakage |
| Retrieval | Parity-critical retrieval is not identity-addressed | Mailbox-identity disclosure on retrieval paths |
| Custody | Custody remains opaque and non-authoritative | Treating custody as replicated truth |

### 2.3 Service-Family Boundary

`Establish`, `Move`, and `Hold` are the privacy-relevant service families. They describe service behavior, not social role. A provider may be admitted because of neighborhood membership, direct friendship, bounded introduction evidence, or descriptor fallback, but the service interface must not reveal which reason dominated.

Trust evidence may affect `Permit` and runtime-local weighting. It must not appear as route shape, descriptor kind, retrieval shape, retention tier, or wire-visible policy class. Coarse selection tiers are local runtime derivations and are not canonical shared state.

## 3. Budgeted Send Invariant

Transport observables require prior local authorization, accounting, and fact-coupling.

Budget state is monotone within its active epoch. Over-budget sends must remain local. Receipt validity is epoch-scoped and old receipts must not be replayable in new epochs.

Forwarding is hop-local. Each hop must validate the required upstream accountability state before emitting downstream transport.

## 4. Leakage Tracking

### 4.1 Observer Classes

Information leakage is tracked per observer class:

| Observer | May Observe | Must Not Learn |
|----------|-------------|----------------|
| `Relationship` | Full context content by consent | Undisclosed contexts |
| `Group` | Group-scoped content | Other group memberships by default |
| `Neighbor` | Encrypted envelope metadata | Plaintext content and endpoint identity |
| `Custody` | Opaque held objects, selectors, and retention behavior | Depositor identity and mailbox identity |
| `External` | Network-level patterns | Protected content and protected endpoint identity |

### 4.2 Leakage Budget

Each observer class has a leakage budget separate from flow budgets.

Leakage is charged before any operation that exposes information to the observer class.

`Custody` observers are special. They may see that an opaque object is deposited, retained, expired, or retrieved. They must not learn mailbox identity or depositor identity from that flow.

### 4.3 Policy Modes

| Policy | Required Behavior | Allowed Exposure |
|--------|-------------------|------------------|
| `Deny` | Reject unbudgeted exposure | None |
| `DefaultBudget(n)` | Apply bounded default headroom | Up to `n` units |
| `LegacyPermissive` | Allow unbounded exposure | Migration-only exception |

## 5. Privacy Boundaries

### 5.1 Relationship Boundary

Within a direct relationship, both parties have consented to share coordination information:

- Visible: Context-specific identity, online status, message content
- Hidden: Activity in other contexts, identity linkage across contexts
- Violation: cross-context identity reuse or disclosure of undisclosed contexts

### 5.2 Neighborhood Boundary

Gossip neighbors forward encrypted traffic:

- Visible: Envelope size (fixed), rotating rtags, timing patterns
- Hidden: Content, ultimate sender/receiver, rtag-to-identity mapping
- Violation: disclosure of plaintext content or protected endpoint identity

### 5.3 Group Boundary

Group participants share group-scoped information:

- Visible: Member identities (within group), group content, threshold operations
- Hidden: Member identities outside group, other group memberships
- Violation: disclosure of unrelated group membership through group participation

### 5.4 External Boundary

External observers have no relationship with you:

- In privacy mode: protected Aura traffic patterns and timing are visible
- In passthrough mode: direct Aura connectivity is visible
- Violation: treating basic availability deployment as a stronger privacy claim

### 5.5 Retrieval Boundary

Retrieval is not identity-addressed at the network boundary.

- Visible to intermediaries: selector traffic shape and reply-path usage
- Hidden from intermediaries: mailbox identity, semantic object meaning, and direct reverse identity
- Violation: identity-addressed retrieval on parity-critical paths

### 5.6 Custody Boundary

`Hold` providers operate on opaque custody objects rather than authoritative truth.

- Visible to the holder: bounded retention requests, opaque held objects, selector usage, and storage pressure
- Hidden from the holder under onion routing: specific depositor identity and mailbox identity
- Violation: treating custody state as authoritative truth or varying retention by social distance

## 6. Time Domain Semantics

Time handling affects privacy through leakage:

| Variant | Purpose | Privacy Impact |
|---------|---------|----------------|
| `PhysicalClock` | Guard charging, receipts, cooldowns | Leaks wall-clock via receipts |
| `LogicalClock` | CRDT causality, journal ordering | No wall-clock leakage |
| `OrderClock` | Privacy-preserving total order | Opaque tokens (no temporal meaning) |
| `Range` | Validity windows, disputes | Bounded uncertainty from physical |

- Cross-domain time comparison must be explicit.
- Privacy-preserving flows must not expose physical time unless that exposure is part of the contract.

## 7. Adversarial Model

### 7.1 Direct Relationship

A party in a direct relationship may observe the full contents of that relationship context by consent.

- Must not: learn undisclosed contexts or link identity across contexts
- Contract boundary: relationship consent does not widen to other contexts

### 7.2 Group Insider

A group member may observe group-scoped activity by consent.

- Must not: learn other group memberships or unrelated identities
- Contract boundary: group visibility remains group-scoped

### 7.3 Gossip Neighbor

Devices forwarding traffic may observe protected metadata.

- Must not: decrypt content, identify protected endpoints, or map routing tags to identity
- Contract boundary: neighbor visibility remains budgeted metadata only

### 7.4 Network Observer

A network observer may observe connectivity and timing patterns.

- Must not: gain stronger privacy guarantees than the active deployment mode provides
- Contract boundary: privacy claims vary with the active protection mode

### 7.5 Compromised Device

A compromised device may reveal its local share and replicated state.

- Must not: unilaterally satisfy threshold requirements or derive protected root material
- Contract boundary: single-device compromise does not imply full authority compromise

## 8. External Observer Limits

Stronger privacy claims against external observers require sufficiently regular protected network behavior.

Basic availability deployments do not claim those stronger bounds. Routing and budgeting must also limit metadata concentration at any single relay or hub.

## 9. Required Properties

### 9.1 Identity and Key Separation

- Contexts must not share reusable identity material.
- Key derivation must remain domain-separated.
- Keys must not be reused across contexts.

### 9.2 Transport Privacy

- Protected transport must use authenticated encrypted envelopes.
- Transport observables must remain budgeted by observer class.
- Accountability return paths must not require direct reverse identity.

### 9.3 Send Authorization

- No transport observable may occur without prior local authorization and accounting.
- Failed authorization or charging must remain local.
- Forwarding must validate the required receipt or accountability state before onward transport.

### 9.4 Retrieval and Custody

- Parity-critical retrieval must use bounded retrieval authority.
- Parity-critical retrieval must not use identity-addressed mailbox polling.
- `Hold` custody must remain opaque and non-authoritative.
- Uniform retention policy must not vary by social distance.
- Applications that require guaranteed durability must use authoritative replicated state rather than `Hold`.

### 9.5 Accountability and Local Consequences

- Accountability evidence must be verified by the relevant local verifier before any local consequence is applied.
- Local scoring, reciprocal budget, and admission effects apply only after successful verification.
- Accountability traffic must not become a new global visibility layer.

### 9.6 Secret Material and Error Channels

- Secret material must not be stored in plaintext.
- Guard failures must return bounded, typed errors only.
- Error payloads must not include raw context payload, peer identity material, or decrypted content.
- Remote peers must not infer internal failure causes beyond allowed protocol-level status codes.

## 10. References

[Distributed Systems Contract](004_distributed_systems_contract.md) covers safety, liveness, and consistency.

[Theoretical Model](002_theoretical_model.md) covers the formal calculus and semilattice laws.

[System Architecture](001_system_architecture.md) describes runtime layering and the guard chain.

[Authorization](106_authorization.md) covers authorization, budgeting, and Biscuit integration.

[Transport and Information Flow](111_transport_and_information_flow.md) documents transport semantics and receipts.

[Relational Contexts](114_relational_contexts.md) documents cross-authority state and context isolation.

[Verification Coverage Report](998_verification_coverage.md) tracks formal verification status.

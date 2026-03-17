# Aura Authorization (Layer 2) - Architecture and Invariants

## Purpose
Define authorization semantics and capability refinement using Biscuit tokens
for cryptographically verifiable capability delegation.

## Inputs
- aura-core (domain types, effect traits, resource scopes).

## Outputs
- Biscuit token model and verification semantics.
- Authorization handler: `WotAuthorizationHandler`.
- Fact types: `WotFact`, `ProposalFact`.
- Flow budget handler: `JournalBackedFlowBudgetHandler`.
- Storage authorization: `StoragePermission`, `AccessDecision`.

## Invariants
- Authority-centric resource scopes (AuthorityOp, ContextOp).
- Capability refinement via meet-semilattice: `C₁ ⊓ C₂ ≤ min(C₁, C₂)`.
- Biscuit tokens for cryptographic delegation.
- Policies are Datalog-based for flexible evaluation.

## Ownership Model

- `aura-authorization` is primarily `Pure`.
- It defines capability and policy semantics rather than owning `ActorOwned`
  runtime state.
- Transfer or attenuation semantics should remain explicit and `MoveOwned`
  rather than implicit shared mutation.
- Capability evaluation here is authoritative input to higher-layer mutation and
  publication gates.
- `Observed` layers may inspect authorization results but must not invent their
  own authority.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| `src/capabilities.rs`, `src/facts.rs`, `src/flow_budget.rs`, `src/view.rs` | `Pure` | Capability semantics, fact reduction, and derived authorization state. |
| `src/storage_authorization.rs` | `Pure`, `MoveOwned` | Storage-token and budget handling remain synchronous and typed; no async owner state or runtime locks. |
| `src/effects.rs` | `Pure` | Authorization effect contracts and pure capability-facing adapters. |
| Actor-owned runtime state | none | Layer 2 authorization must not accumulate background owner tasks. |
| Observed-only surfaces | none | Observation belongs in higher layers that consume authorization results. |

### Capability-Gated Points

- Biscuit validation and attenuation issuance
- storage authorization admission and budget charging
- capability evaluation surfaces consumed by higher-layer mutation/publication
  gates

### Verification Hooks

- `cargo check -p aura-authorization`
- `cargo test -p aura-authorization storage_authorization -- --nocapture`

### Detailed Specifications

### InvariantCapabilityMeetMonotonicity
Capability refinement must be monotone in the meet semilattice and remain context scoped.

Enforcement locus:
- src capability evaluators compute intersections for delegation and attenuation.
- Biscuit validation paths enforce cryptographic token constraints.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just test-crate aura-authorization

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) defines meet monotonicity.
- [Privacy and Information Flow Contract](../../docs/003_information_flow_contract.md) depends on capability checks before send.
## Boundaries
- No cryptographic signing (use aura-signature).
- No transport operations (use effect traits).
- Policy evaluation is pure; I/O via effects.

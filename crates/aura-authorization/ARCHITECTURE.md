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


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

## Boundaries
- No cryptographic signing (use aura-signature).
- No transport operations (use effect traits).
- Policy evaluation is pure; I/O via effects.

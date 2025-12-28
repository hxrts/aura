# Aura Signature (Layer 2) - Architecture and Invariants

## Purpose
Define identity semantics and signature verification logic, combining cryptographic
verification with authority lifecycle management.

## Inputs
- aura-core (domain types, effect traits, cryptographic types).

## Outputs
- Identity proof types: `IdentityProof` (Guardian, Authority, Threshold).
- Key material management: `KeyMaterial`, `SimpleIdentityVerifier`.
- Authority registry: `AuthorityRegistry`, `AuthorityStatus`.
- Verification result: `VerificationResult`, `VerifiedIdentity`.
- Fact types: `VerifyFact` (authority lifecycle state changes).

## Invariants
- Authorities identified by `AuthorityId` (hides device structure).
- Authority lifecycle: active → suspended → revoked.
- Signature verification is pure domain logic.
- Threshold signatures track participant sets.

## Boundaries
- No cryptographic operations (use CryptoEffects).
- No storage operations (use StorageEffects).
- Verification logic is pure; I/O via effects.

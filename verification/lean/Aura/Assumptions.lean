/-!
# Cryptographic Assumptions

This module centralizes all cryptographic hardness assumptions underlying Aura's
security proofs. Reviewers should audit these axioms carefully - they represent
the trust boundary of the formal verification.

## Quint Correspondence
- File: verification/quint/protocol_consensus.qnt
- These axioms justify invariants that Quint model checking assumes

## Rust Correspondence
- File: crates/aura-core/src/crypto/tree_signing.rs (FROST implementation)
- File: crates/aura-core/src/crypto/merkle.rs (hash functions)

## Axiom Categories

1. **FROST Threshold Signatures**: Unforgeability and uniqueness properties
2. **Hash Functions**: Collision resistance for commitment binding
3. **PRF Security**: Key derivation isolation (also in KeyDerivation.lean)

## Trust Model

These axioms hold under standard cryptographic assumptions:
- FROST: Discrete log hardness in the signature group
- Hash: Random oracle model or collision resistance
- PRF: Pseudorandomness of the underlying function

If any axiom is violated, the corresponding security property fails.
-/

namespace Aura.Assumptions

/-!
## FROST Threshold Signature Assumptions

FROST (Flexible Round-Optimized Schnorr Threshold) signatures require k-of-n
participants to produce a valid signature. These axioms capture the security
properties we rely on.
-/

-- Abstract types for cryptographic values
-- Real implementations use curve points and scalars
opaque SignatureShare : Type
opaque AggregateSignature : Type
opaque PublicKey : Type
opaque Message : Type
opaque SessionContext : Type

/-- Threshold parameter: minimum shares needed for valid signature. -/
opaque threshold : Nat

/-- Witness count: total number of potential signers. -/
opaque witnessCount : Nat

/-- Aggregate shares into a signature (abstract operation).
    In Rust: crates/aura-core/src/crypto/tree_signing.rs::aggregate_signatures -/
opaque aggregateShares : List SignatureShare → Option AggregateSignature

/-- Verify a signature against a public key and message.
    In Rust: crates/aura-core/src/crypto/tree_signing.rs::verify_signature -/
opaque verifySignature : AggregateSignature → PublicKey → Message → Bool

/-- Check if shares are from the same session context. -/
opaque sharesFromSameSession : List SignatureShare → SessionContext → Bool

/--
**Axiom: Threshold Unforgeability**

Fewer than k valid shares cannot produce a signature that verifies.
This is the fundamental security property of threshold signatures.

Cryptographic justification: Follows from the discrete log assumption and
Shamir secret sharing - with fewer than k shares, the secret key cannot
be reconstructed, so a valid Schnorr signature cannot be produced.

Quint: Assumed in `InvariantCommitRequiresThreshold`
-/
axiom frost_threshold_unforgeability :
  ∀ (shares : List SignatureShare) (pk : PublicKey) (msg : Message) (sig : AggregateSignature),
    shares.length < threshold →
    aggregateShares shares = some sig →
    verifySignature sig pk msg = false

/--
**Axiom: Signature Uniqueness**

Given the same set of valid shares from the same session, aggregation
always produces the same signature.

Cryptographic justification: FROST aggregation is deterministic - it
combines shares via Lagrange interpolation which has a unique result
for a given set of inputs.

Quint: Assumed in `InvariantUniqueCommitPerInstance`
-/
axiom frost_signature_uniqueness :
  ∀ (shares : List SignatureShare) (ctx : SessionContext),
    sharesFromSameSession shares ctx →
    ∀ (sig1 sig2 : AggregateSignature),
      aggregateShares shares = some sig1 →
      aggregateShares shares = some sig2 →
      sig1 = sig2

/--
**Axiom: Valid Aggregation Requires Threshold**

Aggregation only succeeds with at least k shares.
This is the flip side of unforgeability - the honest path works.

Cryptographic justification: With k or more valid shares, Lagrange
interpolation can reconstruct the secret and produce a valid signature.
-/
axiom frost_aggregation_requires_threshold :
  ∀ (shares : List SignatureShare) (sig : AggregateSignature),
    aggregateShares shares = some sig →
    shares.length ≥ threshold

/-!
## Hash Function Assumptions

Aura uses cryptographic hash functions for:
- Prestate binding in consensus
- Merkle tree construction for commitment trees
- Message authentication
-/

-- Abstract hash output type (e.g., 32-byte digest)
opaque Hash32 : Type

-- Inhabitedness for Hash32 (needed for opaque functions)
axiom Hash32.inhabited : Inhabited Hash32
noncomputable instance : Inhabited Hash32 := Hash32.inhabited

-- Hash function (abstract)
noncomputable opaque hash : List UInt8 → Hash32

/--
**Axiom: Collision Resistance**

It is computationally infeasible to find two distinct inputs that hash
to the same output.

Cryptographic justification: Standard property of SHA-256/BLAKE3.
We assume the hash function is modeled as a random oracle.

Quint: Assumed in `InvariantPrestateBinding` - different prestates
have different hashes, so commits are bound to specific prestates.
-/
axiom hash_collision_resistance :
  ∀ (x y : List UInt8),
    hash x = hash y → x = y

/-!
## Byzantine Fault Tolerance Assumptions

These aren't cryptographic per se, but are fundamental assumptions
about the adversarial model.
-/

/-- Maximum number of Byzantine (malicious) witnesses. -/
opaque maxByzantine : Nat

/--
**Axiom: Byzantine Threshold**

The number of Byzantine witnesses is strictly less than the signing threshold.
This ensures honest witnesses can always reach agreement.

Quint: Enforced by `byzantineThresholdOk` in protocol_consensus_adversary.qnt
-/
axiom byzantine_threshold : maxByzantine < threshold

/--
**Axiom: Honest Majority Sufficiency**

With at least (threshold - maxByzantine) honest witnesses participating,
consensus can complete. This is the liveness condition.

Quint: Assumed in `InvariantHonestMajorityCanCommit`
-/
axiom honest_majority_sufficient :
  witnessCount - maxByzantine ≥ threshold

end Aura.Assumptions

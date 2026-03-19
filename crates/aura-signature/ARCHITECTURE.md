# Aura Signature (Layer 2) - Architecture and Invariants

## Purpose
Define identity semantics and signature verification logic, combining cryptographic
verification with authority lifecycle management and session validation.

## Inputs
- `aura-core`: Domain types, effect traits, cryptographic types, tree primitives.
- `aura-macros`: Error type macros.

## Outputs
- Verification functions: `verify_authority_signature`, `verify_guardian_signature`, `verify_threshold_signature`.
- Key material: `KeyMaterial`, `SimpleIdentityVerifier`.
- Registry: `AuthorityRegistry`, `AuthorityStatus`, `VerificationResult`.
- Session: `SessionTicket`, `SessionScope`, `verify_session_ticket`.
- Identity types: `IdentityProof`, `VerifiedIdentity`, `ThresholdSig`.
- Fact types: `VerifyFact`, `DeviceNamingFact` (Layer 2 pattern).
- Messages: `ResharingMessage` types for threshold key ceremonies.

## Key Modules
- `authority.rs`, `guardian.rs`, `threshold.rs`: Signature verification functions.
- `registry.rs`: Authority lifecycle (Active → Suspended → Revoked).
- `session.rs`: Session ticket validation.
- `event_validation.rs`: Stateless identity validation.
- `facts/`: `VerifyFact`, `DeviceNamingFact` (no aura-journal dependency).
- `messages.rs`: Resharing protocol message types.

## Invariants
- Authority lifecycle: Active → Suspended → Revoked (monotonic).
- Signature verification is pure (no side effects).
- Authority-centric identity: `AuthorityId` hides device structure.
- FROST-compatible threshold verification.
- Device naming LWW: Latest timestamp wins.

## Ownership Model

- `aura-signature` is primarily `Pure`.
- It models verification and signature-domain semantics rather than
  `ActorOwned` service state.
- Any transfer of signing authority should remain explicit and `MoveOwned` in
  higher-layer APIs rather than implicit mutation here.
- Capability and attestation semantics must stay explicit so higher layers can
  gate mutation/publication correctly.
- `Observed` consumers may render signature-derived state but not author it.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| `src/authority.rs`, `src/guardian.rs`, `src/threshold.rs`, `src/session.rs`, `src/event_validation.rs` | `Pure` | Verification and validation logic only; no long-lived mutable owner state. |
| `src/registry.rs` | `Pure`, `MoveOwned` | Authority lifecycle is modeled as explicit value transitions, not shared runtime mutation. |
| `src/facts/`, `src/messages.rs` | `Pure` | Fact/message schemas and typed signature-domain payloads. |
| Actor-owned runtime state | none | Signature semantics must not accumulate service/task ownership in Layer 2. |
| Observed-only surfaces | none | Observation of verification output belongs in higher layers. |

### Capability-Gated Points

- signature and threshold attestation semantics consumed by higher-layer
  mutation/publication gates
- authority/session verification results used as explicit authorization inputs

### Verification Hooks

- `cargo check -p aura-signature`
- `cargo test -p aura-signature -- --nocapture`

### Detailed Specifications

### InvariantAuthorityLifecycleMonotonicity
Authority lifecycle transitions are monotone and threshold signature verification remains pure.

Enforcement locus:
- src authority status reducers enforce monotone lifecycle progression.
- Signature verification paths avoid side effects and preserve binding constraints.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just test-crate aura-signature

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) defines monotone transition laws.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines signature and threshold safety assumptions.
## Testing

### Strategy

aura-signature is a pure verification crate. All tests are inline since
each test is tightly coupled to a specific verification function. The
critical concern is lifecycle monotonicity — a revoked authority must
never be reactivated.

### Coverage matrix

| What breaks if wrong | Test location | Status |
|---------------------|--------------|--------|
| Authority signature verified with wrong key | `src/authority.rs` | covered |
| Session ticket expired but accepted | `src/session.rs` | covered |
| Session scope mismatch accepted | `src/session.rs` | covered |
| Threshold sig with insufficient signers | `src/threshold.rs` | covered |
| Empty signer list accepted | `src/threshold.rs` | covered |
| VerifyFact reducer merge not commutative | `src/facts/verification.rs` | covered (proptest) |
| VerifyFact reducer merge not associative | `src/facts/verification.rs` | covered (proptest) |
| Device naming context non-deterministic | `src/facts/device_naming.rs` | covered |
| Device naming fact encoding breaks | `src/facts/device_naming.rs` | covered |
| Guardian signature with wrong key accepted | `src/guardian.rs` | covered |
| Backward lifecycle transition accepted | `src/registry.rs` | covered (+ monotonicity enforcement fix) |
| Unregistered authority verified | `src/registry.rs` | covered |
| Idempotent status update rejected | `src/registry.rs` | covered |

## Boundaries
- No cryptographic operations (use `CryptoEffects`).
- No key storage (use `StorageEffects`).
- No authorization logic (use `aura-authorization`).
- No handler composition (use `aura-composition`).
- Uses Layer 2 fact pattern (no aura-journal dependency).

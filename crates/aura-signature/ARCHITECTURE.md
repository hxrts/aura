# Aura Signature (Layer 2) - Architecture and Invariants

## Purpose

Define identity semantics and signature verification logic, combining cryptographic signature verification with authority lifecycle management and session validation.

## Position in 8-Layer Architecture

**Layer 2: Specification (Domain Crate)**
- Depends only on `aura-core` (Layer 1) and `aura-macros`
- Provides pure domain logic for identity verification
- NO cryptographic operations (delegates to `CryptoEffects` in aura-effects)
- NO storage operations (delegates to `StorageEffects`)
- Used by Layer 4+ (orchestration, feature implementation, runtime)

## Core Modules

### Identity Verification (`authority.rs`, `guardian.rs`, `threshold.rs`)

**Purpose**: Pure cryptographic signature verification functions.

**Functions**:
- `verify_authority_signature()`: Verify authority signed a message
- `verify_guardian_signature()`: Verify guardian signed a message
- `verify_threshold_signature()`: Verify M-of-N threshold signature (FROST-compatible)
- `verify_threshold_signature_with_signers()`: Verify with signer accountability
- `verify_signature()`: Basic Ed25519 verification (no identity context)

**Key Types**:
- `IdentityProof`: WHO signed (Guardian, Authority, or Threshold)
  - `Guardian { guardian_id, signature }`
  - `Authority { authority_id, signature }` (preferred for cross-authority communication)
  - `Threshold(ThresholdSig)` (M-of-N participants)
- `ThresholdSig`: Aggregated Ed25519 signature + signer indices + signature shares
- `VerifiedIdentity`: Successful verification result with proof and message hash

**Authority Model**:
Protocol participants are identified by `AuthorityId` (not device-level identifiers). Authorities hide their internal device structure from external parties.

### Key Material Management (`lib.rs`)

**Purpose**: Public key storage and verification facades.

**Types**:
- `KeyMaterial`: Public key store (authority, guardian, group keys)
  - Authority keys indexed by `AuthorityId`
  - Guardian keys indexed by `GuardianId`
  - Group keys indexed by `AccountId` (for threshold verification)
- `SimpleIdentityVerifier`: Facade hiding KeyMaterial complexity
  - `add_authority_key()`, `add_guardian_key()`, `add_group_key()`
  - `verify_authority_signature()`, `verify_threshold_signature()`, `verify_guardian_signature()`

**Note**: KeyMaterial provides raw cryptographic material only (no policies or authorization).

### Authority Registry (`registry.rs`)

**Purpose**: Authority lifecycle management and attested operation verification.

**Types**:
- `AuthorityRegistry`: Tracks known authorities and their status
- `AuthorityInfo`: Authority metadata (id, public key, capabilities, status)
- `AuthorityStatus`: Active | Suspended | Revoked
- `VerificationResult`: Verification outcome with confidence score

**Operations**:
- `register_authority()`: Add authority to registry
- `verify_authority()`: Check authority status and compute confidence
- `verify_attested_operation()`: Verify commitment tree operations
  - Validates signer count vs threshold
  - Enforces epoch alignment
  - Delegates to `aura_core::tree::verify_attested_op()`

**Invariants**:
- Authority lifecycle: Active → Suspended → Revoked (monotonic)
- Confidence scores: Active=1.0, Suspended=0.5, Revoked=0.0

### Session Management (`session.rs`)

**Purpose**: Session ticket validation for scoped protocol operations.

**Types**:
- `SessionTicket`: Authorization for operations within a session
  - `session_id`, `issuer_authority`, `issued_at`, `expires_at`, `scope`, `nonce`
- `SessionScope`: DKD | Recovery | Resharing | Protocol operations

**Functions**:
- `verify_session_ticket()`: Verify ticket authenticity and expiry
- `verify_session_authorization()`: Check ticket scope matches operation

**Authority Model**: Tickets issued by authorities (not devices), aligning with authority-centric identity model.

### Event Validation (`event_validation.rs`)

**Purpose**: Pure authentication functions for identity proofs.

**Struct**: `IdentityValidator` (stateless validator)

**Functions**:
- `validate_authority_signature()`: Authority signature on event hash
- `validate_guardian_signature()`: Guardian signature on message
- `validate_threshold_signature()`: Threshold signature with signer validation
- `validate_signer_indices()`: Ensure unique, valid signer indices
- `verify_frost_signature()`: FROST-compatible verification

**Note**: No authorization logic - pure cryptographic verification only.

### Fact Types (`facts/`)

**Purpose**: Domain facts for identity and device lifecycle state changes.

**Architecture**: Layer 2 fact pattern using `aura_core::types::facts`:
- NO dependency on `aura-journal` (prevents Layer 2 → Layer 2 circular dependency)
- Uses `FactTypeId`, `try_encode`, `try_decode` APIs
- Facts wrapped in `RelationalFact::Generic` at usage sites

#### Verification Facts (`facts/verification.rs`)

**Type ID**: `verify/v1` (Schema v2)

**Fact Types** (`VerifyFact` enum):
- `AuthorityRegistered`: New authority enrolled
- `AuthorityStatusChanged`: Lifecycle transition (Active/Suspended/Revoked)
- `PublicKeyRotated`: Key rotation event
- `CapabilityGranted`/`CapabilityRevoked`: Capability lifecycle
- `SignatureVerified`/`SignatureFailed`: Verification audit trail

**Supporting Types**:
- `PublicKeyBytes`: Validated 32-byte Ed25519 public key
- `Confidence`: Score ∈ [0.0, 1.0] for verification confidence
- `VerificationType`: Local | Remote | ThresholdRemote
- `RevocationReason`: Compromised | Expired | Replaced | PolicyViolation | AdminAction

**Reducer**:
- `VerifyFactReducer`: Implements `FactDeltaReducer<VerifyFact, VerifyFactDelta>`
- `VerifyFactDelta`: Counts of authority registrations, status changes, key rotations, etc.

#### Device Naming Facts (`facts/device_naming.rs`)

**Type ID**: `device_naming/v1` (Schema v1)

**Fact Type** (`DeviceNamingFact` enum):
- `SuggestionUpdated`: Device nickname suggestion update (post-enrollment)
  - `context_id`: Derived from authority ID via `derive_device_naming_context()`
  - `authority_id`, `device_id`, `nickname_suggestion`, `updated_at`

**Semantics**:
- **LWW (Last-Writer-Wins)**: Latest `updated_at` timestamp wins during reduction
- **Authority-scoped**: Uses derived context from authority ID
- **Category A**: CRDT, immediate local effect (no threshold signature required)
- **Max size**: 64 bytes for nickname suggestion

**Design**: Device naming facts are authority-scoped but use a derived context to fit the context-based fact model. The derived context ensures isolation and uniform infrastructure.

### Cryptographic Messages (`messages.rs`)

**Purpose**: Protocol message types for resharing ceremonies.

**Message Types**:
- `CryptoMessage`: Wrapper with `CryptoPayload`
- `ResharingMessage`: Resharing protocol messages
  - `InitiateResharingMessage`, `DistributeSubShareMessage`, `AcknowledgeSubShareMessage`
  - `FinalizeResharingMessage`, `AbortResharingMessage`, `RollbackResharingMessage`
- `EncryptedShare`: Encrypted key share for resharing
- `ResharingVerification`: Proof of successful resharing
- `ResharingAbortReason`: Timeout | InsufficientShares | InvalidShare | ProtocolViolation | CoordinatorFailure | ConsensusFailure

**Note**: These types support threshold key resharing ceremonies.

## Inputs

- `aura-core`: Domain types, effect traits, cryptographic types, tree primitives
- `aura-macros`: Error type macros

## Outputs

- Identity verification functions: `verify_authority_signature`, `verify_guardian_signature`, `verify_threshold_signature`
- Key material management: `KeyMaterial`, `SimpleIdentityVerifier`
- Authority registry: `AuthorityRegistry`, `AuthorityStatus`, `VerificationResult`
- Session validation: `verify_session_ticket`, `SessionTicket`, `SessionScope`
- Event validation: `IdentityValidator`
- Fact types: `VerifyFact`, `DeviceNamingFact` (Layer 2 pattern)
- Cryptographic messages: `ResharingMessage` types
- Error types: `AuthenticationError`

## Invariants

1. **Authority Lifecycle**: Active → Suspended → Revoked (monotonic)
2. **Signature Verification**: Pure function (no side effects)
3. **Authority-Centric Identity**: Authorities identified by `AuthorityId` (device structure hidden)
4. **Session Expiry**: Tickets validated against current time
5. **Threshold Verification**: FROST-compatible Ed25519 verification
6. **Confidence Scoring**: Active=1.0, Suspended=0.5, Revoked=0.0
7. **Device Naming LWW**: Latest timestamp wins during reduction
8. **Fact Immutability**: Facts never modified, only added

## Boundaries

**What aura-signature DOES**:
- Define identity semantics and verification logic
- Validate signatures using provided public keys
- Manage authority lifecycle status
- Define session ticket structure and validation rules
- Provide fact types for identity and device lifecycle
- Define resharing protocol message types

**What aura-signature DOES NOT DO**:
- Cryptographic signing/verification (use `CryptoEffects` from aura-effects)
- Key generation or storage (use `CryptoEffects` / `StorageEffects`)
- Authorization or capability enforcement (use `aura-authorization`)
- Handler composition (use `aura-composition`)
- Multi-party protocol coordination (use `aura-protocol`)
- Fact registration in FactRegistry (handled by aura-agent)

## Key Design Patterns

### Layer 2 Fact Pattern

aura-signature uses the Layer 2 fact pattern to avoid circular dependencies:

- Uses `aura_core::types::facts` (NOT `aura_journal::DomainFact`)
- NO dependency on `aura-journal` in `Cargo.toml`
- Facts provide `try_encode()`, `try_decode()`, `to_envelope()` methods
- Reducers implement `FactDeltaReducer` (NOT `FactReducer`)
- Facts wrapped in `RelationalFact::Generic` manually at usage sites
- NOT registered in `FactRegistry`

See `docs/999_project_structure.md` §"Fact Implementation Patterns by Layer" for rationale.

### Derived Context for Authority-Scoped Facts

Device naming facts are authority-scoped but need to fit the context-based fact model. Solution:

```rust
pub fn derive_device_naming_context(authority_id: AuthorityId) -> ContextId {
    let hash = BLAKE3(b"device-naming:" || authority_id.bytes());
    ContextId::from_uuid(Uuid::from_bytes(hash[..16]))
}
```

This provides deterministic, authority-scoped "virtual contexts" enabling:
- Isolation to a single authority
- Standard fact infrastructure usage
- Uniform reduction and registry integration

### FROST-Compatible Threshold Signatures

Threshold signatures use FROST protocol but are compatible with standard Ed25519 verification:

- `ThresholdSig` contains aggregated signature + signer indices + signature shares
- Verification uses `aura_core::ed25519_verify()` with group public key
- Signer accountability via participant tracking

## Dependencies

- `aura-core`: Foundation types and traits
- `aura-macros`: Error type code generation
- External: `serde`, `serde_json`, `uuid`, `tracing`, `hex`

## Dependents

- Layer 3: `aura-effects` (crypto handlers)
- Layer 4: `aura-protocol`, `aura-guards`
- Layer 5: `aura-authentication`, `aura-recovery`, feature crates
- Layer 6: `aura-agent`, `aura-simulator`, `aura-app`

## Documentation

- `docs/102_authority_and_identity.md`: Authority model and identity semantics
- `docs/103_journal.md`: Fact-based journal and lifecycle facts
- `docs/100_crypto.md`: Cryptographic architecture and FROST
- `docs/999_project_structure.md`: Layer 2 architecture and fact patterns

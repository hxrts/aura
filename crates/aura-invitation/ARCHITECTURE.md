# Aura Invitation (Layer 5)

## Purpose

Invitation protocol for establishing relationships between authorities, including invitation creation, redemption, and ceremony coordination.

## Scope

| Belongs here | Does not belong here |
|-------------|---------------------|
| Invitation facts, reducers, and deltas | Relationship state (aura-relational) |
| Ceremony and protocol coordination | Transport coordination (aura-protocol) |
| Invitation lifecycle management | Runtime invitation cache (aura-agent) |
| Authorization guards for invitation flows | |

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| Incoming | aura-core | Effect traits, identifiers |
| Incoming | aura-authentication | Session and identity verification |
| Incoming | aura-authorization | Biscuit tokens for invitation capabilities |
| Incoming | aura-guards | Invitation guards |
| Incoming | aura-macros | Capability boundary and derive macros |
| Outgoing | — | `InvitationFact`, `InvitationFactReducer`, `InvitationDelta` for journal integration |
| Outgoing | — | `InvitationCeremony` for multi-party invitation flows |
| Outgoing | — | `InvitationProtocol` for invitation message exchange |
| Outgoing | — | `InvitationService` for invitation lifecycle management |
| Outgoing | — | `InvitationGuards` for authorization checks |
| Outgoing | — | `Relationship` struct for established connections |

## Invariants

- Enrollment invitation facts emit schema 2 and continue decoding schema-1
  canonical DAG-CBOR map payloads. Missing setup bindings remain absent during
  replay; historical decoding never manufactures response authorization.
- Signed setup admission binds nonce/digest, both physical devices, provisional
  invitee, subject, invitation/ceremony, pending epoch, baseline manifest and
  exact-parent verifier inventory digests. Admission signature verification
  alone does not authenticate that inventory or mint historical signing rights.
  Independently pinned manifest provenance and owned revocation/admission state
  are runtime prerequisites.

- Facts with known context must reduce under their matching `ContextId`.
- Invitation identifiers are treated as stable binding keys.
- `enrollment_setup` owns bounded versioned setup-code decoding and a dedicated
  canonical signing transcript. `VerifiedEnrollmentSetupPossession` is minted
  only after validity, policy, proof binding and cryptographic verification; it
  has private fields and no deserialization path. It proves possession under an
  embedded key, not an independently authenticated authority/device binding.
  Explicit user transfer and ceremony-specific trust remain app/runtime-owned.
  `EnrollmentSetupExportError` preserves readiness, admission, physical-time,
  proof and persistence sources through the runtime bridge instead of erasing
  them into display strings. `EnrollmentSetupVerificationError` likewise
  preserves physical-time, setup-proof and workflow-boundary failures; runtime
  possession verification does not mint the app's explicit user-transfer pin.
- Enrollment receive validation preserves typed mismatches for invitation,
  authority, ceremony, device and epoch. A negative or missing-epoch confirmation
  cannot establish enrollment. Message shape validation is separate from the
  runtime owner's authentication and committed ceremony evidence.
- Invitation redemption creates mutual relational context.
- The `shareable` module owns code decoding, sender-proof verification, expiry and channel-context validation. Only the signature-verifying `verify_code` method mints `ValidatedImportedInvitation`; its private fields prevent a caller from promoting an arbitrary cache record into creation evidence. The optional `test-support` feature exposes unsigned codec fixtures for agent unit tests but cannot mint a validated-import token.
- Invitation projections are created through an app-owned `InvitationCreationWitness` from a sealed validated-import token or `InvitationFact::Sent`; acceptance, decline, and cancellation facts settle existing invitations or wait for their creation evidence rather than inventing missing metadata.
- Invitation choreographies remain theorem-pack-free until they move onto a
  Telltale-native authority/evidence path with a concrete runtime consumer.

### InvariantInvitationRedemptionUniqueness

Invitation redemption must be unique and must produce consistent relational context state.

Enforcement locus:
- src invitation fact reducers validate identifier and context binding.
- Redemption writes journal evidence for replay and audit.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just test-crate aura-invitation

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) defines context-scoped fact semantics.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines invitation safety expectations.

## Ownership Model

> Taxonomy: [Ownership Model](../../docs/122_ownership_model.md)

`aura-invitation` is primarily `Pure` invitation-domain logic plus single-owner workflow contracts.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| facts/reducers/domain types | `Pure` | Deterministic invitation fact reduction and relationship-binding semantics. |
| invitation lifecycle handles, acceptance/redemption flows, ceremony/protocol state | `MoveOwned` | Exclusive invitation authority and lifecycle ownership remain explicit. |
| long-lived invitation coordination | selective single-owner | Ongoing invitation coordination must stay single-owner and capability-gated. |
| capability-gated publication | typed workflow boundary | Invitation creation/acceptance/redemption publication stays explicit and terminally typed. |
| Observed-only surfaces | `Observed` | UI/runtime observation remains downstream. |

### Capability-Gated Points

- invitation creation, acceptance, redemption, and relationship-establishment boundaries
- ceremony/protocol publication consumed by higher-layer runtime and interface flows

## Testing

### Strategy

Invitation redemption uniqueness and ceremony correctness are the primary concerns. Integration tests in `tests/ceremony/` verify end-to-end send/accept flows with guard evaluation. The contact establishment matrix stays top-level as a cross-flow equivalence test. Inline tests verify fact reduction, guard evaluation, descriptor validity, and protocol serialization.

### Commands

```
cargo test -p aura-invitation
```

### Coverage matrix

| What breaks if wrong | Test location | Status |
|---------------------|--------------|--------|
| Fact reduces under wrong context | `src/facts.rs` `test_reducer_rejects_context_mismatch` | Covered |
| Expired descriptor accepted | `src/descriptor.rs` `test_is_expired`, `test_is_valid_at` | Covered |
| Capability check bypassed | `src/service.rs` `test_prepare_send_invitation_missing_capability` | Covered |
| Insufficient budget allows invitation | `src/service.rs` `test_prepare_send_invitation_insufficient_budget` | Covered |
| E2E send → accept produces wrong state | `tests/ceremony/invitation_service_e2e.rs` | Covered |
| Contact flows produce different facts | `tests/contact_establishment_matrix.rs` | Covered |
| Ceremony ID non-deterministic | `src/invitation_ceremony.rs` `test_ceremony_id_determinism` | Covered |
| Fact serialization roundtrip lossy | `src/facts.rs` `test_invitation_fact_serialization` | Covered |
| Reducer non-idempotent | `src/facts.rs` `test_reducer_idempotence` | Covered |
| Protocol message serialization breaks | `src/protocol.rs` (15 inline tests) | Covered |
| Message exceeds max length | `src/service.rs` `test_prepare_send_invitation_message_too_long` | Covered |
| Legacy ceremony payload incompatible | `src/view.rs` `test_view_reducer_handles_legacy_ceremony_committed_payload` | Covered |

## Operation Categories

See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.

## References

- [Theoretical Model](../../docs/002_theoretical_model.md)
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md)
- [Operation Categories](../../docs/109_operation_categories.md)

## Enrollment trust transfer boundary

The bounded enrollment manifest domain defines the signed exact parent/node verifier inventory, setup and invitation bindings, participant/share/package/policy digests, and independently transferred initiator identity statement. Pure signature and baseline verification produce sealed evidence without assigning runtime trust or granting signing/membership rights. Legacy absent bindings may decode but never authorize admission or acceptance.

See [cryptography](../../docs/100_crypto.md), [operation ownership](../../docs/109_operation_categories.md), [shared user flows](../../docs/121_user_flow_harness.md), and [testing](../../docs/804_testing_guide.md).

### Enrollment response manifest binding

`DeviceEnrollmentAccept` includes an optional manifest digest for wire compatibility. Legacy absence does not authorize enrollment. The runtime's v3 acceptance transcript binds that digest together with the canonical invitation, setup, subject, physical device, ceremony and decision. The invitee selects it only through independent manifest admission; the issuer compares it to its own retained signed artifact. Persistence and process-local proof constructors belong to `aura-agent`.

### Required invitation fact decoding

Required recovery uses `InvitationFact::try_from_envelope`, which bounds payloads, accepts only supported schema versions 1–2, obeys the declared DAG-CBOR or JSON encoding, and retains structural envelope and original codec errors. `try_from_envelope_in_context` also validates explicit payload context against the relational wrapper. Historical contextless lifecycle records remain contextless. Observational `DomainFact::from_envelope` retains its optional compatibility contract. Domain regressions exercise independent schema-1 enrollment bytes, encoding mismatches, unsupported schemas, malformed payloads, size bounds and typed sources.

### Device-enrollment response contract

`DeviceEnrollmentResponse` is an untrusted wire sum of accepted and refused
responses. Its fields do not grant acceptance or rejection authority. L6 binds
and verifies the distinct signed decision transcripts against the independently
transferred setup pin before advancing the choreography or settling a ceremony.
The response message does not declare an unconditional accepted journal fact.
Terminal failure remains separate from committed membership and activation.

### Enrollment negative terminal notification

Authenticated enrollment cancellation uses the separately admitted finite
`aura.invitation.device_enrollment_terminal_notice` protocol. Its session identity
is domain separated from the request/response session and binds the original
invitation and admitted manifest digest. A notification never advances the
request/response VM's program counter. The wire envelope is untrusted: only the
independently pinned issuer signature and exact original manifest, invitation,
ceremony and physical-device bindings issue negative terminal evidence.

The issuer sends only after the durable cancelled terminal decision. The
receiver's listener belongs to the existing invitation service task tree and
shares the original admitted window's lease, checkpoint and observations. It
retains a distinct immutable signed failure receipt before reporting failure.
Negative evidence cannot authorize device adoption or committed recovery.
Cancellation and ordinary response races close their actual owned session slots;
forced drop retires exact session custody without claiming asynchronous close
or terminal evidence. Required failure causes remain in native observation and
service task health. Notification retries retain the original deadline and
only reconcile definitely unsent, exactly scoped transport failures.

Reserved invitation creation evaluates capabilities and budget at the current
required guard clock, but emits the original canonical reservation creation and
expiry times. Pure guard tests enforce this distinction and reject expired or
future reservations. Required execution retains `InvitationGuardDenial` and its
structural policy reason; display-only compatibility planning is observational.

### Enrollment manifest v2

The signed manifest has an explicit final active verifier inventory distinct from historical baseline parents. Its version-2 transcript and code prefix bind bounded exact epoch/head/node/mode/roster/quorum/package tuples. Legacy version-1 decoding omits the new field and preserves its original canonical signature bytes; missing inventory is never repaired from historical parents. Shape/signature evidence remains separate from runtime capture custody and independent transfer provenance.

Enrollment setup possession parses bounded public signature encodings before
calling the verification provider. `InputEncoding` retains native public-input
decoder failures; `Crypto` retains provider failures. Neither shape validation
nor parsing manufactures independent setup pin provenance.

The enrollment-manifest domain owns `ENROLLMENT_ALLOCATION_TIMEOUT_MS`, the
600,000-millisecond maximum original device-enrollment allocation policy.
The runtime tracker consumes this value; signed quorum wire validation must
consume the same policy rather than invent another timeout. Setup-code validity
does not authorize a longer allocation. Actual quorum consumer integration
remains required.

### Exact capability declaration evidence

A capability boundary declares its exact capability type in a parsed input or output. Semantic labels may specify `capability_type = Type`; labels and body text do not establish custody. Accessors return that exact type, authorizers retain that typed input or output, and proof issuers also declare their authoritative proof source. Runtime helpers with an actual held receiver may specify `receiver_type = OwnerType`; expansion checks the concrete receiver against that type. This receiver contract does not apply to free functions or replace authorization inputs in authorizers.

Constants, capability-like substrings, incidental body calls, phantom markers and associated projections do not satisfy the declaration. The declaration verifies API shape; private constructors and actual runtime ownership validation establish authority. Pure validators, pure execution-plan builders and observed projections are not capability issuers and carry no decorative capability-boundary declarations. Their domain tests and effect-placement rules remain required.

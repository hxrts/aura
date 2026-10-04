# Aura Protocol (Layer 4)

## Purpose

Coordinate multi-party protocols and guard-chain enforcement. This crate provides orchestration glue, not single-party effect implementations.

## Scope

| Belongs here | Does not belong here |
|--------------|----------------------|
| Guarded transport operations and protocol outcomes | Runtime composition or lifecycle management (Layer 6) |
| Orchestrated consensus and anti-entropy flows | Application-specific protocol logic (Layer 5) |
| Guard chain integration on every send | Production effect implementations |
| Re-export of guard-owned verified ingress types | Direct remote data persistence |
| Session types and choreographic annotations | |

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| Down | `aura-core` | Effect trait definitions, domain types |
| Down | `aura-macros` | Declaration and derive macros |
| In | Effect trait implementations | Assembled by higher layers (agent/simulator) |
| In | Choreographic annotations, session types | Protocol structure |
| In | Journal and authorization facts | From domain crates |
| Out | Guarded transport operations | Protocol outcomes |
| Out | Orchestrated consensus/anti-entropy flows | Coordination results |

## Invariants

- No production effect implementations live in Layer 4.
- Peer-originated data must cross the guard-owned verified ingress typestate
  boundary before it is eligible for state mutation.
- `PersistentTreeHandler` is a storage primitive: callers must authenticate
  parent-epoch verifiers and validate a complete tree-op batch before calling
  its import or replacement methods.
- Guard chain is enforced on every send.
- Journal facts and budgets are coupled atomically before transport.

### InvariantProtocolGuardMediation

Protocol sends must be mediated by the guard chain with budget and journal coupling before transport.

Enforcement locus:
- src handlers and sessions integrate guard decisions into send paths.
- Protocol modules avoid direct production effect implementations.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- `just test-crate aura-protocol` and `just check-arch`

Contract alignment:
- [Privacy and Information Flow Contract](../../docs/003_information_flow_contract.md) defines charge-before-send behavior.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines fact-backed send requirements.

## Ownership Model

> Taxonomy: [Ownership Model](../../docs/122_ownership_model.md)

`aura-protocol` uses `MoveOwned` for delegation, session transfer, and other exclusive orchestration boundaries. `ActorOwned` state is used only for justified long-lived coordinators. Async orchestration flows must reach typed terminal outcomes.

See [System Internals Guide](../../docs/807_system_internals_guide.md) §Core + Orchestrator Rule.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| protocol/session handlers and core builder/config modules | `MoveOwned` | Session transfer, delegation, and typed orchestration boundaries. |
| guard-owned verified remote-ingress boundary re-export | `MoveOwned`, capability-gated | Carries checked evidence before decoded peer data can flow into mutation APIs. |
| long-lived coordinators such as `transport_coordinator` and peer-connection retry actors | `ActorOwned` | Justified orchestration coordinators only; not the default model for protocol logic. |
| guard-chain and effect integration surfaces | capability-gated orchestration | Capability, flow, and journal coupling remain explicit on send paths. |
| observed-only surfaces | none local | Observation belongs in higher layers consuming protocol outputs. |

### Capability-Gated Points

- Guard-chain mediated send paths with budget and journal coupling.
- Typed protocol outcomes consumed by higher-layer runtime and testing lanes.

## Testing

### Strategy

Protocol coordination contracts and guard mediation are the primary concerns. Integration tests in `tests/coordination/` verify transport coordinator behavior; inline tests verify state machines, context immutability, and CRDT delivery semantics.

### Commands

```
cargo test -p aura-protocol
just check-arch
```

### Coverage matrix

| What breaks if wrong | Test location | Status |
|---------------------|--------------|--------|
| Send without guard mediation | `aura-guards` `tests/chain/guard_chain_transport.rs` | Cross-crate |
| Transport coordinator config/error handling | `tests/coordination/transport_coordinator.rs` | Covered |
| Context mutation breaks immutability | `src/handlers/context/mod.rs` (inline) | Covered |
| Version handshake rejects compatible peer | `src/handlers/version_handshake.rs` (inline) | Covered |
| CRDT causal ordering violated | `src/effects/crdt/delivery.rs` (inline) | Covered |
| Intent state lattice ordering incorrect | `src/state/intent_state.rs` (inline, 7 tests) | Covered |
| Peer connection retry budget wrong | `src/handlers/peer_connection.rs` (inline) | Covered |
| Admission capability validation fails | `src/admission.rs` (inline) | Covered |
| Decoded peer data is treated as verified ingress | `aura-guards/src/ingress.rs` (inline) | Initial typestate coverage |

## References

`ChoreographyError::RequiredTime` preserves a required time-effect source and
exposes the stable `choreography_required_time` protocol code. Runtime admission
must obtain required clock evidence before installing session state. Runtime
retirement must still release owned resources when the clock fails, preserving
the primary clock fault and any typed secondary cleanup failure. See
`docs/104_runtime.md` for this lifecycle contract.

- [Privacy and Information Flow Contract](../../docs/003_information_flow_contract.md)
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md)
- [Ownership Model](../../docs/122_ownership_model.md)
- [System Internals Guide](../../docs/807_system_internals_guide.md)

### Complete tree publication

`PersistentTreeHandler` serializes local mutations, writes complete content-addressed operation blobs before publishing the canonical ordered index, and updates its observed cache only after publication is acknowledged. An index-write error is an uncertain outcome: subsequent reads reload the canonical index. Original blobs are retained; reclamation is a separate maintenance responsibility. Storage comparison never authenticates a peer batch. The runtime supplies an independently admitted baseline and the immutable digest of its original local history; replay of an existing complete baseline prefix preserves later local operations. Snapshot-state representation remains a separate contract from this index-publication guarantee.

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

The persistent tree decision lease holds a platform-neutral asynchronous mutation
lock across authenticated reads and installation. Protocol orchestration does
not require a Tokio executor for this custody primitive. The architecture syntax
gate enforces that boundary; actual held-decision replacement and extension
regressions verify writer exclusion and preservation of later evidence.

`TimeoutCoordinator` forwards absolute physical deadline waits to its original
inner provider; it neither chooses a second clock nor derives a renewed delay.

### Public enrollment transcript rounds

The L4 coordinator accepts public commitments, verified shares, exact approved
messages and independently retained public policy. Private participant material
and nonce custody remain in original L6 owners. Packet codecs bound wire shape;
exact session/intent/domain/party/phase validation and individual native share
proofs authenticate admitted rounds without granting user consent or group
signing authority. Only aggregate verification under the retained public group
package establishes the group signature.

The public signing policy borrows the actual `SecurityTranscript` and an active
`TrustedPublicKey` in the authority threshold domain. Required canonical encoding
retains native codec causes. The coordinator independently decodes its retained
public package and checks the selected verifier, epoch presence and key hash
before participant callbacks. L6 additionally requires byte equality with the
original approved domain. Raw-byte policy construction is compile-fail guarded;
wrong domain, revoked key, corrupt hash and codec failures have unit coverage.

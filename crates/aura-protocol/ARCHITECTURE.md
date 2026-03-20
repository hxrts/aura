# Aura Protocol (Layer 4)

## Purpose

Coordinate multi-party protocols and guard-chain enforcement. This crate provides
orchestration glue, not single-party effect implementations.

## Scope

| Belongs here | Does not belong here |
|--------------|----------------------|
| Guarded transport operations and protocol outcomes | Runtime composition or lifecycle management (Layer 6) |
| Orchestrated consensus and anti-entropy flows | Application-specific protocol logic (Layer 5) |
| Guard chain integration on every send | Production effect implementations |
| Session types and choreographic annotations | |

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| Down | `aura-core` | Effect trait definitions, domain types |
| In | Effect trait implementations | Assembled by higher layers (agent/simulator) |
| In | Choreographic annotations, session types | Protocol structure |
| In | Journal and authorization facts | From domain crates |
| Out | Guarded transport operations | Protocol outcomes |
| Out | Orchestrated consensus/anti-entropy flows | Coordination results |

## Invariants

- No production effect implementations live in Layer 4.
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

`aura-protocol` uses `MoveOwned` for delegation, session transfer, and other
exclusive orchestration boundaries. `ActorOwned` state is used only for
justified long-lived coordinators. Async orchestration flows must reach typed
terminal outcomes.

See [System Internals Guide](../../docs/807_system_internals_guide.md) §Core + Orchestrator Rule.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| protocol/session handlers and core builder/config modules | `MoveOwned` | Session transfer, delegation, and typed orchestration boundaries. |
| long-lived coordinators such as `transport_coordinator` and peer-connection retry actors | `ActorOwned` | Justified orchestration coordinators only; not the default model for protocol logic. |
| guard-chain and effect integration surfaces | capability-gated orchestration | Capability, flow, and journal coupling remain explicit on send paths. |
| observed-only surfaces | none local | Observation belongs in higher layers consuming protocol outputs. |

### Capability-Gated Points

- Guard-chain mediated send paths with budget and journal coupling.
- Typed protocol outcomes consumed by higher-layer runtime and testing lanes.

## Testing

### Strategy

Protocol coordination contracts and guard mediation are the primary concerns.
Integration tests in `tests/coordination/` verify transport coordinator
behavior; inline tests verify state machines, context immutability, and
CRDT delivery semantics.

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

## References

- [Privacy and Information Flow Contract](../../docs/003_information_flow_contract.md)
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md)
- [Ownership Model](../../docs/122_ownership_model.md)
- [System Internals Guide](../../docs/807_system_internals_guide.md)

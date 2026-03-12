# Aura Agent (Layer 6) - Architecture and Invariants

## Purpose

Production runtime composition and effect system assembly for authority-based
identity management. Owns structured concurrency, service lifecycle, session
ownership, effect registry, builder infrastructure, and choreography adapters.

## Inputs

- All lower layers (Layers 1-5): core types, effect traits, domain crates, protocols.
- Authority identifiers (`AuthorityId`) and context (`ContextId`, `SessionId`).
- Effect implementations from aura-effects.
- Protocol coordination from aura-protocol.

## Outputs

- `AgentBuilder`, `AuraAgent`, `EffectContext`, `EffectRegistry`.
- `AuraEffectSystem` with subsystems: `CryptoSubsystem`, `TransportSubsystem`, `JournalSubsystem`.
- Service actors: `SessionServiceApi`, `AuthServiceApi`, `RecoveryServiceApi`, `SyncManagerState`.
- `RuntimeSystem`, `LifecycleManager`, `ReceiptManager`, `FlowBudgetManager`.

## Key Modules

- `core/`: Public API (`AgentBuilder`, `AuraAgent`, `AuthorityContext`).
- `builder/`: Platform-specific preset builders (CLI, iOS, Android, Web).
- `runtime/`: Service actors, subsystems, choreography adapters.
- `handlers/`: Service API implementations (auth, session, recovery, etc.).
- `reactive/`: Signal-based notification and scheduling.

## Invariants

- Must NOT create new effect implementations (delegate to aura-effects).
- Must NOT implement multi-party coordination (delegate to aura-protocol).
- Must NOT be imported by Layers 1-5 (prevents circular dependencies).
- Authority-first design: all operations scoped to specific authorities.
- Lazy composition: effects assembled on-demand.
- Mode-aware execution: production, testing, and simulation use same API.

## Structured Concurrency Model

`aura-agent` uses structured concurrency as the only production async model.

Rules:

- Every long-lived async subsystem has one named owner.
- Every owner has one rooted task group.
- Child tasks belong to exactly one task group.
- Detached fire-and-forget tasks are forbidden in production runtime code.
- Shutdown is hierarchical and parent-driven.

### Service Actor Pattern

Long-lived runtime services use actor ownership with typed command channels:

```rust
struct ServiceHandle {
    cmd_tx: mpsc::Sender<ServiceCommand>,
}

struct ServiceActor {
    state: ServiceState,
    cmd_rx: mpsc::Receiver<ServiceCommand>,
    tasks: TaskGroup,
}
```

Each actor maintains:
- Typed command channel for external requests.
- Single event loop driving all state transitions.
- Explicit lifecycle state machine.
- Owned child task group for internal loops.
- Authoritative health derived from actor state.

### Service Lifecycle

All long-lived runtime services implement this lifecycle:

- `New`: Initial state before startup.
- `Starting`: Initialization in progress.
- `Running`: Actor alive and command path available.
- `Stopping`: Graceful shutdown in progress.
- `Stopped`: No live owned tasks and no live command handling.
- `Failed`: Observable failure state.

Rules:

- `start()` is serialized and idempotent.
- `stop()` is serialized and idempotent.
- `Running` implies the command path is available.
- `Stopped` implies no live owned tasks.
- `Failed` does not silently downgrade to healthy operation.

### Command/Reply Pattern

Service APIs expose typed request/reply interactions:

```rust
enum ServiceCommand {
    Start {
        reply: oneshot::Sender<Result<(), ServiceLifecycleError>>,
    },
    Stop {
        reply: oneshot::Sender<Result<(), ServiceLifecycleError>>,
    },
}
```

Rules:

- Requests requiring acknowledgement use typed `oneshot` replies.
- Best-effort fire-and-forget commands are disallowed for correctness-critical work.
- Command handlers surface task failures and ownership violations.

## Canonical Host/VM Boundary

`aura-agent` aligns with Telltale's canonical execution model.

Host async code may:

- Perform external transport I/O.
- Buffer and order external events.
- Drive retries and timers.
- Manage task supervision.
- Schedule admitted work.

Host async code may not:

- Mutate Telltale session state directly from arbitrary tasks.
- Bypass fragment ownership.
- Perform ad hoc delegation handoff.
- Widen concurrency beyond the admitted envelope.

The only legal path from external async input to session mutation:

1. External event received by host runtime.
2. Host runtime converts it to typed ingress.
3. Ingress routed to the current authoritative owner.
4. Owner drives VM/session work at the sanctioned boundary.

This canonical ingress rule is an architectural invariant.

## Session Ownership

Telltale-facing session state follows strict ownership rules.

Rules:

- Each active session or fragment has exactly one current local owner.
- The owner is either a per-session actor or an authoritative choreography runtime loop.
- Network, timer, and external events are queued before touching session state.
- Session ownership and task ownership move together.
- Session-bound effects execute only under the current owner.

### Session Ingress

All external events enter sessions through typed ingress:

```rust
enum SessionIngress {
    NetworkEnvelope(TransportEnvelope),
    Timer(SessionTimerEvent),
    Command(SessionCommand),
    DelegatedEndpoint(DelegationBundle),
}

struct SessionHandle {
    ingress_tx: mpsc::Sender<SessionIngress>,
}
```

### Ownership Transitions

Owner-visible state transitions:

- `Unowned -> Claimed`
- `Claimed -> Running`
- `Running -> DelegatingOut`
- `DelegatingOut -> Released`
- `Running -> Stopping`
- `Stopping -> Stopped`
- `Any -> Failed`

No transition may create overlapping owners.

## Concurrency Profiles

`aura-agent` recognizes three runtime concurrency profiles for choreography work:

- **Canonical**: Exact single-owner reference path.
- **EnvelopeAdmitted**: Disjoint or admitted work preserving safety-visible meaning.
- **Fallback**: Immediate degradation to canonical execution when envelope admission fails.

Correctness never depends on uncontrolled host scheduling. If the runtime cannot
show that a path is envelope-safe, it serializes execution.

## Link and Delegate Boundaries

### Link Boundary

`link` is a static composition boundary.

Runtime consequences:

- Linked bundles define ownership boundaries as well as composition boundaries.
- Linked protocols remain session-disjoint unless composition explicitly shares state.
- Cross-boundary effect routing is explicit.
- Ad hoc shared mutable state across linked boundaries is forbidden.

### Delegate Boundary

`delegate` is an ownership-transfer boundary.

Runtime consequences:

- Endpoint/session ownership transfer is atomic.
- Capability/effect context transfers with the endpoint.
- Stale-owner access after delegation is forbidden.
- Ambiguous local ownership is rejected before the VM observes the transfer.

## Async Primitives

### Preferred

- `tokio::sync::mpsc`
- `tokio::sync::oneshot`
- `tokio::sync::watch` for snapshots rather than command routing
- `tokio::sync::Notify` for ownership-local wakeups
- Supervised task groups
- Actor loops

### Allowed with Justification

- `tokio::sync::RwLock` for shared state that cannot yet be moved into an actor.
- `parking_lot::{Mutex,RwLock}` for brief synchronous critical sections.

### Forbidden in Production

- Raw `tokio::spawn`.
- Raw `spawn_local`.
- Ad hoc background loops without a task owner.
- Direct session mutation from non-owner tasks.
- Multi-writer service state as the default pattern.

## Typed Errors

Runtime errors use typed enums:

- `RuntimeLifecycleError`: Runtime startup/shutdown failures.
- `TaskSupervisionError`: Task spawn/completion/abort failures.
- `ServiceLifecycleError`: Service start/stop failures.
- `ServiceOperationError`: Service operation failures.
- `SessionOwnershipError`: Ownership claim/release failures.
- `SessionIngressError`: Ingress routing failures.
- `DelegationError`: Endpoint transfer failures.
- `LinkBoundaryError`: Cross-link routing failures.
- `ConcurrencyEnvelopeError`: Envelope admission failures.
- `EffectRoutingError`: Effect dispatch failures.
- `ShutdownError`: Teardown failures.
- `InvariantViolationError`: Fatal invariant violations.

Design rules:

- Use narrow enums at module boundaries.
- Use conversion only where information loss is acceptable.
- Do not hide ownership, lifecycle, or shutdown class inside generic strings.

## Instrumentation

Instrumentation is structured and consistent across services.

### Required Event Families

- Runtime startup/shutdown.
- Service lifecycle transition.
- Task spawn/completion/failure/abort.
- Session claim/release/failure.
- Ingress accepted/rejected/dropped.
- Delegation start/commit/rollback/reject.
- Link boundary route/reject.
- Concurrency profile select/fallback.
- Invariant violation.

### Required Fields

Where applicable:

- `service`, `task`, `session_id`, `fragment_key`
- `owner`, `from_owner`, `to_owner`
- `profile`, `error_kind`, `correlation_id`

## Telltale Bridge Ownership

- `aura-agent` owns runtime admission wiring and choreography backend selection.
- `aura-agent` owns telltale runtime parity test lanes and scenario contract gates.
- `aura-agent` must not own bridge schema transformations that belong in `aura-quint`.

## Detailed Specifications

### InvariantStructuredConcurrency

All production async work uses structured concurrency with explicit task ownership.

Enforcement locus:
- `src/runtime/` service actors own their task groups.
- `TaskGroup` enforces parent-child relationships.

Failure mode:
- Detached tasks outlive their owner and mutate torn-down resources.
- Shutdown leaves orphan tasks running.

Verification hooks:
- `just ci-async-task-ownership`
- `just test-crate aura-agent`

Contract alignment:
- [Runtime](../../docs/120_runtime.md) defines service actor patterns.

### InvariantCanonicalIngress

External events reach session state only through typed ingress and the current owner.

Enforcement locus:
- `SessionHandle` provides the only ingress path.
- Session actors consume ingress and drive VM work.

Failure mode:
- Direct session mutation from arbitrary tasks.
- VM/session state diverges from canonical execution.

Verification hooks:
- `just ci-async-session-ownership`
- `just ci-choreo-parity`

Contract alignment:
- [Effect System](../../docs/105_effect_system.md) defines session-local VM bridge.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines canonical execution.

### InvariantSessionOwnership

Each active session has exactly one local owner at any time.

Enforcement locus:
- Ownership transitions are explicit state machine transitions.
- Delegation commits atomically.

Failure mode:
- Overlapping owners mutate the same session.
- Stale owner access after delegation.

Verification hooks:
- `just ci-async-delegation-ownership`
- `just test-crate aura-agent`

Contract alignment:
- [Runtime](../../docs/120_runtime.md) defines session management.

### InvariantRuntimeCompositionBoundary

Runtime composition assembles existing effect handlers without introducing new
effect implementations or protocol logic.

Enforcement locus:
- `src/runtime/` composes handlers and services through registry and builder types.
- `src/builder/` constrains runtime modes and dependency wiring.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- `just check-arch`
- `just test-crate aura-agent`

Contract alignment:
- [Aura System Architecture](../../docs/001_system_architecture.md) defines layer boundaries.
- [Effect System](../../docs/105_effect_system.md) defines composition constraints.

### InvariantBridgeOwnershipAgent

Runtime telltale integration consumes bridge artifacts but does not redefine bridge schema.

Enforcement locus:
- `src/runtime/choreo_engine.rs` and `src/runtime/choreography_adapter.rs` enforce runtime capability admission.
- `tests/telltale_vm_parity.rs` and `tests/telltale_vm_scenario_contracts.rs` run runtime parity and contract lanes.

Failure mode:
- Runtime layer duplicates schema translation code and drifts from `aura-quint`.
- Admission and parity behavior diverges across runtime profiles.

Verification hooks:
- `just ci-choreo-parity`
- `just ci-conformance-contracts`

Contract alignment:
- [Formal Verification Reference](../../docs/119_verification.md) defines runtime parity lanes.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines runtime admission guarantees.

### InvariantFatalViolations

Certain conditions are fatal runtime invariant violations:

- Ambiguous active session ownership.
- Delegated endpoint still usable by old owner after commit.
- Session-bound effect executed by non-owner.
- VM/session mutation from outside canonical ingress.
- Runtime proceeding in a concurrency mode that has not been admitted.
- Teardown that leaves owned tasks mutating torn-down resources.
- Impossible typed state combinations.

Recovery is acceptable only when:
- The violating operation is rejected before state mutation.
- The failure is surfaced as a typed error and structured event.

## Cross-Crate API Boundary

Other crates interact with `aura-agent` through sanctioned public APIs only.

Rules:

- No direct imports of internal runtime modules from other crates.
- No bypass of `AuraAgent` or sanctioned public handles for runtime ownership-sensitive work.
- No cross-crate reach-in to mutate service or session internals.

This is enforced by architectural policy gates.

## Boundaries

- Stateless handlers live in aura-effects.
- Protocol logic lives in aura-protocol.
- Application core lives in aura-app.
- Bridge schema transformations live in aura-quint.

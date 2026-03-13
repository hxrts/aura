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

This model is intentionally split:

- actor services solve long-lived runtime supervision and lifecycle
- move semantics solve session and endpoint ownership transfer

Do not collapse those into one generic async pattern.

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

This actor pattern is the runtime-structure layer.
It is the right tool for:

- startup and shutdown ordering
- retries and maintenance loops
- panic and join supervision
- health reporting
- named task ownership

It is not the ownership-transfer primitive for Telltale sessions or delegated endpoints.

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

This boundary also defines the split in responsibility:

- host runtime structure is actor-supervised
- session mutation authority is owner-routed
- ownership transfer across `delegate` is explicit and singular rather than shared through actor state

The host/VM boundary must also preserve Telltale's communication identity and replay semantics.
Host buffering, retries, and task scheduling may delay or reorder work internally, but they must
not rewrite the identity used by replay or envelope checks.

## Session Ownership

Telltale-facing session state follows strict ownership rules.

This is the move-semantics side of the runtime design.

Rules:

- Each active session or fragment has exactly one current local owner.
- The owner is either a per-session actor or an authoritative choreography runtime loop.
- Network, timer, and external events are queued before touching session state.
- Session ownership and task ownership move together.
- Session-bound effects execute only under the current owner capability.

The current owner may be hosted by an actor, but ownership itself remains a
single-owner move boundary, not a shared mutable actor coordination pattern.

### Owner Record vs Owner Capability

`aura-agent` treats these as distinct runtime concepts:

- the owner record answers who currently owns the session or endpoint
- the owner capability answers what that owner may currently do

Rules:

- owner-record transitions are ownership changes, not implicit authorization grants
- owner-capability checks are authorization decisions, not ownership changes
- a valid owner record without the required capability is insufficient for session-bound work
- a stale capability without the current owner record is invalid
- delegation must update both the owner record and the relevant capability state

Owner identity and owner capability are distinct concepts:

- the owner record identifies who currently owns the session or endpoint
- the owner capability identifies which owner-routed effects that owner may perform

The runtime must validate both. Ownership alone is not a sufficient authorization check.

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

The sanctioned session-mutation entrypoints are intentionally small:

- `open_owned_manifest_vm_session_admitted(...)`
- `OwnedVmSession::queue_send_bytes(...)`
- `OwnedVmSession::advance_round(...)`
- `OwnedVmSession::advance_round_with_signals(...)`
- `OwnedVmSession::inject_blocked_receive(...)`
- `OwnedVmSession::close(...)`
- `handle_owned_vm_round(...)`

Rules:

- Production handler and service code may mutate Telltale session state only through these owner-routed APIs.
- Raw `open_manifest_vm_session_admitted(...)`, direct `start_session(...)`, and direct `end_session(...)` are runtime-internal implementation details.
- Lower-level session mutation stays quarantined to `runtime/session_ingress.rs`, `runtime/vm_host_bridge.rs`, and the choreography runtime internals that implement the ingress contract.
- New public session-mutation entrypoints require an `ARCHITECTURE.md` update and a matching CI policy update.

### Effect Path Classes

`aura-agent` classifies runtime effect paths into three ownership classes:

- `service-owned`: lifecycle, maintenance, discovery, shutdown, reactive scheduling
- `session-owned`: VM/session mutation, blocked-receive injection, owner-routed round advancement
- `capability-gated trust-boundary APIs`: commands that cross subsystem or authority boundaries and must validate current capability scope as well as owner record

Rules:

- Service-owned effects are supervised by the owning service actor and never mutate session state directly.
- Session-owned effects require the current owner record and current owner capability.
- Capability-gated trust-boundary APIs must fail closed on stale owner, stale capability, or wrong-boundary routing.
- Routing must make link boundaries explicit rather than flattening multiple protocol fragments into one ambient authority scope.

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

Telltale's canonical execution at concurrency `n = 1` is the reference behavior.
Higher concurrency is a refinement only when it stays inside the admitted
envelope relation. In practical runtime terms that means:

- `Canonical` is the correctness baseline
- `EnvelopeAdmitted` requires explicit admission and evidence
- failed admission or failed certificate validation degrades to canonical execution
- host-side optimization may not widen safety-visible behavior beyond the admitted envelope

### Envelope Admission Contract

Operational envelope admission is a runtime gate, not a comment-level convention.

The runtime must record and enforce:

- which determinism / concurrency profile was requested
- which evidence or certificate admitted the profile
- whether execution stayed canonical or entered an admitted refinement
- why fallback occurred when admission failed

This is the host-facing projection of Telltale's envelope relation:

- safety-visible observations must remain equivalent to the canonical reference
- every admitted step must have a declared witness path
- profile-side obligations must be checked before execution widens

### Current Path Classification

Canonical-only paths:

- Session ownership claim, assertion, and release.
- Canonical ingress routing for network, timer, and command events.
- Link-boundary routing and delegation ownership transfer.
- Service lifecycle, shutdown, and runtime supervision logic.
- VM policies with cooperative runtime mode:
  - `aura.vm.prod.default`
  - `aura.vm.recovery_grant.prod`
  - `aura.vm.consensus_fast_path.prod`

Envelope-admitted paths:

- VM policies that declare bounded concurrency beyond canonical execution:
  - `aura.vm.consensus_fallback.prod`
  - `aura.vm.dkg_ceremony.prod`
  - `aura.vm.sync_anti_entropy.prod`

### Current Host Bridge Rule

The owned-session host bridge remains canonical-only until threaded execution is
explicitly admitted end-to-end.

Rules:

- Policy classification is resolved from `AuraVmProtocolExecutionPolicy`.
- Non-canonical protocol policies are recorded as envelope-admitted intent.
- The host bridge currently activates explicit canonical fallback for those paths.
- Fallback is visible in runtime instrumentation and in the effective session metadata.
- No session may silently widen from canonical to threaded execution.

## Link and Delegate Boundaries

### Link Boundary

`link` is a static composition boundary.

Runtime consequences:

- Linked bundles define ownership boundaries as well as composition boundaries.
- Linked protocols remain session-disjoint unless composition explicitly shares state.
- Cross-boundary effect routing is explicit.
- Ad hoc shared mutable state across linked boundaries is forbidden.
- `link` must preserve Telltale coherence and harmony obligations at runtime, not just compile-time compatibility.

The runtime therefore models `link` through explicit boundary objects rather than ambient state.
A boundary object must carry enough information to answer:

- which bundle/fragment boundary this effect belongs to
- which owner capability scope is valid at that boundary
- whether a route crosses a boundary that requires explicit reconfiguration handling

Wrong-boundary routing is a runtime error and must be rejected before the VM observes the step.

### Delegate Boundary

`delegate` is an ownership-transfer boundary.

Runtime consequences:

- Endpoint/session ownership transfer is atomic.
- Capability/effect context transfers with the endpoint.
- Stale-owner access after delegation is forbidden.
- Ambiguous local ownership is rejected before the VM observes the transfer.
- Fragment ownership and session footprint state move with the transfer rather than lagging behind it.

Actor messaging may carry delegation requests, but it does not replace the move.
The transfer itself is an ownership handoff with stale-owner invalidation.

Transfer and attenuation are separate concepts:

- transfer changes the authoritative owner
- attenuation narrows the capability scope that moves with the new owner

If the runtime cannot state which one is happening and under which protocol rule,
it must reject the delegation path.

In concrete runtime terms, a successful delegation must move one owned bundle:

- session owner record
- owner capability
- VM fragment ownership
- runtime footprint / reconfiguration state
- delegation audit witness

If these do not move together, the transfer is incomplete and must be treated as a runtime error.

### Theorem-pack / Invariant Alignment

Telltale's theorem pack and invariant space matter at runtime.
`aura-agent` should consume them as executable gates for advanced modes rather than
as proof-only background context.

The runtime must preserve:

- coherence-sensitive session and edge state
- harmony-sensitive reconfiguration steps (`link`, `delegate`)
- adequacy-relevant observable traces
- determinism-profile obligations for admitted concurrency
- replay / communication identity stability across async ingress

Implications:

- advanced runtime modes should be capability- and evidence-gated
- missing invariant evidence must cause rejection or fallback, never silent widening
- instrumentation must make envelope admission, delegation witnesses, and fallback reconstructible

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
- [Runtime](../../docs/104_runtime.md) defines service actor patterns.

Architectural note:
- actor services are the correct abstraction for runtime supervision
- they are not the abstraction that defines session ownership transfer

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
- [Effect System](../../docs/103_effect_system.md) defines session-local VM bridge.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines canonical execution.

Architectural note:
- session ownership is explicit and singular
- delegation is modeled as a move, not as ambient access through a service actor

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
- [Runtime](../../docs/104_runtime.md) defines session management.

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
- [Effect System](../../docs/103_effect_system.md) defines composition constraints.

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
- [Formal Verification Reference](../../docs/120_verification.md) defines runtime parity lanes.
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

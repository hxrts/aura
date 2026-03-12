# Async Runtime Refactor Plan for `aura-agent`

## Status

- Drafted from architecture review on 2026-03-12
- `work/` is non-authoritative scratch, but this document is intended to drive the refactor

## Problem Statement

`aura-agent` currently has multiple async execution models at once:

- tracked tasks via `TaskRegistry`
- detached `tokio::spawn` / `spawn_local`
- service-local shutdown signals
- partially implemented `RuntimeService`
- shared mutable state guarded by `RwLock` across many managers

This makes runtime behavior hard to reason about. The failure modes are predictable:

- tasks outlive their owners
- shutdown races with background work
- start/stop interleave across clones
- panics and join failures are not surfaced
- service health lies or is approximated
- channel closure and dropped work are silently ignored

The goal is to eliminate these bugs by construction, not by adding more logging around them.

## Telltale Alignment

This refactor must align `aura-agent` with Telltale's execution model rather than
inventing a separate async architecture around it.

Key Telltale constraints:

- Canonical correctness reference is cooperative single-owner execution (`n = 1`).
- Session ownership is explicit and must remain unambiguous.
- VM/session mutation happens at an atomic step/commit boundary.
- Higher concurrency is an admitted optimization only if it stays within the
  certified concurrency envelope.
- `link` is a typed static composition boundary.
- `delegate` is a typed ownership-transfer boundary.

This means the async refactor is not only about supervised tasks. It must also:

- preserve canonical session semantics
- funnel external async I/O through authoritative ingress
- make endpoint/session ownership explicit
- make ownership transfer explicit
- keep effect routing aligned with current owner and composition boundaries
- make errors typed and attributable
- make instrumentation consistent, structured, and phase-verifiable

## Architectural Target

### Core model

`aura-agent` runtime should use a single structured concurrency model:

- Every long-lived async subsystem is an owned service actor.
- Every service actor has exactly one owned task root.
- Every child task belongs to a named task group under that service.
- No detached task may exist outside an owned task group.
- Runtime shutdown is parent-driven and hierarchical.
- Telltale session/VM state is touched only by an authoritative owner path.

### Canonical host/VM boundary

The runtime should maintain a hard boundary between host async code and Telltale
VM/session state:

- Host async code owns external I/O, buffering, retries, timers, and scheduling.
- Telltale owns session typing, step/commit atomicity, buffer discipline,
  coherence-sensitive state, and reconfiguration correctness.
- No network task, timer, or callback may directly mutate VM/session state.
- All Telltale-facing work must enter through typed ingress events.

Preferred shape:

```rust
enum SessionIngress {
    NetworkEnvelope(TransportEnvelope),
    Timer(SessionTimerEvent),
    Command(SessionCommand),
    DelegatedEndpoint(DelegationBundle),
}
```

### Session ownership model

In addition to service actors, choreography/session execution should use
authoritative ownership:

- Each active Telltale session has exactly one current owner.
- The owner may be a per-session actor or a single choreography runtime loop that
  dispatches by session.
- External events must be queued before touching session state.
- Session ownership and task ownership must remain aligned.

Preferred shape:

```rust
struct SessionHandle {
    ingress_tx: mpsc::Sender<SessionIngress>,
}

struct SessionActor {
    session_id: RuntimeSessionId,
    state: SessionState,
    ingress_rx: mpsc::Receiver<SessionIngress>,
    tasks: TaskGroup,
}
```

### Service shape

Each runtime service should have:

- a typed command channel
- a single event loop / actor task
- an internal explicit state machine
- owned child task groups for sub-loops only when necessary
- observable health derived from actual actor state

Preferred shape:

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

### Runtime lifecycle

Startup:

1. Construct runtime dependencies.
2. Start core service actors in dependency order.
3. Wait for each service to report `Running`.
4. Start optional maintenance loops as owned service children, not from runtime-global loose timers.
5. Expose public handles only after runtime reaches a consistent ready state.

Shutdown:

1. Mark runtime as stopping.
2. Reject new external work.
3. Cancel service child tasks.
4. Ask each service actor to stop.
5. Await actor termination with bounded timeout.
6. Abort only as a last-resort fallback, and record it as a runtime failure.
7. Tear down shared resources only after all owned tasks are gone.

### Concurrency envelope

The runtime should explicitly distinguish between:

- `Canonical`: exact single-owner execution matching the reference semantics.
- `EnvelopeAdmitted`: bounded concurrency that preserves the same safety-visible
  meaning as canonical execution.
- `Fallback`: immediate degradation to canonical execution whenever admission or
  validation for concurrent execution fails.

Rules:

- Correctness must never depend on uncontrolled host scheduling.
- Higher concurrency may be used only for disjoint or otherwise admitted work.
- If the runtime cannot show that a path is envelope-safe, it must serialize it.
- Threaded/parallel execution is a refinement, not the semantic baseline.

### Link boundaries

`link` should shape runtime structure:

- Linked protocols are static composition boundaries.
- Linked protocol components should remain session-disjoint unless composition
  explicitly says otherwise.
- Effect routing and ownership must respect those boundaries.
- Shared mutable state across linked protocols should be exceptional and explicit.

### Delegation and ownership transfer

`delegate` should shape runtime structure:

- Endpoint transfer is an ownership transfer, not just a message.
- Delegation must move endpoint/session ownership atomically.
- Ownership transfer must include the effect/capability context needed by the new owner.
- Old owners must be unable to act on delegated endpoints after commit.
- Ambiguous or overlapping endpoint ownership is illegal.

### Typed errors and instrumentation

The runtime should standardize on typed failures and structured diagnostics:

- Runtime, service, session, routing, and delegation failures should use typed errors.
- Concurrency-envelope admission/fallback should emit typed outcomes.
- Instrumentation should use a shared event taxonomy for startup, shutdown,
  task failure, ownership transfer, fallback, and invariant rejection.
- Supervised tasks, services, sessions, and delegations should carry stable names
  and correlation fields.

### State ownership

Target ownership model:

- Service-local mutable state lives inside the service actor task.
- Cross-task shared state should be exceptional, minimized, and justified.
- `tokio::sync::RwLock` should not be the default service pattern.
- `parking_lot` locks are only acceptable for short, synchronous, non-async data structures.

If a service needs coordination across tasks, prefer:

- actor message passing
- `watch` for state snapshots
- `mpsc` for commands
- `oneshot` for request/response
- per-session ingress queues

Avoid:

- multi-writer service state behind `Arc<RwLock<_>>`
- read-check-write state transitions across multiple `await` points
- direct async mutation of Telltale session state from arbitrary tasks

### Spawn policy

Allowed:

- spawning through one supervised runtime task API
- spawning child tasks from inside a service actor via its owned task group

Forbidden:

- raw `tokio::spawn`
- raw `spawn_local`
- fire-and-forget background work

These should be enforced with lints, not convention.

## Required Invariants

The refactor is complete only when these invariants hold:

- [ ] No raw `tokio::spawn` or `spawn_local` in production `aura-agent` runtime code
- [ ] Every long-lived task has a named owner
- [ ] Every service has one authoritative start path and one authoritative stop path
- [ ] Start/stop are serialized and idempotent
- [ ] Runtime shutdown cancels tasks before tearing down dependencies
- [ ] No service reports placeholder health
- [ ] No critical send/join/cancellation errors are silently dropped
- [ ] Runtime state machines enforce illegal transition rejection in release builds
- [ ] Public service APIs do not bypass lifecycle ownership
- [ ] Detached maintenance loops are removed
- [ ] No network/timer/callback path touches VM/session state except through authoritative ingress
- [ ] Each active endpoint/session has exactly one current owner
- [ ] Delegation transfers ownership atomically and rejects ambiguous ownership
- [ ] Linked protocol boundaries remain explicit in ownership and effect routing
- [ ] Higher-concurrency execution either proves/adheres to the admitted envelope or falls back to canonical execution
- [ ] Effect execution touching session state is routed through the current owner
- [ ] Runtime/service/session/delegation failures use typed errors instead of free-form strings
- [ ] Instrumentation events are consistent across services, sessions, and ownership boundaries

## Enforcement Mechanisms

These are part of the architecture, not optional cleanup:

- [ ] Add `clippy::disallowed_methods` rules for `tokio::spawn` and `spawn_local` in `aura-agent` production code
- [ ] Add compile-time allowed wrapper API for supervised task spawning
- [ ] Add service-state transition helpers that validate transitions in all builds
- [ ] Add test helpers that assert no runtime-owned task leaks after shutdown
- [ ] Add deterministic shutdown tests for all long-lived services
- [ ] Add restart tests for all startable/stoppable services
- [ ] Add at least one concurrency model check (`loom` where practical) for the core task/lifecycle primitives
- [ ] Add a banned-pattern check for direct async mutation of choreography/session state outside the owner path
- [ ] Add parity tests that compare envelope-admitted execution against canonical execution
- [ ] Add ownership-transfer tests for delegated endpoints
- [ ] Add replay/checkpoint tests ensuring no hidden background tasks mutate restored sessions
- [ ] Add lint or review gates discouraging new `String`-typed runtime error surfaces
- [ ] Add a shared instrumentation schema or event taxonomy for runtime lifecycle and concurrency events

## Phased Work Plan

## Phase 0: Define the Runtime Contract

- [ ] Write an authoritative `aura-agent` runtime design note for structured concurrency and service ownership
- [ ] Write an authoritative host/VM contract for Telltale integration
- [ ] Define the allowed async primitives and ownership model
- [ ] Define the lifecycle state machine shared by runtime services
- [ ] Define what counts as a fatal runtime invariant violation
- [ ] Define standard service states: `New`, `Starting`, `Running`, `Stopping`, `Stopped`, `Failed`
- [ ] Define a standard command/reply pattern for service actors
- [ ] Define session ownership rules and owner-visible state transitions
- [ ] Define the canonical ingress model for network, timer, and external events
- [ ] Define concurrency profiles: `Canonical`, `EnvelopeAdmitted`, `Fallback`
- [ ] Define how `link` and `delegate` map to runtime ownership and effect routing
- [ ] Define the runtime typed-error taxonomy
- [ ] Define the runtime instrumentation/event taxonomy and required fields
- [ ] Run targeted runtime-contract validation/tests and ensure they are green

Success criteria:

- A new contributor can answer:
  - who owns this task?
  - who owns this session/endpoint?
  - how does this service start?
  - how does this service stop?
  - what happens if a child task fails?
  - what is the only legal path from network event to VM/session mutation?
  - when may the runtime use concurrency beyond canonical execution?

## Phase 1: Replace `TaskRegistry` with Structured Task Supervision

- [ ] Remove the current passive handle bag model from `TaskRegistry`
- [ ] Introduce `TaskSupervisor` / `TaskGroup` with:
  - task naming
  - cancellation propagation
  - join tracking
  - panic reporting
  - bounded shutdown
- [ ] Support both native and wasm through the same ownership model
- [ ] Add APIs for:
  - `spawn_child`
  - `spawn_periodic`
  - `shutdown_gracefully`
  - `abort_remaining`
  - `wait_for_idle`
- [ ] Ensure child tasks cannot outlive their task group unless explicitly detached by type, and do not provide such a detach API for runtime code
- [ ] Add typed task-supervision errors for panic, cancellation, timeout, and forced abort
- [ ] Add consistent instrumentation for task spawn, task completion, task failure, and forced abort
- [ ] Run targeted supervision/lifecycle tests and ensure they are green

Success criteria:

- No runtime background task can be created without an owning task group
- Panics and join failures are surfaced as structured runtime errors
- Shutdown can await all owned children

## Phase 1A: Canonical Ingress and Session Ownership

- [ ] Introduce a choreography/session ingress layer for network, timer, and command events
- [ ] Ensure all Telltale-facing work enters through that ingress layer
- [ ] Introduce authoritative session ownership records
- [ ] Route session-bound work only through the current owner
- [ ] Remove direct handler/task access to session state outside the owner path
- [ ] Ensure session/task ownership stay aligned
- [ ] Add typed ownership and ingress errors for stale owner, missing owner, and invalid ingress routing
- [ ] Add consistent instrumentation for ingress receipt, ingress drop, owner assignment, and owner rejection
- [ ] Run targeted session-ingress/ownership tests and ensure they are green

Success criteria:

- Network receives, timers, and callbacks no longer mutate session state directly
- Every active session/endpoint has one authoritative owner
- Session correctness no longer depends on ambient host task scheduling

## Phase 1B: Concurrency Envelope Profiles and Fallback

- [ ] Define which runtime paths are canonical-only
- [ ] Define which runtime paths are envelope-admitted
- [ ] Implement explicit fallback to canonical execution when admission/certification fails
- [ ] Add runtime visibility for concurrency profile selection
- [ ] Add parity tests comparing envelope-admitted execution to canonical execution
- [ ] Add typed errors for envelope admission denial and fallback activation
- [ ] Add instrumentation for profile selection, admission failure, and canonical fallback
- [ ] Run targeted canonical-vs-envelope parity tests and ensure they are green

Success criteria:

- Canonical execution is the reference path
- Higher concurrency is treated as a refinement only
- Unsafe or uncertified concurrency falls back automatically

## Phase 2: Make `RuntimeService` Real or Remove It

- [ ] Remove partial / placeholder lifecycle implementations
- [ ] Redesign `RuntimeService` so it is the only lifecycle API used by runtime startup/shutdown
- [ ] Make health authoritative and derived from actor state
- [ ] Require `start()` to fully initialize the service
- [ ] Require `stop()` to fully quiesce the service
- [ ] Remove side-channel startup APIs like “start this separately with time effects”
- [ ] Encode dependencies in one runtime startup graph
- [ ] Replace stringly lifecycle failures with typed service lifecycle errors
- [ ] Add consistent instrumentation for service lifecycle transitions
- [ ] Run targeted service lifecycle tests and ensure they are green

Success criteria:

- There is exactly one startup path per service
- There is exactly one shutdown path per service
- The runtime no longer has special-case lifecycle code for individual services unless that service model explicitly requires it

## Phase 3: Convert Long-Lived Managers to Actor Services

Priority order:

- [ ] `RendezvousManager`
- [ ] `SyncServiceManager`
- [ ] LAN discovery
- [ ] LAN transport listener
- [ ] reactive pipeline / scheduler
- [ ] receipt cleanup
- [ ] ceremony timeout cleanup

Per service conversion tasks:

- [ ] move mutable state into actor-owned state
- [ ] replace public mutation methods with typed commands
- [ ] serialize start/stop transitions inside the actor
- [ ] move service-local maintenance loops under the actor task group
- [ ] remove `Arc<RwLock<State>>` where not strictly required
- [ ] remove service-local `watch` shutdown patterns if task-group cancellation replaces them
- [ ] add explicit `Failed` state and propagate it
- [ ] introduce typed per-service operational errors
- [ ] add consistent service-level instrumentation fields and events
- [ ] run targeted manager/service actor tests and ensure they are green

Success criteria:

- Service correctness no longer depends on callers avoiding concurrent `start()` or `stop()`
- Service operations are race-safe by construction
- Background loops cannot observe partially torn-down service state

## Phase 3A: Link Boundary Refactor

- [ ] Identify all linked protocol boundaries in `aura-agent`
- [ ] Make static composition boundaries explicit in runtime ownership and routing
- [ ] Ensure linked components remain session-disjoint unless composition explicitly shares state
- [ ] Replace ad hoc cross-protocol shared mutable state with explicit composition interfaces
- [ ] Route effects across linked boundaries through typed boundary objects
- [ ] Add typed link/composition boundary errors
- [ ] Add instrumentation for linked-boundary routing and composition rejection
- [ ] Run targeted linked-protocol boundary tests and ensure they are green

Success criteria:

- Linked protocols can be reasoned about independently
- Runtime ownership mirrors static composition boundaries
- Cross-protocol interference is explicit and minimal

## Phase 3B: Delegation Ownership Transfer Refactor

- [ ] Introduce typed endpoint/session ownership transfer objects
- [ ] Implement atomic delegation handoff
- [ ] Ensure delegated capability/effect context moves with the endpoint
- [ ] Prevent old owners from acting on delegated endpoints after handoff
- [ ] Add drain/cancel/complete behavior around delegation transfer
- [ ] Add typed delegation transfer errors
- [ ] Add instrumentation for delegation start, commit, rollback, and stale-owner rejection
- [ ] Run targeted delegation handoff tests and ensure they are green

Success criteria:

- Delegated endpoints never have overlapping owners
- Handoff is atomic and testable
- Post-delegation stale access is impossible or rejected

## Phase 4: Remove Detached Work from Handlers and Runtime Glue

- [ ] Audit all raw spawns in `aura-agent`
- [ ] Eliminate fire-and-forget work in recovery flows
- [ ] Eliminate nested raw spawns in LAN accept loops
- [ ] Eliminate raw spawns in rendezvous discovery callbacks
- [ ] Route asynchronous follow-up work through owned service/task-group APIs
- [ ] Ensure request-scoped work is awaited or explicitly registered under a task owner
- [ ] Replace detached-path error swallowing with typed surfaced failures
- [ ] Add instrumentation proving follow-up work is supervised
- [ ] Run targeted raw-spawn-elimination regression tests and ensure they are green

Success criteria:

- Production `aura-agent` runtime code contains zero detached tasks
- Every async branch is either awaited or supervised

## Phase 5: Fix Runtime Shutdown Ordering

- [ ] Make runtime shutdown stateful and idempotent
- [ ] Stop accepting new public operations before service stop begins
- [ ] Cancel runtime-owned task groups before shared dependency teardown
- [ ] Ensure reactive pipeline shutdown happens before dependent signal resources are destroyed
- [ ] Ensure transport listeners stop accepting before effect system teardown
- [ ] Add per-service stop timeouts with failure reporting
- [ ] Add typed shutdown and quiescence errors
- [ ] Add instrumentation for shutdown progression and timeout escalation
- [ ] Run targeted shutdown-under-load and teardown-order tests and ensure they are green

Success criteria:

- Repeated shutdown calls are harmless
- Shutdown during load does not leave orphan tasks
- No post-shutdown background log noise remains from owned services

## Phase 6: Make Errors Observable Instead of Best-Effort

- [ ] Replace `let _ = ...` on important async operations with explicit handling
- [ ] Classify dropped update notifications vs fatal work loss
- [ ] Return structured errors for failed fact publication
- [ ] Treat failed finalization / `end_session()` / task joins as explicit failures where correctness depends on them
- [ ] Add a runtime error sink or diagnostics channel for supervised tasks
- [ ] Replace remaining stringly async/runtime errors with typed error enums
- [ ] Standardize structured diagnostics emission for surfaced failures
- [ ] Run targeted error-propagation and diagnostics tests and ensure they are green

Success criteria:

- Silent loss of important work is not possible
- Runtime operators and tests can observe the cause of failure

## Phase 7: Strengthen Invariant Enforcement

- [ ] Replace debug-only state validation for runtime state machines with always-on transition validation
- [ ] Encode legal transitions in helper types or transition functions
- [ ] Prefer exhaustive enums for state over boolean flag combinations
- [ ] Remove “status + optional handles/service” combinations that can drift independently
- [ ] Collapse impossible combinations into typed states
- [ ] Add always-on validation for session ownership and delegation invariants
- [ ] Add always-on validation for canonical vs envelope-admitted execution mode transitions
- [ ] Add typed invariant-violation errors where recovery is possible
- [ ] Add instrumentation for invariant rejection paths
- [ ] Run targeted invariant enforcement tests and ensure they are green

Examples:

- `Stopped` should not coexist with a live handle
- `Running` should imply actor alive and command channel available
- `Stopping` should imply cancellation issued and finalization in progress
- delegated endpoint ownership should never be ambiguous
- envelope-admitted execution should never proceed without explicit admission

Success criteria:

- Invalid runtime states are unrepresentable or rejected immediately
- Release builds retain the safety checks required for correctness

## Phase 7A: Effect Routing by Current Owner

- [ ] Classify effects into external I/O, session-bound, and runtime-global classes
- [ ] Require session-bound effects to execute only under the current owner
- [ ] Add runtime checks rejecting stale or non-owner effect execution
- [ ] Ensure delegated endpoints reroute session-bound effects to the new owner
- [ ] Ensure linked protocol effect routing respects composition boundaries
- [ ] Add typed effect-routing errors for stale owner, wrong boundary, and invalid delegation target
- [ ] Add instrumentation for owner-routed effect execution and rejection
- [ ] Run targeted effect-routing tests and ensure they are green

Success criteria:

- Session-bound effects are owner-routed by construction
- Delegation updates effect routing automatically
- Linked protocols do not leak effect ownership across boundaries

## Phase 8: Simplify Public API Boundaries

- [ ] Ensure public `AuraAgent` APIs interact with services through stable handles, not internal mutable managers
- [ ] Remove legacy convenience APIs that bypass actor ownership
- [ ] Remove obsolete builders, helpers, and partial lifecycle hooks
- [ ] Remove duplicate maintenance startup paths from builder/runtime glue
- [ ] Keep service surfaces narrow and intention-revealing
- [ ] Ensure public APIs expose typed runtime/service errors
- [ ] Ensure public APIs emit consistent instrumentation context or correlation metadata
- [ ] Run targeted public API/runtime integration tests and ensure they are green

Success criteria:

- Public API does not expose internals that allow lifecycle misuse
- Legacy code paths are deleted, not deprecated in place

## Phase 9: Verification and Testing

### Concurrency tests

- [ ] Add start/start race tests per service
- [ ] Add start/stop race tests per service
- [ ] Add stop/stop idempotence tests per service
- [ ] Add shutdown-under-load tests
- [ ] Add restart-after-stop tests
- [ ] Add panic-in-child-task supervision tests

### Deterministic runtime tests

- [ ] Assert no supervised tasks remain after runtime shutdown
- [ ] Assert no messages are processed after service stop acknowledgement
- [ ] Assert no facts are dropped during orderly shutdown
- [ ] Assert reactive pipeline drains or fails explicitly
- [ ] Assert canonical execution remains stable under replay
- [ ] Assert checkpoint/restore does not permit hidden post-restore mutation by orphan tasks
- [ ] Assert delegated endpoint ownership is preserved across restore/replay

### Tooling

- [ ] Enable lint gate preventing raw spawn usage
- [ ] Add tests or scripts that fail if banned async patterns reappear
- [ ] Add a CI lane for targeted async-correctness checks
- [ ] Add CI lanes comparing canonical vs envelope-admitted behavior where concurrency is enabled
- [ ] Add CI checks for typed-error and instrumentation schema consistency

### Model checking

- [ ] Introduce `loom` tests for the core supervision/lifecycle primitives
- [ ] Model:
  - start/stop serialization
  - cancellation propagation
  - no lost wakeups for shutdown / completion

Success criteria:

- Async correctness has dedicated CI coverage
- The most failure-prone lifecycle primitives are model-checked

## Concrete Task Inventory

## A. Runtime foundation

- [ ] Create `runtime/task_supervisor.rs`
- [ ] Create `runtime/service_actor.rs` or equivalent shared actor framework
- [ ] Create typed lifecycle state and transition helpers
- [ ] Add structured runtime diagnostics for task failure and forced abort
- [ ] Create choreography/session ingress and ownership primitives
- [ ] Create typed delegation handoff primitives
- [ ] Create concurrency-profile admission/fallback primitives
- [ ] Create shared runtime error enums/modules
- [ ] Create shared runtime instrumentation/event helpers

## B. Refactor runtime startup/shutdown

- [ ] Rewrite `RuntimeSystem::start_services`
- [ ] Rewrite `RuntimeSystem::shutdown`
- [ ] Remove ad hoc maintenance-task startup from `RuntimeSystem`
- [ ] Move maintenance ownership into services
- [ ] Route choreography/session startup through canonical ingress ownership
- [ ] Add canonical fallback path for non-admitted concurrency

## C. Refactor services

- [ ] Rewrite `RendezvousManager` as actor service
- [ ] Rewrite `SyncServiceManager` as actor service
- [ ] Rewrite LAN discovery ownership around task groups
- [ ] Rewrite LAN transport listener ownership around task groups
- [ ] Rewrite reactive pipeline ownership and shutdown
- [ ] Update `ReceiptManager` lifecycle to be authoritative
- [ ] Update `CeremonyTracker` lifecycle to be authoritative
- [ ] Refactor choreography/session runtime around authoritative session owners
- [ ] Refactor delegation handling around atomic handoff
- [ ] Refactor linked protocol boundaries around explicit routing/ownership
- [ ] Refactor service/session APIs to return typed errors and emit consistent instrumentation

## D. Remove legacy patterns

- [ ] Delete raw-spawn call sites in production runtime code
- [ ] Delete placeholder health implementations
- [ ] Delete partial lifecycle APIs
- [ ] Delete duplicate shutdown signaling mechanisms no longer needed
- [ ] Delete legacy code paths rather than leaving both architectures in place
- [ ] Delete direct session mutation paths outside canonical ingress/owner routing
- [ ] Delete ambiguous endpoint ownership patterns
- [ ] Delete concurrency paths that are not envelope-admitted or cannot fall back safely
- [ ] Delete legacy stringly runtime error paths and ad hoc instrumentation formats

## E. Tooling and tests

- [ ] Add lint enforcement
- [ ] Add leak-detection tests
- [ ] Add service lifecycle race tests
- [ ] Add loom coverage for supervision primitives

## Non-Goals

- Preserving old lifecycle APIs
- Preserving backwards compatibility for internal runtime wiring
- Minimizing diff size
- Keeping every current service abstraction if it no longer improves correctness

## Performance Constraints

The target architecture should preserve performance by:

- using one actor task per long-lived service, not one task per tiny action
- using channels for ownership transfer instead of broad lock contention
- keeping hot-path data local to actor tasks where possible
- avoiding unnecessary serialization or heap churn in per-message paths
- keeping canonical execution cheap and predictable
- using concurrency only where it is envelope-admitted and worth the complexity

Acceptable tradeoff:

- slightly more coordination overhead during startup/shutdown
- stricter ingress and ownership routing around choreography/session state

Not acceptable:

- correctness improvements that depend on globally serializing unrelated runtime work
- host-side concurrency that widens the admitted Telltale behavior envelope
- effect routing that ignores current endpoint/session ownership

## Definition of Done

The async refactor is done when:

- [ ] Production `aura-agent` has no detached background tasks
- [ ] Each long-lived service is actor-owned and supervised
- [ ] Startup and shutdown are deterministic and test-covered
- [ ] Task failure is surfaced, not hidden
- [ ] Lifecycle and health are authoritative
- [ ] Legacy lifecycle code has been removed
- [ ] CI prevents regression into unstructured async patterns
- [ ] Session/endpoint ownership is explicit and test-covered
- [ ] Delegation handoff is atomic and test-covered
- [ ] Concurrency beyond canonical execution is explicitly envelope-admitted or falls back safely
- [ ] Session-bound effects are routed only through current owners
- [ ] Runtime and service failures are typed and test-covered
- [ ] Instrumentation is consistent enough to trace ownership, fallback, and shutdown behavior end-to-end

## Final Success Criteria

We should be able to say all of the following with evidence:

- “Every runtime task has an owner.”
- “Every active session and endpoint has exactly one current owner.”
- “No service can be started twice through a race.”
- “Shutdown cannot race with active background loops in a way that mutates torn-down state.”
- “A child task panic is observable and attributable.”
- “The runtime has tests that prove no task leaks remain after shutdown.”
- “The architecture makes the bad async patterns impossible or lint-failing by default.”
- “No network event, timer, or callback can touch VM/session state except through canonical ingress.”
- “Delegation moves ownership atomically and stale owners cannot keep acting.”
- “Higher-concurrency execution either stays inside the admitted concurrency envelope or degrades to canonical execution.”
- “Failures are typed enough that callers and tests can distinguish lifecycle, ownership, delegation, routing, and shutdown errors.”
- “Instrumentation is consistent enough to reconstruct task ownership, session ownership, fallback decisions, and shutdown progression.”

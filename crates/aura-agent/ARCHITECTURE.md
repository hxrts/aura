# Aura Agent (Layer 6)

## Purpose

Production runtime composition and effect system assembly for authority-based identity management. Owns structured concurrency, service lifecycle, session ownership, effect registry, builder infrastructure, and choreography adapters.

## Scope

| Belongs here | Does not belong here |
|---|---|
| Runtime assembly and effect composition | New effect implementations (aura-effects) |
| Service actor lifecycle and supervision | Multi-party coordination (aura-protocol) |
| Session ownership and ingress routing | Application-level workflow logic (aura-app) |
| Choreography adapter wiring | Bridge schema transformations (aura-quint) |
| Builder infrastructure (CLI, iOS, Android, Web) | Stateless single-party handlers (aura-effects) |
| Structured concurrency and task supervision | Imports from Layers 1-5 back into this crate |

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| Consumes | `aura-core` (L1) | Effect traits, domain types, crypto utilities |
| Consumes | `aura-effects` (L3) | Stateless handler implementations |
| Consumes | `aura-protocol` (L4) | Protocol coordination |
| Consumes | L2 domain crates | Journal, authorization, transport, etc. |
| Consumes | L5 feature crates | End-to-end protocols |
| Produces | `AgentBuilder`, `AuraAgent` | Runtime entry points |
| Produces | `EffectContext`, `EffectRegistry` | Effect composition |
| Produces | `AuraEffectSystem` | Subsystems: Crypto, Transport, Journal |
| Produces | Service APIs | Session, Auth, Recovery, SyncManager |
| Produces | `RuntimeSystem`, `LifecycleManager` | Lifecycle management |

## Key Modules

- `core/`: Public API (`AgentBuilder`, `AuraAgent`, `AuthorityContext`).
- `builder/`: Platform-specific preset builders (CLI, iOS, Android, Web).
- `runtime/`: Service actors, subsystems, choreography adapters.
- `handlers/`: Service API implementations (auth, session, recovery, etc.).
- `reactive/`: Signal-based notification and scheduling.

## Invariants

Summary:

- All production async work uses structured concurrency with explicit task ownership.
- External events reach session state only through typed ingress and the current owner.
- Each active session has exactly one local owner at any time.
- Runtime composition assembles existing handlers; it does not create new effects or protocol logic.
- Runtime telltale integration consumes bridge artifacts but does not redefine bridge schema.
- Certain ownership/session violations are fatal and unrecoverable.
- Authority-first design: all operations scoped to specific authorities.
- Lazy composition: effects assembled on-demand.
- Mode-aware execution: production, testing, and simulation use same API.
- For shared semantic flows, `aura-agent` is the primary `ActorOwned` crate. It may own long-lived mutable async runtime state, but it must not leak that ownership into frontend-local semantic lifecycle authorship.

### InvariantStructuredConcurrency

All production async work uses structured concurrency with explicit task ownership.

Enforcement locus:
- `src/runtime/` service actors own their task groups.
- `TaskGroup` enforces parent-child relationships.

Failure mode:
- Detached tasks outlive their owner and mutate torn-down resources.
- Shutdown leaves orphan tasks running.

Verification hooks:
- `just ci-actor-lifecycle`
- `just test-crate aura-agent`

Contract alignment:
- [Runtime](../../docs/104_runtime.md) defines service actor patterns.
- Actor services are the correct abstraction for runtime supervision; they are not the abstraction that defines session ownership transfer.

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
- Session ownership is explicit and singular; delegation is modeled as a move, not as ambient access through a service actor.

### InvariantSessionOwnership

Each active session has exactly one local owner at any time.

Enforcement locus:
- Ownership transitions are explicit state machine transitions.
- Delegation commits atomically.

Failure mode:
- Overlapping owners mutate the same session.
- Stale owner access after delegation.

Verification hooks:
- `just ci-async-concurrency-envelope`
- `just ci-move-semantics`
- `just test-crate aura-agent`

Contract alignment:
- [Runtime](../../docs/104_runtime.md) defines session management.

### InvariantRuntimeCompositionBoundary

Runtime composition assembles existing effect handlers without introducing new effect implementations or protocol logic.

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
- [System Architecture](../../docs/001_system_architecture.md) defines layer boundaries.
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

## Structured Concurrency Model

`aura-agent` uses structured concurrency as the only production async model. This model is intentionally split:

- actor services solve long-lived runtime supervision and lifecycle
- move semantics solve session and endpoint ownership transfer

Do not collapse those into one generic async pattern.

Rules:

- Every long-lived async subsystem has one named owner.
- Every owner has one rooted task group.
- Child tasks belong to exactly one task group.
- Detached fire-and-forget tasks are forbidden in production runtime code.
- Shutdown is hierarchical and parent-driven.

See [Runtime](../../docs/104_runtime.md) §Service Actor Patterns for the actor struct examples, command/reply pattern, and async primitive preferred/forbidden lists.

### Concurrency Profiles

Three runtime concurrency profiles for choreography work:

- **Canonical**: Exact single-owner reference path (concurrency n=1).
- **EnvelopeAdmitted**: Disjoint or admitted work preserving safety-visible meaning.
- **Fallback**: Immediate degradation to canonical execution when envelope admission fails.

See [Runtime](../../docs/104_runtime.md) §Concurrency Profiles for the full contract, envelope admission rules, and current path classification.

## Session Ownership

Telltale-facing session state follows strict ownership rules. This is the move-semantics side of the runtime design.

Rules:

- Each active session or fragment has exactly one current local owner.
- The owner is either a per-session actor or an authoritative choreography runtime loop.
- Network, timer, and external events are queued before touching session state.
- Session ownership and task ownership move together.
- Session-bound effects execute only under the current owner capability.

The current owner may be hosted by an actor, but ownership itself remains a single-owner move boundary, not a shared mutable actor coordination pattern.

### Owner Record vs Owner Capability

- The owner record answers who currently owns the session or endpoint.
- The owner capability answers what that owner may currently do.
- Delegation must update both the owner record and the relevant capability state.
- A valid owner record without the required capability is insufficient.

### Effect Path Classes

Three ownership classes for runtime effect paths:

- `service-owned`: lifecycle, maintenance, discovery, shutdown, reactive scheduling.
- `session-owned`: VM/session mutation, blocked-receive injection, owner-routed round advancement.
- `capability-gated trust-boundary APIs`: commands crossing subsystem or authority boundaries requiring capability validation.

Service-owned effects never mutate session state directly. Session-owned effects require both current owner record and current owner capability. Capability-gated trust-boundary APIs fail closed on stale owner, stale capability, or wrong-boundary routing.

Reactive signal views are `Observed` bridges, not alternate owners. They may
apply authoritative facts to known entities, but they may not fabricate
canonical channel or invitation metadata from weaker facts such as membership
events or raw identifiers. If runtime acceptance or reconciliation needs to
materialize canonical metadata, one explicit owned handler path must do that
end to end before reactive views are allowed to enrich the projection.

Runtime bridge lookup follows the same strong-ref rule:

- context-scoped routing may use only descriptors bound to the requested
  context, not cross-context or "any descriptor" fallback
- channel-context answers must come from materialized runtime-owned context
  state, not invitation storage or local chat-fact repair
- name lookup may identify only already-materialized channels; it may not
  upgrade imported invitation metadata into an authoritative binding
- invitation-triggered home signal materialization must flow through the
  declared reactive home-signal owner path in `reactive/app_signal_views.rs`,
  not through handler-local signal read/patch/emit logic

## Canonical Host/VM Boundary

`aura-agent` aligns with Telltale's canonical execution model. The only legal path from external async input to session mutation:

1. External event received by host runtime.
2. Host runtime converts it to typed ingress.
3. Ingress routed to the current authoritative owner.
4. Owner drives VM/session work at the sanctioned boundary.

Enforcement notes:

- Raw VM admission helpers stay inside the runtime boundary; higher layers use owned ingress/session wrappers.
- VM fragment ownership mutation stays inside runtime-owned surfaces.
- Link/delegate orchestration uses `ReconfigurationManager`; `ReconfigurationController` remains an internal runtime primitive.

See [Runtime](../../docs/104_runtime.md) §Link and Delegate Boundaries for the full link/delegate boundary contract, delegation bundle composition, and theorem-pack alignment rules.

## Telltale Bridge Ownership

- `aura-agent` owns runtime admission wiring and choreography backend selection.
- `aura-agent` owns telltale runtime parity test lanes and scenario contract gates.
- `aura-agent` must not own bridge schema transformations that belong in `aura-quint`.

## Cross-Crate API Boundary

Other crates interact with `aura-agent` through sanctioned public APIs only.

- No direct imports of internal runtime modules from other crates.
- No bypass of `AuraAgent` or sanctioned public handles for runtime ownership-sensitive work.
- No cross-crate reach-in to mutate service or session internals.

Enforced by architectural policy gates and visibility rules.

## Ownership Model

Ownership categories follow [docs/122_ownership_model.md](../../docs/122_ownership_model.md).

### Ownership Inventory

| Path | Category | Authoritative owner | May mutate | Observe only |
|------|----------|---------------------|------------|--------------|
| Runtime services and long-lived async coordinators | `ActorOwned` | service actor / rooted task group | owning service module and its typed command ingress | `aura-app`, frontends, harness |
| Session / endpoint / fragment transfer surfaces | `MoveOwned` | current owner record and capability scope | sanctioned delegation / transfer APIs only | projections, diagnostics, harness |
| Runtime-facing readiness and lifecycle state consumed by shared semantic flows | `ActorOwned` | runtime readiness/lifecycle coordinator | owning runtime coordinator and sanctioned hooks | `aura-app`, frontends, harness |
| Frontend-visible projections and facts | `Observed` | downstream of runtime/workflow ownership | projection reducers/exporters only | frontends, harness |
| Reducers, validators, typed contracts | `Pure` | compile-time | n/a | all layers |

### Concrete Boundary Map

- `ActorOwned`
  - `runtime/services/sync_manager.rs` via `SyncServiceManager`
  - `runtime/services/rendezvous_manager.rs` via `RendezvousManager`
  - service-local supervision rooted in `task_registry.rs`
- `MoveOwned`
  - `runtime/services/reconfiguration_manager.rs` via `ReconfigurationManager` and `SessionDelegationTransfer`
  - `runtime/session_ingress.rs` via `RuntimeSessionOwner`
  - `runtime/subsystems/vm_fragment.rs` via `VmFragmentRegistry`
- `CapabilityGated`
  - runtime reconfiguration admission in `runtime/services/reconfiguration_manager.rs`
  - session-owner capability checks in `runtime/session_ingress.rs`
  - runtime-facing readiness/lifecycle publication through sanctioned runtime coordinator paths

### Capability-Gated Points

- Runtime-owned readiness and lifecycle publication must flow through sanctioned coordinator APIs and capability checks rather than arbitrary handlers.
- Session and endpoint mutation must validate both current owner record and current owner capability.
- Runtime helper modules may stage work, but they may not author frontend- or harness-visible semantic truth without the owning capability.

Rules:

- Do not replace `MoveOwned` session/delegation transfer with an actor mailbox.
- Do not route long-lived mutable service ownership through move-owned handles.
- Do not author runtime-visible mutation/publication without the relevant capability gate.
- Public service APIs must take shared runtime-owned `TaskSupervisor` /
  `CeremonyRunner` roots from the runtime graph; they must not construct
  private owner trees internally.
- Service health must degrade structurally when maintenance obligations fail;
  loop-local logging is not a substitute for degraded lifecycle state.
- Inbound moderation and membership gating must fail closed when home state is
  unavailable, ambiguous, or missing authoritative roster membership; observed
  chat projection and current-home fallback may not authorize message
  admission.
- Runtime service APIs must not wait on generic "next reactive update" signals
  or return optimistic domain sketches when they claim to return a postcondition;
  converted chat/message/group queries and mutations reduce committed facts
  directly before returning.
- Converted runtime choreography start paths must surface typed start-failure
  reasons such as duplicate active session or stale task binding; caller retry
  policy must bind to that typed reason rather than parsing error strings.

### Verification Hooks

- `cargo check -p aura-agent`
- `just ci-actor-lifecycle`
- `just ci-move-semantics`
- `just ci-capability-boundaries`
- targeted runtime/service tests via `cargo test -p aura-agent`

Architecture/tooling split: runtime ownership boundaries that can be closed by types, visibility, or compile-fail tests should not rely primarily on shell grep checks. `just check-arch` remains the right gate for repo-wide runtime/integration invariants.

## Testing

### Strategy

Structured concurrency, ownership boundaries, and runtime composition are the primary concerns. Compile-fail tests in `tests/ui/` enforce type-level boundaries. Integration tests verify service lifecycle, session management, protocol choreography, and reactive scheduling.

### Commands

```
cargo test -p aura-agent
cargo test -p aura-agent --test compile_fail   # compile-fail boundary tests
```

### Coverage Matrix

| What breaks if wrong | Invariant | Test location | Status |
|---------------------|-----------|--------------|--------|
| Runtime missing required effect handler | InvariantRuntimeCompositionBoundary | `tests/ui/missing_*.rs` (6 compile-fail) | Covered |
| Private runtime internals accessible | InvariantCanonicalIngress | `tests/ui/*_private.rs` (4 compile-fail) | Covered |
| VM fragment registry leaked | InvariantSessionOwnership | `tests/ui/vm_fragment_registry_private.rs` | Covered |
| Service actor handle leaked | InvariantStructuredConcurrency | `tests/ui/service_actor_handle_private.rs` | Covered |
| VM concurrent contract violated | InvariantStructuredConcurrency | `tests/telltale_vm_concurrent_contracts.rs` | Covered |
| VM parity diverges across profiles | InvariantBridgeOwnershipAgent | `tests/telltale_vm_parity.rs` | Covered |
| VM scenario contract fails | InvariantBridgeOwnershipAgent | `tests/telltale_vm_scenario_contracts.rs` | Covered |
| Production manifest fails admission | InvariantRuntimeCompositionBoundary | `tests/production_manifest_admission.rs` | Covered |
| FRP scheduler glitches | — | `tests/frp_glitch_freedom_test.rs` | Covered |
| Journal integration roundtrip | — | `tests/journal_integration_test.rs` | Covered |
| Session service lifecycle wrong | InvariantSessionOwnership | `tests/session_service_test.rs` | Covered |
| Auth service flow broken | — | `tests/auth_service_test.rs` | Covered |
| Recovery service flow broken | — | `tests/recovery_service_test.rs` | Covered |
| Threshold signing E2E fails | — | `tests/threshold_signing_e2e.rs` | Covered |
| Runtime bridge channel resolution wrong | InvariantBridgeOwnershipAgent | `tests/runtime_bridge_channel_resolution.rs` | Covered |
| Bootstrap preconditions not enforced | InvariantRuntimeCompositionBoundary | `tests/bootstrap_required.rs` | Covered |
| Reactive scheduler signals wrong | — | `tests/reactive_scheduler_signals_e2e.rs` | Covered |
| Reconfiguration integration broken | InvariantSessionOwnership | `tests/reconfiguration_integration.rs` | Covered |

## References

- [System Architecture](../../docs/001_system_architecture.md) — layer boundaries, guard chain
- [Authority and Identity](../../docs/102_authority_and_identity.md) — authority model
- [Effect System](../../docs/103_effect_system.md) — effect traits, composition constraints
- [Runtime](../../docs/104_runtime.md) — service actor patterns, concurrency profiles, link/delegate boundaries, async primitives
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) — canonical execution, admission guarantees
- [Formal Verification](../../docs/120_verification.md) — runtime parity lanes
- [Ownership Model](../../docs/122_ownership_model.md) — `Pure`, `MoveOwned`, `ActorOwned`, `Observed` categories
- [Testing Guide](../../docs/804_testing_guide.md) — required ownership tests, test strategy
- [System Internals Guide](../../docs/807_system_internals_guide.md) — instrumentation event families and required fields

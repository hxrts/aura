# Aura Telltale Sync and Async Boundary Refactor Plan

## Objective

Refactor the Aura and Telltale boundary so that:

- Telltale remains the synchronous protocol execution kernel
- Telltale operational envelopes become a first-class runtime contract at the boundary
- Aura remains the async runtime for transport, storage, journals, reactive updates, and services
- bridge-local mutable state moves behind explicit Aura effect interfaces
- `link` and `delegate` become the primary ownership and reconfiguration model
- each admitted VM fragment has one local owner at a time
- the boundary is covered by focused unit, integration, replay, and fault-injection tests

## Design Constraints

- Do not weaken Telltale VM determinism, admission, scheduler policy, replay, or conformance guarantees.
- Do not model envelope-bounded behavior as a local Aura convention; use Telltale-native envelope artifacts and runtime gates.
- Do not move async I/O into VM host callbacks.
- Do not preserve ad hoc bridge queues and state as long-term architecture.
- Do not introduce host-side ownership models that conflict with Telltale transfer guards.
- Keep production choreography execution VM-only.

## Target Model

The target model has four layers at the boundary:

1. Telltale VM owns protocol progression, scheduling, buffered semantics, and delegation safety.
2. Telltale operational envelopes own the admissible concurrency/determinism region for fragment execution.
3. Aura bridge effects expose synchronous session-local operations needed by VM callbacks.
4. Aura runtime services perform async work outside the VM step boundary.
5. `link` and `delegate` define the granularity of local ownership, transfer, and reconfiguration.

That means the host boundary should look like this:

- VM callback performs a small synchronous effect call
- effect call mutates session-local bridge state or snapshots session-local readiness
- async service loop observes bridge state and performs transport, journal, or storage work
- completed async results are reintroduced into the VM through explicit injection points
- eligible fragments run under explicit Telltale envelope policies rather than Aura-only cooperative stepping assumptions

## Phase 1: Boundary Inventory and Invariants

Purpose:
Define the exact semantics that the refactor must preserve before changing runtime code.

Tasks:

- [x] Inventory the current boundary surfaces in `vm_effect_handler`, `vm_host_bridge`, `choreo_engine`, and the runtime services that drive VM sessions.
- [x] Classify each boundary operation as one of:
  - synchronous session-local state mutation
  - synchronous readiness snapshot
  - async host work
  - ownership transfer
  - composition or reconfiguration
- [x] Write explicit invariants for:
  - single local owner per admitted VM fragment
  - no async work inside VM callbacks
  - no direct host mutation outside declared bridge effects
  - no ownership ambiguity across `delegate`
  - session-local state isolation under concurrent runs
- [x] Define the intended fragment granularity for ownership:
  - full session
  - linked sub-session
  - delegated endpoint set
- [x] Identify existing loops that must be eliminated or narrowed after the new effect surfaces exist.

Phase Gate:

- [x] Add or update architecture notes in this plan as the authoritative scratch design for the refactor.
- [x] Enumerate every current production runtime service that drives a VM fragment.

Success Criteria:

- [x] The refactor has a clear preservation target.
- [x] Every current boundary mutation or ownership crossing is named and classified.
- [x] The planned ownership model is stated in terms compatible with Telltale `link` and `delegate`.

### Phase 1 Notes

Current boundary surfaces:

- `AuraChoreoEngine` owns admitted VM startup, synchronous `step()`, termination budgets, determinism policy validation, trace capture, and session cleanup.
- `AuraVmEffectHandler` currently owns synchronous callback-local state for outbound payloads, branch choices, lease tracking, telemetry, scheduler signals, and emitted envelopes.
- `AuraQueuedVmBridgeHandler` currently owns synchronous callback-local state for queued outbound payloads, queued branch choices, pending sends, and scheduler signals.
- `vm_host_bridge` translates between synchronous VM callbacks and async Aura work by:
  - building role-scoped code images
  - opening admitted sessions
  - draining pending sends into `ChoreographicEffects`
  - waiting for blocked receives through Aura transport effects
  - injecting completed receives back into the VM
- `runtime/effects/choreography` currently provides async send and receive operations plus in-memory task-bound session state lookup.

Current operation classification:

- Synchronous session-local state mutation:
  - enqueue outbound payload
  - enqueue branch choice
  - record pending send
  - update scheduler signals
  - update active lease state
  - append emitted envelope and telemetry
- Synchronous readiness snapshot:
  - blocked receive edge lookup
  - scheduler signal snapshot
  - active lease snapshot
  - pending send drain
- Async host work:
  - transport send and receive
  - guard-chain evaluation
  - journal coupling
  - storage or journal publication
  - timeout and cancellation waiting
- Ownership transfer:
  - runtime fragment handoff
  - delegated endpoint transfer
  - fragment teardown and ownership release
- Composition or reconfiguration:
  - `@link`-declared bundle composition
  - delegated fragment activation
  - reconfiguration-manager registration and validation

Invariants to preserve:

- Each active admitted VM fragment has exactly one local owner at a time.
- VM callbacks perform no async work and hold no long-lived host resources.
- Host-visible session-local bridge mutation must move behind declared bridge effects.
- `delegate` must never leave ambiguous local ownership during or after handoff.
- Concurrent runs must remain isolated in session-local bridge state, guards, transport routing, and journal evidence.

Intended ownership granularity:

- Default ownership unit is one admitted VM fragment.
- A whole session may be one fragment when no `link` or `delegate` boundary exists.
- A linked sub-session may be owned independently when composition metadata creates a real fragment boundary.
- A delegated endpoint set becomes a new ownership unit at handoff time.

Existing loops to narrow or replace:

- Repeated `step -> flush sends -> await blocked recv -> inject recv` loops in:
  - `handlers/auth_service.rs`
  - `handlers/chat_service.rs`
  - `handlers/rendezvous_service.rs`
  - `handlers/invitation.rs`
  - `handlers/invitation/guardian.rs`
  - `handlers/invitation/device_enrollment.rs`
  - `handlers/recovery_service.rs`
  - `handlers/recovery_service/state_machine.rs`
  - `handlers/sessions/coordination.rs`
  - `runtime/services/sync_manager.rs`
- Polling-oriented `receive_from_role_bytes()` behavior in `runtime/effects/choreography.rs`
- Direct queue ownership in `AuraVmEffectHandler` and `AuraQueuedVmBridgeHandler`

Current production VM fragment drivers:

- `AuthService`
- `ChatService`
- `RendezvousService`
- `Invitation` handler flows
- `Invitation::guardian` flows
- `Invitation::device_enrollment` flows
- `RecoveryService`
- `RecoveryService` state-machine flows
- `Session` coordination handlers
- `SyncServiceManager`

## Phase 2: Session-Local Bridge Effect Interfaces

Purpose:
Replace ad hoc bridge queues and state with narrow Aura effect contracts that remain synchronous at the VM callback boundary.

Tasks:

- [x] Define a new session-local bridge effect surface in the correct crate and layer.
- [x] Split the surface into small operations such as:
  - outbound payload enqueue
  - inbound payload dequeue or claim
  - branch choice enqueue and consume
  - blocked edge snapshot
  - scheduler signal snapshot
  - lease and transfer metadata snapshot
- [x] Ensure the new effect contracts are synchronous or immediate-result only.
- [x] Wire the production implementation into Layer 6 runtime state instead of direct `Mutex<VecDeque<_>>` ownership inside VM handlers.
- [x] Add test implementations for the bridge effect surface in test infrastructure.
- [x] Remove direct queue ownership from `AuraVmEffectHandler` and `AuraQueuedVmBridgeHandler` where the new effect surface replaces it.

Phase Gate:

- [x] Run focused tests for the new bridge effect implementations.
- [x] Run `cargo test -p aura-agent vm_`
- [x] Run `just check-arch`
- [x] Verify all phase-focused tests are green.

Success Criteria:

- [x] VM callbacks depend on explicit bridge effects rather than ad hoc local queue structures.
- [x] The bridge effect surface is narrow and session-local.
- [x] No async trait surface is introduced into VM callback execution.

## Phase 3: Ownership Model Based on `link` and `delegate`

Purpose:
Bind local ownership to admitted VM fragments and make transfer semantics explicit.

Tasks:

- [x] Define a runtime ownership registry for admitted VM fragments.
- [x] Represent ownership in terms of fragment identity rather than one coarse protocol driver identity.
- [x] Map runtime ownership transitions onto Telltale `delegate` handoff semantics.
- [x] Map static composition boundaries onto Telltale `link` metadata and runtime registration.
- [x] Enforce a single local owner for each active fragment at runtime.
- [x] Reject ambiguous local ownership before a transfer reaches the VM boundary.
- [x] Journal or audit ownership transfer and fragment activation events where production policy requires evidence.
- [x] Remove coarse driver assumptions that treat a whole multi-part protocol as one indivisible host-owned unit.

Phase Gate:

- [x] Add focused tests for:
  - single-owner enforcement
  - successful delegation handoff
  - rejection of ambiguous ownership
  - linked fragment startup and teardown
- [x] Run `cargo test -p aura-agent reconfiguration`
- [x] Run `cargo test -p aura-agent telltale_vm`
- [x] Verify all phase-focused tests are green.

Success Criteria:

- [x] Local ownership is fragment-scoped.
- [x] Ownership transfer aligns with Telltale delegation semantics.
- [x] Runtime services do not hold overlapping ownership of the same active fragment.

## Phase 4: Operational Envelope Adoption

Purpose:
Make Telltale operational envelopes and bounded determinism a real part of the Aura runtime boundary instead of only policy metadata.

Tasks:

- [x] Classify production choreographies into:
  - cooperative-only
  - replay-deterministic threaded
  - envelope-bounded threaded
- [x] Add an explicit Aura runtime selector for cooperative VM versus `ThreadedVM` envelope execution per admitted fragment.
- [x] Wire admissible Telltale envelope profiles into runtime admission rather than only into static policy parsing.
- [x] Bind scheduler/runtime admission to the required envelope artifacts and capability gates for mixed or envelope-bounded execution.
- [x] Surface envelope metadata at the Aura boundary:
  - runtime mode
  - declared wave-width bound
  - determinism tier
  - replay mode
  - scheduler envelope class
- [x] Add envelope diff capture and validation for threaded candidate runs against the cooperative baseline where policy requires it.
- [x] Fail closed when an envelope artifact or runtime gate is missing for a fragment configured to require one.

Phase Gate:

- [x] Add focused tests for:
  - cooperative baseline admission
  - replay-deterministic threaded admission
  - envelope-bounded threaded admission
  - fail-closed rejection for missing envelope capability or artifact
  - envelope diff validation on a known threaded protocol
- [x] Run `cargo test -p aura-agent telltale_vm`
- [x] Run `cargo test -p aura-agent sync_manager`
- [x] Verify all phase-focused tests are green.

Success Criteria:

- [x] Aura actually uses Telltale’s operational-envelope system for the fragments that need it.
- [x] Envelope-bounded determinism is enforced through Telltale-native runtime artifacts and gates.
- [x] Cooperative execution is retained only where the policy class requires it.

## Phase 5: Async Host Work Isolation

Purpose:
Keep transport, journal, and storage work outside the VM step loop while making the boundary easier to reason about.

Tasks:

- [x] Refactor VM-driving loops so their structure is explicit:
  - step cooperative VM or advance threaded wave
  - observe bridge effects
  - perform async host work
  - inject results
  - validate envelope state where configured
- [x] Replace polling-oriented receive behavior where practical with awaitable session-local inbox or notification primitives.
- [x] Keep timeout policy and cancellation behavior explicit at the async host layer.
- [x] Ensure blocked-receive handling always routes through bridge effect state rather than scanning unrelated runtime state.
- [x] Make outbound flushing, inbound injection, and envelope artifact collection reusable across runtime services.
- [x] Ensure shutdown and cancellation cleanly release fragment ownership, session-local bridge state, and any pending envelope-validation state.

Phase Gate:

- [x] Run focused tests:
  - blocked receive wakeup
  - timeout behavior
  - cancellation during blocked receive
  - concurrent inbound delivery to multiple active fragments
- [x] Run `cargo test -p aura-agent runtime::effects::choreography`
- [x] Run `cargo test -p aura-agent sync_manager`
- [x] Verify all phase-focused tests are green.

Success Criteria:

- [x] Async host work is cleanly separated from synchronous VM callbacks and threaded-wave advancement.
- [x] Receive behavior no longer depends on broad host polling where a session-local wait primitive can be used.
- [x] Boundary control flow is uniform across production VM-driving services.

## Phase 6: Service Adoption and Boundary Simplification

Purpose:
Move production runtime services onto the new boundary model and remove obsolete bridge structure.

Tasks:

- [x] Update all production VM-driving services to use the new bridge effect and envelope execution interfaces.
- [x] Update reconfiguration paths to use fragment ownership, transfer semantics, and envelope-aware activation directly.
- [x] Update any remaining service-specific queue or bridging helpers that duplicate the new boundary model.
- [x] Remove dead code and compatibility scaffolding that the new boundary replaces.
- [x] Ensure all production choreography entrypoints use the same fragment ownership, bridge effect, and runtime-envelope path.

Phase Gate:

- [x] Run focused tests:
  - `cargo test -p aura-agent --features choreo-backend-telltale-vm`
  - `cargo test -p aura-agent --test reconfiguration_integration`
  - `cargo test -p aura-agent --test invitation_service_test`
- [x] Verify all phase-focused tests are green.

Success Criteria:

- [x] Production VM-driving services share one consistent boundary model.
- [x] The old ad hoc bridge structure is removed or reduced to test-only support.
- [x] Runtime behavior is simpler to inspect and reason about.

## Phase 7: Determinism, Replay, Conformance, and Envelope Hardening

Purpose:
Prove that the refactor preserved the properties Telltale was chosen for and that envelope-bounded execution stays within declared bounds.

Tasks:

- [x] Add replay tests that capture effect traces before and after boundary refactors where stable comparison is expected.
- [x] Add conformance tests that verify observable, scheduler-step, and effect surfaces remain valid.
- [x] Add tests that exercise `delegate` and `link` under concurrent active fragments.
- [x] Add tests that verify transfer-guard style failures surface as explicit runtime errors rather than silent host divergence.
- [x] Add scheduler fairness and non-starvation regression tests for mixed fragment workloads.
- [x] Add operational-envelope tests for:
  - wave certificate fallback to canonical execution
  - envelope diff generation and persistence
  - envelope diff rejection when scheduler or failure-visible drift exceeds policy
  - mixed fragment workloads where cooperative and threaded fragments coexist
- [x] Add fault-injection tests for:
  - dropped outbound flush
  - delayed inbound delivery
  - cancellation during delegation
  - double ownership claim
  - teardown during pending async work

Phase Gate:

- [x] Run focused tests:
  - `cargo test -p aura-agent telltale_vm`
  - `cargo test -p aura-agent reconfiguration`
  - `cargo test -p aura-harness`
  - `just ci-protocol-compat`
  - `just ci-dry-run`
- [x] Verify all phase-focused checks are green.

Success Criteria:

- [x] The refactored boundary preserves determinism, replay, conformance, and envelope adherence surfaces.
- [x] Delegate and link behavior is exercised under concurrency, not only happy-path serialization.
- [x] Boundary failures are explicit, testable, and auditable.

## Phase 8: Documentation and Architecture Gates

Purpose:
Make the new boundary model durable and reviewable.

Tasks:

- [x] Update work and docs to describe the new boundary in direct terms.
- [x] Update choreography and runtime docs to explain fragment-scoped ownership, bridge effects, and operational-envelope-aware execution.
- [x] Add `check-arch` rules or lint checks that prevent new ad hoc VM bridge queues, ownership bypasses, or unreviewed envelope-mode usage.
- [x] Add review checklist items for:
  - async work outside VM callbacks
  - single local owner per fragment
  - explicit `delegate` and `link` semantics
  - explicit operational-envelope selection and validation
  - test coverage for boundary changes

Phase Gate:

- [x] Run `just ci-crates-doc-links`
- [x] Run `just check-arch`
- [x] Verify all phase-focused checks are green.

Success Criteria:

- [x] The new boundary model is documented.
- [x] Architecture tooling rejects common regressions.
- [x] Reviewers have explicit criteria for future boundary changes.

## Required Test Surface

The refactor is not complete unless all of these surfaces exist and are green:

- [x] unit tests for bridge effect implementations
- [x] unit tests for ownership registry and transfer validation
- [x] integration tests for VM step and async host interaction
- [x] integration tests for concurrent fragments
- [x] reconfiguration tests for `link` and `delegate`
- [x] replay and conformance tests
- [x] operational-envelope admission and diff tests
- [x] cancellation and teardown tests
- [x] fault-injection tests for host-boundary failures
- [x] full `ci-dry-run`

## Final Acceptance Criteria

- [x] Aura and Telltale meet at a narrow, explicit, testable boundary.
- [x] Session-local bridge state is expressed through Aura effects rather than ad hoc bridge queues.
- [x] `link` and `delegate` determine fragment ownership and transfer semantics.
- [x] Each active admitted VM fragment has one local owner at a time.
- [x] Aura uses Telltale operational envelopes directly where bounded determinism is required.
- [x] Async host work is isolated from synchronous VM progression.
- [x] Determinism, replay, and conformance guarantees remain intact after the refactor.
- [x] The full targeted and CI-level test surface is green.

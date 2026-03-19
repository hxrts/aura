# Aura App (Layer 6) - Architecture and Invariants

## Purpose
Portable, platform-agnostic application core containing pure business logic
(intents, reducers, views) without runtime dependencies. Enables dependency
inversion through the `RuntimeBridge` trait.

## Inputs
- `RuntimeBridge` trait implementations (from aura-agent or test mocks).
- `Intent` objects representing user actions.
- Platform-specific feature flags (`native`, `ios`, `android`, `web-js`).

## Outputs
- `AppCore`, `Intent`, `ViewState`, `Screen`.
- Views: `ChatState`, `ContactsState`, `InvitationsState`, `RecoveryState`.
- Reactive signals: `CHAT_SIGNAL`, `SYNC_STATUS_SIGNAL`, `ERROR_SIGNAL`, etc.
- Shared UI contract surfaces: `UiSnapshot`, `RenderHeartbeat`,
  `OperationInstanceId`, `RuntimeEventSnapshot`, `SharedFlowId`,
  `SHARED_FLOW_SUPPORT`, `SHARED_SCREEN_SUPPORT`, `SHARED_MODAL_SUPPORT`,
  `SHARED_LIST_SUPPORT`, `SHARED_SCREEN_MODULE_MAP`.
- `RuntimeBridge`, `OfflineRuntimeBridge` for testing.
- `QueryHandler`, `ReactiveHandler`, `UnifiedHandler`.

## Invariants
- Pure logic: No runtime dependencies or impure I/O.
- Dependency inversion: aura-agent depends on aura-app, never vice versa.
- Push-based reactive flow: Intent → Journal → Reduce → ViewState → Signal → UI.
- Frontend agnostic: Works with multiple platform frontends.
- Shared-flow contract authority: semantic UI ids, flow support declarations,
  typed semantic command-plane metadata, and typed UI diagnostics are defined
  here rather than in frontend-specific crates.
- Shared semantic ownership authority: parity-critical semantic operation
  categories, typed terminal lifecycle, and owner-routed handles/tokens are
  defined here rather than in frontend-local crates.

## Ownership Model

For shared semantic flows, `aura-app` is primarily a `Pure` plus `MoveOwned`
crate.

- `Pure`
  - typed workflow/domain transitions
  - readiness derivation rules
  - snapshot/projection shaping
- `MoveOwned`
  - opaque operation handles
  - owner tokens / handoff objects
  - typed semantic lifecycle and failure contracts
- not `ActorOwned`
  - long-lived mutable async service/runtime state belongs in `aura-agent`
- not `Observed`
  - frontend render crates consume these contracts downstream

If `aura-app` coordinates a parity-critical operation across async boundaries,
one authoritative coordinator must own:

- submission
- phase advancement
- terminal success/failure publication
- cancellation / owner-drop failure

Frontend crates may not invent parallel lifecycle ownership for those
operations.

For parity-critical semantic operations, `aura-app` also owns the stronger
semantic-owner protocol:

- if Layer 7 creates a local submission record, Layer 7 must hand ownership off
  before the first awaited workflow step owned by `aura-app`
- only the canonical workflow owner may publish non-local lifecycle after
  handoff
- terminal publication must happen before any best-effort repair, warming, or
  reconciliation that is allowed to fail
- best-effort failure must never keep the primary operation in `Submitting`
- semantic owners may only use approved bounded-await and retry helpers inside
  owner bodies

If a workflow cannot satisfy those rules directly, it should move long-lived
convergence into a dedicated `ActorOwned` coordinator instead of hiding it in
an unbounded helper await.

### Ownership Inventory

| Path | Category | Authoritative owner | May mutate | Observe only |
|------|----------|---------------------|------------|--------------|
| Semantic command request/receipt types | `Pure` | `aura-app::ui_contract`, `aura-app::scenario_contract` | contract modules | `aura-terminal`, `aura-web`, `aura-harness` |
| Parity-critical semantic operation lifecycle | `MoveOwned` | workflow-local semantic coordinator per operation | `aura-app::workflows::*`, semantic-fact publishers | frontends, harness |
| Invitation/channel/delivery readiness derivation rules | `Pure` + coordinator-consumed `ActorOwned` inputs | readiness coordinators in `aura-app::workflows::*` | workflow/coordinator modules only | frontends, harness |
| Opaque handles / owner-token / handoff surfaces | `MoveOwned` | current token/record holder through sanctioned APIs | contract/workflow transfer APIs | render/projection layers, harness diagnostics |

### Required Ownership Tests

Changes to `aura-app` ownership boundaries should ship with:

- compile-fail guards for opaque handles, owner-token issuance, and
  capability-gated publication surfaces
- dynamic tests proving terminal lifecycle cannot regress for the same logical
  operation instance
- coordinator/concurrency tests proving authoritative semantic-fact updates do
  not lose entries under concurrent publication
- timeout/backoff invariant tests for local-budget propagation and typed
  timeout failure where the workflow owns timeout policy
- semantic-owner invariant tests proving:
  - best-effort failure cannot block terminal publication
  - frontend-local submission cannot mask authoritative terminal state after
    handoff
  - stale authoritative instances cannot overwrite newer local submissions
  - channel identity rebinding in derived chat state is clone-modify-swap, so a
    panic during rebinding cannot leak partial projection mutation

### Capability-Gated Points

- authoritative semantic lifecycle publication in
  `src/workflows/semantic_facts.rs`
- authoritative readiness publication and replacement in
  `src/workflows/semantic_facts.rs`
- workflow-owned semantic operation phase/failure publication in
  `src/workflows/messaging.rs`, `src/workflows/invitation.rs`, and related
  parity-critical workflow modules
- opaque shared command-plane and lifecycle surfaces in
  `src/ui_contract.rs` and `src/scenario_contract.rs`

### Verification Hooks

- `cargo check -p aura-app`
- `cargo test -p aura-app --lib concurrent_authoritative_fact_updates_do_not_lose_entries -- --nocapture`
- `cargo test -p aura-app --lib shared_flow_support_contract_is_consistent -- --nocapture`
- `cargo test -p aura-app --test compile_fail -- --nocapture`
- `just ci-capability-boundaries`
- `just ci-move-semantics`

### Detailed Specifications

### InvariantAppWorkflowPurity
Application workflows remain pure and frontend agnostic. Runtime effects are consumed through abstraction boundaries.

Enforcement locus:
- src workflows perform intent and state transitions.
- src core exposes platform-neutral integration surfaces.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just lint-arch-syntax
- just check-arch and just test-crate aura-app

Contract alignment:
- [Aura System Architecture](../../docs/001_system_architecture.md) defines dependency inversion.
- [Effect System and Runtime](../../docs/103_effect_system.md) defines purity boundaries.

Architecture/tooling split:
- syntax-owned purity/runtime-coupling checks should fail through
  `just lint-arch-syntax`
- workflow docs traceability and other repo-wide workflow integration checks
  remain in `just check-arch`

### InvariantSharedUiContractAuthority
`aura-app` is the authoritative home for shared semantic UI identity,
shared semantic command-plane types, shared-flow parity declarations, shared
screen/modal/list parity declarations, typed harness-visible diagnostics,
shared focus/selection semantics, shared action/readiness metadata, and the
machine-checkable screen/module map used for web/TUI parity enforcement.

Enforcement locus:
- `src/ui_contract.rs` defines semantic ids, `UiSnapshot`,
  `RenderHeartbeat`, `RuntimeEventSnapshot`, `SHARED_FLOW_SUPPORT`,
  `SHARED_SCREEN_SUPPORT`, `SHARED_MODAL_SUPPORT`, and
  `SHARED_LIST_SUPPORT`, plus `SHARED_SCREEN_MODULE_MAP` and shared semantic
  command-plane types.
- `src/ui.rs` re-exports the contract for harness and frontend consumption.

Failure mode:
- Frontends drift in naming or capability and harness scenarios stop being
  portable across TUI and web.
- Timeout diagnostics lose a single authoritative semantic contract.
- Frontends invent local command request or readiness shapes and shared-flow
  execution stops being uniform.

Verification hooks:
- `cargo test -p aura-app shared_flow_support_contract_is_consistent`
- `cargo test -p aura-app shared_screen_modal_and_list_support_is_unique_and_addressable`
- `cargo test -p aura-app shared_screen_module_map_uses_canonical_screen_names`
- `just ci-shared-flow-policy`

Contract alignment:
- [Testing Guide](../../docs/804_testing_guide.md) defines semantic shared-flow
  policy and timeout diagnostics.
- [Verification and MBT Guide](../../docs/806_verification_guide.md) defines the
  Quint/simulator/harness handoff around the shared contract.
## Boundaries
- No aura-agent imports (uses RuntimeBridge trait instead).
- No direct effect implementations.
- Platform-specific code isolated behind feature flags.

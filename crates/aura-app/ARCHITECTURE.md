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
  `SHARED_FLOW_SUPPORT`.
- `RuntimeBridge`, `OfflineRuntimeBridge` for testing.
- `QueryHandler`, `ReactiveHandler`, `UnifiedHandler`.

## Invariants
- Pure logic: No runtime dependencies or impure I/O.
- Dependency inversion: aura-agent depends on aura-app, never vice versa.
- Push-based reactive flow: Intent → Journal → Reduce → ViewState → Signal → UI.
- Frontend agnostic: Works with multiple platform frontends.
- Shared-flow contract authority: semantic UI ids, flow support declarations,
  and typed UI diagnostics are defined here rather than in frontend-specific
  crates.

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
- just check-arch and just test-crate aura-app

Contract alignment:
- [Aura System Architecture](../../docs/001_system_architecture.md) defines dependency inversion.
- [Effect System and Runtime](../../docs/105_effect_system.md) defines purity boundaries.

### InvariantSharedUiContractAuthority
`aura-app` is the authoritative home for shared semantic UI identity,
shared-flow support declarations, and typed harness-visible diagnostics.

Enforcement locus:
- `src/ui_contract.rs` defines semantic ids, `UiSnapshot`,
  `RenderHeartbeat`, `RuntimeEventSnapshot`, and `SHARED_FLOW_SUPPORT`.
- `src/ui.rs` re-exports the contract for harness and frontend consumption.

Failure mode:
- Frontends drift in naming or capability and harness scenarios stop being
  portable across TUI and web.
- Timeout diagnostics lose a single authoritative semantic contract.

Verification hooks:
- `cargo test -p aura-app shared_flow_support_contract_is_consistent`
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

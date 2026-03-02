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
- `RuntimeBridge`, `OfflineRuntimeBridge` for testing.
- `QueryHandler`, `ReactiveHandler`, `UnifiedHandler`.

## Invariants
- Pure logic: No runtime dependencies or impure I/O.
- Dependency inversion: aura-agent depends on aura-app, never vice versa.
- Push-based reactive flow: Intent → Journal → Reduce → ViewState → Signal → UI.
- Frontend agnostic: Works with multiple platform frontends.

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
## Boundaries
- No aura-agent imports (uses RuntimeBridge trait instead).
- No direct effect implementations.
- Platform-specific code isolated behind feature flags.


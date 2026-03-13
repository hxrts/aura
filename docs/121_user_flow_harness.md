# User Flow Harness

This document defines the harness architecture for testing parity-critical user flows.
It supplements [Testing Guide](804_testing_guide.md).
The harness validates that shared flows execute identically across TUI and browser frontends.

## 1. Purpose

The harness orchestrates multi-instance scenario execution against real frontends.
It submits typed semantic commands, observes authoritative projections, and validates deterministic behavior.
Shared flows run through one semantic contract rather than through frontend-specific scripting.

## 2. Semantic Command Plane

Shared scenarios execute through a typed command plane defined in `aura-app::scenario_contract`.
Commands include account creation, contact invitation, channel membership, and message sending.
Each command returns a typed response containing a value, submission state, and operation handle.

The `SharedSemanticBackend` trait defines the command submission interface.

```rust
pub trait SharedSemanticBackend {
    fn shared_projection(&self) -> Result<UiSnapshot>;
    fn submit_semantic_command(
        &mut self,
        request: SemanticCommandRequest,
    ) -> Result<SemanticCommandResponse>;
    fn wait_for_shared_projection_event(
        &self,
        timeout: Duration,
        after_version: Option<u64>,
    ) -> Option<Result<UiSnapshotEvent>>;
}
```

This trait abstracts command submission across backend implementations.
Both `LocalPtyBackend` and `PlaywrightBrowserBackend` implement this interface.
Commands flow through the real app update loop rather than through renderer shortcuts.

## 3. Execution Lanes

The harness distinguishes two execution lanes.
Shared semantic scenarios use typed commands and projection waits.
Frontend conformance scenarios use raw UI mechanics for renderer-specific validation.

The shared semantic lane targets product workflow correctness.
Failures in this lane indicate product bugs or command bridge issues.
The conformance lane validates that frontend controls map correctly to shared semantics.
Failures there indicate renderer or control binding issues.

Shared scenarios cannot call raw UI methods.
This separation prevents renderer timing from contaminating shared flow validation.

## 4. Backend Abstraction

Each backend implements three traits.
`InstanceBackend` provides lifecycle management, health checks, and basic observation.
`RawUiBackend` provides click, keystroke, and fill operations for conformance testing.
`SharedSemanticBackend` provides the semantic command plane for shared flows.

The `BackendHandle` enum dispatches to the appropriate backend implementation.

```rust
pub enum BackendHandle {
    Local(LocalPtyBackend),
    Browser(Box<PlaywrightBrowserBackend>),
    Ssh(SshTunnelBackend),
}
```

Local PTY backends communicate with the running app via Unix socket RPC.
Browser backends communicate via a Node.js Playwright driver subprocess.
Both translate semantic commands into real app update events.

## 5. Observation Model

`UiSnapshot` is the authoritative semantic observation surface.
It contains screen state, modal state, list contents, and runtime events.
Every snapshot carries revision metadata for monotone freshness validation.

Parity-critical waits resolve against typed readiness contracts.
The harness provides helper functions for common wait patterns.

```rust
fn wait_for_modal_visible(
    backend: &dyn InstanceBackend,
    modal_id: ModalId,
    timeout: Duration,
) -> Result<()>;

fn wait_for_screen_visible(
    backend: &dyn InstanceBackend,
    screen_id: ScreenId,
    timeout: Duration,
) -> Result<()>;
```

These functions poll `ui_snapshot()` until the condition holds or timeout expires.
Raw text or DOM inspection is diagnostic only and must not determine success.

## 6. Operation Handles

Semantic commands return operation handles for tracking async completion.
The handle contains an operation ID and instance ID.
The harness can observe operation state through the projection.

```rust
pub fn observe_operation(
    snapshot: &UiSnapshot,
    operation_id: &OperationId,
) -> Option<ObservedOperation>;

pub fn wait_for_operation_submission(
    backend: &dyn InstanceBackend,
    operation_id: OperationId,
    previous: Option<ObservedOperation>,
    timeout: Duration,
) -> Result<UiOperationHandle>;
```

Operation observation detects state changes without side effects.
The wait function polls until the operation reaches the expected submission state.

## 7. Scenario Execution

The `ScenarioExecutor` drives scenario steps within budget constraints.
Each step has a timeout and the scenario has a global budget.
The executor tracks flow state across instances and validates action preconditions.

Flow state machines govern action sequencing.

| Phase | States |
|-------|--------|
| Account | New, Ready |
| Contact | None, InvitationReady, Linked |
| Channel | None, InvitationPending, MembershipReady |
| Messaging | None, Ready, Visible |

The executor validates that each action is permitted given current state.
State transitions are recorded for trace validation.

## 8. Canonical Traces

Every scenario run produces a canonical trace.
The trace contains action events, transition events, and terminal facts.
Trace shape conformance is part of parity validation.

A shared flow is deterministic when repeated runs produce identical trace shapes.
The harness derives per-scenario and per-instance seeds from the run configuration.
Identical seeds must produce identical semantic traces.

## 9. Revision Freshness

Every parity-critical transition must advance monotonic revision metadata.
Post-action observation must require a strictly newer snapshot than the pre-action baseline.
Render heartbeat divergence indicates stale state rather than completion.

Browser cache invalidation occurs at declared lifecycle boundaries.
Session start, authority switch, device import, and storage reset are lifecycle events.
Flow-specific cache invalidation is not allowed.

## 10. Harness Mode Constraints

Harness mode may add instrumentation and render stability hooks.
Harness mode must not change product business semantics.
Any harness-only branch that changes parity-critical flow meaning is a defect.

Allowlisted harness mode hooks carry owner and justification metadata.
The policy script `scripts/check/ux-policy-guardrails.sh` enforces these constraints.

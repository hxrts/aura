# Aura UI (Layer 7)

## Purpose

Shared Dioxus UI core for Aura providing platform-agnostic UI state, deterministic key routing, and canonical text snapshot rendering used by harness automation.

## Scope

| Belongs here | Does not belong here |
|-------------|---------------------|
| Shared Dioxus component tree and UI state model | Browser API usage (`web_sys`, `wasm_bindgen`, `js_sys`) |
| Deterministic keyboard routing for harness-driven scenarios | Desktop/mobile shell integration code |
| Canonical snapshot text rendering for harness introspection | Runtime/effect handler implementation ownership |
| Typed DOM-id helpers reused by Layer 7 shells | Browser shell bridge ownership and publication policy |
| Platform-neutral harness bridge primitives | Parity-critical semantic lifecycle authorship |
| Dioxus-specific spawn wiring for the shared frontend task-owner | Shell-specific runtime/bootstrap orchestration ownership |
| Shared semantic UI contract materialization from `aura-app` | Callback-owned readiness synthesis |

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| Incoming | aura-app | Semantic UI contract (`ui_contract`), authoritative workflow publication, shared frontend primitives (`frontend_primitives`) |
| Outgoing | — | Typed screen, modal, operation, toast, list, and runtime-event state |
| Outgoing | — | `UiSnapshot` for canonical semantic projection export |
| Outgoing | — | Typed DOM-id helpers for shared/web Layer 7 rendering |
| Outgoing | — | Platform-neutral harness bridge primitives |
| Outgoing | `aura-web` | Dioxus root component, rendering, keyboard routing |

## Invariants

- Shared core remains platform agnostic; shell crates own platform interop.
- Snapshot output remains deterministic for equivalent state and key streams.
- Keyboard routing is centralized and side-effect order is deterministic.
- Shared state is keyed by semantic ids and typed operation/runtime-event snapshots rather than frontend-local row indexes or renderer-only state.
- Shared channel/contact selection keys use canonical ids from runtime projections; display labels stay display-only and must not be upgraded back into semantic identity.
- Shared channel list item ids and click paths must stay on canonical channel ids when the runtime projection already provides them; render code may not bounce back through display-name selection on those paths.
- Boundary-time name input may identify a channel for local keyboard/demo helpers, but converted shared UI submission paths must switch to the canonical channel id returned by `aura-app` before storing selection or publishing runtime facts.
- Shared screen and modal structure remains stable enough for semantic harness execution and render-convergence checks.
- Parity-critical IDs, focus semantics, and action shapes are consumed from `aura-app::ui_contract`; they are not locally reinvented here.
- Contacts-screen friend-management action availability must follow shared `aura-app` relationship-state controls; `aura-ui` may not invent a separate friendship state machine or alternate action matrix.
- Layer 7 shells may reuse `aura-ui`'s shared frontend operation-label taxonomy for user-facing error reporting instead of maintaining parallel label enums.
- Parity-relevant ceremony progress in shared modals must consume upstream-owned lifecycle helpers from `aura-app::ui::workflows`; `aura-ui` must not keep bespoke poll/sleep loops for those paths.
- Device-enrollment import and accept flows must rely on the upstream
  invitation workflow's bounded convergence contract rather than adding a
  frontend-local runtime pre-warm or peer-connectivity loop in `aura-ui`.
- The add-device confirmation display and refresh path must read typed
  `CeremonyStatusHandle` lifecycle status from `aura-app::ui::workflows`
  rather than inferring progress from local timers, local counters, or modal
  transitions alone.
- Published observed semantic projections must support stale-state detection through shared revision/sequence and render-convergence semantics.
- Onboarding must publish through the same semantic snapshot path as every other screen.

### InvariantUiSnapshotReflectsSemanticState

`aura-ui` exports observed semantic projections that match the shared contract rather than frontend-local incidental structure.

Enforcement locus:
- `model.rs` owns typed selection, operation, toast, and runtime-event state.
- `semantic_snapshot()` exports the canonical `UiSnapshot`.

Failure mode:
- Harness assertions depend on renderer text or row order instead of semantic ids.
- Browser and TUI drift in observable state despite sharing the same flows.

Verification hooks:
- `cargo test -p aura-ui semantic_snapshot_includes_runtime_events`
- `cargo test -p aura-ui restarting_operation_generates_new_operation_instance_id`

Contract alignment:
- [Testing Guide](../../docs/804_testing_guide.md) defines harness snapshot expectations.

### InvariantSharedFlowShapesAreUniform

Shared screens and modals expose consistent semantic structure for frontends and the harness.

Enforcement locus:
- shared modal/button/field ids are driven from the `aura-app` contract.
- keyboard and click flows resolve through shared control ids and typed modal state.

Failure mode:
- Harness execution requires per-screen or per-frontend special cases.
- Shared scenarios regress back to raw mechanics.

Verification hooks:
- `just ci-shared-flow-policy`

Contract alignment:
- [Testing Guide](../../docs/804_testing_guide.md) defines shared flow uniformity requirements.

## Ownership Model

> Taxonomy: [Ownership Model](../../docs/122_ownership_model.md)

`aura-ui` is an `Observed` shared UI core for parity-critical semantic flows. It may render lifecycle and submit frontend-local UI transitions, but terminal semantic truth stays in authoritative workflow/runtime ownership upstream.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| Semantic snapshot shaping and projection export | `Observed` | Authoritative facts and shared UI state reducers own truth; `model.rs` shapes presentation. |
| Keyboard/focus/modal state | `Observed` | Shared UI model owns state; `keyboard.rs`, `model.rs` update it. |
| Parity-critical operation rendering | `Observed` | Authoritative semantic facts from `aura-app` own truth; `model.rs` projects. |
| Shared-flow completion helpers | `Observed` | Upstream workflow/runtime coordinators own truth; helpers dismiss UI state only. |
| Notification action bar and action dispatchers | `Observed` | `notification_actions.rs` submits operations via handoff owners and renders action buttons; terminal truth stays in `aura-app` workflows. |
| Dioxus-specific spawn wiring for shared task-owner | `ActorOwned` helper for Dioxus shells | `task_owner.rs` provides the Dioxus-specific default spawn wiring. The core `FrontendTaskOwner` type lives in `aura-app::frontend_primitives`. |
| Mounted shell signal subscriptions | `ActorOwned` helper scoped to component lifetime | `app/shell/subscriptions.rs` owns cancellable component-scoped subscription tasks so preserved-profile rebootstrap tears down old generation observers instead of accumulating immortal frontend loops. |

### Capability-Gated Points

- shared semantic lifecycle and readiness must be consumed from `aura-app::ui_contract` / `aura-app` authoritative workflow publication, never authored locally in `aura-ui`
- shared-flow completion helpers may dismiss UI state and surface observed progress, but may not publish terminal semantic truth
- keyboard and focus routing may trigger frontend-local transitions, but parity-critical command ownership remains upstream in shell/workflow boundaries

## Testing

### Strategy

Snapshot determinism and shared-flow shape uniformity are the primary concerns. Tests verify semantic snapshot correctness, operation instance lifecycle, and shared screen/modal structure.

### Commands

```
cargo test -p aura-ui
just ci-shared-flow-policy
just ci-observed-layer-boundaries
```

### Coverage matrix

| What breaks if wrong | Invariant | Test location | Status |
|---------------------|-----------|--------------|--------|
| Snapshot missing runtime events | UiSnapshotReflectsSemanticState | `semantic_snapshot_includes_runtime_events` | Covered |
| Restarted operation reuses stale id | UiSnapshotReflectsSemanticState | `restarting_operation_generates_new_operation_instance_id` | Covered |
| Shared frontend task owner stops reporting live after shutdown/drop | Ownership inventory | `task_owner::tests` | Covered |
| Shared flow shapes diverge per frontend | SharedFlowShapesAreUniform | `just ci-shared-flow-policy` | Covered |

## References

- [Testing Guide](../../docs/804_testing_guide.md)

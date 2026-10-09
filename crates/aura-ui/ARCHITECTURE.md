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

- Runtime observation counts use the shared fixed-width contract. Native lengths
  are checked before publication; conversion failures are diagnosed and cannot
  publish a truncated count or advance the observed-device baseline.

- Shared core remains platform agnostic; shell crates own platform interop.
- Add-device submission requires the actual new device's user-transferred setup
  code. The shared modal renders separate name/setup fields with shared field IDs
  and forwards the code to the app-owned verification and issuance workflow.
  Keyboard state cannot fabricate an invitee authority or enrollment code;
  only runtime issuance advances the modal to code sharing.
- Snapshot output remains deterministic for equivalent state and key streams.
- Keyboard routing is centralized and side-effect order is deterministic.
- Shared state is keyed by semantic ids and typed operation/runtime-event snapshots rather than frontend-local row indexes or renderer-only state.
- Shared channel/contact selection keys use canonical ids from runtime projections; display labels stay display-only and must not be upgraded back into semantic identity.
- Shared channel list item ids and click paths must stay on canonical channel ids when the runtime projection already provides them; render code may not bounce back through display-name selection on those paths.
- Boundary-time name input may identify a channel for local keyboard/demo helpers, but converted shared UI submission paths must switch to the canonical channel id returned by `aura-app` before storing selection or publishing runtime facts.
- Shared screen and modal structure remains stable enough for semantic harness execution and render-convergence checks.
- Parity-critical IDs, focus semantics, and action shapes are consumed from `aura-app::ui_contract`; they are not locally reinvented here.
- Contacts-screen friend-management action availability must follow shared `aura-app` relationship-state controls; `aura-ui` may not invent a separate friendship state machine or alternate action matrix.
- AMP channel transition notification state must be rendered from
  `RuntimeFact::AmpChannelTransitionUpdated` snapshots and shared
  `aura-app::ui_contract` action/control ids. `aura-ui` may expose observed
  affordances for emergency alarm, quarantine approval, cryptoshred approval,
  conflict evidence, and finalization status, but it may not infer send/receive
  authority from local message-ratchet state.
- Layer 7 shells may reuse `aura-ui`'s shared frontend operation-label taxonomy for user-facing error reporting instead of maintaining parallel label enums.
- Parity-relevant ceremony progress in shared modals must consume upstream-owned lifecycle helpers from `aura-app::ui::workflows`; `aura-ui` must not keep bespoke poll/sleep loops for those paths.
- Device-enrollment import and accept flows must rely on the upstream
  invitation workflow's bounded convergence contract rather than adding a
  frontend-local runtime pre-warm or peer-connectivity loop in `aura-ui`.
- The add-device confirmation display and refresh path must read typed
  `CeremonyStatusHandle` lifecycle status from `aura-app::ui::workflows`
  rather than inferring progress from local timers, local counters, or modal
  transitions alone.
- The shared UI observes separate code-issuance and enrollment-completion
  operations. Modal or wizard state cannot synthesize a terminal completion
  snapshot; exported operation state preserves the app-owned instance,
  cancellation, and typed failure domain/code. Guardian acceptance uses its
  own operation kind and waits for verified follow-up evidence.
- Published observed semantic projections must support stale-state detection through shared revision/sequence and render-convergence semantics.
- `UiController` exports the app snapshot's `projection_source_revisions` with `UiSnapshot`. Those source graph revisions describe the observed entity values and remain separate from the UI semantic/render `revision`; the UI must not mint or advance source revisions locally.
- Onboarding must publish through the same semantic snapshot path as every other screen.
- Mounted runtime subscriptions form one component-owned group per runtime generation, with one observer per distinct required signal. The group reports typed attachment and stream health; a failed attach or closed stream cannot leave the group apparently ready.
- Unmount and generation change cancel the entire subscription group, including refresh work, before replacement observers attach. Subscription tasks cannot retain the group owner through their own captures.
- Attachment and recovery resnapshot current signal state after the receiver is live. Lag may skip intermediate values, so observed projections must converge from a newer current snapshot.

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
| AMP transition notification projection | `Observed` | Reducer-derived runtime events from `aura-app` own transition truth; `app/runtime_views/notifications.rs` chooses shared labels/actions without local ratchet guesses. |
| Dioxus-specific spawn wiring for shared task-owner | `ActorOwned` helper for Dioxus shells | `task_owner.rs` provides the Dioxus-specific default spawn wiring. The core `FrontendTaskOwner` type lives in `aura-app::frontend_primitives`. |
| Mounted shell signal subscriptions | `ActorOwned` helper scoped to component lifetime and runtime generation | `app/shell/subscriptions.rs` owns one supervised group with typed health, unique signal receivers, current-state resnapshot, and explicit cancellation on unmount or rebootstrap. Listener tasks may hold a task spawner, but not a clone of the group's lifetime owner. |

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
| Failed attach or stream closure silently freezes the UI | Mounted subscription ownership | Fault-injected attach/closure and recovery tests in `app/shell/subscriptions.rs` | Required |
| Rebootstrap retains old observers or misses current state | Mounted subscription ownership | Generation-change, repeated-mount, subscriber-count, and post-lag resnapshot tests | Required |

## References

- [Testing Guide](../../docs/804_testing_guide.md)

## Enrollment trust transfer boundary

Device enrollment import uses the shared three-field contract and transfers submission ownership to the app before awaiting pin/import/acceptance. The shell does not derive an initiator verifier from the received payload or adopt its authority/device identifiers before acceptance. Issuer output preserves actual signed manifest and independent verifier transfer material. Browser account persistence requires an app-issued completed result; legacy pending-code-only replay fails closed.

See [cryptography](../../docs/100_crypto.md), [operation ownership](../../docs/109_operation_categories.md), [shared user flows](../../docs/121_user_flow_harness.md), and [testing](../../docs/804_testing_guide.md).

### Observed runtime projection events

Chat and contact loaders use the shared pure `ui_contract` projection observation
builders. They publish `ChatSignalUpdated` and the legacy `RemoteFactsPulled`
projection counts only; the latter is not evidence of a successful network pull.
Membership, recipient resolution, and message delivery readiness come only from
the app-owned authoritative semantic fact subscription. A visible channel or its
member list cannot mint those readiness facts. Current projection observations
replace the previous observation of the same kind, including when counts fall.
Equal channel names use canonical IDs to order the default selection, and
message lookup retains the selected ID. Duplicate-name regression coverage
checks both the browser view and shared observation builder.

Controller snapshot publication derives `home_modes` through the app-owned `observed_home_modes` helper from the same `StateSnapshot` as `projection_source_revisions.homes`. Runtime view labels and frontend caches cannot reconstruct home modes or their context. Both model observation and published browser snapshots use this shared enrichment path.

If the shared controller cannot acquire the app snapshot, it exports the existing loading/busy snapshot rather than claiming authoritative empty home modes with ready state.

Completed owned runtime refreshes schedule semantic publication even when rendered view values compare equal: mode-only canonical changes must wake pushed snapshot observers. Display signals remain updated only when their values change, and the existing snapshot sink deduplicates identical observations.

The subscription supervisor awaits the app-owned attached receiver before reading initial state or reporting Healthy. Native and browser observers share this ordering; owned cancellation still drops the receiver with the mounted subscription owner.

Native integration tests may opt into app `test-support` fixture publication, which delegates existing observed-projection owners to publish a detached HomesState or mutate only the mode of an exact materialized home. The feature is absent from default/wasm production frontends and does not add a production mutation bypass.

# Aura Web (Layer 7)

## Purpose

Browser/WASM shell for Aura. Remains thin and delegates shared UI state, routing, and snapshot rendering to `aura-ui`.

## Scope

| Belongs here | Does not belong here |
|-------------|---------------------|
| Bootstrap Aura runtime for browser/WASM environments | Shared UI logic ownership (lives in aura-ui) |
| Mount the shared `aura-ui` Dioxus root | Effect trait or runtime handler ownership |
| Browser-specific adapters (clipboard, JS harness bridge) | Domain/protocol logic ownership |
| `window.__AURA_HARNESS__` for Playwright-driven automation | Parity-critical semantic lifecycle ownership |
| Semantic `UiSnapshot` and `RenderHeartbeat` publication in harness mode | |
| Typed semantic command bridge for shared-flow execution | |

## Dependencies

| Direction | Crate | What is consumed / produced |
|-----------|-------|-----------------------------|
| Consumes | `aura-ui` | Shared Dioxus root, UI state, routing, snapshot rendering |
| Consumes | `aura-app` | `AppCore`, `ui_contract`, workflow types, shared frontend primitives (`frontend_primitives`) |
| Consumes | `aura-core` | Types, identifiers |
| Produces | — | Browser shell, harness bridge surface, semantic projections |

## Invariants

- Browser-only APIs stay in this crate.
- Shared UI behavior remains in `aura-ui`.
- Browser shell DOM-id resolution reuses the shared typed helper surface from
  `aura-ui` rather than re-opening `web_dom_id().expect(...)` chains at each
  browser callsite.
- Browser task ownership reuses the shared `FrontendTaskManager` / `FrontendTaskOwner` from
  `aura-app::frontend_primitives` with browser-specific spawn wiring
  (`wasm_bindgen_futures::spawn_local`) rather than keeping a forked stack.
- Harness bridge methods are deterministic and backwards-compatible.
- Bridge responses that claim to return a channel binding must originate from authoritative selected-channel context materialization; selected ids without context are not a binding.
- When bridge flows can only observe a selected channel id without authoritative
  context, they must publish an explicit weak selection payload instead of a
  binding-shaped response.
- Browser-owned semantic snapshot publication goes through one helper aligned
  with `UiController::publish_ui_snapshot`; shell code must not repeat ad hoc
  `semantic_model_snapshot()` plus publish pairs at bootstrap/finalization
  callsites.
- Browser bootstrap handoff stays explicit: runtime identity is staged through
  the dedicated `stage_runtime_identity` bridge entrypoint rather than through
  ambient storage or a generic bootstrap trigger.
- Browser bootstrap storage is explicit and typed: the shell persists selected
  runtime identity, pending bootstrap metadata, and browser-local
  `AccountConfig` metadata separately so preserved-profile restarts can rebind
  one active generation and recover the canonical runtime bootstrap path
  without browser-local semantic repair.
- Persisted browser `AccountConfig` context selection is fail-closed and
  authority-scoped: when the current runtime has no active home yet, browser
  bootstrap storage may reuse only the persisted `AccountConfig` for that same
  authority, otherwise it must fall back to
  `default_context_id_for_authority(authority_id)` rather than fabricating a
  browser-local home or treating "no home yet" as fatal during bootstrap.
- Browser startup discovery uses the broker-backed bootstrap plane rather than
  native UDP LAN discovery. Broker results are surfaced as bootstrap
  candidates for enrollment and must not be counted or rendered as ordinary
  peers before invitation/device-enrollment succeeds.
- Browser bootstrap/rebootstrap bridge promises resolve on completion of the
  owned bootstrap transition, not merely on enqueue, so harness/browser callers
  do not mistake acceptance for success.
- Browser bootstrap/rebootstrap completion is generation-based: the promise
  resolves only after the active web shell generation has published the new
  generation's semantic snapshot through the canonical browser publication
  path, and the page-owned `__AURA_UI_ACTIVE_GENERATION__` /
  `__AURA_UI_READY_GENERATION__` diagnostics reflect that transition. Render
  heartbeat remains the separate render-convergence signal.
- Harness publication is semantic-first: pushed shared-contract state and render heartbeat are authoritative; DOM inspection is secondary diagnostics only.
- Browser harness publication is render-aligned: semantic snapshot publication
  and render heartbeat emission must be scheduled through
  `requestAnimationFrame` so harness render-convergence contracts observe
  post-render browser state instead of pre-paint intermediate state.
- Browser/DOM fallback paths are diagnostic-only and must not become parity-critical success-path observation.
- Browser semantic observation must fail closed when published semantic state is
  unavailable; it must not repair observation by reading a live controller
  snapshot behind the harness bridge.
- Browser `ui_state` observation remains observation-only: it may use the
  page-owned publication path and pushed caches, but navigation/session
  recovery stays on the explicit `recover_ui_state` path rather than being
  folded into ordinary semantic observation reads.
- Missing or degraded semantic snapshot/render-heartbeat publication must be
  surfaced explicitly through browser-side publication state, not just console
  logging or `null`/default fallbacks.
- Browser generation rebinding must be surfaced explicitly through page-owned
  generation diagnostics, so harness observation can distinguish rebinding from
  stale or missing semantic publication.
- Preserved-profile rebootstrap must tear down the previous browser runtime
  generation before starting the next one; clearing controller/publication
  state without stopping the old `aura-agent` runtime is forbidden because it
  can stack whole runtimes on the single browser main thread.
- Harness mode may change instrumentation and render stability, but not business-flow semantics.
- Shared browser-flow execution must go through the semantic command bridge and real app workflows; DOM clicks and selector helpers are frontend-conformance-only.
- Playwright-to-page semantic submission uses the page-owned semantic queue
  (`window.__AURA_DRIVER_SEMANTIC_ENQUEUE__`) so the driver does not own
  browser semantic lifecycle or runtime/bootstrap state.
- Browser semantic routing, weak selected-channel fallback handling, and raw
  semantic response construction stay centralized in `src/harness/commands.rs`;
  `src/harness/install.rs` and sibling bridge files must delegate through that
  module rather than constructing parallel submission paths.
- The page-owned semantic queue and runtime-stage queue contract has one
  canonical ownership split across `src/harness/driver_contract.rs` for raw
  driver key names and payload schema, `src/harness/window_contract.rs` for
  the narrow Rust-owned browser-global access helper, and
  `src/harness/page_owned_queue.rs` for the Rust-owned queue behavior.
  `src/harness/install.rs` is only the typed installer for those modules and
  must not become a second contract owner via ad hoc queue logic.
- Browser semantic submit readiness publication is page-owned and must report
  whether the page-owned enqueue surface is installed (`enqueue_ready`) so
  driver startup/recovery waits bind to generation-owned readiness instead of
  stale driver-local probes.
- Browser semantic submit readiness publication must also carry the active
  generation, ready generation, controller presence, current browser shell
  phase, and any in-flight bootstrap transition detail so the driver can
  observe product-owned bootstrap/rebinding state instead of inferring
  lifecycle from page-evaluate failures.
- Driver-owned `restart_page_session` is infrastructure recovery only. Semantic
  command submission and runtime-identity staging must wait on or fail from the
  page-owned bootstrap/publication contract rather than replaying work through
  browser-session restart.
- Long-lived browser maintenance tasks must surface terminal pause/failure
  through observed UI state or equivalent structured browser signals rather than
  relying on console logging alone.
- Browser-owned maintenance loops such as ceremony acceptance and background
  sync are explicitly non-semantic upkeep. They may keep local runtime/browser
  state moving, but they must not become browser-owned semantic lifecycle
  authorities.
- Browser-owned maintenance upkeep for a generation is serialized under one
  generation-owned supervisor loop. Distinct cadences such as harness transport
  polling, ceremony acceptance, and background sync may share one bounded sleep
  helper, but they must not run as parallel long-lived loops that compete for
  the same browser main thread and runtime bridge.
- Browser-hosted harness transport polling is mailbox upkeep only. After a poll
  drains envelopes, the browser may process the local inbox and refresh
  observed account state, but full discovery/sync convergence stays on the
  slower background-sync cadence so transport polling does not create a
  browser-owned convergence feedback loop.
- In harness mode, browser background sync must leave an explicit initial
  interactivity window after bootstrap handoff and use a coarser cadence than
  production so upkeep does not starve Playwright page-execution and semantic
  enqueue channels during preserved-profile scenario startup.
- The browser shell is an `Observed` plus bridge crate for shared semantic flows. It may submit commands and expose projections, but it must not own terminal semantic lifecycle truth for parity-critical operations.

### InvariantBrowserHarnessBridgePublishesSemanticState

The browser shell exports structured observed semantic UI projections and render convergence signals for harness observation.

Enforcement locus:
- `src/harness/publication.rs` is the canonical owner of semantic snapshot,
  render-heartbeat, and semantic-submit publication surfaces.
- `src/harness/publication.rs` schedules render heartbeat through
  `requestAnimationFrame` after semantic snapshot publication.
- `src/harness/install.rs` wires the page-owned observation/readiness surfaces.
- `src/harness/driver_contract.rs` owns the raw browser-driver key names and
  queue payload schema.
- `src/harness/window_contract.rs` owns the thin browser-global access helper
  used by queue/publication code instead of open-coded `Reflect::get` /
  `Reflect::set(window, ...)` patterns.
- `src/harness/page_owned_queue.rs` owns the page-side semantic and
  runtime-stage queue behavior that consumes that contract.
- `src/harness_bridge.rs` remains the thin stable facade rather than the
  publication owner.

Failure mode:
- Browser harness runs must infer state from DOM text or ad hoc JS scraping.
- Post-action hangs cannot be attributed to semantic state vs render convergence.
- Harness may observe semantic state published ahead of the browser paint
  boundary and misdiagnose render lag as workflow completion.
- State reads silently repair stale state or hide observation side effects.
- Publish failures disappear into logs while the bridge still appears healthy.

Verification hooks:
- Browser harness contract tests
- Playwright semantic bridge tests

### InvariantBrowserSharedFlowExecutionUsesSemanticBridge

The browser shell accepts shared semantic commands through an explicit bridge and executes them through real app workflows.

Enforcement locus:
- `src/harness_bridge.rs` owns typed command submission and unsupported-command failures.
- Selector-click helpers remain outside the shared semantic path.

Failure mode:
- Shared browser scenarios debug DOM/selector timing instead of production workflows.
- Browser and TUI shared lanes drift because they do not share one execution contract.

Verification hooks:
- Browser harness contract tests
- Playwright semantic bridge tests

## Ownership Model

Reference: [docs/122_ownership_model.md](../../docs/122_ownership_model.md)

For shared semantic flows, `aura-web` uses `Observed` for browser-side projection publication, render-convergence publication, and compatibility/version metadata. Narrow `ActorOwned` bridge ownership is permitted for browser bridge installation and command ingress (long-lived mutable async browser surfaces). The browser shell must not own terminal semantic lifecycle truth; browser code must not allocate local semantic owners for parity-critical operations or keep browser-local `OperationState::Submitting` state after handoff to shared workflow ownership per docs/122 section 16.

### Inventory

| Path | Category | Authoritative owner | May mutate | Observe only |
|------|----------|---------------------|------------|--------------|
| Browser harness bridge installation and command ingress | `ActorOwned` | `aura-web::harness_bridge` bridge loop | bridge ingress/installation code | browser render layer, harness |
| Browser semantic lifecycle rendering | `Observed` | authoritative semantic facts from `aura-app` | browser presentation state only | harness, user-visible rendering |
| Render-convergence and projection publication | `Observed` | browser projection/export path | `aura-web::harness::publication` only | Playwright/harness |
| Web onboarding/bootstrap command helpers for shared flows | `Observed` shell over upstream `MoveOwned`/`ActorOwned` coordination | shared workflow/runtime coordinators | browser-local UI state only; never terminal truth | harness, DOM/render readers |
| Shared browser task-owner cancellation/spawn mechanics | `ActorOwned` helper from `aura-app::frontend_primitives` | shared frontend task-manager implementation | browser shell wiring only | harness, render layer |

### Capability-Gated Points

- Harness bridge command ingress and compatibility-gated semantic command execution in `src/harness_bridge.rs`.
- Browser-side semantic projection, render-heartbeat publication, and typed
  publication-state diagnostics in `src/harness/publication.rs`.
- Page-owned harness observation/submit readiness installation in
  `src/harness/install.rs`.
- Browser clipboard and bootstrap adapters that may trigger upstream workflows, but may not author terminal semantic lifecycle locally.

### Verification Hooks

- `cargo check -p aura-web`
- Browser harness contract tests
- Playwright semantic bridge tests
- `just ci-observed-layer-boundaries`
- `just ci-frontend-handoff-boundary`
- `just ci-actor-lifecycle`
- `just ci-frontend-portability`

### Compatibility Policy

- The harness bridge request/response surface carries explicit compatibility metadata so callers can detect versioned behavior changes.
- Explicit bridge entrypoints currently include semantic command submission,
  observed snapshot/render publication, semantic-submit readiness metadata, and
  runtime identity staging for owned browser rebootstrap during create-account
  style flows.
- The browser bootstrap/account-config compatibility surface also includes the
  persisted-account-context resolution rule in `src/bootstrap_storage.rs`:
  active home context wins, otherwise reuse the persisted context only when it
  matches the selected authority, otherwise fall back to
  `default_context_id_for_authority(authority_id)`.
- The browser also exports explicit diagnostic publication-state globals for
  semantic snapshot and render-heartbeat availability so harness failures can
  distinguish unavailable publication from stale render convergence.
- Those publication diagnostics are typed internally through publication
  status, binding-mode, and reliability classes before they are serialized onto
  the browser globals.
- Browser-owned async account/bootstrap flows must not silently drop Dioxus
  signal writes when the shell is contended or unmounting. The shell may retry
  state publication on the next browser tick for the active generation, but it
  must emit a structured warning if the write still cannot be observed.
- Runtime identity staging and bootstrap handoff completion semantics are part
  of that compatibility surface; changes must preserve the distinction between
  accepted/enqueued work and completed bootstrap state.
- Additive fields and additive non-breaking methods are allowed when old callers continue to observe the same behavior.
- Breaking request/response or observation-shape changes must update explicit compatibility metadata and tests.

## Testing

### Strategy

Browser harness bridge correctness, semantic projection publication, and shared-flow execution parity are the primary concerns.

### Commands

```
cargo check -p aura-web
```

### Coverage matrix

| What breaks if wrong | Invariant | Test location | Status |
|---------------------|-----------|--------------|--------|
| Harness bridge publishes wrong state | BrowserHarnessBridgePublishesSemanticState | Browser harness contract tests | Covered |
| Shared flow uses selector instead of bridge | BrowserSharedFlowExecutionUsesSemanticBridge | Playwright semantic bridge tests | Covered |
| Render convergence signals wrong | BrowserHarnessBridgePublishesSemanticState | Playwright driver self-test | Covered |

## References

- [Aura System Architecture](../../docs/001_system_architecture.md)
- [Ownership Model](../../docs/122_ownership_model.md)
- [Testing Guide](../../docs/804_testing_guide.md)
- [Project Structure](../../docs/999_project_structure.md)

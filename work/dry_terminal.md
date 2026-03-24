# Plan: Reduce `aura-terminal` Redundancy Without Violating Layer 7 Boundaries

## Intent

This plan aims to make `aura-terminal` materially smaller, less repetitive, and
more intelligible by removing duplicate implementations and converging on one
correct path per behavior.

The primary goal is **not** to hit an arbitrary percentage reduction. The goal
is to reduce LOC as a consequence of:

- collapsing duplicate logic
- making ownership boundaries easier to audit
- making parity-critical behavior flow through fewer code paths
- deleting legacy wrappers, mirror types, and repeated test scaffolding

If a proposed LOC reduction would blur Layer 7 boundaries or move semantic
ownership into the terminal shell, it is out of scope.

## Current Motivation

`aura-terminal` is large enough that repeated patterns are obscuring the actual
owned behavior:

- `shell.rs` contains multiple near-parallel dispatch paths
- subscriptions and callback factories repeat the same scaffolding many times
- modal rendering and props wiring carry a lot of repeated chrome/shape code
- terminal-local mirror types and toast wrappers duplicate upstream concepts
- tests repeat large state/setup builders instead of expressing a smaller number
  of canonical flows

These are worthwhile refactor targets, but only if the result strengthens the
crate's existing architecture rather than chasing line counts for their own
 sake.

## Architectural Guardrails

This plan assumes the existing terminal and ownership contracts remain
authoritative:

- `aura-terminal` remains a Layer 7 `Observed` plus command-ingress shell.
- Shared semantic command ingress must continue to flow through the real TUI
  update/event loop.
- The terminal must not become an owner of parity-critical semantic truth.
- Callback and subscription bridges may adapt authoritative state for
  rendering, but may not publish terminal semantic truth.
- Reactive helper abstractions must preserve the reactive contract:
  unregistered-signal failure, lag-to-newer-snapshot semantics, and explicit
  degraded state on exhaustion.
- Cross-crate extraction into `aura-app` is allowed only when it improves the
  authoritative contract and preserves ownership boundaries. It is not a quota
  filler for LOC reduction.

## Success Criteria

The plan succeeds if, at the end:

1. There are materially fewer duplicate implementations of the same behavior.
2. Parity-critical flows route through fewer code paths than they do today.
3. The crate is easier to audit because helper abstractions reflect real
   contract classes rather than superficial syntax similarity.
4. Legacy wrappers, duplicate constants, mirror types, and repeated test
   harness setup are deleted rather than left beside the new model.
5. All architecture and ownership checks remain green.

LOC reduction is measured and reported, but it is an outcome metric, not the
hard acceptance gate.

## Non-Goals

- Forcing a 25% reduction regardless of architectural fit.
- Moving UI/render concerns into `aura-app` just to shrink the terminal crate.
- Introducing generic helpers that erase important parity-critical distinctions.
- Keeping old and new implementations alive in parallel "for safety."
- Using macros to hide complexity that should instead be made explicit.

## Refactoring Principles

### 1. Consolidate by Contract, Not by Superficial Shape

Two code paths should only be unified when they share the same:

- ownership semantics
- error-handling contract
- readiness / degraded-state contract
- terminality expectations
- publication path

If those differ, the code should remain separate or be unified only below the
point where those differences matter.

### 2. Delete Legacy Paths in the Same Phase

Each phase must remove superseded helpers, wrappers, and repeated branches in
the same change. The codebase should not accumulate compatibility duplicates.

### 3. Preserve the Real Ingress Path

Harness, callback, and observed-shell refactors must not create a second
semantically equivalent "direct execution" path that bypasses the real update
loop or app/runtime handoff rules.

### 4. Treat Reactive Helpers as Contract-Carrying Abstractions

Subscription helpers must be organized by reactive contract class, not just by
"subscribe, transform, update." The helper surface should make it harder to:

- ignore unregistered-signal failures
- treat lag as lossless replay
- hide structural degraded state
- silently convert contention into empty authoritative state

### 5. Prefer Fewer Canonical Shapes

Where the contract really is the same, the end state should be one obvious
implementation path:

- one dispatch engine per ingress class
- one modal-shell chrome implementation
- one canonical callback spawn pattern per ownership class
- one test harness builder path per test shape

## Workstreams

Before starting any workstream:

- [x] Record a baseline size audit with `find crates/aura-terminal -name "*.rs" -type f -print0 | xargs -0 wc -l | sort -nr`.
- [x] Record which touched modules are `Pure`, `MoveOwned`, `ActorOwned`, or `Observed`.
- [x] Prefer stronger typed request/response structs, enums, and owner tokens over stringly typed flags, ad hoc tuples, or boolean mode switches.
- [x] Tighten APIs toward the strongest authoritative input available; do not add new raw-id repair, fallback lookup, or ownerless callback paths.
- [x] For every touched parity-critical boundary, preserve the current ownership rule: handoff before first await unless the local owner is explicitly allowed to settle locally.
- [x] Keep every touched Rust file at or below 1500 LOC; when a file exceeds that limit, split it into coherent modules with descriptive names instead of moving complexity into a larger replacement file.
- [x] When a workstream introduces a new boundary or changes ownership semantics, update the relevant crate docs in the same change.

Recorded for Workstream A:

- Baseline size audit before the split: `shell.rs` 5402 LOC, `handlers/tui.rs` 1776 LOC, `tui/types.rs` 1774 LOC, `subscriptions.rs` 1611 LOC.
- Touched ownership classes: `shell/dispatch.rs` keeps the `ActorOwned` event-loop ingress plus `MoveOwned` operation owner allocation; `shell.rs`, `shell/runtime.rs`, `shell/props.rs`, and `shell/updates.rs` remain `Observed` composition/presentation shells around authoritative `aura-app` state; `shell/input.rs` and `shell/state.rs` are `Pure`/`Observed` transition and render-state adapters.

## Workstream A — Shell Dispatch Consolidation

Target: reduce duplication in `tui/screens/app/shell.rs` while preserving the
event-loop ingress boundary.

### A1. Dispatch Core Extraction

- [x] Audit the current dispatch entry points in `src/tui/screens/app/shell.rs`, `src/tui/screens/app/shell/events.rs`, `src/tui/screens/app/shell/input.rs`, `src/tui/context/dispatch.rs`, and `src/tui/harness_state/`.
- [x] For each command path, classify what is shared `Pure`/`Observed` preparation versus what must remain event-loop-owned submission logic.
- [x] Introduce a typed preparation surface such as `DispatchPreparation`, `DispatchSelection`, or similarly explicit request structs instead of passing loose tuples and cloned local fragments.
- [x] Keep the event-loop wrapper as the only authoritative ingress owner for parity-critical submissions.
- [x] Ensure harness follow-up paths observe or verify the same sanctioned ingress path rather than building a direct execution shortcut.

### A2. Modal-Open Command Extraction

- [x] Inventory modal-opening branches in `src/tui/screens/app/shell.rs` and group them by contract class:
- [x] authoritative-selection based
- [x] observed-only convenience modal
- [x] ceremony or wizard bootstrap requiring explicit handoff inputs
- [x] Extract helpers only within those contract classes; do not unify authoritative and observed-only modal launches behind one opaque helper.
- [x] Replace ad hoc modal-open parameter assembly with typed requests where the shell currently threads multiple related values together.

### A3. Ceremony Start/Cancel Consolidation

- [x] Compare ceremony start, progress, cancel, and rollback paths command by command.
- [x] Verify owner-handoff timing, terminal outcome mapping, degraded-state handling, and cancellation semantics before unifying any helper.
- [x] If the ceremony variants differ only by typed parameters, extract a canonical typed helper; if they differ by ownership or terminal semantics, keep them separate.
- [x] Ensure ceremony code continues to consume typed upstream lifecycle terminality rather than local inference or string matching.

### A4. IoApp Props Cleanup

- [x] Replace duplicated `element!` construction and repeated cfg branching with a small typed constructor or `cfg_if!` grouping where that improves clarity.
- [x] Keep this cleanup presentation-local; do not move semantic decisions into props assembly.

### A5. Shell File Decomposition

- [x] Reduce `src/tui/screens/app/shell.rs` from 5402 LOC to under 1500 LOC.
- [x] Split extracted logic into coherent siblings under `src/tui/screens/app/shell/` with descriptive names such as `dispatch_preparation.rs`, `modal_commands.rs`, `ceremony_dispatch.rs`, and `io_app_props.rs` if those shapes fit the final design.
- [x] Keep `shell.rs` as a small composition root or barrel, not a second catch-all.

### A6. Workstream A Verification and Commit

- [x] Run `cargo check -p aura-terminal`.
- [x] Run `cargo test -p aura-terminal harness_command_invite_actor_to_channel_emits_dispatch_followup -- --nocapture`.
- [x] Run `cargo test -p aura-terminal authoritative_submitting_after_terminal_allocates_new_instance -- --nocapture`.
- [x] Run `cargo test -p aura-terminal --test compile_fail`.
- [x] Run `just ci-observed-layer-boundaries`.
- [x] Run `just ci-frontend-handoff-boundary`.
- [x] Confirm every targeted check is green before committing.
- [x] Create a commit such as `git add work/dry_terminal.md crates/aura-terminal && git commit -m "refactor(aura-terminal): consolidate shell dispatch"`.

## Workstream B — Subscription and Callback Consolidation

Target: reduce scaffolding without weakening the reactive contract or callback
ownership boundaries.

### B1. Subscription Contract Classification

- [x] Audit every `use_*_subscription` helper in `src/tui/screens/app/subscriptions.rs`.
- [x] Classify each subscription as:
- [x] observed-only projection subscription
- [x] parity-relevant lifecycle or readiness subscription
- [x] structural degraded-state subscription
- [x] side-effect or update-loop bridge
- [x] Document the class in the code shape, helper naming, and module layout rather than relying on comments alone.

### B2. Typed Subscription Helpers

- [x] Introduce one helper family per subscription contract class instead of a single generic subscription wrapper.
- [x] Add typed result/degradation payloads where current helpers pass loosely structured closures and string reasons.
- [x] Make registration failure, lag-to-newer-snapshot semantics, and degraded-state publication explicit in the helper signatures.
- [x] Avoid helpers that hide whether a given subscription is parity-relevant or merely observed.

### B3. Callback Factory Cleanup

- [x] Audit factories in `src/tui/callbacks/factories/mod.rs`, `chat.rs`, `contacts.rs`, `invitation.rs`, `recovery.rs`, and `settings.rs`.
- [x] Collapse trivial alias types and repeated `new()` boilerplate only when the ownership class is identical.
- [x] Keep separate factory surfaces where APIs require different owner types such as `LocalTerminalOperationOwner` versus `WorkflowHandoffOperationOwner`.
- [x] Replace weakly typed callback setup data with typed request structs where factories currently rely on repeated parameter clusters.

### B4. Spawn Helper Consolidation

- [x] Consolidate spawn helpers by ownership and publication class:
- [x] observed dispatch with toast/reporting
- [x] local owner submission
- [x] workflow handoff submission
- [x] Preserve distinct helper entry points when handoff timing, terminal publication, or capability-gated settlement differ.
- [x] Do not introduce any ownerless shortcut for parity-critical callbacks.

### B5. Subscription and Callback File Decomposition

- [x] Reduce `src/tui/screens/app/subscriptions.rs` from 1611 LOC to under 1500 LOC.
- [x] Split `subscriptions.rs` into coherent modules with descriptive names such as `shared_state.rs`, `observed_projections.rs`, `lifecycle.rs`, `degraded_state.rs`, and `display_clock.rs` where they match the final classification.
- [x] If callback cleanup materially grows `src/tui/callbacks/factories/mod.rs`, `chat.rs`, or `settings.rs`, split them by owner class or domain contract before they exceed 1500 LOC.

### B6. Workstream B Verification and Commit

- [x] Run `cargo check -p aura-terminal`.
- [x] Run `cargo test -p aura-terminal --test wiring`.
- [x] Run `cargo test -p aura-terminal --test integration_bridge`.
- [x] Run `cargo test -p aura-terminal --test compile_fail`.
- [x] Run `just ci-parity-critical-callback-settlement`.
- [x] Run `just ci-observed-layer-boundaries`.
- [x] Run `just ci-frontend-handoff-boundary`.
- [x] Run `just ci-actor-lifecycle`.
- [x] Confirm every targeted check is green before committing.
- [x] Create a commit such as `git add work/dry_terminal.md crates/aura-terminal && git commit -m "refactor(aura-terminal): classify subscriptions and callbacks"`.

## Workstream C — Modal and Props Simplification

Target: reduce repeated shell/layout code without moving canonical semantics out
of their proper owner.

### C1. Modal Overlay Chrome Consolidation

- [x] Audit modal chrome repeated across `src/tui/screens/app/modal_overlays.rs` and `src/tui/components/*modal*.rs`.
- [x] Extract reusable chrome only for shared visibility, border, title, footer, and error presentation concerns.
- [x] Keep workflow-specific state transitions, owner tokens, and semantic lifecycle handling outside the shared modal wrapper.

### C2. Props Shape Simplification

- [x] Audit prop structs and destructuring in `src/tui/props.rs`, `src/tui/screens/app/modal_overlays.rs`, and screen-specific modal files.
- [x] Replace repeated flat prop bundles with narrower typed prop structs or nested domain-specific prop groups where that reduces accidental mismatch.
- [x] Keep authoritative semantics out of prop assembly; this workstream is limited to layout and shape cleanup.

### C3. Template Component Consolidation

- [x] Review `account_setup_modal_template.rs`, `contact_select_modal_template.rs`, `form_modal_template.rs`, `text_input_modal_template.rs`, and related shared components.
- [x] Unify template components around presentational chrome and layout primitives only.
- [x] Do not hide ownership requirements, workflow gating, or terminality transitions inside generic template abstractions.

### C4. Props File Decomposition

- [x] Reduce `src/tui/props.rs` from 1054 LOC if it grows materially during cleanup; if it crosses 1500 LOC at any point, split it immediately into coherent files such as `screen_props.rs`, `modal_props.rs`, and `workflow_props.rs`.
- [x] Keep any newly introduced prop modules named after stable UI domains rather than temporary refactor phases.

### C5. Workstream C Verification and Commit

- [x] Run `cargo check -p aura-terminal`.
- [x] Run `cargo test -p aura-terminal --test integration_props`.
- [x] Run `cargo test -p aura-terminal --test e2e_guardian_display`.
- [x] Run `cargo test -p aura-terminal --test e2e_terminal_state`.
- [x] Run `just lint-arch-syntax`.
- [x] Confirm every targeted check is green before committing.
- [x] Create a commit such as `git add work/dry_terminal.md crates/aura-terminal && git commit -m "refactor(aura-terminal): simplify modal and props wiring"`.

## Workstream D — Terminal-Local Type Reduction

Target: delete terminal-local duplication where the terminal is not the correct
owner of the type.

### D1. Toast Model Simplification

- [x] Audit toast-related types in `src/tui/components/toast.rs`, `src/tui/context/toasts.rs`, and any queue or lifecycle adapters that shape toast state.
- [x] Separate render-facing payload, queued local state, and helper/context surfaces explicitly.
- [x] Unify types only when the resulting API still preserves the distinction between observed presentation and locally owned queue mechanics.

### D2. Mirror Type Elimination

- [x] Audit `src/tui/types.rs` and identify terminal-local types that mirror authoritative `aura-app` or `aura_app::ui_contract` concepts.
- [x] For each mirrored type, decide whether the terminal should:
- [x] consume the upstream authoritative type directly
- [x] wrap it in a thin presentation adapter
- [x] keep a terminal-local type because the terminal truly owns the concern
- [x] Prefer direct consumption of authoritative upstream types wherever the terminal is not the owner.
- [x] Replace `From`-shim chains and duplicated enums with typed adapters or extension traits when the shell only formats or groups upstream data.

### D3. Snapshot and Delegation Wrapper Collapse

- [x] Audit wrappers in `src/tui/context/snapshots.rs`, `src/tui/context/dispatch.rs`, `src/tui/harness_state/snapshot.rs`, and related delegation surfaces.
- [x] Remove trivial pass-through wrappers that add no local contract.
- [x] Keep wrappers that protect Layer 7 boundaries, preserve ownership categories, or enforce stronger typed inputs.

### D4. Type File Decomposition

- [x] Reduce `src/tui/types.rs` from 1774 LOC to under 1500 LOC.
- [x] Split the file into coherent modules with descriptive names such as `channels.rs`, `contacts.rs`, `settings.rs`, `recovery.rs`, and `presentation.rs` where that matches the real ownership split.
- [x] If snapshot or toast cleanup grows a supporting file beyond 1500 LOC, split it in the same workstream rather than carrying the debt forward.

### D5. Workstream D Verification and Commit

- [x] Run `cargo check -p aura-terminal`.
- [x] Run `cargo test -p aura-terminal --test integration_flow`.
- [x] Run `cargo test -p aura-terminal --test unit_dispatch_errors`.
- [x] Run `cargo test -p aura-terminal --test compile_fail`.
- [x] Run `cargo test -p aura-terminal --test integration_effect_commands`.
- [x] Run `just lint-arch-syntax`.
- [x] Run `just ci-observed-layer-boundaries`.
- [x] Confirm every targeted check is green before committing.
- [x] Create a commit such as `git add work/dry_terminal.md crates/aura-terminal && git commit -m "refactor(aura-terminal): reduce terminal-local mirror types"`.

## Workstream E — Test and Demo DRY

Target: shrink repetitive test setup while preserving scenario clarity and
coverage of boundary contracts.

### E1. Canonical Test Harness Builder

- [x] Audit repeated setup across `tests/e2e_terminal_state.rs`, `tests/integration_comprehensive.rs`, `tests/integration_flow.rs`, `tests/wiring.rs`, and `tests/support/`.
- [x] Extract one expressive builder path in `tests/support/` for repeated terminal-state setup.
- [x] Keep the builder explicit about authoritative seed state, callback wiring, demo-mode toggles, and expected observed outputs.
- [x] Avoid test builders that hide ownership handoff, runtime readiness, or authoritative input setup behind magic defaults.

### E2. Assertion Helpers

- [x] Introduce focused assertion helpers such as `assert_toast`, `assert_modal`, `assert_screen`, or `assert_semantic_status` only where they remove boilerplate without hiding important contract state.
- [x] Keep helpers domain-specific enough that failures still point to the relevant ownership or observed-state contract.

### E3. Demo Internal Cleanup

- [x] Audit repeated demo wiring in `src/demo/mod.rs`, `src/demo/simulator.rs`, and `src/demo/signal_coordinator.rs`.
- [x] Consolidate repeated agent-event handling and setup only if the demo path continues to exercise the same production-facing shell behavior where intended.
- [x] Do not let demo shortcuts become a second semantic execution model.

### E4. Large Test File Decomposition

- [x] Reduce `tests/e2e_terminal_state.rs` from 3627 LOC to under 1500 LOC by moving coherent groups into supporting modules under `tests/e2e_terminal_state/`.
- [x] Reduce `tests/integration_comprehensive.rs` from 1526 LOC to under 1500 LOC by splitting scenario groups into descriptive modules under `tests/integration_comprehensive/`.
- [x] If new helper extraction causes `tests/verification_generative.rs`, `tests/integration_effect_commands.rs`, or any other test file to cross 1500 LOC, split them in the same workstream.
- [x] Keep split file names descriptive of scenario families rather than generic `part1`/`part2`.

### E5. Workstream E Verification and Commit

- [x] Run `cargo check -p aura-terminal`.
- [x] Run `cargo test -p aura-terminal --test demo`.
- [x] Run `cargo test -p aura-terminal --features testing --test e2e_terminal_state`.
- [x] Run `cargo test -p aura-terminal --test integration_comprehensive`.
- [x] Run `cargo test -p aura-terminal --test verification_demo_itf`.
- [x] Run `cargo test -p aura-terminal --test verification_generative`.
- [x] Confirm every targeted check is green before committing.
- [x] Create a commit such as `git add work/dry_terminal.md crates/aura-terminal && git commit -m "refactor(aura-terminal): dry test and demo scaffolding"`.

## Workstream F — Handler and Entry-Point Decomposition

Target: keep terminal boot and handler composition readable and strongly typed
without turning Layer 7 startup code into a semantic owner.

### F1. TUI Handler Boundary Audit

- [x] Audit `src/handlers/tui.rs`, `src/handlers/tui_stdio.rs`, `src/tui/runtime.rs`, and `src/main.rs` for mixed concerns: startup wiring, stdio setup, runtime launch, demo selection, and shell handoff.
- [x] Identify areas where weakly typed mode switches, repeated startup tuples, or config branching can become typed structs or enums.
- [x] Keep handler code as shell composition and launch wiring; do not move business logic or semantic ownership into this layer.

### F2. Handler Decomposition

- [x] Reduce `src/handlers/tui.rs` from 1776 LOC to under 1500 LOC.
- [x] Split it into coherent files with descriptive names such as `startup.rs`, `demo_mode.rs`, `stdio.rs`, `runtime_boot.rs`, or `launch.rs` where those match the actual responsibilities.
- [x] Preserve one clear startup path per mode instead of accumulating wrapper-on-wrapper composition.

### F3. Entry-Point Typing Improvements

- [x] Replace repeated startup parameter clusters with typed config structs where they are currently threaded through multiple layers.
- [x] Prefer typed enums for launch mode and harness/demo options over string constants or bool combinations.
- [x] Keep ownership boundaries explicit when shell startup hands work off to runtime or app workflows.

### F4. Workstream F Verification and Commit

- [x] Run `cargo check -p aura-terminal`.
- [x] Run `cargo test -p aura-terminal --test integration_effect_commands`.
- [x] Run `cargo test -p aura-terminal --features testing --test e2e_terminal_state`.
- [x] Run `cargo test -p aura-terminal --test demo`.
- [x] Run `just ci-observed-layer-boundaries`.
- [x] Confirm every targeted check is green before committing.
- [ ] Create a commit such as `git add work/dry_terminal.md crates/aura-terminal && git commit -m "refactor(aura-terminal): split tui handler entrypoints"`.

## Explicitly Deferred

These may be revisited later, but they are not required for the initial pass:

- any cross-crate move into `aura-app` whose main justification is line count
- broad data-driven modal generation
- aggressive macro generation for TUI render code
- any refactor that needs new architectural exceptions to land

## Verification

Each workstream should run its targeted verification checklist above. In
addition, before merging the full series, run the full terminal boundary sweep:

1. `cargo check -p aura-terminal`
2. `cargo test -p aura-terminal`
3. `cargo test -p aura-terminal --test compile_fail`
4. `just lint-arch-syntax`
5. `just ci-parity-critical-callback-settlement`
6. `just ci-observed-layer-boundaries`
7. `just ci-frontend-handoff-boundary`
8. `just ci-actor-lifecycle`
9. `just build-dev && timeout 10 ./bin/aura tui --demo`

For any phase that changes subscriptions or readiness handling, add focused
reactive and degraded-state regression coverage before calling it complete.

For any phase that changes dispatch or callback submission, add focused tests
that prove parity-critical work still routes through the sanctioned ingress and
handoff paths.

## Measurement

Track these after each completed workstream:

- source LOC delta
- test LOC delta
- number of deleted duplicate helpers / wrappers / mirror types
- number of remaining parallel implementations for the same behavior

The review question is not "did this hit 25%?"

The review question is:

"Did this phase remove a real duplicate implementation, preserve the Layer 7
contract, and leave the crate easier to reason about than before?"

## Recommended Order

1. Workstream A
2. Workstream B
3. Workstream F
4. Workstream C
5. Workstream D
6. Workstream E

Reason:

- dispatch and ingress boundaries should be clarified before helper extraction
- reactive/callback contracts should be classified before more UI shell cleanup
- handler and startup typing should be cleaned up before test fixtures encode the old launch shapes
- type deduplication is safer after dispatch/subscription ownership is clearer
- tests should be consolidated after the production shapes stabilize

# Aura Terminal (Layer 7) - Architecture and Invariants

## Purpose
Terminal-based CLI and TUI interfaces for account management, authentication,
recovery, and diagnostics. Uses AppCore as unified backend while remaining
platform-agnostic.

## Inputs
- aura-app (pure application core: `AppCore`, `Intent`, `ViewState`).
- aura-agent (runtime layer: `AuraAgent`, `EffectContext`, services).
- aura-core (types only: errors, identifiers, execution modes).

## Outputs
- CLI handlers and command implementations.
- TUI screens, components, and layouts.
- Terminal-specific rendering and input handling.
- Human-friendly error messages and visualization.

## Invariants
- Must NOT create effect implementations or handlers.
- Must NOT be imported by Layer 1-6 crates.
- Uses dependency inversion: imports from both aura-app and aura-agent.
- Terminal-specific rendering must stay in this layer.
- Parse/validation failures for identifiers and thresholds are handled at UI/input boundaries.
- Internal command and modal contracts should prefer typed IDs/domain values over raw `String`.
- Operational responses for core flows should use structured variants instead of free-form strings.
- Parity-critical IDs, focus semantics, and action metadata must come from
  `aura-app::ui_contract`, not frontend-local derivation.
- Harness mode may add instrumentation or render-stability hooks, but it must
  not bypass normal user-visible execution semantics for parity-critical flows.
- The TUI must expose shared semantic command ingress through its real
  update/event loop for shared-flow execution; command handling may not depend
  on render-time polling or PTY timing.
- Renderer-specific key driving is frontend-conformance-only and must not be
  the primary shared-flow execution path.
- Parity-critical semantic export must not depend on placeholder IDs,
  override-backed lists, or heuristic runtime-event inference.
- The TUI is an `Observed` plus command-ingress surface for shared semantic
  flows. It may submit commands and render lifecycle, but it must not own
  terminal semantic truth for parity-critical operations.

## Ownership Model

For shared semantic flows, `aura-terminal` should use:

- `Observed`
  - render state
  - projections
  - snapshots
  - user-visible progress/status
- narrow `ActorOwned` ingress only where necessary
  - the TUI command/update loop may own command application mechanics because it
    is a long-lived mutable async frontend loop

It must not use:

- frontend-local `MoveOwned` semantic ownership for parity-critical operation
  truth
- callback-owned terminal success/failure
- shell-owned readiness synthesis

The correct split is:

- the TUI ingress loop is allowed to be actor-like because it is a real
  long-lived mutable event loop
- semantic operation ownership remains upstream in authoritative workflow/runtime
  coordinators
- the shell observes and renders lifecycle but does not decide it

The frontend handoff rule is strict:

- if a callback or shell-local owner settles the operation, it must publish a
  local terminal state itself
- if `aura-app` or runtime workflow ownership is required, the frontend must
  relinquish local ownership before awaiting that workflow
- there is no supported mixed state where the frontend keeps a local
  `Submitting` record alive while an app-owned workflow is running

The sanctioned frontend ownership boundary is also structural:

- local semantic-owner allocation is allowed only at the shell/update-loop
  submission boundary
- handoff into `aura-app` ownership is allowed only in callback factory
  boundaries
- authoritative operation-state mutation is allowed only in the shell/state
  reducer path that consumes authoritative updates
- TUI tests may exercise these helpers in
  [src/tui/semantic_lifecycle.rs](/Users/hxrts/projects/aura/crates/aura-terminal/src/tui/semantic_lifecycle.rs),
  but production code must not introduce new allocation/handoff sites outside
  the sanctioned boundary modules

The frontend also may not place best-effort work on the primary semantic-owner
path. Any best-effort local adaptation must happen after authoritative terminal
publication or under a different explicitly-owned local task that does not own
parity-critical lifecycle truth.

### Ownership Inventory

| Path | Category | Authoritative owner | May mutate | Observe only |
|------|----------|---------------------|------------|--------------|
| TUI command ingress queue and wakeup path | `ActorOwned` | TUI update/event loop | ingress/update-loop code | shell render code, harness |
| Shell-rendered semantic operation lifecycle | `Observed` | authoritative semantic facts from `aura-app` | local UI presentation state only | harness, user-visible rendering |
| Callback/subscription bridges for parity-critical flows | `Observed` | upstream workflow/runtime coordinators | local UI adaptation only; never terminal semantic truth | harness, shell |
| Local focus/selection and nonsemantic view state | `Observed` | TUI shell/model | shell/update-loop code | harness snapshots |

### Required Ownership Tests

Changes to parity-critical TUI ownership boundaries should ship with:

- dynamic tests proving dropped semantic-operation owners publish explicit
  terminal failure rather than hanging
- invariant tests proving authoritative and local operation snapshots do not
  regress terminal state on the same logical instance
- handle/instance tests proving stale or replaced operation handles do not
  match the wrong lifecycle record
- boundary tests showing relinquished callback ownership does not continue to
  author semantic truth after handoff
- invariant tests showing local `Submitting` state cannot mask authoritative
  terminal publication after required handoff

### Capability-Gated Points

- shared semantic command ingress and receipt handling through the real TUI
  update/event loop
- authoritative semantic lifecycle/readiness mirroring consumed from
  `aura-app::ui_contract` and `aura-app::workflows::semantic_facts`, never
  authored locally
- callback factories and subscription bridges that may adapt authoritative
  operation state for rendering, but may not publish terminal semantic truth

### Verification Hooks

- `cargo check -p aura-terminal`
- `cargo test -p aura-terminal harness_command_invite_actor_to_channel_emits_dispatch_followup -- --nocapture`
- `cargo test -p aura-terminal authoritative_submitting_after_terminal_allocates_new_instance -- --nocapture`
- `just ci-observed-layer-boundaries`
- `just ci-frontend-handoff-boundary`
- `just ci-actor-lifecycle`

Architecture/tooling split:
- direct import and boundary-shape violations should be pushed toward module
  visibility, sanctioned facade APIs, compile-fail tests, and Rust-native lints
- `just check-arch` remains the right place for repo-wide frontend integration
  checks, docs traceability, and semantic/reactive heuristics that depend on
  workspace context

### Detailed Specifications

### InvariantTerminalUiBoundary
Terminal interfaces must remain a presentation layer over aura-app and must not introduce runtime effect implementations.

Enforcement locus:
- src tui and command handlers map user intents to app workflows.
- User interface state changes are derived from reactive app signals.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.
- Shared harness execution depends on TUI render timing or PTY choreography
  instead of the normal command/update path.

Verification hooks:
- just check-arch and just test-crate aura-terminal

Contract alignment:
- [Aura System Architecture](../../docs/001_system_architecture.md) defines interface layer boundaries.
- [Effect System and Runtime](../../docs/103_effect_system.md) defines signal and workflow integration.
## Boundaries
- Business logic lives in aura-app.
- Effect implementations live in aura-effects.
- Runtime composition lives in aura-agent.
- Shared-flow command ingress and projection export belong here; shared command
  contract ownership does not.

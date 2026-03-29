# Aura Terminal (Layer 7)

## Purpose

Terminal-based CLI and TUI interfaces for account management, authentication, recovery, and diagnostics. Uses AppCore as unified backend while remaining platform-agnostic.

## Scope

| Belongs here | Does not belong here |
|-------------|---------------------|
| CLI handlers and command implementations | Effect implementations or handlers |
| TUI screens, components, and layouts | Business logic (lives in aura-app) |
| Terminal-specific rendering and input handling | Runtime composition (lives in aura-agent) |
| Human-friendly error messages and visualization | Shared-flow command contract ownership |
| Shared-flow command ingress and projection export | Parity-critical semantic lifecycle publication |
| Parse/validation at UI/input boundaries | Direct import by Layer 1-6 crates |

## Dependencies

| Direction | Crate | What is consumed / produced |
|-----------|-------|-----------------------------|
| Consumes | `aura-app` | `AppCore`, `Intent`, `ViewState`, `ui_contract`, `workflows::semantic_facts`, shared frontend primitives (`frontend_primitives`) |
| Consumes | `aura-agent` | `AuraAgent`, `EffectContext`, services |
| Consumes | `aura-core` | Types only: errors, identifiers, execution modes |
| Produces | — | CLI handlers, TUI screens, terminal rendering |

## Invariants

- Terminal interfaces must remain a presentation layer over aura-app.
- Parity-critical IDs, focus semantics, and action metadata must come from `aura-app::ui_contract`, not frontend-local derivation.
- Harness mode may add instrumentation or render-stability hooks but must not bypass normal execution semantics for parity-critical flows.
- The TUI must expose shared semantic command ingress through its real update/event loop; command handling may not depend on render-time polling.
- `src/tui/screens/app/shell/dispatch.rs` is the sanctioned event-loop-owned command ingress boundary for shell dispatch preparation, local owner allocation, and shell-state coordination.
- Direct semantic owner allocation stays behind the sanctioned submit helpers in
  `src/tui/screens/app/shell/dispatch.rs` and `src/tui/semantic_lifecycle.rs`;
  callback factories must call those helpers instead of allocating
  `LocalTerminalOperationOwner` or `WorkflowHandoffOperationOwner` ad hoc.
- Owner-typed callback families may invoke upstream `aura-app::ui::workflows::*` directly only when the callback API itself requires the correct ownership token at the boundary and the callback does not create a parallel terminal-owned semantic lifecycle path.
- Observed callbacks and ownerless helper utilities must not become alternate semantic ingress paths. If a flow is parity-critical and does not already enter through an owner-typed callback boundary, ownership allocation and submission must stay in the event-loop path rather than moving into render helpers or callback-free utility modules.
- Parity-critical semantic export must not depend on placeholder IDs, override-backed lists, or heuristic runtime-event inference.
- The TUI is an `Observed` plus command-ingress surface for shared semantic flows. It may submit commands and render lifecycle, but it must not own terminal semantic truth for parity-critical operations.
- Parity-critical callback families must require the appropriate owner type at the API boundary; ownerless callbacks are observed-only.
- Snapshot contention must be surfaced explicitly on parity-relevant paths; the shell may not treat lock contention as an empty authoritative state.
- Best-effort snapshot helpers may return defaults only for explicitly observed-only, non-authoritative reads such as deterministic tests or narrow display-only helpers. They must not be reused as an authoritative input surface for parity-critical decisions.
- Long-lived subscription exhaustion must become structural degraded state, not a log-only event.
- Selected-channel bindings may only reflect the current authoritative channel projection; the shell must not preserve a missing `context_id` from prior UI state.
- Parity-relevant terminal updates must choose an explicit publication class. Ordered-required and required-unordered updates must backpressure instead of silently degrading to best-effort `try_send`, while lossy publication is restricted to observed-only UI maintenance.
- Shared channel projection must be recomputed by one owned coordinator from authoritative `CHAT`, `SETTINGS`, and neighborhood-scope inputs. Any smoothing that keeps a selected DM-like channel visible during convergence must stay in a pure render-only adapter and must not fabricate canonical channel metadata.
- Channel-targeting flows must consume either a committed selection token carried forward from authoritative UI focus or a typed workflow-returned `ChannelBindingWitness`. The shell may not re-resolve or repair targets from channel names, last visible messages, or other heuristic UI state.
- Converted ceremony-monitoring paths must consume typed upstream lifecycle terminality and surface timeout or rollback-incomplete outcomes explicitly; the TUI may not silently discard those terminal states.
- Relative-time display clocks are local observed-only maintenance for formatting. They may refresh labels such as "2m ago", but they must not gate, infer, or repair parity-critical ceremony or readiness state.
- Slash-command outcome metadata must consume the upstream typed strong-command
  completion/degraded classification from `aura-app`; `aura-terminal` may
  format that metadata for users, but it must not infer semantic reason codes
  from local error-string matching.

### InvariantTerminalUiBoundary

Terminal interfaces must remain a presentation layer over aura-app and must not introduce runtime effect implementations.

Enforcement locus:
- `src/tui/` and command handlers map user intents to app workflows.
- User interface state changes are derived from reactive app signals.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.
- Shared harness execution depends on TUI render timing or PTY choreography instead of the normal command/update path.

Verification hooks:
- `just check-arch` and `just test-crate aura-terminal`

Contract alignment:
- [Aura System Architecture](../../docs/001_system_architecture.md) defines interface layer boundaries.
- [Effect System and Runtime](../../docs/103_effect_system.md) defines signal and workflow integration.

## Ownership Model

Reference: [docs/122_ownership_model.md](../../docs/122_ownership_model.md)

For shared semantic flows, `aura-terminal` uses `Observed` for render state, projections, snapshots, and user-visible progress. Narrow `ActorOwned` ingress is permitted only for the TUI command/update loop (a long-lived mutable async frontend loop). The frontend must not own terminal semantic truth for parity-critical operations; frontend-local submission ownership must hand off before the first awaited app/runtime workflow step per docs/122 section 16.

### Inventory

| Path | Category | Authoritative owner | May mutate | Observe only |
|------|----------|---------------------|------------|--------------|
| TUI command ingress queue and wakeup path | `ActorOwned` | TUI update/event loop | ingress/update-loop code | shell render code, harness |
| Shell-rendered semantic operation lifecycle | `Observed` | authoritative semantic facts from `aura-app` | local UI presentation state only | harness, user-visible rendering |
| Owner-typed callback bridges for parity-critical flows | `Observed` shell over upstream `MoveOwned` / `ActorOwned` coordination | upstream workflow/runtime coordinators | local adaptation and owned handoff only; never terminal semantic truth | harness, shell |
| Observed callback and subscription bridges | `Observed` | upstream workflow/runtime coordinators | local UI adaptation only; never terminal semantic truth | harness, shell |
| Local focus/selection and nonsemantic view state | `Observed` | TUI shell/model | shell/update-loop code | harness snapshots |

### Capability-Gated Points

- Shared semantic command ingress and receipt handling through the real TUI update/event loop.
- Owner-typed callback handoff into upstream `aura-app::ui::workflows::*` where the callback boundary already carries the required owner token and does not fork semantic lifecycle ownership.
- Authoritative semantic lifecycle/readiness mirroring consumed from `aura-app::ui_contract` and `aura-app::workflows::semantic_facts`, never authored locally.
- Callback factories and subscription bridges that may adapt authoritative operation state for rendering, but may not publish terminal semantic truth.
- Explicit shell-owned degraded-state publication for permanently failed frontend subscriptions.

### Verification Hooks

- `cargo check -p aura-terminal`
- `just lint-arch-syntax`
- `cargo test -p aura-terminal harness_command_invite_actor_to_channel_emits_dispatch_followup -- --nocapture`
- `cargo test -p aura-terminal authoritative_submitting_after_terminal_allocates_new_instance -- --nocapture`
- `just ci-observed-layer-boundaries`
- `just ci-frontend-handoff-boundary`
- `just ci-actor-lifecycle`
- `just ci-ownership-policy`

## Testing

### Strategy

UI boundary correctness and demo mode fidelity are the primary concerns. Tests are organized into `tests/demo/` for demo-mode flows, `tests/wiring/` for callback and signal dispatch, `tests/regression/` for bug regressions, and top-level files for integration, unit, and verification tests.

### Commands

```
cargo test -p aura-terminal
```

### Coverage matrix

| What breaks if wrong | Test location | Status |
|---------------------|--------------|--------|
| Demo mode diverges from production paths | `tests/demo/` (8 files) | Covered |
| Callback dispatches wrong operation | `tests/wiring/` (3 callback files) | Covered |
| Reactive signal dropped or glitches | `tests/wiring/integration_reactive_dispatch.rs` | Covered |
| Signal wiring incorrect | `tests/wiring/integration_signals.rs` | Covered |
| State machine invalid transition | `tests/unit_state_machine.rs` | Covered |
| Slash command parsed wrong | `tests/unit_slash_commands.rs` | Covered |
| Dispatch error not surfaced | `tests/unit_dispatch_errors.rs` | Covered |
| Guardian display E2E broken | `tests/e2e_guardian_display.rs` | Covered |
| Terminal state lifecycle wrong | `tests/e2e_terminal_state.rs` | Covered |
| Effect command integration broken | `tests/integration_effect_commands.rs` | Covered |
| Bridge integration broken | `tests/integration_bridge.rs` | Covered |
| Demo mobile enrollment regression | `tests/regression/regression_demo_mobile_enrollment.rs` | Covered |
| Guardian ceremony no-peers regression | `tests/regression/regression_guardian_ceremony_no_peers.rs` | Covered |
| ITF trace verification wrong | `tests/verification_demo_itf.rs` | Covered |

## References

- [Aura System Architecture](../../docs/001_system_architecture.md)
- [Effect System and Runtime](../../docs/103_effect_system.md)
- [Ownership Model](../../docs/122_ownership_model.md)
- [Testing Guide](../../docs/804_testing_guide.md)
- [Project Structure](../../docs/999_project_structure.md)

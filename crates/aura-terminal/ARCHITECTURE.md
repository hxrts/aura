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
| Consumes | `aura-macros` | Ownership declaration macros |
| Produces | — | CLI handlers, TUI screens, terminal rendering |

## Invariants

- Account commands are typed `command::Request` values executed by
  `command::execute` through `aura_app::ui::workflows` only, the functions
  the TUI and web call; `command::Response` is the typed result. The bpaf
  parsers build requests and nothing else, and `aura rpc` reads the same
  `Request` from JSON lines, so CLI and RPC responses are identical. The
  `cli-workflow-facade` check (`just ci-frontend-handoff-boundary`) rejects
  agent APIs, crate-root `aura_app::*` reach-ins, agent service accessors and
  local file access in `src/command`, `src/cli` and `src/rpc`, and any new
  module under `src/handlers` (offline tools and long-running modes only).

- One process holds an account's profile. A node (the TUI in production
  mode, or `aura serve`) hosts `rpc_socket` at `<data-dir>/aura.sock`: mode
  `0600`, same-uid peers only, no network listener. Account commands and
  `aura rpc` route to that socket first; otherwise the CLI opens the
  production runtime itself through `handlers::tui::open_production_runtime`,
  the TUI's own assembly. The published schema
  (`schema/aura-rpc-v1.json`) is generated from the types and checked by
  `just ci-rpc-schema`.

- `aura rpc` keeps one runtime online across requests. RPC events derive
  from the reactive signals the TUI observes; each subscription has a bounded
  queue and a lagging subscriber receives one `resync` snapshot instead of
  the dropped events.

- Every CLI command ends in one outcome: its structured `CliOutput` (text,
  or one `{"ok":true,"result":..}` document under `--json`) with exit code 0,
  or a typed `command::CommandError` whose `ErrorCode` fixes the exit code and
  whose message is worded by `user_errors::classify`. Under `--json`, stdout
  carries only that document. Destructive commands confirm through
  `command::confirm` (`--yes`), and `--timeout` is measured on the runtime
  clock.

- Runtime bring-up retains the original agent error as its native source.
  `AURA_SECURE_STORAGE_BACKEND` explicitly selects `platform` or
  `filesystem-fallback`; invalid values fail construction. The runtime owns
  admission of filesystem fallback. LAN tooling requests that provider to
  avoid credential prompts while retaining production runtime semantics.

- Device enrollment issuance accepts an explicit user-transferred setup code.
  Frontends forward it to the app-owned verification and issuance workflow;
  raw authority IDs, demo autofill and discovery metadata cannot replace the
  setup verifier pin or infer that the new device accepted enrollment.

- Terminal interfaces must remain a presentation layer over aura-app.
- Parity-critical IDs, focus semantics, and action metadata must come from `aura-app::ui_contract`, not frontend-local derivation.
- Harness mode may add instrumentation or render-stability hooks but must not bypass normal execution semantics for parity-critical flows.
- Terminal-local async task ownership must reuse the shared frontend task-root
  from `aura-app::frontend_primitives`; `src/tui/tasks.rs` may add terminal
  spawn wiring, but it may not keep a forked task-owner implementation.
- The TUI must expose shared semantic command ingress through its real update/event loop; command handling may not depend on render-time polling.
- `src/tui/screens/app/shell/dispatch.rs` is the sanctioned event-loop-owned command ingress boundary for shell dispatch preparation, local owner allocation, and shell-state coordination.
- Direct semantic owner allocation stays behind the sanctioned submit helpers in
  `src/tui/screens/app/shell/dispatch.rs` and `src/tui/semantic_lifecycle.rs`;
  callback factories must call those helpers instead of allocating
  `LocalTerminalOperationOwner` or `WorkflowHandoffOperationOwner` ad hoc.
- Owner-typed callback families may invoke upstream `aura-app::ui::workflows::*` directly only when the callback API itself requires the correct ownership token at the boundary and the callback does not create a parallel terminal-owned semantic lifecycle path.
- Observed callbacks and ownerless helper utilities must not become alternate semantic ingress paths. If a flow is parity-critical and does not already enter through an owner-typed callback boundary, ownership allocation and submission must stay in the event-loop path rather than moving into render helpers or callback-free utility modules.
- Parity-critical semantic export must not depend on placeholder IDs, override-backed lists, or heuristic runtime-event inference.
- The TUI harness snapshot copies `projection_source_revisions` from the same `AppCore` state snapshot as its observed entity lists. It must not substitute the frontend semantic/render revision for a source graph revision.
- The TUI is an `Observed` plus command-ingress surface for shared semantic flows. It may submit commands and render lifecycle, but it must not own terminal semantic truth for parity-critical operations.
- Parity-critical callback families must require the appropriate owner type at the API boundary; ownerless callbacks are observed-only.
- Contacts-screen friend-management affordances and dispatch rules must consume shared `ContactRelationshipState` and shared contact-action contracts from `aura-app`; the TUI may not keep a separate friendship state machine.
- Snapshot contention must be surfaced explicitly on parity-relevant paths; the shell may not treat lock contention as an empty authoritative state.
- Best-effort snapshot helpers may return defaults only for explicitly observed-only, non-authoritative reads such as deterministic tests or narrow display-only helpers. They must not be reused as an authoritative input surface for parity-critical decisions.
- Long-lived subscription exhaustion must become structural degraded state, not a log-only event.
- TUI subscriptions attach before reading their catch-up snapshot, and every shell or mounted-screen observer reports typed retry, healthy, and exhausted transitions through the shell update loop. The harness snapshot exposes this health; a fresh snapshot clears prior degradation.
- Selected-channel bindings may only reflect the current authoritative channel projection; the shell must not preserve a missing `context_id` from prior UI state.
- Parity-relevant terminal updates must choose an explicit publication class. Ordered-required and required-unordered updates must backpressure instead of silently degrading to best-effort `try_send`, while lossy publication is restricted to observed-only UI maintenance.
- Shared channel projection must be recomputed by one owned coordinator from authoritative `CHAT`, `SETTINGS`, and neighborhood-scope inputs. A newer `CHAT_SIGNAL` snapshot removes channels absent from it, including selected DM-like channels; shell rendering must not restore entities from a prior snapshot.
- Channel-targeting flows must consume either a committed selection token carried forward from authoritative UI focus or a typed workflow-returned `ChannelBindingWitness`. The shell may not re-resolve or repair targets from channel names, last visible messages, or other heuristic UI state.
- Converted ceremony-monitoring paths must consume typed upstream lifecycle terminality and surface timeout or rollback-incomplete outcomes explicitly; the TUI may not silently discard those terminal states.
- The TUI exports separate enrollment code-issuance and completion instances
  with the app-owned terminal state, cancellation, and typed failure domain/code.
  Its event loop may submit a guardian action only under the guardian operation
  kind after the invitation type is verified; it may not infer completion from
  a changed device count or a successful local import.
- Relative-time display clocks are local observed-only maintenance for formatting. They may refresh labels such as "2m ago", but they must not gate, infer, or repair parity-critical ceremony or readiness state.
- Slash-command outcome metadata must consume the upstream typed strong-command
  completion/degraded classification from `aura-app`; `aura-terminal` may
  format that metadata for users, but it must not infer semantic reason codes
  from local error-string matching.
- AMP channel transition state in notifications and harness snapshots must
  come from `RuntimeFact::AmpChannelTransitionUpdated` and the shared
  `aura-app::ui_contract` payload. The TUI may surface live successor,
  pending finalization, conflict, emergency quarantine, and cryptoshred
  consequences, but it must not reconstruct those states from local
  send/receive ratchet guesses.

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
| AMP transition notification projection | `Observed` | `aura-app::ui_contract::RuntimeFact::AmpChannelTransitionUpdated` | local presentation state only | harness snapshots, notifications screen |
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
| CLI `--json` output or exit codes wrong | `tests/cli_json.rs`, `src/command/error.rs` | Covered |
| CLI and RPC drift, RPC flows or events broken, CLI send differs from TUI send | `tests/cli_rpc.rs` (virtual-time two-runtime fixture) | Covered |
| CLI does not reach a running node, socket not owner-only, schema drift | `tests/cli_socket.rs`, `src/rpc/schema.rs` (`just ci-rpc-schema`) | Covered |
| CLI bypasses the app workflows | `toolkit/xtask` `cli-workflow-facade` | Covered |

## References

- [Aura System Architecture](../../docs/001_system_architecture.md)
- [Effect System and Runtime](../../docs/103_effect_system.md)
- [Ownership Model](../../docs/122_ownership_model.md)
- [Testing Guide](../../docs/804_testing_guide.md)
- [Project Structure](../../docs/999_project_structure.md)

## Enrollment trust transfer boundary

Device enrollment import uses the shared three-field contract and transfers submission ownership to the app before awaiting pin/import/acceptance. The shell does not derive an initiator verifier from the received payload or adopt its authority/device identifiers before acceptance. Issuer output preserves actual signed manifest and independent verifier transfer material. Browser account persistence requires an app-issued completed result; legacy pending-code-only replay fails closed.

See [cryptography](../../docs/100_crypto.md), [operation ownership](../../docs/109_operation_categories.md), [shared user flows](../../docs/121_user_flow_harness.md), and [testing](../../docs/804_testing_guide.md).

Sync command service ownership requires source-preserving timer, shutdown-signal and runtime supervision outcomes. Every daemon run exit awaits service stop. Failed execution remains primary when stop also fails, with a separately retained typed cleanup cause. A failed timer or backward physical clock cannot publish a tick or successful shutdown. Native terminal diagnostics retain concrete sources; cloned source-bearing errors compare retained source identity rather than matching message text.

Fresh runtime-free account bootstrap resumes in process with the original
`AppCore` and attaches its first runtime. It never reconstructs terminal account
success from persisted account metadata. Provisional runtime enrollment and
explicit authority switching retain the process reload path; this attachment
API does not prove physical provider transfer.

Fullscreen teardown drops hook futures, closes frontend task admission, and
requires actual admitted-future drainage within the owned terminal cleanup wait
before returning or starting another shell generation. A failed drain prevents
bootstrap reload; a simultaneous fullscreen failure retains both local causes.

## Runtime-free account creation ownership

The configured native staging adapter delegates actual pending-bootstrap and encrypted account-profile writes to the app-owned staging workflow. The original operation instance transfers before the first awaited producer step. Only acknowledged writes mint app-owned terminal success; frontend callbacks observe completion and request bootstrap reload. The retained AppCore supplies that original history to the new runtime signal graph. Runtime attachment rejects replacement, and pending-file reconciliation cannot manufacture account-create success.

Regression coverage must exercise actual storage production and observe the original operation after runtime attachment; manually seeded semantic facts do not establish producer ownership. The CreateAccount callback requires a workflow handoff owner, preventing submission with a local terminal owner.

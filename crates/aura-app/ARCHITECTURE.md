# Aura App (Layer 6)

## Purpose

Portable, platform-agnostic application core containing pure business logic (intents, reducers, views) without runtime dependencies. Enables dependency inversion through the `RuntimeBridge` trait.

## Scope

| Belongs here | Does not belong here |
|---|---|
| Pure workflow logic (intents, reducers, views) | Runtime assembly or lifecycle management (`aura-agent`) |
| Shared UI contract surfaces (`UiSnapshot`, `RenderHeartbeat`, `SHARED_FLOW_SUPPORT`, etc.) | Direct effect implementations |
| Shared semantic command-plane types | Long-lived mutable async state (`ActorOwned`) |
| Opaque operation handles and owner tokens | Platform-specific rendering logic (`aura-terminal`, `aura-web`) |
| Reactive signals (`CHAT_SIGNAL`, `SYNC_STATUS_SIGNAL`, etc.) | Handler composition or multi-handler coordination |
| `RuntimeBridge`, `OfflineRuntimeBridge`, `QueryHandler`, `ReactiveHandler` | Direct impure I/O or runtime imports from `aura-agent` |

## Dependencies

| Direction | Crates / surfaces |
|---|---|
| Consumes | `aura-core` (effect traits, domain types, ownership vocabulary), `aura-chat`, `aura-invitation`, `aura-recovery`, `aura-journal`, `aura-authorization` |
| Produces | `AppCore`, `Intent`, `ViewState`, `Screen`, view states (`ChatState`, `ContactsState`, `InvitationsState`, `RecoveryState`), `RuntimeBridge` trait, shared UI contract surfaces, reactive signals |
| Consumed by | `aura-agent` (runtime assembly), `aura-terminal` (TUI), `aura-web` (browser), `aura-harness` (test), `aura-testkit` (mocks) |

## Invariants

- **Pure logic**: no runtime dependencies or impure I/O.
- **Dependency inversion**: `aura-agent` depends on `aura-app`, never vice versa.
- **Push-based reactive flow**: Intent -> Journal -> Reduce -> ViewState -> Signal -> UI.
- **Frontend agnostic**: works with multiple platform frontends.
- **Shared-flow contract authority**: semantic UI ids, flow support declarations, typed command-plane metadata, and typed diagnostics are defined here.
- **Shared semantic ownership authority**: parity-critical semantic operation categories, typed terminal lifecycle, and owner-routed handles/tokens are defined here rather than in frontend-local crates.
- Platform-specific code isolated behind feature flags (`native`, `ios`, `android`, `web-js`).

### InvariantAppWorkflowPurity

Application workflows remain pure and frontend agnostic. Runtime effects are consumed through abstraction boundaries.

Enforcement locus:
- `src/workflows/` performs intent and state transitions.
- `src/core/` exposes platform-neutral integration surfaces.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- `just lint-arch-syntax`
- `just check-arch` and `just test-crate aura-app`

Contract alignment:
- [System Architecture](../../docs/001_system_architecture.md) defines dependency inversion.
- [Effect System](../../docs/103_effect_system.md) defines purity boundaries.

### InvariantSharedUiContractAuthority

`aura-app` is the authoritative home for shared semantic UI identity, shared semantic command-plane types, shared-flow parity declarations, shared screen/modal/list parity declarations, typed harness-visible diagnostics, shared focus/selection semantics, shared action/readiness metadata, and the machine-checkable screen/module map used for web/TUI parity enforcement.

Enforcement locus:
- `src/ui_contract.rs` defines semantic ids, `UiSnapshot`, `RenderHeartbeat`, `RuntimeEventSnapshot`, `SHARED_FLOW_SUPPORT`, `SHARED_SCREEN_SUPPORT`, `SHARED_MODAL_SUPPORT`, `SHARED_LIST_SUPPORT`, `SHARED_SCREEN_MODULE_MAP`, and shared semantic command-plane types.
- `src/ui.rs` re-exports the contract for harness and frontend consumption.

Failure mode:
- Frontends drift in naming or capability and harness scenarios stop being portable across TUI and web.
- Timeout diagnostics lose a single authoritative semantic contract.
- Frontends invent local command request or readiness shapes and shared-flow execution stops being uniform.

Verification hooks:
- `cargo test -p aura-app shared_flow_support_contract_is_consistent`
- `cargo test -p aura-app shared_screen_modal_and_list_support_is_unique_and_addressable`
- `cargo test -p aura-app shared_screen_module_map_uses_canonical_screen_names`
- `just ci-shared-flow-policy`

Contract alignment:
- [Testing Guide](../../docs/804_testing_guide.md) defines semantic shared-flow policy and timeout diagnostics.
- [Verification Guide](../../docs/806_verification_guide.md) defines the Quint/simulator/harness handoff around the shared contract.

## Ownership Model

See [docs/122_ownership_model.md](../../docs/122_ownership_model.md) for the full ownership framework.

For shared semantic flows, `aura-app` is primarily a `Pure` plus `MoveOwned` crate.

- `Pure` — typed workflow/domain transitions, readiness derivation rules, snapshot/projection shaping.
- `MoveOwned` — opaque operation handles, owner tokens / handoff objects, typed semantic lifecycle and failure contracts.
- not `ActorOwned` — long-lived mutable async service/runtime state belongs in `aura-agent`.
- not `Observed` — frontend render crates consume these contracts downstream.

If `aura-app` coordinates a parity-critical operation across async boundaries, one authoritative coordinator must own submission, phase advancement, terminal success/failure publication, and cancellation / owner-drop failure. Frontend crates may not invent parallel lifecycle ownership for those operations.

### Ownership Inventory

| Path | Category | Authoritative owner | May mutate | Observe only |
|------|----------|---------------------|------------|--------------|
| Semantic command request/receipt types | `Pure` | `aura-app::ui_contract`, `aura-app::scenario_contract` | contract modules | `aura-terminal`, `aura-web`, `aura-harness` |
| Parity-critical semantic operation lifecycle | `MoveOwned` | workflow-local semantic coordinator per operation | `aura-app::workflows::*`, semantic-fact publishers | frontends, harness |
| Invitation/channel/delivery readiness derivation rules | `Pure` + coordinator-consumed `ActorOwned` inputs | readiness coordinators in `aura-app::workflows::*` | workflow/coordinator modules only | frontends, harness |
| Opaque handles / owner-token / handoff surfaces | `MoveOwned` | current token/record holder through sanctioned APIs | contract/workflow transfer APIs | render/projection layers, harness diagnostics |

Strict authoritative-ref rule for parity-critical workflows:

- once a workflow has authoritative context, later helpers must consume the
  strongest available typed input such as `Authoritative*Ref`
- raw identifiers may reference but may not authorize
- parity-critical helpers may not re-resolve context, ownership, or readiness
  from weaker ids after authoritative handoff
- boundary-time name lookup may identify only already-materialized runtime
  channels; it may not mine pending invitations or renderer-local hints to
  repair missing authoritative context
- public by-name workflow entry points that succeed must return the canonical
  channel id they materialized so downstream Layer 7 code can carry that
  strong identity forward instead of rebinding by display name
- fallback/default helpers such as `*_or_fallback` are forbidden on
  parity-critical paths
- once canonical entity metadata has an owned materialization path, downstream
  reactive or observed code may not recreate that metadata from weaker facts
  such as membership-only events or raw ids

### Capability-Gated Points

- Authoritative semantic lifecycle publication in `src/workflows/semantic_facts.rs`.
- Authoritative readiness publication and replacement in `src/workflows/semantic_facts.rs`.
- Workflow-owned semantic operation phase/failure publication in `src/workflows/messaging.rs`, `src/workflows/invitation.rs`, and related parity-critical workflow modules.
- Opaque shared command-plane and lifecycle surfaces in `src/ui_contract.rs` and `src/scenario_contract.rs`.

Authoritative resolution is an explicit pre-step, not an implicit helper side
effect. Public parity-critical workflow APIs should either:

- resolve a strong typed reference once at the boundary, or
- require that strong typed reference directly

They must not accept raw ids and silently derive stronger truth internally.

Converted semantic-owner paths also follow two stricter publication rules:

- authoritative semantic-fact reads must fail explicitly when the authoritative
  signal is unavailable; owner code may not collapse that state to
  `Default::default()`
- runtime-backed hook installation must fail explicitly when the required task
  spawner is unavailable; Layer 6 may not report hook installation success and
  then silently skip authoritative refresh ownership
- converted ceremony-processing convergence in invitation/device-enrollment
  workflows must fail immediately on runtime processing errors; owner code may
  not log those errors and continue into later polling/count-based success tests
- accepting a pending home/channel invitation requires the current
  runtime-authoritative pending invitation witness; owner code may not spin a
  local retry window waiting for that invitation to appear after dispatch
- join-channel and pending-channel-accept workflows that return terminal-facing
  channel selection data must return a typed `ChannelBindingWitness` (and, for
  pending acceptance, an `AcceptedPendingChannelBinding`) from the owned
  workflow path instead of asking Layer 7 to rediscover the canonical channel
  identity by name or local projection heuristics
- each converted semantic domain should have one publication helper and one
  ownership label; context/home/neighborhood workflows must not drift into
  mirrored `views_mut().set_*` plus ad hoc signal emission paths
- converted homes and recovery projection publication now routes through the
  shared observed-projection helper path in `src/workflows/observed_projection.rs`;
  workflow modules must reuse that helper instead of defining local
  `emit_*_state_observed` variants
- parity-critical strong-command and semantic-query paths may not treat
  unverifiable scope/home state as success, and they may not upgrade legacy
  `dm:` descriptors or empty observed membership into canonical participant
  truth
- strong-command create intent may carry a normalized channel name, but it may
  not synthesize a canonical `ChannelId` or `CommandScope::Channel` before the
  runtime materializes that channel; until then, completion is `Accepted`, not
  replicated by fabricated identity
- strong-command execution owns the authoritative terminal-facing slash-command
  failure classification; Layer 7 renderers may format the shared
  status/reason metadata, but they may not derive semantic reason codes from
  local `AuraError` string parsing

## Testing

### Strategy

Workflow purity and shared UI contract authority are the primary concerns. Compile-fail tests in `tests/ui/` enforce type-level boundaries: private semantic owner types, handle consumption semantics, and workflow internals. Inline tests verify view reduction, shared contract consistency, and concurrent fact publication safety.

### Commands

```
cargo test -p aura-app
cargo test -p aura-app --test compile_fail         # semantic boundary tests
cargo test -p aura-app --test compile_fail_signals  # signal boundary tests
just ci-capability-boundaries
just ci-move-semantics
```

### Coverage matrix

| What breaks if wrong | Invariant | Test location | Status |
|---------------------|-----------|--------------|--------|
| Handle used after consumption | InvariantAppWorkflowPurity | `tests/ui/` cancel-after-cancel, accept-after-cancel, cancel-after-accept (3 compile-fail) | Covered |
| Semantic owner type accessible from frontend | InvariantSharedUiContractAuthority | `tests/ui/` *_private.rs (9 compile-fail) | Covered |
| String executor accepted where typed required | InvariantAppWorkflowPurity | `tests/ui/string_executor_rejected.rs` (compile-fail) | Covered |
| Shared flow support contract inconsistent | InvariantSharedUiContractAuthority | `src/ui_contract.rs` `shared_flow_support_contract_is_consistent` | Covered |
| Shared screen/modal/list not unique | InvariantSharedUiContractAuthority | `src/ui_contract.rs` `shared_screen_modal_and_list_support_is_unique_and_addressable` | Covered |
| Screen module map uses non-canonical names | InvariantSharedUiContractAuthority | `src/ui_contract.rs` `shared_screen_module_map_uses_canonical_screen_names` | Covered |
| Operation lifecycle allows terminal regression | InvariantAppWorkflowPurity | `src/ui_contract.rs` `semantic_operation_phase_generated_lifecycle_rejects_terminal_regression` | Covered |
| Concurrent fact updates lose entries | InvariantAppWorkflowPurity | `src/workflows/semantic_facts.rs` `concurrent_authoritative_fact_updates_do_not_lose_entries` | Covered |
| Operation lifecycle loses instance identity | InvariantAppWorkflowPurity | `src/workflows/semantic_facts.rs` `exact_operation_lifecycle_publication_retains_instance_identity` | Covered |
| Invitation accept succeeds before authoritative materialization | InvariantAppWorkflowPurity | `src/workflows/invitation.rs` `channel_reconcile_materialization_preserves_terminal_success`, `accept_pending_home_invitation_with_terminal_status_returns_direct_failure_status` | Covered |
| Messaging reducer parity regresses back to direct mutation | InvariantAppWorkflowPurity | `src/workflows/messaging.rs` `test_mark_message_delivery_failed_reduces_delivery_status`, `test_ensure_channel_visible_after_join_*`, `test_join_channel_success_implies_membership_ready_postcondition` | Covered |
| Signal boundary leaked | InvariantSharedUiContractAuthority | `tests/ui_signals/` (1 compile-fail) | Covered |
| Home role E2E flow broken | — | `tests/home_role_e2e.rs` | Covered |

## References

- [System Architecture](../../docs/001_system_architecture.md)
- [Effect System](../../docs/103_effect_system.md)
- [Ownership Model](../../docs/122_ownership_model.md)
- [Testing Guide](../../docs/804_testing_guide.md)
- [Verification Guide](../../docs/806_verification_guide.md)
- [Project Structure](../../docs/999_project_structure.md)

# Aura App (Layer 6)

AMP lifecycle failures retain concrete effect causes through the native runtime boundary. Canonical checkpoint absence has a private producer in the AMP journal reader. Scoped duplicate diagnostics require an exact requested entity and an independent successful canonical read before reconciliation; diagnostic wording and error records alone cannot suppress mutation failures. `AmpChannelError` carries source-bearing `AuraError` values and no longer promises equality; compare typed variants or stable categories. Foreign diagnostics explicitly discard native causes only at the presentation adapter.

## Purpose

Portable, platform-agnostic application core containing pure business logic (intents, reducers, views) without runtime dependencies. Enables dependency inversion through the `RuntimeBridge` trait.

## Scope

| Belongs here | Does not belong here |
|---|---|
| Pure workflow logic (intents, reducers, views) | Runtime assembly or lifecycle management (`aura-agent`) |
| Shared UI contract surfaces (`UiSnapshot`, `RenderHeartbeat`, `SHARED_FLOW_SUPPORT`, etc.) | Direct effect implementations |
| Shared semantic command-plane types | General long-lived mutable async state (`ActorOwned`) outside the narrow shared frontend task-root primitive |
| Opaque operation handles and owner tokens | Platform-specific rendering logic (`aura-terminal`, `aura-web`) |
| Reactive signals (`CHAT_SIGNAL`, `SYNC_STATUS_SIGNAL`, etc.) | Handler composition or multi-handler coordination |
| `RuntimeBridge`, `OfflineRuntimeBridge`, `QueryHandler`, `ReactiveHandler` | Direct impure I/O or runtime imports from `aura-agent` |

## Dependencies

| Direction | Crates / surfaces |
|---|---|
| Consumes | `aura-core` (effect traits, domain types, ownership vocabulary), `aura-chat`, `aura-invitation`, `aura-recovery`, `aura-journal`, `aura-authorization`, `aura-macros` (ownership declaration macros) |
| Produces | `AppCore`, `Intent`, `ViewState`, `Screen`, view states (`ChatState`, `ContactsState`, `InvitationsState`, `RecoveryState`), `RuntimeBridge` trait, shared UI contract surfaces, reactive signals |
| Consumed by | `aura-agent` (runtime assembly), `aura-terminal` (TUI), `aura-web` (browser), `aura-harness` (test), `aura-testkit` (mocks) |

## Module Layout

The crate uses explicit concern-owned submodules.

| Area | Current structure | Ownership intent |
|---|---|---|
| `src/core/app.rs` | `config.rs`, `hooks.rs`, `runtime_access.rs`, `signals.rs`, `state.rs` | Keep `AppCore` as the app boundary, not a mini runtime |
| `src/runtime_bridge.rs` | `bridge_trait.rs`, `offline.rs`, `types/{sync,ceremony,invitation,settings,offline_state}.rs` | Keep runtime inversion auditable by contract, DTOs, and offline behavior |
| `src/ui_contract.rs` | `ids.rs`, `operations.rs`, `shared_flow_support.rs`, `harness_metadata.rs`, `parity.rs`, `snapshots.rs` | Own shared semantic UI ids, shared-flow metadata, and typed diagnostics |
| `src/scenario_contract.rs` | `actions.rs`, `expectations.rs`, `submission.rs`, `values.rs` | Keep shared scenario and semantic command inventory explicit |
| `src/workflows/messaging.rs` | `channel_refs.rs`, `channels.rs`, `followups.rs`, `invites.rs`, `readiness.rs`, `routing.rs`, `send.rs`, `validation.rs` | One owned implementation path per messaging operation family |
| `src/workflows/invitation.rs` | `accept.rs`, `create.rs`, `device_enrollment.rs`, `export.rs`, `followups.rs`, `import.rs`, `pending_accept.rs`, `readiness.rs`, `utils.rs` | Separate invitation create/accept/import/export/readiness phases |
| `src/workflows/strong_command.rs` | `dispatch.rs`, `execute.rs`, `execution_model.rs`, `parse.rs`, `plan.rs`, `resolve.rs`, `resolved_refs.rs`, `snapshot.rs`, `terminal.rs` | Keep authoritative execution and failure classification inside app-owned workflow code |
| `src/workflows/semantic_facts.rs` | `owner.rs`, `publication.rs`, `lifecycle.rs`, `proofs/{issues,validation}.rs` | Separate authoritative mutation, publication, and proof shaping |
| `src/views/chat.rs`, `src/views/home.rs`, `src/views/recovery.rs` | split state/model/helper submodules | Keep view-state shaping explicit and scoped by concern |

## Invariants

- Shared scenario list counts are `u32` and runtime-event count observations
  are `u64`, so their wire ranges do not depend on frontend pointer width.

- Home channel mode is derived from authorized `SocialFact::HomeModeSet`
  governance register writes. Settings workflows commit the fact and report
  that commit; only governance reduction writes the observed `mode_flags`.

- **Pure logic**: no runtime dependencies or impure I/O.
- **Dependency inversion**: `aura-agent` depends on `aura-app`, never vice versa.
- **Home governance projection**: `views::home::reduce_home_governance` derives
  overrides, capability configuration, moderator roles, bans, mutes and kick
  history from a home's whole governance fact set. Writers obtain causal
  stamps through `RuntimeBridge::causal_stamp`; offline bridges cannot
  author governance or contact facts.
- **Contacts projection**: contact workflows stamp every `ContactFact` through
  `RuntimeBridge::causal_stamp`; `ContactFactLog` and
  `ContactsState::apply_reduced_contact` materialize contacts from the whole
  contact fact set.
- **Enrollment setup export**: `RuntimeBridge` exports the runtime-owned setup
  code with `EnrollmentSetupExportError` preserved across the inversion boundary.
  Unavailable exporters fail explicitly; callers cannot assemble the device/key
  snapshot from separate bridge reads. Export does not mint user-transfer trust.
  `export_device_enrollment_setup_code` bounds the runtime export call and
  preserves typed readiness, storage and deadline failures; submission adapters
  transfer its exact returned code rather than reconstructing device identity.
  The explicit user-transfer workflow verifies possession through a bounded
  runtime call before constructing private-field `UserTransferredEnrollmentSetup`.
  This pin has no deserialization path and is not durable authority/device trust.
  Enrollment issuance APIs require this strongest pin through owner and retry
  boundaries. Production issuance rechecks validity and uses its exact device
  identity; frontend migration and persisted replay/acceptance binding remain
  incomplete until their integration coverage passes.
- **Push-based reactive flow**: Intent -> Journal -> Reduce -> ViewState -> Signal -> UI.
- **Complete signal initialization**: each required app signal is ensured independently without resetting an existing value. A single registered signal never proves that the full set is ready; partial initialization is retryable.
- **Runtime hook ownership**: one `AppCore` hook group owns one subscription per required signal for one runtime attachment. Installation is serialized, enters `Installing`, and reaches `Ready` only after every receiver attaches and listener startup is acknowledged. Failure returns to `Stopped` with a typed reactive cause; detach drops the group and cancels its listeners.
- **Owned refresh loops**: each signal listener is the sole owner of its refresh pass. It receives the next update only after the current pass ends, so updates during a pass remain in the bounded stream. Lag may discard intermediate snapshots; the next pass reads current authoritative state and must converge without relying on every event being delivered.
- **Observed projection writer**: `ProjectionOwner` admits only the six canonical entity slots. Each delta is one serialized graph update; a replacement derived from an earlier snapshot must compare its source revision. `AppCore` mirrors committed graph values into `ViewState` only when their source revision exceeds the prior mirror. Source revisions in `StateSnapshot` and `UiSnapshot` are separate from frontend semantic/render revisions.
- **Canonical channel projection**: `ChatState::materialize_canonical_channel` requires the private-field `aura-chat::CanonicalChannelCreation` witness extracted from `ChannelCreated`. Metadata and message facts may be staged before creation but cannot make a channel visible. Duplicate creation replay preserves newer metadata; channel update fields use fact timestamps to reject stale values. Local-only channel/DM workflows issue owner-local creation facts through the same reducer. If join or invitation acceptance outruns signal delivery, the workflow retrieves a committed creation witness through `RuntimeBridge::canonical_channel_creation` using its authoritative channel/context binding; name hints cannot rebind channel identity.
- **Canonical home projection**: `HomesState::materialize_created_home` requires a private-field `HomeCreationWitness` extracted from `SocialFact::HomeCreated` through `ProjectionOwner`; raw home insertion is app-private. The witness proves the creation fact's shape, while the runtime owner must establish that the fact was committed before projection. Membership facts and invitation hints enrich an existing home but cannot create one. A join staged before creation is reconciled only when both its home ID and context match. Trusted query hydration uses an internal constructor; nonempty home and authority-keyed maps must survive JSON serialization and restart.
- **Navigation coherence**: app-owned homes selection and neighborhood position transitions share a gate with the homes signal mirror. The mirror rechecks the graph source revision after reconciling selection and position, so runtime publications arriving during a paired transition are applied before its mirror pass completes.
- **Replay mirrors**: the runtime hook group attaches `RECOVERY_SIGNAL` with the other observed entity signals and mirrors replayed chat and recovery values during installation, then mirrors later revisions through owned listeners. Recovery status and chat snapshots must not remain on older view cells after runtime fact replay.
- **Frontend agnostic**: works with multiple platform frontends.
- **Shared frontend task-root exception is narrow**: `frontend_primitives::FrontendTaskManager` may own cancellation/spawn state for Layer 7 shells, but `aura-app` must not grow general runtime service ownership.
- **Observed subscription health is typed**: `UiSnapshot.subscription_health` carries one frontend observer state per signal, including typed registration, stream-closure, and snapshot-read failures. Frontend owners update the field across attachment and recovery; a missing observer must not appear healthy.
- **Shared-flow contract authority**: semantic UI ids, flow support declarations, typed command-plane metadata, and typed diagnostics are defined here.
- **Shared semantic ownership authority**: parity-critical semantic operation categories, typed terminal lifecycle, and owner-routed handles/tokens are defined here rather than in frontend-local crates.
- The frontend `ui::contract` facade exports `SemanticOperationKind` alongside
  operation identifiers. Frontends read retained `UiOperationHandle` instances
  through the public typed accessor; private handle fields remain inaccessible.
- **Contacts relationship authority**: `ContactRelationshipState` and shared friend-management control availability are defined here and derived from runtime-fed projections rather than frontend-local state machines.
- **Canonical contact creation**: `ContactsState::apply_contact` requires a `ContactAddedWitness` derived from `ContactFact::Added` by the owned projection path. The raw Added-fact witness constructor is app-private; `ProjectionOwner` supplies it to the runtime projector. `set_relationship_state` enriches an existing contact only, so friendship and guardian evidence cannot create a minimal contact. `from_contacts` remains typed query hydration and test-fixture support, not a publication path.
- **Canonical invitation creation**: `InvitationsState::add_invitation` requires an `InvitationCreationWitness` derived from `InvitationFact::Sent` by the owned projection path or an `aura-invitation::shareable::ValidatedImportedInvitation` token minted after code and sender-proof verification. The raw Sent-fact witness constructor is app-private; `ProjectionOwner` supplies it to the runtime projector. Raw cached invitations and status-only facts cannot create pending rows. `from_parts` remains typed query hydration and test-fixture support, not a publication path.
- Platform-specific code isolated behind feature flags (`native`, `ios`, `android`, `web-js`).

### InvariantAppWorkflowPurity

Application workflows remain pure and frontend agnostic. Runtime effects are consumed through abstraction boundaries.

Enforcement locus:
- `src/workflows/` performs intent and state transitions.
- `src/core/app.rs` and `src/core/app/*.rs` expose platform-neutral integration surfaces.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- `just lint-arch-syntax`
- `just check-arch` and `just test-crate aura-app`
- `just ci-ownership-policy`

Contract alignment:
- [System Architecture](../../docs/001_system_architecture.md) defines dependency inversion.
- [Effect System](../../docs/103_effect_system.md) defines purity boundaries.

### InvariantSharedUiContractAuthority

`aura-app` is the authoritative home for shared semantic UI identity, shared semantic command-plane types, shared-flow parity declarations, shared screen/modal/list parity declarations, typed harness-visible diagnostics, shared focus/selection semantics, shared action/readiness metadata, and the machine-checkable screen/module map used for web/TUI parity enforcement.
AMP channel transition diagnostics are part of this contract: transition state,
A2 live/conflict state, emergency policy, suspect authorities, quarantine and
cryptoshred/prune status, and accusation/cooldown evidence are represented once
in `src/ui_contract/snapshots.rs` for both frontends.
Shared AMP transition affordances are also rooted here. The frontend-visible
controls and operation ids for emergency alarm, quarantine approval,
cryptoshred approval, conflict-evidence viewing, and finalization-status
viewing live in `src/ui_contract/ids.rs` and `src/ui_contract/operations.rs`;
frontend crates may render or submit them, but they may not mint alternate
semantic ids.

Enforcement locus:
- `src/ui_contract.rs` defines the shared contract boundary, with ownership split across `src/ui_contract/ids.rs`, `operations.rs`, `shared_flow_support.rs`, `harness_metadata.rs`, `parity.rs`, and `snapshots.rs`.
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
| Authoritative semantic-fact storage | `MoveOwned` | `AppCore` semantic-fact store with workflow-owned mutation helpers | `aura-app::workflows::semantic_facts`, sanctioned owner helpers | signals, frontends, harness |
| Runtime refresh hook group | `MoveOwned` | `AppCore::hook_install_state` for the attached runtime | `src/core/app/hooks.rs`, `src/workflows/system/hooks.rs` | frontends, harness |
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
- Workflow-owned semantic operation phase/failure publication in the parity-critical workflow families under `src/workflows/messaging/*.rs`, `src/workflows/invitation/*.rs`, `src/workflows/strong_command/*.rs`, and related semantic-owner modules.
- Workflow-owned readiness publication helpers in `src/workflows/messaging/readiness.rs` and `src/workflows/invitation/readiness.rs` now carry declaration-layer capability-boundary markers when they mint or publish authoritative readiness state directly.
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
- authoritative semantic facts now live in one `AppCore`-owned store; signal
  emission mirrors that store for observers but is not the source of truth for
  parity-critical workflow reads or updates
- strong-command completion barriers now return typed completion witnesses or
  explicit degraded outcomes such as runtime-unavailable and timed-out; app or
  frontend code may not encode those states as string-parsed `AuraError`
  payloads
- invitation acceptance policy uses native structural failure reasons retained
  through the original error chain. Only an observed `Accepted` status may
  classify a failed attempt as already handled; `Cancelled`, `Expired`, and
  `Declined` remain distinct failures. Contact confirmation reasons and
  deadlines select stable semantic codes without parsing diagnostic text.
  Device enrollment acceptance failures settle the same owner that dispatched
  the workflow and return the original runtime or convergence cause.
- workflow context wrappers accept concrete standard errors and retain their
  original causes. Converting a workflow error to `AuraError` preserves the
  typed context; core passthrough preserves its category and direct source.
  Time-query and parity-time failures retain their bounded/runtime sources.
  Human-readable detail is diagnostic only and cannot replace source provenance.
- runtime bridge composition is the outbound error-classification boundary:
  runtime-facing implementations may keep local error styles internally, but
  native acceptance and time calls return `RuntimeBridgeError`, retaining
  original causes and an exhaustive typed diagnostic kind. The existing foreign
  `IntentError` enum stays unchanged. Only an explicit terminal diagnostic
  adapter may omit native sources; diagnostic text never authorizes a retry.
  Other bridge operations remain scheduled for the same native migration
- runtime-backed hook installation must fail explicitly when the required task
  spawner is unavailable; Layer 6 may not report hook installation success and
  then silently skip authoritative refresh ownership
- hook installation must attach every required signal receiver before publishing
  `Ready`; a failed attachment leaves no live partial group, and retry must
  preserve existing signal values while restoring the full set of listeners
- detaching or replacing a runtime must cancel its old hook group before the
  next generation can become `Ready`; the install gate serializes these changes
- app-owned system refresh hooks may coalesce repeated events, but they must
  not silently drop refresh/publication failures inside a pass; hook-owned
  diagnostics must retain the first failure explicitly
- each signal's refresh loop must remain serialized within its hook group;
  an update received during an in-progress pass must still lead to a later
  pass, while lagged intermediate values may be skipped in favor of current
  authoritative state
- converted ceremony-processing convergence in invitation/device-enrollment
  workflows must fail immediately on runtime processing errors; owner code may
  not log those errors and continue into later polling/count-based success tests
- enrollment issuance has one app semantic owner per submission. Both the
  verified setup entry point and the user-transferred code entry point publish
  their own start proof from the actual runtime result; common runtime preparation
  never publishes success and never nests a second owner.
- enrollment failure codes are selected structurally from typed setup and issuance
  errors, retaining the original cause through the standard error source chain.
  Setup validity failures use `InvalidArgument`: a setup may be expired or not
  yet valid and is not an issued invitation. Invalid possession signatures and
  proof bindings use `PermissionDenied`; bounded workflow deadlines use
  `OperationTimedOut`. Local cryptographic, time, and retained-package failures
  use `CeremonyRuntimeFailed`. Message text never determines these codes.
- device-enrollment code issuance and completion use separate semantic
  operation instances linked by ceremony ID. The app/runtime hook group owns
  completion observation and reattachment across frontend remount and runtime
  restart; terminal publication consumes the runtime's typed outcome exactly
  once. A frontend-local monitor cannot own this lifecycle.
- `OperationSnapshot` retains the authoritative operation instance, state,
  and stable failure domain/code. Cancellation remains distinct from failure,
  and guardian acceptance cannot publish success until the runtime supplies
  authenticated post-verification completion evidence.
- channel join requires every canonical runtime read to succeed, including
  the read after a rejected join attempt. Query failure cannot become absent
  state, allow another mutation, or publish membership readiness. The
  `signals` test lane injects failures after successful earlier reads and checks
  original query sources, mutation counts, and absent membership publication.
- channel-membership readiness facts are owner-published and runtime-revalidated;
  refresh helpers may reconcile or prune existing authoritative facts, but they
  may not mine `observed_chat_snapshot` or renderer-local chat projection state
  to seed new authoritative readiness truth
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
- `ChatState` serializes channels only in the canonical `HashMap<ChannelId,
  Channel>` form, and app/workflow callers must iterate messages per channel
  explicitly rather than relying on broad compatibility helpers
- home creation in `src/views/home.rs` requires a `HomeCreationWitness`;
  callers that remove homes choose any fallback selection explicitly instead
  of depending on implicit wrapper policy
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

Workflow purity and shared UI contract authority are the primary concerns. Compile-fail tests in `tests/ui/` enforce type-level boundaries: private semantic owner types, handle consumption semantics, and workflow internals. Inline tests verify view reduction, shared contract consistency, runtime-bridge/query ownership splits, and concurrent fact publication safety.

### Commands

```
cargo test -p aura-app
cargo test -p aura-app --test compile_fail         # semantic boundary tests
cargo test -p aura-app --test compile_fail_signals  # signal boundary tests
just ci-capability-boundaries
just ci-move-semantics
just ci-ownership-policy
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
| Invitation accept succeeds before authoritative materialization | InvariantAppWorkflowPurity | `src/workflows/invitation.rs` `channel_reconcile_materialization_preserves_terminal_success`, `accept_pending_channel_invitation_with_terminal_status_returns_direct_failure_status` | Covered |
| Messaging reducer parity regresses back to direct mutation | InvariantAppWorkflowPurity | `src/workflows/messaging.rs` `test_mark_message_delivery_failed_reduces_delivery_status`, `test_ensure_channel_visible_after_join_*`, `test_join_channel_success_implies_membership_ready_postcondition` | Covered |
| Signal boundary leaked | InvariantSharedUiContractAuthority | `tests/ui_signals/` (1 compile-fail) | Covered |
| Home role E2E flow broken | — | `tests/home_role_e2e.rs` | Covered |

Pending invitation acceptance distinguishes absent pending entities (`NotFound`)
from a selected entity of the wrong kind (`InvalidState`) with explicit owner
error variants. Both remain recoverable through the native error source chain;
display text does not choose the semantic code. The acceptance owner regression
`pending_selection_failures_have_explicit_semantic_codes_and_typed_causes`
enforces this mapping.

Pending invitation selection propagates authoritative lookup and required
invitation readiness refresh failures. Account-wide settings/recovery enrichment
is explicitly best-effort for this operation. Cached pending or accepted entries cannot repair a
lookup error. Accepted-history recovery uses a typed not-materialized cause or
an actual deadline; clock unavailability and invalid policy remain failures.
The signals-enabled lookup fault regression and the readiness failure matrix
enforce these boundaries.

## References

- [System Architecture](../../docs/001_system_architecture.md)
- [Effect System](../../docs/103_effect_system.md)
- [Ownership Model](../../docs/122_ownership_model.md)
- [Testing Guide](../../docs/804_testing_guide.md)
- [Verification Guide](../../docs/806_verification_guide.md)
- [Project Structure](../../docs/999_project_structure.md)

Native AMP checkpoint resolution, staged transition diagnostics, materialized name identification, and membership repair return `RuntimeBridgeError`. Required query causes remain available through standard error sources; foreign diagnostic conversion is explicit. Deadline exhaustion is a timeout, while clock unavailability and invalid budgets remain distinct failures.

## Enrollment trust transfer boundary

The app owns explicit independently transferred initiator manifest selection and one ImportDeviceEnrollmentCode operation covering pin, runtime import, acceptance, and convergence. Raw form/semantic inputs are not trust tokens. A completion result has private fields and is minted only after verified acceptance/adoption; frontend identity persistence consumes that result. Typed missing-pin causes survive the workflow and handoff error chain.

See [cryptography](../../docs/100_crypto.md), [operation ownership](../../docs/109_operation_categories.md), [shared user flows](../../docs/121_user_flow_harness.md), and [testing](../../docs/804_testing_guide.md).

### Native settings and identity failures

Required identity/settings bridge queries and mutations retain `RuntimeBridgeError` through bounded runtime calls and workflow error sources. `settings_snapshot` preserves budget failures and native query causes; its `None` represents an explicitly absent runtime. Aggregate runtime status retains native authentication failures, while its legacy sync/rendezvous diagnostic components do not provide stronger readiness evidence. Trait signatures and native-error compile-fail guards prevent implicit conversion back into `IntentError`.

### Native failure projection

Native runtime failures keep their category and concrete sources through required settings workflow context. Callback foreign error payloads project the native category directly; crypto and codec failures have explicit `crypto_error` and `serialization_error` codes. Semantic projection traverses retained native sources and classifies all native categories exhaustively. Diagnostic words never supply category evidence. The callback payload remains a terminal display projection and cannot carry a Rust error source.

### Native semantic failure codes

Native crypto, serialization, storage, journal, and reactive failures project to distinct shared semantic failure codes. Typed native evidence is read through retained workflow sources before legacy terminal classification. Foreign callback codes and shared semantic snapshot codes remain explicit presentation projections, never trust evidence.

### Native account bootstrap failures

Account initialization retains native runtime sources through failure publication and return. Signing bootstrap is one required bounded operation on the existing runtime owner on both native and browser; permanent native faults are not rewritten as transient readiness or retried blindly. Failed semantic publication retains both the original bootstrap cause and publication failure structurally.

### Typed command failure classification

Authoritative strong-command execution retains typed resolver and plan failures as standard sources. Terminal classification uses these domain variants and structural native categories; Invalid and PermissionDenied display text cannot assert missing scope, stale snapshots, membership denial, mute, or ban subreasons.

### Typed moderation decisions

Messaging send/join denial is derived from the required runtime-owned moderation status and retains exact context, channel, and authority in `ModerationDenial`. NotMember/Muted/Banned semantics are read from retained typed sources. Send failure goes through its existing semantic owner terminal publisher; failed status reads do not manufacture a denial subreason.

Enrollment import completion retains the original provisional identity,
invitation, ceremony, pending epoch, setup digest and independent manifest digest
alongside the resulting subject/device. These values identify the runtime-owned
immutable confirmation receipt for subsequent profile handoff; they are not
serializable completion capabilities or authority reconstructed from received
identifiers. Completion is returned only after authenticated committed
confirmation and sanctioned exact-generation activation succeed.

### Enrollment acceptance ownership

Enrollment acceptance terminal publication belongs to the annotated acceptance owner. Both the import wrapper and direct acceptance wrapper await that owner; neither publishes a second success. The acceptance owner requires runtime-confirmed activation and settled local state before publishing the typed imported proof.

### Checkpoint failure category

Required timeout checkpoint storage/codec failures retain their actual lower-owner category through semantic projection. The shared budget classifier walks retained sources; actual clock failures remain unavailable and actual elapsed deadlines remain timed out. Diagnostic wording and unclassified IO do not identify a service category.

### Required refresh attachment health

One hook group owns its attached signal streams, bounded first failure, and shared cancellation. Required signal receipt or interval failure cancels the entire group and returns the original native error to the fallible runtime spawner. A failed signal-driven refresh belongs to that update only: the listener retains it as a typed per-update failure (`AppCore::refresh_hook_update_failures`, stage `Refresh`, original cause) and keeps serving later updates, which re-read current state. `AppCore::refresh_hook_failure` exposes the typed stage and original cause of a terminal failure even without tracing. Failed groups are inactive; explicit reattachment replaces attachment health but does not clear the runtime supervisor's retained failure. Cancellation alone remains successful task completion.

### Enrollment terminal failure projection

The native runtime bridge retains a structural enrollment terminal reason in
addition to its original cause. Shared workflow projection handles all terminal
reasons exhaustively before generic native categories; cancellation and rejection
remain distinct from crypto verification faults. The reason is a diagnostic, not
a signed-proof constructor or retry witness. The foreign IntentError payload
contract remains unchanged.

Required flow accounting denial projects to `BudgetExceeded` across native UI,
semantic operation and command terminal contracts. Authorization remains a
separate category. Exhaustive enum mappings and source-boundary regressions
enforce this distinction; foreign display diagnostics never supply policy proof.

Compile-fail test harnesses share aura-testkit's workspace process lock with agent and signals suites. Descriptor custody releases on process exit, and bounded acquisition retains native IO or contention causes. Tests never remove/recreate the lock inode or create a parallel suite-specific lock namespace. See docs/804_testing_guide.md.

### Exact capability declaration evidence

A capability boundary declares its exact capability type in a parsed input or output. Semantic labels may specify `capability_type = Type`; labels and body text do not establish custody. Accessors return that exact type, authorizers retain that typed input or output, and proof issuers also declare their authoritative proof source. Runtime helpers with an actual held receiver may specify `receiver_type = OwnerType`; expansion checks the concrete receiver against that type. This receiver contract does not apply to free functions or replace authorization inputs in authorizers.

Constants, capability-like substrings, incidental body calls, phantom markers and associated projections do not satisfy the declaration. The declaration verifies API shape; private constructors and actual runtime ownership validation establish authority. Pure validators, pure execution-plan builders and observed projections are not capability issuers and carry no decorative capability-boundary declarations. Their domain tests and effect-placement rules remain required.

### Bootstrap runtime attachment

A runtime-free bootstrap `AppCore` may attach its first runtime while retaining
its original authoritative semantic operation history. Attachment rejects an
existing runtime, including after detachment through an irreversible spent
witness; it does not authorize provider replacement or retirement.
The app unit regression checks history continuity and rejects replacement.

Frontend task admission and shutdown share one atomic closed/count state. Every
admitted future, including escaped owned-spawner tasks, retains a completion
lease until its actual future is destroyed. Portable async serialized observers
wait for zero retained tasks; cancellation flags are not drainage proof. The
required workspace regressions cover escaped noncancellable work, closed
admission, and destruction of an unpolled original future before acknowledgment.
Bootstrap attachment mirrors existing app-owned semantic history into the new
observed signal graph before refresh hooks, without issuing new terminal facts.

## Runtime-free account creation ownership

The configured native staging adapter delegates actual pending-bootstrap and encrypted account-profile writes to the app-owned staging workflow. The original operation instance transfers before the first awaited producer step. Only acknowledged writes mint app-owned terminal success; frontend callbacks observe completion and request bootstrap reload. The retained AppCore supplies that original history to the new runtime signal graph. Runtime attachment rejects replacement, and pending-file reconciliation cannot manufacture account-create success.

Regression coverage must exercise actual storage production and observe the original operation after runtime attachment; manually seeded semantic facts do not establish producer ownership. The CreateAccount callback requires a workflow handoff owner, preventing submission with a local terminal owner.

### Explicit original-device enrollment signing consent

Shared workflows select a versioned transfer intent and issue a move-only
`UserApprovedEnrollmentSigningIntent` only for explicit local approval through
its original runtime bridge. Consent covers exact manifest, public v3 invitation
transport and initial Request transcripts. The runtime independently validates
current active membership, native policy and local share custody. Public bytes,
remote packets, parsed transfer data and matching ids cannot manufacture this
approval. Its public API forbids construction, Clone and deserialization.

### Enrollment approval origin identity

Explicit user approval retains its original runtime bridge allocation. Runtime
admission compares that allocation's data address, since compiler-generated
trait vtables may be duplicated at different addresses across codegen units. Concrete and
dynamic references to the same retained object preserve origin; a separately
allocated bridge over the same native agent remains foreign. Authority IDs,
effect equivalence, and public metadata cannot replace this owner identity. The
actual native quorum fixture verifies both original-reference admission and
foreign-facade rejection before consuming explicit participant approval.

Channel readiness uses the exact full participant count from a successful strong
native channel read. Observed row counts and prior readiness facts cannot widen
that count; recipient resolution derives from the same participant read. Required
participant lookup errors retain their native cause through the workflow boundary.

The shared `UiSnapshot.home_modes` observation is required on observation surface version 2. `observed_home_modes` projects canonical `HomesState` entries with channel/context bindings and mode flags; entries without a canonical context are omitted. It does not establish readiness or authorize mutations. Loading snapshots explicitly carry an empty vector, and no legacy wire decoder supplies the field.

Frontend signal observers await `AppCore::subscribe_attached` before their initial snapshot and healthy publication. The existing reactive owner installs the graph receiver before returning, so an immediate update cannot fall between reported attachment and actual admission.

Native integration tests may opt into app `test-support` fixture publication, which delegates existing observed-projection owners to publish a detached HomesState or mutate only the mode of an exact materialized home. The feature is absent from default/wasm production frontends and does not add a production mutation bypass.

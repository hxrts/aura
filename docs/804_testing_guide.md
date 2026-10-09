# Testing Guide

Browser onboarding regressions must check guarded signal writes after async
enrollment workflows, including identity persistence failures. Completion and
error feedback use the browser retry helper; task spawning remains owned by
the shared web task owner.

For runtime tests that manually advance physical time, inject
`aura_testkit::time::ManualPhysicalClock` before service assembly. Its sleeps wait
for explicit clock advancement, so background cleanup loops cannot consume the
ceremony lifetime by advancing time themselves. Retain the same provider across
participants where the scenario requires one physical clock; restart tests must
also retain original durable window evidence. Use real held ownership capabilities
for persistent enrollment fixtures rather than constructing raw registration state.

Multi-agent runtime tests (several `AgentBuilder` runtimes on one
`SharedTransport`) share one `aura_testkit::time::QuiescentClock`, injected with
`AgentBuilder::with_physical_time_provider`, and run under
`#[tokio::test(start_paused = true)]`. The clock advances in small virtual
steps only when every task is idle, so runtime timeouts, retries and periodic
sync fire at the same logical point regardless of host load. Bound waits in
virtual time and re-check conditions at quiescent points (see
`crates/aura-agent/tests/support`); never wait on wall-clock sleeps.

## AMP lifecycle replay

The leave action drives the native runtime key ceremony and member consensus
through the simulation factory's bounded, lexically owned peer drivers.
Members must agree on canonical membership and epoch before the attempt.
Each member verifies the resulting epoch commit against its own DKG group
key before retaining the agreement evidence. The replay rereads that exact
evidence from each member's canonical typed-fact store and checks identical
successor base keys for remaining members and no successor key for Carol.
This covers membership rekeying; the later observational transition-policy
steps still do not establish native normal/emergency finalization.
An adjacent regression enforces a 16 KiB caller-future budget for membership
rekeying, alongside execution on the default test stack.

`just ci-test` first runs `just ci-amp-lifecycle-trace`, which regenerates the
seed-424242, 24-step AMP harness trace and compares it with
`verification/quint/traces/amp_channel.itf.json`. The workspace test lane runs
`amp_channel_itf::replay_amp_channel_lifecycle_trace` against real simulation
agents for steps 1–14. Steps 15–24 use a private simulator-only observational
transition-policy adapter: original scope binding, model phase sequencing,
single-live-successor, conflict suppression and emergency policy invariants.
These steps publish no native certificate/finalization facts, mint no signatures
or consensus IDs, and prove neither cryptographic admission nor physical
cryptoshred. Actual A2 witness issuance and owned A3 committee coverage remain
unfinished in `work/8.md` (systemic `S` tasks). Missing artifacts and replay failures fail the test; they cannot skip
execution. Each fixture agent must complete native threshold-service authority
bootstrap before replay, retaining its protected genesis, active epoch and
physical signing allocation. Do not replace that setup with channel bootstrap
metadata or raw seeded epoch/key records. The full replay then checks actual
invitation issuance and acceptance. Join delivery must retain the actual source
producer's membership entries and the original channel checkpoint required by
canonical reduction, with exact context/channel selection and original entry
keys, order and payload. The source actor alone produces a departure; peers
replicate that original Left event and acknowledge absence. Missing evidence or
failed canonical acknowledgment must fail the replay; final membership assertions
must not repair state. Schema-one reduction tests must invert opaque tokens,
insertion and journal merge order, reject unversioned rejoin and all-departed
sender bypass, and preserve original clock failure with no append. Selected
transition commitments are not successor roster/inclusion evidence; that owned
witness remains an explicit unfinished task in `work/8.md` (systemic `S` tasks). To regenerate
the fixture, run:

```sh
QUINT_TRACE_MAIN=harness_amp_channel QUINT_TRACE_MAX_STEPS=24 scripts/verify/quint-trace.sh generate verification/quint/harness/amp_channel.qnt verification/quint/traces/amp_channel.itf.json
```

This guide covers how to write tests for Aura protocols using the testing infrastructure. It includes unit testing, integration testing, property-based testing, conformance testing, and runtime harness validation.

For infrastructure details, see [Test Infrastructure Reference](118_testkit.md). For the deterministic shared-flow design rules, see [User Flow Harness](121_user_flow_harness.md).

## 1. Core Philosophy

Aura tests follow four principles. Tests use effect traits and never call direct impure functions. Tests run actual protocol logic through real handlers. Tests produce reproducible results through deterministic configuration. Tests validate both happy paths and error conditions.

Parity-critical ownership work also requires compile-fail coverage for ownership and capability boundaries enforced in types. This includes forbidden capability construction paths such as `CapabilityId::from("...")` or invalid `capability_name!(...)` literals. It also includes invariant tests for owner drop, stale-handle rejection, and terminality. Timeout and backoff tests should prove typed timeout failure, remaining-budget propagation, and bounded retries.

Run the relevant ownership `just ci-*` policy checks alongside crate tests. Run `just lint-arch-syntax` when changing capability parsing boundaries, typed capability-family usage, or choreography capability admission rules.

### Harness Policy

The runtime harness is the primary end-to-end validation lane. Default harness runs exercise the real Aura runtime with real TUI and web frontends. The goal is to catch integration failures in the actual product, not just prove a model.

The harness has two distinct responsibilities. The shared semantic lane executes parity-critical shared flows through the shared semantic command plane. It waits on typed handles, readiness facts, runtime events, quiescence, and authoritative projections. This is the primary lane for debugging production code paths.

The frontend-conformance lane validates renderer-specific control wiring, DOM structure, PTY key mappings, and shell-level integration. It may use renderer-specific mechanics intentionally. It must not be the primary execution substrate for shared scenarios.

Quint and other verification tools generate models, traces, and invariants. They are not a replacement for real frontends.

`aura-app` owns the shared semantic scenario, command-plane, and UI contracts. `aura-harness` consumes those contracts and submits shared semantic commands to real frontends. `aura-simulator` is a separate alternate runtime substrate.

User-facing docs and harness guidance must not point readers at scratch-note or
ephemeral local-output paths. Describe outputs in terms of the stable harness
artifact bundle, scenario reports, and configured run outputs rather than
repo-local scratch directories.

Use this lane matrix when selecting harness mode.

| Lane | Backend | Command |
|------|---------|---------|
| Local deterministic | `mock` | `just harness-run -- --config configs/harness/local-loopback.toml --scenario scenarios/harness/real-runtime-mixed-startup-smoke.toml` |
| Patchbay relay realism | `patchbay` | `just harness-run -- --config configs/harness/local-loopback.toml --scenario scenarios/harness/real-runtime-mixed-startup-smoke.toml --network-backend patchbay` |
| Patchbay-vm relay realism | `patchbay-vm` | `just harness-run -- --config configs/harness/local-loopback.toml --scenario scenarios/harness/real-runtime-mixed-startup-smoke.toml --network-backend patchbay-vm` |
| Browser | Playwright | `just harness-run-browser scenarios/harness/semantic-observation-browser-smoke.toml` |

All shared flows should use typed scenario primitives, typed semantic command submission, and structured snapshot and readiness waits.

Native TUI harness IPC is part of that shared compatibility surface now. In
explicit harness mode the command socket and semantic snapshot mirrors are
scoped under `AURA_HARNESS_INSTANCE_TRANSIENT_ROOT`, and command submission
must authenticate with the per-run `AURA_HARNESS_RUN_TOKEN`. Setting
`AURA_TUI_COMMAND_SOCKET`, `AURA_TUI_UI_STATE_SOCKET`, or
`AURA_TUI_UI_STATE_FILE` outside explicit harness mode must stay inert or fail
closed; those env vars are not a production backdoor.

Shared-semantic preflight is intentionally stricter than generic backend startup. A run config that includes SSH instances does not automatically qualify for the shared semantic lane. Until a backend implements the shared semantic contract, SSH remains diagnostic-only for harness purposes. Shared-semantic scenarios must fail closed before execution.

For SSH-backed diagnostic runs, remote artifact capture now has two explicit modes. When `ssh_dry_run = true`, the harness records a simulated sync summary only. When `ssh_dry_run = false`, the harness copies `logs/` from the instance's `remote_workdir` back into the local artifact bundle under `remote/<instance-id>/logs/` using `scp`, then records the copied file manifest and checksums in the per-instance sync summary plus `remote_artifact_sync.json`. Use `require_remote_artifact_sync = true` when the run should fail closed if that SSH artifact copy does not complete.

`aura-app::ui_contract` is the canonical module for shared flow support. It defines `SharedFlowId`, `SHARED_FLOW_SUPPORT`, `SHARED_FLOW_SCENARIO_COVERAGE`, `UiSnapshot`, `compare_ui_snapshots_for_parity`, `OperationInstanceId`, and `RuntimeEventSnapshot`. The root file is a facade; parity metadata, harness/browser bridge metadata, and shared-flow support tables may live in dedicated `ui_contract/*` modules, but the canonical public contract stays `aura-app::ui_contract`. Use semantic readiness and state assertions before using fallback text matching.

Direct usage of `SystemTime::now()`, `thread_rng()`, `File::open()`, or `Uuid::new_v4()` is forbidden. These operations must flow through effect traits.

### Shared UX Contract and Determinism

The shared UX contract is defined in [User Interface](117_user_interface.md). The `aura-app::ui_contract` module is the canonical authority for parity-critical UI identity, readiness semantics, and typed observation payloads. The shared semantic scenario contract remains `aura-app::scenario_contract`. Its root may delegate contract families such as submission, actions, expectations, and values into `scenario_contract/*` modules without changing the public harness contract.

Shared scenarios must submit typed semantic commands through the frontend bridge. They must not use raw PTY keys, raw selector clicks, raw label matching, or incidental focus stepping as primary mechanics. Frontend-specific UI I/O belongs in frontend-conformance coverage rather than the main shared semantic lane. Unsupported semantic commands must fail closed and diagnostically.

Command submission must enter the frontend through its real update and event path. It must not use render-coupled polling or ad hoc harness shims.

Contacts friend management is part of that shared UX contract. The canonical relationship states are `contact`, `pending_outbound`, `pending_inbound`, and `friend`, and they must be projected from runtime-owned relational facts rather than shell-local heuristics. The shared contract owns the parity-critical contacts controls for `send friend request`, `accept friend request`, `decline friend request`, and `remove friend`, and both TUI and web tests should assert those actions through the same semantic surface. Harness and frontend-conformance coverage may verify renderer wiring around those controls, but the lifecycle itself stays anchored to Scenario 13 and the shared semantic command/observation path.

### Shared Semantic Ownership Model

Parity-critical shared semantic flows must use one explicit ownership category. Do not mix categories casually inside the same flow. The four ownership categories (`Pure`, `MoveOwned`, `ActorOwned`, `Observed`) are defined in [Ownership Model](122_ownership_model.md).

`aura-app` owns authoritative semantic operation coordination and typed lifecycle and error publication. `aura-agent` owns long-lived runtime and service actors and other actor-owned async state. `aura-terminal` and `aura-web` submit commands and observe lifecycle but do not own terminal semantic truth. `aura-harness` consumes typed handles, readiness, and projections but does not mutate semantic lifecycle directly.

Terminal convenience modals stay in that observed-only category. Opening or editing a local TUI modal such as the contact invitation sheet may prefill or reshape local display state, but it must not become an alternate semantic ingress path or carry authoritative receiver ownership. Tests should prove the real semantic boundary remains the typed dispatch command and upstream `aura-app` workflow submission path rather than modal-local state.

Clipboard copy affordances in TUI modals follow the same rule. Copy buttons and local clipboard helpers are convenience-only observed behavior, not shared semantic evidence. Headless CI and rustdoc builds must not rely on the host system clipboard being available. When a test or harness run needs to assert copied content, use `AURA_CLIPBOARD_MODE=file_only` together with `AURA_CLIPBOARD_FILE` and treat that capture file as diagnostic output rather than as proof of semantic success.

If a migrated parity-critical flow needs both actor and move semantics, the split must stay explicit. The actor owns mutable lifecycle state. Move-owned handles and tokens define which caller may advance or transfer it. If that split is not explicit, the flow is not considered correct by construction.

### Parity-Critical Observation

`UiSnapshot` and render-convergence data are authoritative. Observation surfaces must be side-effect free. Recovery and retries must be explicit and separate from observation.

A failed `OperationSnapshot` carries the owner-reported `failure_code` (a `SemanticFailureCode`, serialized in snake case and omitted when absent). Assert refusals on that code, for example `permission_denied` for a moderation command from an actor without the role or capability, not on toast text.

Home role commands are tracked operations: `/op` and `/deop` publish `grant_moderator` and `revoke_moderator`, and `/admit` publishes `admit_member` (`SemanticOperationKind::AdmitMember`). A participant must be admitted before `/op` succeeds; `/op` on a participant fails with an invalid-target refusal. The multi-runtime regressions live in the `home_flows` binary (`home_membership`).

`UiSnapshot.supervised_task_failures` lists dead runtime-supervised tasks (`group`, `task`, `cause`) from `RuntimeBridge::supervised_task_failures`. It is diagnostic only: excluded from parity comparison, readiness and quiescence, and never a wait condition. The TUI export populates it; the browser publishes it empty. On LAN runs, `deadtasks <inst>` in `scripts/harness/lan/lib.sh` prints it.

Browser `ui_state` remains observation-only and must not perform implicit navigation or state recovery. Explicit recovery goes through `recover_ui_state` and `readStructuredUiStateWithNavigationRecovery(...)`. DOM and text fallback paths are diagnostics only and must not become success-path observation behavior.

Browser semantic observation must fail closed when the published snapshot is unavailable. It must not silently repair by reading a live controller or model snapshot behind the harness bridge. Channel-binding responses must either carry authoritative context materialization or fail explicitly. Selected ids or labels alone are not semantic bindings.

For enrollment, assert code issuance and ceremony completion as separate
operation instances linked by ceremony ID. Browser `stage_runtime_identity`
and the page-owned semantic queue may submit or stage work, but neither owns
completion publication. A test that remounts the UI or reboots the browser
must reattach to the app/runtime-owned completion result and compare its typed
state and failure domain/code in `ui_state` on both frontends. Exercise refusal,
timeout, cancellation, duplicate terminal delivery, and restart with an
authoritative ceremony result; local wizard completion and device counts are
diagnostics only.

The bounded `enrollment_host_injection_resumes_separate_role_owners` test runs
the actual manifest on separate VM owners and reports scheduler/coroutine state
on stalled progress. `device_enrollment_owned_sessions_exchange_request_accept_confirm`
exercises device-addressed shared transport and repeats admission after terminal
VM reaping to catch leaked runtime owners. The actual invitee handler test
`device_enrollment_invitee_rejects_wrong_request_and_negative_confirmation`
requires typed permanent failure before the human retry window. These tests
cover transport, teardown and message binding; authenticated confirmation and
cross-machine terminal parity still require their own end-to-end evidence.

The `enrollment_setup` domain tests reject substituted identities, nonce,
validity, epoch, package and signatures, along with proof-policy mismatches and
oversized codes. Compile-fail doctests prevent constructing or deserializing
possession evidence directly. Signing-owner tests check runtime device capture,
exact retained code, serialized concurrent bootstrap, corrupt/partial bootstrap
without key replacement, and missing wrapping keys without regeneration.
Service recreation with shared effects tests verifier continuity at that scope;
it does not prove native/browser process restart, explicit user transfer,
request consumption, or authenticated ceremony completion.

Signing lifecycle regressions additionally cover recovery after service-only
rotation, immediate removal of stale local membership, pending-package overwrite
rejection, stale commit and active rollback rejection. Retained-share tests must
check a share from another group before activation and after recovery; requiring
a whole quorum is not necessary to prove one local share matches its group.
These checks do not replace genesis failure/restart or effects/service activation
boundary coverage.

Genesis readiness tests inject a corrupt tree index before materialization and
require retry to use the original key without exposing a signing context early.
They reject a wrong completion digest and a cached creation without its durable
index, and allow legacy migration only with an authenticated existing creation.
A missing active epoch after completion requires recovery, rather than defaulting
to epoch zero. These service/checkpoint tests do not establish process-restart
coverage or every native/browser storage failure boundary.

Channel list item ids and selected-channel snapshot ids must stay keyed by canonical channel ids when the runtime projection already provides them. Harness and browser code should not round-trip through display labels on those paths. Diagnostic tool and query surfaces should say `diagnostic_*` at the API boundary when they are derived from screen or DOM capture rather than authoritative semantic state. Onboarding must publish through the same semantic snapshot path as the rest of the UI.

Placeholder IDs, override-backed exports, and heuristic success or event synthesis are not acceptable correctness paths.

### Parity-Critical Waits and Assertions

Waits must bind to declared readiness, event, or quiescence conditions. They may also bind to typed operation handles or strictly newer authoritative projections when the shared contract defines them.

When a runtime bridge surface exposes typed lifecycle such as `DiscoveryTriggerOutcome`, `CeremonyProcessingOutcome`, or an explicit mutation outcome, tests should assert those variants directly. Do not treat a unit success result as sufficient proof of progress. Executor-side follow-on waits should carry typed submission evidence from the issued receipt into the declared contract barriers. Do not keep a second harness-local convergence graph.

Projection-based semantic waits may resume across bounded browser or runtime restarts only by clearing stale freshness baselines and re-entering typed snapshot observation. Runtime-event, toast, and exact operation-state waits still fail closed across restarts. Semantic issue success must come from typed command receipts and authoritative runtime facts, not from visible homes, modal closure, message appearance, selected-list state, or a frontend-local submitting phase.

Shared semantic harness core should decode typed `ToolPayload` and bridge structs directly. Keep raw `serde_json::Value` plumbing at outer CLI and browser adapters only. Raw sleeps, redraw polling, DOM scraping, and fallback text matching are diagnostics only.

Scenario-language text assertions must keep the same split. `message_contains` means authoritative `UiSnapshot.messages`. `diagnostic_screen_contains` is frontend-conformance-only rendered text. Harness mode may change instrumentation and render stability, but it must not change business-flow semantics.

### Ownership Test Expectations

When a change introduces or modifies a parity-critical ownership boundary, the test plan should include compile-fail tests for private constructors, capability misuse, or stale move-owned handles where the boundary is type-enforced. It should include invariant tests proving owner drop reaches explicit failure or cancellation. It should include invariant tests proving terminal lifecycle does not regress on the same logical instance.

It should also include invariant tests proving observed layers do not author semantic lifecycle. Include tests proving frontend-local submission yields immediately to the app-owned workflow owner after handoff. Include timeout and backoff tests proving local wall-clock policy only changes budget and diagnostics, not semantic success or failure rules. Run the relevant ownership and time `just ci-*` policy checks in addition to crate tests.

For shared semantic workflow changes, `aura-app::workflows` is the authoritative publication owner. `aura-terminal`, `aura-web`, and `aura-harness` must not retain a parallel terminal publication path after handoff. Review and test plans should name the terminal owner explicitly and treat frontend layers as submit and observe boundaries.

Use physical time for local deadline and backoff policy. Do not use wall-clock timeouts as the primary proof of distributed completion or ordering.

### Failure Analysis

Prefer canonical action, event, and state traces along with structured timeout diagnostics. Treat final text or screenshot inspection as supporting evidence, not the primary oracle. Replay bundles should compare typed tool-response payload meaning, not just a binary success-versus-error shape.

### Ownership Cleanup Discipline

Every shared UX or harness contract hardening change should remove obsolete compatibility code, stale allowlist entries, and transitional comments in the same milestone or the next explicit cleanup pass. Prefer extending typed governance in `cargo run -p aura-harness --bin aura-harness --quiet -- governance ...` over adding standalone shell policy logic.

Each parity-critical ownership change must include explicit cleanup work for the abstraction it replaces. Do not treat the ownership model as additive.

For every migrated flow, delete actor wrappers around purely local or value transitions that should stay `Pure`. Delete shared mutable ownership state where a `MoveOwned` handoff or owner-token surface is the correct model. Delete detached callback or task ownership for state that should instead live under one `ActorOwned` coordinator.

If a change leaves one of those old abstractions in place, record it as explicit ownership cleanup debt with the owning module and removal milestone. Do not hide it behind temporary ambient lifecycle helpers, duplicate readiness emitters, or shell-local terminal state.

The authoritative written update map for these surfaces now lives in Aura's `toolkit/xtask` user-flow guidance sync check and is enforced by `just ci-user-flow-policy`. Ownership-model policy for the shared semantic lane is enforced through the final CI entrypoints `just ci-ownership-policy`, `just ci-harness-ownership-policy`, and `just ci-user-flow-policy`.

### Testing and Enforcement Split

Prefer `trybuild` compile-fail coverage when the misuse is fundamentally an API-shape or visibility violation. Prefer Rust-native lint binaries in `aura-macros` when the misuse is a syntax-level boundary or naming and flow-shape rule. Keep shell scripts for repo-wide governance, integration topology, or end-to-end harness policy that cannot realistically be proved at compile time. When a stronger contract lands, remove the superseded legacy helper, compatibility branch, migration shim, or stale regression fixture rather than leaving both paths active.

The authoritative frontend matrix for converted shared scenarios comes from `scenarios/harness_inventory.toml` and is enforced by `just ci-harness-matrix-inventory`. Allowlisted harness-mode hooks must carry explicit owner, justification, and design-note references enforced by Aura's `toolkit/xtask` user-flow policy guardrails. The diff-aware user-flow policy lane must tolerate empty local diff sets so `just ci-user-flow-policy` fails on real policy drift rather than environment-specific diff resolution.

Changes to the browser harness bridge request, response, or observation surface must update both `crates/aura-web/ARCHITECTURE.md` and this guide so compatibility expectations stay explicit.

For service-family work, keep the test/evidence split concrete:

- type/API and proc-macro boundaries first
- `trybuild` compile-fail tests for misuse that should not compile
- `aura-macros` lint binaries for syntax-owned rules
- thin shell scripts only for integration-wide or artifact-governance checks

The default contributor verification path for this class of change is:

1. `just lint-arch-syntax`
2. `just ci-ownership-policy`
3. `just check-arch`
4. `just ci-adaptive-privacy-tuning` if the change touches adaptive privacy
   policy constants, simulator evidence, or telltale-backed control-plane parity

When a new `Establish`, `Move`, or `Hold` surface lands, the same change should
also add:

- the service-surface declaration macros
- compile-fail or invariant coverage for the strongest rejectable misuse
- any required `aura-macros` lint coverage or thin script glue
- the updated crate `ARCHITECTURE.md` and authoritative docs
- removal of the superseded compatibility helper or explicit inventorying of
  the deferred cleanup

### Browser Compatibility Surface

The browser compatibility surface includes the explicit `stage_runtime_identity` bootstrap handoff entrypoint plus the page-owned semantic submission queue (`window.__AURA_DRIVER_SEMANTIC_ENQUEUE__`). The browser also publishes page-owned semantic submit readiness metadata. This includes whether the enqueue surface is installed (`enqueue_ready`), the active vs ready generation boundary, controller presence, current shell phase, and any in-flight bootstrap transition detail. Driver startup and recovery waits bind to product-owned bootstrap and rebinding state instead of stale driver-local probes.

The bootstrap staging and handoff promise is completion-based. Callers may treat it as confirmation that the owned bootstrap or rebootstrap transition finished, not merely that the request was queued. For generation-changing bootstrap flows, that completion means the new page-owned shell generation has published its semantic snapshot through the canonical publication path. The browser diagnostics `window.__AURA_UI_ACTIVE_GENERATION__` and `window.__AURA_UI_READY_GENERATION__` reflect the active vs ready generation boundary. Render heartbeat remains the separate browser render-convergence signal.

Browser bootstrap storage is explicit. Preserved-profile recovery correctness depends on the typed selected runtime identity, pending bootstrap metadata, and browser-local `AccountConfig` metadata remaining distinct. The next generation must recover the canonical runtime bootstrap path without falling back to browser-local semantic repair. Channel-returning bridge responses now distinguish weak selected-channel ids from authoritative channel bindings. A payload that lacks context is not a binding.

Browser bootstrap broker credentials are intentionally not query-string
parameters. Tests may stage the broker URL through controlled bootstrap setup,
but bearer and invitation-retrieval tokens must use session-scoped browser
storage or header-bearing runtime configuration. Harnesses should assert this
through the browser storage/bridge contract rather than by inspecting URL
parameters.

Browser harness failures surface explicit publication-state diagnostics through `window.__AURA_UI_PUBLICATION_STATE__`, `window.__AURA_RENDER_HEARTBEAT_PUBLICATION_STATE__`, and the page-owned semantic submit publication surface. Those globals are diagnostic-only and do not replace the authoritative `UiSnapshot` and `RenderHeartbeat` payloads. They are the canonical observed source for browser bootstrap and rebinding state. Driver-owned `restart_page_session` is infrastructure recovery only. Semantic command submission and runtime-identity staging must wait on or fail from the page-owned publication contract rather than replaying work through a restarted browser session.

`submitSemanticCommand` follows that rule directly. After bounded same-page recovery it must fail closed instead of replaying the semantic request through a fresh browser session. The browser publication owner classifies diagnostics by typed publication status, binding mode, and reliability before serializing them to page globals. Compatibility-sensitive waits keep one canonical publication path instead of ad hoc string assembly.

The driver raises a semantic-revision floor before each browser action so a later `ui_state` cannot return the pre-action snapshot. A failed action (absent, hidden or disabled control, exhausted click retries) restores the floor it raised, because the page never publishes the newer revision for an action that did not happen. Observation must keep working after a refused action; the Playwright driver smoke test covers a failed click followed by `ui_state`.

A successful action is not proof of a no-op. After a successful action (other than `submit_semantic_command`, which keeps its own typed publication contract) the driver observes the page for a bounded settle window. A snapshot at or above the floor satisfies it. In-flight semantic work reported by the page (`operation_submitting` or `readiness_loading` quiescence reasons, or a submitting operation) keeps the floor raised, as does missing quiescence or DOM evidence. Only a settled, unchanged semantic snapshot restores the pre-action floor: a confirmed no-op (DOM unchanged) or DOM-only navigation (DOM changed, then stable). The action result carries `post_action_observation` with the outcome, and the smoke test covers each case.

Browser semantic navigation follows the same separation. Page-owned navigation helpers such as `navigate_screen` and settings-section opening may publish the target `UiSnapshot` before the browser finishes painting the new screen. Harness navigation success must therefore wait for both the target semantic screen and the matching post-render `RenderHeartbeat` or equivalent render-convergence proof before treating the control activation as complete. DOM selectors remain diagnostic corroboration only; they must not replace the semantic-plus-render contract.

Browser-owned semantic snapshot publication should flow through one helper aligned with `UiController::publish_ui_snapshot`. Browser-owned maintenance polling should share one bounded helper for sleep, cancellation, and pause reporting so those paths stay uniform and clearly non-semantic. Parity exceptions must remain typed metadata in `aura-app::ui_contract` with a reason code, scope, affected surface, and authoritative doc reference.

Browser-owned async account/bootstrap flows must also fail closed on shell-state publication. If a Dioxus signal write collides with an unmounting or busy component, the browser shell may retry on the next browser tick for the active generation, but it must not silently drop the state transition.

Browser harness mode now has an authenticated runtime bootstrap rule as well.
When the wasm agent runtime is launched under the browser harness, the
authenticated query parameters `__aura_harness_instance` and
`__aura_harness_token` are part of the canonical harness-mode contract. The
browser shell and runtime must agree on that authenticated handoff before
taking any harness-only invitation or device-enrollment relaxation path, and
browser build or cache reuse must preserve the `web,harness` feature set that
installs that bridge.

### Shared-Flow Coverage Anchors

The canonical shared-flow coverage anchors for the current parity-critical user flows are listed below.

- `real-runtime-mixed-startup-smoke.toml` for startup, onboarding, and shared neighborhood navigation
- `scenario13-mixed-contact-channel-message-e2e.toml` for the shared chat, contacts, invitation, home creation, channel join, and message-send flow
- `scenario12-mixed-device-enrollment-removal-e2e.toml` for device add and remove
- `shared-notifications-and-authority.toml` and `shared-settings-parity.toml` for the remaining shared settings, authority, and navigation flows
- `amp-transition-normal-shared.toml`,
  `amp-transition-delayed-witness-shared.toml`,
  `amp-transition-conflict-subtractive-shared.toml`,
  `amp-transition-emergency-shared.toml`, and
  `amp-transition-negative-shared.toml` for shared AMP transition
  observation coverage

The current `aura-app` split keeps those anchors unchanged while moving the
authoritative flow owners into more specific modules. Shared-flow source-area
metadata should point at the owner modules that now carry those flows:
`workflows/context/neighborhood.rs` for neighborhood/home creation,
`workflows/invitation/{create,accept,readiness}.rs` for contacts and
invitation acceptance, and `workflows/messaging/{channel_refs,channels,send}.rs`
for chat navigation, join, and message-send paths. The `aura-app::ui_contract`
facade remains the canonical export surface for that coverage metadata.

Scenario 12 has an additional browser parity rule now. The shared semantic
snapshot does not fabricate a selected row for `ListId::Devices`; current-device
markers and removable-device targeting remain separate concepts. Browser harness
submission for `remove_selected_device` must therefore fall back to the
authoritative removable device from settings state when the snapshot has no
explicit list selection, and the canonical mixed-runtime anchor remains
`scenario12-mixed-device-enrollment-removal-e2e.toml`.

For enrollment, create the invitee's real provisional account before
`prepare_device_enrollment_setup`. That variable action exports its signed setup
code through the runtime semantic queue and stores the exact code for the
initiator's `setup_code` input. Successful export establishes signing readiness;
an unavailable or unready exporter fails explicitly. Authority staging files,
derived browser device identifiers, and synthetic setup codes cannot replace
this transfer. Keep setup codes out of diagnostic event payloads.
Acceptance fixtures must retain the explicitly transferred verifier before
checking the response. Sign the canonical response with the real prepared
runtime, verify it against that retained key and policy, and record the sealed
evidence. A participant-count fixture or a signature checked only against its
own embedded key is negative coverage. Distinguish crypto/activation tests from
VM delivery, authority adoption and process-restart tests in reported evidence.
The semantic `ExportDeviceEnrollmentSetup` command returns the exact code in
an immediate `DeviceEnrollmentSetup { setup_code }` response with no operation
handle. Both shells call the bounded app export workflow; the native socket
ingress and browser page queue are transport owners, while the runtime owns
the signed retained request. An unavailable runtime or signing context fails
explicitly. Contract roundtrip tests check the value and absent ceremony handle;
they do not establish native/browser end-to-end enrollment completion.

LAN bootstrap candidates are observable as `ListId::BootstrapCandidates` in the
TUI `ui_state`: items are the candidate authority ids in Contacts display
order, and the selected item follows the LAN peer selection only while the
Contacts list focus is on LAN peers. Assert discovery, selection and invites
through this list rather than the rendered peer panel, which the TUI
diagnostic capture does not include.

Scenario 13 has an additional mixed-runtime receive contract now. On the
current TUI/browser path, authoritative inbound shared-channel messages may
surface as sealed placeholders rather than plaintext payloads. Harness
assertions for `scenario13-mixed-contact-channel-message-e2e.toml` should treat
the `[sealed:` prefix as the canonical browser/TUI parity expectation for those
receives instead of requiring renderer-local plaintext recovery.

Harness-mode timing exceptions remain narrowly allowlisted. The current shared allowlist includes the browser maintenance cadence plus the runtime and workflow instrumentation hooks that feed observed-shell timing helpers; those branches may tune observation cadence only and must not change business-flow semantics.

Shared pending-invitation acceptance has an additional invariant now:
`SemanticOperationKind::AcceptPendingChannelInvitation` entry points must not
strand the authoritative semantic lifecycle at
`SemanticOperationPhase::WorkflowDispatched` when the shared browser/TUI flow
fails before the owned accept path settles. If an early error escapes the
owned path, the wrapper or `*_with_instance` entry point must synthesize the
terminal failure publication for the same operation instance before returning
to the shell or harness. Scenario 13 remains the canonical mixed-runtime anchor
for that browser shared-channel receive parity.

Note-to-self is a real AMP channel provisioned at account bootstrap, not a display-only entry. It appears as a first-class channel backed by the runtime from first use, with its own context, deterministic channel ID, and standard message delivery. Channel creation parity coverage must not treat "has at least one contact" as a prerequisite for opening chat creation. TUI and web shells should expose the same semantic create-channel path when the only available participant is self, and scenario coverage should keep that path distinct from pairwise or group-member invitation flows.

The notifications shared-flow anchor remains navigation-only. Parity coverage for notifications navigation requires the TUI and web shells to expose the same semantic screen transition and detail-view contract, but notification empty-state copy is informational only and must not introduce parity-critical invitation or recovery actions outside the canonical shared workflows.

AMP channel transition frontend coverage uses the same semantic observation lane. Transition state, live successor and finalization state, conflict evidence, emergency quarantine, cryptoshred status, and suspect exclusion must be asserted through `RuntimeFact::AmpChannelTransitionUpdated` entries in `UiSnapshot.runtime_events` plus shared notification list ids.

Web and TUI frontends may render local affordances for emergency alarm, quarantine approval, cryptoshred approval, conflict evidence, and finalization status, but the controls and operation ids must come from `aura-app::ui_contract`. Tests must not infer AMP send or receive authority from local message-ratchet state or frontend-specific text. Destructive cryptoshred affordances must surface an explicit confirmation label and the loss of pre-emergency readability.

Native invitation and device-enrollment exports have an additional transport contract now. In non-wasm runs, `sender_hint` is a transport hint and must use the canonical `tcp://host:port` form rather than websocket-style `ws://` or `wss://` URLs. LAN integration and harness assertions should treat that field as a native direct-transport hint, not as a browser transport endpoint. When the runtime has both a stored rendezvous descriptor and a LAN-discovered descriptor for the same peer, invitation seeding should prefer the discovered descriptor if it adds a `TcpDirect` transport hint that the stored descriptor lacks so native shared-flow tests continue to exercise the direct LAN path.

### Shared Semantic Ownership Inventory

Use this as the authoritative ownership map for the shared semantic stack. If code does not match this table, treat it as ownership cleanup debt rather than as an acceptable alternate pattern.

| Subsystem | Crate / locus | Ownership | Authoritative owner | May mutate | May observe |
|-----------|---------------|-----------|---------------------|------------|-------------|
| Semantic command / handle contract | `aura-app::ui_contract`, `aura-app::scenario_contract` | `Pure` + `MoveOwned` | `aura-app` contract surfaces | `aura-app` contract and workflow modules | `aura-terminal`, `aura-web`, `aura-harness` |
| Semantic operation lifecycle | `aura-app::workflows::*` | `MoveOwned` | authoritative workflow coordinator | workflow and coordinator modules in `aura-app` | frontend render crates, harness |
| Channel / invitation / delivery readiness | `aura-app::workflows::*` | `ActorOwned` | single-owner readiness coordinator | coordinator modules and sanctioned hooks | shell, subscription, render, harness |
| Runtime-facing async service state | `aura-agent::runtime::*`, `aura-agent::handlers::*` | `ActorOwned` | runtime service actor | actor and sanctioned commands | `aura-app`, frontends, harness |
| TUI command ingress | `aura-terminal::tui::harness_state`, update loop | `ActorOwned` ingress + `Observed` rendering | TUI update and event loop | ingress and update-loop code only | shell render, harness |
| TUI shell / callbacks / subscriptions | `aura-terminal::tui::screens`, `callbacks` | `Observed` | downstream of authoritative state | local UI state only | harness, rendering |
| Browser harness bridge | `aura-web::harness_bridge` | `ActorOwned` bridge + `Observed` publication | browser bridge module | bridge module only | Playwright, harness, render |
| Harness executor / wait model | `aura-harness::executor`, `backend::*` | `Observed` + orchestration `ActorOwned` | harness coordinator | harness orchestration state only | scenario authors, CI |
| Ownership transfer / stale-owner invalidation | operation handles, owner tokens | `MoveOwned` | current token holder | sanctioned transfer APIs only | projections, render, diagnostics |

The required split is that actor-owned subsystems own long-lived mutable async state and lifecycle. Move-owned surfaces own exclusive right-to-act and ownership transfer. Observed surfaces render, wait, and diagnose without authoring semantic truth.

App bootstrap tests must fail each required signal-registration and refresh-hook attachment step, then retry the same runtime generation. A successful hook installation means every signal receiver is attached and its listener has acknowledged startup; tests assert one live subscriber per required signal. Detach must cancel that generation's hook group before rebootstrap attaches another. Query-bound signal registration has the same retry contract, including a failed query binding after signals were registered. A task spawner that discards listeners must fail installation rather than report readiness.

Mounted UI subscription tests must separately fail attachment of every distinct required signal, close an attached stream, and restart the runtime generation. Assert typed health, bounded recovery, one receiver per required signal, and cancellation of the previous generation on unmount and repeated mount. Attach before reading the initial state, then force updates across that boundary and after a lag to prove the projection converges from a fresh snapshot. Test both TUI and browser observation of the recovered authoritative state; neither a stale projection nor an empty default is evidence of success. A listener task that captures a clone of its own lifetime owner must fail the duplicate-ownership regression test.

Do not use this table to justify ambient shared ownership. If a subsystem needs both actor and move semantics, the actor owns mutable lifecycle state while the move-owned handle or token defines who may advance or transfer it.

### Reactive Subscription Policy

Subscribing before registration must fail with `ReactiveError::SignalNotFound`. Tests must not treat an empty stream as equivalent to "signal not registered." Lagging subscribers are allowed to miss intermediate updates. Assertions should target eventual newer snapshots, not lossless delivery.

TUI-local semantic submission is limited to the sanctioned local-terminal and workflow-handoff owner wrappers. Browser bridge concurrency is limited to `WebTaskOwner` and does not own parity-critical lifecycle. Playwright stages browser runtime identity through the explicit bridge entrypoint before rebootstrap and submits semantic commands through the page-owned semantic queue.

Authoritative readiness refresh remains private to `aura-app::workflows` and is compile-fail tested in both default and `signals` configurations.

### Required Ownership Invariants

Ownership-model migrations are not complete until the following test classes exist for the affected parity-critical surface. Include compile-fail guards for private constructors, wrong-capability issuance, and stale-owner misuse where the boundary is enforced in types. Include dynamic invariant tests proving owner drop reaches explicit terminal failure or cancellation. Include dynamic invariant tests proving terminal states do not regress on the same logical operation instance.

Include handle and instance tests proving stale handles do not match or advance the wrong operation instance after transfer or replacement. Include concurrency tests for actor-owned coordinators where lost updates or multiple live owners are plausible. Include timeout and backoff invariant tests proving typed timeout failure, remaining-budget propagation, bounded attempts, and local-choice scaling.

If a flow changes ownership model or timeout policy and these test classes do not move with it, treat the migration as incomplete.

### Release and Update Matrix Expectations

OTA and module release and update validation must follow the same semantic-lane contract as other parity-critical shared flows. The OTA contract requirements are defined in [Distributed Maintenance Architecture](116_maintenance.md). These include typed command and control surfaces, scoped activation lifecycle, and rollback semantics. Each release row in [UX Flow Coverage Report](997_flow_coverage.md) must map to those typed lifecycle surfaces.

Frontend-conformance coverage may validate release-screen wiring, but it does not satisfy OTA or module lifecycle validation on its own.

## 2. The `#[aura_test]` Macro

The macro provides async test setup with tracing and timeout.

```rust
use aura_macros::aura_test;
use aura_testkit::*;

#[aura_test]
async fn test_basic_operation() -> aura_core::AuraResult<()> {
    let fixture = create_test_fixture().await?;
    let result = some_operation(&fixture).await?;
    assert!(result.is_valid());
    Ok(())
}
```

The macro wraps the test body with tracing initialization and a 30-second timeout. Create fixtures explicitly rather than relying on ambient test state.

## 3. Test Fixtures

### Platform credential store in tests

Test builds never open the real platform credential store. The `test-keyring`
feature of `aura-effects`, enabled only through dev-dependencies (and always
for its own unit tests), routes every `keyring::Entry` to a file-backed store
under `$TMPDIR/aura-test-keyring` (override with `AURA_TEST_KEYRING_DIR`).
Rebuilt test binaries otherwise trigger a macOS Keychain prompt on every run.
Release builds do not compile this store. Add the feature to a crate's
`aura-effects` dev-dependency when its tests assemble a production runtime.

Fixtures provide consistent test environments with deterministic configuration.

### Creating Fixtures

```rust
use aura_testkit::infrastructure::harness::TestFixture;

let fixture = TestFixture::new().await?;
let device_id = fixture.device_id();
let context = fixture.context();
```

The `TestFixture` provides a pre-configured environment with deterministic identifiers and effect handlers.

### Custom Configuration

```rust
use aura_testkit::infrastructure::harness::{TestFixture, TestConfig};

let config = TestConfig {
    name: "threshold_test".to_string(),
    deterministic_time: true,
    capture_effects: true,
    timeout: Some(Duration::from_secs(60)),
};
let fixture = TestFixture::with_config(config).await?;
```

Custom configuration controls deterministic time, effect capture, and per-test timeouts. Name each configuration to aid failure identification.

### Deterministic Identifiers

```rust
use aura_core::types::identifiers::AuthorityId;

let auth1 = AuthorityId::from_entropy([1u8; 32]);
let auth2 = AuthorityId::from_entropy([2u8; 32]);
```

Incrementing byte patterns create distinct but reproducible identifiers. This ensures tests produce the same identifiers across runs.

## 4. Unit Tests

Unit tests validate individual functions or components.

```rust
#[aura_test]
async fn test_single_function() -> aura_core::AuraResult<()> {
    let fixture = create_test_fixture().await?;
    let input = TestInput::new(42);
    let output = process_input(&fixture, input).await?;
    assert_eq!(output.value, 84);
    Ok(())
}
```

Each unit test should be fast and focused, testing one behavior per function. Name tests descriptively to communicate the expected behavior.

## 5. Integration Tests

Integration tests validate complete workflows across multiple components.

```rust
use aura_agent::runtime::AuraEffectSystem;
use aura_agent::AgentConfig;

#[aura_test]
async fn test_threshold_workflow() -> aura_core::AuraResult<()> {
    let fixture = create_test_fixture().await?;
    let device_ids: Vec<_> = (0..5)
        .map(|i| DeviceId::new_from_entropy([i as u8 + 1; 32]))
        .collect();

    let effect_systems: Result<Vec<_>, _> = (0..5)
        .map(|i| {
            AuraEffectSystem::simulation_for_named_test_with_salt(
                &AgentConfig::default(),
                "test_threshold_workflow",
                i as u64,
            )
        })
        .collect();

    let result = execute_protocol(&effect_systems?, &device_ids).await?;
    assert!(result.is_complete());
    Ok(())
}
```

Use `simulation_for_test*` helpers for all tests. For multi-instance tests from one callsite, use `simulation_for_named_test_with_salt(...)` and keep the identity and salt stable. This allows failures to be replayed deterministically.

## 6. Property-Based Testing

Property tests validate invariants across diverse inputs using proptest.

### Synchronous Properties

```rust
use proptest::prelude::*;

fn arbitrary_message() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 1..=1024)
}

proptest! {
    #[test]
    fn message_roundtrip(message in arbitrary_message()) {
        let encoded = encode(&message);
        let decoded = decode(&encoded).unwrap();
        assert_eq!(message, decoded);
    }
}
```

Synchronous property tests verify that invariants hold across randomly generated inputs. The `arbitrary_message` strategy produces byte vectors of varying length.

### Async Properties

```rust
proptest! {
    #[test]
    fn async_property(data in arbitrary_message()) {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let fixture = create_test_fixture().await.unwrap();
            let result = async_operation(&fixture, data).await;
            assert!(result.is_ok());
        });
    }
}
```

Async properties require creating a Tokio runtime inside the test body because proptest does not natively support async closures.

## 7. GuardSnapshot Pattern

The guard chain separates pure evaluation from async execution. This enables testing guard logic without an async runtime.

### Testing Pure Guard Logic

```rust
#[test]
fn test_cap_guard_denies_unauthorized() {
    let snapshot = GuardSnapshot {
        capabilities: vec![],
        flow_budget: FlowBudget { limit: 100, spent: 0, epoch: 0 },
        ..Default::default()
    };
    let result = CapGuard::evaluate(&snapshot, &SendRequest::default());
    assert!(result.is_err());
}
```

This test verifies that the capability guard rejects requests when no capabilities are present. The `GuardSnapshot` captures the state needed for pure evaluation.

### Testing Flow Budget

```rust
#[test]
fn test_flow_guard_blocks_over_budget() {
    let snapshot = GuardSnapshot {
        flow_budget: FlowBudget { limit: 100, spent: 95, epoch: 0 },
        ..Default::default()
    };
    let request = SendRequest { cost: 10, ..Default::default() };
    let result = FlowGuard::evaluate(&snapshot, &request);
    assert!(matches!(result.unwrap_err(), GuardError::BudgetExceeded));
}
```

This test verifies that the flow guard blocks sends when the requested cost would exceed the remaining budget.

## 8. TUI and CLI Testing

### TUI State Machine Tests

```rust
mod support;
use support::TestTui;
use aura_terminal::tui::screens::Screen;

#[test]
fn test_screen_navigation() {
    let mut tui = TestTui::new();
    tui.assert_screen(Screen::Block);
    tui.send_char('2');
    tui.assert_screen(Screen::Neighborhood);
}
```

TUI state machine tests validate screen transitions and keyboard input handling without requiring a real terminal.

### CLI and RPC Testing

CLI account commands and `aura rpc` share one typed command model
(`aura_terminal::command::{Request, Response, execute}`), so most coverage
drives requests directly:

```rust
use aura_terminal::command::{execute, Request, Response};

let response = execute(&peer.ctx, Request::ChatList).await?;
assert!(matches!(response, Response::Channels(_)));
```

- `tests/cli_rpc.rs` runs simulation runtimes on the shared virtual-time
  fixture (`tests/cli_rpc/simnet.rs`, paused tokio clock) and drives `aura
  rpc` sessions over in-memory pipes. Wait for outcomes with `subscribe`
  events or bounded `call_until` re-checks at quiescent points, never wall
  clock sleeps. It also checks that CLI and RPC give identical responses and
  that a CLI send matches the TUI send's semantic outcome.
- Failures keep their typed category end to end: assert the RPC error
  `code` (for example `not_found` for an unknown ceremony id) and, with the
  real binary in `tests/cli_production.rs`, the process exit code it maps to
  (`not_found` exits 3).
- A slash command whose target does not resolve settles its semantic
  operation failed with an error toast
  (`tests/unit_slash_commands.rs`), so lastop never keeps showing an earlier
  operation.
- `tests/cli_json.rs` and `tests/cli_socket.rs` run the real `aura` binary
  against a node serving the socket.
- `tests/cli_production.rs` creates an account through the TUI's staging
  path, then runs the binary against the production profile (no node) and
  through `aura serve`. It relies on the test keyring above; set
  `AURA_TEST_KEYRING_DIR` per test so the test and binary share one store.
- The published protocol schema is checked by `just ci-rpc-schema`.

### Quint Trace Usage

Quint traces are model artifacts. Export them through the shared semantic scenario contract and execute real TUI and web flows through `aura-harness` rather than replaying Quint traces directly against the TUI implementation.

## 9. Conformance Testing

Conformance tests validate that implementations produce identical results across environments.

### Conformance Lanes

CI runs two lanes. The strict lane compares native vs WASM cooperative execution. The differential lane compares native threaded vs cooperative execution.

```bash
# Strict lane
just ci-conformance-strict

# Differential lane
just ci-conformance-diff

# Both lanes
just ci-conformance
```

These commands run the conformance test suite and report any divergence between execution environments.

### Mismatch Taxonomy

| Type | Description | Fix |
|------|-------------|-----|
| `strict` | Byte-level difference | Remove hidden state or ordering-sensitive side effects |
| `envelope_bounded` | Outside declared envelopes | Add or correct envelope classification |
| `surface_missing` | Required surface not present | Emit observable, scheduler_step, and effect |

### Reproducing Failures

```bash
AURA_CONFORMANCE_SCENARIO=scenario_name \
AURA_CONFORMANCE_SEED=42 \
cargo test -p hxrts-aura-agent \
  --test telltale_machine test_name \
  -- --nocapture
```

Set the scenario name and seed to reproduce a specific conformance failure deterministically.

## 10. Runtime Harness

The runtime harness executes real Aura instances in PTYs for end-to-end validation.

### Harness Overview

The harness is the single executor for real frontend scenarios. Scripted mode uses the shared semantic scenario contract. Agent mode uses LLM-driven execution toward goals.

Multi-host runs use one harness per host with the same run token. Set `[run] fixed_ports = true` so configured `bind_address` and `lan_discovery.port` values stay literal instead of being namespaced by the run token; peers on different hosts must agree on these ports.

The tracked LAN helpers live in `scripts/harness/lan/`. `configs/harness/lan-host-{a,b}.toml` name each host's LAN address as `__HOST_ADDR__`; `drv.sh start` renders the config with `AURA_E2E_HOST_ADDR` (detected from the primary interface by default), so no address is committed. Build the same commit on both hosts with `build.sh <lane>`, then run `AURA_E2E_REMOTE=user@host scripts/harness/lan/fresh.sh <run-token>` from the first host: it starts both drivers on the token, onboards the standard cast and links contacts; `lib.sh` provides the request helpers used by scenario scripts.

Local TUI instances write plaintext runtime tracing to `runtime.log` under the instance's transient root (passed as `AURA_TUI_RUNTIME_LOG_FILE`, honored only in harness mode, filtered by `RUST_LOG`). `tail_log` reads that file first and falls back to the PTY capture. Treat it as diagnostic output, not semantic evidence.

Shared flows should be authored semantically once, then executed through the harness using either the TUI or browser driver. Do not create a second frontend execution path for MBT or simulator replay. Core shared scenarios should use semantic actions and state-based assertions. Avoid raw selector steps, raw `press_key` steps, and label-based browser clicks except in dedicated low-level driver tests.

### Run Config

```toml
schema_version = 1

[run]
name = "local-loopback-smoke"
pty_rows = 40
pty_cols = 120
seed = 4242

[[instances]]
id = "alice"
mode = "local"
data_dir = "artifacts/harness/state/local-loopback/alice"
device_id = "alice-dev-01"
bind_address = "127.0.0.1:41001"
```

The run config declares the execution environment. Each instance gets a unique data directory, device ID, and bind address. The seed ensures deterministic behavior across runs.

### Scenario File

```toml
id = "discovery-smoke"
goal = "Validate semantic harness observation against a real TUI"

[[steps]]
id = "launch"
action = "launch_actors"
timeout_ms = 5000

[[steps]]
id = "nav-chat"
actor = "alice"
action = "navigate"
screen_id = "chat"
timeout_ms = 2000

[[steps]]
id = "chat-ready"
actor = "alice"
action = "readiness_is"
readiness = "ready"
timeout_ms = 2000
```

Scenarios define ordered steps with timeouts. Each step targets a specific actor and asserts a condition or triggers an action.

### Running the Harness

```bash
# Lint before running
just harness-lint -- --config configs/harness/local-loopback.toml \
  --scenario scenarios/harness/semantic-observation-tui-smoke.toml

# Execute
just harness-run -- --config configs/harness/local-loopback.toml \
  --scenario scenarios/harness/semantic-observation-tui-smoke.toml

# Replay for deterministic reproduction
just harness-replay -- --bundle artifacts/harness/local-loopback-smoke/replay_bundle.json
```

Always lint before running to catch configuration errors early. Use replay bundles to reproduce failures deterministically.

### Interactive Mode

Use `tool_repl` for manual validation.

```bash
cargo run -p aura-harness --bin tool_repl -- \
  --config configs/harness/local-loopback.toml
```

The REPL accepts JSON requests for screen inspection, key input, and wait conditions.

```json
{"id":1,"method":"screen","params":{"instance_id":"alice"}}
{"id":2,"method":"send_keys","params":{"instance_id":"alice","keys":"3n"}}
{"id":3,"method":"wait_for","params":{"instance_id":"alice","pattern":"Create","timeout_ms":4000}}
```

These requests query screen state, send key sequences, and wait for patterns to appear in the rendered output.

### Harness CI

```bash
just ci-harness-build
just ci-harness-contract
just ci-harness-replay
just ci-harness-matrix
just ci-shared-flow-policy
```

These commands build the harness, validate the contract, replay recorded coverage, run the full shared frontend matrix, and enforce shared-flow policy.

`just ci-shared-flow-policy` validates the shared-flow contract end to end. It checks that `aura-app` shared-flow support declarations are internally consistent. It verifies that every fully shared flow has explicit parity-scenario coverage and that required shell and modal ids still exist. It confirms browser control and field mappings still line up with the shared contract and that core shared scenarios have not drifted back to raw mechanics. The shared-flow aggregate now calls Aura policy code for the adaptive-privacy runtime-locality and legacy-sweep gates through `toolkit/xtask`, while the remaining shared-flow checks stay as thin shell orchestration around harness governance and targeted contract tests.

`just ci-user-flow-policy` is the diff-aware guidance gate for this surface. When shared UX contributor policy or parity-sensitive TUI and browser semantics change, update this guide in the same change so the user-flow guidance sync stays green. Local `.claude` skills may remain gitignored, but the authoritative contributor-facing testing guidance must still land in tracked docs.

The shared-flow policy scripts target the published Cargo package names for renamed Layer 6 crates and macros. When invoking raw Cargo commands behind these lanes, use `hxrts-aura-app` and `hxrts-aura-macros` package ids instead of the legacy `aura-app` and `aura-macros` selectors. File-system crate paths remain `crates/aura-app` and `crates/aura-macros`.

When shared flows export data through runtime events, the event payload is part of the contract. Invitation and device-enrollment code capture should come from `RuntimeFact` payloads in `UiSnapshot.runtime_events`, not clipboard scraping or frontend-local heuristics. Shared chat waits should bind to semantic selection state so the harness targets the single shared channel instead of falling back to incidental render order.

AMP transition waits follow that runtime-event rule. Shared scenarios for normal transition, delayed or offline witnesses, conflicting `A2` certificates, subtractive membership, emergency quarantine, cryptoshred, rejected emergency attempts, cooldowns, duplicate-signing evidence, recovery replay, and authority-governance non-removal should wait on `RuntimeEventKind::AmpChannelTransitionUpdated`, parity snapshots, operation lifecycle, quiescence, and final reduced channel state.

These scenarios must stay actor-based and semantic-only. Raw DOM selectors, PTY keys, compatibility steps, and label-only assertions are diagnostic or frontend-conformance tools. They are not shared AMP evidence.

Use `just ci-ui-parity-contract` for the narrower parity gate. That lane validates shared screen and module mappings, shared-flow scenario coverage, and parity-manifest consistency without running a full scenario matrix.

## 11. Test Organization

Organize tests by category within each crate.

```rust
#[cfg(test)]
mod tests {
    mod unit {
        #[aura_test]
        async fn test_single_function() -> aura_core::AuraResult<()> { Ok(()) }
    }

    mod integration {
        #[aura_test]
        async fn test_full_workflow() -> aura_core::AuraResult<()> { Ok(()) }
    }

    mod properties {
        proptest! {
            #[test]
            fn invariant_holds(input in any::<u64>()) {
                assert!(input == input);
            }
        }
    }
}
```

Grouping tests by category makes it easy to run subsets and understand coverage at a glance.

### Running Tests

```bash
# All tests
just test

# Specific crate
just test-crate aura-agent

# With output
cargo test --workspace -- --nocapture

# TUI state machine tests
cargo test --package aura-terminal --test unit_state_machine
```

Use `just test` for the full suite. Use `just test-crate` for focused iteration on a single crate.

### Build and Caching

Route builds through `scripts/dev/build-budget.sh`. It writes to the
checkout's own `target/` (it unsets `CARGO_TARGET_DIR`), sweeps that target to a
per-checkout soft cap (`AURA_BUILD_TARGET_CAP_GIB`, default 10), and admits a
build only if the volume keeps `AURA_BUILD_MIN_FREE_GIB` (default 15) free
after the reservations of other admitted builds. Each admitted build reserves
`AURA_BUILD_RESERVE_GIB` (default 4) under `~/.cache/aura-build/reservations`
until it exits. The admission lock is held only while checking and reserving,
and only builders of the same checkout block its sweeps, so two worktrees can
build at once.

The dev shell exports `RUSTC_WRAPPER=sccache` with one shared store at
`SCCACHE_DIR` (default `~/.cache/aura-sccache`, capped by
`SCCACHE_CACHE_SIZE`, default 10G). Every worktree and checkout reuses its
compiled dependencies. sccache caches only non-incremental compilations, the
default, and passes `CARGO_INCREMENTAL=1` compilations through. Set
`AURA_NO_SCCACHE=1` at shell entry or on a single cargo command to opt out.
`just disk-report` shows the shared store's size. `sccache --show-stats`
shows hit rates.

LAN run binaries are built by `scripts/harness/lan/ship.sh` from a clean
commit with Cargo's `lan` profile: release optimization without whole-program
LTO (`lto = false`, 16 codegen units). `aura` (with the `terminal` feature) and
`tool_repl` build in the checkout's `target/` through the build budget, so
unchanged crates come from `target/` and sccache and a one-crate change
rebuilds only that crate and its dependents. `ship.sh` rsyncs the binaries,
sends the Nix store paths they reference at run time (dev-shell libraries,
listed by `scripts/harness/lan/runtime-refs.sh`) with `nix copy` (the remote
user must be a Nix trusted-user), and keeps the web bundle on `dx`. The dev
shell profile and those run-time paths are GC roots under `.nix-ship/` on both
hosts; `just nix-store-gc` reports the roots it keeps and the reclaimable
store paths, and `just nix-store-gc --apply` collects them.

The dev profile builds third-party dependencies at `opt-level = 1` (2 for
`curve25519-dalek` and `frost-ed25519`) without debuginfo, and workspace crates
with line tables only. Set `CARGO_PROFILE_DEV_DEBUG=true` for a debugging
session; it rebuilds the workspace crates.

For a local edit-test loop in `aura-agent`, opt in to incremental compilation
and run one integration binary or a lib filter:

```bash
CARGO_INCREMENTAL=1 bash scripts/dev/build-budget.sh --lane agent-loop -- \
  cargo test -p hxrts-aura-agent --lib <filter>
CARGO_INCREMENTAL=1 bash scripts/dev/build-budget.sh --lane agent-loop -- \
  cargo test -p hxrts-aura-agent --test runtime_integration <module>::
```

A one-file edit then recompiles in about 10-30 s instead of 1-2 minutes. The
incremental cache costs several GiB; `just prune-inactive-lane
debug-incremental` reclaims it. CI and gates stay non-incremental.

`aura-agent` sets `autotests = false` and aggregates its integration tests
into a few binaries: `runtime_integration`, `home_flows` (also the
reduced-stack accept-chain lane), `telltale_machine`, plus the separate
`lan_integration`, `compile_fail`, `custom_provider_fidelity` and
`web_runtime_bridge_wasm`. Add a new test file as a `mod` of the matching root
file in `crates/aura-agent/tests/`. Select one file with its module path, for
example `--test telltale_machine telltale_machine_parity::`.

`cargo nextest run -p <crate>` is available in the dev shell for local
per-test timing. Gates keep libtest, since their parsers read libtest output.

## 12. Best Practices

Test one behavior per function and name tests descriptively. Use fixtures for common setup. Prefer real handlers over mocks. Test error conditions explicitly. Avoid testing implementation details and focus on observable behavior.

Keep tests fast and parallelize independent tests.

## 13. Holepunch Backends and Artifact Triage

Use the harness `--network-backend` option to select execution mode.

```bash
# Deterministic local backend
cargo run -p aura-harness --bin aura-harness -- \
  run --config configs/harness/local-loopback.toml \
  --network-backend mock

# Native Linux Patchbay (requires Linux + userns/capabilities)
cargo run -p aura-harness --bin aura-harness -- \
  run --config configs/harness/local-loopback.toml \
  --network-backend patchbay

# Cross-platform VM runner (macOS/Linux)
cargo run -p aura-harness --bin aura-harness -- \
  run --config configs/harness/local-loopback.toml \
  --network-backend patchbay-vm
```

Patchbay is the authoritative NAT-realism backend for holepunch validation. Use native `patchbay` on Linux CI and Linux developers when capabilities are available. Use `patchbay-vm` on macOS and as Linux fallback to run the same scenarios in a Linux VM. Keep deterministic non-network logic in `mock` backend tests to preserve fast feedback.

The Linux harness pins Patchbay to upstream revision
`cecd3b22e23396874169fa12d4441a6bdcaa1de9`, which migrates its DNS implementation
to Hickory 0.26. Its two lab construction paths use the upstream builder API.
The Linux dependency manifest additionally requires Hickory Proto `=0.26.1`:
0.26.0 is affected by RUSTSEC-2026-0119, while 0.25 is also affected by
RUSTSEC-2026-0118. Keep the backend enabled and validate the Linux harness,
including its integration-test targets; a macOS build cannot establish this
target-specific compatibility. Regenerate the tracked `Cargo.nix` after changing
these dependencies and run `just ci-security-audit`. The ignored `Cargo.lock`
does not enforce the fixed dependency versions on a fresh checkout.

`patchbay-vm` relies on the explicit harness work and artifact directories and `QEMU_VM_WORK_DIR`. The removed `.qemu-vm` redirect path is no longer part of the supported workflow.

### Backend Resolution

The harness writes backend resolution details to `artifacts/harness/<run>/network_backend_preflight.json`. Implementation follows three tiers. Tier 1 covers deterministic and property tests in `aura-testkit` for retry and path-selection invariants. Tier 2 covers Patchbay integration scenarios in `aura-harness` for PR gating. Tier 3 covers Patchbay stress and flake detection suites on scheduled CI.

### Triaging Failures

When a scenario fails, triage artifacts in this order.

1. Check `network_backend_preflight.json` to confirm selected backend and fallback reason.
2. Check `startup_summary.json` and `scenario_report.json` for run context and failing step.
3. Check `events.json` and backend timeline artifacts for event ordering.
4. Check namespace and network dumps and pcap files for packet and routing diagnosis.
5. Check agent logs for authority-local failures and retry state transitions.

For harness-specific state debugging, treat `timeout_diagnostics.json` as the first failure bundle. It includes semantic state snapshots, render readiness, and runtime event history.

## 14. Browser Harness Workflow

Use this flow to run harness scenarios in browser mode with WASM and Playwright.

```bash
# 1) Check wasm/frontend compilation
just web-check

# 2) Install/update Playwright driver deps
cd crates/aura-harness/playwright-driver
npm ci
npm run install-browsers
npm test
cd ../..

# 3) Serve the web app
just web-serve
```

These steps verify WASM compilation, install Playwright dependencies, and start the web server.

In a second shell, run the browser scenarios.

```bash
# Lint browser run/scenario config
just harness-lint-browser scenarios/harness/semantic-observation-browser-smoke.toml

# Run browser scenarios
just harness-run-browser scenarios/harness/semantic-observation-browser-smoke.toml

# Replay the latest browser run bundle
just harness-replay-browser
```

Browser harness artifacts are written under `artifacts/harness/browser/`.

### LAN Run Artifact Retention

The tracked LAN driver sets `AURA_HARNESS_WEB_PREBUILT_ONLY=1`. Its static
server requires a current `web,harness` bundle; missing, stale or non-harness
assets fail startup without an implicit Dioxus build or release-cache deletion.
Stop the run and use `scripts/harness/lan/build.sh web` before retrying.
The LAN driver explicitly selects `AURA_SECURE_STORAGE_BACKEND=filesystem-fallback`
for its isolated native profiles. Runtime admission still checks harness
authorization for that provider; ordinary production defaults to platform
credentials. This avoids OS credential prompts without bypassing crypto,
authorization or lifecycle behavior. Required LAN safety fixtures enforce the
provider handoff. Secure retrieval distinguishes exact missing records from
native provider failures; workspace unit coverage enforces that distinction.
`just ci-build-cache-policy` verifies this boundary with isolated positive and
negative fixtures. Successful asset admission is build evidence only; observe
the authoritative frontend readiness and semantic operation contracts normally.

Enrollment submission carries the exact typed instance from the retained
`UiOperationHandle` accessor into the app-owned workflow. The app facade
exports the semantic operation kind; private handle fields remain private.
Compile the actual `web,harness` binary when this bridge changes: an effects
or agent-only WASM check cannot catch stale frontend imports or field access.
The required push-time `just ci-agent-wasm` gate also checks this browser binary
with `web,harness` enabled and warnings treated as errors.

LAN E2E bundles under `.tmp/e2e/run/<host>/artifacts/runs/` are separate from
Cargo compiler caches. `scripts/harness/lan/drv.sh start <config>` creates a
unique bundle and active retention manifest using `AURA_E2E_RUN_TOKEN`.
After stopping the driver and capturing evidence, run
`scripts/harness/lan/drv.sh finish success` or `finish failed` on that host.
The driver refuses to mark an outcome while its REPL is running. Use
`scripts/dev/retain-e2e-runs.sh --root <runs-dir> pin <run-id>` to protect an
important success. Interrupted runs stay active in the manifest and are
preserved for review; an existing bundle cannot be reused by `start`.

Preview retention with:

```bash
scripts/dev/retain-e2e-runs.sh --root <runs-dir> prune --dry-run
```

Apply it only when no harness consumer is active by replacing `--dry-run`
with `--apply`. The default policy retains the newest ten successful runs
within 512 MiB, always keeps the newest success, and preserves failures,
pinned runs, active runs, and older bundles without a manifest. The command
lists exact candidate paths and estimated bytes before removing anything.
Each host runs retention only against its own artifact root. Never delete
the whole `.tmp/` tree as CI cleanup.
The harness matrix owns separate run and transient roots under
`.tmp/harness/`; its run-scope cleanup does not manage LAN bundles or
override their outcome-based retention.

### Debugging Browser Failures

Check `web-serve.log` for bundle and runtime startup issues. Check `preflight_report.json` for browser prerequisites including Node, Playwright, and app URL. Check `timeout_diagnostics.json` for authoritative and normalized snapshots and per-instance log tails. Playwright screenshots and traces are stored under each instance `data_dir` in `playwright-artifacts/`.

`timeout_diagnostics.json` is the primary authoritative failure bundle. It contains `UiSnapshot`, runtime event history through `runtime_events`, operation lifecycle and instance ids, and render and readiness diagnostics along with backend log tails.

For mixed-runtime debugging, inspect `runtime_events` before logs when a code exchange or chat handoff fails. The expected evidence is a typed event payload, the selected semantic target in the snapshot, and only then supporting browser or TUI render diagnostics. For browser runs, the harness observes the semantic state contract first and uses DOM and text fallbacks only for diagnostics. If semantic state and rendered UI diverge, treat that as a product or frontend contract bug rather than papering over it with text-based assertions.

### Frontend Shell Roadmap

`aura-ui` is the shared Dioxus UI core. It supports web-first delivery today and future multi-target shells.

1. `aura-web` (current): browser shell and harness bridge
2. Desktop shell (future): desktop-specific shell reusing `aura-ui`
3. Mobile shell (future): mobile-specific shell reusing `aura-ui`

## Related Documentation

- [Test Infrastructure Reference](118_testkit.md) for infrastructure details
- [Simulation Guide](805_simulation_guide.md) for fault injection testing
- [Verification and MBT Guide](806_verification_guide.md) for formal methods

### Testing independent enrollment transfer

Create the invitee's real provisional account and export its signed setup request. Transfer that request to the initiator's app pin workflow, issue enrollment, then transfer the actual resulting enrollment code, manifest code, and separately obtained initiator verifier code to the invitee. Both native and browser forms expose all three inputs. Shared semantic commands carry the same explicit values and hand off to the app's single import owner.

Capture `device_enrollment_code_ready` as `device_code` to obtain `${device_code}`, `${device_code_manifest}`, and `${device_code_initiator_verifier}`. Supply the latter two using the scenario's `manifest_code` and `initiator_verifier_code` fields. Readiness capture requires the authoritative runtime payload; clipboard reads are not enrollment evidence. Do not fabricate a digest, key, physical device, or trusted admission witness in a positive fixture.

Security integration coverage must exercise actual export → app setup pin → reserved issuance → independent manifest pin → immutable admission → signed response → issuer expected-manifest verification. Negative cases include missing or substituted external pins, wrong physical invitee, altered baseline/parent/node/package/roster/policy, legacy unbound records, and corrupt admission storage. Process restart must reverify retained evidence. Unsupported exact-node inventory, unclosed profile storage ownership, or unavailable threshold quorum signing cannot count as a completed enrollment test.

### Enrollment observation durability regressions

Drive the registered timeout owner before asserting that a restarted readout sees a timed-out terminal decision. Advancing a fake clock alone does not authorize an observed read to publish failure. Exercise secure checkpoint recovery after progress, rollback above the original start, missing state, injected storage failures before operation polling and after completion, and cancellation before checkpoint acknowledgment. Inspect standard error source chains for the original concrete storage error. The Rust-native enrollment boundary fixtures must reject raw/no-op executor aliases and weaker budget parameters while accepting the sealed owner.

### Shared window laws

Core `types::window` tests enforce inclusive start/exclusive end, empty generation allowance, checked endpoint overflow, exact validated serde restore and membership arithmetic near `u64::MAX`. Its two compile-fail examples prevent physical/receipt coordinate interchange. Physical policy tests additionally reject empty/sub-millisecond and invalid restored budgets while preserving the existing timeout snapshot fields. Existing clone/child/retry/restart rollback and required-checkpoint tests remain mandatory. The flow owner must exercise dual-window epoch closure and checked progression when adopting the shared primitive.

### Native semantic failure codes

Native failure projection coverage must assert the operation domain and stable shared code after JSON snapshot roundtrip, alongside the concrete Rust source chain before the foreign boundary. Test cryptographic and codec failures independently of display text, and preserve typed invitation rejection overrides only when actual rejection evidence exists.

### Enrollment window phase faults

Inject interruption after immutable allocation but before initial clock/live registration; recovery must retain the exact original start/deadline despite a later clock. Delete the clock after live admission and assert registration/execution fail without recreating it. Delete a legacy live marker together with the clock while retaining canonical registration and assert conservative rejection. Replace a live budget with the same interval and lease but a distinct observation owner and assert no checkpoint overwrite. Existing tests also require durable highwater/sticky-state monotonicity and allocation continuity through acknowledged writes.

For enrollment receipt tests, use actual setup export, explicit app pin transfer,
issuer-owned ceremony registration, cryptographically verified invitee acceptance
and authoritative finalizer completion. Obtain the committed frame through the
real owner signer and verify it under the retained independent initiator pin.
Exercise immutable receipt publication, recovery signature verification, exact
generation matching, missing/corrupt receipts, modified confirmation epochs,
expired-but-originally-valid acknowledgements, later epoch rollback refusal and
retirement. Raw statuses, manually completed tracker counts and fabricated
manifest digests are not positive trust fixtures. Resource-level process/frame
death tests complement, and do not replace, actual enrollment/profile restart
and cross-frontend user-flow tests.

### Enrollment task source continuity

Persistent profile fixtures allocate a fresh OS temporary directory for each test participant. Process-local counters alone repeat across test binaries and can reuse immutable admissions or secure keys. A restart case reuses its captured original profile path intentionally; unrelated participants and later test runs receive new paths. This applies to simulation effect factories as well as runtime builders.

Exercise an actual registered-window admission rejection through the required enrollment initiator failure settlement path and the fallible one-shot supervisor. Assert the original concrete `AgentError` is reachable from both retained health failure and drain error, and any failed terminal settlement retains its own native cause. Do not replace real window admission with a fabricated error or synthetic successful task.

Required enhanced-time scheduling propagates actual provider query and sleep failures. A failed timer does not constitute deadline expiration or successful wake-up. Native source traversal retains the concrete time-effect error through runtime scheduling and effect-system forwarding.

Sync command service ownership requires source-preserving timer, shutdown-signal and runtime supervision outcomes. Every daemon run exit awaits service stop. Failed execution remains primary when stop also fails, with a separately retained typed cleanup cause. A failed timer or backward physical clock cannot publish a tick or successful shutdown. Native terminal diagnostics retain concrete sources; cloned source-bearing errors compare retained source identity rather than matching message text.

### Enrollment import publication regressions

The protocol tree tests inject failure before and after canonical index publication, reload the handler, and require a complete original or complete replacement history. Replay tests require preservation of later operations and reject an unrelated replacement. The actual two-runtime committed-confirmation fixture installs the independently admitted generation through the production import owner, verifies activation, and then checks that import replay preserves the adopted encrypted share and final configuration. Native encrypted-storage tests share a real profile lease/provider between independently initialized wrappers and preserve a malformed original master key. These tests do not establish the still-required frontend profile WAL, same-epoch revocation authorization, or quorum path.

### VM send custody regressions

Exercise the actual runtime bridge queue under an exclusive delivery lease.
Require rejection of a second owner, retained FIFO order after a successful
prefix and definitely-unsent failure, and enqueue behind the retained suffix.
Cancel an awaited in-flight future and verify that the original frame remains
with `DeliveryUnknown`, preventing automatic replay. Native flush failures must
retain their concrete transport or configuration source and pending frame.
Compile-fail coverage prevents cloning the lease and using a destructive drain
API. Match typed variants; matching rendered diagnostics does not prove custody
or retry authorization. The stateful test provider implements the same delivery
contract instead of claiming success from a drained queue.

### Required refresh supervision checks

Exercise the actual signal subscription and refresh owner with a failing native provider, then assert the typed attachment failure, original concrete source, group cancellation, and retained supervisor failure. Required task admission tests cover native/local execution and rejection dropping the supplied unpolled future. Keep the required-owned-task compile-fail guard: a unit future must not satisfy the required task API. Runtime health remains failed until its owner replaces the failed runtime generation; app reattachment alone is not a health reset.

### Enrollment first-decision provider tests

Use a real runtime-exported setup and actual app transfer pin to test duplicate retained binding and contradictory generation rejection; verify original secure bytes remain unchanged. Pair owner tests with `aura-effects/tests/secure_immutable_publication.rs` actual process contention, asserting exactly one complete first publication. Provider lifetime protection must additionally prove generic mutable write cannot replace a previously immutable value; absence-only races do not establish that stronger property.

### Held registration boundary checks

Use an actual prepared unissued rotation reservation to reject another runtime's effects and a previous ceremony's authentic invitation/state. Assert structural native causes. Existing real issuance tests supply the positive owned path; rerun them after changes to roster or deadline checks. Keep the reservation type private-field and non-deserializable, and require it in the live registration signature. Raw registration snapshots alone must not satisfy that API.

### Persistent allocation owner regression

In the actual prepared unissued-rotation fixture, attempt generic tracker registration while the real generation reservation is held. Assert typed `HeldEnrollmentRegistrationError::RequiredOwner` and absence of the durable allocated record. Existing actual issuance fixtures exercise successful owned registration. Persistent enrollment clock tests must obtain the real held reservation; do not re-enable raw snapshot allocation or add a test-only authorization bypass to simplify those fixtures. Nonpersistent tracker state-model tests can keep generic registration because they cannot authorize production activation.

## Actual secure-provider fault validation

The optional native credential-provider lane uses the selected OS keyring with
a private fixture service and the actual profile and namespace owners:

```sh
cargo test -p hxrts-aura-effects --features test-support platform_secure_record_fault_tests
```

The lane checks legacy original sealing, mutation refusal, actual wrapping-key
loss without replacement, and legacy payload collisions with the protected
record format. Provider unavailability is a typed failure; tests do not skip or
switch to the filesystem fallback. Fixture cleanup targets only its private
service and is a test-owned backing-provider fault operation. A successful
filesystem lane does not validate this OS-provider lane or the browser
WebLocks/IndexedDB lane. Cross-process OS-provider recovery and actual browser
multi-context fault coverage remain separate required validation.

### Enrollment peer epoch fence migration

Run the sync enrollment_epoch_commit_migration_tests lane to verify that the
actual historical schema encoded by the runtime DAG-CBOR codec decodes with no
v2 fence and cannot authorize enrollment. The old rotation/removal transcript
must remain byte-identical. The pure v2 transcript tests check domain separation
and binding to each operation hash; they do not substitute for genuine quorum
participation, held peer activation, or process-restart tests.

### Enrollment notice identity and production compilation

Run the exact retained notice identity regression alongside the actual
default-stack enrollment caller fixture. The former rejects digest, transcript
and expiry rebinding; the latter uses real setup, registration and admission
before checking the issuer/invitee 16 KiB caller-future budget. Neither pure
identity tests nor increased stack settings establish complete cancellation
delivery or restart behavior.

Check the agent and simulator libraries without `cfg(test)` as a distinct
validation lane. Notice draining must retain its native failures in production
builds as well as unit tests; a test-only helper cannot supply production
cleanup. Run `cargo check -p hxrts-aura-agent -p aura-simulator --lib` in the Nix
environment with the same bounded build queue used for the focused tests.

### Startup authorization fault coverage

Run the agent `required_biscuit_hydration_` test filter when changing credential
restoration or builder error adapters. Its real profile/token fixtures cover
confirmed absence, malformed payload, successful retained-token hydration,
returning-runtime rejection and encrypted backing corruption. The corruption
fixture changes selected provider ciphertext under native test support; ordinary
production overwrite/delete protections must remain enforced. Check native
error causes as well as the serialized failure classification, and separately
compile the production agent library so test-only methods cannot hide missing
required runtime APIs.

### Enrollment custody contention regressions

The runtime tests `enrollment_custody_waits_generation_then_decision_then_tree`
and `physical_generation_cannot_enter_another_runtime_tracker` use the actual
export, app pin, retained manifest and physical storage fixture. They poll an
owned future once to verify a held mutex dependency, rather than sleeping for
an assumed ordering. The first test holds the physical generation gate and
checks that activation has not acquired the tracker decision, then holds the
tree lease and checks that fresh roster preparation has already acquired the
tracker decision. It also checks that a second real owner advances only after
the first composite owner releases. The second test rejects custody from a
different real runtime. Run these tests with the agent suite and ownership
compile-fail lane when changing the custody hierarchy; a count-only tracker
fixture does not verify physical generation ownership.

Reserved issuance regressions must exercise the actual pure guard outcome and
native boundary, distinguish insufficient budget from missing capability, and
assert the concrete structural source remains reachable. Canonical original
creation/expiry are tested against a newer guard clock and expired reservations.

### Pinned VM lifecycle regressions

The lifecycle gate resolves the locked crates.io `telltale-machine` release
`17.0.1` through Cargo metadata and rejects local overrides or other versions.
Its unit tests have their own development dependencies and run using the resolved
package manifest. Run the required source, discovery and execution inventory:

```sh
just ci-vm-session-lifecycle
```

These regressions exercise actual scoped worker completion, unrelated-session
survival after coroutine compaction, stable IDs for later sessions, and required
lock/index/epoch failures before mutation. They must accompany changes to the
dependency lifecycle API. Also run actual Aura backend close/cancellation tests;
dependency success alone does not verify host binding or fragment retirement.
Cooperative teardown needs its own nonterminal-session and surviving-session
coverage. A closed status or removal from the host's active set alone does not
prove target coroutine/resource disposal. Regenerate `Cargo.nix` when changing
the pinned dependency release.

Deadline attenuation regressions in `aura-core::time::timeout` must prove that
shorter signed validity retains the original start and parent deadline, child
exhaustion stays sticky after restore, and parent/child rollback history is shared.
The pure tests reject mismatched paired snapshots; they do not prove durable
checkpoint provenance. Pair them with the actual domain owner's required-write,
restart, expiry and missing-checkpoint fault tests when changing peer admission.

### Required VM disposal regression lane

Run `just ci-vm-session-lifecycle` after changing a VM backend, targeted session
close/reap, coroutine index ownership, worker acknowledgment or forced-drop
custody. `just ci-ownership-policy` includes the same lane. The Rust inventory
checks actual nonignored test declarations and published harness names before
running the dependency and runtime suites in sequence. A renamed, absent,
feature-excluded or ignored required fixture fails instead of producing a
successful zero-test result.

The dependency tests exercise targeted removal, same-role surviving sessions,
new-session dispatch, genuine scoped worker acknowledgment, actual poison and
epoch faults, natural terminal epoch preservation and cooperative deserialization
index continuity. Runtime tests exercise both real backends, original error
source chains, exact forced-drop owner retirement, stale-owner rejection and
cleanup before supervisor idle publication.

Session owner coverage additionally uses equal metadata in two actual runtime
registries to prove that foreign claims cannot validate, release or transfer
ownership. The lane discovers and executes the opaque capability doctests,
including a valid observation consumer and rejected construction, field mutation
and wire deserialization. Their actual harness inventory prevents absent guards
from becoming a successful zero-test run.

The dependency override is outside the workspace. This lane invokes its actual
manifest with the multi-thread feature and shares the selected target directory
with runtime validation. It does not run competing builds. Keep its original
release provenance and license together with the patch record; update Cargo/Nix
pins when replacing the override. Disposal acknowledgment is local cleanup;
signed protocol outcome and remote delivery retain their separate owners.

### Final active enrollment inventory regression gates

Exercise a genuine first enrollment through authoritative commit, then a second independently pinned issuance after its attested epoch fence. Assert that the new final inventory uses the actual active threshold package while all historical parents remain at their authentic earlier epochs. Test canonical prechange version-1 schema bytes, original signature-domain compatibility, missing-inventory rejection, v2 old-domain signature rejection, prefix/version mismatch and tuple substitution. Internal trait assertions guard capture cloning/deserialization. Keep nonroot persistence, existing-peer response restart, distributed quorum and native/browser profile WAL coverage explicitly outstanding until exercised.

### Public FROST primitive evidence

Run `just ci-public-frost-signing` when changing public commitment construction
or independently bound-message signing. The native gate requires real harness
publication of both nonignored primitive regressions and the public-only API
compile-fail guard before running them. The positive test verifies a native
2-of-3 signature; the negative test rejects transcript, roster, policy and nonce
substitutions. Synthetic key bytes cannot substitute for failed dealer
execution. Passing this lane establishes primitive behavior; runtime quorum
admission, durable one-use nonce custody and multi-runtime recovery require
separate owned protocol tests.

The VM lifecycle lane requires public live cancellation/retry to retain its
primary Cancelled decision and original interval. It also requires an actual
task-registry source-chain/drain regression for subsidiary failures recorded
after primary completion. These checks complement signed notice delivery after
real runtime reopening; observing local cancellation alone does not establish
remote delivery.

The required VM lifecycle inventory also discovers and executes the confirmed
parent archive ownership guard and the actual committed-confirmation fixture.
That fixture covers immutable v1 preservation, explicit v2 publication, selected
provider reload, substituted history and foreign-runtime rejection. It does not
establish process restart or native/browser profile WAL handoff; those require
their own connected runtime tests.

The same required inventory executes generation-history migration and actual
same-epoch reissue. Those tests must preserve original allocation/registration
bytes, reject forged mutable slot policy, and complete genuine wrapping-secret
retirement before reuse. A slot-only test or a bypass of immutable provider
protection cannot satisfy this gate.

Required admitted-clock regressions distinguish immutable original anchors from
mutable checkpoints, verify repeated progression/restoration, and reject missing
ever-live checkpoints. Legacy intervals remain attenuated; fresh intervals use
the signed manifest endpoint. Full process reconstruction and legacy builder
migration require additional connected tests.

The lifecycle gate also requires the distinct response-policy regression:
remote response quorum and signing-key quorum are separate protected policies.
Recovery compares each original commitment exactly, including typed mismatch
causes; it cannot clamp recovered signing quorum into a response quorum.

Historical enrollment response-policy coverage must exercise an encoded allocation
that actually lacks the newer response commitment. The required agent fixture
`runtime::effects::crypto::missing_response_policy_history_tests::truly_old_missing_response_policy_uses_protected_original_registration_and_preserves_bytes`
uses genuine protected original registration/setup evidence and selected-provider
fault injection. It checks unchanged historical bytes and deadline, absence of
verified policy after serialization, and source-bearing failure on supplement
loss. The native VM lifecycle inventory requires discovery and actual execution;
profile-layout migration alone does not prove this old-schema contract. Full
AgentBuilder reconstruction remains separate required integration coverage.

The VM lifecycle gate checks each required suite at three boundaries: nonignored
source declaration, publication by the actual harness, and exact executed
`test <required-name> ... ok` evidence after successful Cargo execution. A green
process status or test listing alone cannot satisfy execution coverage. Its
adversarial unit coverage rejects zero-test, ignored, failed, listing-only, and
similar-name outputs.

Registered notice binding changes must retain the actual public live cancellation
and retry fixture, original-window assertions and exact manifest/transcript/expiry
substitution negatives. A single-assignment cell must compare every candidate
against its original value; replacing a blocking mutex does not excuse dropping
those checks. Strict Clippy and the connected ownership lane remain required.

Threshold enrollment signing-owner selection must cover actual finalization into
a threshold epoch before invoking the next identity selector. Required native
coverage distinguishes genuine `QuorumOwnerRequired` Service from actual protected
share loss (Storage). A separate deterministic native FROST dealer regression
checks unsupported threshold-one construction; domain-valid metadata alone does
not establish backend support. The second-issuance runtime fixture remains required
until distributed signing and bounded owner assembly are connected.

Registered enrollment window regressions must use the real signed issuance path
before acquiring the strongest registered generation. Retain the original manual
clock and tracker/runtime provenance; do not create a test-only canonical
invitation or weaken admission to a raw ceremony ID. Releasing the sender owner
must preserve actual active eligibility, and contention must retain typed
AlreadyOwned plus the concrete semaphore cause. Pre-live clock laws use their
actual held allocation separately.

The public FROST signing gate requires exact per-test executed `ok` evidence for
its native dealer and public-only threshold fixtures, in addition to source
inventory and harness discovery. A successful command with zero, ignored or
merely listed tests cannot establish the required primitive evidence. Unified
threshold verification must decode the canonical public package and verify with
its group point; the real two-of-three fixture also rejects substituted messages
and malformed public signature inputs. These tests do not prove distributed
runtime quorum custody or profile recovery.
Rustdoc discovery omits the execution suffixes ` - compile` and ` - compile fail`.
The execution validator normalizes those two suffixes when matching exact test
names; changed source lines, failed/ignored results and discovery-only output
remain rejected. The API guard inventory requires both the positive compile
consumer and the negative compile-fail case to execute successfully.

### Compile-fail process lock recovery

App, signal-workflow and agent compile-fail harnesses acquire one `TrybuildProcessLock` from aura-testkit for the workspace. The helper derives `target/tests/.aura-trybuild.lock`; suites must not introduce package-specific or signal-specific lock namespaces. It owns an actual file descriptor and uses fs2 0.4.3's Unix/Windows advisory locking, without requiring post-MSRV standard-library file-lock APIs. Closing the descriptor or terminating its process releases the lock. The lock file stays in place, so no removal/recreation can split the lock inode. Old `trybuild-lock` directories no longer authorize or block acquisition.

Acquisition waits at most 900 seconds in compile-fail suites. A timeout retains the actual last native contention error; opening/locking failures retain the original IO error. The process-lock integration test launches a real holding subprocess, proves contention, kills it without Rust cleanup, and verifies bounded acquisition of the same persistent lock file. Its namespace test also covers stale legacy directories and ordinary descriptor release. Run `cargo test -p aura-testkit --test process_lock` before the existing app/agent compile-fail ownership gate. Dependency changes require Cargo.lock and Cargo.nix regeneration/validation before integration.

`just ci-ownership-policy` executes the app/agent compile-fail suites, explicitly enabled signals compile-fail suite and native process-lock integration regression sequentially. Required suites fail when Cargo is unavailable or returns an unsuccessful version status; a signals-disabled build contains no pretend passing guard. The fs2 implementation is pinned to 0.4.3 and uses pre-MSRV descriptor APIs; Windows and Unix use their native process-scoped locks.

Required ownership coverage checks three separate forms of evidence: Rust test attributes without ignore annotations, exact names published by the selected harness, and successful execution of every required test. The signals harness is selected with `--features signals`. The process-lock lane requires the forced termination, shared namespace and native IO source regressions; its child-process helper does not satisfy coverage. Captured pretty harness output keeps nested trybuild diagnostics from splitting result lines. Zero-test, ignored, failed and name-lookalike output cannot satisfy the gate. When adding a required ownership test, update the typed suite inventory and its validator regressions; do not replace execution evidence with a successful Cargo exit status.

### Public signature ingress provenance regressions

Run both `crypto::signature_input::tests` cases in `hxrts-aura-core` for real
SingleSigner inputs and genuine dealer public-package parsing. Run
`enrollment_setup::tests::malformed_peer_encoding_is_distinct_from_required_provider_failure`
in `hxrts-aura-invitation` for a genuinely signed request and the Layer 8
`aura-testkit` verification-outage mock.

Malformed peer package and fixed-length signature inputs must produce
`InputEncoding` before provider invocation. A correctly encoded signed request
must reach the injected provider and retain the exact original native cause as
`Crypto`. These tests cover input and error provenance; they do not establish
independent pinning, runtime quorum or restart ownership.

The native `public-frost-signing` gate requires all three tests as nonignored
source declarations, discovers their exact names from the actual library
harnesses, and checks successful per-test `--exact` execution. Missing, ignored
or zero-test coverage fails the gate.

The native `public-frost-signing` gate requires all three ingress tests as
nonignored source declarations, discovers their exact names from the actual
library harnesses, and checks successful per-test `--exact` execution. Missing,
ignored or zero-test coverage fails the gate.

The ownership aggregate also discovers and executes the two native agent deterministic RNG custody regressions individually with exact test names. The interleaved clone/continuation regression compares actual subsystem draws against one independently seeded reference, including after original handles are dropped. The independent-seed regression proves reproducibility without shared custody. Missing or ignored coverage cannot satisfy the gate.

The production task-spawn lint permits concurrency fault-injection inside
provably test-only configurations. Its parsed configuration rule accepts
`cfg(test)` and `cfg(all(test, unix))`, and continues checking `cfg(not(test))`,
`cfg(any(test, unix))` and feature names containing `test`. Ownership CI requires
actual execution of the adversarial configuration regression.

Core and macros guard suites depend on the same host-only descriptor lock package
that aura-testkit reexports: `toolkit/test-support/process_lock.rs`. This keeps
L1/L2 test infrastructure free of upward Aura dependencies. Core, choreography,
marker and service-surface suites are part of the exact required ownership
source/discovery/execution inventory; successful zero-test exits do not count.

`just ci-agent-wasm`, invoked by the existing conformance CI workflow, runs
warnings-as-errors Clippy for the wasm effects library before the agent backend
matrix and the actual `aura-web` harness binary. This gates browser
secure-provider API, frontend facade and lint regressions that native
Clippy cannot observe.

Capability-boundary changes require the full macro validator regressions and compile-fail suite, followed by ownership CI. Use an exact signature type; when a semantic capability label differs, add `capability_type = Type`. A held runtime receiver can declare `receiver_type = OwnerType` and must pass the generated concrete type check. Do not add decorative capability constants or annotate String/AuraError as authorization evidence. Classify pure validators and observed projections accurately. Readiness publication helpers retain the actual readiness capability through publication. The aggregate requires source attributes, actual discovery and successful exact execution of the full-validator adversarial tests; compile-fail snapshots must be checked against real compiler output.

The parsed test-only cfg rule applies to the common ownership lint helper,
including enrollment window and semantic boundary visitors. Tests must cover
production-capable negations and disjunctions; substring-based exceptions are
not permitted in those visitors.

The security boundary gate requires exact source, discovery and executed results
for the selected filesystem allocation lifetime inventory. The 24 native tests
cover first physical-provider attachment, original seals and checkpoints,
retirement ACK/replay, legacy immutable preservation, lifecycle loss, codec
classification and total ciphertext limits. Zero-test or ignored output does
not satisfy this lane. Native keyring/browser parity needs separate tests.

Allocation lifetime syntax checks normalize source paths against the explicit
checkout root before comparing sanctioned owner files. Outside-checkout sources
fail closed. The security gate self-checks absolute owner and foreign paths;
retain these regressions when changing file discovery or path handling.

The private lifetime codec also requires byte-array compatibility and
malformed/oversized/later-field decoder regressions. Secret fields retain
Zeroizing ownership during incomplete deserialization, before record Drop.

For a scoped secret-codec exception, run both
`just _policy-check check secret-field-wrappers` and
`just _policy-check check security-exception-metadata`. The complete security
boundary gate includes both; a focused checker must name a registered command.

`just _policy-check check secret-lifetime-regressions` runs the security
gate's exact selected-provider source/discovery/execution inventory as a focused
lane. It requires all declared native tests to execute successfully with no
ignored or zero-test substitute. Run it for private lifetime codec/provider changes.

Custom builder changes must run the required `custom_provider_fidelity` native
integration harness through the ownership aggregate. Its async and sync tests
exercise actual selected storage/crypto/random/console owners and native outage
chains; transport tests reject fallback after selecting a failing provider. Keep
stateful fault providers in L8. Successful construction or a compile-only
typestate guard alone does not prove configured providers reach dispatch.

### Allocation lifetime integration fixtures

Use the crate-private native unit-test `TestingOwnedProfileCapability::acquire(&config)` and `EffectSystemBuilder::testing_with_owned_profile(capability)` when a real enrollment test requires selected filesystem custody. Acquire it before signing bootstrap. The ordinary Testing builder intentionally has no physical lifetime owner; supplying its production `with_profile_owner` ingress is an error. Keep the capability paired with the original configuration and use the shared transport and physical-time provider builder methods normally.

For restart evidence, acknowledge old task shutdown, drop the old runtime and any retained effect handles, then acquire the same original profile for the new owned Testing assembly. Preserve original allocation, decision and window checkpoints. Do not copy or synthesize a lifetime root or use a missing-custody fallback. The ownership adapter regression covers actual lease retention, foreign configuration rejection and the unchanged production-lease guard; the real enrollment retirement/reissue and cancelled-notice restart tests exercise ledger recovery end to end.

Owned Testing profile assembly changes must preserve the three actual custody
regressions in the required VM lifecycle source/discovery/execution inventory:
lease retention through shutdown, foreign configuration rejection, and ordinary
Testing rejection of production lease ingress. They supplement the connected
enrollment history and cancelled-notice restart evidence.

For canonical participant envelope changes, run the actual threshold-signing
producer/consumer regression and connected enrollment history, cancelled-notice
restart, and secret-retirement/reissue restart tests. The required VM lifecycle
inventory checks their declarations, actual harness discovery and successful
execution. Preserve explicit foreign-authority and raw-package rejection and
concrete codec sources. A legacy-only fixture cannot prove allocation-version
consumer custody.

Required identity envelope changes retain the exact bootstrap codec/AEAD/bounds
regression in the VM lifecycle execution inventory. The strongest original
runtime/epoch/participant context remains required; failed canonical decoding
cannot select a companion package or older identity. Preserve bounded encoded
input and nonempty bounded ciphertext before cryptographic work.

Active identity handler changes preserve required rendezvous corrupted-primary
and contact missing-key regressions in the VM lifecycle inventory. Fixtures use
actual canonical threshold bootstrap and matching runtime authority; do not
install raw guessed-epoch packages. Failed signing identity selection or package
reads must retain their typed cause and cannot report response success.

Rendezvous manager identity ingress retains the actual selected runtime and
active physical identity context. Preserve its required corrupted-primary
regression, concrete provider/codec source, and absent-descriptor postcondition
in the VM lifecycle inventory. Generic permissive storage/crypto mocks cannot
prove authoritative identity selection.

Native test profile allocation is fallible and uses exclusive creation. The
required ownership gate executes collision exhaustion/preexisting-permission
and native creation-fault regressions. Old temporary directories are never
reused, deleted or chmodded to repair a fixture.

The selected-lifetime required inventory includes a real subprocess killed
after protected ciphertext staging and before final link at eight initial
publication boundaries. It also requires malformed/anonymous/conflicting-stage,
once-live loss and actual positive-first-decision preservation regressions.
The subprocess helper itself is not counted as coverage. Older anonymous stages
and interrupted mutable replacement remain explicit fail-closed cases, not
authorization to clear a profile or allocate a replacement root.

Required lifecycle source inventories parse the sanctioned
`large_stack_async_test!(name, { ... })` declaration as a typed Rust macro
invocation. Ignored, malformed, unrelated and qualified invocations cannot
satisfy required evidence. Source recognition is followed by actual Cargo
harness discovery and exact successful execution; declaration presence alone
never establishes coverage. Required ownership coverage also executes the
persistent tree's held-decision replacement and authenticated-extension tests.

Required invitation stage adapters retain actual clock, policy and timeout
sources. Their child budgets share the original observation owner; rollback and
required checkpoint faults must not be classified as elapsed deadlines. Required
network retry timers propagate failure, and automatic transport retries are
limited to native definitely-unsent destination-unreachable errors for the exact
requested destination. The lifecycle gate includes the generated rollback,
original deadline and invalid-policy source regression.

### Original invitation identity evidence

Run `just ci-vm-session-lifecycle` after changing invitation signer birth,
retention, recovery, package selection or required test declarations. It requires
actual Contact success and revoked refusal, original Contact/Guardian rotation
and profile restart, copied-record rejection and equal-ID foreign-owner rejection.
Positive fixtures bootstrap real canonical physical identity packages; raw
`SingleSignerKeyPackage` fixtures do not authorize production export.

The inventory parses the exact `large_stack_async_test!(name, { ... })` source
grammar as well as real test functions, rejects ignored/malformed/unrelated
declarations, discovers exact harness names, and requires successful execution.
This recognizes existing legacy contact fixtures; it does not establish their
ordinary-stack compatibility. New original-issuer ownership regressions use
ordinary Tokio tests. Missing required records retain their logical absence
cause; tests must not fabricate an operating-system error for absence.

The required contact verifier regression exercises actual Ed25519 signing and
verification, an actual provider key-length failure, and a well-formed invalid
signature. The provider failure must survive standard `Error::source` downcast;
only the invalid signature may return `Ok(false)`. Run the exact required
`required_contact_verifier_retains_native_failure_and_distinguishes_invalid_signature`
case through `just ci-vm-session-lifecycle`.

Required selected-lifetime coverage includes the actual target-insertion
interleavings after staged observation and between the held source check and
link (including equal ciphertext on a substituted inode), identical ciphertext acknowledgment,
foreign stages in nested namespaces before and after initial handoff, and an
allowed filename carrying invalid ciphertext. The Rust-native required gate
checks source declarations, test discovery and exact execution; zero executed
tests cannot satisfy this coverage. Mutable successor process-death recovery
requires separate original monotonic-transition evidence and coverage.

Stage-inventory coverage also scans the maximum supported allocation-leaf
layout together with more than 4096 ordinary leaves, and verifies actual typed
ambiguity/depth failures retain backing evidence. The scan is constant-memory
with respect to ordinary leaf count; its native IO latency is linear and
original builder execution-window integration remains open.

The exact original-stage reader also streams unrelated namespace entries while
retaining at most one candidate name. Required execution reopens a genuinely
initialized profile with more than 4096 namespace siblings and acknowledges only
its identical original protected seal. An ordinary namespace count is not a
new lifetime-admission limit.

Required initialization tests also terminate the actual writer after successful
link and before stage removal at seven original immutable targets, then reopen
the same original selected profile. They verify inode link counts return from
two to one, root identity and immutable ciphertext remain unchanged, and an
unrelated identical ciphertext alias is rejected with retained native evidence.
Mutable successor publication remains a separately verified transition scope.

Process-death checkpoint waits use `PhysicalTimeEffects` and one original
`TimeoutBudget`; each diagnostic pause is bounded by the remaining budget.
Test-only fault registries use `tokio::sync::Mutex::try_lock` with explicit
busy failures and never hold locks across awaits. Strict effects test Clippy
enforces these boundaries.

### Contact continuation ownership regression inventory

The VM lifecycle lane requires the actual signed-import corruption, decision lease, original-parent deadline/shared rollback and native clock failure tests in `owned_contact_continuation_tests`. Run the existing Contact confirmed and revoked integration cases in the same serialized lane. `ManualPhysicalClock::provider_faults_are_one_shot_and_wake_original_waiting_sleep` verifies provider fault injection without physical progress. Automatic recovery of an interrupted Contact wait requires retained original-window and payload evidence; these in-process tests do not establish that recovery contract.

The required VM lifecycle lane executes Contact continuation regressions through
the agent harness and the one-shot physical clock fault/wakeup regression through
the testkit harness. Each suite must provide nonignored source declarations,
actual harness discovery and successful exact-name execution; tests assigned to
a different package cannot satisfy the requirement.

Required transcript encoding evidence runs in the `hxrts-aura-signature` library
harness within `just ci-vm-session-lifecycle`; Guardian partial-key reopen,
concurrent original-pair birth, and native signer/verifier failure evidence runs
in the `hxrts-aura-agent` harness. The inventory requires real source declarations,
harness discovery, and successful execution by exact name. Partial-key tests
reopen actual exclusively owned encrypted profiles and verify that retained
original bytes survive without a replacement half. This evidence does not cover
interrupted initial pair publication or complete historical loss of both halves.

The mandatory signed Guardian offline-response regression also corrupts actual
retained imported metadata before invoking the response handler. It requires a
native JSON codec failure and absence of both recovery key halves, restores the
original stored record, and retains the original offline-principal timeout
expectation. Cached imported state cannot substitute for required backing evidence.

Required Guardian window coverage holds the actual owned runtime's imported
decision lease before polling the public response operation. Advancing its
selected manual physical provider past the original deadline, or failing the
already waiting provider sleep, must terminate with the corresponding typed
cause before any recovery key birth. This is actual operation/preparation
coverage; acknowledged VM teardown remains a separate requirement.

The required Guardian terminal-close regression uses the actual VM engine and
runtime session owner. It retires that owner before advance/close, checks that a
successful primary cannot hide failed close, and checks that simultaneous native
operation and close failures both survive. It also verifies no runtime binding or
fragment custody remains. Generic mocked close results do not prove this boundary.

### Required original runtime shutdown regressions

The native VM lifecycle gate runs the exact operation-drain, capacity and actual
effects-admission suites plus the retained invitation-service clone and original
TaskGroup shutdown tests. Each required name must be discovered and reported as
executed successfully. These fixtures cover original lease retention through
awaits, equal-valued foreign profiles, closed admission, cancellation, required
clock source retention, and actual supervised descendant destruction. Run the
existing required lifecycle/ownership lane after changing these contracts.
Passing this slice does not certify quorum enrollment or provider handoff.

Runtime activity observation doc guards require three real cases in ownership
CI: permitted state observation, compile-fail external admission closure, and
compile-fail external shutdown-completion publication. Required discovery and
execution must retain both privacy negatives rather than accepting only a
positive getter example.

### Architecture lint scan inputs

Architecture syntax lints validate each requested path and inspect tracked plus
new untracked Rust sources. Directory scans exclude ignored artifacts; explicitly
requested Rust files remain inputs even when ignored. Missing input or malformed
new source fails the command. Run `cargo test -p hxrts-aura-macros --test
lint_input_discovery` for the actual CLI discovery regressions.
A scan with no Rust inputs fails explicitly. An empty Git inventory does not
fall back to traversing ignored artifacts; filesystem fallback is reserved for
unavailable Git discovery.

Run `just _policy-check check secret-lifetime-regressions` after changing original
initialization cutover or recovery. The required native inventory covers actual
child termination at journal stage/ACK, individual custody publications, atomic
exchange, ACK and archival, plus source/target substitution, consecutive original
index transitions, historical corruption, unknown metadata and first-decision
preservation. Worker helpers are not standalone coverage. Preserve the original
selected lease and use backing filesystem faults in negative fixtures; never
weaken immutable-provider APIs to corrupt evidence. The initial transaction tests
do not complete live mutable checkpoint or original builder-window recovery.

The archived-custody regression physically removes or substitutes each predecessor, successor and displaced record with identical ciphertext at a different inode. Actual reopen must retain the original native failure and preserve both conflicting evidence and original bytes.

Required archived-custody validation also rejects ciphertext corruption at each of the three retained paths, preserving the original birth and current root checkpoint bytes without replacement.

## Original service-stop regression

The required VM lifecycle lane executes the exact
`required_service_stop_retains_original_shutdown_deadline_under_actual_state_contention`
regression. It holds actual threshold-service state across the original shutdown
deadline, checks retained native time/provider failures, and verifies that failed
required disposal leaves authority status nonterminal. A second actual owned-task
failure regression verifies that successful later service cleanup cannot publish
authority termination after prior task-tree failure. Run `just ci-ownership-policy`
when changing this boundary; discovery without actual execution is insufficient.

The required original service-stop suite also blocks the actual threshold
service's health read after its stop completes, using its real fair lifecycle
lock. Deadline expiry must leave runtime/authority termination unpublished.
Retrying with a rolled-back then restored physical clock retains the original
sticky rollback cause rather than starting another observation window.

The required Guardian native-provider regression constructs the actual custom
runtime with `CustomCryptoProbe`, then faults generation, continuity signing and
verification, and canonical response signing/verification. It requires retained
native provider causes, no key publication after failed generation, unchanged
original keys on later faults, and genuine verification after restoring the
same provider. This does not certify interrupted key birth or VM disposal.

Required session-owner and runtime-admission doctests use exact rustdoc discovery
and execution evidence in the VM lifecycle lane. Every unique published guard
must pass; a successful Cargo exit or duplicate/name-lookalike listings cannot
replace execution. Adding or removing guards requires updating the expected
inventory and its validator regressions. Run `just ci-ownership-policy` when
changing these ownership boundaries.

Repo-local policy validators have a separate strict lint lane:
`just ci-policy-toolkit-clippy`. The crate is excluded from the Aura Cargo
workspace, so workspace Clippy does not validate it. `just ci-clippy` includes
both lanes. The repo-local lane also executes all policy-toolkit unit tests,
including lexical test-scope and semantic-owner raw-commit regressions.
Pair validator changes with the relevant toolkit tests; warnings
must be repaired without weakening validation predicates or adding suppressions.

The mandatory Guardian lifecycle inventory also verifies exact recovery-key
continuity transcript encoding against the original domain/schema/payload.
Continuity uses the required typed signing and verification helpers, so concrete
provider faults remain errors rather than signature refusal or replacement-key
permission. The signed-transcript policy must pass without widening exceptions.

The Contact required-verifier regression uses a genuinely issued and imported
invitation, acknowledged acceptance and original retained issuer signing context.
It checks actual matching-key verification, foreign-runtime rejection, exclusive
custody across verification, signature refusal without terminal proof, and native
provider failure from a malformed required stored key. The typed syntax validator
requires the primitive key to come from the original capability's checked
accessor, rejecting peer keys, fallback repair, shadowed inputs and deserialization.

The mandatory lifecycle lane also discovers and executes the public invitation
handler's three verifier guards: public observation compiles, private verifier
construction fails, and observed envelopes cannot call internal terminal response
publication. Preserve both the positive control and exact compile-fail execution;
private helpers must not become raw-key or observation-based authority APIs.

### Required sync native-source evidence

`ci-vm-session-lifecycle` also executes the exact `aura-sync` native-source inventory: real malformed JSON decoding with direct concrete downcast and Clone, pure diagnostic category preservation, and retained nested/terminal causes. Source validation, Cargo discovery, and execution are all required. The same lane discovers and executes the `SyncDiagnostic` and `sync_error_with_cause` compile-fail guards, rejecting existing error/terminal conversion and string-only native causes. These tests establish provenance; they do not establish requested-peer success or remote teardown.

### Guardian verifier role regression coverage

Guardian verification has distinct required owners for original local-pair
integrity, imported issuer continuity, and first-binding recovery possession.
The Rust key-origin validator checks exact immutable helper inputs, retained key
accessors and original runtime/lease checks. Its negative cases reject raw and
peer keys, qualified or wrong-role types, public fields, reconstructed owners,
and shadowed bindings. Production-source mutation cases also reject missing
issued-runtime, invitation-id and issuer-key comparisons.

Required runtime regressions use actual owned profiles, genuinely issued
invitations and real two-runtime choreography. Same-authority foreign-runtime
pair verification is rejected; original configured-provider faults retain native
causes. Primitive provider-fault fixtures have no terminal publication authority.
Run the strict toolkit lane and `just _policy-check check security-boundary-policy`
with the required lifecycle inventory. A focused validator result does not prove
whole-runtime teardown, durable historical recovery, or enrollment quorum handoff.

Owned Guardian verifier byte origins also use a Rust AST check: a declared
`SecurityTranscript` and the fallible required canonical encoder must produce
immutable bytes passed into the primitive. Actual-source mutations reject raw
or fallback bytes, shadowed bindings, missing trait declarations, wrong
confirmation factory return types and replaced encoder origins. Unproved paths
retain the existing syntax checks; a nearby transcript comment cannot override
a rejected typed origin. Run `just _policy-check check signed-transcript-boundary`
and then the complete security boundary policy.

Security bypass test exclusions are lexical Rust `cfg(test)` scopes. Mixed
`any(test, production_feature)` remains checked as production. Actual-source
coverage removes the Contact fixture's test cfg and verifies that its test-like
module name does not exempt the unsigned source. Nested test impls and later
production items are covered separately; parse failures fail the policy check.

### Required Sync command evidence

The lifecycle aggregate discovers and executes exact Sync registry,
requested-session and session-window tests. Registry coverage includes actual
foreign admission rejection, clock failure before actor birth, admission release
at registered handoff, real requested-peer authorization failure, idle-round
classification, actual periodic protocol failure and whole-runtime stop ordering.
Session tests cover exact local entry custody under cancellation, partial
admission cleanup, endpoint overflow, original deadlines and sticky rollback.
These tests do not establish remote session teardown, network delivery or durable
restart continuation. Failed-start cleanup and retained command stop retries
remain required integration scope.

The required construction inventory includes `production_seed_rejection_precedes_profile_io_with_custom_real_crypto`. Typed entropy-origin syntax validation checks the exact private factory, capability signature and consumer receiver; decorative metadata, public seed fields, success/error generic substitution, raw seeds and an unchecked extra factory are rejected. Runtime rejection remains necessary evidence; syntax alone does not prove provider entropy quality.

### Nonproduction constructor entropy fidelity

The required secret-lifetime gate discovers and executes the actual Simulation
constructor stream-continuation and configured-provider regressions. Constructor
tests interleave effect dispatch with a clone of the constructed crypto subsystem
and compare against an independently seeded reference, including the receipt-key
draw performed during assembly. Configured crypto and random handlers must remain
the actual dispatch owners even when deterministic fallback entropy is admitted.
Owned Testing constructor coverage uses the original pre-IO build operation and
selected profile custody; primitive unowned fixtures do not establish that contract.

### Required committed Chat read faults

The VM lifecycle gate also discovers and executes the two exact matching Chat
fact corruption regressions. Each persists a matching malformed JSON or
unsupported-schema envelope through the actual required journal ingress, then
checks all three committed group/message/group-list readers. A matching corrupt
record must retain its native codec/schema cause rather than report absence or
an empty successful result. Test disposal awaits the actual scheduler under one
effect-backed resource window.

### Raw threshold route enforcement

`public-frost-signing` parses the raw threshold service and rejects quorum primitive calls on that surface, including calls hidden in renamed helpers. This syntax fence complements the actual finalized-enrollment native-source regression `runtime_bridge::error_boundary::actual_enrolled_identity_quorum_tests::actual_activated_threshold_identity_requires_quorum_owner_with_native_source`, which checks both valid threshold state and actual local backing loss. The fence does not prove the distributed owner implementation. Genuine repeated enrollment after threshold activation remains required integration coverage.

### Required reactive callback outcomes

Reactive callback fixtures must return explicit typed outcomes. The scheduler
regression `required_projection_failure_preserves_native_cause_and_blocks_batch_ack`
checks native failure provenance, skipped later projection work, unchanged
processing acknowledgment and absence of successful batch diagnostics. Issued
target failure propagation and domain codec failures require their own runtime
regressions; this batch-level test alone does not establish full readiness.

The required VM lifecycle inventory also discovers and executes the actual
fallible reactive batch, issued-target failure and all-five signal-view snapshot
fault regressions. A required projection failure retains its native cause and
cannot acknowledge processing progress for that batch or issued target.
An unregistered required signal snapshot must fail explicitly on each view.

The matching invitation and Chat projection codec regression feeds malformed
JSON and unsupported schemas through actual registered signal views and checks
that native causes are retained without advancing either projection revision.
Invitation projection additionally validates the payload context against its
journal wrapper. Codec failures are terminal outcomes, not display-only errors.

The core regressions
`required_fact_decoder_rejects_json_through_both_entry_points` and
`required_fact_decoder_retains_native_cbor_cause_through_both_entry_points`
feed declared JSON and malformed DAG-CBOR through both the envelope and encoded
fact APIs: JSON is rejected as non-canonical, and a DAG-CBOR failure keeps its
concrete serialization source.
Domain projections must use required decoders on matching fact types; optional
observational decoding does not establish processing success.

The required source inventory includes actual matching Invitation/Chat projection
codec faults and the core JSON decoder's native-source regression through both
validated envelope and byte entry points. These tests must be discovered and
executed in their actual package harnesses; corruption cannot become an empty
successful projection or erase its original codec category.

The projection codec regression also covers matching Contact, Friendship and
Recovery records through their domain-owned required decoders. Malformed JSON
and unsupported schemas must retain native causes and leave Contact and
Recovery source revisions unchanged, alongside Invitation and Chat revisions.

The required core timeout inventory executes
`required_timeout_drops_cancelled_query_before_reacquiring_observation_owner`
against the actual shared observation gate. Cancellation must destroy the
losing query future before the timeout owner reacquires that gate. This
regression proves cancellation custody; it does not establish bounded initial
clock queries or final service cleanup acknowledgement.

`required_signal_views_matching_domain_codec_faults_are_terminal` also covers all
nine moderation domains before Home materialization. Declared corruption and
unsupported schemas cannot be hidden behind absent Home state. The required
lifecycle inventory tracks this expanded name and each native cause.

Required Sync local session-custody tests inspect the actual retained session
records, including initializing allocations. Operational `total_sessions`
statistics count terminal outcomes and cannot prove allocation presence or
retirement. Exact owner Drop, partial admission and cancelled-future regressions
must establish removal of real original allocations and preservation of foreign
records; a diagnostic zero count is insufficient.

### Required absolute terminal observation coverage

`just _policy-check check absolute-time-observation` verifies exact source,
nonignored harness discovery and successful execution for the core terminal
acknowledgment tests, the real native provider, and `ManualPhysicalClock`.
The same inventory runs in `ci-vm-session-lifecycle`. Required cases include a
hung current read, observation-lock contention, delayed checkpoint with endpoint
priority, native timer failure without invented expiry, unsupported providers,
and publication under the original guard. Actual provider cases exercise fixed
endpoints after delayed registration, rollback, native failure custody and real
native timer wake observations. A focused pass does not prove the broader
runtime shutdown, command admission, browser execution or enrollment lifecycle.

The fixed-endpoint inventory also discovers and executes the exact physical-
deadline trait compile-fail doctest. It rejects receipt-generation coordinates;
positive provider tests simultaneously require the physical coordinate API.
The manual `TimeError` display and source matches are exhaustive, so new variants
require an explicit native-source decision at compile time.

Architecture reactive enforcement inspects actual direct generic fact commits
inside declared semantic owners. Those owners use required commit and
processing capabilities when their terminal contract requires projection
acknowledgment. Durable actor publication is a separate contract; invitation
acceptance need not wait for contact-link convergence. AST regressions reject
marker-word and unrelated-helper substitutes for completion while preserving
ordinary actor publication. Interprocedural ownership remains enforced by the
annotation ratchet and typed capability gates. The required policy-toolkit lane
executes these regressions before strict Clippy.

### Fresh terminal bootstrap history continuity

A fresh terminal account starts with a runtime-free `AppCore`. Its bootstrap
shell transition retains that exact app in process and attaches the first
runtime, preserving the original account creation terminal operation. The
shared semantic snapshot must continue to expose that operation after runtime
readiness; account files or a stale pending bootstrap record cannot manufacture
another success. The original global tracing writer remains owned across this
shell transition.

The app regression
`bootstrap_attachment_retains_original_terminal_history_and_rejects_replacement`
checks exact operation history and rejects an existing runtime replacement,
including after detachment. First-runtime attachment is irreversibly spent;
it runs in the required workspace test lane. Production shared settings parity
also exercises account creation, runtime readiness and the subsequent semantic
snapshot across the transition. Provisional enrollment and explicit authority
switching still use process reload; first-runtime attachment does not establish
physical provider transfer or runtime retirement.

### Projection observations and authoritative readiness

Browser and terminal shells describe observed canonical chat/contact snapshots
with the same pure `aura-app::ui_contract` builders. `ChatSignalUpdated` describes
the current selected (or deterministic default) channel and view counts. The
default orders equal names by canonical channel ID, and browser message lookup
keeps that selected ID rather than resolving again by display name. The
legacy `RemoteFactsPulled` name describes contact/discovery projection counts;
it does not attest to transport activity or successful anti-entropy. These
observations replace earlier counts rather than retaining stale count keys.
Shared parity still compares them. Business-flow waits must use the app-owned
membership, recipient, delivery, and operation facts; frontend projection loaders
cannot synthesize readiness from a channel row or a member list.

Frontend task shutdown must acknowledge destruction of admitted futures before
another in-process shell generation starts. The terminal closes task admission
and observes completion with its bounded cleanup helper after dropping fullscreen
hook futures. Cancellation flags alone cannot satisfy this boundary; drain
failure prevents reload. Required workspace tests cover escaped owned-spawner
work, rejected post-shutdown admission and never-polled future destruction before
completion acknowledgment. Bootstrap continuity coverage also reads the newly
attached runtime's semantic signal, rather than checking only the app store.

The native shell starts one effect-backed shutdown window when a bootstrap
handoff notification arrives, or after ordinary fullscreen termination. The
same original deadline bounds fullscreen exit, clearing harness submission
ownership, and acknowledged child destruction. Failed clock allocation or an
expired window cannot start a replacement deadline. Teardown preserves native
notification, clock, sender-clearing, fullscreen, and child-drain failures.
The controlled-clock `bootstrap_exit_and_task_drain_share_original_shutdown_endpoint`
regression verifies that four seconds spent exiting leaves one second to drain;
workspace unit CI runs this test alongside the actual shared parity scenarios.
The sender-clearing failure regression verifies that a later drain timeout
retains both native causes even when the bounded future is dropped.

## Runtime-free account creation ownership

The configured native staging adapter delegates actual pending-bootstrap and encrypted account-profile writes to the app-owned staging workflow. The original operation instance transfers before the first awaited producer step. Only acknowledged writes mint app-owned terminal success; frontend callbacks observe completion and request bootstrap reload. The retained AppCore supplies that original history to the new runtime signal graph. Runtime attachment rejects replacement, and pending-file reconciliation cannot manufacture account-create success.

Regression coverage must exercise actual storage production and observe the original operation after runtime attachment; manually seeded semantic facts do not establish producer ownership. The CreateAccount callback requires a workflow handoff owner, preventing submission with a local terminal owner.

### Account creation submission enforcement

Run `just ci-frontend-handoff-boundary` after changing account creation callbacks
or submission ownership. Its parsed Rust guard rejects local-terminal submission
with the actual `SemanticOperationKind::CreateAccount` argument. Preserve the
callback's workflow handoff type and the original operation instance before the
first awaited native staging step. The guard's positive/negative Syn fixtures
cover helper/direct calls, qualified paths, unrelated strings/types, and proven
lexical test scopes. These syntax checks supplement actual storage-producer
regressions; seeded operation history alone does not prove production lifecycle
publication or continuity after runtime attachment.

### Required native enrollment quorum evidence

After quorum signing-owner, prepared issuer, packet authentication, retained
initial Request or restricted completion observer changes, run the serialized
`just ci-vm-session-lifecycle`, ownership/annotation and security-boundary lanes.
Required fixtures must be nonignored, discovered by the actual selected harness,
and individually reported as successfully executed. A zero-match green Cargo
process or compile-fail import alone does not prove runtime custody.

The real confirmation fixture activates and reopens the original enrolled
physical profile before explicit approval on both actual native threshold devices.
It must retain all archive, immutable envelope, replay and corruption assertions,
then prove genuine next issuance with no local aggregation of remote shares.
Initial Request signature substitution, old intent replay, current package/epoch
mismatch, expired original window, actor cancellation and native drain failure
need adversarial execution evidence. Initial issuance is not later confirmation
quorum, profile restart continuity or complete service disposal evidence.

Public approval compile-fail guards cover construction, Clone and serde rejection.
The completion observer public opacity guard is distinct from native internal
regressions proving permit release, protected-clock continuity and the absence of
arbitrary execution/child/raw-clock authority. Keep exact required names in the
typed VM lifecycle inventory and retain its discovery/execution validator tests.

The required native
`real_committed_confirmation_is_durable_and_reverified_before_activation_capability`
fixture also checks actual repeated confirmed handoff, preservation of signed raw
share/configuration bytes, unchanged original activation envelope, refusal to
recreate a lost envelope after birth, and the native codec source for a corrupt
envelope. Fault injection uses the selected provider's test boundary and restores
only the captured original bytes before the existing profile reopen and actual
explicit two-runtime quorum issuance assertions. These cases do not establish
profile WAL or process restart continuity.

Confirmed wrapping evidence must also exercise the actual allocation manager:
original birth, read and positive sealing under the verified import receipt,
exact scope/reference substitution refusal, and interrupted publication refusing
a second allocation for an already born scope. An unchanged envelope alone does
not establish managed secret lifetime custody. Preserve native provider sources
and distinguish these assertions from legacy envelope encryption coverage.

### Authoritative fact policy test scopes

Frontend authoritative-fact restrictions inspect Rust paths and macro tokens.
Genuine lexical test scopes may observe the original published facts in native
producer regressions. Comments, string diagnostics, and preceding test modules
cannot exempt adjacent production. Mixed `cfg(any(test, feature = ...))` remains
production-capable and checked; the toolkit regression suite enforces this split.

Schema-one AMP membership projection regressions must cover both opaque fact
orders, split updates, duplicate replay, restart replay and metadata after leave.
The shared compact remove-wins reducer supplies both native and reactive views.
A foreign context must not hide a channel, and local departure cannot be undone
by an unversioned join or channel hint. Departed senders must fail projection
admission even with known-member or invitation enrichment. App readiness tests
must prove native participant counts defeat stale row/published count hints and
removed peers do not enable recipient or delivery readiness. Certified successor
membership requires actual verified inclusion evidence; transition ids and hashes
are insufficient.

### Ownership checks in clean checkouts

Service-surface governance scans required production roots even when ignored
local `work/` notes are absent. Existing scratch remains subject to exception
metadata checks. The toolkit regression exercises a clean fixture without
scratch, rejects an unowned production exception, and checks existing scratch
metadata. Do not create or publish work plans merely to satisfy a CI validator.

### Mock runtime physical time and required tasks

`MockRuntimeBridge` observes one manually controlled physical clock. Reads and
polling `sleep_ms` do not advance it. Drive timer expiration explicitly with
`advance_time_ms`; use `set_time_ms` to inject rollback and retain the resulting
native `TimeError`. Register notifications before rechecking the clock so a
concurrent advance cannot be lost. Entity and message uniqueness use their
separate sequence owners rather than changing physical time on observation.

Required refresh hooks must run through the mock's fallible task owner. Observe
named native failures and cancellation with `shutdown_owned_tasks`, which awaits
actual future destruction. Keep the native error/destruction, explicit timer
progress/rollback, and genuine listener-startup regressions in ordinary test
coverage. A timeout poll must not spend a listener's entire startup window before
that listener can acknowledge readiness.

# Aura Harness (Tooling)

## Purpose

Multi-instance orchestration harness for Aura runtime testing and operator workflows. Coordinates local PTY and SSH-backed instances, exposes a typed tool API, executes semantic scenarios against real frontends through a typed semantic command plane, and produces replay and artifact bundles.

## Scope

| Belongs here | Does not belong here |
|-------------|---------------------|
| Multi-instance orchestration and routing | Effect trait definitions or domain semantics |
| Typed semantic command-plane submission | Protocol safety rules |
| Replay bundle validation and deterministic seeds | Direct protocol mutation |
| Frontend-conformance coverage (quarantined) | Simulator-specific runner logic |
| Preflight capability and environment checks | Runtime-owned caches or coordinator state |
| Artifact bundles for CI and debugging | Parity-critical semantic lifecycle authorship |

## Dependencies

| Direction | Crate | What is consumed / produced |
|-----------|-------|-----------------------------|
| Consumes | `aura-app` | Shared scenario contract, semantic command types |
| Consumes | `aura-core` | Types, identifiers |
| Produces | — | Tool API responses, scenario reports, replay bundles, artifact bundles |

## Key Modules

- `config.rs` — Schema parsing for run config, semantic scenarios, and the narrow remaining compatibility-only executor fixtures.
- `compatibility_step.rs` — Internal compatibility IR retained only for synthetic phase-state-machine fixtures plus executor-local metadata/wait shaping; shared inventory scenarios are semantic-only, and the public `ScenarioConfig::compatibility_steps()` accessor is explicitly deprecated to discourage new usage.
- `coordinator.rs` — Multi-instance orchestration and per-instance command routing.
- `tool_api.rs` — Versioned request and typed response surface used by tests and automation.
- `executor.rs` — Semantic and compatibility scenario execution with deterministic budgets.
- `replay.rs` — Replay bundle validation, same-prebuilt-revision admission, and first typed response divergence.
- `preflight.rs` — Capability, binary, storage, port, and SSH baseline checks plus semantic-lane admission.
- `scripts/harness/lan/` — External process tooling: one kernel lifecycle owner serializes driver start/stop/finish and exact-token batch finalization. Launch identities bind the token, PID birth, executable and checkout; unknown ownership retains evidence without signalling. Batch interruption drains its owned command before independently finalizing both hosts. This infrastructure ownership never authors semantic business completion.
- `backend/` — Local PTY and SSH backend adapters.

## Invariants

- Complete browser enrollment uses an explicitly started loopback bootstrap
  broker. The harness accepts the broker URL and authorization token together,
  requires a loopback URL and a long hex token, and gives each browser instance
  a separate unpredictable invitation retrieval credential retained across its
  restart. Synchronous credential construction uses the effects-owned OS
  random handler and fails if entropy is unavailable. The browser driver places only the endpoint in the query and
  initializes credentials in origin session storage before page bootstrap;
  startup logs and the page URL omit both credentials. Missing provisioning leaves the production broker
  disabled, so no harness mode bypasses sender authorization.

- Harness IPC uses one selected short local runtime root, shared by run
  provisioning and coordinator admission. Run, instance/profile and session
  generation remain isolated independently of checkout depth. Explicit roots
  remain selected and fail with a concrete path error before socket binding if
  they cannot fit the platform limit. Namespace, instance and socket directories
  must be native owner directories; symlink/foreign-owner paths cannot redirect
  creation, chmod or cleanup. Private directories use `0700` and sockets `0600`.

- The LAN driver signals only a REPL whose retained birth, executable, config
  and checkout identity match the live process. Missing or mismatched identity
  and surviving checkout-owned harness IPC fail closed without discarding run
  evidence. An empty-run stop holds the persistent lifecycle lock and checks
  the same-checkout process inventory without creating a private request lease;
  stale launch identity or a live helper still refuses. TERM enters the REPL's
  existing owned shutdown; arbitrary child process signalling is forbidden.

- Paired enrollment transfers use the same ToolApi owner over its owner-only
  Unix socket. The LAN driver verifies the existing REPL process identity
  before forwarding one stdin request to that socket; the private response
  returns through the socket and never enters retained `repl.out`. General
  semantic evidence helpers remain unsuitable for secret-bearing responses.
  The private evidence helper writes owner-only files and whitelists operation
  id/instance/state, typed failure domain/code, host acknowledgment, and
  readiness without copying codes, labels, screen names, device-list IDs, or
  diagnostic detail. Browser runtime-event IDs use the canonical
  `runtime-event-<sequence>` string shape; numeric IDs are refused. If the
  whitelist rejects a private ToolApi response, the helper retains the raw
  response and any validator stderr only in `0600` artifacts inside the `0700`
  run directory alongside a separate redacted validation category. Those
  private artifacts never enter the
  shared checklist result, REPL output, URL or command arguments and requires
  protected handling during diagnosis. Reopen still requires a fresh
  authoritative observation after restart.
  The paired LAN bootstrap omits ordinary contact links so enrollment cannot
  inherit an established contact route. Its checklist transfers signed setup
  and issued code/manifest/verifier through private stdin requests, keeps the
  exact approval handle and host acknowledgment, and compares authority and
  device observations after same-profile restart. Read-only preflight records
  fixed commits, compared artifacts, planned host/process identity and the
  development-only broker before either owner starts; the result records
  actual process identity and receipts. Native restart retains the original
  operation through the private receipt because only browser restart accepts
  an expected operation instance. The pinned native manifest inspector reads
  the issuer's code on stdin and emits only the manifest/device selector;
  decoding does not verify or authorize it. After the explicit recipient
  import, the checklist requires the broker's authenticated exact-message
  ACK, which follows the original receiver's verified immutable retention,
  before accepting terminal success. The redacted result binds that ACK to
  distinct host/process identities and the signed-manifest selector.
  The single-host diagnostic reuses this private checklist with two native
  owners in clean same-commit checkouts and disjoint loopback profiles. Its
  preflight and result state `cross_host_asserted:false`; a loopback broker ACK
  and same-host reopen cannot satisfy the paired-host evidence requirement.

- Device enrollment issuance accepts an explicit user-transferred setup code.
  Frontends forward it to the app-owned verification and issuance workflow;
  raw authority IDs, demo autofill and discovery metadata cannot replace the
  setup verifier pin or infer that the new device accepted enrollment.
  `PrepareDeviceEnrollmentSetup` exports the exact signed code through the
  selected runtime's semantic queue after real account creation. It does not
  create authority staging files, derive physical device IDs, or mint user
  trust. Signing readiness is established by successful runtime export;
  unavailable or unready export fails explicitly. The coordinator rejects
  onboarding before export, and scenarios own the account-creation prerequisite.
  The browser driver's canonical immediate setup export is a runtime read: its
  page-owned queue result completes the command without demanding a later UI
  revision. Other semantic submissions retain the mutation revision floor.

- Config-first execution: invalid run or scenario configs fail before instance startup.
- Scenario list counts use `u32` on the wire; comparison with a local snapshot
  uses a checked index conversion, including the quarantined compatibility IR.
- Semantic wait lowering retains the exact typed expectation as private,
  nonserialized metadata. List membership and confirmation do not require
  selection; selection requires an explicit matching selection and listed item.
  Ordinary control visibility requires an authoritative visibility observation;
  focus alone cannot satisfy it. Unsupported visibility fails explicitly.
- Instance isolation: each action is scoped by `instance_id` with unique `data_dir`.
- Deterministic seeds: identical run config and seed produce identical seed bundles.
- API compatibility: negotiation selects the highest shared tool API version or fails closed.
- Serialized `submit_semantic_command` accepts a typed shared intent and derives
  its canonical contract once at the tool boundary. It returns the original
  submission receipt, operation-instance handle, value, and canonical contract.
  Required handles are checked against that contract; immediate account creation
  uses the new shell readiness/screen postcondition rather than its old handle.
  A refusal before operation admission is a typed `SemanticCommandRejection`
  with a stable failure domain/code and, when backed by a concrete native enum,
  a typed reason; it carries no accepted operation handle. The harness must not
  infer a rejection reason from diagnostic text or treat a refused submission
  as an admitted terminal operation.
  `instance_metadata` exposes the exact coordinator-owned profile path without
  opening or modifying it. Serialized
  `wait_for_ui_snapshot_event` preserves the pushed snapshot and backend version;
  an absent event is a bounded observation timeout, never semantic success.
- Monotonic event identifiers: event stream IDs strictly increase and preserve append-only ordering.
- Bounded execution: step and global scenario budgets cap execution time with diagnostic timeouts.
- Secure SSH defaults: strict host key checking stays enabled with enforced fingerprint policy.
- Primary-lane policy: default lane targets real Aura runtime and real TUI/web surfaces.
- Semantic-command-plane execution: shared scenarios submit typed semantic commands and await typed readiness/handle/quiescence/projection contracts.
- Shared scenarios can capture an observed operation instance before a later
  submission and require `operation_state_is.after_instance` to observe a
  different instance, so a setup result cannot satisfy the later operation.
- Where an actor has no prior operation, `assert_operation_absent` checks that
  actor's semantic snapshot before submission; a later terminal assertion then
  observes that actor's operation.
- Operation waits require the selected actor's observed ID/state and optional
  fresh-instance condition before an available matching submission handle is
  checked for exact instance/state. Browser approval activates the mounted
  `modal_confirm_button` through the semantic control contract.
  A mounted control may not return a submission handle to the harness, so an
  explicit captured-instance predicate binds its later success and pre-restart
  state to the same observed operation instance.
- Browser enrollment conformance requires the accepted-device runtime fact and
  two-device host projection before restarting the original receiver. The
  driver requires the captured instance to be `Succeeded`, then writes a
  bounded pre-restart artifact with that operation, typed failures, host
  added-device projection event, and safe console diagnostics before closing the old
  process. The browser backend carries that evidence into the new driver. A
  separate post-restart private artifact requires that event's device ID
  in the newly observed two-device list.
- Frontend-conformance isolation: renderer-specific mechanics are conformance-only and must not be the primary execution substrate for shared flows.
- Shared scenario inventory discipline: `scenarios/harness_inventory.toml` is semantic-only for shared flows. The remaining compatibility-step IR is justified only by `tests/phases/phase3_state_machine.rs` plus executor-internal metadata/wait shaping, and must not expand back into authoritative scenario coverage.
- AMP transition shared scenarios observe reducer-derived transition facts
  through `RuntimeEventKind::AmpChannelTransitionUpdated` in `UiSnapshot`; they
  must not use frontend-local scripted compatibility steps, raw labels, or DOM
  selectors to prove live-successor, conflict, quarantine, cryptoshred, or
  finalization state.
- Explicit lane capability contract: shared-semantic, raw-UI, and diagnostic-observation access are declared separately at preflight time; SSH is diagnostic-only until it implements the shared semantic contract.
- Backend enum routing must expose exact lifecycle, diagnostic, observation, raw-UI, and semantic operations. Do not reintroduce broad `as_*` capability-casting wrappers or generic unsupported-capability bailouts.
- The harness is an `Observed` plus orchestration crate. It may submit commands, wait on typed handles/readiness, and read projections, but it must not author semantic lifecycle truth.
- Shared semantic execution must not keep a duplicate lifecycle graph, phase cache, or heuristic identifier repair path for accounts, contacts, channels, or messaging state.
- Executor sequencing carries typed submission evidence into contract-declared `before_next_intent` waits; it must not rebuild a pending convergence graph or discharge post-operation progress locally.
- Parity-critical create/join/accept shared-channel flows must receive canonical operation handles and channel bindings from the authoritative submission/receipt path; post-hoc polling repair is forbidden.
- Local PTY shared-semantic submissions must treat harness command receipts as the only success witness at issue time. Visible homes, modal closure, message appearance, selected-list bindings, and `Submitting` are not semantic completion signals.
- Raw renderer capture is diagnostic-only and is exposed through explicitly named `diagnostic_*` observation surfaces; typed `UiSnapshot` / `UiSnapshotEvent` remain the only authoritative shared-semantic observation plane, and diagnostic query APIs keep the `diagnostic_*` naming through the tool boundary.
- Shared `UiSnapshot` carries `projection_source_revisions` for parity-critical observed entities. Cross-frontend projection comparisons use those revisions after quiescence; `UiSnapshot.revision` remains a frontend semantic/render publication sequence.
- In semantic scenarios, `message_contains` is authoritative `UiSnapshot.messages` observation only. Rendered-text assertions must use explicitly named conformance-only diagnostic actions such as `diagnostic_screen_contains`, and those actions are forbidden in shared-semantic scenarios.
- Projection-based semantic waits may resume across bounded browser/runtime restarts only by clearing stale freshness baselines and re-entering typed snapshot observation. Runtime-event, toast, and exact operation-state waits remain fail-closed across restarts.
- Time-bounded loops in shared semantic code are allowed only for infrastructure readiness, transport, or bounded observation waits whose owner is explicit; ownership transfer itself must not depend on settle windows or heuristic polling.
- The Playwright browser driver should prefer pushed UI/DOM observer waiters over fixed settle sleeps when deterministic publication state is already available.
- Harness-owned browser driver and owned web-server child processes must be reclaimed on both normal teardown and stale-run startup sweep; orphaned harness child processes must not survive to perturb later runs.
- The Playwright browser driver's `restart_page_session` path is infrastructure
  recovery only. Semantic command submission and runtime-identity staging must
  fail closed or wait on page-owned bootstrap/publication state rather than
  replaying semantic work through a restarted browser session.
- Do not add backwards-compatibility, migration, fallback, or legacy code paths for removed shared-semantic harness behavior. Delete obsolete paths instead.

### InvariantHarnessDeterministicReplayInputs

Harness replay depends on validated config, negotiated API version, and deterministic seed bundles.

Enforcement locus:
- `src/replay.rs` validates schema and tool API compatibility before replay.
- `src/determinism.rs` derives stable seeds from run configuration.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- `just test-crate aura-harness` and `just check-arch`

Contract alignment:
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines reproducibility assumptions for testing.
- [Project Structure](../../docs/999_project_structure.md#invariant-traceability) defines canonical naming.

### InvariantSharedFlowExecutionIsSemantic

Shared harness scenarios remain portable across TUI and web because they target the shared semantic contract rather than frontend mechanics.

Enforcement locus:
- `config.rs` parses the shared scenario contract from `aura-app`.
- `executor.rs` submits typed semantic commands, tracks handles/readiness, and waits on authoritative projection changes.
- `backend/` implements the shared semantic command-plane bridge per frontend.
- Policy checks reject raw mechanics in the core shared scenario set.

Failure mode:
- Scenarios become backend-specific and require per-frontend special-casing.
- Browser/TUI parity regresses and core scenarios stop being reusable.

Verification hooks:
- `bash scripts/check/shared-flow-policy.sh`
- `cargo test -p aura-harness --lib tui_semantic_actions_emit_expected_tool_requests`
- `cargo test -p aura-harness --lib browser_driver_maps_shared_controls_to_selectors`

### InvariantSharedSemanticLaneIsNotRendererDriven

The main shared-flow lane debugs production workflows through semantic command submission and authoritative projections rather than incidental frontend I/O.

Enforcement locus:
- Shared semantic backend traits expose submit/observe/projection surfaces.
- Frontend-conformance-only helpers are isolated from shared semantic execution.
- CI policy checks reject raw PTY/selector/text mechanics in shared-flow paths.

Failure mode:
- Harness failures become ambiguous mixes of product bugs and renderer I/O races.
- Shared matrix stability depends on focus, timing, or DOM/PTY structure.

Verification hooks:
- `bash scripts/check/shared-flow-policy.sh`
- `just ci-harness-matrix`

### InvariantObservationUsesAuthoritativeSemanticState

The harness observes semantic state first and uses DOM/text fallbacks only for debugging.

Enforcement locus:
- Browser backend consumes pushed `UiSnapshot` and `RenderHeartbeat` data.
- Tool API snapshot endpoints return structured shared-contract payloads.

Failure mode:
- Timeouts become ambiguous because the harness cannot distinguish semantic state drift from renderer drift.
- Observation reads mutate state or silently repair it, making failures non-deterministic.

Verification hooks:
- Playwright driver self-test
- Browser backend contract tests

## Ownership Model

Reference: [docs/122_ownership_model.md](../../docs/122_ownership_model.md)

For shared semantic flows, `aura-harness` uses `Observed` for typed projection reads, readiness waits, handle observation, and timeout diagnostics. Narrow `ActorOwned` orchestration is permitted for executor/coordinator processes that own multi-instance orchestration state. The harness must not author semantic lifecycle truth; it consumes typed move-owned handles/tokens but does not create or advance them outside approved command-plane surfaces.

### Inventory

| Path | Category | Authoritative owner | May mutate | Observe only |
|------|----------|---------------------|------------|--------------|
| Harness coordinator / multi-instance orchestration | `ActorOwned` | `HarnessCoordinator` and executor-owned orchestration state | coordinator/executor orchestration code | scenarios, CI, diagnostics |
| Shared semantic command submission results and handles | `Observed` over upstream `MoveOwned` handles | frontend/product command plane | backend adapters store handle references only | waits, diagnostics, replay |
| Readiness waits and projection reads | `Observed` | product readiness coordinators and authoritative facts | harness-local timeout/trace bookkeeping only | scenario authors, CI |
| Frontend-conformance raw mechanics | frontend-local only, never shared semantic ownership | frontend under test | conformance adapters only | shared semantic lane must not depend on them |

### Capability-Gated Points

- Shared semantic command-plane submission and handle propagation must consume the authoritative `aura-app` contract rather than inventing harness-local semantic ownership.
- Readiness waits and projection reads may track timeout/trace metadata locally, but may not mutate or repair product semantic truth.
- Frontend-conformance helpers are explicitly quarantined from the shared semantic lane and may not bypass typed command/observation surfaces.
- Weak identifiers may help diagnostics, but they may not be upgraded into authoritative channel/context bindings after handoff.
- Diagnostic screen and DOM captures may support failure attribution, but they may not satisfy shared-semantic success conditions or repair missing authoritative state.
- Remaining wall-clock bounds in shared semantic code must name their owner and infrastructure reason; they must not implement semantic migration or legacy fallback behavior.

### Verification Hooks

- `cargo check -p aura-harness`
- `cargo test -p aura-harness -- --nocapture`
- `just ci-actor-lifecycle`
- `just ci-move-semantics`
- `just ci-timeout-policy`
- `just ci-semantic-owner-awaits`
- `just ci-best-effort-side-effects`

## Contributor Guidance

- Treat `SharedSemanticBackend` plus `UiSnapshot` / `UiSnapshotEvent` as the primary shared-semantic contract. If you need raw screen or DOM data, the API and variable names must say `diagnostic`.
- In scenario files, use `message_contains` only for authoritative snapshot message content. Use `diagnostic_screen_contains` only in frontend-conformance scenarios when you intentionally need rendered shell text.
- When a parity-critical command result needs an operation handle, channel binding, or other owned token, require it in the immediate typed receipt. Do not add later polling, re-resolution, or inferred repair.
- If a convenience helper still needs exported data after submission, the follow-on wait must bind to an authoritative runtime event or projection contract, not a modal/screen side effect.
- Shared semantic executor/replay/backend core must decode typed `ToolPayload` and bridge structs directly. Keep `serde_json::Value` and `serde_json::from_value` quarantined to outer CLI/browser boundary adapters.
- Keep unsupported capability failures attached to the exact unsupported operation. Do not add back generic backend-cast helpers that hide which contract was actually requested.
- If a cleanup removes an old harness path, delete it. Do not preserve it behind compatibility branches, migration helpers, or fallback adapters.
- Do not add new inventory-backed or shared-flow dependents to `compatibility_step.rs`. If you touch that IR, keep the retention surface limited to `tests/phases/phase3_state_machine.rs` and executor-internal metadata/wait shaping, or delete it instead.
- The network lab has two backends, `mock` and native `patchbay` (Linux). The retired `patchbay-vm` mode must not come back as a shell-out to patchbay's sim runner; a lab backend launches real Aura instances inside its namespaces or it is not a backend.
- `ProvisionedLab` owns the lab for the whole run: provisioned in `main` before the coordinator exists, torn down by `finish` after the coordinator is dropped, so namespaces outlive every child. `InstanceNamespace::run_sync` is the only way a child enters a lab namespace (patchbay namespaces are anonymous fds). Packet captures started at provisioning are stopped by `finish` before the lab drops; a capture left running would keep its namespace alive.
- The backend timeline is built from patchbay's flushed `events.jsonl` plus harness receipts after teardown; anything collected while the lab is alive is provisional and marked so.
- A topology with `required_relay_authority` gives each lab instance its relay environment at provisioning (`relay_launch_env`): `AURA_RELAY_SERVE=1` for the relay authority, and for everyone else `AURA_RELAY_ADDR` built from the relay device's lab address and the relay's configured transport port. Nothing is hard-coded: both come from the provisioned topology, and the harness never relays frames itself.
- `ToolApi` owns the `ProvisionedLab` while scenarios run so the `network_*` environment actions can apply lab events; `main` takes it back after the coordinator is dropped and finishes it. Network event scenario steps resolve actors to lab authorities through the coordinator, never by device name.
- Matrix-run temp cleanup stays the default, but diagnostics may retain the per-run temp and transient roots by setting `AURA_HARNESS_KEEP_TMP=1`; this changes artifact retention only and must not change scenario semantics.

## Testing

### Strategy

Deterministic replay, semantic flow execution, and tool API correctness are the primary concerns. Replay now compares typed tool-response meaning rather than only `Ok`/`Error` shape. Tests are organized into `tests/phases/` for the phased harness evolution (phase 1 through 5), `tests/holepunch/` for NAT traversal tiers, and top-level contract tests.

### Commands

```
cargo test -p aura-harness
```

### Coverage matrix

| What breaks if wrong | Invariant | Test location | Status |
|---------------------|-----------|--------------|--------|
| LAN automation loses operation identity, resets its deadline, or treats a timeout as success | ObservationUsesAuthoritativeSemanticState | `src/tool_api.rs` tests; `scripts/harness/lan/test-semantic.sh` | Covered |
| Tool API maps wrong operation | DeterministicReplayInputs | `tests/phases/phase1_tool_api.rs` | Covered |
| State machine invalid transition | SharedFlowExecutionIsSemantic | `tests/phases/phase3_state_machine.rs` | Covered |
| Reliability under failure | — | `tests/phases/phase4_reliability.rs` | Covered |
| API negotiation wrong | — | `tests/phases/phase5_api_negotiation.rs` | Covered |
| Routing replay diverges | DeterministicReplayInputs | `tests/phases/phase2_routing_replay.rs` | Covered |
| Phase 1 regression re-introduced | — | `tests/phases/phase1_regression.rs` | Covered |
| Phase 2 regression re-introduced | — | `tests/phases/phase2_regression.rs` | Covered |
| Phase 5 regression re-introduced | — | `tests/phases/phase5_regression.rs` | Covered |
| Holepunch E2E patchbay broken | — | `tests/holepunch/holepunch_e2e_runtime_patchbay.rs` | Covered |
| Holepunch tier 2 broken | — | `tests/holepunch/holepunch_tier2_patchbay.rs` | Covered |
| Holepunch stress fails | — | `tests/holepunch/holepunch_tier3_stress.rs` | Covered |
| Link condition lands on the wrong node | — | `tests/holepunch/holepunch_tier2_patchbay.rs` (`link_conditions_land_on_the_node_that_owns_the_link`), `network_lab::tests` | Covered |
| Lab timeline misses provisioning or teardown | — | `tests/holepunch/holepunch_tier2_patchbay.rs` (`finished_lab_timeline_covers_provisioning_through_teardown`) | Covered |
| Restricted-cone NAT or stress presets fail to provision | — | `tests/holepunch/holepunch_tier2_patchbay.rs`, `topology_adapter::impair_value_tests` | Covered |
| Local loopback contract fails | ObservationUsesAuthoritativeSemanticState | `tests/contract_local_loopback.rs` | Covered |
| Contract suite fails | SharedFlowExecutionIsSemantic | `tests/contract_suite.rs` | Covered |

## References

- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md)
- [Ownership Model](../../docs/122_ownership_model.md)
- [Testing Guide](../../docs/804_testing_guide.md)
- [Project Structure](../../docs/999_project_structure.md)

## Enrollment trust transfer boundary

Device enrollment import carries three explicit actor-supplied values. Enrollment readiness capture reads actual manifest/verifier payloads from the matched runtime event and stores companion variables; it fails when those values are absent. Clipboard fallback, payload-derived initiator pins, and independently invented devices cannot authorize enrollment.

See [cryptography](../../docs/100_crypto.md), [operation ownership](../../docs/109_operation_categories.md), [shared user flows](../../docs/121_user_flow_harness.md), and [testing](../../docs/804_testing_guide.md).

Ordinary control visibility requires an actual rendered-control observation
with the current semantic source and Visible state. Focus cannot satisfy it.
Observations retain duplicate control instances. Missing producer coverage stays
unsupported; onboarding is the first native/web producer, not a full inventory.

Modal focus observation retains the exact canonical `ControlId::Field(FieldId)`
from the active modal field. A generic modal-input identity cannot replace that
field and focus does not prove rendered visibility. The unused invitation-type
list/field and generic modal-input wire variants are removed with their raw-key
selector/fallback paths; no compatibility decoder remains. Actual native and
shared UI snapshot regressions cover all four contact-invitation fields. These
are local observation checks, not paired-client business completion evidence.

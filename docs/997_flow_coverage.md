# User Flow Coverage Report

This document tracks end-to-end user coverage for Aura's runtime harness scenarios across TUI and web surfaces.

## Coverage Boundary Statement

User flow coverage validates user-visible behavior and interaction wiring through runtime harness scenarios.
It does not replace protocol conformance, theorem proofs, or differential parity lanes.
Use this report for UI/product flow traceability and regression targeting.

The harness coverage model has two explicit lanes:

- shared semantic lane:
  - parity-critical shared flows execute through the shared semantic command plane
  - this is the primary lane for debugging production workflows
- frontend-conformance lane:
  - renderer-specific control wiring, DOM/PTy mechanics, and shell integration
    are validated separately
  - these scenarios are intentionally not the primary shared-flow substrate

## Summary Metrics

| Metric | Count |
|--------|-------|
| Harness User Flow Scenarios | 16 |
| Shared Semantic Scenarios | 11 |
| Mixed-Runtime Scenarios (TUI + Web distinct keys) | 3 |
| Frontend-Conformance Scenarios | 5 |
| Core User Flow Domains | 13 |

## Coverage Classes

Aura tracks three different coverage classes in this document:

| Class | Meaning | Main Artifact |
|-------|---------|---------------|
| Parity-critical shared flow | One semantic flow that must remain portable across TUI and web and execute through the shared semantic command plane | `aura-app::ui_contract` + canonical harness scenarios |
| Mixed-runtime interoperability | User-visible flow that intentionally spans different frontend/runtime combinations | Canonical mixed-runtime scenarios |
| Frontend-specific or auxiliary coverage | Focused smoke, modal, renderer-specific, or conformance-only coverage that is useful but not the parity contract | Supplementary scenarios |

This report is a traceability document for those classes. It is not a proof of protocol correctness, and it does not replace conformance or verification lanes.

## Canonical UX Scenario Set

| Scenario | File | Primary Flow |
|----------|------|--------------|
| Startup Smoke | `scenarios/harness/real-runtime-mixed-startup-smoke.toml` | Shared runtime startup and onboarding readiness |
| TUI Global Navigation/Help Hotkeys | `scenarios/harness/tui-conformance-global-navigation-help-hotkeys.toml` | TUI frontend-conformance: global navigation, key mappings, and help modal behavior |
| TUI Neighborhood Keypaths/Detail | `scenarios/harness/tui-conformance-neighborhood-keypaths-and-detail.toml` | TUI frontend-conformance: neighborhood keypaths, rendered map/detail text, and toast wiring |
| Scenario 12 | `scenarios/harness/scenario12-mixed-device-enrollment-removal-e2e.toml` | Mixed TUI/Web device enrollment + removal |
| Scenario 13 | `scenarios/harness/scenario13-mixed-contact-channel-message-e2e.toml` | Shared contact invite + channel messaging |
| Contact Invite Notification Roundtrip | `scenarios/harness/mixed-contact-invite-notification-roundtrip.toml` | Mixed TUI/Web symmetric contact invite acceptance notifications |
| Shared Settings | `scenarios/harness/shared-settings-parity.toml` | Shared semantic settings parity |
| Shared Notifications/Authority | `scenarios/harness/shared-notifications-and-authority.toml` | Shared semantic notifications navigation and authority-switch handling |
| AMP Normal Transition | `scenarios/harness/amp-transition-normal-shared.toml` | Shared AMP observed to A2 live to A3 finalized transition observation |
| AMP Delayed Witness Transition | `scenarios/harness/amp-transition-delayed-witness-shared.toml` | Shared AMP delayed/offline witness pending and convergence observation |
| AMP Conflict/Subtractive Transition | `scenarios/harness/amp-transition-conflict-subtractive-shared.toml` | Shared AMP conflicting A2 certificate and subtractive membership observation |
| AMP Emergency Transition | `scenarios/harness/amp-transition-emergency-shared.toml` | Shared AMP emergency quarantine, cryptoshred, and governance non-removal observation |
| AMP Negative Transition | `scenarios/harness/amp-transition-negative-shared.toml` | Shared AMP rejected emergency, cooldown duplicate evidence, and recovery replay observation |
| Browser Observation | `scenarios/harness/semantic-observation-browser-smoke.toml` | Browser semantic observation contract smoke |
| TUI Observation | `scenarios/harness/semantic-observation-tui-smoke.toml` | TUI semantic observation contract smoke |
| Quint Observation | `scenarios/harness/quint-semantic-observation-smoke.toml` | Quint-origin semantic observation reference |

The two TUI-only conformance scenarios plus Scenario 13 are retained as frontend-conformance coverage. All harness scenarios in this inventory now use the semantic scenario format.

## User Flow Matrix

| Flow Domain | Main Coverage | Secondary Coverage | Runtime Context |
|------------|----------------|--------------------|-----------------|
| Startup and onboarding readiness | `real-runtime-mixed-startup-smoke.toml` | `quint-semantic-observation-smoke.toml` | TUI + Web |
| Navigate neighborhood | `real-runtime-mixed-startup-smoke.toml` | TUI Neighborhood Keypaths/Detail | TUI + Web |
| Navigate chat | Scenario 13 | `semantic-observation-browser-smoke.toml`, `semantic-observation-tui-smoke.toml` | TUI + Web |
| Navigate contacts | Scenario 13 | `mixed-contact-invite-notification-roundtrip.toml`, `semantic-observation-browser-smoke.toml`, `semantic-observation-tui-smoke.toml` | TUI + Web |
| Send friend request | Scenario 13 | `semantic-observation-browser-smoke.toml`, `semantic-observation-tui-smoke.toml` | TUI + Web |
| Accept inbound friend request | Scenario 13 | `semantic-observation-browser-smoke.toml`, `semantic-observation-tui-smoke.toml` | TUI + Web |
| Decline inbound friend request | Scenario 13 | `semantic-observation-browser-smoke.toml`, `semantic-observation-tui-smoke.toml` | TUI + Web |
| Remove friend / revoke outbound friendship | Scenario 13 | `semantic-observation-browser-smoke.toml`, `semantic-observation-tui-smoke.toml` | TUI + Web |
| Navigate notifications | `shared-notifications-and-authority.toml` | `mixed-contact-invite-notification-roundtrip.toml`, `semantic-observation-browser-smoke.toml`, `semantic-observation-tui-smoke.toml` | TUI + Web |
| Navigate settings | `shared-settings-parity.toml` | `shared-notifications-and-authority.toml`, `semantic-observation-browser-smoke.toml`, `semantic-observation-tui-smoke.toml`, `quint-semantic-observation-smoke.toml` | TUI + Web |
| Create invitation | Scenario 13 | `mixed-contact-invite-notification-roundtrip.toml`, `semantic-observation-browser-smoke.toml`, `semantic-observation-tui-smoke.toml` | TUI + Web |
| Accept invitation | Scenario 13 | `mixed-contact-invite-notification-roundtrip.toml`, `semantic-observation-browser-smoke.toml`, `semantic-observation-tui-smoke.toml` | TUI + Web |
| Create home | Scenario 13 | `semantic-observation-browser-smoke.toml`, `semantic-observation-tui-smoke.toml` | TUI + Web |
| Join channel | Scenario 13 | `semantic-observation-browser-smoke.toml`, `semantic-observation-tui-smoke.toml` | TUI + Web |
| Send chat message | Scenario 13 | `semantic-observation-browser-smoke.toml`, `semantic-observation-tui-smoke.toml` | TUI + Web |
| Add device | Scenario 12 | `shared-settings-parity.toml` | Mixed runtime |
| Remove device | Scenario 12 | `shared-settings-parity.toml` | Mixed runtime |
| Switch authority | `shared-notifications-and-authority.toml` | `shared-settings-parity.toml` | TUI + Web |
| Global navigation/help | TUI Global Navigation/Help Hotkeys | None | TUI frontend-conformance |
| Neighborhood keypath navigation | TUI Neighborhood Keypaths/Detail | `real-runtime-mixed-startup-smoke.toml` | TUI frontend-conformance + shared startup |
| Semantic observation contract | `semantic-observation-browser-smoke.toml` | `semantic-observation-tui-smoke.toml`, `quint-semantic-observation-smoke.toml` | Browser + TUI |
| AMP transition frontend observation | `amp-transition-normal-shared.toml`, `amp-transition-delayed-witness-shared.toml`, `amp-transition-conflict-subtractive-shared.toml`, `amp-transition-emergency-shared.toml`, `amp-transition-negative-shared.toml` | Runtime-event parity contract tests; simulator transition scenarios from Phase 9 | TUI + Web observation contract |

Current parity-critical source changes touched the following shared-flow areas
and continue to map to the same canonical coverage anchors:

- Shared flow and scenario contract metadata now live behind facade roots in
  `aura-app::ui_contract` and `aura-app::scenario_contract`, with dedicated
  module families for parity metadata, harness metadata, shared-flow support,
  action contracts, expectations, submission, and value types. The canonical
  public contract and the coverage anchors below do not change with that
  internal split.
- Notifications navigation remains anchored by
  `shared-notifications-and-authority.toml`, with
  `semantic-observation-browser-smoke.toml` and
  `semantic-observation-tui-smoke.toml` as secondary observation coverage.
  The current notifications-screen change is limited to neutral empty-state
  copy and detail text, and the coverage expectation remains that
  notifications navigation exercises shared semantic navigation only rather than
  frontend-specific invitation or recovery actions.
- The TUI semantic-observation contract keeps the same canonical anchor in
  `semantic-observation-tui-smoke.toml`, but the native harness ingress now
  requires explicit harness mode, the per-run `AURA_HARNESS_RUN_TOKEN`, and
  transient-root-scoped `AURA_TUI_COMMAND_SOCKET`,
  `AURA_TUI_UI_STATE_SOCKET`, and `AURA_TUI_UI_STATE_FILE` values. That change
  hardens the existing observation scenario; it does not introduce a new
  shared-flow anchor or a production-only harness surface.
- Neighborhood navigation stays anchored by
  `real-runtime-mixed-startup-smoke.toml`
- Chat/contact navigation, the contact-to-friend lifecycle, invitation, home
  creation, channel join, and message-send flows stay anchored by
  `scenario13-mixed-contact-channel-message-e2e.toml`
- Contacts navigation, invitation creation, and invitation acceptance remain
  mapped to Scenario 13 plus the semantic observation smoke scenarios. The
  current terminal-side change only removes stale modal-local ownership
  assumptions and keeps those flows on the same typed dispatch and shared
  workflow path.
- Mixed contact-invite acceptance notifications now also have a dedicated
  symmetric mixed-runtime anchor in
  `mixed-contact-invite-notification-roundtrip.toml`. That scenario exercises
  TUI-create/Web-accept and Web-create/TUI-accept flows and verifies the
  creator-visible acceptance notification on the notifications screen without
  replacing Scenario 13 as the canonical contacts/chat lifecycle anchor.
- Pending channel-invitation acceptance now also requires terminal-status
  wrappers and `*_with_instance` entry points to publish a terminal failure for
  the same operation instance if a browser/TUI shared-flow error escapes before
  the owned accept path settles. That keeps Scenario 13 authoritative for
  contacts navigation, invitation create/accept, channel join, and
  shared-channel receive parity rather than leaving the lifecycle stranded at
  `SemanticOperationPhase::WorkflowDispatched`.
- `aura-app` splits these same flows across more specific
  owner modules while preserving the coverage anchors above:
  `workflows/context/neighborhood.rs`,
  `workflows/invitation/{create,accept,readiness}.rs`, and
  `workflows/messaging/{channel_refs,channels,send}.rs`. Shared-flow source
  metadata continues to publish through the `aura-app::ui_contract` facade.
- Settings/device and notifications/authority work keep the same canonical
  anchors. Device add/remove and shared settings remain bound to
  `scenario12-mixed-device-enrollment-removal-e2e.toml` plus
  `shared-settings-parity.toml`, while notifications navigation and authority
  switching remain bound to `shared-notifications-and-authority.toml`.
- Scenario 12 now also carries the browser-specific removable-device parity
  rule: the shared semantic snapshot exports current-device markers without
  fabricating a selected device row, so mixed browser runs must resolve
  `remove_selected_device` from the authoritative removable device in settings
  state when no explicit `ListId::Devices` selection exists.
- Scenario 13 now also carries the mixed-runtime sealed-message receive rule:
  the current TUI/browser shared-channel receive path may converge on sealed
  authoritative placeholders, so the canonical receive assertions for the
  mixed-runtime anchor match the `[sealed:` prefix instead of renderer-local
  plaintext recovery.
- AMP channel transition frontend observation is covered by shared semantic AMP
  transition scenarios plus runtime-event parity contract tests. TUI and web
  consume `RuntimeFact::AmpChannelTransitionUpdated` through
  `UiSnapshot.runtime_events` and shared `aura-app::ui_contract`
  action/control ids; the shared scenarios drive typed AMP transition fixtures
  through the semantic command plane and assert
  `RuntimeEventKind::AmpChannelTransitionUpdated` without frontend-specific
  compatibility steps.

Scenario 13 remains the canonical anchor for the shared contacts lifecycle
because it exercises the parity-critical semantic controls for `send friend
request`, `accept friend request`, `decline friend request`, and `remove
friend` while preserving the runtime-projected relationship states `contact`,
`pending_outbound`, `pending_inbound`, and `friend` across both TUI and web.

## Frontend-Conformance Coverage

These scenarios are maintained outside the main shared semantic lane:

| Scenario File | Focus |
|---------------|-------|
| `tui-conformance-global-navigation-help-hotkeys.toml` | TUI hotkeys, global navigation, help modal wiring |
| `tui-conformance-neighborhood-keypaths-and-detail.toml` | TUI neighborhood keypaths, rendered map/detail text, toast wiring |
| `semantic-observation-browser-smoke.toml` | Browser observation contract smoke |
| `semantic-observation-tui-smoke.toml` | TUI observation contract smoke |
| `quint-semantic-observation-smoke.toml` | Reference semantic observation smoke |

## Planned Release And Update Validation Matrix

This matrix is the planned coverage target for harness-driven validation of module and OTA distribution/update behavior.

The intended rollout order is:

1. mechanism validation matrix
2. candidate-release rehearsal matrix
3. live-release promotion gates

Mechanism validation proves the lifecycle machinery itself under synthetic releases and controlled failures. Candidate-release rehearsal proves that one specific release works before promotion or real cutover.

| Order | Domain | Mode | Target | Example Coverage Goal | Status |
|------|--------|------|--------|------------------------|--------|
| 1 | OTA | Mechanism validation | Native host shell + runtime payload | Synthetic release publication, staging, bootloader handoff, health confirmation, rollback | Planned |
| 2 | OTA | Mechanism validation | Browser-extension host shell + runtime payload | Synthetic extension-target OTA staging, compatibility block, handoff, recovery | Planned |
| 3 | OTA | Mechanism validation | Mobile host shell + runtime payload | Synthetic mobile-target staging, blocked activation when host shell is too old | Planned |
| 4 | Module | Mechanism validation | Generic module lifecycle | Synthetic module discovery, verification, staging, admission, cutover, rollback | Planned |
| 5 | Module | Mechanism validation | Cross-host artifact availability | Non-executing hosts serve artifacts to execution-compatible hosts | Planned |
| 6 | OTA | Candidate-release rehearsal | Specific Aura runtime release | Candidate runtime release staged and rehearsed before promotion/cutover | Planned |
| 7 | Module | Candidate-release rehearsal | Specific module release | Candidate module release staged and rehearsed before promotion/cutover | Planned |
| 8 | Example module | Candidate-release rehearsal | Browser-extension host | Candidate Example module release exercised end to end before promotion | Planned |
| 9 | OTA | Promotion gate | Release operation | Real cutover remains blocked until rehearsal passes | Planned |
| 10 | Module | Promotion gate | Release operation | Real publication/activation remains blocked until rehearsal passes | Planned |

This matrix should remain typed-lifecycle driven. It should not be satisfied by log scraping or ad hoc manual release notes.

### Typed Release Validation Contract

Each planned release/update row above must map to a typed harness contract before it is counted as implemented.

| Coverage Entry | Typed command/control surface | Typed lifecycle evidence | Primary lane |
|---------------|-------------------------------|--------------------------|--------------|
| OTA mechanism validation | `PublishSyntheticOtaRelease`, `StageOtaCandidate`, `TriggerBootloaderHandoff`, `ConfirmCandidateHealth`, `RollbackOtaCandidate` | `OtaReleasePublished`, `OtaArtifactAvailable`, `OtaStaged`, `OtaCompatibilityBlocked`, `OtaCandidateLaunched`, `OtaHealthConfirmed`, `OtaRolledBack` | Shared semantic lane |
| OTA candidate-release rehearsal | `PublishCandidateOtaRelease`, `StageOtaCandidate`, `ApproveOtaCutover`, `ConfirmCandidateHealth`, `RollbackOtaCandidate` | `OtaCandidatePublished`, `OtaStaged`, `OtaPromotionStateChanged`, `OtaCandidateLaunched`, `OtaHealthConfirmed`, `OtaRehearsalPassed` | Shared semantic lane |
| Module mechanism validation | `PublishSyntheticModuleRelease`, `StageModuleCandidate`, `PrepareModuleAdmission`, `CommitModuleCutover`, `RollbackModuleCutover` | `ModuleReleasePublished`, `ModuleArtifactAvailable`, `ModuleVerified`, `ModuleStaged`, `ModuleAdmissionPrepared`, `ModuleCutoverCommitted`, `ModuleRolledBack` | Shared semantic lane |
| Module candidate-release rehearsal | `PublishCandidateModuleRelease`, `StageModuleCandidate`, `ApproveModuleCutover`, `CommitModuleCutover`, `RollbackModuleCutover` | `ModuleCandidatePublished`, `ModuleStaged`, `ModulePromotionStateChanged`, `ModuleCutoverCommitted`, `ModuleHealthConfirmed`, `ModuleRehearsalPassed` | Shared semantic lane |

These rows are intentionally semantic-lane requirements. Frontend-conformance coverage may validate renderer wiring for release screens or controls, but it does not satisfy release/update lifecycle coverage on its own.

## Coverage Expectations

### Shared Flow Contract Expectations

Every parity-critical shared flow should have, in code and metadata:

- a canonical shared flow identifier in `aura-app::ui_contract`
- a typed semantic command path on both TUI and web
- semantic action contracts with preconditions and terminal success/failure conditions
- an authoritative readiness, event, or quiescence owner for waits
- any parity exception recorded as typed metadata in `aura-app::ui_contract`
  with a reason code, scope, affected surface, and authoritative doc reference
- at least one canonical scenario reference in this report

Shared-flow scenarios must not rely on raw PTY keys, raw selector clicks, raw label matching, or incidental focus-stepping as their primary mechanics. Those behaviors belong in frontend-conformance coverage instead.

Frontend-specific flows may still have scenario coverage, but they are not part of the portability contract unless explicitly promoted into the shared contract.

### PR Gate Expectations

1. Changes to global navigation, settings, chat, contacts, neighborhood, or ceremonies should have at least one impacted canonical scenario updated or re-validated.
2. Changes that affect both TUI and web behavior should be validated against parity-critical scenarios in the shared semantic lane on both runtimes.
3. Changes to mixed-instance behavior should include scenario 12 and/or 13 coverage.
4. Contacts-surface changes that alter relationship state or action availability must preserve the shared semantic lifecycle for `contact`, `pending_outbound`, `pending_inbound`, and `friend`, and they must stay anchored to Scenario 13 rather than shell-specific smoke coverage.
5. Mixed-runtime code exchange and chat routing changes should preserve the
   event-driven contract used by scenarios 12 and 13: invitation/device codes
   come from typed runtime-event payloads, and chat assertions bind to the
   selected shared channel rather than frontend-specific ordering.
6. Browser shared-flow bridge changes should preserve the explicit runtime
   identity staging handoff and the page-owned semantic command queue used by
   the Playwright lane. Coverage remains anchored in the shared semantic
   scenarios rather than DOM-driving fallback mechanics.
7. Changes to renderer-specific control wiring should add or update
   frontend-conformance coverage rather than weakening the shared semantic lane.
8. Changes to OTA or module release/update architecture should update the
   planned release/update matrix above when they add, remove, or reorder
   mechanism-validation or release-rehearsal coverage.

### CI Enforcement

Fast CI currently uses two separate gates:

- `just ci-user-flow-coverage` enforces traceability heuristics between changed user flow-facing source files, canonical scenarios, and this report
- `AURA_ALLOW_FLOW_COVERAGE_SKIP=1` is a local-only escape hatch. CI rejects it.
- `just ci-user-flow-policy` enforces documentation and contributor-guidance updates for shared user flow contract and determinism surfaces via Aura's `toolkit/xtask` user-flow guidance sync check
- OTA and module release/update validation rows in this report are part of that same user-flow guidance surface and must be kept in sync as the release matrix evolves
- The release/update rows are expected to land in staged order: mechanism validation first, candidate rehearsal second, and promotion-gate coverage last
- `just ci-harness-matrix-inventory` enforces that scenario classification drives the TUI/web matrix lanes
- shared semantic scenarios and frontend-conformance scenarios are expected to
  remain distinct classifications. CI policy should reject shared-flow drift
  back to renderer-driven mechanics.

Current limitation:

- `ci-user-flow-coverage` still infers some ownership from filenames and does not yet prove that the correct scenario set changed
- docs updates and coverage traceability are distinct concerns. This report should not claim stronger behavioral enforcement than CI actually provides.

### Residual Risk Areas

| Area | Current Risk | Mitigation Direction |
|------|--------------|----------------------|
| Long-tail modal sequencing | Medium | Add focused scenario fragments for rare wizard branch paths |
| Toast timing/race windows | Medium | Prefer persistent-state assertions over toast-only checks |
| Cross-topology regressions | Medium | Keep mixed-topology smoke scenarios in scheduled lanes |

## References

- [Testing Guide](804_testing_guide.md)
- `toolkit/xtask` user-flow guidance sync check
- [Simulation Guide](805_simulation_guide.md)
- [Verification Coverage Report](998_verification_coverage.md)
- [Project Structure](999_project_structure.md)

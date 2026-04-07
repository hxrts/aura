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
| Harness User Flow Scenarios | 10 |
| Shared Semantic Scenarios | 5 |
| Mixed-Runtime Scenarios (TUI + Web distinct keys) | 2 |
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
| Scenario 13 | `scenarios/harness/scenario13-mixed-contact-channel-message-e2e.toml` | TUI-stable contact invite + channel messaging (browser shared receive parity pending) |
| Shared Settings | `scenarios/harness/shared-settings-parity.toml` | Shared semantic settings parity |
| Shared Notifications/Authority | `scenarios/harness/shared-notifications-and-authority.toml` | Shared semantic notifications navigation and authority-switch handling |
| Browser Observation | `scenarios/harness/semantic-observation-browser-smoke.toml` | Browser semantic observation contract smoke |
| TUI Observation | `scenarios/harness/semantic-observation-tui-smoke.toml` | TUI semantic observation contract smoke |
| Quint Observation | `scenarios/harness/quint-semantic-observation-smoke.toml` | Quint-origin semantic observation reference |

The two TUI-only conformance scenarios are retained as frontend-conformance coverage. All harness scenarios in this inventory now use the semantic scenario format.

## User Flow Matrix

| Flow Domain | Main Coverage | Secondary Coverage | Runtime Context |
|------------|----------------|--------------------|-----------------|
| Startup and onboarding readiness | `real-runtime-mixed-startup-smoke.toml` | `quint-semantic-observation-smoke.toml` | TUI + Web |
| Navigate neighborhood | `real-runtime-mixed-startup-smoke.toml` | TUI Neighborhood Keypaths/Detail | TUI + Web |
| Navigate chat | Scenario 13 | `semantic-observation-browser-smoke.toml`, `semantic-observation-tui-smoke.toml` | TUI + Web |
| Navigate contacts | Scenario 13 | `semantic-observation-browser-smoke.toml`, `semantic-observation-tui-smoke.toml` | TUI + Web |
| Send friend request | Scenario 13 | `semantic-observation-browser-smoke.toml`, `semantic-observation-tui-smoke.toml` | TUI + Web |
| Accept inbound friend request | Scenario 13 | `semantic-observation-browser-smoke.toml`, `semantic-observation-tui-smoke.toml` | TUI + Web |
| Decline inbound friend request | Scenario 13 | `semantic-observation-browser-smoke.toml`, `semantic-observation-tui-smoke.toml` | TUI + Web |
| Remove friend / revoke outbound friendship | Scenario 13 | `semantic-observation-browser-smoke.toml`, `semantic-observation-tui-smoke.toml` | TUI + Web |
| Navigate notifications | `shared-notifications-and-authority.toml` | `semantic-observation-browser-smoke.toml`, `semantic-observation-tui-smoke.toml` | TUI + Web |
| Navigate settings | `shared-settings-parity.toml` | `shared-notifications-and-authority.toml`, `semantic-observation-browser-smoke.toml`, `semantic-observation-tui-smoke.toml`, `quint-semantic-observation-smoke.toml` | TUI + Web |
| Create invitation | Scenario 13 | `semantic-observation-browser-smoke.toml`, `semantic-observation-tui-smoke.toml` | TUI + Web |
| Accept invitation | Scenario 13 | `semantic-observation-browser-smoke.toml`, `semantic-observation-tui-smoke.toml` | TUI + Web |
| Create home | Scenario 13 | `semantic-observation-browser-smoke.toml`, `semantic-observation-tui-smoke.toml` | TUI + Web |
| Join channel | Scenario 13 | `semantic-observation-browser-smoke.toml`, `semantic-observation-tui-smoke.toml` | TUI + Web |
| Send chat message | Scenario 13 | `semantic-observation-browser-smoke.toml`, `semantic-observation-tui-smoke.toml` | TUI + Web |
| Add device | Scenario 12 | `shared-settings-parity.toml` | Mixed runtime |
| Remove device | Scenario 12 | `shared-settings-parity.toml` | Mixed runtime |
| Switch authority | `shared-notifications-and-authority.toml` | `shared-settings-parity.toml` | TUI + Web |
| Global navigation/help | TUI Global Navigation/Help Hotkeys | None | TUI frontend-conformance |
| Neighborhood keypath navigation | TUI Neighborhood Keypaths/Detail | `real-runtime-mixed-startup-smoke.toml` | TUI frontend-conformance + shared startup |
| Semantic observation contract | `semantic-observation-browser-smoke.toml` | `semantic-observation-tui-smoke.toml`, `quint-semantic-observation-smoke.toml` | Browser + TUI |

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
- `aura-app` splits these same flows across more specific
  owner modules while preserving the coverage anchors above:
  `workflows/context/neighborhood.rs`,
  `workflows/invitation/{create,accept,readiness}.rs`, and
  `workflows/messaging/{channel_refs,channels,send}.rs`. Shared-flow source
  metadata continues to publish through the `aura-app::ui_contract` facade.

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
- `just ci-user-flow-policy` enforces documentation and contributor-guidance updates for shared user flow contract and determinism surfaces via `scripts/check/user-flow-guidance-sync.sh`
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
- [User Flow Guidance Sync Gate](../scripts/check/user-flow-guidance-sync.sh)
- [Simulation Guide](805_simulation_guide.md)
- [Verification Coverage Report](998_verification_coverage.md)
- [Project Structure](999_project_structure.md)

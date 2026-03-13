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
| Harness User Flow Scenarios | 13 |
| Parity-Critical Scenarios (TUI + Web) | 11 |
| Mixed-Runtime Scenarios (TUI + Web distinct keys) | 2 |
| Auxiliary Coverage Scenarios | 8 |
| Core User Flow Domains | 11 |

## Coverage Classes

Aura tracks three different coverage classes in this document:

| Class | Meaning | Main Artifact |
|-------|---------|---------------|
| Parity-critical shared flow | One semantic flow that must remain portable across TUI and web and execute through the shared semantic command plane | `aura-app::ui_contract` + canonical harness scenarios |
| Mixed-runtime interoperability | User-visible flow that intentionally spans different frontend/runtime combinations | Canonical mixed-runtime scenarios |
| Frontend-specific or auxiliary coverage | Focused smoke, modal, renderer-specific, or conformance-only coverage that is useful but not the parity contract | Supplementary scenarios |

This report is a traceability document for those classes. It is not a proof of
protocol correctness, and it does not replace conformance or verification lanes.

## Canonical UX Scenario Set

| Scenario | File | Primary Flow |
|----------|------|--------------|
| Scenario 1 | `scenarios/harness/scenario1-invitation-chat-e2e.toml` | Invitation acceptance + shared channel + bidirectional chat |
| Scenario 2 | `scenarios/harness/scenario2-social-topology-e2e.toml` | Social topology and neighborhood operations |
| Scenario 3 | `scenarios/harness/scenario3-irc-slash-commands-e2e.toml` | Slash command lifecycle and moderation commands |
| Scenario 4 | `scenarios/harness/scenario4-global-nav-and-help-e2e.toml` | TUI frontend-conformance: global navigation and help modal behavior |
| Scenario 5 | `scenarios/harness/scenario5-chat-modal-and-retry-e2e.toml` | Chat wizard/modals and retry actions |
| Scenario 6 | `scenarios/harness/scenario6-contacts-lan-and-contact-lifecycle-e2e.toml` | Contacts, LAN scan, contact removal |
| Scenario 7 | `scenarios/harness/scenario7-neighborhood-keypath-parity-e2e.toml` | TUI frontend-conformance: neighborhood keypath parity and detail navigation |
| Scenario 8 | `scenarios/harness/scenario8-settings-devices-authority-e2e.toml` | Settings: profile, devices, authority panels |
| Scenario 9 | `scenarios/harness/scenario9-guardian-and-mfa-ceremonies-e2e.toml` | Guardian and MFA ceremony flows |
| Scenario 10 | `scenarios/harness/scenario10-recovery-and-notifications-e2e.toml` | Recovery request and notifications surfaces |
| Scenario 11 | `scenarios/harness/scenario11-demo-full-tui-flow-e2e.toml` | Full end-to-end demo-grade TUI flow |
| Scenario 12 | `scenarios/harness/scenario12-mixed-device-enrollment-removal-e2e.toml` | Mixed TUI/Web device enrollment + removal |
| Scenario 13 | `scenarios/harness/scenario13-mixed-contact-channel-message-e2e.toml` | Mixed TUI/Web contact invite + channel messaging |

Scenarios 4 and 7 are retained as TUI frontend-conformance coverage. They are
not part of the shared semantic product-flow suite.

## User Flow Matrix

| Flow Domain | Main Coverage | Secondary Coverage | Runtime Context |
|------------|----------------|--------------------|-----------------|
| Invitation create/accept | Scenario 1 | Scenarios 2, 5, 6, 9, 11, 13 | TUI + Web |
| Contact lifecycle | Scenario 6 | Scenarios 1, 2, 5, 9, 13 | TUI + Web |
| Chat channel + messaging | Scenario 1 | Scenarios 3, 5, 11, 13 | TUI + Web |
| Slash commands and moderation | Scenario 3 | `moderation-and-modal-coverage.toml`, `moderator-assign.toml` | TUI-heavy |
| Global navigation/help | Scenario 4 | Scenario 11 | TUI frontend-conformance |
| Neighborhood/home operations | `scenarios/harness/real-runtime-mixed-startup-smoke.toml` | Scenarios 2, 7, 11, `home-roles.toml` | Shared semantic + TUI conformance |
| Settings panels | `scenarios/harness/shared-settings-parity.toml` | Scenarios 8, 9, 10, 12 | TUI + Web |
| Device add/remove | Scenario 12 | Scenario 8 | Mixed runtime |
| Guardian/MFA ceremonies | Scenario 9 | Scenario 10 | TUI + Web |
| Recovery + notifications | Scenario 10 | Scenario 8 | TUI + Web |
| Mixed-device and mixed-user interoperability | Scenarios 12 and 13 | `cross-authority-contact.toml` | Mixed runtime |

## Auxiliary Scenario Coverage

These scenarios are maintained as focused supplements and smoke checks:

| Scenario File | Focus |
|---------------|-------|
| `local-discovery-smoke.toml` | Local discovery smoke coverage |
| `mixed-topology-smoke.toml` | Mixed-topology connectivity smoke |
| `mixed-topology-agent.toml` | Agent-level mixed topology behavior |
| `moderation-and-modal-coverage.toml` | Moderation + modal interaction sweep |
| `moderator-assign.toml` | Moderator assignment and kick operations |
| `access-override.toml` | Access override modal flow |
| `shared-storage.toml` | Shared-storage user flow |
| `cross-authority-contact.toml` | Cross-authority contact + neighborhood path |

## Planned Release And Update Validation Matrix

This matrix is the planned coverage target for harness-driven validation of
module and OTA distribution/update behavior.

The intended rollout order is:

1. mechanism validation matrix
2. candidate-release rehearsal matrix
3. live-release promotion gates

Mechanism validation proves the lifecycle machinery itself under synthetic
releases and controlled failures. Candidate-release rehearsal proves that one
specific release works before promotion or real cutover.

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

This matrix should remain typed-lifecycle driven. It should not be satisfied by
log scraping or ad hoc manual release notes.

## Coverage Expectations

### Shared Flow Contract Expectations

Every parity-critical shared flow should have, in code and metadata:

- a canonical shared flow identifier in `aura-app::ui_contract`
- a typed semantic command path on both TUI and web
- semantic action contracts with preconditions and terminal success/failure conditions
- an authoritative readiness, event, or quiescence owner for waits
- any parity exception recorded as typed metadata in `aura-app::ui_contract`
- at least one canonical scenario reference in this report

Shared-flow scenarios must not rely on raw PTY keys, raw selector clicks, raw
label matching, or incidental focus-stepping as their primary mechanics.
Those behaviors belong in frontend-conformance coverage instead.

Frontend-specific flows may still have scenario coverage, but they are not part
of the portability contract unless explicitly promoted into the shared contract.

### PR Gate Expectations

1. Changes to global navigation, settings, chat, contacts, neighborhood, or ceremonies should have at least one impacted canonical scenario updated or re-validated.
2. Changes that affect both TUI and web behavior should be validated against parity-critical scenarios in the shared semantic lane on both runtimes.
3. Changes to mixed-instance behavior should include scenario 12 and/or 13 coverage.
4. Mixed-runtime code exchange and chat routing changes should preserve the
   event-driven contract used by scenarios 12 and 13: invitation/device codes
   come from typed runtime-event payloads, and chat assertions bind to the
   selected shared channel rather than frontend-specific ordering.
5. Changes to renderer-specific control wiring should add or update
   frontend-conformance coverage rather than weakening the shared semantic lane.
6. Changes to OTA or module release/update architecture should update the
   planned release/update matrix above when they add, remove, or reorder
   mechanism-validation or release-rehearsal coverage.

### CI Enforcement

Fast CI currently uses two separate gates:

- `just ci-user-flow-coverage` enforces traceability heuristics between changed user flow-facing source files, canonical scenarios, and this report
- `AURA_ALLOW_FLOW_COVERAGE_SKIP=1` is a local-only escape hatch; CI rejects it
- `just ci-user-flow-policy` enforces documentation and contributor-guidance updates for shared user flow contract and determinism surfaces via `scripts/check/user-flow-guidance-sync.sh`
- OTA and module release/update validation rows in this report are part of that same user-flow guidance surface and must be kept in sync as the release matrix evolves
- The release/update rows are expected to land in staged order: mechanism validation first, candidate rehearsal second, and promotion-gate coverage last
- `just ci-harness-matrix-inventory` enforces that converted scenario classification drives the TUI/web matrix lanes
- shared semantic scenarios and frontend-conformance scenarios are expected to
  remain distinct classifications; CI policy should reject shared-flow drift
  back to renderer-driven mechanics

Current limitation:

- `ci-user-flow-coverage` still infers some ownership from filenames and does not yet prove that the correct scenario set changed
- docs updates and coverage traceability are distinct concerns; this report should not claim stronger behavioral enforcement than CI actually provides

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

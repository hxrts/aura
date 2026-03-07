# UX Flow Coverage Report

This document tracks end-to-end UX coverage for Aura's runtime harness scenarios across TUI and web surfaces.

## Coverage Boundary Statement

UX flow coverage validates user-visible behavior and interaction wiring through runtime harness scenarios.
It does not replace protocol conformance, theorem proofs, or differential parity lanes.
Use this report for UI/product flow traceability and regression targeting.

## Summary Metrics

| Metric | Count |
|--------|-------|
| Harness UX Scenarios | 13 |
| Parity-Critical Scenarios (TUI + Web) | 11 |
| Mixed-Runtime Scenarios (TUI + Web distinct keys) | 2 |
| Auxiliary Coverage Scenarios | 8 |
| Core UX Flow Domains | 11 |

## Canonical UX Scenario Set

| Scenario | File | Primary Flow |
|----------|------|--------------|
| Scenario 1 | `scenarios/harness/scenario1-invitation-chat-e2e.toml` | Invitation acceptance + shared channel + bidirectional chat |
| Scenario 2 | `scenarios/harness/scenario2-social-topology-e2e.toml` | Social topology and neighborhood operations |
| Scenario 3 | `scenarios/harness/scenario3-irc-slash-commands-e2e.toml` | Slash command lifecycle and moderation commands |
| Scenario 4 | `scenarios/harness/scenario4-global-nav-and-help-e2e.toml` | Global navigation and help modal behavior |
| Scenario 5 | `scenarios/harness/scenario5-chat-modal-and-retry-e2e.toml` | Chat wizard/modals and retry actions |
| Scenario 6 | `scenarios/harness/scenario6-contacts-lan-and-contact-lifecycle-e2e.toml` | Contacts, LAN scan, contact removal |
| Scenario 7 | `scenarios/harness/scenario7-neighborhood-keypath-parity-e2e.toml` | Neighborhood keypath parity and detail navigation |
| Scenario 8 | `scenarios/harness/scenario8-settings-devices-authority-e2e.toml` | Settings: profile, devices, authority panels |
| Scenario 9 | `scenarios/harness/scenario9-guardian-and-mfa-ceremonies-e2e.toml` | Guardian and MFA ceremony flows |
| Scenario 10 | `scenarios/harness/scenario10-recovery-and-notifications-e2e.toml` | Recovery request and notifications surfaces |
| Scenario 11 | `scenarios/harness/scenario11-demo-full-tui-flow-e2e.toml` | Full end-to-end demo-grade TUI flow |
| Scenario 12 | `scenarios/harness/scenario12-mixed-device-enrollment-removal-e2e.toml` | Mixed TUI/Web device enrollment + removal |
| Scenario 13 | `scenarios/harness/scenario13-mixed-contact-channel-message-e2e.toml` | Mixed TUI/Web contact invite + channel messaging |

## UX Flow Matrix

| Flow Domain | Main Coverage | Secondary Coverage | Runtime Context |
|------------|----------------|--------------------|-----------------|
| Invitation create/accept | Scenario 1 | Scenarios 2, 5, 6, 9, 11, 13 | TUI + Web |
| Contact lifecycle | Scenario 6 | Scenarios 1, 2, 5, 9, 13 | TUI + Web |
| Chat channel + messaging | Scenario 1 | Scenarios 3, 5, 11, 13 | TUI + Web |
| Slash commands and moderation | Scenario 3 | `moderation-and-modal-coverage.toml`, `moderator-assign.toml` | TUI-heavy |
| Global navigation/help | Scenario 4 | Scenario 11 | TUI + Web |
| Neighborhood/home operations | Scenario 2 | Scenarios 7, 11, `home-roles.toml` | TUI + Web |
| Settings panels | Scenario 8 | Scenarios 9, 10, 12 | TUI + Web |
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

## Coverage Expectations

### PR Gate Expectations

1. Changes to global navigation, settings, chat, contacts, neighborhood, or ceremonies should have at least one impacted canonical scenario updated or re-validated.
2. Changes that affect both TUI and web behavior should be validated against parity-critical scenarios (1-11) in both runtimes.
3. Changes to mixed-instance behavior should include scenario 12 and/or 13 coverage.

### CI Enforcement

Fast CI runs `scripts/check/ux-flow-coverage.sh` (`just ci-ux-flow-coverage`) to enforce that flow-relevant source changes are paired with scenario updates or an update to this report.

### Residual Risk Areas

| Area | Current Risk | Mitigation Direction |
|------|--------------|----------------------|
| Long-tail modal sequencing | Medium | Add focused scenario fragments for rare wizard branch paths |
| Toast timing/race windows | Medium | Prefer persistent-state assertions over toast-only checks |
| Cross-topology regressions | Medium | Keep mixed-topology smoke scenarios in scheduled lanes |

## References

- [Testing Guide](804_testing_guide.md)
- [Simulation Guide](805_simulation_guide.md)
- [Verification Coverage Report](998_verification_coverage.md)
- [Project Structure](999_project_structure.md)

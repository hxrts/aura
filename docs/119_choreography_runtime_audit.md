# Choreography Runtime Audit

This audit tracks every `choreography!` usage in the codebase and whether it is wired into the runtime execution path. Each entry is either **wired** (executed via the choreographic runtime) or **spec-only** with a migration ticket.

**Last Updated**: 2026-01-01

## Summary

- **Total choreographies**: 14
- **Wired**: 1 (AuraConsensus)
- **Spec-only**: 13

## Audit Table

| Location | Protocol | Status | Migration Ticket | Notes |
| --- | --- | --- | --- | --- |
| `crates/aura-consensus/src/protocol.rs` | AuraConsensus | **Wired** | — | Has `choreography_runtime.rs` with `run_coordinator`/`run_witness` functions and shared bus test. |
| `crates/aura-amp/src/choreography.rs` | AmpKeyExchange | Spec-only | CHOREO-AMP-001 | Needs MPST runner + adapter wiring in `aura-amp`. |
| `crates/aura-rendezvous/src/protocol.rs` | RendezvousExchange | Spec-only | CHOREO-RDV-001 | Direct peer discovery choreography not wired to runtime transport. |
| `crates/aura-rendezvous/src/protocol.rs` | RelayedRendezvous | Spec-only | CHOREO-RDV-002 | Relay-assisted connection choreography not wired. |
| `crates/aura-agent/src/handlers/sessions/coordination.rs` | CoordinationDemo | Spec-only | CHOREO-AGENT-001 | Demo choreography not executed via MPST adapter. |
| `crates/aura-authentication/src/guardian_auth_relational.rs` | GuardianAuthRelational | Spec-only | CHOREO-AUTH-001 | Needs runtime adapter integration. |
| `crates/aura-authentication/src/dkd.rs` | DkdHandshake | Spec-only | CHOREO-AUTH-002 | DKD choreography is specification-only. |
| `crates/aura-recovery/src/recovery_protocol.rs` | RecoveryProtocol | Spec-only | CHOREO-REC-001 | Recovery choreography not executed via MPST runtime. |
| `crates/aura-recovery/src/guardian_membership.rs` | GuardianMembership | Spec-only | CHOREO-REC-002 | Not wired to runtime. |
| `crates/aura-recovery/src/guardian_ceremony.rs` | GuardianCeremony | Spec-only | CHOREO-REC-003 | Not wired to runtime. |
| `crates/aura-recovery/src/guardian_setup.rs` | GuardianSetup | Spec-only | CHOREO-REC-004 | Not wired to runtime. |
| `crates/aura-invitation/src/protocol.rs` | InvitationProtocol | Spec-only | CHOREO-INV-001 | Invitation choreography not executed via MPST runtime. |
| `crates/aura-invitation/src/protocol.rs` | InvitationCompletion | Spec-only | CHOREO-INV-002 | Completion choreography not wired. |
| `crates/aura-sync/src/protocols/epochs.rs` | EpochSync | Spec-only | CHOREO-SYNC-001 | Sync choreography not wired to runtime adapter. |

## Runtime Infrastructure

The runtime provides `ChoreographicEffects` implementation in `aura-agent/src/runtime/effects/choreography.rs`:

- `AuraEffectSystem` implements `ChoreographicEffects` trait
- Provides `send_to_role_bytes`, `receive_from_role_bytes`, `broadcast_bytes`
- Integrates with guard chain for capability checks and flow costs
- Session management via `start_session`/`end_session`

To wire a choreography:
1. Create a `choreography_runtime.rs` module in the crate
2. Implement `run_coordinator` and `run_witness` (or equivalent role functions)
3. Use `ChoreographicEffects` for message passing
4. Add integration tests with shared bus pattern (see `aura-consensus` example)

## Migration Priority

1. **High**: RecoveryProtocol, GuardianCeremony (security-critical paths)
2. **Medium**: InvitationProtocol, RendezvousExchange (user-facing flows)
3. **Low**: DkdHandshake, EpochSync, CoordinationDemo (can remain spec-only longer)

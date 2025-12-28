# Choreography Runtime Audit

This audit tracks every `choreography!` usage in the codebase and whether it is wired into the runtime execution path. Each entry is either **wired** (executed via the choreographic runtime) or **spec-only** with a migration ticket.

| Location | Protocol | Status | Migration Ticket | Notes |
| --- | --- | --- | --- | --- |
| `crates/aura-consensus/src/consensus/protocol.rs` | AuraConsensus | **Wired** | — | Executed via `consensus::choreography_runtime` with shared bus test. |
| `crates/aura-amp/src/choreography.rs` | AmpKeyExchange | Spec-only | CHOREO-AMP-001 | Needs MPST runner + adapter wiring in `aura-amp`. |
| `crates/aura-rendezvous/src/protocol.rs` | RendezvousDiscovery | Spec-only | CHOREO-RDV-001 | Discovery choreography not wired to runtime transport. |
| `crates/aura-rendezvous/src/protocol.rs` | RendezvousReceipt | Spec-only | CHOREO-RDV-002 | Receipt choreography not wired. |
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

## Next Steps

- Add MPST runners per migration ticket using `aura-protocol::choreography::MpstExecutor`.
- Wire each protocol to the choreographic runtime and add end-to-end tests similar to consensus.

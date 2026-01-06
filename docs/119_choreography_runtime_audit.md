# Choreography Runtime Audit

This audit tracks every `choreography!` usage in the codebase and whether it is wired into the runtime execution path. Each entry is either Wired (executed via the choreographic runtime) or Spec-only (definition exists but not yet integrated).

**Last Updated**: 2026-01-06

---

## Summary

| Status | Count | Protocols |
|--------|-------|-----------|
| Wired | 0 | — |
| Spec-only | 14 | All protocols |
| **Total** | **14** | |

---

## Choreography Inventory

### Consensus

| Protocol | Location | Status | Ticket | Notes |
|----------|----------|--------|--------|-------|
| AuraConsensus | `crates/aura-consensus/src/protocol/choreography.choreo` | Spec-only | CHOREO-CONS-001 | Awaiting `execute_as` wiring |

### Transport & Channels

| Protocol | Location | Status | Ticket | Notes |
|----------|----------|--------|--------|-------|
| AmpTransport | `crates/aura-amp/src/choreography.choreo` | Spec-only | CHOREO-AMP-001 | Needs MPST runner + adapter wiring |

### Rendezvous

| Protocol | Location | Status | Ticket | Notes |
|----------|----------|--------|--------|-------|
| RendezvousExchange | `crates/aura-rendezvous/src/protocol.rendezvous_exchange.choreo` | Spec-only | CHOREO-RDV-001 | Direct peer discovery |
| RelayedRendezvous | `crates/aura-rendezvous/src/protocol.relayed_rendezvous.choreo` | Spec-only | CHOREO-RDV-002 | Relay-assisted connection |

### Authentication

| Protocol | Location | Status | Ticket | Notes |
|----------|----------|--------|--------|-------|
| GuardianAuthRelational | `crates/aura-authentication/src/guardian_auth_relational.choreo` | Spec-only | CHOREO-AUTH-001 | Needs runtime adapter |
| DkdChoreography | `crates/aura-authentication/src/dkd.choreo` | Spec-only | CHOREO-AUTH-002 | Distributed key derivation |

### Recovery

| Protocol | Location | Status | Ticket | Notes |
|----------|----------|--------|--------|-------|
| RecoveryProtocol | `crates/aura-recovery/src/recovery_protocol.choreo` | Spec-only | CHOREO-REC-001 | Account recovery flow |
| GuardianMembershipChange | `crates/aura-recovery/src/guardian_membership.choreo` | Spec-only | CHOREO-REC-002 | Guardian add/remove |
| GuardianCeremony | `crates/aura-recovery/src/guardian_ceremony.choreo` | Spec-only | CHOREO-REC-003 | Guardian key ceremony |
| GuardianSetup | `crates/aura-recovery/src/guardian_setup.choreo` | Spec-only | CHOREO-REC-004 | Initial guardian setup |

### Invitation

| Protocol | Location | Status | Ticket | Notes |
|----------|----------|--------|--------|-------|
| InvitationExchange | `crates/aura-invitation/src/protocol.invitation_exchange.choreo` | Spec-only | CHOREO-INV-001 | Contact invitation |
| GuardianInvitation | `crates/aura-invitation/src/protocol.guardian_invitation.choreo` | Spec-only | CHOREO-INV-002 | Guardian invitation |

### Sync

| Protocol | Location | Status | Ticket | Notes |
|----------|----------|--------|--------|-------|
| EpochRotationProtocol | `crates/aura-sync/src/protocols/epochs.choreo` | Spec-only | CHOREO-SYNC-001 | Epoch rotation sync |

### Demo

| Protocol | Location | Status | Ticket | Notes |
|----------|----------|--------|--------|-------|
| SessionCoordinationChoreography | `crates/aura-agent/src/handlers/sessions/coordination.choreo` | Spec-only | CHOREO-AGENT-001 | Demo/test choreography |

---

## Migration Priority

| Priority | Protocols | Rationale |
|----------|-----------|-----------|
| High | RecoveryProtocol, GuardianCeremony, GuardianSetup | Security-critical paths |
| Medium | InvitationExchange, GuardianInvitation, RendezvousExchange | User-facing flows |
| Low | DkdChoreography, EpochRotationProtocol, SessionCoordinationChoreography | Can remain spec-only longer |

---


## Wiring Plan + Ownership (MPST `execute_as`)

This section captures the **owner**, **runtime entry point**, and **blocking dependencies** for each choreography under the v0.8.0 `execute_as` pattern.

| Protocol | Owner (crate) | Runtime entry point (execute_as) | Guard chain integration | Dependencies / Blocks |
|----------|--------------|-----------------------------------|-------------------------|-----------------------|
| AuraConsensus | `aura-consensus` | `crates/aura-agent/src/runtime_bridge/consensus.rs` | `crates/aura-agent/src/runtime/effects/choreography.rs` | Role‑family resolution in rumpsteak‑aura + wiring |
| RecoveryProtocol | `aura-recovery` | `crates/aura-agent/src/handlers/recovery_service.rs` | `crates/aura-agent/src/runtime/effects/choreography.rs` | Role‑family resolution + wiring |
| GuardianCeremony | `aura-recovery` | `crates/aura-agent/src/handlers/recovery_service.rs` | `crates/aura-agent/src/runtime/effects/choreography.rs` | Role‑family resolution + wiring |
| GuardianSetup | `aura-recovery` | `crates/aura-agent/src/handlers/recovery_service.rs` | `crates/aura-agent/src/runtime/effects/choreography.rs` | Role‑family resolution + wiring |
| GuardianMembershipChange | `aura-recovery` | `crates/aura-agent/src/handlers/recovery_service.rs` | `crates/aura-agent/src/runtime/effects/choreography.rs` | Role‑family resolution + wiring |
| InvitationExchange | `aura-invitation` | `crates/aura-agent/src/handlers/invitation_service.rs` | `crates/aura-agent/src/runtime/effects/choreography.rs` | Role‑family resolution + wiring |
| GuardianInvitation | `aura-invitation` | `crates/aura-agent/src/handlers/invitation_service.rs` | `crates/aura-agent/src/runtime/effects/choreography.rs` | Role‑family resolution + wiring |
| RendezvousExchange | `aura-rendezvous` | `crates/aura-agent/src/handlers/rendezvous_service.rs` | `crates/aura-agent/src/runtime/effects/choreography.rs` | Role‑family resolution + wiring |
| RelayedRendezvous | `aura-rendezvous` | `crates/aura-agent/src/runtime/services/rendezvous_manager.rs` | `crates/aura-agent/src/runtime/effects/choreography.rs` | Role‑family resolution + wiring |
| AmpTransport | `aura-amp` | `crates/aura-agent/src/runtime_bridge/amp.rs` | `crates/aura-agent/src/runtime/effects/choreography.rs` | Role‑family resolution + wiring |
| GuardianAuthRelational | `aura-authentication` | `crates/aura-authentication/src/guardian_auth_relational.rs` | `crates/aura-agent/src/runtime/effects/choreography.rs` | Role‑family resolution + wiring |
| DkdChoreography | `aura-authentication` | `crates/aura-authentication/src/dkd.rs` | `crates/aura-agent/src/runtime/effects/choreography.rs` | Role‑family resolution + wiring |
| EpochRotationProtocol | `aura-sync` | `crates/aura-agent/src/runtime/services/sync_manager.rs` | `crates/aura-agent/src/runtime/effects/choreography.rs` | Role‑family resolution + wiring |
| SessionCoordinationChoreography | `aura-agent` | `crates/aura-agent/src/handlers/sessions/coordination.rs` | `crates/aura-agent/src/runtime/effects/choreography.rs` | Role‑family resolution + wiring |

## Decision Sourcing (provide_message / select_branch)

This table captures how generated runners obtain outbound messages and branch decisions. These are **initial wiring assumptions** to be verified during integration.

| Protocol | provide_message source | select_branch source |
|----------|------------------------|----------------------|
| AuraConsensus | Derived from `ConsensusParams` + local consensus state | N/A (no choices) |
| RecoveryProtocol | Derived from recovery request + journal state | N/A (no choices) |
| GuardianCeremony | Ceremony proposal/response/commit or abort from service state | Initiator policy/UI decision (Commit/Abort) |
| GuardianSetup | Guardian setup request + local key state | N/A (no choices) |
| GuardianMembershipChange | Membership change request + journal state | N/A (no choices) |
| InvitationExchange | Invitation payload from params + local facts | N/A (no choices) |
| GuardianInvitation | Guardian invite payload from params + local facts | N/A (no choices) |
| RendezvousExchange | Rendezvous descriptors + discovery payload from manager state | N/A (no choices) |
| RelayedRendezvous | Relayed rendezvous payloads from manager state | N/A (no choices) |
| AmpTransport | AMP message payloads from channel state | N/A (no choices) |
| GuardianAuthRelational | Auth challenge/response derived from auth state | N/A (no choices) |
| DkdChoreography | DKG messages derived from DKG state | N/A (no choices) |
| EpochRotationProtocol | Epoch rotation messages from sync manager state | N/A (no choices) |
| SessionCoordinationChoreography | Session request/invite/response from session manager state | Participants: accept/reject from UI/policy; Coordinator: success/failure from validation |

## Runtime Infrastructure

The runtime provides `ChoreographicEffects` implementation in `aura-agent/src/runtime/effects/choreography.rs`.
Generated runners are driven via `AuraProtocolAdapter` in
`crates/aura-agent/src/runtime/choreography_adapter.rs`, which bridges
`ChoreographicEffects` to the `ChoreographicAdapter` API.

### ChoreographicEffects Trait

| Method | Purpose |
|--------|---------|
| `send_to_role_bytes` | Send message to specific role |
| `receive_from_role_bytes` | Receive message from specific role |
| `broadcast_bytes` | Broadcast to all roles |
| `start_session` | Initialize choreography session |
| `end_session` | Terminate choreography session |

### Integration Features

- Guard chain integration (`CapGuard` → `FlowGuard` → `JournalCoupler`)
- Transport effects for message passing
- Session lifecycle management with metrics

### Wiring a Choreography (v0.8.0)

1. Store the protocol in a `.choreo` file **next to the Rust module** that loads it.
2. Use `choreography!(include_str!("..."))` to generate the protocol module and runners.
3. Build an `AuraProtocolAdapter` (from `crates/aura-agent/src/runtime/choreography_adapter.rs`)
   and call `Protocol::execute_as(role, &mut adapter, params)` from the runtime bridge/service.
4. Provide decision sources for `provide_message` and `select_branch` per the table above.

---

## Protocol Version Negotiation

All choreographic protocols participate in version negotiation during connection establishment.

### Version Handshake Flow

```
Initiator                    Responder
   |                            |
   |-- VersionHandshakeRequest -->
   |     (version, min_version, capabilities, nonce)
   |                            |
   |<-- VersionHandshakeResponse -|
   |     (Accepted/Rejected)
   |                            |
[Use negotiated version or disconnect]
```

Handler: `aura-protocol/src/handlers/version_handshake.rs`

### Handshake Outcomes

| Outcome | Response Contents |
|---------|-------------------|
| Compatible | `negotiated_version` (min of both peers), shared `capabilities` |
| Incompatible | `reason`, peer version, optional `upgrade_url` |

### Protocol Capabilities

| Capability | Min Version | Description |
|------------|-------------|-------------|
| `ceremony_supersession` | 1.0.0 | Ceremony replacement tracking |
| `version_handshake` | 1.0.0 | Protocol version negotiation |
| `fact_journal` | 1.0.0 | Fact-based journal sync |

### Handshake Integration Points

| Location | Status | Notes |
|----------|--------|-------|
| `aura-rendezvous/src/flood/mod.rs` | Planned | Add `perform_handshake()` before peer exchange |
| `aura-invitation/src/protocol.rs` | Planned | Version check before ceremony initiation |
| Transport establishment | Planned | Handshake on WebSocket/QUIC connection |

---

## Ceremony Supersession

See `docs/118_key_rotation_ceremonies.md` for the complete supersession specification.

### Supersession Facts

All ceremony fact enums include a `CeremonySuperseded` variant for explicit replacement tracking:

| Crate | Fact Enum | Location |
|-------|-----------|----------|
| aura-invitation | `InvitationFact` | `src/facts.rs` |
| aura-recovery | `CeremonyFact` | `src/guardian_ceremony.rs` |
| aura-recovery | `RecoveryCeremonyFact` | `src/recovery_ceremony.rs` |
| aura-sync | `OTACeremonyFact` | `src/protocols/ota_ceremony.rs` |

### CeremonyTracker API

Location: `aura-agent/src/runtime/services/ceremony_tracker.rs`

| Method | Purpose |
|--------|---------|
| `supersede(old_id, new_id, reason)` | Record supersession event |
| `check_supersession_candidates(prestate_hash, op_type)` | Find stale ceremonies |
| `get_supersession_chain(ceremony_id)` | Get full supersession history |
| `is_superseded(ceremony_id)` | Check if ceremony was replaced |

---

## Migration Infrastructure

The `MigrationCoordinator` (`aura-agent/src/runtime/migration.rs`) orchestrates data migrations between protocol versions.

### Migration Trait

```rust
#[async_trait]
pub trait Migration: Send + Sync {
    fn source_version(&self) -> SemanticVersion;
    fn target_version(&self) -> SemanticVersion;
    fn name(&self) -> &str;
    async fn validate(&self, ctx: &MigrationContext) -> Result<(), MigrationError>;
    async fn execute(&self, ctx: &MigrationContext) -> Result<(), MigrationError>;
}
```

### Coordinator API

| Method | Purpose |
|--------|---------|
| `needs_migration(from)` | Check if upgrade is needed |
| `get_migration_path(from, to)` | Find ordered migration sequence |
| `migrate(from, to)` | Execute migrations with validation |
| `validate_migration(from, to)` | Dry-run validation only |

### Migration Guarantees

- Migrations are ordered by target version
- Each migration runs at most once (idempotent via version tracking)
- Failed migrations leave the system in a consistent state
- Progress is recorded in the journal for auditability

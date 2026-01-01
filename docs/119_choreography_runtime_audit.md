# Choreography Runtime Audit

This audit tracks every `choreography!` usage in the codebase and whether it is wired into the runtime execution path. Each entry is either **Wired** (executed via the choreographic runtime) or **Spec-only** (definition exists but not yet integrated).

**Last Updated**: 2026-01-01

---

## Summary

| Status | Count | Protocols |
|--------|-------|-----------|
| Wired | 1 | AuraConsensus |
| Spec-only | 13 | All others |
| **Total** | **14** | |

---

## Choreography Inventory

### Consensus

| Protocol | Location | Status | Ticket | Notes |
|----------|----------|--------|--------|-------|
| AuraConsensus | `aura-consensus/src/protocol.rs` | **Wired** | — | Has `choreography_runtime.rs` with shared bus test |

### Transport & Channels

| Protocol | Location | Status | Ticket | Notes |
|----------|----------|--------|--------|-------|
| AmpTransport | `aura-amp/src/choreography.rs` | Spec-only | CHOREO-AMP-001 | Needs MPST runner + adapter wiring |

### Rendezvous

| Protocol | Location | Status | Ticket | Notes |
|----------|----------|--------|--------|-------|
| RendezvousExchange | `aura-rendezvous/src/protocol.rs` | Spec-only | CHOREO-RDV-001 | Direct peer discovery |
| RelayedRendezvous | `aura-rendezvous/src/protocol.rs` | Spec-only | CHOREO-RDV-002 | Relay-assisted connection |

### Authentication

| Protocol | Location | Status | Ticket | Notes |
|----------|----------|--------|--------|-------|
| GuardianAuthRelational | `aura-authentication/src/guardian_auth_relational.rs` | Spec-only | CHOREO-AUTH-001 | Needs runtime adapter |
| DkdChoreography | `aura-authentication/src/dkd.rs` | Spec-only | CHOREO-AUTH-002 | Distributed key derivation |

### Recovery

| Protocol | Location | Status | Ticket | Notes |
|----------|----------|--------|--------|-------|
| RecoveryProtocol | `aura-recovery/src/recovery_protocol.rs` | Spec-only | CHOREO-REC-001 | Account recovery flow |
| GuardianMembershipChange | `aura-recovery/src/guardian_membership.rs` | Spec-only | CHOREO-REC-002 | Guardian add/remove |
| GuardianCeremony | `aura-recovery/src/guardian_ceremony.rs` | Spec-only | CHOREO-REC-003 | Guardian key ceremony |
| GuardianSetup | `aura-recovery/src/guardian_setup.rs` | Spec-only | CHOREO-REC-004 | Initial guardian setup |

### Invitation

| Protocol | Location | Status | Ticket | Notes |
|----------|----------|--------|--------|-------|
| InvitationExchange | `aura-invitation/src/protocol.rs` | Spec-only | CHOREO-INV-001 | Contact invitation |
| GuardianInvitation | `aura-invitation/src/protocol.rs` | Spec-only | CHOREO-INV-002 | Guardian invitation |

### Sync

| Protocol | Location | Status | Ticket | Notes |
|----------|----------|--------|--------|-------|
| EpochRotationProtocol | `aura-sync/src/protocols/epochs.rs` | Spec-only | CHOREO-SYNC-001 | Epoch rotation sync |

### Demo

| Protocol | Location | Status | Ticket | Notes |
|----------|----------|--------|--------|-------|
| SessionCoordinationChoreography | `aura-agent/src/handlers/sessions/coordination.rs` | Spec-only | CHOREO-AGENT-001 | Demo/test choreography |

---

## Migration Priority

| Priority | Protocols | Rationale |
|----------|-----------|-----------|
| High | RecoveryProtocol, GuardianCeremony, GuardianSetup | Security-critical paths |
| Medium | InvitationExchange, GuardianInvitation, RendezvousExchange | User-facing flows |
| Low | DkdChoreography, EpochRotationProtocol, SessionCoordinationChoreography | Can remain spec-only longer |

---

## Runtime Infrastructure

The runtime provides `ChoreographicEffects` implementation in `aura-agent/src/runtime/effects/choreography.rs`.

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

### Wiring a Choreography

1. Create `choreography_runtime.rs` module in the crate
2. Implement `run_coordinator` and `run_witness` (or equivalent role functions)
3. Use `ChoreographicEffects` for message passing
4. Add integration tests with shared bus pattern (see `aura-consensus` example)

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

# Key Rotation Ceremonies (Category C)

Aura treats *membership changes* and *key rotations* as **Category C ceremonies**: blocking, multi-step operations that must either **commit** atomically or **abort** cleanly. This document describes the shared contract used by production and demo/simulator runtimes.

## Why ceremonies?

Operations like “add a device”, “add/remove guardians”, “change group membership”, or “change home membership” all change who can produce valid signatures (or who is expected to participate in signing). These operations:

- require multi-party participation and explicit consent,
- must be bound to a **prestate** to avoid TOCTOU / replay,
- must support **rollback** if the ceremony fails or is cancelled.

See `docs/117_operation_categories.md` for the Category C requirements.

**Finalization rule**: Provisional or coordinator fast paths may be used to stage intent, but key rotation is only durable once consensus finalizes the ceremony (commit facts + transcript commit).

## Shared contract

All key rotation ceremonies follow this common shape:

1. **Compute prestate**
   - Derive a stable prestate hash from the authority/context state being modified.
   - The prestate must include the current epoch and the effective participant set / policy.

2. **Propose operation**
   - Define the operation being performed (e.g. add leaf, remove leaf, policy change, rotate epoch).
   - Compute an operation hash bound to the proposal parameters.

3. **Enter pending epoch (prepare)**
   - Generate new key material at a **pending epoch** without invalidating the old epoch yet.
   - Store enough metadata to allow either commit or rollback of the pending epoch.

4. **Collect responses**
   - Send invitations/requests to participants (devices, guardians, group members).
   - Participants respond using their full runtimes; their responses must be authenticated and recorded (facts/messages).

5. **Commit or abort**
   - If acceptance/threshold conditions are met, **commit**:
     - commit the pending epoch (making it authoritative),
     - emit the resulting facts/tree ops (e.g. binding facts, membership facts, attested ops).
   - Otherwise **abort**:
     - emit an abort fact with a reason,
     - rollback the pending epoch and leave the prior epoch active.

## Lifecycle Taxonomy

Aura models ceremonies with **two orthogonal axes**:

### Key Generation (K)

| Code | Method | Description |
|------|--------|-------------|
| K1 | Single-signer | No DKG required; local key generation |
| K2 | Dealer-based DKG | Trusted coordinator distributes shares |
| K3 | Consensus-finalized DKG | BFT-DKG with transcript commit |
| DKD | Distributed key derivation | Multi-party derivation (non-DKG) |

### Agreement Level (A)

| Code | Level | Description |
|------|-------|-------------|
| A1 | Provisional | Usable immediately, not final |
| A2 | Coordinator Soft-Safe | Bounded divergence with convergence cert |
| A3 | Consensus-Finalized | Unique, durable, non-forkable |

Fast paths (A1/A2) are **provisional**. Durable shared state must be finalized by A3.

---

## Per-Ceremony Policy Matrix

### Authority & Device Ceremonies

| Ceremony | Key Gen | Agreement | Fallback | Notes |
|----------|---------|-----------|----------|-------|
| Authority bootstrap | K1 | A3 | None | Local, immediate, no consensus needed |
| Device enrollment | K2 | A1→A2→A3 | A1/A2 | Provisional → soft-safe → finalize |
| Device MFA rotation | K3 | A2→A3 | A2 | Consensus-finalized keys |
| Device removal | K3 | A2→A3 | A2 | Remove via rotation; consensus keys |

### Guardian Ceremonies

| Ceremony | Key Gen | Agreement | Fallback | Notes |
|----------|---------|-----------|----------|-------|
| Guardian setup/rotation | K3 | A2→A3 | A2 | Consensus-finalized keys for durability |
| Recovery approval | — | A2→A3 | A2 | Soft-safe approvals → consensus commit |
| Recovery execution | — | A2→A3 | A2 | Consensus-finalized commit |

### Channel & Group Ceremonies

| Ceremony | Key Gen | Agreement | Fallback | Notes |
|----------|---------|-----------|----------|-------|
| AMP channel epoch bump | — | A1→A2→A3 | A1/A2 | Proposed → convergence cert → commit |
| AMP channel bootstrap | — | A1→A2→A3 | A1/A2 | Provisional channel → group key rotation |
| Group/Block creation | K3 | A1→A2→A3 | A1/A2 | Provisional bootstrap → consensus group key |
| Rendezvous secure-channel | — | A1→A2→A3 | A1/A2 | Provisional link → consensus secure channel |

### Other Ceremonies

| Ceremony | Key Gen | Agreement | Fallback | Notes |
|----------|---------|-----------|----------|-------|
| Invitation (contact/channel/guardian) | — | A3 | None | Consensus-finalized only; no A1/A2 |
| OTA activation | — | A2→A3 | A2 | Threshold-signed → consensus finalize |
| DKD ceremony | DKD | A2→A3 | A2 | Multi-party derivation → consensus commit |

## Ceremony kinds

### Guardian key rotation

**What changes**: The guardian participant set and threshold configuration for the account authority.

**Acceptance**: The invited guardians must accept and store their shares. Threshold rules for completion are policy-defined (often “all invited guardians accepted”).

**Commit result**:
- Pending epoch becomes active.
- Guardian-binding facts are emitted (fact-based journals).

### Device enrollment (“Add device”)

**What changes**: The device participant set for the *account authority* (a membership change under the account’s commitment tree) and the signing configuration associated with that membership.

**Acceptance**: The invited device runtime must accept and install the share. Depending on policy, existing participants may also need to approve (e.g., current device + guardians).

**Commit result**:
- Pending epoch becomes active (or a new epoch is rotated as part of the tree op).
- A device leaf is added/activated in the commitment tree (or equivalent membership facts are emitted).
- Device list / membership views update via the same reactive signals as production.

**Multi-authority note**: Device enrollment adds signing capability for the account authority but does not replace any other authorities the device participates in. Devices may hold threshold shares for multiple authorities concurrently.

### Group / home membership changes

These are conceptually identical ceremonies applied to different authorities/contexts:

- Group authorities: membership changes affect group signing participants.
- Block contexts: membership/steward changes may require signing changes depending on policy.

This document defines the *contract*; specific protocol details live in the feature crates that own those domains.

## Demo/simulator requirement

Demo mode must use the **same runtime-backed machinery** as production:

- The simulator instantiates real agent runtimes (Alice/Carol) and drives them on their behalf.
- Demo uses an **in-memory transport implementation** that still passes through the guard chain and transport semantics (it is “real transport”, not a side-channel).
- The UI must not “seed” ceremony outcomes (e.g. fake peer counts or fake device additions). All state changes must come from facts/signals emitted by the runtime.

## UI contract

Frontends (TUI, mobile, web) should treat ceremonies as first-class operations with a consistent UX:

- Show a “ceremony started” state and any shareable code needed by the other party.
- Show progress (accepted vs required), errors, and a clear “cancel” affordance.
- On cancellation/failure: show explicit rollback messaging; do not leave UI in a partially-updated state.

The UI must not invent state transitions: ceremony progress should be driven from runtime status + signals.

## Ceremony Supersession

When a new ceremony replaces an old one (e.g., due to prestate changes, explicit cancellation, or concurrent ceremony resolution), Aura emits explicit **supersession facts** that propagate via anti-entropy.

### Supersession Reasons

- **PrestateStale**: The prestate changed while the ceremony was pending, invalidating it.
- **NewerRequest**: An explicit newer request from the same initiator supersedes the old one.
- **ExplicitCancel**: Manual cancellation by an authorized participant.
- **Timeout**: The ceremony exceeded its validity window.
- **Precedence**: A concurrent ceremony won (conflict resolution).

### Supersession Facts

Each ceremony fact enum includes a `CeremonySuperseded` variant:

```rust
CeremonySuperseded {
    superseded_ceremony_id: String,
    superseding_ceremony_id: String,
    reason: String,
    trace_id: Option<String>,  // Correlation with superseding ceremony
    timestamp_ms: u64,
}
```

### Supersession Audit Trail

The `CeremonyTracker` maintains supersession records for auditability:

- `supersede(old_id, new_id, reason)`: Record a supersession event
- `check_supersession_candidates(prestate_hash, op_type)`: Find ceremonies that would be superseded
- `get_supersession_chain(ceremony_id)`: Get the full supersession history

### Propagation

Supersession facts propagate via the existing anti-entropy mechanism—no special protocol is required. Peers receiving a `CeremonySuperseded` fact update their local ceremony state accordingly.

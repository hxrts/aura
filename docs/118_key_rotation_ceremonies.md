# Key Rotation Ceremonies (Category C)

Aura treats *membership changes* and *key rotations* as **Category C ceremonies**: blocking, multi-step operations that must either **commit** atomically or **abort** cleanly. This document describes the shared contract used by production and demo/simulator runtimes.

## Why ceremonies?

Operations like “add a device”, “add/remove guardians”, “change group membership”, or “change block membership” all change **who can produce valid signatures** (or who is expected to participate in signing). These operations:

- require multi-party participation and explicit consent,
- must be bound to a **prestate** to avoid TOCTOU / replay,
- must support **rollback** if the ceremony fails or is cancelled.

See `docs/117_operation_categories.md` for the Category C requirements.

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

### Group / block membership changes

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

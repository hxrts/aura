# 101 · Threshold Identity

## Relational Identity Model

### Account Structure

An Aura account is a relational object anchored in the journal described in `001` and `002`. The account root is a ratchet tree root commitment as in `123`.

Every device and guardian occupies a leaf in this tree. Every branch stores a threshold policy. The journal stores attested tree operations as facts and capability refinements as caps.

### State Management

No device edits state directly. A device proposes a tree operation through a choreography that runs in `aura-protocol`. The choreography emits exactly one `AttestedOp` or aborts.

Reduction of the operation log yields the canonical tree. This ensures a deterministic, auditable state that all devices can verify independently.

### Privacy and Consistency

Deterministic Key Derivation binds relationship contexts to the account root so every RID inherits the same graph state. This model ensures that the social graph is a first-class resource that follows the semilattice rules in `001` and the privacy guarantees in `004`.

### Tree Data Structures

The reduced tree exposes three structures: `TreeSnapshot`, `DeviceLeaf`, and `GuardianLeaf`.

- **TreeSnapshot**: Carries the current commitment, epoch, leaf metadata, and the per-branch threshold policy
- **DeviceLeaf**: Stores the device identifier, the MLS-style key package used for HPKE and signing, and an optional default flow limit
- **GuardianLeaf**: Stores the guardian identifier and its attestation key

All of these values live in journal facts under the account namespace. They never leave the CRDT context, so reconciliation always follows monotone rules.

### Deterministic Key Derivation

Deterministic Key Derivation is the entry point for all context-specific identities. Every device builds a `DKDCapsule { app_id, context_label, ttl, issued_at }` and serializes it to canonical CBOR.

The device computes the context identifier using the `aura.context_id.v1` domain separation tag. The device then evaluates the DKD function inside the `DeterministicEffects` handler.

DKD multiplies the local FROST share by the capsule hash, clears the cofactor, and feeds the result into HKDF to derive `ContextIdentity`. The handler also produces a keyed MAC over the capsule so other devices can verify the derived key without recomputing the operation.

### Epoch Scoping

The derived identifier is scoped to the account epoch. When the session epoch in the journal bumps, devices must reissue presence tickets so old context identities cannot authenticate.

---

## Threshold Signature System

### Account Root Key Structure

The account root key is not a single key. It is a FROST aggregate derived from the leaves that carry signing authority. Each eligible leaf holds a share.

Shares never appear in the journal. This keeps cryptographic material isolated from the distributed ledger.

### Signing Sessions

A signing session is expressed as a choreography that references the tree commitment and policy identifier. The session performs share generation, nonce exchange, and final signing.

The handler records `ThresholdSignResult { tree_commitment, policy_id, agg_sig, witness }` as an ephemeral proof. This proof allows any verifier to check that the aggregate signature corresponds to a policy in the reduced tree.

### Fork Prevention

Honest devices refuse to sign if the tree commitment presented by the session does not match their locally reduced state. This prevents forked policies from producing signatures.

The signing choreography is the only interface that can produce account-level authorizations such as ledger checkpoints, DKD attestations, or guardian votes. All other flows reference those attestations as facts.

### Cryptographic Implementation

All threshold work uses Ed25519-FROST with deterministic nonce derivation (`FROST-ED25519-SHA512`). The choreography exposes three phases:

1. **NonceExchange**: Broadcasts binding commitments and charges the flow ledger for every participant. Spam control applies even during coordination.

2. **ShareSign**: Computes the partial signatures and returns them to the leader.

3. **Aggregate**: Verifies each share against the published commitments, aggregates them, and emits the `ThresholdSignResult`.

### Session Type Guarantees

Each phase runs inside a session type that enforces ordering and ensures devices cannot skip guards. The final aggregate signature is bound to the tree commitment hash and the policy node identifier, so replaying a signature under a different tree state fails verification.

---

## Multi-Device Coherence

### Device Representation

Accounts often run on several devices. Each device is a leaf with role `Device` in the ratchet tree.

Adding or removing a device is a `TreeOpKind::AddLeaf` or `TreeOpKind::RemoveLeaf` fact. Devices learn about changes through journal reduction.

### State Tracking

Every device tracks the account epoch, the per-context rendezvous counter, and the flow-budget ledger. This local state enables devices to verify updates and enforce spam control independently.

### Device Initialization

When a new device boots, it performs three steps:

1. Merge the latest journal snapshot
2. Verify the tree commitment against the attested root
3. Request the encrypted relationship keys from peers using the `PairwiseKeyUpdate` facts in `123`

### Flow Budget Enforcement

Transport-facing modules rely on the same flow-budget interface defined in `103`. A device cannot publish envelopes or handshakes if the ledger reports exhausted budget for that context.

This keeps spam control consistent even when multiple devices act in parallel.

### Device Coordination Choreographies

Device coordination relies on three choreographies:

- **AddDevice**: Authenticates the joining device, records the new leaf, and distributes updated relationship keys through the journal
- **RemoveDevice**: Records the removal reason and rotates the affected policies
- **RotateShares**: Refreshes FROST shares without changing the tree topology

### Algebraic Consistency

Each choreography uses the session-type algebra from `001`:

- Capability guards run first
- Journal merges happen before the message send
- Leakage plus flow budgets are charged per transition

This keeps the entire multi-device workflow aligned with the calculus and the system architecture.

### Device Flow-Budget Coordination

Multiple devices under the same account share the per-context FlowBudget facts described in `docs/103_info_flow_budget.md`. To avoid double-spending these budgets the identity layer maintains a per-account allocator:

1. **Reservation**: Before publishing any envelope or rendezvous descriptor, a device requests a reservation token from the allocator stored in the journal (`FlowReservation { ctx, device_id, amount, expires_at }`). Only one reservation per `(ctx, device_id)` is outstanding at a time.
2. **Charge**: The device passes the reservation token to `FlowGuard`, which charges the global `(ctx, neighbor)` ledger entry. If the global limit is reached, the reservation fails and the device backs off.
3. **Refresh**: Reservations expire automatically when the session epoch rolls forward or after a configurable timeout. Devices then re-reserve before sending more traffic.

This ensures sibling devices cannot accidentally exhaust each other’s budgets and keeps rendezvous counters, FlowBudget facts, and envelope publication in sync.

---

## Social Recovery

### Guardian Structure

Guardians are leaves with role `Guardian`. Each guardian branch carries a threshold policy that references guardian leaves only.

### Recovery Process

Social recovery is a choreography that reads the current tree and verifies the guardian policy. It runs a new threshold signing session where guardians sign a `RecoveryGrant`.

The grant authorizes a change such as:
- `TreeOpKind::AddLeaf` for a replacement device
- `TreeOpKind::ChangePolicy` to rotate the guardian set

### Attestation and Auditability

The choreography outputs the attested tree operation plus a `RecoveryEvidence` fact. The journal applies the operation after reduction validates the signature.

Because every step is recorded as a journal fact, any replica can audit the recovery by replaying the same reduction and verifying the guardian aggregate signature. No off-ledger state is required.

### Guardian Replacement

If a guardian becomes untrusted, the account owner runs a tree operation to replace that guardian leaf and publishes the result. Flow budgets still apply to all envelopes generated during the recovery.

### Privacy Preservation

Privacy is preserved because the recovery facts live under the account namespace and are not gossiped outside authorized contexts. No external service learns which guardians participated.

### Guardian Protocol Details

The guardian choreography relies on the same Noise IKpsk2 handshake used by rendezvous. Each guardian receives the `RecoveryProposal` and decrypts it with the pairwise RID.

Guardians validate the included `TreeSnapshot`. They refuse to participate if the snapshot commitment differs from their local reduction. After verifying, each guardian runs the `GuardianConsent` session which records a share contribution and charges flow budget.

### Implementation Status

- **Reference implementation**: `crates/aura-recovery/src/choreography_impl.rs` implements the `G_recovery` choreography with FlowGuard hints on every send plus `RecoveryEvidence` emission.
- **Cooldown enforcement**: `GuardianProfile::cooldown_secs` and the shared cooldown ledger ensure a guardian cannot approve two back-to-back recoveries. The CLI and agent commands surface cooldown errors explicitly.
- **Operator tooling**: `aura recovery start` and the agent-side `RecoveryOperations` expose start/status flows, while acceptance by guardian devices is deferred to aura-agent.

### Grant Issuance

Once enough shares arrive, the initiator emits the `RecoveryGrant` fact, attaches the aggregated signature, and publishes the desired `TreeOp`.

The grant references the policy node and includes the guardian participant list so audits can check quorum. This flow creates an append-only record of every recovery and keeps the account consistent with the privacy model.

### Guardian Trust Transitions

Guardians can be suspended or rotated when they misbehave:

1. **Suspension Evidence**: Any device can publish `GuardianIncident { guardian_id, context_id, evidence_cid }`. This fact is signed by a device policy threshold and records why the guardian should be paused (non-response, malicious signature, etc.).
2. **Temporary Quorum Adjustment**: When an incident fact exists, the guardian policy reduces the quorum by one via a `TreeOpKind::ChangePolicy` signed by the remaining guardians. This lets the account proceed without the suspect guardian.
3. **Replacement**: The account owner (or a guardian quorum) runs `TreeOpKind::RemoveLeaf` followed by `AddLeaf` to install a replacement guardian, using the same FlowBudget-guarded protocols as any other tree update.
4. **Audit Trail**: All incident facts and policy changes remain in the journal so future devices can audit the guardian history and verify that quorum transitions followed the documented process.

These rules ensure social recovery remains safe even when individual guardians fail or behave maliciously.

---

## Interfaces

The following interfaces summarize the boundary between the identity layer and the rest of the system. They align with the algebraic effect model in `002`.

### Data Structures

```rust
pub struct DeviceLeaf {
    pub leaf_id: LeafId,
    pub device_id: DeviceId,
    pub key_package: Vec<u8>,
    pub flow_limit: u64,
}

pub struct GuardianLeaf {
    pub leaf_id: LeafId,
    pub guardian_id: DeviceId,
    pub attest_key: Vec<u8>,
}

pub struct TreeSnapshot {
    pub commitment: Hash32,
    pub epoch: Epoch,
    pub devices: Vec<DeviceLeaf>,
    pub guardians: Vec<GuardianLeaf>,
    pub policies: Vec<(NodeIndex, Policy)>,
}

pub struct ThresholdSignRequest {
    pub tree_commitment: Hash32,
    pub policy: Policy,
    pub message: [u8; 32],
}

pub struct RecoveryGrant {
    pub tree_commitment: Hash32,
    pub proposed_op: TreeOp,
    pub reason_code: u8,
}

pub struct DKDCapsule {
    pub app_id: String,
    pub context_label: String,
    pub ttl: Option<u64>,
    pub issued_at: u64,
}

pub struct ContextIdentity {
    pub context_id: [u8; 32],
    pub derived_key: Vec<u8>,
    pub capsule_mac: [u8; 32],
}

pub struct PresenceTicket {
    pub issued_by: DeviceId,
    pub expires_at: u64,
    pub capability: Vec<u8>,
}
```

### Effect Trait

```rust
#[async_trait]
pub trait IdentityEffects {
    async fn load_tree(&self) -> Result<TreeSnapshot, IdentityError>;
    async fn propose_tree_op(&self, op: TreeOp) -> Result<(), IdentityError>;
    async fn threshold_sign(&self, req: ThresholdSignRequest) -> Result<ThresholdSignResult, IdentityError>;
    async fn initiate_recovery(&self, grant: RecoveryGrant) -> Result<(), IdentityError>;
    async fn derive_context_identity(&self, capsule: DKDCapsule) -> Result<(ContextIdentity, PresenceTicket), IdentityError>;
}
```

### Method Descriptions

- **load_tree**: Reads the reduced state from the journal
- **propose_tree_op**: Runs the appropriate choreography and writes the attested fact
- **threshold_sign**: Handles aggregate authorizations
- **initiate_recovery**: Executes the guardian workflow
- **derive_context_identity**: Performs DKD, issues a presence ticket tied to the current session epoch, and records any necessary facts

These functions provide the surface needed by higher-level protocols and keep the identity system aligned with the unified calculus and privacy boundaries already defined.

---

## Implementation Notes

### Current Implementation Status

**✅ FROST Threshold Signatures**:
- **FROST implementation**: [`crates/aura-frost/`](../crates/aura-frost/) - Complete threshold signing implementation
  - Distributed keygen: [`src/distributed_keygen.rs`](../crates/aura-frost/src/distributed_keygen.rs)
  - Threshold signing: [`src/threshold_signing.rs`](../crates/aura-frost/src/threshold_signing.rs)
  - Signature aggregation: [`src/signature_aggregation.rs`](../crates/aura-frost/src/signature_aggregation.rs)
  - Key resharing: [`src/key_resharing.rs`](../crates/aura-frost/src/key_resharing.rs)
- **Tree signing integration**: [`crates/aura-crypto/src/frost/tree_signing.rs`](../crates/aura-crypto/src/frost/tree_signing.rs)

**✅ Ratchet Tree & Journal Integration**:
- **Tree operations**: [`crates/aura-core/src/tree.rs`](../crates/aura-core/src/tree.rs) - `TreeOp` and `AttestedOp` types
- **Tree state management**: [`crates/aura-journal/src/ratchet_tree/`](../crates/aura-journal/src/ratchet_tree/) - Complete implementation
- **Journal facts**: [`crates/aura-core/src/journal.rs`](../crates/aura-core/src/journal.rs) - CRDT semilattice implementation

**✅ Deterministic Key Derivation**:
- **DKD implementation**: [`crates/aura-crypto/src/key_derivation.rs`](../crates/aura-crypto/src/key_derivation.rs)
- **Context types**: [`crates/aura-core/src/identifiers.rs`](../crates/aura-core/src/identifiers.rs) - `DkdContextId` and related types

**⚠️ Partial Implementation**:
- **Identity management**: [`crates/aura-identity/`](../crates/aura-identity/) - Crate exists, core protocols under development
- **Guardian ceremonies**: [`crates/aura-recovery/`](../crates/aura-recovery/) - Infrastructure present, choreographies pending
- **Device coordination**: Choreographic protocols defined but not yet fully implemented

**❌ Planned Implementation**:
- Complete device initialization workflows
- Guardian recovery ceremonies with cooldown periods
- Flow budget enforcement integration with transport layer
- Epoch transition choreographies

### API Verification

The described APIs in this document correspond to planned interfaces. Current implementation provides:

**Working Today**:
- FROST threshold signature generation and verification
- Ratchet tree state management and commitment verification
- DKD context identity derivation
- Journal fact storage and semilattice operations

**Under Development**:
- `ThresholdIdentity` trait implementations
- Device coordination choreographies
- Guardian recovery workflows

**Future Work**:
- Complete integration between all components
- Production-ready device initialization
- Advanced privacy features and flow budget enforcement

For current API usage patterns, see [`docs/800_building_on_aura.md`](800_building_on_aura.md) and the implementation in [`crates/aura-identity/src/lib.rs`](../crates/aura-identity/src/lib.rs).

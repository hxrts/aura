# Threshold Identity

An Aura account is a relational identity anchored in the journal as described in [Theoretical Foundations](001_theoretical_foundations.md) and [System Architecture](002_system_architecture.md). Every device and guardian occupies a leaf in a ratchet tree. Every branch stores a threshold policy. The journal stores attested tree operations as facts and capability refinements as caps.

## Account State Management

No device edits account state directly. A device proposes a tree operation through a choreography that runs in `aura-protocol`. The choreography emits exactly one `AttestedOp` or aborts. Reduction of the operation log yields the canonical tree, ensuring deterministic auditable state that all devices can verify independently.

Deterministic Key Derivation binds relationship contexts to the account root so every RID inherits the same graph state. This ensures the social graph is a first-class resource following semilattice rules and privacy guarantees.

The reduced tree exposes three structures. `TreeSnapshot` carries the current commitment, epoch, leaf metadata, and per-branch threshold policy. `DeviceLeaf` stores device identifier, MLS-style key package, and optional flow limit. `GuardianLeaf` stores guardian identifier and attestation key. All values live in journal facts under account namespace and never leave CRDT context so reconciliation always follows monotone rules.

### Deterministic Key Derivation

Every device builds a `DKDCapsule` with application ID, context label, optional TTL, and issue timestamp. The device serializes it to canonical CBOR and computes the context identifier using domain separation tag `aura.context_id.v1`. The device evaluates the DKD function inside the `DeterministicEffects` handler.

DKD multiplies the local FROST share by the capsule hash, clears the cofactor, and feeds the result into HKDF to derive `ContextIdentity`. The handler produces a keyed MAC over the capsule so other devices can verify the derived key without recomputing the operation.

The derived identifier scopes to the account epoch. When the session epoch in the journal bumps, devices must reissue presence tickets so old context identities cannot authenticate.

## Threshold Signature System

The account root key is not a single key but a FROST aggregate derived from the leaves carrying signing authority. Each eligible leaf holds a share. Shares never appear in the journal, keeping cryptographic material isolated from the distributed ledger.

A signing session is expressed as a choreography that references the tree commitment and policy identifier. The session performs share generation, nonce exchange, and final signing. The handler records `ThresholdSignResult` with tree commitment, policy ID, aggregate signature, and witness. Any verifier can check that the aggregate signature corresponds to a policy in the reduced tree.

Honest devices refuse to sign if the tree commitment presented does not match their locally reduced state. This prevents forked policies from producing signatures. The signing choreography is the only interface producing account-level authorizations like ledger checkpoints, DKD attestations, or guardian votes. All other flows reference those attestations as facts.

All threshold work uses Ed25519-FROST with deterministic nonce derivation. The choreography exposes three phases. NonceExchange broadcasts binding commitments and charges the flow ledger for every participant with spam control applied. ShareSign computes partial signatures and returns them to the leader. Aggregate verifies each share against published commitments, aggregates them, and emits the `ThresholdSignResult`.

Each phase runs inside a session type that enforces ordering and ensures devices cannot skip guards. The final aggregate signature binds to the tree commitment hash and policy node identifier so replaying a signature under different tree state fails verification.

## Multi-Device Coherence

Accounts often run on several devices with each device a leaf with role `Device` in the ratchet tree. Adding or removing a device is a `TreeOpKind::AddLeaf` or `TreeOpKind::RemoveLeaf` fact. Devices learn about changes through journal reduction.

Every device tracks the account epoch, per-context rendezvous counter, and flow-budget ledger. This local state enables devices to verify updates and enforce spam control independently. When a new device boots, it merges the latest journal snapshot, verifies the tree commitment against the attested root, and requests encrypted relationship keys from peers.

Transport modules rely on the same flow-budget interface defined in [Information Flow Budget](103_info_flow_budget.md). A device cannot publish envelopes or handshakes if the ledger reports exhausted budget for that context. This keeps spam control consistent even when multiple devices act in parallel.

Multiple devices under the same account share per-context `FlowBudget` facts. The identity layer maintains a per-account allocator to avoid double-spending. Before publishing any envelope or rendezvous descriptor, a device requests a reservation token from the allocator stored in the journal as `FlowReservation`. Only one reservation per context and device is outstanding at a time.

The device passes the reservation token to `FlowGuard` which charges the global context and neighbor ledger entry. If the global limit is reached the reservation fails and the device backs off. Reservations expire automatically when the session epoch rolls forward or after a configurable timeout. Devices then re-reserve before sending more traffic. This ensures sibling devices cannot accidentally exhaust each other's budgets.

## Social Recovery

Guardians are leaves with role `Guardian` with each guardian branch carrying a threshold policy referencing guardian leaves only. Social recovery is a choreography that reads the current tree, verifies the guardian policy, and runs a new threshold signing session where guardians sign a `RecoveryGrant`. The grant authorizes changes like adding a replacement device or rotating the guardian set.

The choreography outputs the attested tree operation plus a `RecoveryEvidence` fact. The journal applies the operation after reduction validates the signature. Because every step is recorded as a journal fact, any replica can audit the recovery by replaying the same reduction and verifying the guardian aggregate signature. No off-ledger state is required.

Privacy is preserved because recovery facts live under the account namespace and are not gossiped outside authorized contexts. The recovery choreography relies on the same Noise IKpsk2 handshake used by rendezvous. Each guardian receives the `RecoveryProposal` and decrypts it with the pairwise RID.

Guardians validate the included `TreeSnapshot` and refuse to participate if the snapshot commitment differs from their local reduction. After verifying, each guardian runs the `GuardianConsent` session which records a share contribution and charges flow budget. Once enough shares arrive, the initiator emits the `RecoveryGrant` fact, attaches the aggregated signature, and publishes the desired `TreeOp`. The grant references the policy node and includes the guardian participant list so audits can check quorum.

## Guardian Trust Transitions

Guardians can be suspended or rotated when they misbehave. Any device can publish `GuardianIncident` with guardian ID, context ID, and evidence CID. This fact is signed by a device policy threshold and records why the guardian should be paused.

When an incident fact exists, the guardian policy reduces the quorum by one via `TreeOpKind::ChangePolicy` signed by the remaining guardians. This lets the account proceed without the suspect guardian. The account owner or guardian quorum then runs `TreeOpKind::RemoveLeaf` followed by `AddLeaf` to install a replacement guardian using the same FlowBudget-guarded protocols as any other tree update.

All incident facts and policy changes remain in the journal so future devices can audit the guardian history and verify that quorum transitions followed documented process. These rules ensure social recovery remains safe even when individual guardians fail or behave maliciously.

## System Implementation

FROST threshold signature implementation in `crates/aura-frost/` provides distributed keygen, threshold signing, signature aggregation, and key resharing. Ratchet tree and journal integration in `crates/aura-journal/src/ratchet_tree/` delivers comprehensive state management. Deterministic key derivation in `crates/aura-crypto/src/key_derivation.rs` supports all identity contexts.

Identity management in `crates/aura-verify/` provides core protocol implementations. Guardian ceremonies in `crates/aura-recovery/` include comprehensive infrastructure with choreographic protocols. Device coordination uses choreographic protocol definitions for multi-party coordination.

The system supports device initialization workflows, guardian recovery ceremonies with cooldown periods, flow budget enforcement integration with transport layer, and epoch transition choreographies.

The implementation provides FROST threshold signature generation and verification, ratchet tree state management and commitment verification, DKD context identity derivation, and journal fact storage with semilattice operations. The `ThresholdIdentity` trait implementations, device coordination choreographies, and guardian recovery workflows are fully functional.

See [Building on Aura](800_building_on_aura.md) for current API usage patterns and implementation in `crates/aura-verify/src/lib.rs`.

# Authorization Pipeline

## Overview

Aura authorizes every observable action through Biscuit capability evaluation combined with sovereign policy and flow budgets. The authorization pipeline spans `AuthorizationEffects`, the guard chain, and receipt accounting. This document describes the data flow and integration points so that all crates implement the same procedure.

## Biscuit Capability Model

Biscuit tokens encode attenuation chains. Each attenuation step applies additional caveats that shrink authority through meet composition. Aura stores Biscuit material outside the replicated CRDT. Local runtimes evaluate tokens at send time and cache the resulting lattice element for the active `ContextId`. Cached entries expire on epoch change or when policy revokes a capability. Policy data always participates in the meet so a token can only reduce authority relative to local policy.

```text
Evaluation(frontier, token):
1. Parse token and verify signature chain against authority public key.
2. Apply each caveat in order and intersect the result with `frontier`.
3. Intersect the result with sovereign policy for the target context.
4. Return the resulting `Cap` element for guard evaluation.
```

This algorithm produces a meet-monotone capability frontier. Step 1 ensures provenance. Steps 2 and 3 ensure no evaluation widens authority. Step 4 feeds the guard chain with a cached outcome.

## Guard Chain

Authorization evaluation feeds the transport guard chain. All documents reference this section to avoid divergence.

```mermaid
flowchart LR
    A[Send request] --> B[CapGuard<br/>Biscuit evaluation];
    B --> C[FlowGuard<br/>budget charge];
    C --> D[JournalCoupler<br/>fact commit];
    D --> E[Transport send];
```

CapGuard invokes `AuthorizationEffects` with the cached frontier and any inline Biscuit token on the message. FlowGuard charges the `FlowBudget` fact for `(ContextId, peer)` and produces a signed receipt. JournalCoupler merges any journal deltas atomically with the send. Only after all guards pass does transport emit a packet. Any failure returns locally and leaves no observable side effect.

## Worked Example

```rust
async fn send_storage_put(
    authz: &dyn AuthorizationEffects,
    flow: &dyn FlowEffects,
    journal: &dyn JournalEffects,
    transport: &dyn TransportEffects,
    ctx: ContextId,
    peer: AuthorityId,
    token: Biscuit,
    payload: PutRequest,
) -> Result<()> {
    let cap_frontier = authz.evaluate_guard(token, CapabilityPredicate::StorageWrite)?;
    authz.ensure_allowed(&cap_frontier, CapabilityPredicate::StorageWrite)?;
    let receipt = flow.charge(ctx, peer, payload.flow_cost)?;
    journal.merge_facts(payload.delta.copy_with_receipt(receipt));
    transport.send(peer, Msg::new(ctx, payload, cap_frontier.summary()))?;
    Ok(())
}
```

This example evaluates a token, ensures the guard predicate holds, charges flow budget, merges the journal delta that records the receipt, and finally sends the message. Each interface call matches an effect trait so the same order applies across runtimes.

## Failure Handling and Caching

Runtimes cache evaluated capability frontiers per `(ContextId, CapabilityPredicate)` with an epoch tag. Cache entries invalidate when journal policy facts change or when the epoch rotates. CapGuard failures return `AuthorizationError::Denied` without charging flow or touching the journal. FlowGuard failures return `FlowError::InsufficientBudget` without emitting transport traffic. JournalCoupler failures surface as `JournalError::CommitAborted`, which instructs the protocol to retry after reconciling journal state. This isolation keeps the guard chain deterministic and side-channel free.

## Dual Authorization Modes

Aura implements two complementary authorization mechanisms: capability semilattice and Biscuit tokens. This dual approach provides both performance (local checks) and security (cryptographic verification).

### Mode 1: Capability Semilattice (Local)

The capability semilattice provides fast, local authorization checks through meet-based refinement.

**Mathematical Foundation:**
```text
Capabilities form a meet-semilattice (C, ⊓, ⊤) where:
- x ⊓ y = y ⊓ x (commutative)
- x ⊓ (y ⊓ z) = (x ⊓ y) ⊓ z (associative)
- x ⊓ x = x (idempotent)
- refine_caps(c) never increases authority (monotonic)
```

**Implementation:**
```rust
use aura_wot::{CapabilitySet, Capability};
use aura_core::semilattice::MeetSemiLattice;

// Create capability set
let caps = CapabilitySet::from_permissions(&[
    "read:storage/*",
    "write:storage/user/*",
    "execute:sync",
]);

// Refinement via meet operation
let sovereign_policy = CapabilitySet::from_permissions(&[
    "read:storage/*",  // Allows reads
    // No write permission in policy
]);

let effective_caps = caps.meet(&sovereign_policy);
// Result: Only read:storage/* (write permission removed by meet)

// Check permission
if effective_caps.permits("read:storage/file.txt") {
    // Authorized
}
```

**Characteristics:**
- **Fast**: O(log n) meet operation, no cryptographic verification
- **Local**: Evaluated entirely on local device
- **Monotonic**: Refinement only reduces authority, never increases
- **Policy Integration**: Sovereign policy always participates in meet

**Use Cases:**
- Local device operations
- Fast path authorization checks
- Policy enforcement
- Debugging and testing

### Mode 2: Biscuit Tokens (Cryptographic)

Biscuit tokens provide cryptographically verifiable, attenuated delegation chains.

**Implementation:**
```rust
use aura_wot::{AccountAuthority, BiscuitTokenManager};
use biscuit_auth::{Biscuit, Authorizer, macros::*};

// 1. Create root authority
let authority = AccountAuthority::new(account_id);

// 2. Issue device token
let device_token = authority.create_device_token(device_id)?;
// Contains: account({id}), device({id}), role("owner"), capability("read"), ...

// 3. Attenuate for delegation
let manager = BiscuitTokenManager::new(device_id, device_token);
let read_only_token = manager.attenuate_read("storage/public/*")?;
// Adds: check if operation("read"), check if resource($res), $res.starts_with("storage/public/")

// 4. Verify token
let mut authorizer = Authorizer::new();
authorizer.add_token(&read_only_token)?;
authorizer.add_fact(fact!("operation(\"read\")"))?;
authorizer.add_fact(fact!("resource(\"storage/public/file.txt\")"))?;

if authorizer.authorize().is_ok() {
    // Cryptographically verified authorization
}
```

**Attenuation Chain:**
```text
Root Authority Token
       ↓ (append block with caveats)
Device Token (capability="read", capability="write")
       ↓ (append check if operation("read"))
Read-Only Token
       ↓ (append check if resource starts_with("public/"))
Public Read-Only Token
```

**Characteristics:**
- **Secure**: Cryptographic signature chain
- **Delegatable**: Attenuation preserves signature chain
- **Offline**: Recipients verify without contacting issuer
- **Non-Repudiable**: Signatures prove token lineage
- **Revocable**: Epoch rotation invalidates old tokens

**Use Cases:**
- Cross-authority delegation
- API access tokens
- Guardian authorization
- Capability forwarding
- Audit trails

### Integration Pattern

Both modes integrate through the guard chain:

```rust
async fn authorize_and_send(
    authz: &dyn AuthorizationEffects,
    flow: &dyn FlowEffects,
    journal: &dyn JournalEffects,
    transport: &dyn TransportEffects,
    ctx: ContextId,
    peer: AuthorityId,
    token: Option<Biscuit>, // Optional Biscuit token
    operation: Operation,
) -> Result<()> {
    // Phase 1: Biscuit token verification (if provided)
    let biscuit_cap = if let Some(token) = token {
        // Cryptographic verification
        authz.evaluate_biscuit_token(token, &operation.predicate())?
    } else {
        // No token - use default
        CapabilitySet::empty()
    };

    // Phase 2: Capability semilattice evaluation
    let local_caps = authz.get_local_capabilities(ctx)?;
    let sovereign_policy = authz.get_sovereign_policy(ctx)?;

    // Meet operation: token ⊓ local ⊓ policy
    let effective_caps = biscuit_cap
        .meet(&local_caps)
        .meet(&sovereign_policy);

    // Phase 3: Guard chain
    authz.ensure_allowed(&effective_caps, operation.predicate())?;
    let receipt = flow.charge(ctx, peer, operation.flow_cost)?;
    journal.merge_facts(operation.delta_with_receipt(receipt));
    transport.send(peer, operation.message())?;

    Ok(())
}
```

### Decision Matrix: When to Use Each Mode

| Scenario | Capability Semilattice | Biscuit Tokens | Both |
|----------|------------------------|----------------|------|
| Local device operations | ✓ | | |
| Cross-authority delegation | | ✓ | |
| Policy enforcement | ✓ | | |
| API access control | | ✓ | |
| Guardian recovery | | ✓ | |
| Storage operations | ✓ | | ✓ |
| Fast path checks | ✓ | | |
| Audit requirements | | ✓ | |
| Relaying/forwarding | | ✓ | |
| Offline verification | | ✓ | |

### Performance Characteristics

**Capability Semilattice:**
- Meet operation: O(log n) in capability set size
- Permission check: O(log n) lookup
- Cache friendly: Results cacheable per (ContextId, Predicate)
- No network: Fully local evaluation

**Biscuit Tokens:**
- Signature verification: O(chain length) cryptographic operations
- Authorization: O(facts × rules) datalog evaluation
- Attenuation: O(1) to append block
- Verification: Requires public key

### Best Practices

**Capability Semilattice:**
1. Always participate sovereign policy in meet operation
2. Cache effective capabilities per context
3. Invalidate cache on epoch rotation
4. Use for hot path authorization checks
5. Normalize contradictory states (e.g., All + None)

**Biscuit Tokens:**
1. Minimize attenuation chain length (each block adds verification cost)
2. Use datalog checks for complex constraints
3. Include authority_id fact in root authority block
4. Rotate tokens on epoch change
5. Store only serialized tokens, not sensitive key material
6. Verify tokens once, cache result with epoch tag

**Integration:**
1. Always use meet operation to combine modes: `biscuit_cap ⊓ local_cap ⊓ policy`
2. Fail authorization if either mode denies (conservative)
3. Cache combined results per (ContextId, TokenHash, Predicate)
4. Invalidate cache on epoch rotation or policy update
5. Log authorization decisions for audit

### Security Considerations

**Capability Semilattice:**
- Assumes trusted local policy store
- No protection against local policy tampering
- Meets ensure refinement never widens authority
- Policy changes require epoch rotation for cross-device effect

**Biscuit Tokens:**
- Protects against token forgery via signature verification
- Vulnerable to token replay (mitigated by epoch scoping)
- Attenuation chains grow with each delegation
- Root key compromise invalidates all derived tokens
- No built-in revocation (handled via epoch rotation)

**Combined:**
- Meet operation ensures conservative authorization
- Epoch rotation provides global token invalidation
- Sovereign policy overrides even valid Biscuit tokens
- Guard chain failures leak no information

### Implementation References

- **Capability Semilattice**: `aura-wot/src/capability.rs` - CapabilitySet, meet operations
- **Biscuit Integration**: `aura-wot/src/biscuit_token.rs` - AccountAuthority, attenuation
- **Guard Chain**: `aura-protocol/src/guards/` - CapGuard, FlowGuard, JournalCoupler
- **Authorization Effects**: `aura-core/src/effects/authorization.rs` - AuthorizationEffects trait
- **Example Usage**: `aura-store/src/biscuit_authorization.rs` - Storage authorization

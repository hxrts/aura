# Authorization

## Overview

Aura authorizes every observable action through Biscuit capability evaluation combined with sovereign policy and [flow budgets](109_transport_and_information_flow.md). The authorization pipeline spans `AuthorizationEffects`, the guard chain, and receipt accounting. This document describes the data flow and integration points so that all crates implement the same procedure.

## Biscuit Capability Model

Biscuit tokens encode attenuation chains. Each attenuation step applies additional caveats that shrink authority through meet composition. Aura stores Biscuit material outside the replicated CRDT. Local runtimes evaluate tokens at send time and cache the resulting lattice element for the active `ContextId`.

Cached entries expire on epoch change or when policy revokes a capability. Policy data always participates in the meet. A token can only reduce authority relative to local policy.

```text
Evaluation(frontier, token):
1. Parse token and verify signature chain against authority public key.
2. Apply each caveat in order and intersect the result with `frontier`.
3. Intersect the result with sovereign policy for the target context.
4. Return the resulting `Cap` element for guard evaluation.
```

This algorithm produces a meet-monotone capability frontier. Step 1 ensures provenance. Steps 2 and 3 ensure evaluation never widens authority. Step 4 feeds the guard chain with a cached outcome.

## Guard Chain

Authorization evaluation feeds the transport guard chain. All documents reference this section to avoid divergence.

```mermaid
flowchart LR
    A[Send request] --> B[CapGuard<br/>Biscuit evaluation];
    B --> C[FlowGuard<br/>budget charge];
    C --> D[JournalCoupler<br/>fact commit];
    D --> E[Transport send];
```

Guard evaluation is pure and synchronous over a prepared `GuardSnapshot`. CapGuard reads the cached frontier and any inline Biscuit token already present in the snapshot. FlowGuard and JournalCoupler emit `EffectCommand` items (charges, receipts, fact commits, transport intents) rather than executing I/O directly. An async interpreter executes those commands in production or simulation.

Only after all guards pass does transport emit a packet. Any failure returns locally and leaves no observable side effect.

DKG payloads require special budget handling because dealer packages and transcript exchanges are large. Flow and leakage budgets must be charged proportionally to payload size before any transport send. This ensures DKG traffic cannot bypass the guard chain and prevents unaccounted leakage during fast-path coordination.

## Failure Handling and Caching

Runtimes cache evaluated capability frontiers per `(ContextId, CapabilityPredicate)` with an epoch tag. Cache entries invalidate when journal policy facts change or when the epoch rotates.

CapGuard failures return `AuthorizationError::Denied` without charging flow or touching the journal. FlowGuard failures return `FlowError::InsufficientBudget` without emitting transport traffic. JournalCoupler failures surface as `JournalError::CommitAborted`. This error instructs the protocol to retry after reconciling journal state.

This isolation keeps the guard chain deterministic and side-channel free.

## Biscuit Token Authorization

Aura implements cryptographically secure authorization using Biscuit tokens. This unified approach provides strong security through cryptographic verification while maintaining efficient authority-centric authorization.

### Biscuit Token Implementation

Biscuit tokens provide cryptographically verifiable, attenuated delegation chains. The following code shows a typical implementation workflow:

```rust
use aura_authorization::{AccountAuthority, BiscuitTokenManager, ResourceScope, AuthorityOp};
use aura_protocol::authorization::BiscuitAuthorizationBridge;
use biscuit_auth::{Biscuit, Authorizer, macros::*};
use aura_core::{AuthorityId, DeviceId};

// 1. Create root authority
let authority = AccountAuthority::new(account_id);

// 2. Issue device token
let device_token = authority.create_device_token(device_id)?;
// Contains: account({id}), device({id}), role("owner"), capability("read"), ...

// 3. Attenuate for delegation
let manager = BiscuitTokenManager::new(device_id, device_token);
let read_only_token = manager.attenuate_read("storage/public/*")?;
// Adds: check if operation("read"), check if resource($res), $res.starts_with("storage/public/")

// 4. Authorization via Bridge
let bridge = BiscuitAuthorizationBridge::new(authority.root_public_key(), device_id);
let resource_scope = ResourceScope::Storage {
    authority_id: AuthorityId::new_from_entropy([1u8; 32]),
    path: "public/file.txt".to_string(),
};

let result = bridge.authorize(&read_only_token, "read", &resource_scope)?;
if result.authorized {
    // Cryptographically verified authorization with delegation depth tracking
}
```

Tokens follow an attenuation chain where each block adds restrictions. The sequence is:

```text
Root Authority Token
       ↓ (append block with caveats)
Device Token (capability="read", capability="write")
       ↓ (append check if operation("read"))
Read-Only Token
       ↓ (append check if resource starts_with("public/"))
Public Read-Only Token
```

Biscuit tokens have the following characteristics:
- Secure: Cryptographic signature chain prevents forgery
- Delegatable: Attenuation preserves signature chain
- Offline: Recipients verify without contacting issuer
- Non-Repudiable: Signatures prove token lineage
- Revocable: Epoch rotation invalidates old tokens

Typical use cases for Biscuit tokens include cross-authority delegation where one authority delegates to another. API access tokens can be issued to applications with specific scopes. Guardian authorization uses tokens to prove recovery permissions. Capability forwarding passes attenuated tokens to other parties. Audit trails are built from token usage logs and verification records.

### Guard Chain Integration

Biscuit authorization integrates seamlessly with the guard chain:

```rust
async fn authorize_and_send(
    bridge: &BiscuitAuthorizationBridge,
    interpreter: &dyn EffectInterpreter,
    guards: &GuardChain,
    ctx: ContextId,
    peer: AuthorityId,
    token: &Biscuit, // Required Biscuit token
    operation: Operation,
    resource_scope: &ResourceScope,
) -> Result<()> {
    // Phase 1: Cryptographic token verification and Datalog evaluation
    let auth_result = bridge.authorize(token, operation.name(), resource_scope)?;
    if !auth_result.authorized {
        return Err(AuraError::permission_denied("Token authorization failed"));
    }

    // Phase 2: Prepare snapshot (async) and evaluate guards (sync)
    let snapshot = prepare_guard_snapshot(ctx, peer, &auth_result.cap_frontier).await?;
    let outcome = guards.evaluate(&snapshot, &operation.request());
    if outcome.decision.is_denied() {
        return Err(AuraError::permission_denied("Guard evaluation denied"));
    }

    // Phase 3: Execute effect commands (async)
    for cmd in outcome.effects {
        interpreter.exec(cmd).await?;
    }

    Ok(())
}
```

### Authorization Scenarios

Biscuit tokens handle all authorization scenarios through cryptographic verification:

| Scenario | Implementation |
|----------|----------------|
| Local device operations | Device tokens with full capabilities |
| Cross-authority delegation | Attenuated tokens with resource restrictions |
| Policy enforcement | Sovereign policy integrated into Datalog evaluation |
| API access control | Scoped tokens with operation restrictions |
| Guardian recovery | Guardian tokens with recovery capabilities |
| Storage operations | Storage-scoped tokens with path restrictions |
| Relaying/forwarding | Context tokens with relay permissions |
| Offline verification | Self-contained cryptographic verification |

### Performance Characteristics

Biscuit token authorization has predictable performance characteristics across all operations. Signature verification requires O(chain length) cryptographic operations proportional to the attenuation depth. Authorization evaluation takes O(facts × rules) time for Datalog evaluation over ambient and token facts. Attenuation is efficient at O(1) cost to append blocks to the token. Verification uses cryptographic proofs with embedded public keys for offline checking. Token results are cacheable with epoch-based invalidation to avoid repeated evaluation. Delegation depth is efficiently tracked via block count for security monitoring.

### Best Practices

Biscuit Token Management requires careful attention to several factors. Minimize attenuation chain length because each block adds verification cost. Use Datalog checks for complex resource constraints. Include authority_id fact in root authority block to ensure proper scoping. Rotate tokens on epoch change to maintain security and prevent stale tokens. Store only serialized tokens and never persist sensitive key material. Cache authorization results with epoch-based invalidation to improve performance.

Resource Scope Design should follow authority-centric patterns. Use authority-centric ResourceScope for clear ownership and prevent confusion about who controls resources. Structure paths hierarchically for efficient pattern matching in Datalog evaluation. Separate Authority operations from Context operations to maintain clear boundaries. Design resource patterns for effective attenuation so that scope restrictions are easy to verify.

Authorization Pipeline implementation must follow a strict sequence:
1. Always verify tokens cryptographically before capability checks
2. Integrate sovereign policy into Datalog evaluation
3. Cache authorization results per (AuthorityId, TokenHash, ResourceScope)
4. Invalidate cache on epoch rotation or policy update
5. Track delegation depth for security monitoring
6. Log all authorization decisions for audit trails

### Security Considerations

Biscuit Token Security:

Forgery Protection: Cryptographic signature verification prevents token forgery.

Replay Mitigation: Epoch scoping limits token lifetime and replay attacks.

Delegation Chains: Attenuation preserves security while growing verification cost.

Root Key Security: Root key compromise invalidates all derived tokens. Secure storage is critical.

Revocation: Handled through epoch rotation. No individual token revocation exists.

Offline Verification: Self-contained tokens enable secure offline authorization.

Authority-Centric Security:

Resource Isolation: Authority-based ResourceScope prevents cross-authority access.

Sovereign Policy: Local policy integration provides additional security layer.

Delegation Depth: Monitored to prevent excessive delegation chains.

Epoch Invalidation: Global token invalidation occurs on epoch rotation.

Guard Chain Isolation: Authorization failures leak no sensitive information.

Audit Trail: All authorization decisions are logged for security monitoring.

### Implementation References

Core Cap Type: `aura-core/src/domain/journal.rs` contains the `Cap` type, which wraps serialized Biscuit tokens with optional root key storage. The `Cap::meet()` implementation computes capability intersection. Tokens from the same issuer return the more attenuated token (more blocks means less authority). Tokens from different issuers return bottom (empty). Use `Cap::from_biscuit_with_key()` to enable proper meet semantics.

Biscuit Authorization Bridge: `crates/aura-guards/src/authorization.rs` contains `BiscuitAuthorizationBridge`.

Biscuit Token Management: `aura-authorization/src/biscuit_token.rs` contains `AccountAuthority`, `BiscuitTokenManager`, and attenuation logic.

Biscuit Authorization: `aura-authorization/src/biscuit_authorization.rs` contains `BiscuitAuthorizationBridge` and `AuthorizationResult`.

Authority-Based Resources: `aura-authorization/src/resource_scope.rs` contains `ResourceScope`, `AuthorityOp`, and `ContextOp`.

Guard Chain: `crates/aura-guards/src/guards/capability_guard.rs` contains `CapabilityGuard` for Biscuit integration.

Storage Authorization: `aura-authorization/src/storage_authorization.rs` contains storage-specific Biscuit authorization (moved from aura-store).

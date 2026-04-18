# Aura Authorization (Layer 2)

## Purpose

Define authorization semantics and capability refinement using Biscuit tokens
for cryptographically verifiable capability delegation, explicit issuance
profiles, and evaluated capability frontiers.

## Scope

| Belongs here | Does not belong here |
|-------------|---------------------|
| Biscuit token model and verification semantics | Cryptographic signing (use aura-signature) |
| Explicit token grant profile expansion | Owning first-party capability families (those stay in the owning crates) |
| Authorization handler: `WotAuthorizationHandler` | Transport operations (use effect traits) |
| Fact types: `WotFact`, `ProposalFact` | Runtime handler composition |
| Flow budget handler: `JournalBackedFlowBudgetHandler` | |
| Storage authorization: `StoragePermission`, `AccessDecision` | |
| Policy evaluation (pure; I/O via effects) | |

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| Inbound | `aura-core` | Domain types, effect traits, resource scopes |

## Invariants

- Authority-centric resource scopes (AuthorityOp, ContextOp).
- Capability refinement via meet-semilattice: `C₁ ⊓ C₂ ≤ min(C₁, C₂)`.
- Biscuit tokens for cryptographic delegation.
- Issuance profiles are explicit and reviewable; there is no implicit
  "grant every declared capability" path.
- Evaluated frontiers in guard snapshots are distinct from issuance profiles and
  declared capability families.
- Policies are Datalog-based for flexible evaluation.

### InvariantCapabilityMeetMonotonicity

Capability refinement must be monotone in the meet semilattice and remain context scoped.

Enforcement locus:
- src capability evaluators compute intersections for delegation and attenuation.
- Biscuit validation paths enforce cryptographic token constraints.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just test-crate aura-authorization

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) defines meet monotonicity.
- [Privacy and Information Flow Contract](../../docs/003_information_flow_contract.md) depends on capability checks before send.

## Ownership Model

> Taxonomy: [Ownership Model](../../docs/122_ownership_model.md)

`aura-authorization` is primarily `Pure`. Capability and policy semantics do not require `ActorOwned` runtime state. Transfer or attenuation semantics remain explicit and `MoveOwned`. `Observed` layers may inspect authorization results but must not invent their own authority.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| `src/effect_policy.rs`, `src/facts.rs`, `src/flow_budget.rs`, `src/view.rs` | `Pure` | Generic capability families, effect timing semantics, fact reduction, and derived authorization state. |
| `src/storage_authorization.rs` | `Pure`, `MoveOwned` | Storage-token and budget handling remain synchronous and typed; no async owner state or runtime locks. |
| `src/effects.rs` | `Pure` | Authorization effect contracts and pure capability-facing adapters. |
| Actor-owned runtime state | none | Layer 2 authorization must not accumulate background owner tasks. |
| Observed-only surfaces | none | Observation belongs in higher layers that consume authorization results. |

### Capability-Gated Points

- Biscuit validation and attenuation issuance
- Storage authorization admission and budget charging
- Capability evaluation surfaces consumed by higher-layer mutation/publication gates

## Testing

### Strategy

aura-authorization is security-critical — if capability attenuation widens scope or cross-authority tokens are accepted, privilege escalation is possible. Testing priorities:

1. **Cross-authority token rejection**: tokens signed by one root key must fail verification against a different root key
2. **Attenuation monotonicity**: attenuated tokens must never grant more than the base token
3. **Policy evaluation**: Datalog rules must deny missing/insufficient capabilities
4. **Scope isolation**: resource scopes must bind to the correct authority

### Commands

```
cargo test -p aura-authorization --test contracts  # authorization contracts
cargo test -p aura-authorization --lib             # inline unit tests
```

### Coverage matrix

| What breaks if wrong | Test location | Status |
|---------------------|--------------|--------|
| Cross-authority token accepted | `tests/contracts/authorization_isolation.rs` | covered |
| Attenuation widens scope (read→write) | `tests/contracts/token_attenuation.rs` | covered |
| Double attenuation restores capabilities | `tests/contracts/authorization_isolation.rs` | covered |
| Missing capability authorized | `tests/contracts/authorization_isolation.rs` | covered |
| Read capability implies write | `tests/contracts/authorization_isolation.rs` | covered |
| Token roundtrip breaks verification | `src/storage_authorization.rs` inline | covered |
| Permission string mapping wrong | `src/storage_authorization.rs` inline | covered |
| Scope conversion loses authority binding | `src/storage_authorization.rs` inline | covered |
| Flow cost calculation wrong | `src/storage_authorization.rs` inline | covered |

## References

- [Authorization & Biscuit](../../docs/106_authorization.md)
- [Theoretical Model](../../docs/002_theoretical_model.md)
- [Privacy and Information Flow Contract](../../docs/003_information_flow_contract.md)

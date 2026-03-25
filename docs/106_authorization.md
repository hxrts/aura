# Authorization

## Overview

Aura authorizes every observable action through Biscuit capability evaluation combined with sovereign policy and flow budgets. The authorization pipeline spans `AuthorizationEffects`, the guard chain, and receipt accounting. This document describes the data flow and integration points.

## Canonical Capability Vocabulary

Aura uses one canonical capability vocabulary based on validated
`CapabilityName` values.

- First-party capabilities are declared in the crate that owns the behavior,
  using typed capability families generated from `#[capability_family(...)]`.
- Token issuance uses explicit grant profiles or explicit validated
  `CapabilityName` sets at the issuance boundary. There is no implicit
  "grant every declared capability" path.
- Guard snapshots carry evaluated frontiers only. They do not carry declared
  families, candidate sets, or fallback broad grants.
- Raw capability strings are admitted only at explicit parsing boundaries such
  as Biscuit decoding and choreography DSL parsing, where invalid values fail
  closed.

Out-of-tree modules follow the same shape, but their capability families must
be declared in admitted module manifests rather than handwritten in host
runtime code.

## Biscuit Capability Model

Biscuit tokens encode attenuation chains. Each attenuation step applies
additional caveats that shrink authority through meet composition. Aura stores
Biscuit material outside the replicated CRDT. Local runtimes evaluate tokens at
send time against typed candidate sets supplied by the owning domain and cache
the resulting lattice element for the active `ContextId`.

Cached entries expire on epoch change or when policy revokes a capability. Policy data always participates in the meet. A token can only reduce authority relative to local policy.

```mermaid
flowchart LR
    A[frontier, token] -->|verify signature| B[parsed token]
    B -->|apply caveats| C[frontier ∩ caveats]
    C -->|apply policy| D[result ∩ policy]
    D -->|return| E[Cap element]
```

This algorithm produces a meet-monotone capability frontier. Step 1 ensures provenance. Steps 2 and 3 ensure evaluation never widens authority. Step 4 feeds the guard chain with a cached outcome.

## Guard Chain

Authorization evaluation feeds the transport guard chain. All documents reference this section to avoid divergence.

```mermaid
flowchart LR
    A[Send request] --> B[CapGuard]
    B --> C[FlowGuard]
    C --> D[JournalCoupler]
    D --> E[Transport send]
```

This diagram shows the guard chain sequence. CapGuard performs Biscuit evaluation. FlowGuard charges the budget. JournalCoupler commits facts before transport.

Guard evaluation is pure and synchronous over a prepared `GuardSnapshot`.
CapGuard reads an evaluated frontier and any inline Biscuit token already
present in the snapshot. Snapshot builders may begin with a typed candidate
set, but they must evaluate that set against the Biscuit/policy frontier before
publishing capabilities into the snapshot. FlowGuard and JournalCoupler emit
`EffectCommand` items rather than executing I/O directly. An async interpreter
executes those commands in production or simulation.

Only after all guards pass does transport emit a packet. Any failure returns locally and leaves no observable side effect. DKG payloads require proportional budget charges before any transport send.

## Telltale Integration

Aura uses Telltale runtime admission and VM guard checkpoints. Runtime admission gates whether a runtime profile may execute. VM acquire and release guards gate per-session resource leases inside VM execution. The Aura guard chain remains the authoritative policy and accounting path for application sends.

Failure handling is layered. Admission failure rejects engine startup. VM acquire deny blocks the guarded VM action. Aura guard-chain failure denies transport and returns deterministic effect errors.

## Runtime Capability Admission

Aura uses a dedicated admission surface for theorem-pack and runtime capability checks before choreography execution. `RuntimeCapabilityEffects` in `aura-core` defines capability inventory queries and admission checks. `RuntimeCapabilityHandler` in `aura-effects` stores a boot-time immutable capability snapshot. The `aura-protocol::admission` module declares protocol requirements and maps them to capability keys.

Current protocol capability keys include `byzantine_envelope` for consensus ceremony admission, `termination_bounded` for sync epoch-rotation admission, `reconfiguration` for dynamic topology transfer paths, and `mixed_determinism` for cross-target mixed lanes.

Execution order is runtime capability admission first, then VM profile gates, then the Aura guard chain. Admission diagnostics must respect Aura privacy constraints. Production runtime paths must not emit plaintext capability inventory events. Admission failures use redacted capability references.

## Failure Handling and Caching

Runtimes cache evaluated capability frontiers per context and predicate with an epoch tag. Cache entries invalidate when journal policy facts change or when the epoch rotates.

CapGuard failures return `AuthorizationError::Denied` without charging flow or touching the journal. FlowGuard failures return `FlowError::InsufficientBudget` without emitting transport traffic. JournalCoupler failures surface as `JournalError::CommitAborted` and instruct the protocol to retry after reconciling journal state.

This isolation keeps the guard chain deterministic and side-channel free.

## Biscuit Token Workflow

Biscuit tokens guarantee cryptographically verifiable, attenuated delegation chains. Each token carries a signature chain that prevents forgery and supports offline verification without contacting the issuer. Attenuation is monotone: each delegation step can only reduce authority, never widen it. Epoch rotation provides revocation by invalidating old tokens.

Issuance is explicit. The runtime selects a reviewed token grant profile and
materializes a concrete `Vec<CapabilityName>` at the issuance boundary. That
issuance profile is separate from the evaluated frontier that later appears in
guard snapshots. The two must not be conflated: profiles declare what may be
granted, while snapshots publish what is currently admitted after Biscuit and
policy evaluation.

See [Effects and Handlers Guide](802_effects_guide.md) for Biscuit workflow implementation.

## Guard Chain Integration

Biscuit authorization integrates with the guard chain through three phases: cryptographic verification, synchronous guard evaluation over a prepared `GuardSnapshot`, and effect command interpretation. If any phase fails, the operation returns an error without observable side effects.

See [Effects and Handlers Guide](802_effects_guide.md) for guard chain integration patterns.

## Authorization Scenarios

All authorization scenarios (local device operations, cross-authority delegation, API access control, guardian recovery, storage, and relaying) are handled through Biscuit token attenuation and sovereign policy integration. Token scope and restrictions vary by scenario but follow the same meet-monotone evaluation path.

See [Effects and Handlers Guide](802_effects_guide.md) for authorization scenario patterns.

## Performance and Caching

Authorization results are cached per authority, token hash, and resource scope with epoch-based invalidation. Cache entries invalidate on epoch rotation or policy update. Signature verification scales with chain length. Datalog evaluation scales with facts times rules. Attenuation is constant-cost.

See [Distributed Maintenance Guide](808_maintenance_guide.md) for cache configuration.

## Security Model

Cryptographic signature verification prevents token forgery. Epoch scoping limits token lifetime and replay attacks. Attenuation preserves security while growing verification cost proportional to chain length. Root key compromise invalidates all derived tokens.

Authority-based `ResourceScope` prevents cross-authority access. Local sovereign policy integration provides an additional security layer. Guard chain isolation ensures authorization failures leak no sensitive information.

## Implementation References

The `Cap` type in `aura-core/src/domain/journal.rs` wraps serialized Biscuit tokens with optional root key storage. The `Cap::meet()` implementation computes capability intersection. Tokens from the same issuer return the more attenuated token. Tokens from different issuers return bottom.

`BiscuitAuthorizationBridge` in `aura-guards/src/authorization.rs` handles
guard chain integration. `TokenAuthority`, `TokenGrantProfile`, and
`BiscuitTokenManager` in `aura-authorization/src/biscuit_token.rs` handle token
creation and attenuation. Capability families live in the owning feature or
domain crates, not in one central global enum. `ResourceScope` in
`aura-core/src/types/scope.rs` defines authority-centric resource patterns.

See [Transport and Information Flow](111_transport_and_information_flow.md) for flow budget details. See [Journal](105_journal.md) for fact commit semantics.

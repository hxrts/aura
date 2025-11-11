# 099 · Architectural Glossary

**Purpose**: Canonical definitions for Aura's core architectural concepts. All documentation must reference these terms consistently.

**Usage**: When writing documentation, use these exact terms and link to this glossary for definitions. When introducing new architectural concepts, add them here first.

---

## Effect System Architecture

### Core Components

**AuraEffectSystem** - The main runtime façade for all effect operations. Contains a `CompositeHandler` and manages unified context flow.
- Implementation: [`crates/aura-protocol/src/effects/system.rs`](../crates/aura-protocol/src/effects/system.rs)
- Usage: Primary entry point for applications using the effect system

**CompositeHandler** - Internal delegation component within `AuraEffectSystem`. Routes effect calls to specialized handlers.
- Implementation: [`crates/aura-protocol/src/handlers/composite.rs`](../crates/aura-protocol/src/handlers/composite.rs)
- Usage: Internal composition pattern, not used directly by applications

**AuraHandler** - Unified trait interface for type-erased effect handlers. Enables dynamic dispatch across all effect types.
- Implementation: [`crates/aura-protocol/src/handlers/erased.rs`](../crates/aura-protocol/src/handlers/erased.rs)
- Usage: Base trait for all concrete handler implementations

**AuraEffectSystemFactory** - Factory for creating configured `AuraEffectSystem` instances with mode-specific handlers.
- Implementation: [`crates/aura-protocol/src/handlers/factory.rs`](../crates/aura-protocol/src/handlers/factory.rs)
- Usage: Create systems for testing, production, or simulation

### Effect Categories

**Core Effects** - Foundational effect interfaces defined in `aura-core`:
- `TimeEffects`, `CryptoEffects`, `StorageEffects`, `NetworkEffects`, `JournalEffects`, `ConsoleEffects`, `RandomEffects`
- Location: [`crates/aura-core/src/effects/`](../crates/aura-core/src/effects/)

**Extended Effects** - Higher-level effect interfaces defined in `aura-protocol`:
- `SystemEffects`, `LedgerEffects`, `ChoreographicEffects`, `TreeEffects`, `AgentEffects`
- Location: [`crates/aura-protocol/src/effects/`](../crates/aura-protocol/src/effects/)

---

## Core Terms

**ContextId (κ)** - Relationship- or group-scoped identifier derived via DKD that defines a privacy boundary. Concrete forms include `RID` (pairwise) and `GID` (group). Messages and budgets are scoped to a single `ContextId`.

**Epoch** - Monotone, context-scoped counter used to gate FlowBudget replenishment and bind receipts. Epoch updates converge by meet on the maximum observed epoch.

**FlowBudget** - Journal fact regulating observable communication per `(ctx, peer)`:
```
FlowBudget { limit: u64, spent: u64, epoch: Epoch }
```
- `limit` merges by meet; `spent` merges by join (max). Charged by `FlowGuard` before any transport side effect.

**Receipt** - Per-hop proof of a successful budget charge bound to `(ctx, src, dst, epoch, cost)` with anti‑replay chaining and signature. Required for relays to forward.

**Capability (Cap)** - Meet-semilattice element representing authorization. Enforcement checks the guard `need(m) ≤ Caps(ctx)`.

**Fact** - Join-semilattice element representing durable knowledge. Journal commits are join‑only (no negative facts).

**Guard Chain** - Mandatory order of checks for transport effects: `CapGuard` → `FlowGuard` → `JournalCoupler`. Named invariants: Charge‑Before‑Send; No‑Observable‑Without‑Charge; Deterministic‑Replenishment.

**Choreography** - Global protocol specification written with `choreography!`. Source of truth for distributed protocol intent.

**Projection** - Compilation step from a choreography to per‑role local session types (MPST). Denoted π(G, ρ).

**Session Type** - Local, role-specific protocol type ensuring safety properties (e.g., deadlock freedom). Executed via the effect system interpreter/bridge.

**AuthorizationContext** - The evaluated capability view for the active `ContextId`, carried through sessions/effects to enforce the predicate `need(m) ≤ Caps(ctx) ∧ headroom(ctx, cost)` at send sites.

---

## Data Layer

### Journal vs Ledger

**Journal** - High-level CRDT implementing the formal `Journal { facts: Fact, caps: Cap }` semilattice model. Manages distributed threshold identity state.
- Implementation: [`crates/aura-journal/`](../crates/aura-journal/)
- Core types: [`crates/aura-core/src/journal.rs`](../crates/aura-core/src/journal.rs)
- Contains: Ratchet tree operations, device membership, threshold policies, intent pool
- Relationship: **Uses** Ledger effects for primitive operations

**Ledger** - Lower-level effect interface providing primitive operations that Journal depends on.
- Implementation: [`crates/aura-protocol/src/effects/ledger.rs`](../crates/aura-protocol/src/effects/ledger.rs)
- Provides: Device management, crypto utilities, graph operations, event sourcing
- Relationship: **Supports** Journal through effect interface

**Key Distinction**: Journal is the high-level replicated state; Ledger is the low-level effect interface.

---

## Protocol Layer

### Choreographic Programming Stack

**Choreographies** - Global protocol specifications written using `choreography!` macro. Describe distributed protocols from bird's-eye view.
- Example: [`crates/aura-protocol/src/choreography/protocols/frost.rs`](../crates/aura-protocol/src/choreography/protocols/frost.rs)
- Status: Currently used as documentation/specification, not executable
- Purpose: Global view of multi-party protocols

**Session Types** - Local projections of choreographies. Type-safe communication patterns for individual participants.
- Infrastructure: [`crates/aura-mpst/`](../crates/aura-mpst/)
- Status: Infrastructure exists, but choreography projection not yet implemented
- Purpose: Local type safety and deadlock freedom

**Protocols** - Current manual async implementations that execute the choreographic intent.
- Location: Various handler implementations throughout codebase
- Status: Working implementations used until choreographic projection is complete
- Purpose: Actual executable protocol implementations

**Key Relationship**: Choreographies → (projection) → Session Types → (implementation) → Protocols

---

## Authentication & Authorization

### Authentication Layer

**aura-verify** - Pure cryptographic identity verification. Proves "WHO signed something" without policy context.
- Implementation: [`crates/aura-verify/`](../crates/aura-verify/)
- Provides: Device signatures, guardian signatures, threshold signatures, session tickets
- Principle: Stateless identity verification

**aura-authenticate** - Choreographic authentication protocols using Multi-Party Session Types.
- Implementation: [`crates/aura-authenticate/`](../crates/aura-authenticate/)
- Provides: Device authentication ceremonies, session establishment, distributed auth protocols
- Dependencies: Uses `aura-verify` for identity verification

### Authorization Layer

**aura-wot** - Web-of-Trust capability-based authorization. Proves "WHAT you can do" based on capabilities.
- Implementation: [`crates/aura-wot/`](../crates/aura-wot/)
- Provides: Meet-semilattice capabilities, policy enforcement, delegation chains
- Principle: Capability-based access control with formal semilattice properties

### Integration

**Authorization Bridge** - Clean integration layer combining authentication (WHO) with authorization (WHAT).
- Implementation: [`crates/aura-protocol/src/authorization_bridge.rs`](../crates/aura-protocol/src/authorization_bridge.rs)
- Pattern: `authenticate_and_authorize(identity_proof, authz_context, operation)`
- Principle: Composition without coupling

---

## Storage & Content

### Content Addressing

**Hash32** - 32-byte Blake3 hash used as foundation for all content addressing.
- Implementation: [`crates/aura-core/src/content.rs`](../crates/aura-core/src/content.rs)
- Usage: Base type for all content identifiers

**ContentId** - High-level content identifier with optional size metadata.
- Structure: `{ hash: Hash32, size: Option<u64> }`
- Usage: Application-level content references

**ChunkId** - Storage-layer chunk identifier with optional sequence metadata.
- Structure: `{ hash: Hash32, sequence: Option<u32> }`
- Usage: Storage system chunk management

---

## Simulation & Testing

**Deterministic Simulation** - Testing framework with injectable effects for reproducible execution.
- Implementation: [`crates/aura-simulator/`](../crates/aura-simulator/)
- Provides: Seeded PRNG, controllable time, fault injection, property testing
- Usage: `AuraEffectSystemFactory::for_simulation(seed)`

**Execution Modes** - Runtime configuration determining which handler implementations to use.
- Types: `Testing`, `Production`, `Simulation`
- Control: Handler selection, determinism, fault injection
- Implementation: [`crates/aura-protocol/src/effects/system.rs`](../crates/aura-protocol/src/effects/system.rs)

---

## Privacy & Security

**Context Isolation** - Privacy principle ensuring no information flow between different cryptographic contexts.
- Enforcement: Transport layer context checking, message envelope validation
- Related: `RelayId`, `GroupId`, `DkdContextId` context types

**Capability Soundness** - Security property ensuring `need(operation) ≤ available_caps(context)` before execution.
- Enforcement: Guard checking in message handlers, MPST capability guards
- Related: Meet-semilattice capability operations in `aura-wot`

**Leakage Budgets** - Privacy mechanism limiting information flow through social graph relationships.
- Implementation: [`crates/aura-mpst/src/leakage.rs`](../crates/aura-mpst/src/leakage.rs)
- Tracking: `(ℓ_ext, ℓ_ngh, ℓ_grp)` annotations on choreographic operations

---

## Maintenance

**Implementation Status Tags** - Standardized indicators for feature completion:
- ✅ **COMPLETE**: Fully implemented and tested
- ⚠️ **IN PROGRESS**: Partial implementation exists  
- ❌ **NOT STARTED**: Not yet implemented
- 🗑️ **REMOVED**: Intentionally deleted or deprecated

**Canonical Sources** - Single sources of truth for major architectural concepts:
- Effect System: This glossary + [`docs/002_system_architecture.md`](002_system_architecture.md)
- Journal vs Ledger: [`docs/105_journal.md`](105_journal.md)
- Auth/Authz: [`docs/101_auth_authz.md`](101_auth_authz.md)
- Protocol Stack: [`docs/002_system_architecture.md`](002_system_architecture.md) Protocol Stack section

---

## Contributing Guidelines

1. **New Architectural Concepts**: Add to this glossary first, then reference in your documentation
2. **Naming Consistency**: Use exact terms from this glossary in all documentation
3. **Link References**: Link to relevant implementation files when introducing concepts
4. **Status Accuracy**: Use implementation status tags accurately based on actual code state
5. **Canonical Links**: Reference canonical documentation sources rather than duplicating explanations

---

*Last Updated: 2024-11-09*  
*Maintainer: Keep this synchronized with actual implementation*

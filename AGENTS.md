# CLAUDE.md

## Session Initialization

**IMPORTANT**: When starting any session, immediately:
1. Enter the Nix environment if not already in the shell: `nix develop`
2. Read `.claude/skills/aura-quick-ref/SKILL.md` for enhanced context

## Project Overview

Aura is a threshold identity and encrypted storage platform using threshold cryptography and social recovery. Choreographic programming with session types coordinates distributed protocols. Algebraic effects provide modular runtime composition.

- **Primary specs**: `docs/` directory (authoritative)
- **Architecture**: `docs/001_system_architecture.md`, `docs/999_project_structure.md`
- **Scratch**: `work/` is non-authoritative and may be removed

## Development Commands

**Required**: Nix with flakes enabled. Run `nix develop` first.

| Category | Command | Purpose |
|----------|---------|---------|
| Build | `just build` | Build all crates |
| Build | `just check` | Check without building |
| Build | `just clippy` | Lint (warnings as errors) |
| Format | `just fmt` | Format code |
| Format | `just fmt-check` | Check formatting |
| Test | `just test` | Run all tests |
| Test | `just test-crate <name>` | Test specific crate |
| Test | `just ci-dry-run` | Local CI checks |
| Nix | `nix build` | Hermetic build |
| Nix | `nix flake check` | Hermetic tests |
| Nix | `crate2nix generate` | Regenerate after dep changes |
| Dev | `just watch` | Rebuild on changes |
| Dev | `just clean` | Clean artifacts |
| Arch | `just check-arch` | Verify architecture compliance |

## Architecture Overview

### 8-Layer Structure

| Layer | Crates | Purpose |
|-------|--------|---------|
| L1 Foundation | `aura-core` | Effect traits, domain types, crypto utilities |
| L2 Specification | `aura-journal`, `aura-authorization`, `aura-signature`, `aura-store`, `aura-transport`, `aura-mpst`, `aura-macros` | Domain semantics, no runtime |
| L3 Implementation | `aura-effects`, `aura-composition` | Stateless handlers, composition |
| L4 Orchestration | `aura-protocol`, `aura-guards`, `aura-consensus`, `aura-amp`, `aura-anti-entropy` | Multi-party coordination |
| L5 Features | `aura-authentication`, `aura-chat`, `aura-invitation`, `aura-recovery`, `aura-relational`, `aura-rendezvous`, `aura-social`, `aura-sync` | End-to-end protocols |
| L6 Runtime | `aura-agent`, `aura-simulator`, `aura-app` | System assembly |
| L7 Interface | `aura-terminal` | CLI/TUI entry points |
| L8 Testing | `aura-testkit`, `aura-quint`, `aura-harness` | Test infrastructure |

### Key Invariants

- **Dependencies flow downward only** — no circular dependencies
- **Effect traits defined in `aura-core` only** — all trait definitions, nowhere else
- **Guard chain sequence**: AuthorizationEffects (Biscuit/capabilities) → FlowBudgetEffects (charge-before-send) → LeakageEffects → JournalEffects (fact commit) → TransportEffects
- **Consensus is NOT linearizable** — use session types for operation sequencing
- **Hybrid journal**: fact journal (join) + capability frontier (meet) combined as `JournalState`
- **Flow budgets**: only `spent` counters are facts; limits derived at runtime from Biscuit + policy
- **No direct impure functions** outside effect implementations — no `SystemTime::now()`, `thread_rng()`, `std::fs` in application code
- **Unified encryption-at-rest**: `aura-effects::EncryptedStorage` wraps `StorageEffects`; no ad-hoc storage encryption

### Authority Model

- Identity via opaque `AuthorityId` and relational `ContextId`
- Commitment trees expressed as fact-based `AttestedOps` (`aura-journal/src/fact.rs`)
- Relational contexts (guardian bindings, recovery grants) live in their own journals
- Aura Consensus is the sole strong-agreement mechanism
- **Transaction Model**: (1) Authority Scope (single vs cross-authority) × (2) Agreement Level (monotone/CRDT vs consensus). Monotone = 0 RTT, consensus = 1-3 RTT.

### Layer 5 Conventions

- Each crate includes `ARCHITECTURE.md` with facts, invariants, operation categories
- Each crate exposes `OPERATION_CATEGORIES` mapping operations to A/B/C classes
- Runtime-owned caches (invitation/rendezvous descriptors) live in L6 handlers, not L5
- Facts use versioned binary encoding with JSON fallback; bump schema constants on breaking changes
- FactKey helper types required for reducers/views to avoid key drift
- Ceremony facts include optional `trace_id` for correlation

## Agent Decision Aids

### Code Location Decision Tree

```
What am I implementing?
├─ Effect trait definition → aura-core (L1)
├─ Single-party stateless handler → aura-effects (L3)
├─ Multi-party coordination → aura-protocol + L4 subcrates
├─ Domain-specific logic → Domain crate (L2)
├─ Complete end-to-end protocol → Feature crate (L5)
├─ Runtime assembly → aura-agent (L6)
├─ CLI/TUI command → aura-terminal (L7)
└─ Mock/test handler → aura-testkit (L8)
```

### Effect Classification

| Question | Infrastructure (aura-effects) | Application (domain crate) |
|----------|------------------------------|---------------------------|
| OS integration needed? | ✓ Yes | ✗ No (inject effects) |
| Contains domain semantics? | ✗ No | ✓ Yes |
| Aura-specific logic? | ✗ No | ✓ Yes |
| Reusable outside Aura? | ✓ Yes | ✗ No |

**Quick test**: OS integration? → Infrastructure. Aura domain knowledge? → Application. Convenience wrapper? → Composite/extension trait.

### Fact Pattern Selection

```
Is this a Layer 2 domain crate?
├─ Yes → Use aura-core pattern (FactTypeId, try_encode, FactDeltaReducer)
│         Do NOT depend on aura-journal
│
└─ No (Layer 4/5) → Use DomainFact trait pattern
                    Depend on aura-journal, register in FactRegistry
```

### Layer Rules

| Layer | What Goes Here | What Doesn't |
|-------|----------------|--------------|
| L1 (`aura-core`) | Effect trait definitions, domain types, crypto utilities, Arc blankets, extension traits | Implementations, business logic, handlers |
| L2 (Domain) | Pure domain semantics, CRDT logic, fact types, validation rules | OS access, Tokio, handler composition, runtime state |
| L3 (`aura-effects`) | Stateless single-party handlers, OS integration | Multi-handler coordination, stateful impls, mock handlers |
| L4 (Orchestration) | Multi-party coordination, guard chain, consensus runtime, cross-handler decisions | Effect definitions, single-party handlers, runtime assembly |
| L5 (Features) | End-to-end protocols, OPERATION_CATEGORIES, domain facts | Runtime caches (those go in L6), UI concerns |
| L6 (Runtime) | Lifecycle management, effect system assembly, runtime-owned caches | Handler implementations, protocol coordination |
| L7 (`aura-terminal`) | CLI/TUI entry points, main() | Business logic (import from `aura-app` only) |
| L8 (Testing) | Mock handlers, test fixtures, stateful test handlers | Production code |

### Crate Selection by Implementation

| Implementing... | Crate |
|-----------------|-------|
| Hash function (pure) | `aura-core` |
| Cryptographic operations | Effect traits; see `docs/100_crypto.md` |
| FROST primitives | `aura-core::crypto::tree_signing` |
| Guardian recovery | `aura-recovery` |
| Journal fact validation | `aura-journal` |
| Network transport | `aura-transport` (abstractions) + `aura-effects` (TCP) |
| CLI command | `aura-terminal` |
| Test scenario | `aura-testkit` |
| Choreography protocol | Feature crate + `aura-mpst` |
| Authorization logic | `aura-authorization` |
| Social topology | `aura-social` |
| Quint specification | `verification/quint/` |

### Before Removing a Stub Handler

1. Check if the trait is used anywhere
2. If **unused**: Remove both trait (aura-core) AND implementation (aura-effects)
3. If **used**: Keep a properly-named fallback handler

### Compliance Checklist

- [ ] Layer dependencies flow downward only
- [ ] Effect traits in `aura-core` only
- [ ] Infrastructure effects in `aura-effects`, application effects in domain crates
- [ ] No direct impure functions outside effect implementations
- [ ] Production handlers are stateless

## Documentation Lookup

### By Task

| Task | Doc | Code |
|------|-----|------|
| Adding effect trait | `docs/105_effect_system_and_runtime.md` | `aura-core/src/effects/` |
| Building choreography | `docs/108_mpst_and_choreography.md` | Feature crate + `aura-mpst` |
| Understanding authorities | `docs/102_authority_and_identity.md` | `aura-core/src/authority.rs` |
| Implementing consensus | `docs/106_consensus.md` | `aura-consensus/` |
| Working with journals | `docs/103_journal.md` | `aura-journal/` |
| Recovery flows | `docs/112_relational_contexts.md` | `aura-recovery/` |
| Architecture debugging | `docs/999_project_structure.md` | `just check-arch` |

### By Concept

| Concept | Documentation |
|---------|---------------|
| Authorities & identity | `docs/102_authority_and_identity.md` |
| Commitment trees | `docs/102_authority_and_identity.md` |
| Consensus | `docs/106_consensus.md` |
| Effect system | `docs/105_effect_system_and_runtime.md` |
| Protocols & choreography | `docs/108_mpst_and_choreography.md` |
| Guard chain | `docs/001_system_architecture.md` §5 |
| Journals & facts | `docs/103_journal.md` |
| State reduction | `docs/103_journal.md` |
| Privacy & flow budgets | `docs/003_information_flow_contract.md` |
| Relational contexts | `docs/112_relational_contexts.md` |
| Transport & receipts | `docs/109_transport_and_information_flow.md` |
| Rendezvous | `docs/111_rendezvous.md` |
| Social topology | `docs/114_social_architecture.md` |
| Cryptography | `docs/100_crypto.md` |
| Authorization & Biscuit | `docs/104_authorization.md` |
| Identifiers & boundaries | `docs/101_identifiers_and_boundaries.md` |
| Operation categories | `docs/107_operation_categories.md` |
| Database & queries | `docs/113_database.md` |
| Distributed systems | `docs/004_distributed_systems_contract.md` |
| Theoretical model | `docs/002_theoretical_model.md` |
| Testing | `docs/804_testing_guide.md` |
| Simulation | `docs/805_simulation_guide.md` |
| Verification (Quint/Lean) | `docs/806_verification_guide.md` |
| Maintenance & OTA | `docs/115_maintenance.md`, `docs/808_maintenance_guide.md` |
| Effects & handlers | `docs/802_effects_guide.md` |
| Choreography | `docs/803_choreography_guide.md` |
| System internals | `docs/807_system_internals_guide.md` |
| Getting started | `docs/801_hello_world_guide.md` |

## Domain Concepts

### Terminology

From `docs/002_theoretical_model.md#shared-terms-and-notation`:
- **Roles**: `Member`, `Participant`, `Moderator`
- **Access levels**: `Full`, `Partial`, `Limited`
- **Topology**: `1-hop` / `n-hop` links

### Threshold Lifecycle (K/A Modes)

Key generation and agreement are orthogonal:

| Mode | Key Generation | Agreement |
|------|---------------|-----------|
| K1 | Local/Single-signer | A1: Provisional |
| K2 | Dealer-based DKG | A2: Coordinator soft-safe |
| K3 | Quorum/BFT-DKG | A3: Consensus-finalized |

Fast paths (A1/A2) must be superseded by A3 for durable shared state.

### Time System

Four domains via effect traits (no direct `SystemTime::now()` or chrono):

| Effect Trait | TimeStamp Variant | Use Case |
|--------------|-------------------|----------|
| `PhysicalTimeEffects` | `PhysicalClock(PhysicalTime)` | Wall-clock, expiration, receipts |
| `LogicalClockEffects` | `LogicalClock(LogicalTime)` | Vector/Lamport for causality |
| `OrderClockEffects` | `OrderClock(OrderTime)` | Privacy-preserving ordering (no timing leakage) |
| `TimeAttestationEffects` | `Range(RangeTime)` | Validity windows, provenance proofs |

**Key principles**: Domain separation based on semantics. OrderClock leaks no timing. All time access via traits.

### Authorization

1. **Capability semantics** (`aura-authorization`): Meet-semilattice evaluation
2. **Biscuit tokens**: Cryptographically verifiable, attenuated
3. **Guard chain**: CapGuard → FlowGuard → JournalCoupler → LeakageTracker

## Usage Efficiency

- Prefer specific file paths over broad searches
- Use `just check-arch` before complex refactoring
- Use `.claude/skills/` for project-specific knowledge
- Batch operations and parallel tool calls when possible

## Legacy Notes

- `aura-frost` deprecated → use `aura-core::crypto::tree_signing`
- Graph-based `journal_ops` removed → use fact-based `AttestedOps`
- `DeviceMetadata`/`DeviceRegistry` removed → derive from `LeafNode`

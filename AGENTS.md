# CLAUDE.md

## Session Initialization

**IMPORTANT**: When starting any session, immediately:
1. Enter the Nix environment if not already in the shell: `nix develop`
2. Read `.claude/skills/aura-quick-ref/SKILL.md` for enhanced context

## Project Overview

Aura is a threshold identity and encrypted storage platform using threshold cryptography and social recovery. Choreographic programming with session types coordinates distributed protocols. Algebraic effects provide modular runtime composition.

- **Primary specs**: `docs/` directory (authoritative)
- **Architecture**: `docs/001_system_architecture.md`, `docs/999_project_structure.md`
- **Per-crate architecture docs**: each crate root has an `ARCHITECTURE.md` that explains the crate's purpose, boundaries, invariants, and key integration points; read it before making non-trivial changes in that crate
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
| Arch | `just lint-arch-syntax` | Run Rust-native syntax/policy lints that replaced grep-heavy `arch.sh` checks |

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
- **Hybrid journal**: fact journal (join) + capability frontier (meet) combined as journal state
- **Flow budgets**: only `spent` counters are facts; limits derived at runtime from Biscuit + policy
- **No direct impure functions** outside effect implementations — no `SystemTime::now()`, `thread_rng()`, `std::fs` in application code
- **Unified encryption-at-rest**: `aura-effects::EncryptedStorage` wraps `StorageEffects`; no ad-hoc storage encryption
- **Shared UX contract ownership**: parity-critical UI ids, focus semantics, action contracts, and parity metadata come from `aura-app::ui_contract`
- **Harness mode discipline**: `AURA_HARNESS_MODE` may change instrumentation or rendering stability, but must not change parity-critical business-flow semantics
- **Harness mode exceptions**: allowlisted harness-only hooks must carry owner, justification, and design-note metadata in `scripts/check/user-flow-policy-guardrails.sh`
- **Browser bridge compatibility**: changes to browser harness bridge or observation surfaces must update `crates/aura-web/ARCHITECTURE.md` and `docs/804_testing_guide.md`
- **Parity exception metadata**: every `ParityException` must have structured metadata in `aura-app::ui_contract` including reason code, scope, affected surface, and doc reference
- **Parity-critical waits**: use authoritative readiness, event, or quiescence contracts; raw sleeps, raw polling, and fallback text/DOM checks are diagnostics only
- **Shared user-flow documentation sync**: shared user-flow contract or policy changes must update the mapped authoritative targets enforced by `scripts/check/user-flow-guidance-sync.sh`
- **Shared user-flow contributor sync**: when shared UX policy scripts change, keep `AGENTS.md` and the mapped local skills aligned with the updated contributor guidance in the same change
- **Shared scenario boundary**: shared scenarios stay actor-based and semantic-only; the legacy compatibility-step scenario language is quarantined to explicit non-shared fixtures
- **Typed governance first**: extend typed validator domains before adding new shell policy logic; `scripts/check/` wrappers should stay thin and workflow-oriented
- **Shared semantic lifecycle ownership**: `aura-app::workflows` owns authoritative parity-critical semantic lifecycle publication after handoff; `aura-terminal`, `aura-web`, and `aura-harness` submit and observe but do not keep parallel terminal publication paths
- **Frontend/app facade boundary**: frontend parity-critical imports go through `aura_app::ui` and `aura_app::ui::workflows`; do not reach into crate-root `aura_app::workflows` or private semantic helper modules
- **Runtime-private ownership boundaries**: raw VM admission helpers, VM fragment ownership registry mutation, and `ReconfigurationController` stay internal to `aura-agent`; use the sanctioned ingress and manager surfaces instead
- **Architecture enforcement split**: prefer type/API design, compile-fail
  tests, and Rust-native lints for syntactic or boundary-shape rules;
  `just check-arch` should stay focused on workspace topology, governance, and
  integration checks that are not realistically provable at compile time
- **Architecture syntax lint gate**: run `just lint-arch-syntax` when changing
  effect placement, runtime-coupling, raw impure/time/random usage,
  concurrency escape hatches, crypto-boundary syntax, or syntax-owned
  serialization/style rules

### Conditional Compilation

Use `cfg_if::cfg_if!` to group related conditional items when it improves readability:
- **Good candidates**: 3+ consecutive items with same `#[cfg(...)]`, mutually exclusive platform code (wasm32 vs native), feature-gated module/import groups
- **Not recommended**: Individual methods in impl blocks (cfg_if is for top-level items), interleaved conditional and non-conditional exports, simple 2-line patterns

### Authority Model

- Identity via opaque `AuthorityId` and relational `ContextId`
- Commitment trees expressed as fact-based `AttestedOp` (`aura-journal/src/fact.rs`)
- Relational contexts (guardian bindings, recovery grants) live in their own journals
- Aura Consensus is the sole strong-agreement mechanism
- **Transaction Model**: (1) Authority Scope (single vs cross-authority) × (2) Agreement Level (monotone/CRDT vs consensus). Monotone = 0 RTT, consensus = 1-3 RTT.

### Layer 5 Conventions

- Each Layer 5 crate exposes its operation categories and keeps them aligned with its crate-root `ARCHITECTURE.md`
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
| Adding effect trait | `docs/103_effect_system.md` | `aura-core/src/effects/` |
| Building choreography | `docs/110_mpst_and_choreography.md` | Feature crate + `aura-mpst` |
| Understanding authorities | `docs/102_authority_and_identity.md` | `aura-core/src/authority.rs` |
| Implementing consensus | `docs/108_consensus.md` | `aura-consensus/` |
| Working with journals | `docs/105_journal.md` | `aura-journal/` |
| Recovery flows | `docs/114_relational_contexts.md` | `aura-recovery/` |
| Architecture debugging | `docs/999_project_structure.md` | `just check-arch` |

### By Concept

| Concept | Documentation |
|---------|---------------|
| Authorities & identity | `docs/102_authority_and_identity.md` |
| Commitment trees | `docs/102_authority_and_identity.md` |
| Consensus | `docs/108_consensus.md` |
| Effect system | `docs/103_effect_system.md` |
| Runtime | `docs/104_runtime.md` |
| Protocols & choreography | `docs/110_mpst_and_choreography.md` |
| Guard chain | `docs/001_system_architecture.md` §5 |
| Journals & facts | `docs/105_journal.md` |
| State reduction | `docs/105_journal.md` |
| Privacy & flow budgets | `docs/003_information_flow_contract.md` |
| Relational contexts | `docs/114_relational_contexts.md` |
| Transport & receipts | `docs/111_transport_and_information_flow.md` |
| Rendezvous | `docs/113_rendezvous.md` |
| Social topology | `docs/115_social_architecture.md` |
| Cryptography | `docs/100_crypto.md` |
| Authorization & Biscuit | `docs/106_authorization.md` |
| Identifiers & boundaries | `docs/101_identifiers_and_boundaries.md` |
| Operation categories | `docs/109_operation_categories.md` |
| Database & queries | `docs/107_database.md` |
| Distributed systems | `docs/004_distributed_systems_contract.md` |
| Theoretical model | `docs/002_theoretical_model.md` |
| Testing | `docs/804_testing_guide.md` |
| Simulation | `docs/805_simulation_guide.md` |
| Verification (Quint/Lean) | `docs/806_verification_guide.md` |
| Maintenance & OTA | `docs/116_maintenance.md`, `docs/808_maintenance_guide.md` |
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

## Ownership Model

- `Pure` → reducers, validators, typed contracts
- `MoveOwned` → handles, owner tokens, transfer/handoff records,
  `OperationContext`, consumed `TerminalPublisher`
- `ActorOwned` → long-lived mutable async state, supervisors, coordinators,
  `OwnedTaskSpawner`, `OwnedShutdownToken`, bounded ingress
- `Observed` → projections, rendering, harness reads

Rules:
- parity-critical mutation/publication must be capability-gated
- long-lived async flows need a single owner
- terminal lifecycle belongs to owner modules, not UI/harness layers
- parity-critical operations must end in typed success, failure, or
  cancellation
- frontend-local submission ownership must hand off before the first awaited
  app/runtime workflow step if the frontend is not the terminal owner
- best-effort work must not block primary terminal publication
- semantic owners may only use approved bounded-await / retry helpers
- canonical ownership/runtime primitives for parity-critical code come from
  `aura-core::ownership` through the explicit `actor_owned`, `move_owned`, and
  `capability_gated` surfaces
- parity-critical boundaries must use the `aura-macros` declaration layer:
  `#[semantic_owner(..., category = "move_owned")]`,
  `#[actor_owned(..., category = "actor_owned")]`,
  `#[capability_boundary(category = "capability_gated", ...)]`, and
  `#[ownership_lifecycle(...)]` where a small state machine is appropriate
- ownership shell scripts are secondary escape-hatch fences; primary
  enforcement belongs in types, macros, and compile-fail coverage

### Time System

Four domains via effect traits (no direct `SystemTime::now()` or chrono):

| Effect Trait | TimeStamp Variant | Use Case |
|--------------|-------------------|----------|
| `PhysicalTimeEffects` | `PhysicalClock(PhysicalTime)` | Wall-clock, expiration, receipts |
| `LogicalClockEffects` | `LogicalClock(LogicalTime)` | Vector/Lamport for causality |
| `OrderClockEffects` | `OrderClock(OrderTime)` | Privacy-preserving ordering (no timing leakage) |
| `TimeComparison` | `Range(RangeTime)` | Validity windows, ordering comparison |

**Key principles**: Domain separation based on semantics. OrderClock leaks no timing. All time access via traits.

### Authorization

1. **Capability semantics** (`aura-authorization`): Meet-semilattice evaluation
2. **Biscuit tokens**: Cryptographically verifiable, attenuated
3. **Guard chain**: CapGuard → FlowGuard → JournalCoupler → LeakageTracker

## Usage Efficiency

- Prefer specific file paths over broad searches
- Use `just check-arch` before complex refactoring
- For shared user-flow or harness policy work, run `just ci-user-flow-policy`
- Use `.claude/skills/` for project-specific knowledge
- Batch operations and parallel tool calls when possible

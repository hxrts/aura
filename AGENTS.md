# AGENTS.md + CLAUDE.md

## Session Initialization

**IMPORTANT**: When starting any session, immediately:
1. Enter the Nix environment if not already in the shell: `nix develop`
3. Read `.claude/skills/aura-quick-ref/SKILL.md` for enhanced context

## Project Overview

Aura is a threshold identity and encrypted storage platform built on relational security principles. It uses threshold cryptography and social recovery to eliminate the traditional choice between trusting a single device or a centralized entity.

**Architecture**: Choreographic programming with session types for coordinating distributed protocols. Uses algebraic effects for modular runtime composition. The `docs/` directory is the **primary, authoritative spec**; `work/` is non-authoritative scratch and may be removed.
See `docs/001_system_architecture.md` and `docs/999_project_structure.md` for the latest crate breakdown.

## Development Setup

**Required**: Nix with flakes enabled

```bash
nix develop                           # Enter development shell
# OR
echo "use flake" > .envrc && direnv allow  # Auto-activate with direnv
```

All commands below must be run within `nix develop`.

## Common Commands

### Build & Check
- `just build` - Build all crates
- `just check` - Check without building
- `just fmt` - Format code
- `just fmt-check` - Check formatting
- `just clippy` - Lint (warnings as errors)

### Hermetic Builds (crate2nix)
- `nix build` - Build with hermetic Nix (reproducible)
- `nix build .#aura-terminal` - Build specific package
- `nix run` - Run aura CLI hermetically
- `nix flake check` - Run hermetic tests
- `crate2nix generate` - Regenerate Cargo.nix after dependency changes

### Testing
- `just test` - Run all tests (preferred)
- `just test-crate <name>` - Test specific crate
- `just ci-dry-run` - Local CI checks (format, lint, test)
- `just smoke-test` - Phase 0 integration tests
- `cargo test --workspace -- --nocapture` - Tests with output

### Development Workflow
- `just watch` - Rebuild on changes
- `just watch-test` - Retest on changes
- `just clean` - Clean artifacts
- `just docs` - Generate documentation

### Phase 0 Demo
- `just init-account` - Initialize 2-of-3 threshold account
- `just status` - Show account status

## Architecture Essentials

### 8-Layer Architecture

The codebase follows a strict 8-layer architecture with zero circular dependencies:

1. **Foundation** (`aura-core`): Effect traits (crypto, network, storage, unified time system, journal, console, random, transport), domain types (`AuthorityId`, `ContextId`, `SessionId`, `FlowBudget`), cryptographic utilities (FROST, merkle trees), semilattice traits, unified errors (`AuraError`), and reliability utilities. Other crates depend on `aura-core`, but it depends on none of them.

2. **Specification** (Domain Crates + `aura-mpst` + `aura-macros`):
   - Domain crates (`aura-journal`, `aura-authorization`, `aura-signature`, `aura-store`, `aura-transport`): CRDT domains, capability systems, transport semantics. `aura-journal` now exposes fact-based journals and reduction pipelines (`docs/103_journal.md`, `docs/115_maintenance.md`).
   - `aura-mpst`: Session type runtime with guard extensions and leakage tracking (`LeakageTracker`).
   - `aura-macros`: Choreography DSL parser/annotation extractor (`guard_capability`, `flow_cost`, `journal_facts`, `leak`) that emits Telltale projections.

3. **Implementation** (`aura-effects` + `aura-composition`): Stateless, single-party handlers (`aura-effects`) and handler composition infrastructure (`aura-composition`). Production handlers implement core effect traits (crypto, network, storage, randomness, console, etc.). Mock/test handlers are in `aura-testkit`.
   - **Unified encryption-at-rest**: `aura-effects::EncryptedStorage` wraps `StorageEffects` and persists the master key via `SecureStorageEffects` (Keychain/TPM/Keystore; filesystem fallback during bring-up). Application code should not implement ad-hoc storage encryption (e.g., `LocalStore`).

4. **Orchestration** (`aura-protocol` + `aura-guards`, `aura-consensus`, `aura-amp`, `aura-anti-entropy`): Multi-party coordination and guard infrastructure: handler adapters, CrdtCoordinator, GuardChain (CapGuard → FlowGuard → JournalCoupler), Consensus runtime, AMP orchestration, anti-entropy/snapshot helpers.

5. **Feature/Protocol** (`aura-authentication`, `aura-chat`, `aura-invitation`, `aura-recovery`, `aura-relational`, `aura-rendezvous`, `aura-social`, `aura-sync`): End-to-end protocol crates (auth, secure messaging, guardian recovery, rendezvous, social topology, storage, etc.) built atop the orchestration layer. `aura-frost` is deprecated; FROST primitives live in `aura-core::crypto::tree_signing`.

6. **Runtime Composition** (`aura-agent`, `aura-simulator`, `aura-app`): Runtime assembly of effect systems (agent), deterministic simulation (simulator), and portable application core (app). `aura-agent` now owns the effect registry/builder infrastructure; `aura-protocol` no longer exports the legacy registry. `aura-app` provides the platform-agnostic business logic consumed by all frontends.

7. **User Interface** (`aura-terminal`): Terminal-based CLI and TUI entry points. Imports only from `aura-app` (never `aura-agent` directly). Uses `AppCore` as the unified backend interface for all operations. Exposes scenario/admin/recovery/invitation flows plus authority/context inspection commands.

8. **Testing & Tools** (`aura-testkit`, `aura-quint`): Shared fixtures, simulation harnesses, property tests, Quint interop.

### Layer 5 Conventions
- Each Layer 5 crate includes `ARCHITECTURE.md` describing facts, invariants, and operation categories.
- Each Layer 5 crate exposes `OPERATION_CATEGORIES` mapping operations to A/B/C classes.
- Runtime-owned caches (e.g., invitation/rendezvous descriptors) must live in Layer 6 handlers.
- Layer 5 facts use versioned binary encoding (bincode) with JSON fallback for debug; bump per-crate schema constants on breaking changes.
- FactKey helper types are required for reducers/views to avoid ad-hoc key drift.
- Ceremony facts include optional `trace_id` for correlation (typically set to the ceremony id).

**Where does my code go?** See the docs under `docs/001_system_architecture.md` and `docs/102_authority_and_identity.md` for the latest authority-centric guidance.

## Architecture Essentials (Authority Model)

Aura now models identity via opaque authorities (`AuthorityId`) and relational contexts (`ContextId`). Key points:

- commitment tree updates and device membership are expressed as fact-based AttestedOps (`aura-journal/src/fact.rs`). No graph-based `journal_ops` remain.
- Relational contexts (guardian bindings, recovery grants, rendezvous receipts) live in their own journals (`docs/112_relational_contexts.md`).
- Aura Consensus is the sole strong-agreement mechanism (`docs/106_consensus.md`). Fast path + fallback gossip integrate with the guard chain.
- Guard chain sequence: `AuthorizationEffects` (Biscuit/capabilities) → `FlowBudgetEffects` (charge-before-send) → `LeakageEffects` (`docs/003_information_flow_contract.md`) → `JournalEffects` (fact commit) → `TransportEffects`.
- Flow budgets: only the `spent` counters are facts; limits are derived at runtime from Biscuit + policy.
- **Hybrid journal model**: fact journal (join) + capability frontier (meet) combined as `JournalState` for effects/runtime use.
- **Transaction Model**: Database operations coordinate via two orthogonal dimensions: (1) Authority Scope (Single vs Cross-authority) and (2) Agreement Level (Monotone/CRDT vs Consensus). Monotone operations use CRDT merge (0 RTT). Non-monotone operations use consensus (1-3 RTT). Cross-authority operations work with both. Consensus is NOT linearizable - use session types for operation sequencing. See `docs/113_database.md` §8.

## Threshold Lifecycle Taxonomy

Aura separates **key generation** from **agreement/finality**:
- **K1**: Local/Single‑Signer (no DKG)
- **K2**: Dealer‑Based DKG (trusted coordinator)
- **K3**: Quorum/BFT‑DKG (consensus‑finalized transcript)

Agreement modes are orthogonal:
- **A1**: Provisional (usable immediately, not final)
- **A2**: Coordinator Soft‑Safe (bounded divergence + convergence cert)
- **A3**: Consensus‑Finalized (unique, durable, non‑forkable)

Leader selection and pipelining are **orthogonal optimizations**, not agreement modes. Fast paths (A1/A2) must be explicitly marked provisional and superseded by A3 for durable shared state.

## Distributed Systems Contract

See `docs/004_distributed_systems_contract.md` for the distributed-systems guarantees (safety, liveness, partial synchrony assumptions, latency expectations, adversarial models, and monitoring guidance).

## Information Flow / Privacy

Reference `docs/003_information_flow_contract.md` for the unified flow-budget/metadata-leakage contract. Key notes:
- Charge-before-send invariant enforced by FlowGuard + JournalCoupler.
- Receipts propagate via relational contexts (`docs/109_transport_and_information_flow.md`).
- Leakage budgets tracked via `LeakageEffects` and choreography annotations.

## Authorization Systems

1. **Traditional Capability Semantics** (`aura-authorization`): Meet-semilattice capability evaluation for local checks.
2. **Biscuit Tokens** (`aura-authorization/src/biscuit/`, `aura-guards/src/authorization.rs`): Cryptographically verifiable, attenuated tokens.
3. **Guard Integration**: `aura-guards::{CapGuard, FlowGuard, JournalCoupler, LeakageTracker}` enforce Biscuit/policy requirements, flow budgets, journal commits, and leakage budgets per message.

## Unified Time System

Aura uses a unified `TimeStamp` with domain-specific traits; legacy `TimeEffects`/chrono use is forbidden in application code.

1. **PhysicalTimeEffects** (`aura-core/src/effects/time.rs`): Wall-clock time for timestamps, expiration, cooldowns, receipts.
2. **LogicalClockEffects**: Vector + Lamport clocks for causal ordering (CRDT/session happens-before).
3. **OrderClockEffects**: Privacy-preserving total ordering tokens with no temporal meaning.
4. **TimeAttestationEffects**: Optional provenance/consensus proof wrapper around `TimeStamp` when attested time is required.

**TimeStamp Variants** (`aura-core/src/time.rs`):
- `PhysicalClock(PhysicalTime)`: ms since UNIX epoch + optional uncertainty
- `LogicalClock(LogicalTime)`: vector + Lamport clocks for causality  
- `OrderClock(OrderTime)`: Opaque 32-byte tokens for deterministic ordering without leakage
- `Range(RangeTime)`: Validity windows/constraints (compose with PhysicalClock)

**Key Principles**:
- Domain separation: choose Physical/Logical/Order/Range based on semantics
- Privacy: OrderClock leaks no timing; provenance is orthogonal via attestation
- Effect integration: all time access via traits; no direct `SystemTime::now()`/chrono outside handlers
- Explicit ordering: use `TimeStamp::compare(policy)` for cross-domain comparisons

## Documentation Map

- Core overview: `docs/000_project_overview.md`
- Theoretical model: `docs/002_theoretical_model.md`
- Architecture: `docs/001_system_architecture.md`
- Privacy: `docs/003_information_flow_contract.md`
- Distributed systems contract: `docs/004_distributed_systems_contract.md`
- Authority/Relational identity: `docs/102_authority_and_identity.md`, `docs/112_relational_contexts.md`
- Consensus & BFT-DKG: `docs/106_consensus.md`
- Transport/receipts: `docs/109_transport_and_information_flow.md`, `docs/111_rendezvous.md`
- AMP messaging: `docs/110_amp.md`
- Developer guides: `docs/108_mpst_and_choreography.md`, `docs/105_effect_system_and_runtime.md`
- Cryptography & VSS: `docs/100_crypto.md`
- Operation categories and ceremonies: `docs/107_operation_categories.md`
- Reference: `docs/999_project_structure.md`

## Agent Quick Reference

### "Where does my code go?" Decision Tree
- **Single-party stateless operation** → `aura-effects`
- **Multi-party coordination** → `aura-protocol` + Layer 4 subcrates (`aura-guards`, `aura-consensus`, `aura-amp`, `aura-anti-entropy`)
- **Domain-specific logic** → Domain crate (`aura-journal`, etc.)
- **Domain service handler (stateless)** → Domain crate `*FactService` (e.g., `aura-chat::ChatFactService`)
- **RwLock wrapper service** → `aura-agent/src/handlers/*_service.rs`
- **Complete end-to-end protocol** → Feature crate (e.g., `aura-authentication`; `aura-frost` deprecated)
- **Effect trait definition** → `aura-core`
- **Mock/test handlers** → `aura-testkit`

### Common Development Tasks → Docs
- **Adding new effect trait**: `docs/105_effect_system_and_runtime.md` → `docs/805_development_patterns.md`
- **Building choreography**: `docs/108_mpst_and_choreography.md` → `docs/803_coordination_guide.md`
- **Understanding authorities**: `docs/102_authority_and_identity.md` → `docs/103_journal.md`
- **Debugging architecture**: `docs/999_project_structure.md` + `just check-arch`
- **Implementing consensus**: `docs/106_consensus.md` → `crates/aura-consensus/src/consensus/`
- **Working with journals**: `docs/103_journal.md` → `aura-journal/src/`
- **Creating recovery flows**: `docs/112_relational_contexts.md` → `aura-recovery/`

### Architecture Compliance Checklist
- [ ] Layer dependencies flow downward only (see dependency graph in `docs/999_project_structure.md`)
- [ ] Effect traits defined in `aura-core` only
- [ ] Infrastructure effects implemented in `aura-effects`
- [ ] Application effects in domain crates
- [ ] No direct impure function usage outside effect implementations
- [ ] All async functions propagate `EffectContext`
- [ ] Production handlers are stateless, test handlers in `aura-testkit`

### Layer-Based Development Workflow
- **Working on Layer 1 (Foundation)?** Read: `docs/105_effect_system_and_runtime.md`
- **Working on Layer 2 (Domains)?** Read: Domain-specific docs (`docs/100-112`)
- **Working on Layer 3 (Effects)?** Read: `docs/805_development_patterns.md`
- **Working on Layer 4 (Protocols)?** Read: `docs/108_mpst_and_choreography.md`
- **Working on Layer 5 (Features)?** Read: `docs/803_coordination_guide.md`
- **Working on Layer 6 (Runtime)?** Read: `aura-agent/` and `aura-simulator/`
- **Working on Layer 7 (Terminal)?** Read: `aura-terminal/` + `aura-app/` + scenario docs
- **Working on Layer 8 (Testing)?** Read: `docs/805_testing_guide.md`

### Task-Oriented Crate Selection

#### "I'm implementing..."
- **A new hash function** → `aura-core` (pure function) + `aura-effects` (if OS integration needed)
- **Cryptographic operations** → Use effect traits; see `docs/100_crypto.md` for layer rules
- **FROST primitives** → `aura-core::crypto::tree_signing`; `aura-frost` deprecated
- **Guardian recovery flow** → `aura-recovery`
- **Journal fact validation** → `aura-journal`
- **Network transport** → `aura-transport` (abstractions) + `aura-effects` (TCP implementation)
- **CLI command** → `aura-terminal`
- **Test scenario** → `aura-testkit`
- **Choreography protocol** → Feature crate + `aura-mpst`
- **Authorization logic** → `aura-authorization`
- **Social topology/relay selection** → `aura-social`
- **Quint specification** → `verification/quint/` + `docs/807_verification_guide.md`
- **Generative test** → `aura-simulator/src/quint/` + `docs/809_generative_testing_guide.md`

#### "I need to understand..."
- **How authorities work** → `docs/102_authority_and_identity.md`
- **How consensus works** → `docs/106_consensus.md`
- **How effects compose** → `docs/105_effect_system_and_runtime.md`
- **How protocols are designed** → `docs/108_mpst_and_choreography.md`
- **How the guard chain works** → `docs/001_system_architecture.md` (sections 2.1-2.3)
- **How crypto architecture works** → `docs/100_crypto.md` + `just check-arch --crypto`
- **How journals work** → `docs/103_journal.md`
- **How the query system works** → `docs/113_database.md` (Datalog queries, isolation levels, statistics)
- **How testing works** → `docs/805_testing_guide.md` + `docs/806_simulation_guide.md`
- **How to write tests** → `docs/805_testing_guide.md`
- **How privacy and flow budgets work** → `docs/003_information_flow_contract.md`
- **How distributed system guarantees work** → `docs/004_distributed_systems_contract.md`
- **How commitment trees work** → `docs/102_authority_and_identity.md`
- **How relational contexts work** → `docs/112_relational_contexts.md`
- **How transport and receipts work** → `docs/109_transport_and_information_flow.md`
- **How rendezvous and peer discovery work** → `docs/111_rendezvous.md`
- **How social topology and homes work** → `docs/114_social_architecture.md`
- **How state reduction works** → `docs/103_journal.md`
- **How the mathematical model works** → `docs/002_theoretical_model.md`
- **How identifiers and boundaries work** → `docs/101_identifiers_and_boundaries.md`
- **How authorization and capabilities work** → `docs/104_authorization.md`
- **How Biscuit tokens work** → `docs/104_authorization.md` + `aura-authorization/src/biscuit/`
- **How to get started as a new developer** → `docs/801_hello_world_guide.md`
- **How core systems work together** → `docs/802_core_systems_guide.md`
- **How to design advanced protocols** → `docs/804_advanced_coordination_guide.md`
- **How simulation works** → `docs/806_simulation_guide.md`
- **How verification works** → `docs/807_verification_guide.md` (Quint specs + Lean proofs)
- **How generative testing works** → `docs/809_generative_testing_guide.md`
- **How maintenance and OTA updates work** → `docs/808_maintenance_guide.md` + `docs/115_maintenance.md`
- **How development patterns work** → `docs/805_development_patterns.md`
- **The project's goals and constraints** → `docs/000_project_overview.md`

## Legacy Cleanup Status

- Graph-based `journal_ops` directory removed; guard/tests now track fact deltas.
- `DeviceMetadata`/`DeviceType`/`DeviceRegistry` removed - device information now derived from `LeafNode` in commitment tree (`aura-core/src/tree/types.rs`). Device views are obtained via `TreeEffects::get_current_state()`.

## Usage Efficiency Guidelines

To conserve agent usage, prefer:
- Specific file paths over broad searches when known
- Targeted grep patterns over reading entire files
- Architecture compliance (`just check-arch`) before complex refactoring
- Quick reference skills over re-reading documentation
- Batch operations and parallel tool calls when possible
- Use `.claude/skills/` for project-specific knowledge
- Note: `work/` is ignored; do not commit files from this directory

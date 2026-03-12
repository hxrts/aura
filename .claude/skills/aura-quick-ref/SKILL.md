---
name: aura-quick-ref
description: Essential Aura project overview including 8-layer architecture, authority model, guard chain, effect system rules, code location decisions, and development commands. Use when asked about project architecture, where code belongs, how systems work together, or starting work on any Aura task.
---

# Aura Quick Reference

## Critical First Steps
1. **Enter the Nix environment if not already in the shell**: `nix develop`
2. **Verify architecture**: `just check-arch`
3. **Check available skills**: `ls .claude/skills/` for specific guidance

## Project Identity (30-second overview)
**What**: Threshold identity platform using choreographic programming + algebraic effects
**Goal**: 20 close friends, twice weekly usage - eliminate single device or centralized service trust
**Core Innovation**: Authority-first design (opaque `AuthorityId` + relational `ContextId`) with session types
**Source of truth**: The `docs/` directory is the primary spec; `work/` is non-authoritative scratch.

## Three Pillars Architecture
1. **Algebraic State**: Fact-based CRDT journals (join-semilattice) + Biscuit capabilities (meet-semilattice)
2. **Choreographic Protocols**: Global protocol → local session types (deadlock-free, compile-time safe)
3. **Effect System**: Stateless handlers + guard chain (Authorization → Flow → Journal → Leakage → Transport)

**Hybrid journal model**: Fact journal (join) + capability frontier (meet) combined as JournalState for effects/runtime usage.

## Threshold Lifecycle Taxonomy
**Key generation (K)**: K1 Local/Single‑Signer, K2 Dealer‑Based DKG, K3 Quorum/BFT‑DKG.  
**Agreement (A)**: A1 Provisional, A2 Coordinator Soft‑Safe, A3 Consensus‑Finalized.  
Leader selection and pipelining are **orthogonal optimizations**. Fast paths (A1/A2) are provisional; durable shared state must be finalized by A3.

## 8-Layer Architecture (Zero Circular Dependencies)
1. **Foundation**: `aura-core` (effect traits, domain types, crypto utils, errors)
2. **Specification**: Domain crates (`aura-journal`, `aura-authorization`, etc.) + `aura-mpst` + `aura-macros`
3. **Implementation**: `aura-effects` (stateless handlers) + `aura-composition` (handler assembly)
   - Unified encryption-at-rest: `aura-effects::EncryptedStorage` wraps `StorageEffects` and uses `SecureStorageEffects` for master-key persistence
4. **Orchestration**: `aura-protocol` + subcrates (`aura-guards`, `aura-consensus`, `aura-amp`, `aura-anti-entropy`)
5. **Features**: `aura-authentication`, `aura-chat`, `aura-invitation`, `aura-recovery`, `aura-relational`, `aura-rendezvous`, `aura-social`, `aura-sync`, `aura-maintenance` (FROST primitives in `aura-core::crypto::tree_signing`)
6. **Runtime**: `aura-agent`, `aura-simulator`, `aura-app`
7. **Interface**: `aura-terminal` (CLI + TUI entry points; uses `aura-app`/`AppCore`)
8. **Testing**: `aura-testkit` (mocks/fixtures), `aura-quint` (formal verification)

## Authority & Identity Model
- **Authority**: Cryptographic actor with private internal structure (never exposed)
- **Account Authority**: Long-term identity with commitment tree managing device membership
- **Relational Context**: Shared state between authorities (`ContextId` scoped)
- **Key Insight**: Identity is contextual, not global. Relationships exist only within contexts.

## Terminology Baseline
- Role terms: `Member`, `Participant`, `Moderator`
- Access terms: `Full`, `Partial`, `Limited`
- Topology terms: `1-hop` / `n-hop`
- Storage/pinning terms: `Shared Storage`, `allocation`, `pinned`

## Guard Chain Enforcement (Critical Pattern)
Every transport send flows through:
```
Send Request → CapGuard (authorization) → FlowGuard (budget) → JournalCoupler (facts) → LeakageTracker → Transport
```
**No observable behavior without authorization, accounting, and leakage checks.**

## Effect System Rules (Architecture Compliance)
- **Infrastructure effects** → `aura-effects` (OS integration: crypto, network, storage, time)
- **Application effects** → domain crates (Aura-specific: journal, authority, flow budget)
- **Composite effects** → extension traits (convenience combinations)
- **NEVER** use direct impure functions (`SystemTime::now`, `thread_rng()`) outside effect implementations
- **ALWAYS** propagate `EffectContext` through async functions

## Shared UX / Harness Rules
- Parity-critical UI ids, focus semantics, and action contracts come from `aura-app::ui_contract`
- Harness mode may add instrumentation or render-stability controls, but must not change business-flow semantics
- Allowlisted harness-mode hooks must carry owner, justification, and design-note metadata in `scripts/check/ux-policy-guardrails.sh`
- Browser harness bridge surface changes must update `crates/aura-web/ARCHITECTURE.md` and `docs/804_testing_guide.md`
- Every `ParityException` must carry structured metadata in `aura-app::ui_contract`
- Parity-critical waits must bind to authoritative readiness, runtime-event, or quiescence contracts
- Observation must be side-effect free; recovery/retries are explicit and separate
- Shared UX docs and contributor guidance are synchronized through `scripts/check/ux-guidance-sync.sh` and `just ci-ux-policy`
- Shared scenarios stay actor-based and semantic-only; the legacy scripted scenario language is quarantined to explicit non-shared fixtures
- Extend typed validator domains first and keep `scripts/check/` wrappers thin when adding new shared UX policy

## Code Location Decision Tree
- **Single-party stateless operation** → `aura-effects`
- **Handler composition/assembly** → `aura-composition`
- **Multi-party coordination** → `aura-protocol` + Layer 4 subcrates
- **Domain-specific logic** → Domain crate (`aura-journal`, etc.)
- **Complete end-to-end protocol** → Feature crate (e.g., `aura-authentication`)
- **Effect trait definition** → `aura-core`
- **Mock/test handlers** → `aura-testkit`

## Essential Commands (Must be in nix shell)
```bash
# Always start with
just check-arch      # Catch violations early (run before any work)
just build           # Verify everything compiles
just test            # Run all tests

# Development cycle
just watch           # Auto-rebuild on changes
just fmt             # Format code
just clippy          # Lint (warnings as errors)
just ci-dry-run      # Full CI checks locally

# Testing patterns
just test-crate <name>    # Test single crate
just quickstart smoke    # Quickstart integration checks
just docs               # Generate documentation

# Hermetic builds (after dependency changes)
crate2nix generate      # Update Cargo.nix
nix build              # Hermetic build
nix flake check        # Hermetic tests
```

## Common Patterns & Anti-Patterns

### ✓ Correct Effect Usage
```rust
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::effects::RandomEffects;

async fn my_function<T: PhysicalTimeEffects + RandomEffects>(
    ctx: &EffectContext, effects: &T
) -> Result<Data> {
    let physical_time = effects.physical_time().await?;
    let timestamp = TimeStamp::PhysicalClock(physical_time);
    let nonce = effects.random_bytes(32).await;
    // Use timestamp and nonce...
}
```

## Storage Notes (Unified Encryption)
- Application code should treat `StorageEffects` as the only persistence surface.
- Production runtime wires `StorageEffects` through `EncryptedStorage` by default; `aura-agent` supports `StorageConfig.encryption_enabled` and `StorageConfig.opaque_names`.
- LocalStore no longer implements its own encryption; encryption happens beneath it via `EncryptedStorage`.

### ✗ Architecture Violations
```rust
let now = SystemTime::now();           // Use PhysicalTimeEffects instead
let random = thread_rng().gen();       // Use RandomEffects instead
let ts = chrono::Utc::now().timestamp(); // Use unified time system instead
```

## When You Need More Context
**For specific deep dives:**
- Authority model: `docs/102_authority_and_identity.md`
- Journal system: `docs/103_journal.md`
- Consensus & BFT-DKG: `docs/106_consensus.md` (includes transcript binding, coordinator selection)
- Effect system: `docs/105_effect_system_and_runtime.md`
- Choreographies: `docs/108_mpst_and_choreography.md`
- Database & transactions: `docs/113_database.md`
- Crypto & VSS: `docs/100_crypto.md` (includes lifecycle taxonomy, VSS, dealer packages)
- Operation categories: `docs/107_operation_categories.md` (per-ceremony K/A policy matrix)
- Full architecture: `docs/001_system_architecture.md`
- Crate structure: `docs/999_project_structure.md`

**For threshold lifecycle & ceremony questions:**
- Per-ceremony K/A policy matrix → `docs/107_operation_categories.md`
- Lifecycle patterns (0-4) → `docs/107_operation_categories.md`
- BFT-DKG transcript finalization → `docs/106_consensus.md` §19
- Coordinator selection (decentralized lottery) → `docs/106_consensus.md` §20
- VSS and dealer packages → `docs/100_crypto.md` §8

**For transaction & coordination questions:**
- How transactions work with consensus → `docs/113_database.md` §8 + `docs/106_consensus.md`
- Transaction coordination dimensions → `docs/113_database.md` §8
- Query isolation levels → `docs/113_database.md`

## Task-Oriented Quick Start

**"I'm debugging an error"** → Use the `architecture` skill (see troubleshooting.md)
**"I'm adding a new feature"** → Use the `architecture` skill for placement guidance
**"I'm working with the harness"** → Use the `testing` or `harness-run` skill
**"I'm writing tests"** → Use the `testing` skill for patterns and harness workflows
**"I don't know where to find X"** → Use the `architecture` skill
**"I'm new to the codebase"** → Read `docs/801_hello_world_guide.md`
**"I'm implementing database transactions"** → `docs/113_database.md` §8 → `docs/106_consensus.md`
**"I need to understand transaction isolation"** → `docs/113_database.md`
**"I'm sequencing consensus operations"** → `docs/108_mpst_and_choreography.md`
**"I need the harness reference"** → `docs/804_testing_guide.md`
**"I'm working on web/WASM"** → Use the `web` skill for Dioxus patterns
**"I'm implementing a ceremony"** → `docs/107_operation_categories.md` for K/A policy matrix
**"I need threshold signing details"** → `docs/100_crypto.md` for lifecycle taxonomy + VSS
**"I'm writing Quint/Lean specs"** → Use the `verification` skill
**"I'm reviewing code"** → Use the `style` skill for TigerStyle checklist

## Layer 5 Notes (Recent Conventions)
- Each Layer 5 crate now includes an `ARCHITECTURE.md` with facts/invariants/category table.
- `OPERATION_CATEGORIES` constants map operations to A/B/C gating classes.
- Runtime-owned caches live in Layer 6 handlers (avoid caches in Layer 5 services).
- Layer 5 facts use versioned binary encoding (bincode) with JSON fallback for debug.
- FactKey helper types are required for reducers/views; avoid ad-hoc tuple keys.
- Ceremony facts include optional `trace_id` for cross-protocol correlation.

## Project Constraints (The "Why")
Aura must simultaneously satisfy:
1. **Network as platform** - No separate infrastructure layer
2. **Privacy by design** - Selective, consent-based disclosure
3. **Cross-platform** - Web/mobile/desktop via WASM
4. **Social-graph coordination** - Discovery/storage via social network
5. **Offline-first** - Works in airplane mode, syncs when connected
6. **Decentralized secrets** - No external backups required
7. **No single point of failure** - Real security across devices/guardians
8. **Version compatibility** - Older clients work with newer ones

These constraints drive every architectural decision.

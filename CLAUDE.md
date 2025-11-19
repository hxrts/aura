# CLAUDE.md + AGENTS.md

## Project Overview

Aura is a threshold identity and encrypted storage platform built on relational security principles. It uses threshold cryptography and social recovery to eliminate the traditional choice between trusting a single device or a centralized entity.

**Architecture**: Choreographic programming with session types for coordinating distributed protocols. Uses algebraic effects for modular runtime composition. See `docs_2/001_system_architecture.md` and `docs/999_project_structure.md` for the latest crate breakdown.

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
- `nix build .#aura-cli` - Build specific package
- `nix run` - Run aura CLI hermetically
- `nix flake check` - Run hermetic tests
- `crate2nix generate` - Regenerate Cargo.nix after dependency changes

### Testing
- `just test` - Run all tests (preferred)
- `just test-crate <name>` - Test specific crate
- `just ci` - Full CI checks (format, lint, test)
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
- `just test-dkd <app_id> <context>` - Test key derivation

## Architecture Essentials

### 8-Layer Architecture

The codebase follows a strict 8-layer architecture with zero circular dependencies:

1. **Foundation** (`aura-core`): Effect traits (crypto, network, storage, time, journal, console, random, transport), domain types (`AuthorityId`, `ContextId`, `SessionId`, `FlowBudget`), cryptographic utilities (FROST, merkle trees), semilattice traits, unified errors (`AuraError`), and reliability utilities. Other crates depend on `aura-core`, but it depends on none of them.

2. **Specification** (Domain Crates + `aura-mpst` + `aura-macros`):
   - Domain crates (`aura-journal`, `aura-wot`, `aura-verify`, `aura-store`, `aura-transport`): CRDT domains, capability systems, transport semantics. `aura-journal` now exposes fact-based journals and reduction pipelines (`docs_2/102_journal.md`, `docs_2/110_state_reduction_flows.md`).
   - `aura-mpst`: Session type runtime with guard extensions and leakage tracking (`LeakageTracker`).
   - `aura-macros`: Choreography DSL parser/annotation extractor (`guard_capability`, `flow_cost`, `journal_facts`, `leak`) that emits rumpsteak projections.

3. **Implementation** (`aura-effects`): Stateless, single-party handlers (real & mock) implementing core effect traits (crypto, network, storage, randomness, console, leakage, etc.).

4. **Orchestration** (`aura-protocol`): Multi-party coordination and guard infrastructure: handler adapters, CrdtCoordinator, GuardChain (CapGuard → FlowGuard → JournalCoupler), Capability evaluator, Aura Consensus runtime, anti-entropy/snapshot helpers.

5. **Feature/Protocol** (`aura-authenticate`, `aura-frost`, `aura-invitation`, `aura-recovery`, `aura-rendezvous`, `aura-storage`, `aura-sync`): End-to-end protocol crates (auth, guardian recovery, rendezvous, storage, etc.) built atop the orchestration layer.

6. **Runtime Composition** (`aura-agent`, `aura-simulator`): Runtime assembly of effect systems (agent) and deterministic simulation (simulator). `aura-agent` now owns the effect registry/builder infrastructure; `aura-protocol` no longer exports the legacy registry.

7. **User Interface** (`aura-cli`): CLI entry points driving the agent runtime. Current CLI exposes scenario/admin/recovery/invitation flows plus the new authority/context inspection commands.

8. **Testing & Tools** (`aura-testkit`, `aura-quint-api`): Shared fixtures, simulation harnesses, property tests, Quint interop.

**Where does my code go?** See the docs under `docs_2/001_system_architecture.md` and `docs/AUTHORITY_ARCHITECTURE.md` for the latest authority-centric guidance.

## Architecture Essentials (Authority Model)

Aura now models identity via opaque authorities (`AuthorityId`) and relational contexts (`ContextId`). Key points:

- Ratchet tree updates and device membership are expressed as fact-based AttestedOps (`aura-journal/src/fact_journal.rs`). No graph-based `journal_ops` remain.
- Relational contexts (guardian bindings, recovery grants, rendezvous receipts) live in their own journals (`docs_2/103_relational_contexts.md`).
- Aura Consensus is the sole strong-agreement mechanism (`docs_2/104_consensus.md`). Fast path + fallback gossip integrate with the guard chain.
- Guard chain sequence: `AuthorizationEffects` (Biscuit/capabilities) → `FlowBudgetEffects` (charge-before-send) → `LeakageEffects` (`docs_2/003_privacy_and_information_flow.md`) → `JournalEffects` (fact commit) → `TransportEffects`.
- Flow budgets: only the `spent` counters are facts; limits are derived at runtime from Biscuit + policy.

## Distributed Systems Contract

See `docs_2/004_distributed_systems_contract.md` for the distributed-systems guarantees (safety, liveness, partial synchrony assumptions, latency expectations, adversarial models, and monitoring guidance).

## Information Flow / Privacy

Reference `docs_2/003_privacy_and_information_flow.md` for the unified flow-budget/metadata-leakage contract. Key notes:
- Charge-before-send invariant enforced by FlowGuard + JournalCoupler.
- Receipts propagate via relational contexts (`docs_2/107_transport_and_information_flow.md`).
- Leakage budgets tracked via `LeakageEffects` and choreography annotations.

## Authorization Systems

1. **Traditional Capability Semantics** (`aura-wot`): Meet-semilattice capability evaluation for local checks.
2. **Biscuit Tokens** (`aura-wot/src/biscuit_*`, `aura-protocol/src/authorization/*`): Cryptographically verifiable, attenuated tokens.
3. **Guard Integration**: `aura-protocol::guards::{CapGuard, FlowGuard, JournalCoupler, LeakageTracker}` enforce Biscuit/policy requirements, flow budgets, journal commits, and leakage budgets per message.

## Documentation Map

- Core overview: `docs/000_project_overview.md`
- Theoretical model: `docs_2/001_theoretical_model.md`
- Architecture: `docs_2/001_system_architecture.md`
- Privacy: `docs_2/003_privacy_and_information_flow.md`
- Distributed systems contract: `docs_2/004_distributed_systems_contract.md`
- Authority/Relational identity: `docs/AUTHORITY_ARCHITECTURE.md`, `docs_2/103_relational_contexts.md`
- Consensus: `docs_2/104_consensus.md`
- Transport/receipts: `docs_2/107_transport_and_information_flow.md`, `docs_2/108_rendezvous.md`
- Developer guides: `docs/803_coordination_systems_guide.md`, `docs/804_advanced_choreography_guide.md`, `docs/805_testing_guide.md`
- Reference: `docs/999_project_structure.md`

## Legacy Cleanup Status

- Graph-based `journal_ops` directory removed; guard/tests now track fact deltas.
- `DeviceMetadata`/`DeviceType` removal in progress (see work plan in `refactor.md`). Until the new authority-derived device view lands, legacy structs remain in `aura-journal::types`, ledger APIs, and testkit builders.


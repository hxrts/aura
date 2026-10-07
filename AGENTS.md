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
- **Scratch**: scratch notes are non-authoritative and may be removed

### Documentation Layers

Three documentation layers serve distinct purposes.

The 100-series docs (`docs/1xx_*.md`) are architectural specifications. They define system contracts, invariants, behavioral guarantees, type definitions, and protocol specifications. They describe what the system is and what it guarantees. They do not contain implementation patterns, coding instructions, or step-by-step guidance.

The 800-series docs (`docs/8xx_*.md`) are developer guides. They contain implementation patterns, usage examples, decision frameworks, coding instructions, and operational workflows. They describe how to build with and operate the system. They reference 100-series specs for the underlying contracts but do not redefine them.

Per-crate `ARCHITECTURE.md` files describe a single crate's purpose, scope, dependencies, invariants, ownership inventory, and testing strategy. They follow a normalized template and cross-reference both 100-series specs and 800-series guides where appropriate. Read the relevant `ARCHITECTURE.md` before making changes in a crate.

## Development Commands

**Required**: Nix with flakes enabled. Run `nix develop` first.

The Nix shell dispatches Cargo and Clippy from its pinned Rust toolchain. Keep
this scoped dispatch: Cargo otherwise prefers installed Cargo-home plugins,
which can select an older Clippy even when the Nix toolchain leads PATH.
`just ci-build-cache-policy` verifies argument and exit-status forwarding.

The Cargo development profile disables incremental compilation by default,
including ordinary Cargo and editor checks. `CARGO_INCREMENTAL=1` remains an
explicit override. The cache-policy gate compiles a tiny isolated probe to
verify both effective compiler settings without building the workspace.

| Category | Command | Purpose |
|----------|---------|---------|
| Build | `just build` | Build all crates |
| Build | `just build-release` | Build and install the deployable terminal release binary |
| Build | `just build-workspace-release` | Full workspace release validation |
| Build | `just e2e-build-terminal`, `just e2e-build-web`, `just e2e-build-harness` | Disk-budgeted LAN rebuilds; run on each host |
| Build | `scripts/harness/lan/build.sh <lane>` | Tracked LAN host build entry point with optional `AURA_EXPECT_COMMIT` check |
| Build | `just disk-report`, `just cache-inventory`, `just build-budget-dry-run` | Read-only disk and cache inventory, cleanup preview |
| Build | `just prune-inactive-lane wasm-debug --apply` | Guarded whole-lane cleanup when that lane is idle |
| Build | `just prune-inactive-lane debug-incremental --dry-run` | Preview an idle incremental cache without removing loaded debug libraries |
| Build | `just check` | Check without building |
| Build | `just clippy` | Lint (warnings as errors) |
| Build | `just ci-policy-toolkit-clippy` | Strict all-target lint of the excluded repo-local policy toolkit; also runs in `just ci-clippy` |
| Format | `just fmt` | Format code |
| Format | `just fmt-check` | Check formatting |
| Test | `just test` | Run all tests |
| Test | `just test-crate <name>` | Test specific crate |
| Test | `just ci-dry-run` | Local CI checks |
| Security | `just ci-security-audit` | Dependency advisory policy gate (`cargo deny` with RustSec fetch) |
| Nix | `nix build` | Hermetic build |
| Nix | `nix flake check` | Hermetic tests |
| Nix | `crate2nix generate` | Regenerate after dep changes |
| Dev | `just watch` | Rebuild on changes |
| Dev | `just clean` | Clean artifacts |
| Arch | `just check-arch` | Verify architecture compliance |
| Test | `just ci-build-cache-policy` | Verify guarded builds, cache cleanup and E2E evidence retention with isolated fixtures |
| Arch | `just ci-ownership-policy` | Run ownership/runtime boundary enforcement |
| Arch | `just lint-arch-syntax` | Run Rust-native syntax/policy lints that replaced grep-heavy `arch.sh` checks |
| Arch | `just ci-annotation-ratchet` | Run changed-files ownership annotation ratchets and ignored-test-count ratchets |
| Arch | `just ci-frontend-portability` | Run shared frontend portability and semantic-bridge syntax lints |
| Arch | `just ci-frontend-handoff-boundary` | Run frontend semantic owner allocation / handoff boundary lints |

### Cargo Package Names

Published workspace crates use `hxrts-aura-*` Cargo package names even though their source directories remain `crates/aura-*`. When writing hooks, CI scripts, or raw Cargo commands, target package ids such as `hxrts-aura-app`, `hxrts-aura-agent`, `hxrts-aura-core`, `hxrts-aura-macros`, and `hxrts-aura-protocol` instead of the legacy `aura-*` package selectors. Non-published shells and test infra such as `aura-terminal`, `aura-ui`, `aura-web`, `aura-harness`, `aura-simulator`, `aura-quint`, and `aura-testkit` keep their existing package names.

## Architecture Overview

### 8-Layer Structure

| Layer | Crates | Purpose |
|-------|--------|---------|
| L1 Foundation | `aura-core` | Effect traits, domain types, crypto utilities |
| L2 Specification | `aura-journal`, `aura-authorization`, `aura-signature`, `aura-store`, `aura-transport`, `aura-maintenance`, `aura-mpst`, `aura-macros` | Domain semantics, no runtime |
| L3 Implementation | `aura-effects`, `aura-composition` | Stateless handlers, composition |
| L4 Orchestration | `aura-protocol`, `aura-guards`, `aura-consensus`, `aura-amp`, `aura-anti-entropy` | Multi-party coordination |
| L5 Features | `aura-authentication`, `aura-chat`, `aura-invitation`, `aura-recovery`, `aura-relational`, `aura-rendezvous`, `aura-social`, `aura-sync` | End-to-end protocols |
| L6 Runtime | `aura-agent`, `aura-simulator`, `aura-app` | System assembly |
| L7 Interface | `aura-terminal`, `aura-ui`, `aura-web` | Terminal shell, shared UI core, and browser shell |
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
- **Harness mode exceptions**: allowlisted harness-only hooks must carry owner, justification, and design-note metadata enforced by `toolkit/xtask` via `just ci-user-flow-policy`
- **Browser bridge compatibility**: changes to browser harness bridge, bounded browser task ownership, or observation surfaces must update `crates/aura-web/ARCHITECTURE.md` and `docs/804_testing_guide.md`; this includes the explicit `stage_runtime_identity` bootstrap handoff and the page-owned semantic queue (`window.__AURA_DRIVER_SEMANTIC_ENQUEUE__`)
- **Parity exception metadata**: every `ParityException` must have structured metadata in `aura-app::ui_contract` including reason code, scope, affected surface, and doc reference
- **Parity-critical waits**: use authoritative readiness, event, or quiescence contracts; raw sleeps, raw polling, and fallback text/DOM checks are diagnostics only
- **Authoritative fact syntax scope**: frontend authoritative-fact restrictions
  inspect real Rust paths, including macro tokens, with lexical `cfg(test)`
  classification. Comments, strings, or an earlier test module cannot exempt
  later production code; mixed `cfg(any(test, feature = ...))` stays checked.
- **Canonical entity materialization only**: reactive/view/harness-facing code may enrich already-materialized channel or invitation state, but it may not fabricate canonical metadata from partial facts such as membership events or raw ids; one explicit owned path must materialize the canonical entity shape end to end
- **Channel creation witness**: `aura-chat::CanonicalChannelCreation` comes only from `ChannelCreated`; `ChannelUpdated`, messages, name hints, and prior UI snapshots may stage or enrich but cannot insert a visible channel. Its constructor stays private to `aura-chat`, and `ChatViewReducer` use outside the sanctioned owner modules fails the ownership policy gate. Keep pure creation evidence out of capability-boundary helper inventories. A plain fact shape does not prove journal commit provenance; that requires a journal-issued token.
- **Reactive subscriptions**: subscribing before registration must fail fast; lagging subscribers may miss intermediate updates and resume from a newer snapshot
- **Shared user-flow documentation sync**: shared user-flow contract or policy changes must update the mapped authoritative targets enforced by `toolkit/xtask` via `just ci-user-flow-policy`
- **Shared user-flow contributor sync**: when shared UX policy checks change (Rust in `toolkit/xtask/src/checks/policy.rs` or shell wrappers in `scripts/check/`), keep `AGENTS.md` and the mapped local skills aligned with the updated contributor guidance in the same change
- **Enrollment verifier custody**: quorum signature verification retains the
  original approved native threshold policy or sealed issued/retained enrollment
  owner. Raw verifier bytes and self-certified manifest fields cannot replace
  that custody; typed trusted-key checks require exact owner/accessor origins
  with negative regression coverage.
- **Security boundary policy sync**: when adding or changing security-sensitive toolkit checks in `toolkit/xtask/src/checks/policy.rs`, keep this guidance aligned and run `just _policy-check check security-boundary-policy` before broader CI
- **Shared scenario boundary**: shared scenarios stay actor-based and semantic-only; the legacy compatibility-step scenario language is quarantined to explicit non-shared fixtures
- **Typed governance first**: extend typed validator domains before adding new shell policy logic; `scripts/check/` wrappers should stay thin and workflow-oriented
- **Optional scratch governance**: ownership checks must work in clean
  checkouts without ignored `work/` notes; absent scratch cannot exempt
  required production roots or exception metadata.
- **Authoritative-ref discipline**: once parity-critical code has
  authoritative context, later APIs must require the strongest typed reference;
  raw-id re-resolution, `resolve_*` downgrade, and `*_or_fallback` repair are
  forbidden on that path
- **Frontend ownership discipline**: parity-critical frontend submission uses only the sanctioned local-terminal / workflow-handoff owner path; browser/TUI task ownership uses only `WebTaskOwner` / `UiTaskOwner`; readiness refresh remains private to `aura-app::workflows`
- **Shared semantic lifecycle ownership**: `aura-app::workflows` owns authoritative parity-critical semantic lifecycle publication after handoff; `aura-terminal`, `aura-web`, and `aura-harness` submit and observe but do not keep parallel terminal publication paths
- **Frontend/app facade boundary**: frontend parity-critical imports go through `aura_app::ui` and `aura_app::ui::workflows`; do not reach into crate-root `aura_app::workflows` or private semantic helper modules
- **Runtime-private ownership boundaries**: raw VM admission helpers, VM fragment ownership registry mutation, and reconfiguration-controller internals stay internal to `aura-agent`; use the sanctioned ingress and manager surfaces instead
- **Runtime structured-concurrency boundary**: production raw spawn stays inside `aura-agent::task_registry`; long-lived runtime services use bounded actor ingress and owned task handles instead of ad hoc `tokio::spawn`
- **Architecture enforcement split**: prefer type/API design, compile-fail
  tests, and Rust-native lints for syntactic or boundary-shape rules;
  `just check-arch` should stay focused on workspace topology, governance, and
  integration checks that are not realistically provable at compile time
- **Test-scope enforcement**: classify Rust test exclusions by parsed lexical
  scope and positive cfg predicates. Mixed test/production predicates and
  production declarations following test items retain production enforcement.
- **Architecture syntax lint gate**: run `just lint-arch-syntax` when changing
  effect placement, runtime-coupling, raw impure/time/random usage,
  concurrency escape hatches, crypto-boundary syntax, or syntax-owned
  serialization/style rules
- **Retained `check-arch` scope**: dependency direction, `ARCHITECTURE.md`
  invariants/docs governance, reactive and ceremony integration heuristics,
  workflow docs traceability, canonical wire-format integration checks, repo
  hygiene, and test-seed uniqueness remain shell-owned checks
- **Ownership CI gate**: run `just ci-ownership-policy` when changing
  parity-critical ownership/runtime boundaries; it is the default aggregate lane
  for compile-fail ownership guards, Rust-native ownership lints, retained
  runtime/integration checks, and governance wrappers
- **Required VM lifecycle evidence**: run `just ci-vm-session-lifecycle` after
  changing session close/reap, coroutine indexing, worker acknowledgment, VM
  drop custody, session owner capabilities, or the pinned engine override. The ownership aggregate includes
  this serialized lane. Its Rust inventory requires nonignored test declarations
  and actual test-harness discovery before execution; a missing or zero-match
  required fixture cannot pass as clean. Opaque session owner observation and
  reconstruction doctests must also be published by the actual harness.
- **Annotation ratchet gate**: new parity-critical workflow boundaries,
  runtime services, and first-party capability gates must pass the
  changed-files ratchets in `just ci-annotation-ratchet`;
  the same lane also enforces the ignored-test-count ratchet, so new
  `#[ignore]` coverage must carry an intentional inventory update;
  prefer adding the declaration-layer attribute over adding a shell allowlist.
  A public `*_with_terminal_status` handoff may delegate to a private
  `#[semantic_owner]` function when the attribute names that exact public
  wrapper; both the changed-files ratchet and Rust-native ownership lint
  verify this declared relationship
  Signature changes must retain the attribute on the actual Rust declaration;
  unchanged diff context is accepted through syntax inspection. An attribute
  on another function or an identically named unannotated boundary does not
  authorize the change.
- **Frontend handoff boundary**: direct `LocalTerminalOperationOwner::submit`
  and `WorkflowHandoffOperationOwner::submit` allocation stays inside the
  sanctioned terminal/browser submission boundaries; callback factories and
  bridge helpers must go through the exported submit helpers instead of
  allocating owners ad hoc
- **Account creation handoff**: `CreateAccountCallback` requires a workflow
  handoff owner. Runtime-free native staging delegates actual profile writes to
  the app-owned producer and preserves its original operation instance through
  runtime attachment. `just ci-frontend-handoff-boundary` rejects parsed
  frontend-local submissions with `SemanticOperationKind::CreateAccount`;
  comments, strings, and genuine test scopes do not authorize production bypasses.
- **Shared frontend portability**: code under
  `aura-app::frontend_primitives` must stay wasm-safe and platform-neutral;
  do not introduce blocking locks, native thread primitives, platform-specific
  spawn/sleep helpers, or handwritten browser-only coordination there

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
├─ Shared UI/view logic → aura-ui (L7)
├─ Browser/WASM shell → aura-web (L7)
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
| L7 (`aura-terminal`, `aura-ui`, `aura-web`) | Terminal/browser shells, shared observed UI core, bounded ingress/bridge mechanics | Business logic, parity-critical semantic lifecycle ownership, readiness publication (keep those in `aura-app`) |
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
| Shared UI/view logic | `aura-ui` |
| Browser frontend / harness bridge | `aura-web` |
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
- parity-critical APIs must accept the strongest available typed input; raw ids
  may identify but may not authorize once authoritative context exists
- authoritative workflow/runtime slices must not call fallback helpers or
  re-resolve context from weaker ids after handoff
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

- Guard repeated Cargo/Dioxus builds with `scripts/dev/build-budget.sh`;
  it defaults to `CARGO_INCREMENTAL=0`. Opt in explicitly with
  `CARGO_INCREMENTAL=1` when measured rebuild savings justify cache retention.
- Use `scripts/harness/lan/build.sh all` for a clean, fixed-commit,
  sequential terminal/web/harness build cycle on each host.
- Write new repository automation as Bash `.sh` scripts, not Python scripts.
- Prefer specific file paths over broad searches
- Use `just check-arch` before complex refactoring
- For shared user-flow or harness policy work, run `just ci-user-flow-policy`
- Use `.claude/skills/` for project-specific knowledge
- Batch operations and parallel tool calls when possible

### Build and caching

See `docs/804_testing_guide.md` "Build and Caching" for details.

- All worktrees share one sccache store (`RUSTC_WRAPPER=sccache`,
  `SCCACHE_DIR=~/.cache/aura-sccache`, 10G cap), so dependencies compile once.
  Opt out with `AURA_NO_SCCACHE=1` (shell entry or a single cargo command).
- `scripts/dev/build-budget.sh` builds into the checkout's own `target/`,
  sweeps it to a per-checkout cap (10 GiB default) automatically, and admits a
  build only if the volume stays above a shared 15 GiB floor after other
  admitted builds' reservations. Do not run `cargo sweep` by hand.
- Two worktrees can build at once: `git worktree add ../aura-wt2 <branch>`,
  enter `nix develop` there, and build in one while editing the other.
- While iterating, build narrowly (`-p <crate>`, one `--test` binary, a lib
  filter). For local `aura-agent` loops use `CARGO_INCREMENTAL=1` (sccache
  passes those through); CI and gates stay non-incremental.
- `aura-agent` integration tests are aggregated binaries (`autotests = false`);
  add new test files as a `mod` of the matching root in `crates/aura-agent/tests/`.
- Run `just web-check` in pre-ship batches. `scripts/harness/lan/ship.sh`
  builds from a clean commit; do not edit that checkout while a ship build
  runs. It builds `aura` and `tool_repl` with crate2nix (`just nix-build-lan`)
  and sends them with `nix copy`; the web bundle stays on `dx`. Run
  `crate2nix generate` after dependency changes; `just nix-store-gc` reports
  (and `--apply` collects) unreachable store paths.

### Durable enrollment window discipline

Production enrollment uses the runtime-private sealed `EnrollmentWindowCapability` for execution, children, retries, and required observation acknowledgment. Do not introduce raw/no-op timeout executors, aliases, fresh duration reconstruction, or weaker budget parameters on that path. The Rust-native `async-session-ownership` lane requires sealed window inputs on attempt functions and methods regardless of parameter name; explicit test-only fixtures must use a positive test predicate. Required maintenance uses fallible owned interval outcomes and preserves concrete sources through service supervision. Run ownership and annotation gates when changing these APIs.

## Trusted enrollment verifier governance

The trusted-key boundary gate uses lexical Rust AST scope and exact key origin for enrollment verifier owners. Canonical sealed admission/retained references must supply their own expected key; comments, nearby resolver names, raw remote fields and aliases cannot substitute for that origin. Manifest signature integrity is distinct from runtime admission and requires an independent verifier argument. Test exclusion must be an actual cfg(test) scope, including all/any semantics, and must not hide subsequent production code. Update AST negative fixtures for ownership changes and run `just _policy-check check security-boundary-policy` before broader CI.

Cancelled enrollment notification recovery uses the original retained clock and
its distinct negative-only capability. `just ci-vm-session-lifecycle` also requires
negative-owner fault/expiry regressions and genuine two-runtime signed notice
delivery after reopening the original persisted runtime. Local recovery ingress
bounds cannot renew signed notice eligibility.

Public FROST effect changes require `just ci-public-frost-signing`: actual
nonignored audited signing/substitution tests and the public-only API guard must
be discoverable. Primitive evidence does not establish runtime quorum custody,
nonce retirement or recovery. Never replace failed real crypto in a verification
test with synthetic success bytes.

Confirmed enrollment public archive changes require `just ci-vm-session-lifecycle`
with the actual confirmation fixture and non-cloneable/non-deserializable ownership
guard. Preserve original immutable archive versions; explicit reverified receipt
publication is required for a new version. These tests do not establish profile
WAL or process restart continuity.

Generation history/live-slot changes require the same lifecycle gate's actual
legacy migration and same-epoch reissue tests. Retire wrapping secrets through
their original allocation custody; generic immutable Delete stays forbidden.

Admitted window persistence changes require that gate's actual anchor/checkpoint
and interval regressions. Preserve the original protected deadline and reject
missing checkpoints after ever-live acknowledgment; reimport cannot renew it.

Historical enrollment response-policy changes must retain the genuine missing-field
runtime fixture in the native VM lifecycle inventory. Protected original tracker
registration and setup evidence are required for supplementation; reconstructed
threshold arithmetic is not evidence. Run the lifecycle gate when changing this
boundary and keep `docs/104_runtime.md`, `docs/804_testing_guide.md`, and the agent
architecture aligned. Profile migration does not replace old-schema coverage.

VM lifecycle enforcement must verify exact per-test executed success after source
inventory and harness discovery. Do not treat a zero-test green Cargo process or
listing-only evidence as coverage; retain the adversarial execution-evidence
regressions when changing this gate.

Enrollment signing-owner changes must retain the actual finalized-threshold
native classification fixture in the VM lifecycle inventory. Genuine retained
threshold material requiring a coordinator remains Service with its concrete
QuorumOwnerRequired source; actual protected material loss remains Storage.
Do not manufacture solo authority from one share or backend support from metadata.

Required compile-fail ownership coverage must execute with Cargo available; missing or unsuccessful Cargo invocation is a failure. App/agent/signals suites use aura-testkit's one workspace-derived descriptor lock with bounded acquisition and never remove its lock inode. The ownership aggregate explicitly enables signals and runs its nonempty guard suite, agent guards, and the real forced-process lock recovery test. Do not restore directory polling locks or feature-disabled empty passing guard tests.

Required ownership coverage checks three separate forms of evidence: Rust test attributes without ignore annotations, exact names published by the selected harness, and successful execution of every required test. The signals harness is selected with `--features signals`. The process-lock lane requires the forced termination, shared namespace and native IO source regressions; its child-process helper does not satisfy coverage. Captured pretty harness output keeps nested trybuild diagnostics from splitting result lines. Zero-test, ignored, failed and name-lookalike output cannot satisfy the gate. When adding a required ownership test, update the typed suite inventory and its validator regressions; do not replace execution evidence with a successful Cargo exit status.

Runtime deterministic RNG clones must retain the original shared stream owner; never clone seeded generator state into a new lock. Ownership CI requires actual native interleaved-clone/continuation and independent-seed reproducibility regressions with source, discovery and execution evidence.

Production task-spawn policy excludes only parsed configurations requiring
`test`; do not weaken it with substring detection, `not(test)` or `any(test,
unix)` exclusions. Concurrency fault-injection coverage stays test-only, and
ownership CI requires exact execution of the configuration regression.

All required compile-fail harnesses, including core/macros, share the persistent
host-only descriptor lock in toolkit/test-support/process_lock.rs, reexported
by aura-testkit. Foundation tests use its unpublished host package rather than importing
higher-layer Aura crates. Never restore directory locks, per-suite lock names,
or successful Cargo-unavailable skips.

Capability-boundary changes require the full macro validator regressions and compile-fail suite, followed by ownership CI. Use an exact signature type; when a semantic capability label differs, add `capability_type = Type`. A held runtime receiver can declare `receiver_type = OwnerType` and must pass the generated concrete type check. Do not add decorative capability constants or annotate String/AuraError as authorization evidence. Classify pure validators and observed projections accurately. Readiness publication helpers retain the actual readiness capability through publication. The aggregate requires source attributes, actual discovery and successful exact execution of the full-validator adversarial tests; compile-fail snapshots must be checked against real compiler output.

Every ownership lint test-only exception uses parsed cfg entailment, including
semantic and enrollment-window policies. Do not introduce a second substring
predicate or exempt a production-capable configuration by its spelling.

Capability type matching retains configured type/const generic arguments.
Borrow lifetimes may be omitted from nominal labels; do not erase unrelated
value types while matching a configured owner or capability container.


### Allocation lifetime provider ownership

Selected allocation lifetime factories and provider identities originate only
from the actual exclusive physical profile owner. Native crypto retirement
requires the held original negative decision capability; positive sealing
requires the original activation capability. A serialized allocation locator
is observation, never recovery or retirement authority. Trusted custom effect
implementations are an explicit provider boundary.

Run `just _policy-check check security-boundary-policy` before broader CI when
changing these seams. Its Rust syntax check covers factory references, UFCS
aliases and macro tokens, while private constructors, declaration attributes
and compile-fail guards enforce actual capability custody. Keep immutable
legacy secrets permanent; allocation tombstone ACK is not a physical or backup
erasure guarantee.

Selected secret-lifetime changes must pass the security-boundary-policy gate's
required source/discovery/execution inventory for all native provider regressions,
including physical custody, ACK faults, legacy lifecycle loss, codec domains and
whole-profile byte limits. Provider tests cannot silently disappear or be ignored.

Capability-boundary result evidence follows only success values, including the
canonical AgentResult alias. Error-arm types, labels and body markers never
supply custody; retain negative full-validator coverage and actual compilation.

Allocation lifetime syntax checks normalize source paths against the explicit
checkout root before comparing sanctioned owner files. Outside-checkout sources
fail closed. The security gate self-checks absolute owner and foreign paths;
retain these regressions when changing file discovery or path handling.

For a scoped secret-codec exception, run both
`just _policy-check check secret-field-wrappers` and
`just _policy-check check security-exception-metadata`. The complete security
boundary gate includes both; a focused checker must name a registered command.

`just _policy-check check secret-lifetime-regressions` runs the security
gate's exact selected-provider source/discovery/execution inventory as a focused
lane. It requires all declared native tests to execute successfully with no
ignored or zero-test substitute. Run it for private lifetime codec/provider changes.

Custom runtime provider changes must preserve selected-handler dispatch through
async/sync assembly and persistent/auth/journal owners. Run the ownership
aggregate's exact `custom_provider_fidelity` native harness; configured outages
must retain their actual cause and cannot select default providers. Keep mutable
provider sentinels in L8 and secure lifetime/profile custody independent of
ordinary storage injection.

Owned Testing profile assembly changes must preserve the three actual custody
regressions in the required VM lifecycle source/discovery/execution inventory:
lease retention through shutdown, foreign configuration rejection, and ordinary
Testing rejection of production lease ingress. They supplement the connected
enrollment history and cancelled-notice restart evidence.

Required identity envelope changes retain the exact bootstrap codec/AEAD/bounds
regression in the VM lifecycle execution inventory. The strongest original
runtime/epoch/participant context remains required; failed canonical decoding
cannot select a companion package or older identity. Preserve bounded encoded
input and nonempty bounded ciphertext before cryptographic work.

Active identity handler changes preserve required rendezvous corrupted-primary
and contact missing-key regressions in the VM lifecycle inventory. Fixtures use
actual canonical threshold bootstrap and matching runtime authority; do not
install raw guessed-epoch packages. Failed signing identity selection or package
reads must retain their typed cause and cannot report response success.

Rendezvous manager identity ingress retains the actual selected runtime and
active physical identity context. Preserve its required corrupted-primary
regression, concrete provider/codec source, and absent-descriptor postcondition
in the VM lifecycle inventory. Generic permissive storage/crypto mocks cannot
prove authoritative identity selection.

Capability declaration matching rejects foreign qualified container lookalikes.
Preserve canonical wrapper paths, exact capability generic arguments, and the
required full-validator foreign Result/Arc/AgentResult regressions. Unqualified
names remain declaration syntax; opaque APIs and compiler checks establish actual
custody independently.

Nonproduction default-profile factory changes preserve required exclusive
namespace collision/native creation-fault regressions in ownership CI. Return
only freshly created paths; propagate original IO sources through runtime
construction. Never reuse an unchecked fallback after collision exhaustion.

Required invitation identity evidence belongs to `just ci-vm-session-lifecycle`.
Its Rust inventory parses exact `large_stack_async_test!(name, { ... })`
declarations and recursively rejects ignored attributes; actual harness
discovery and successful execution remain mandatory. Do not replace required
Contact success/refusal with wrapper tests or raw-key fixtures. Existing legacy
stack-adapter execution is not ordinary-stack proof. Original signer recovery
must be load-only after fresh issuance; missing protected records cannot renew
the signer epoch.

Required Guardian recovery pair and invitation transcript boundary changes retain
actual owned-profile reopen/concurrent-pair/native-cause regressions in
`just ci-vm-session-lifecycle`. Canonical codec evidence runs under
`hxrts-aura-signature`, runtime custody evidence under `hxrts-aura-agent`.
Preserve package-specific source discovery and exact execution. Partial-key
rejection does not establish interrupted-birth recovery or authorize rekeying.

Runtime activity observation must not expose admission closure or completed
shutdown publication. Keep original gate/effects/facade lease factories annotated
with their exact declaration-layer capability types and authoritative source.
Retain all three actual activity getter doc guards in the VM lifecycle inventory.

Contact response verification must retain its actual imported-record/decision
capability through the primitive and terminal handoff. Raw public-key slices or
copied metadata cannot replace the original verifier owner; code-key continuity
does not authorize device membership. Run the trusted-key/security policy gates,
required Contact regressions and toolkit strict lint after changing this boundary.

### Sync failure provenance checks

For new sync codec, transport, or journal producer adapters, compose a source-free `SyncDiagnostic` and retain the actual native error through `sync_error_with_cause`. Do not convert an existing error or terminal outcome into that diagnostic, replace a pre-existing source, or infer retry/permission from text. Run `just ci-vm-session-lifecycle` for the exact native-source and compile-fail inventory after changing these boundaries.

### Guardian verification role discipline

Guardian primitive verification requires the exact private original-pair,
imported-confirmation or issued-invitation possession capability for that role.
Raw keys and response fields cannot replace its owner. Keep local pair integrity,
imported issuer continuity and first-binding recovery possession distinct from
trusted device membership. Changes must preserve actual runtime/lease checks and
pass the Rust key-origin validators plus `just _policy-check check security-boundary-policy`.

The owned Guardian verifier sites use Rust-native canonical transcript byte-origin
validation. Preserve the actual SecurityTranscript declaration, required encoding
and immutable byte binding; raw bytes, shadowed bindings and fallback encodings
are rejected even when nearby text mentions a transcript. Run the signed-transcript
boundary gate with the required security policy after changing this source flow.

Security bypass test exclusions use lexical Rust cfg(test) ownership. Mixed
`cfg(any(test, production_feature))`, test-like names and nearby comments do not
exempt production code; preserve the actual-source scope rejection regressions.

## Required reactive publication ownership

Required runtime processing uses a scheduler-issued exact target retained from
accepted publication. Keep original runtime ingress and operation/startup window
through mutation, replay and retries. Diagnostic Batch subscriptions cannot
prove completion; standalone or foreign ingress cannot attach to canonical
runtime publication. Preserve native queue/watch/time failures and exact required
lifecycle source/discovery/execution evidence. Configured-view processing does
not replace canonical entity or app semantic readiness.

Registered Sync commands retain actual runtime-issued admission and task-root
custody. Required peer work uses the original resource window; manager stop
precedes root cancellation under the same original shutdown capability. Idle
peer discovery and local session retirement do not prove peer synchronization
or remote teardown.

### Checked runtime entropy admission

Production runtime assembly must reject deterministic seeds before profile acquisition and configured provider invocation, including custom real-crypto assembly. Deterministic crypto and RNG producers require the private checked nonproduction seed owner. Preserve the exact rooted source path, actual qualified execution-mode type, canonical Result/Option/error provenance, and factory/receiver metadata in the entropy-origin gate; comments and simulation names do not authorize its private seed source. Run `just _policy-check check security-boundary-policy` and the required secret-lifetime inventory when changing this boundary. The inventory requires actual constructor and absolute-path/adversarial AST execution evidence; excluded toolkit targets use their explicit manifest, never the workspace package selector alone.

- Raw threshold service enforcement uses the shared Rust cfg classifier through
  the toolkit policy API. Test-only scope must be established by cfg semantics;
  mixed production/test predicates remain production for quorum primitive fences.

- Required terminal clock observations use the original fixed physical endpoint.
  Provider wrappers forward `wait_until_physical_deadline` to their configured
  provider; cached-time relative sleeps cannot bound a required clock read.
  Run `just _policy-check check absolute-time-observation` for this boundary.
  The same exact nonignored source/discovery/execution inventory runs in
  `just ci-vm-session-lifecycle`; a focused pass is not whole-runtime proof.

Architecture reactive checks inspect direct generic fact commits inside actual
`#[semantic_owner]` declarations. Use the required commit and processing
capabilities when terminal success requires projection acknowledgment. Durable
actor publication alone does not promise UI readiness: preserve the distinct
`InvitationAccepted` and `ContactLinkReady` contracts. Comments, marker words,
and unrelated await helpers cannot establish owner completion. Required
`just ci-policy-toolkit-clippy` executes the AST regressions before strict lint.

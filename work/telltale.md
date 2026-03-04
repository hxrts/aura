# Telltale Integration Analysis for Aura

This document captures the analysis of Telltale's capabilities and recommendations for expanded integration with Aura's runtime and protocol systems.

---

## Telltale Reference (DeepWiki)

**Repository**: [hxrts/telltale](https://github.com/hxrts/telltale)
**Documentation**: [DeepWiki - Telltale](https://deepwiki.com/hxrts/telltale)

### What is Telltale?

Telltale is a combined formal verification framework and Rust implementation for **asynchronous buffered multiparty session types (MPST)**. Its central goal is to statically prevent deadlocks and enforce protocol conformance in distributed systems, bridging mechanized proof and production-usable runtime code.

The system addresses a specific gap in the MPST literature: prior work formulates coherence at the global-type level, requiring per-step global re-derivation during preservation proofs. Telltale replaces this with a local, compositional **operational coherence invariant** `Coherent(G,D)` over local-type environments and buffered-trace environments, mechanized in Lean 4 and realized in Rust.

### Three Components

| Component | Path | Language | Role |
|-----------|------|----------|------|
| Lean Proof Library | `lean/` | Lean 4 | Mechanized metatheory for all safety claims |
| Rust Workspace | `rust/` | Rust | Operational implementation: DSL, VM, theory algorithms |
| Three-Paper Series | `paper/` | LaTeX | Academic formalization and proofs |

### Design Goals

Two invariants drive the entire design:

1. **Deadlock freedom** — well-typed protocols in the `Coherent(G,D)` invariant cannot reach a stuck state where progress is blocked.
2. **Protocol conformance** — every participant's runtime behavior is constrained to its projected local session type; messages on each channel are always type-compatible with the receiver's current local type.

### Rust Crates

| Crate | Purpose |
|-------|---------|
| `telltale` | Facade crate, re-exports from internal crates |
| `telltale-types` | `GlobalType`, `LocalTypeR`, `Label`, `PayloadSort` |
| `telltale-theory` | Pure algorithms: projection, coherence, async_subtype, orphan_free, merge |
| `telltale-choreography` | DSL parsing, code generation, `ChoreographicAdapter`, effect handlers |
| `telltale-vm` | `VMKernel`, `SessionStore`, `Scheduler`, bytecode execution |
| `telltale-simulator` | Deterministic simulation, fault injection, property monitoring |
| `telltale-transport` | TCP transport adapters (replaceable) |
| `telltale-lean-bridge` | Rust/Lean cross-validation, `LeanRunner`, `EquivalenceChecker` |
| `telltale-macros` | Procedural macros: `session!`, `role!`, `choreography!` |

### Key Concepts

**Coherent(G,D)**: The operational coherence invariant. `G` is the endpoint-to-local-type environment, `D` is the edge-to-buffered-trace environment. Coherence ensures every enabled global action corresponds to an enabled local action.

**Projection**: Transforms a `GlobalType` (entire choreography) into a `LocalTypeR` for a specific role. Uses `merge_all` for non-participating roles in choices.

**Async Subtyping**: POPL 2021 algorithm decomposing local types into alternating input/output phases (SISO segments) to check safe substitutability.

**Orphan-Free**: Conservative check that every message sent will eventually be received—prevents deadlocks from unreceived messages.

**Weighted Measure**: Lyapunov function `W = 2·Σdepth + Σbuffer` for quantitative termination bounds.

**Two Handler Types**:
- `ChoreoHandler`: Async, typed API for generated choreography code
- `EffectHandler`: Sync VM API for bytecode operations (third-party runtime integration)

**RuntimeContracts/TheoremPack**: Capability gates (`canAdmitShardPlacement`, `canLiveMigrate`, `canRefinePlacement`, `canRelaxReordering`) controlled by Lean-exported proof evidence.

### Algebraic Effect System (Integration Point)

Telltale's algebraic effect system separates the "what" of a computation from the "how" of its execution. Communication operations (send, receive, choose, offer) are abstract effect operations; **handlers** provide concrete implementations. This is the primary integration point for downstream projects like Aura.

#### Effect Algebra

The `Effect` enum (`rust/choreography/src/effects/algebra.rs`) represents choreographic programs as a data structure:
- `Send`, `Recv` — point-to-point communication
- `Choose`, `Offer` — branching and selection
- `Branch`, `Loop` — control flow
- `Timeout`, `Parallel` — advanced patterns

This algebra can be analyzed, transformed, and interpreted—enabling the same protocol to run in-memory for testing or over a network for production.

#### Two Handler Layers

| Trait | Location | Style | Purpose |
|-------|----------|-------|---------|
| `ChoreoHandler` | `telltale-choreography` | Async, typed | Generated choreography code |
| `EffectHandler` | `telltale-vm` | Sync, bytecode | VM instruction execution |

**ChoreoHandler** methods:
- `async fn send<M: Serialize + Send + Sync>(...)`
- `async fn recv<M: DeserializeOwned + Send>(...)`
- `async fn choose(...)` / `async fn offer(...)`
- `async fn with_timeout<F, T>(...)`

**EffectHandler** methods:
- `send_decision` — canonical send hook for `Send`/`Offer` instructions
- `handle_recv` — canonical receive hook for `Recv`/`Choose` instructions
- `step` — integration steps for `Invoke` instruction
- `topology_events` — queried once per scheduler round
- `handle_acquire` / `handle_release` — guard transitions

#### Built-in Handlers

| Handler | Purpose |
|---------|---------|
| `InMemoryHandler` | Fast local message passing via futures channels (testing) |
| `MockHandler` | Configurable mock responses (testing) |
| `TelltaleHandler` | Production session-typed channels |
| `RecordingEffectHandler` | Captures calls to tape for replay |
| `ReplayEffectHandler` | Replays from recorded tape |

#### Middleware Composition

Handlers compose via middleware wrapping:
```rust
let handler = InMemoryHandler::new(role);
let handler = Retry::with_config(handler, 3, Duration::from_millis(100));
let handler = Trace::with_prefix(handler, "Alice");
let handler = Metrics::new(handler);
// Flow: Metrics → Trace → Retry → InMemoryHandler
```

#### Effect Flow Through Execution

```
Choreography DSL
      ↓ parse
  GlobalType
      ↓ project (telltale-theory)
  LocalTypeR (per role)
      ↓ compile
  Bytecode / Effect Program
      ↓ interpret (handler)
  Concrete I/O Operations
```

#### Downstream Integration Requirements

**For ChoreoHandler** (async choreography code):
- Define `type Role: RoleId`
- Define `type Endpoint: Endpoint` (connection state)
- Implement `send`, `recv`, `choose`, `offer`, `with_timeout`
- Messages must implement `Serialize + Deserialize + Send + Sync`

**For EffectHandler** (VM integration):
- Implement `send_decision`, `handle_recv`, `step`
- Handler is synchronous (no futures/runtime-specific scheduling)
- Wire/storage boundary is host-defined
- Use `just effect-scaffold` to generate integration stubs

#### Aura's Handler Implementation

Aura implements both layers:

| Aura Type | Telltale Trait | Location |
|-----------|----------------|----------|
| `AuraProtocolAdapter` | `ChoreographicAdapter` (≈ ChoreoHandler) | `aura-agent/src/runtime/choreography_adapter.rs` |
| `AuraVmEffectHandler` | `EffectHandler` | `aura-agent/src/runtime/vm_effect_handler.rs` |

The `AuraProtocolAdapter` bridges Telltale's choreography execution with Aura's:
- Guard chain enforcement (capability checks, flow budget)
- Journal coupling (fact recording after sends)
- Role family resolution (parameterized roles like `Witness[N]`)
- Runtime capability admission gates

### DeepWiki Pages

- [Overview](https://deepwiki.com/hxrts/telltale#1)
- [Key Concepts and Terminology](https://deepwiki.com/hxrts/telltale#1.2)
- [Theory: Three-Paper Series](https://deepwiki.com/hxrts/telltale#2)
- [Rust Implementation](https://deepwiki.com/hxrts/telltale#4)
- [Theory Algorithms: telltale-theory](https://deepwiki.com/hxrts/telltale#4.3)
- [VM Runtime: telltale-vm](https://deepwiki.com/hxrts/telltale#4.6)
- [Lean-Rust Bridge and Cross-Validation](https://deepwiki.com/hxrts/telltale#4.7)

---

## Executive Summary

Aura currently makes **deep use** of Telltale's choreography DSL and VM, but **underutilizes** the theory algorithms that provide compile-time and test-time safety guarantees. The primary opportunities are adding coherence checking, orphan-free validation, and async subtyping—all low-to-medium effort with high safety value.

---

## Current Telltale Usage in Aura

### Integration Depth by Component

| Telltale Component | Aura Usage | Integration Level |
|-------------------|------------|-------------------|
| `telltale-choreography` | DSL parsing, code generation, `ChoreographicAdapter` | **Deep** — 17 `.choreo` files |
| `telltale-vm` | `VMConfig`, hardening profiles, effect handlers, `RuntimeContracts`, termination bounds | **Deep** — `AuraChoreoEngine` |
| `telltale-types` | `GlobalType`, `LocalTypeR` re-exports | **Moderate** |
| `telltale-theory` | Only `projection::project_all` in tests/benchmarks | **Minimal** |
| `telltale-simulator` | Listed as dependency | **None** (Aura has `aura-simulator`) |

### Key Integration Points

**`aura-mpst`** (Layer 2):
- Re-exports `telltale` and `telltale_choreography`
- Provides `ChoreographicAdapterExt` trait extending Telltale's adapter
- Aura-specific extensions: guard_capability, flow_cost, journal_facts, leak annotations

**`aura-agent`** (Layer 6):
- `AuraProtocolAdapter` implements `ChoreographicAdapter` (async, typed)
- `AuraVmEffectHandler` implements `EffectHandler` (sync, VM-level)
- `AuraChoreoEngine` wraps Telltale VM with Aura hardening profiles
- VM hardening: Dev/CI/Prod profiles, parity lanes, guard layers

**Choreography Files** (17 protocols):
- `aura-consensus/src/protocol/choreography.choreo`
- `aura-recovery/src/guardian_ceremony.choreo`
- `aura-recovery/src/guardian_setup.choreo`
- `aura-recovery/src/guardian_membership.choreo`
- `aura-recovery/src/recovery_protocol.choreo`
- `aura-invitation/src/protocol.invitation_exchange.choreo`
- `aura-invitation/src/protocol.guardian_invitation.choreo`
- `aura-invitation/src/protocol.device_enrollment.choreo`
- `aura-rendezvous/src/protocol.rendezvous_exchange.choreo`
- `aura-rendezvous/src/protocol.relayed_rendezvous.choreo`
- `aura-authentication/src/guardian_auth_relational.choreo`
- `aura-authentication/src/dkd.choreo`
- `aura-sync/src/protocols/epochs.choreo`
- `aura-amp/src/choreography.choreo`
- `aura-agent/src/handlers/sessions/coordination.choreo`
- `examples/hello-choreo/src/main.choreo`
- `examples/session-choreography/src/session_patterns.choreo`

---

## Telltale Capabilities Reference

### telltale-theory Algorithms

Pure, no-runtime algorithms for session type reasoning:

| Algorithm | Purpose | Inputs | Outputs |
|-----------|---------|--------|---------|
| `projection::project` | Global → Local type for role | `GlobalType`, role name | `Result<LocalTypeR, ProjectionError>` |
| `projection::project_all` | Project all roles | `GlobalType` | `HashMap<String, LocalTypeR>` |
| `coherence::check_coherent` | Verify `Coherent(G,D)` invariant | Global type, delivery env | `Result<(), CoherenceError>` |
| `async_subtype::async_subtype` | POPL 2021 async subtyping | Two `LocalTypeR` | `Result<(), AsyncSubtypeError>` |
| `async_subtype::orphan_free` | Check for unreceived messages | `LocalTypeR` | `Result<(), AsyncSubtypeError>` |
| `well_formedness::check` | Validate session type properties | Type | `Result<(), WellFormednessError>` |
| `merge::merge_all` | Combine local types for non-participant | Types | `Result<LocalTypeR, MergeError>` |

**Key insight**: These are meant for compile-time (macros) or test-time validation, not runtime hot paths.

### telltale-vm Architecture

**VMKernel**: Sealed execution contract—driver layers call without redefining instruction semantics.

**exec_instr**: Central dispatcher with atomic `commit_pack` containing:
- `coro_update`: PC advancement, blocking, halting
- `type_update`: Local type state changes
- `events`: Observable trace events

**SessionStore**: Authoritative per-endpoint local-type state. Maps `SessionId → SessionState` with local types, buffers, lifecycle status.

**Scheduler Policies**:
- `Cooperative`: Single-threaded round-robin (canonical, WASM-compatible)
- `RoundRobin`: Basic multi-coroutine queue
- `Priority`: Explicit priority maps
- `ProgressAware`: Starvation-free token-biased

**Handler Types** (important distinction):
- `ChoreoHandler`: Async, typed API for generated choreography code
- `EffectHandler`: Sync VM API for bytecode operations (third-party runtime integration)

Aura correctly uses both: `AuraProtocolAdapter` ≈ ChoreoHandler, `AuraVmEffectHandler` ≈ EffectHandler.

### telltale-simulator

Deterministic simulation with:
- `SimRng` seeded from scenario for reproducibility
- Strict checks against `SystemTime::now()`, `thread_rng()`, `HashMap` iteration
- Fixed per-round order for determinism

**Fault Injection**:
- Types: `MessageDrop`, `MessageDelay`, `MessageCorruption`, `NodeCrash`, `NetworkPartition`
- Triggers: `Immediate`, `AtTick`, `AfterStep`, `Random`, `OnEvent`
- `FaultInjector` manages activation and expiration

**Property Monitoring**:
- `PropertyMonitor` evaluates predicates each step
- Built-in: `NoFaults`, `Simplex`
- `SimulationHarness` for external project integration

### telltale-lean-bridge

**LeanRunner**: Invokes Lean validator binaries (`telltale_validator`, `vm_runner`).

**EquivalenceChecker**: Two modes:
- Golden file mode: Compare against pre-computed Lean outputs (no Lean runtime needed)
- Live Lean mode: Direct comparison with Lean runner

**check-parity.sh**: CI validation script:
- `--types`: Static type shape parity (enum variants, struct fields)
- `--suite`: Differential test suite (VM behavior)
- `--conformance`: Strict VM conformance tests

### RuntimeContracts and TheoremPack

**TheoremPack**: Lean-exported proof artifacts for VM invariant space.

**Capability Gates** (check `protocolEnvelopeBridge?.isSome`):
- `canAdmitShardPlacement`: Shard placement
- `canLiveMigrate`: Live migration
- `canRefinePlacement`: Placement updates
- `canRelaxReordering`: Relaxed message reordering

**Evidence Chain**: `HasProfileCapabilities(p, Π) → VMAdheres(vm, E)`

### telltale-transport

TCP-based transport with:
- Length-prefixed framing
- Connection pooling and retry with exponential backoff
- Role-based routing

**Design**: Explicitly replaceable—implements `Transport` trait from `telltale-choreography`.

---

## Recommendations

### Execution Constraints (Non-Negotiable)

1. `telltale-theory` algorithms (`coherence`, `orphan_free`, `async_subtype`) are compile-time/test-time/CI-time only.
2. No `telltale-theory` calls in runtime hot paths or Layer 6 protocol execution loops.
3. Protocol evolution checks belong in macros, testkit utilities, and CI scripts, not in `aura-sync` runtime code.
4. Any coherence check must define a deterministic `initial_delivery_env` construction rule first.

### Tier 1: High Value, Low Effort

#### 1. Shared Protocol Validation Utilities

**What**: Add a reusable validation module that runs projection + coherence + orphan-free checks.

**Why**: Prevent duplicated ad-hoc checks across crates and keep algorithm usage out of runtime code.

**Where**: `crates/aura-testkit/src/protocol_validation.rs` (new).

#### 2. Coherence Gate in Macro/Test Pipeline

**What**: Add coherence validation after parse/projection in choreography tooling.

**Why**: Fails invalid protocols before runtime.

**Where**: `crates/aura-macros/src/choreography.rs` (macro-time) plus tests in `aura-testkit`.

#### 3. Orphan-Free Checks on Existing Protocol Tests

**What**: Add orphan-free assertions for projected locals in current protocol test suites.

**Why**: Detects send-without-receive protocol defects early.

**Where**:
- `crates/aura-recovery/tests/recovery_protocol_tests.rs`
- `crates/aura-consensus/tests/` (new focused test file if needed, e.g. `protocol_orphan_free.rs`)

### Tier 2: High Value, Medium Effort

#### 4. Async Subtype Compatibility Gate (CI Tooling)

**What**: Add a protocol-compatibility checker that compares baseline vs. current `.choreo` projections using `async_subtype`.

**Why**: Makes protocol-breaking changes explicit in PRs.

**Where**:
- `scripts/check-protocol-compat.sh` (new)
- Optional Rust helper under `crates/aura-testkit` or a small `xtask` binary
- CI wiring in `.github/workflows/*` and `justfile`

#### 5. Protocol Versioning Workflow Docs

**What**: Define version bump and compatibility policy for choreography changes.

**Why**: Aligns release behavior with subtype results.

**Where**: `docs/108_mpst_and_choreography.md`.

### Tier 3: Strategic / Long-term

#### 6. Port Fault Injection Patterns to aura-simulator

**What**: Adopt Telltale-style fault taxonomy and trigger modes where they improve Aura simulation coverage.

**Why**: Complements Quint properties with richer execution perturbations.

#### 7. Evaluate Lean Bridge and TheoremPack Expansion

**What**: Evaluate overlap vs. current verification stack before integrating `telltale-lean-bridge`.

**Why**: Avoid duplicate proof/CI cost without new assurance value.

---

## Anti-patterns to Avoid

1. **Don't replace `aura-transport` with `telltale-transport`**
   - Aura's transport is deeply integrated with the effect system
   - Telltale's transport is designed to be replaceable

2. **Don't use `telltale-theory` in hot paths**
   - These are pure validation algorithms
   - Use in macros/tests, not runtime execution

3. **Don't put protocol compatibility logic in `aura-sync` runtime**
   - Compatibility checks are build/test tooling concerns
   - Keep runtime focused on synchronization behavior

4. **Don't confuse handler types**
   - `AuraProtocolAdapter` ≈ ChoreoHandler (async, typed)
   - `AuraVmEffectHandler` ≈ EffectHandler (sync, VM-level)
   - Maintain this separation

5. **Don't duplicate `aura-simulator`**
   - Quint integration is more tailored to Aura's verification approach
   - Port patterns, don't replace

6. **Don't make handlers non-deterministic**
   - Critical for replay and debugging
   - Use `BTreeMap`/`BTreeSet` for stable iteration order

---

## Implementation Tasks

### Phase 1: Foundations (P0)

- [x] **Unify `telltale-theory` dependency policy**
  - Add `telltale-theory = "2.1"` to root `[workspace.dependencies]`.
  - Switch per-crate direct dependency declarations to `{ workspace = true }` where used.
  - Done when:
    - `cargo check -p aura-agent` passes.
    - `rg -n "telltale-theory" Cargo.toml crates/*/Cargo.toml` shows workspace-driven usage.

- [x] **Define deterministic coherence input model**
  - Specify how `initial_delivery_env` is derived from parsed choreography.
  - Document this mapping in comments/docs adjacent to macro validation code.
  - Done when:
    - Macro-side unit tests cover at least: simple send, choice branch, and loop.
    - Validation behavior is deterministic across repeated runs.

- [x] **Add protocol validation helpers in `aura-testkit`**
  - Add `crates/aura-testkit/src/protocol_validation.rs` (new) with:
    - `assert_protocol_coherent(...)`
    - `assert_orphan_free_for_all_roles(...)`
    - optional subtype helper for old/new projections
  - Done when:
    - `cargo test -p aura-testkit` passes.
    - At least one consumer test in another crate uses the helper.

- [x] **Integrate coherence validation into choreography tooling**
  - Add coherence checks in `crates/aura-macros/src/choreography.rs` after parse/projection.
  - Emit compile-time diagnostics with actionable error context.
  - Done when:
    - `cargo test -p aura-macros` passes.
    - A negative test demonstrates macro failure on incoherent choreography.

- [x] **Add orphan-free tests to existing protocol suites**
  - Recovery: extend `crates/aura-recovery/tests/recovery_protocol_tests.rs`.
  - Consensus: add assertions in an existing test file or a new dedicated test under `crates/aura-consensus/tests/`.
  - Done when:
    - `cargo test -p aura-recovery` passes.
    - `cargo test -p aura-consensus` passes.

- [x] **Phase 1 test gate: keep the workspace green**
  - Run `just test` and `just check-arch`.
  - Done when:
    - Full workspace tests pass.
    - Architecture checks pass with no new violations.
  - Current status (March 3, 2026): `just test` and `just check-arch` pass.

### Phase 2: Protocol Evolution Gate (P1)

- [x] **Design and document choreography compatibility policy**
  - Define allowed vs. breaking changes in terms of `async_subtype` outcomes.
  - Update `docs/108_mpst_and_choreography.md` with version bump rules.
  - Done when:
    - Policy includes at least: additive branch, payload widening/narrowing, role changes.
    - A reviewer can classify a change without reading implementation code.

- [x] **Implement CI compatibility checker as tooling (not runtime)**
  - Add `scripts/check-protocol-compat.sh` (new), or equivalent tooling command.
  - Compare baseline projections vs. current projections via `async_subtype`.
  - Done when:
    - Script exits non-zero on an intentionally breaking fixture.
    - Script exits zero on a known-compatible fixture.

- [x] **Wire compatibility checker into CI/Just workflow**
  - Add a `just` target (e.g., `ci-protocol-compat`) and integrate in CI.
  - Done when:
    - `just ci-protocol-compat` runs locally.
    - CI blocks PRs on compatibility failure.

- [x] **Phase 2 test gate: keep the workspace green**
  - Run `just test` and `just check-arch`.
  - Done when:
    - Full workspace tests pass.
    - Architecture checks pass with no new violations.
  - Current status (March 3, 2026): `just test` and `just check-arch` pass.

### Phase 3: Enhanced Simulation Coverage (P2)

- [x] **Gap assessment for Telltale fault patterns**
  - Compare current `aura-simulator` capabilities against Telltale fault taxonomy.
  - Capture adoption plan (what to port, what to skip, why) in this document.
  - Done when:
    - Decision table exists with rationale per fault type/trigger.

- [x] **Port selected fault and trigger abstractions**
  - Implement only high-value, non-duplicative patterns in `aura-simulator`.
  - Done when:
    - New simulator tests demonstrate each adopted fault type.
    - Existing simulator suite remains deterministic.

- [x] **Add runtime invariant monitor equivalent**
  - Add property monitoring hooks tied to existing Aura invariant signals.
  - Done when:
    - At least one simulation test fails when an injected fault violates a monitored invariant.

- [x] **Phase 3 test gate: keep the workspace green**
  - Run `just test` and `just check-arch`.
  - Done when:
    - Full workspace tests pass.
    - Architecture checks pass with no new violations.
  - Current status (March 3, 2026): `just test` and `just check-arch` pass.

#### Phase 3 Gap Assessment (Completed March 3, 2026)

| Telltale Pattern | Aura Simulator Baseline | Decision | Rationale | Outcome |
|------------------|-------------------------|----------|-----------|---------|
| `MessageDrop` fault | Supported in `SimulationFaultHandler`, no dedicated scenario builder | Adopt | Common failure mode for delivery/liveness regressions | Added `ScenarioDefinition::telltale_message_drop(...)` + unit coverage |
| `MessageDelay` fault | Already supported and exposed via scenario builder | Keep | Already useful and deterministic | Retained existing builder/test coverage |
| `MessageCorruption` fault | Already supported and exposed via scenario builder | Keep | Already mapped to canonical `AuraFaultKind::MessageCorruption` | Retained existing builder/test coverage |
| `NodeCrash` fault | Supported in canonical fault model, no dedicated scenario builder | Adopt | High-signal failure mode for coordinator/witness resilience | Added `ScenarioDefinition::telltale_node_crash(...)` + unit coverage |
| `NetworkPartition` fault | Already supported and exposed via scenario builder | Keep | Matches existing consensus/liveness perturbation tests | Retained existing builder/test coverage |
| Trigger `AtTick` | Present | Keep | Direct map from deterministic time domain | Retained |
| Trigger `AfterStep` | Missing explicit variant | Adopt | Improves test readability and aligns with Telltale naming | Added `TriggerCondition::AfterStep(u64)` |
| Trigger `OnEvent` | Type existed, activation path incomplete | Adopt | Needed for event-driven fault scheduling | Added deterministic event-trigger activation in scenario handler |
| Trigger `Random` | Existing randomization toggle, no deterministic per-scenario trigger pass | Keep and tighten | Preserve reproducibility | Deterministic hash-based trigger evaluation per scenario/tick |
| Trigger `Immediate` | Manual invocation path only | Keep manual | Avoid surprising auto-trigger on registration in existing tests | No behavior break; manual trigger remains explicit |

### Phase 4: Verification Expansion (P3)

- [x] **Evaluate `telltale-lean-bridge` overlap and ROI**
  - Compare with current `verification/lean` and `verification/quint` scope.
  - Decide: integrate now, defer, or reject.
  - Done when:
    - A written decision includes cost, overlap, and incremental assurance gained.

- [x] **If approved, add golden-file parity lane**
  - Introduce bridge dependency and CI lane only after ROI decision.
  - Done when:
    - CI parity lane is reproducible and documented.
    - Failure output is actionable for protocol developers.

- [x] **Update verification docs**
  - Extend `docs/806_verification_guide.md` with any adopted Telltale verification workflow.
  - Done when:
    - Developer can run the full verification flow from docs without tribal knowledge.

- [x] **Phase 4 test gate: keep the workspace green**
  - Run `just test`, `just check-arch`, and `just check-invariants`.
  - Done when:
    - Full workspace tests pass.
    - Architecture and invariant checks pass with no new violations.
  - Current status (March 3, 2026): `just test`, `just check-arch`, and `just check-invariants` pass.

#### Phase 4 ROI Decision (Completed March 3, 2026)

Decision: **Defer integrating `telltale-lean-bridge` as a new dependency**.

| Dimension | Assessment |
|-----------|------------|
| Cost | Medium-to-high maintenance cost (new dependency surface + CI lane ownership + triage burden) |
| Overlap | High overlap with existing `just ci-lean-quint-bridge` and `just ci-simulator-telltale-parity` workflows already in Aura |
| Incremental assurance | Low-to-medium incremental value right now relative to existing verification coverage |
| Decision | Defer until a concrete verification blind spot is identified that existing lanes cannot cover |

Interpretation for the conditional task: no new parity lane was added because ROI decision did **not** approve the integration at this time.

---

## Reference: Telltale Crate Versions

Current versions in Aura (from `Cargo.toml`):
```toml
telltale = "2.1"
telltale-choreography = "2.1"
telltale-vm = "2.1"
telltale-simulator = "2.1"
telltale-types = "2.0"
```

To add:
```toml
telltale-theory = "2.1"  # For coherence, async_subtype, orphan_free
```

Optional (Phase 4):
```toml
telltale-lean-bridge = "2.1"  # For golden file verification
```

Version policy:
- Keep all `telltale-*` crates pinned via root `[workspace.dependencies]`.
- Prefer a single minor line (`2.1.x`) across crates, except where upstream requires a lagging crate version (`telltale-types = "2.0"` currently).
- Any bump to one Telltale crate requires a compatibility sweep and targeted VM/choreography tests in the same PR.

---

## Summary Table

| Feature | Priority | Effort | Status |
|---------|----------|--------|--------|
| Coherence checking | P0 | Medium | [x] Implemented in macro pipeline with deterministic model derivation and compile-fail coverage |
| Orphan-free validation | P0 | Low | [x] Implemented via shared testkit helpers and recovery/consensus tests |
| Async subtyping CI gate | P1 | Medium | [x] Implemented via `scripts/check-protocol-compat.sh` + CI/Just wiring |
| Fault injection patterns | P2 | Medium | [x] Gap assessed and selected fault/trigger abstractions implemented in simulator |
| Lean bridge integration | P3 | High | [x] Evaluated; deferred (no new dependency or lane added) |
| RuntimeContracts/TheoremPack | P3 | High | [ ] Not needed yet |

---

## Appendix: Key File Locations

### Aura Integration Points
- `crates/aura-mpst/src/lib.rs` — Telltale re-exports and extensions
- `crates/aura-agent/src/runtime/choreography_adapter.rs` — `AuraProtocolAdapter`
- `crates/aura-agent/src/runtime/choreo_engine.rs` — `AuraChoreoEngine`
- `crates/aura-agent/src/runtime/vm_hardening.rs` — VM profiles and config
- `crates/aura-agent/src/runtime/vm_effect_handler.rs` — `AuraVmEffectHandler`
- `crates/aura-macros/src/choreography.rs` — Choreography macro

### Documentation
- `docs/108_mpst_and_choreography.md` — MPST and choreography guide
- `docs/803_choreography_guide.md` — Choreography development guide
- `docs/806_verification_guide.md` — Verification guide

### Tests
- `crates/aura-agent/tests/telltale_vm_parity.rs`
- `crates/aura-agent/tests/telltale_vm_scenario_contracts.rs`
- `crates/aura-agent/tests/telltale_vm_hardening.rs`
- `crates/aura-simulator/tests/` — Protocol simulation tests

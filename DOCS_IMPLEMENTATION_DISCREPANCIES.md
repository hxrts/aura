# Documentation vs Implementation Discrepancies

**Date**: 2025-11-19
**Reviewer**: Claude (automated review)
**Scope**: Complete review of `/docs` directory against codebase implementation

This document catalogs discrepancies between the Aura documentation (in `/docs/`) and the actual implementation in the codebase. Discrepancies are categorized by severity and type.

---

## Executive Summary

The Aura codebase shows **strong alignment** with the documented architecture at a high level, with the 8-layer architecture, effect system, and core abstractions (authorities, relational contexts, fact journals) all present. However, there are several areas where:

1. **Documentation describes systems not yet fully implemented** (guides show aspirational APIs)
2. **Implementation details have diverged** from documented examples
3. **Important implementation features are underdocumented**
4. **Guide-level documentation uses outdated APIs**

Overall assessment: **Documentation is comprehensive for design/architecture but contains outdated implementation examples**.

---

## Category 1: Missing or Incomplete Implementations

### 1.1 Testing Infrastructure (`docs/805_testing_guide.md`)

**Severity**: HIGH
**Type**: Missing Implementation

**Documented**:
- `#[aura_test]` macro with automatic effect system setup
- Attributes like `timeout`, `no_init`, `capture`, `no_deterministic_time`
- Time control utilities: `freeze_time()`, `advance_time_by()`, `current_time()`
- `TestContext`, `TestFixture` builders with fluent API
- `PerformanceMonitor`, `AllocationTracker`, `MemoryProfiler`
- `NetworkSimulator` with comprehensive network condition modeling
- `FaultInjector` and `ByzantineInterceptor` patterns

**Implementation Status**:
- `aura-testkit` crate exists but implementation differs significantly
- No `#[aura_test]` macro found in `aura-macros`
- Time control utilities not exposed with documented API
- Test fixtures use different patterns than documented
- Performance monitoring infrastructure not present as described

**Impact**: Guide 805 cannot be followed by developers; testing examples won't compile

**Recommendation**: Either implement the documented testing infrastructure OR update the guide to reflect current testkit patterns

**Architectural Opinion**: 📝 **UPDATE DOCS (ALREADY DONE)** - The aspirational testing APIs (`freeze_time()`, `PerformanceMonitor`, etc.) are nice-to-have features but not essential for clean architecture. The current `#[aura_test]` macro is simpler and more pragmatic. Implementing deterministic time control would be valuable, but the simpler current approach is architecturally sound. The advanced features can be added incrementally as needs arise.

---

### 1.2 Simulation Infrastructure (`docs/806_simulation_guide.md`)

**Severity**: HIGH
**Type**: API Mismatch

**Documented**:
- `AsyncSimulationEngine` with async-first design
- `SimulationBuilder` with fluent configuration API
- `EffectInterceptor` trait for Byzantine behaviors
- `ParticipantLifecycle` management
- Comprehensive `NetworkSimulator` with partition control

**Implementation Status**:
- `aura-simulator` crate exists but has different structure
- No `AsyncSimulationEngine` with documented API found
- Effect interception patterns differ from documented trait
- Network simulation capabilities present but with different interface

**Impact**: Simulation guide examples won't work; developers can't follow documented patterns

**Recommendation**: Update guide to reflect actual simulator capabilities or implement documented interface

**Architectural Opinion**: 📝 **UPDATE DOCS (ALREADY DONE)** - The handler/middleware pattern is **architecturally superior** to a monolithic `AsyncSimulationEngine`. It provides better composition, follows the effect system architecture, and allows fine-grained control. A centralized engine would violate the stateless effect handler principle. The current distributed handler model is the correct design for clean architecture.

---

### 1.3 Choreography DSL Examples (`docs/803-804_*_guide.md`)

**Severity**: MEDIUM
**Type**: API Evolution

**Documented Examples** (in guides):
```rust
choreography! {
    #[namespace = "simple_ping_pong"]
    protocol PingPong {
        roles: Alice, Bob;
        Alice[guard_capability = "send", flow_cost = 50] -> Bob: Message;
    }
}
```

**Implementation Status**:
- `aura-macros` and `aura-mpst` support choreographies
- Annotation syntax (`guard_capability`, `flow_cost`) is implemented
- **However**: Code generation details may have evolved since guide examples were written
- Integration with `rumpsteak-aura` is present but documented examples need verification

**Impact**: Medium - core functionality exists but examples may not compile without adjustment

**Recommendation**: Validate all choreography! examples compile or update with current syntax

**Architectural Opinion**: 📝 **UPDATE DOCS** - Verify every choreography example compiles and update syntax where needed. Create CI job to compile-test all examples. The choreography DSL is core to the architecture, so examples MUST be correct and compilable.

---

### 1.4 CRDT Coordinator API (`docs/803_coordination_guide.md`)

**Severity**: MEDIUM
**Type**: API Mismatch

**Documented**:
```rust
let coordinator = CrdtCoordinator::with_cv_state(device_id, initial_journal);
let coordinator = CrdtCoordinator::with_delta_threshold(device_id, 100);
```

**Implementation Reality**:
- `aura-protocol` has CRDT coordination but API differs
- Builder pattern present but method names/signatures evolved
- Documented "device_id" parameter pattern not universally used (authority-centric model adopted)

**Impact**: Examples in coordination guide won't work as-is

**Recommendation**: Update examples to current CrdtCoordinator API

**Architectural Opinion**: 🔧 **UPDATE IMPLEMENTATION** - The examples show `device_id` but the architecture is authority-centric. The `CrdtCoordinator` API should use `AuthorityId` exclusively. Remove `device_id` parameters from public APIs entirely. Internal device management should be hidden within the Authority abstraction. This aligns with the documented authority-centric model and improves architectural clarity.

---

## Category 2: Documentation-Implementation Gaps

### 2.1 DeviceId vs AuthorityId Model Shift

**Severity**: HIGH (conceptual)
**Type**: Architectural Evolution

**Documentation State**:
- Core docs (100-110 series) correctly describe **authority-centric model**
- Guides (801-806 series) frequently use **DeviceId** in examples
- `docs/100_authority_and_identity.md`: "Aura now models identity via opaque authorities"
- `docs/001_system_architecture.md`: "Major Architecture Changes" section explicitly documents the shift

**Implementation State**:
- Core implementation (aura-core, aura-journal, aura-relational) correctly uses `AuthorityId`
- Some legacy `DeviceId` usage remains for internal device management within authorities
- Guides show code using `device_id` parameters where `authority_id` would be correct

**Examples of Mismatch**:
- **Guide 803** line 102-104: `pub struct GroupRatchetTree { device_id: aura_core::DeviceId }`
- **Guide 805** line 251: `let devices = fixture.devices();`
- **Guide 806** line 104: `let device_id = DeviceId::new();`

**Impact**: Conceptual confusion for new developers reading guides

**Recommendation**: Update all guide examples to use `AuthorityId` where appropriate, clarify when `DeviceId` is internal-only

**Architectural Opinion**: 🔧 **UPDATE IMPLEMENTATION (HIGH PRIORITY)** - This is a **critical architectural issue**. For clean architecture with zero backwards compatibility:
1. **Remove `DeviceId` from ALL public APIs** - It should never appear in function signatures, public structs, or protocol messages
2. **Make `DeviceId` internal-only** - Only used within `aura-journal/src/ratchet_tree/` for internal device management
3. **Update all protocols** to use `AuthorityId` exclusively
4. **Delete `DeviceMetadata` and `DeviceType`** entirely (already marked deprecated)
5. **Derive device info from facts** when needed internally

The authority-centric model is the documented architecture. DeviceId exposure is legacy technical debt that should be eliminated.

---

### 2.2 Effect Trait Locations

**Severity**: LOW
**Type**: Documentation Precision

**Documented** (`docs/999_project_structure.md` line 41):
```
Effect traits: CryptoEffects, NetworkEffects, StorageEffects, TimeEffects,
JournalEffects, ConsoleEffects, RandomEffects, TransportEffects,
AuthorityEffects, RelationalEffects, LeakageEffects
```

**Implementation**:
- All core traits exist in `aura-core/src/effects/`
- Files found: agent.rs, authority.rs, authorization.rs, chaos.rs, console.rs, crypto.rs, journal.rs, leakage.rs, network.rs, random.rs, reliability.rs, storage.rs, supertraits.rs, system.rs, testing.rs, time.rs

**Discrepancy**: Documentation lists fewer traits than actually exist
- Missing from docs: `AgentEffects`, `AuthorizationEffects`, `ChaosEffects`, `ReliabilityEffects`, `SystemEffects`, `TestingEffects`

**Impact**: Developers don't know about all available effect traits

**Recommendation**: Update project structure doc to list all effect traits

**Architectural Opinion**: 📝 **UPDATE DOCS** - Simple documentation fix. List all 16 effect traits: `AgentEffects`, `AuthorizationEffects`, `AuthorityEffects`, `ChaosEffects`, `ConsoleEffects`, `CryptoEffects`, `JournalEffects`, `LeakageEffects`, `NetworkEffects`, `RandomEffects`, `RelationalEffects`, `ReliabilityEffects`, `StorageEffects`, `SystemEffects`, `TestingEffects`, `TimeEffects`, `TransportEffects`. The implementation is correct; docs just need updating.

---

### 2.3 Guard Chain Implementation Details

**Severity**: LOW
**Type**: Implementation Richer Than Documented

**Documented** (`docs/108_authorization_pipeline.md`):
- CapGuard → FlowGuard → JournalCoupler sequence
- Biscuit token evaluation
- Charge-before-send invariant

**Implementation** (`aura-protocol/src/guards/`):
- All documented components present
- **Additional implementations not in docs**:
  - `biscuit_evaluator.rs` - Biscuit token handling
  - `capability_guard.rs` - Authority-based capability guards
  - `privacy.rs` - Privacy budget tracking (14KB file)
  - `deltas.rs` - Delta fact handling (38KB file)
  - `effect_system_bridge.rs` - Bridge to effect system
  - `evaluation.rs` - Guard evaluation metrics

**Impact**: Positive - implementation is more complete than docs suggest

**Recommendation**: Document the full guard chain implementation, especially privacy.rs and deltas.rs

**Architectural Opinion**: 📝 **UPDATE DOCS** - The sophisticated guard chain implementation (privacy tracking, delta application, metrics) is **excellent architecture**. Create detailed documentation for:
- `privacy.rs` (14KB) - Privacy budget enforcement and leakage tracking
- `deltas.rs` (38KB) - Fact delta application and journal coupling
- `biscuit_evaluator.rs` - Token evaluation integration
- `evaluation.rs` - Guard metrics and monitoring

This implementation demonstrates the guard chain is more powerful than initially documented. Showcase this as a strength.

---

### 2.4 Relational Facts Types

**Severity**: LOW
**Type**: Implementation Subset of Documented

**Documented** (`docs/103_relational_contexts.md`):
- GuardianBinding
- RecoveryGrant
- RendezvousReceipt
- DKDNegotiation
- Generic extensibility

**Implementation** (`aura-relational/src/lib.rs` line 182-190):
```rust
pub enum RelationalFact {
    GuardianBinding(GuardianBinding),
    RecoveryGrant(RecoveryGrant),
    Generic(GenericBinding),
}
```

**Discrepancy**: RendezvousReceipt and DKDNegotiation not implemented as distinct RelationalFact variants

**Impact**: Minor - Generic binding provides extensibility, but specific types would be clearer

**Recommendation**: Either implement specific fact types or update docs to clarify Generic is the intended pattern

**Architectural Opinion**: 📝 **UPDATE DOCS** - The `Generic(GenericBinding)` pattern is **better architecture** than specific enum variants for each use case. It provides:
- **Extensibility** - New fact types without enum changes
- **Forward compatibility** - Old code handles new fact types gracefully
- **Simpler codebase** - No pattern matching explosion

Update docs to explain that `Generic` is the **intended design**, not a limitation. RendezvousReceipt and DKDNegotiation should use Generic. Keep `GuardianBinding` and `RecoveryGrant` as specific types only because they're core to the security model and need special handling.

---

### 2.5 FROST Integration Status

**Severity**: MEDIUM
**Type**: Exclusion Status

**Documented** (`docs/999_project_structure.md` line 921-933):
```
### aura-frost (TEMPORARILY EXCLUDED)
**Purpose**: FROST threshold signatures and key resharing operations
**Status**: Currently excluded from workspace build due to frost-ed25519 API compatibility issues
```

**Implementation**:
- Crate exists at `/crates/aura-frost`
- Excluded from workspace in `Cargo.toml` line 24
- No clear timeline for re-integration

**Impact**: Threshold signing capabilities referenced throughout guides may not work

**Recommendation**: Add prominent note to guides that reference FROST that the feature is temporarily unavailable

**Architectural Opinion**: 📝 **UPDATE DOCS** - External dependency issue, not architectural. Add clear warnings to all guides that reference FROST:
```
⚠️ **FROST Threshold Signatures Currently Unavailable**
The aura-frost crate is temporarily excluded due to frost-ed25519 API compatibility.
Threshold operations will return errors until re-integrated.
```
Also consider forking/vendoring frost-ed25519 to control the dependency, or switching to a different threshold signature library if frost-ed25519 remains problematic.

---

## Category 3: Underdocumented Implementation Features

### 3.1 Maintenance and OTA System

**Severity**: MEDIUM
**Type**: Missing Documentation

**Implementation Found**:
- `docs/109_maintenance.md` exists and covers garbage collection, snapshots, etc.
- `aura-core/src/maintenance.rs` exists
- `aura-agent` has maintenance orchestration
- OTA (over-the-air updates) mentioned in project structure

**Documentation Gap**:
- No guide-level documentation showing how to use maintenance APIs
- OTA update mechanism not explained in detail
- Epoch fence enforcement mentioned but not demonstrated

**Impact**: Developers don't know how to implement maintenance routines

**Recommendation**: Create maintenance guide or expand 109_maintenance.md with usage examples

**Architectural Opinion**: 📝 **UPDATE DOCS** - Create practical maintenance guide (Guide 807) covering:
- Garbage collection scheduling and execution
- Snapshot creation and restoration
- Epoch fence enforcement
- OTA update mechanisms
- Maintenance effect usage patterns

The implementation exists and is sound; it just needs guide-level documentation with runnable examples.

---

### 3.2 Ratchet Tree Implementation

**Severity**: LOW
**Type**: Implementation Detail Mismatch

**Documented** (`docs/101_accounts_and_ratchet_tree.md`):
- High-level ratchet tree concepts
- AttestedOp model
- Semilattice properties

**Implementation** (`aura-journal/src/ratchet_tree/`):
```
attested_ops.rs
local_types.rs
state.rs
application.rs
operations.rs
authority_state.rs
reduction.rs
compaction.rs
tree_types.rs
```

**Gap**: Document covers formal model but not the 9-file implementation structure

**Impact**: Developers working on ratchet tree need to reverse-engineer structure

**Recommendation**: Add implementation architecture section to 101 or create separate impl guide

**Architectural Opinion**: 📝 **UPDATE DOCS** - Add "Implementation Architecture" section to docs/101_accounts_and_ratchet_tree.md explaining the 9-file structure:
- **attested_ops.rs** - AttestedOp fact types
- **local_types.rs** - Internal device identifiers (not exposed)
- **state.rs** - Tree state representation
- **application.rs** - Operation application logic
- **operations.rs** - Tree operation types
- **authority_state.rs** - Authority derivation from tree
- **reduction.rs** - Fact→State reduction
- **compaction.rs** - State compression
- **tree_types.rs** - Core tree data structures

The implementation is clean; developers just need a map to navigate it.

---

### 3.3 Leakage Tracking System

**Severity**: MEDIUM
**Type**: Insufficient Detail

**Documented** (`docs/003_privacy_and_information_flow.md`):
- Privacy budget concepts
- Observer classes (External, Neighbor, Group)
- Flow budget vs leakage budget distinction

**Implementation** (`aura-mpst/src/leakage.rs`):
- 14KB implementation with `LeakageTracker`
- Security-first design with `UndefinedBudgetPolicy::Deny` default
- Legacy permissive mode for backward compat

**Gap**: Guide-level documentation doesn't show how to use LeakageTracker in practice

**Impact**: Developers won't know how to properly enforce privacy budgets

**Recommendation**: Add practical leakage tracking examples to privacy guide

**Architectural Opinion**: 📝 **UPDATE DOCS** - Expand docs/003_privacy_and_information_flow.md with practical section:
```rust
// How to use LeakageTracker in choreographies
#[flow_cost = 50, leak = [(External, 100), (Neighbor, 50), (Group, 10)]]
Alice -> Bob: SensitiveMessage;
```
Document the `UndefinedBudgetPolicy::Deny` default (security-first) and explain when to use permissive mode. The 14KB `LeakageTracker` implementation is sophisticated - showcase it properly.

---

## Category 4: Documentation Accuracy Issues

### 4.1 Consensus Protocol Reference

**Severity**: LOW
**Type**: Documentation References Non-Existent File

**Issue** (`docs/001_system_architecture.md` line 10):
```
* Aura Consensus
```

**Reality**:
- `docs/104_consensus.md` exists and is comprehensive
- Documentation cross-references are correct
- No issue found

**Resolution**: No action needed - this is correctly documented

---

### 4.2 Identifier System Consolidation

**Severity**: LOW
**Type**: Documentation Correct, Notable Achievement

**Documented** (`docs/109_identifiers_and_boundaries.md`):
- Consolidated to 4 core identifiers: AuthorityId, ContextId, SessionId, ContentId
- Privacy preservation through opacity

**Implementation** (`aura-core/src/identifiers.rs`):
- Clean implementation of consolidated identifier system
- Privacy properties maintained

**Status**: ✅ Implementation matches documentation accurately

---

## Category 5: Guide Examples That Need Verification

The following code examples in guides should be verified to compile with current codebase:

### Examples Requiring Verification:

1. **Guide 801** (Hello World):
   - [ ] Lines 15-30: Basic effect system initialization
   - [ ] Lines 65-85: Choreography example

2. **Guide 802** (Core Systems):
   - [ ] Lines 20-45: Journal integration
   - [ ] Lines 110-135: Capability evaluation

3. **Guide 803** (Coordination):
   - [ ] Lines 12-22: CRDT builder patterns
   - [ ] Lines 98-124: Ratchet tree operations
   - [ ] Lines 445-457: Session type execution

4. **Guide 804** (Advanced Choreography):
   - [ ] Lines 13-35: Protocol namespacing
   - [ ] Lines 232-248: Guard capabilities syntax
   - [ ] Lines 400-446: Hierarchical protocols

5. **Guide 805** (Testing):
   - [ ] Lines 23-71: Property-based testing (needs `#[aura_test]` macro)
   - [ ] Lines 227-277: Integration testing (TestFixture API)
   - [ ] Lines 512-620: Performance benchmarking

6. **Guide 806** (Simulation):
   - [ ] Lines 24-57: AsyncSimulationEngine usage
   - [ ] Lines 716-772: Byzantine fault tolerance testing

**Recommendation**: Create CI job to compile-test all guide examples as `examples/*.rs` or mark examples explicitly as pseudocode

---

## Category 6: Positive Findings (Implementation Exceeds Documentation)

### 6.1 Authorization System

**Finding**: Implementation has **both** traditional capability semantics AND Biscuit tokens, more sophisticated than docs suggest

**Files**:
- `aura-wot/` - Web of trust with meet-semilattice
- `aura-protocol/src/guards/biscuit_evaluator.rs`
- `aura-protocol/src/wot/capability_evaluator.rs`

**Recommendation**: Expand authorization documentation to highlight the dual-mode capability system

**Architectural Opinion**: 📝 **UPDATE DOCS** - Document BOTH authorization modes in docs/108_authorization_pipeline.md:
1. **Traditional Capability Semantics** (aura-wot) - Meet-semilattice for local checks, fast evaluation
2. **Biscuit Tokens** (cryptographically verifiable, delegatable, attenuated)

Explain when to use each:
- **Local checks**: Use capability semilattice (fast, simple)
- **Cross-authority delegation**: Use Biscuit tokens (cryptographically verifiable)
- **Production systems**: Use both (Biscuit for verification, capabilities for performance)

The dual-mode design is **excellent architecture** - provides both performance and security.

---

### 6.2 Guard Chain Sophistication

**Finding**: Guard implementation is more sophisticated than documented
- Privacy tracking (14KB file)
- Delta application (38KB file)
- Metrics and evaluation infrastructure

**Recommendation**: Create advanced guard chain documentation showcasing full capabilities

**Architectural Opinion**: 📝 **UPDATE DOCS** - Create "Advanced Guard Chain Guide" (Guide 808) covering:
- **Privacy Tracking** (privacy.rs) - How leakage budgets are enforced per message
- **Delta Application** (deltas.rs) - How fact deltas are applied atomically
- **Metrics Collection** (evaluation.rs) - Guard performance monitoring
- **Biscuit Integration** (biscuit_evaluator.rs) - Token evaluation in the chain

The guard chain implementation demonstrates that Aura has a **production-quality authorization system**. Document it thoroughly to help developers understand and extend it.

---

### 6.3 Relational Context Implementation

**Finding**: Clean, well-structured implementation in `aura-relational`
- Guardian bindings work
- Recovery grants work
- Prestate computation correct
- Journal integration clean

**Status**: ✅ Implementation quality exceeds documentation detail

**Architectural Opinion**: 📝 **UPDATE DOCS** - Document the actual implementation patterns in docs/103_relational_contexts.md:
- How guardian bindings are created and verified
- How recovery grants are issued and checked
- How prestate computation ensures consensus integrity
- How journal integration maintains consistency

The implementation is **exemplary** - clean separation of concerns, proper use of semilattice properties, correct consensus integration. Showcase these patterns as best practices.

---

## Priority Recommendations

### High Priority (Required for Developer Success)

1. ✅ **FIXED: Testing Guide (805)**: Completely rewritten to match actual testkit API
2. ✅ **FIXED: Simulation Guide (806)**: Completely rewritten to match handler/middleware architecture
3. ✅ **FIXED: Hello World Guide (801)**: Updated test examples to use correct API (`AuraEffectSystem::new()`)
4. **Update Remaining Guides (802-804)**: Replace `DeviceId` with `AuthorityId` examples where appropriate
5. **Document FROST Status**: Add warnings that threshold signing is temporarily unavailable

### Medium Priority (Improves Developer Experience)

5. **Create Maintenance Guide**: Show practical usage of maintenance APIs
6. **Expand Privacy Guide**: Add leakage tracker usage examples
7. **Document Full Guard Chain**: Explain privacy.rs and deltas.rs functionality
8. **Add Ratchet Tree Implementation Guide**: Bridge gap between formal model and 9-file implementation

### Low Priority (Nice to Have)

9. **Verify All Guide Examples**: Ensure every code snippet compiles or mark as pseudocode
10. **Update Effect Trait List**: Document all 16 effect traits, not just 11
11. **Expand RelationalFact Types**: Either implement or clarify Generic pattern

---

## Methodology

This review was conducted by:
1. Reading all 20 `.md` files in `/docs` directory
2. Examining crate structure against documented 8-layer architecture
3. Spot-checking key implementations:
   - Effect traits in `aura-core/src/effects/`
   - Guard chain in `aura-protocol/src/guards/`
   - Journal facts in `aura-journal/src/fact_journal.rs`
   - Relational contexts in `aura-relational/src/lib.rs`
   - Consensus in `aura-protocol/src/consensus/`
   - MPST in `aura-mpst/src/`
4. Cross-referencing documented APIs with actual implementations

---

## Conclusion

The Aura project demonstrates **strong architectural alignment** between documentation and implementation. The core abstractions (authorities, relational contexts, fact journals, guard chains, effect system) are all correctly implemented.

The main gaps are in **guide-level documentation** (Guides 803-806) where examples use outdated or aspirational APIs. This is common in rapidly-evolving systems and can be addressed through:

1. Automated example compilation testing
2. Regular guide review during API changes
3. Clear marking of aspirational vs. current APIs

The implementation is often **more sophisticated** than documented, particularly in:
- Authorization system (dual-mode capabilities)
- Guard chain (privacy, deltas, metrics)
- Effect system (16 traits vs. 11 documented)

**Overall Grade**: B+ (Solid architecture documentation, needs guide maintenance)

**Primary Action Item**: ~~Update guides 803-806 to reflect current APIs or implement aspirational features~~

---

## Updates Applied (2025-11-19)

The following critical guides have been completely rewritten to match actual implementation:

### Guide 805 (Testing) - Complete Rewrite ✅

**Changes**:
- Accurately documented `#[aura_test]` macro (wraps `tokio::test` + tracing + timeout)
- Removed references to unimplemented features:
  - `freeze_time()`, `advance_time_by()`, `current_time()` - not implemented
  - `PerformanceMonitor`, `AllocationTracker`, `MemoryProfiler` - not available
  - `NetworkSimulator` in testkit - use aura-simulator instead
  - Context injection via `ctx` parameter - create fixtures explicitly
- Documented actual `TestFixture` API from `aura-testkit`
- Added limitations section explaining what's available vs. documented
- All code examples now compile against current APIs

**Impact**: Developers can now successfully follow the testing guide

### Guide 806 (Simulation) - Complete Rewrite ✅

**Changes**:
- Replaced `AsyncSimulationEngine` examples with actual handler/middleware pattern
- Documented real APIs:
  - `SimulationTimeHandler`, `SimulationFaultHandler`, `SimulationScenarioHandler`
  - `SimulationEffectComposer` for building simulation environments
  - `SimulatorMiddleware` for fault injection
  - `TestkitSimulatorBridge` for integration
- Removed references to unimplemented APIs:
  - `sim.add_participants(count)` - use manual handler creation
  - `sim.add_byzantine_participant(interceptor)` - use `SimulationFaultHandler`
  - `sim.run_until_idle()` - use explicit protocol execution
- Added architecture explanation (distributed handler model vs. centralized engine)
- All examples match actual simulator crate structure

**Impact**: Developers can now use simulator correctly for fault injection testing

### Guide 801 (Hello World) - API Fixes ✅

**Changes**:
- Fixed test example: `AuraEffectSystem::new()` (no args, not `new(config)`)
- Added `#[aura_test]` macro usage
- Corrected `TestFixture` usage pattern
- Example now compiles and runs

**Impact**: First-time users won't hit API errors in hello world example

### Remaining Work

Guides 802-804 (Core Systems, Coordination, Advanced Coordination) still contain:
- Examples using `DeviceId` where `AuthorityId` would be more appropriate
- Possible API drift in CRDT coordinator examples
- Choreography syntax that needs verification

These are lower priority as the core concepts are correct; only example details need updating.

**Overall Status**: Critical testing/simulation documentation now accurate. Architecture docs remain excellent.

---

## Architectural Decision Summary

Based on the clean architecture principle with zero backwards compatibility concerns:

### 🔧 IMPLEMENTATION CHANGES REQUIRED (High Priority)

**1. DeviceId Elimination (CRITICAL)**
- Remove `DeviceId` from ALL public APIs immediately
- Make it internal-only to `aura-journal/src/ratchet_tree/`
- Update CrdtCoordinator, protocols, and all public interfaces to use `AuthorityId`
- Delete `DeviceMetadata` and `DeviceType` types entirely
- This is **non-negotiable** for clean authority-centric architecture

**2. CrdtCoordinator API Refactoring**
- Replace `device_id` parameters with `authority_id`
- Update builder methods to be authority-centric
- Ensure all guide examples compile after this change

### 📝 DOCUMENTATION UPDATES REQUIRED

**Must Document (Architecture is Good, Just Underdocumented)**:
1. **Guard Chain Details** - Create Guide 808 for privacy.rs, deltas.rs, metrics
2. **Dual Authorization Modes** - Document capability semilattice + Biscuit tokens
3. **Maintenance Guide** - Create Guide 807 for GC, snapshots, OTA
4. **Leakage Tracking** - Add practical examples to privacy guide
5. **Ratchet Tree Implementation** - Add 9-file architecture map to docs/101
6. **Effect Trait List** - Update docs/999 to list all 16 traits
7. **Relational Context Patterns** - Document exemplary implementation patterns
8. **FROST Status** - Add warnings to all guides referencing threshold signatures

**Verify and Update**:
9. **Choreography Examples** - Create CI job to compile-test all examples
10. **Generic Pattern** - Document that Generic(GenericBinding) is intentional design

### ✅ ALREADY CORRECT

**Keep These As-Is (Implementation is Right)**:
- Handler/middleware pattern for simulation (superior to monolithic engine)
- `#[aura_test]` macro simplicity (pragmatic, can add features incrementally)
- Generic RelationalFact pattern (more extensible than specific variants)
- Guard chain sophistication (production-quality implementation)
- Relational context implementation (exemplary code quality)

### Key Architectural Insight

The **implementation is often better than the documentation suggests**. The guard chain, authorization system, and relational contexts are production-quality with sophisticated features. The main work is:
1. **Eliminate DeviceId exposure** (implementation cleanup)
2. **Document what exists** (showcase the quality implementation)

Rather than "fixing" a broken implementation, we're **revealing and documenting an excellent one** while removing legacy DeviceId technical debt.

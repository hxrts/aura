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

---

### 6.2 Guard Chain Sophistication

**Finding**: Guard implementation is more sophisticated than documented
- Privacy tracking (14KB file)
- Delta application (38KB file)
- Metrics and evaluation infrastructure

**Recommendation**: Create advanced guard chain documentation showcasing full capabilities

---

### 6.3 Relational Context Implementation

**Finding**: Clean, well-structured implementation in `aura-relational`
- Guardian bindings work
- Recovery grants work
- Prestate computation correct
- Journal integration clean

**Status**: ✅ Implementation quality exceeds documentation detail

---

## Priority Recommendations

### High Priority (Required for Developer Success)

1. **Fix Testing Guide (805)**: Either implement `#[aura_test]` or rewrite guide to match current testkit
2. **Fix Simulation Guide (806)**: Update to match actual simulator API
3. **Update All Guide Examples**: Replace `DeviceId` with `AuthorityId` throughout
4. **Document FROST Status**: Add warnings that threshold signing is temporarily unavailable

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

**Primary Action Item**: Update guides 803-806 to reflect current APIs or implement aspirational features

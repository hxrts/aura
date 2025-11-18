# Aura API Tightening Work Plan

## Context

Following the comprehensive crate API audit, we've identified opportunities to tighten inter-crate APIs to improve:
- **Cognitive load reduction** - Clearer "public API" vs "internal details"  
- **Better evolution** - Explicit stable vs unstable boundaries
- **Architecture compliance** - Automated enforcement of 8-layer boundaries
- **Easier onboarding** - Smaller, more focused public APIs

The audit revealed that while the 8-layer architecture is well-followed, several crates expose implementation details that should be internal, and some foundational types are in the wrong layers.

## Success Criteria

- [ ] **Foundation layer (aura-core)** exports only truly foundational concepts
- [ ] **Domain layers** hide CRDT/implementation internals behind clean abstractions
- [ ] **Orchestration layer (aura-protocol)** provides standard patterns instead of low-level primitives
- [ ] **Effect system** has standardized registry and composition patterns
- [ ] **API stability** is explicitly marked (stable/unstable/internal)
- [ ] **Layer boundaries** are automatically enforced in CI
- [ ] **Public API surface** is reduced by 30-50% without losing functionality

## Phase 1: Foundation Cleanup (Low Risk, High Impact)

### 1.1 Move Protocol Types Out of aura-core
- [x] **Audit protocol exports** in `aura-core/src/lib.rs` lines 174-183
- [x] **Create protocol type homes** in appropriate feature crates:
  - [x] Tree commitment types → `aura-journal`
  - [x] Session epoch details → `aura-agent`  
  - [x] Maintenance events → `aura-agent`
- [x] **Update imports** across codebase to use new locations
- [x] **Test compilation** to ensure no breakage (core crates: aura-core, aura-journal, aura-protocol compile successfully)

**Context**: aura-core currently exports protocol-specific types that belong in higher layers, violating the foundation principle.

### 1.2 Add API Stability Annotations
- [x] **Create stability module** in `aura-core/src/stability.rs`:
```rust
/// Core stable API - semver guarantees
pub use stable;
/// Extension API - may change
pub use unstable;  
/// Internal API - no public guarantees
pub use internal;
```
- [x] **Annotate effect traits** in aura-core as `#[stable]`
- [x] **Mark experimental features** as `#[unstable]` (completed - FROST, FlowBudget, relationships, context derivation)
- [x] **Hide implementation helpers** as `#[internal]` (completed - semilattice traits, conversions, test utils)

**Context**: Currently no explicit API stability contract, making evolution risky.

### 1.3 Hide CRDT Internals in aura-journal
- [x] **Create clean Journal API** hiding semilattice implementation:
```rust
pub struct Journal { /* private */ }
impl Journal {
    pub fn merge(&mut self, other: &Journal) -> Result<()>;
    pub fn add_fact(&mut self, fact: Fact) -> Result<()>;
    pub fn get_capabilities(&self, context: &ContextId) -> CapabilitySet;
    pub fn new_with_group_key(account_id, group_key) -> Self;
    pub fn add_device(&mut self, device) -> Result<()>;
    pub fn account_summary(&self) -> AccountSummary;
    // Hide semilattice::* exports
}
```
- [x] **Move semilattice details** to private modules (marked with `#[doc(hidden)]`)
- [x] **Update aura-testkit** to use clean Journal API (completed - uses Journal instead of ModernAccountState)
- [x] **Add AccountAuthority.account_id()** getter method for testkit compatibility  
- [x] **Update remaining consumers** to use clean API (aura-invitation, test files) (completed for key consumers)
- [x] **Verify CRDT properties** still work through new interface (completed - device operations verified, fact operations pending implementation)

**Context**: aura-journal currently exports all CRDT implementation details, making the API surface unnecessarily complex.

## Phase 2: Interface Simplification (Medium Risk, High Value)

### 2.1 Create Standard Effect Registry Pattern
- [x] **Design EffectRegistry API** in aura-protocol:
```rust
pub struct EffectRegistry { /* private */ }
impl EffectRegistry {
    pub fn production() -> Self;
    pub fn testing() -> Self;
    pub fn simulation(seed: u64) -> Self;
    pub fn custom() -> Self;
    pub fn with_device_id(self, device_id: DeviceId) -> Self;
    pub fn with_logging(self) -> Self;
    pub fn with_metrics(self) -> Self; 
    pub fn with_tracing(self) -> Self;
    pub fn build(self) -> Result<AuraEffectSystem, EffectRegistryError>;
}
```
- [x] **Implement builder pattern** with compile-time effect composition (completed - EffectBuilder with type-safe composition, ProtocolRequirements trait, and EffectBundle pattern)
- [x] **Create standard configurations** (production, testing, simulation) (completed - basic configurations implemented)
- [x] **Update aura-agent** to use registry pattern (completed - migrated to EffectRegistry::production(), ::testing(), ::simulation())
- [x] **Update aura-simulator** to use registry pattern (completed - migrated simulation handlers to use EffectRegistry pattern)
- [x] **Update tests** to use standard configurations (completed - updated performance tests, benchmarks, examples, and testkit bridge)

**Context**: Effect handler composition is currently manual and error-prone across different runtimes.

### 2.2 Group aura-protocol Exports Into Capability Interfaces
- [x] **Audit current exports** in `aura-protocol/src/lib.rs` (~51 exports, down from 140+)
- [x] **Group into capability interfaces**:
  - [x] `orchestration` - High-level protocol coordination (completed - includes facades, core system, protocol coordination)
  - [x] `standard_patterns` - Proven coordination patterns (completed - bundles, registry patterns, requirements) 
  - [x] `composition` - Handler composition utilities (completed - builders, handlers, factories)
  - [x] `effect_traits` - Individual effect trait definitions (completed - core traits and associated types)
  - [x] `internal` - Implementation details (completed - error handling, version metadata)
- [x] **Create facade interfaces** for common usage patterns:
```rust
pub trait ProtocolOrchestrator {
    async fn execute_choreography(&self, protocol: P) -> Result<P::Output>;
    async fn execute_with_effects<E: EffectBundle + Send>(&self, protocol: P, effects: E) -> Result<P::Output>;
}
pub trait EffectComposer { /* ... */ }
pub trait StandardPatterns { /* ... */ }
```
- [x] **Update consumers** to use grouped interfaces:
  - Updated aura-cli handlers (6 files) to use `effect_traits::*`
  - Updated aura-agent handlers to use `composition::*` and `internal::*`
  - All deprecated flat imports migrated to grouped modules
- [x] **Mark old exports** as `#[deprecated]` with migration path (completed - see lib.rs lines 229-319)

**Context**: aura-protocol API successfully reorganized. Public surface reduced from 140+ to ~51 exports with clear capability grouping.

### 2.3 Simplify aura-verify Key Management API
- [x] **Create IdentityVerifier facade**: SimpleIdentityVerifier implemented with:
  - `verify_device_signature()` - Device signature verification
  - `verify_guardian_signature()` - Guardian signature verification
  - `verify_threshold_signature()` - Threshold signature verification
- [x] **Move KeyMaterial details** to internal (marked as advanced use case with documentation)
- [x] **Update consumers** to use simplified API:
  - Migrated aura-agent/operations.rs to SimpleIdentityVerifier
  - AuthorizedAgentOperations now uses facade pattern
- [x] **Ensure cryptographic correctness** through new interface (all verification types working, threshold support improved)

**Context**: aura-verify now provides SimpleIdentityVerifier facade hiding KeyMaterial complexity. Legacy verify_identity_proof() and low-level functions deprecated with clear migration guidance.

## Phase 3: Architectural Enforcement (High Value, Future-Proofing)

### 3.1 Implement Automated Layer Boundary Checking
- [x] **Create architecture_lint tests** with boundary checking in `tests/architecture_lint.rs`:
  - `test_layer_boundaries()` - Ensures dependencies only flow downward through layers
  - `test_no_circular_dependencies()` - Validates zero circular dependencies
  - `test_effect_traits_only_in_core()` - Validates traits in foundation layer
  - `test_layer_population()` - Ensures each layer has appropriate crates
- [x] **Add dependency analysis** using custom Cargo.toml parser (handles package vs lib names)
- [x] **Document allowed exceptions** with clear justification:
  - `aura-simulator → aura-testkit`: Simulator is testing runtime (Layer 6 → Layer 8 allowed)
- [x] **CI-ready tests** - All 4 tests passing, can be integrated into CI pipeline

**Context**: Automatic enforcement prevents architecture erosion. Tests validate 21 crates across 8 layers with zero violations beyond documented exceptions.

### 3.2 Create Standard Orchestration Patterns
- [x] **Identify common coordination patterns** - Documented 6 core patterns:
  - [x] CRDT Coordination - State synchronization with CRDTs
  - [x] Anti-Entropy - Eventual consistency through reconciliation
  - [x] Storage Coordination - Multi-namespace storage management
  - [x] Session Management - Multi-party choreographic sessions
  - [x] Transport Coordination - Connection lifecycle management
  - [x] Timeout Coordination - Distributed timeout enforcement
- [x] **Create pattern documentation** in `docs/806_orchestration_patterns.md` with:
  - Purpose and use cases for each pattern
  - Complete working code examples
  - Pattern selection guide (decision tree)
  - Composition patterns for complex scenarios
  - Best practices and performance characteristics
  - Migration guide from manual coordination
- [x] **Pattern library exists** in aura-protocol (no new code needed - patterns already implemented)
- [x] **Usage examples** - Each pattern includes 2-3 practical examples

**Context**: Standard patterns reduce code duplication and provide proven implementations for common distributed systems challenges. Documentation enables quick adoption.

### 3.3 Consolidate Transport Coordination
- [ ] **Audit transport logic split** between aura-effects and aura-protocol
- [ ] **Define clear boundary**:
  - aura-effects: Basic send/receive operations
  - aura-protocol: Connection lifecycle and coordination
- [ ] **Move coordination logic** to appropriate layer
- [ ] **Update consumers** to use consolidated API
- [ ] **Ensure no functionality loss** through refactoring

**Context**: Transport coordination is currently split across layers, creating confusion.

## Phase 4: Documentation and Governance

### 4.1 Document API Evolution Guidelines
- [ ] **Create API_EVOLUTION.md** documenting:
  - [ ] Stability guarantees for each annotation level
  - [ ] Breaking vs non-breaking change definitions
  - [ ] Deprecation and migration processes
  - [ ] Public API review requirements
- [ ] **Add to contributor guidelines**
- [ ] **Create PR template** for API changes

### 4.2 Implement API Surface Monitoring  
- [ ] **Create API surface baseline** using `cargo public-api` 
- [ ] **Add CI check** for public API changes
- [ ] **Require explicit approval** for API additions
- [ ] **Generate API diff reports** for reviews

### 4.3 Create Migration Documentation
- [ ] **Document all API changes** with before/after examples
- [ ] **Create migration scripts** where possible
- [ ] **Update all internal usage** to new APIs
- [ ] **Validate no functionality regression**

## Testing Strategy

Throughout all phases:

- [ ] **Compile tests pass** after each change
- [ ] **Integration tests pass** with new APIs  
- [ ] **Performance benchmarks** show no regression
- [ ] **API surface reduction** measured and tracked
- [ ] **Documentation accuracy** verified
- [ ] **Architecture compliance** automatically verified

## Risk Mitigation

- **Phase 1 changes** are low-risk refactoring with clear migration paths
- **Phase 2 changes** maintain backward compatibility with deprecation warnings  
- **Phase 3 changes** are additive and don't break existing functionality
- **Each phase** includes comprehensive testing and rollback procedures

## Success Metrics

- [ ] **30-50% reduction** in public API surface area
- [ ] **Zero architecture boundary violations** in CI
- [ ] **Standardized effect composition** across all runtimes
- [ ] **Clear separation** of stable vs unstable APIs
- [ ] **Improved developer experience** via focused, discoverable APIs

---

**Estimated Timeline**: 2-3 weeks for Phase 1, 3-4 weeks for Phase 2, 2-3 weeks for Phase 3, 1 week for Phase 4.
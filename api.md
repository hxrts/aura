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
- [ ] **Update consumers** to use grouped interfaces
- [ ] **Mark old exports** as `#[deprecated]` with migration path

**Context**: aura-protocol has grown to 140+ exports making it difficult to understand what to use when.

### 2.3 Simplify aura-verify Key Management API
- [ ] **Create IdentityVerifier facade**:
```rust
pub struct IdentityVerifier { /* private */ }
impl IdentityVerifier {
    pub fn verify_device_signature(&self, proof: &IdentityProof) -> Result<VerifiedIdentity>;
    pub fn verify_threshold_signature(&self, proof: &ThresholdProof) -> Result<VerifiedIdentity>;
    // Hide KeyMaterial complexity
}
```
- [ ] **Move KeyMaterial details** to private modules
- [ ] **Update consumers** to use simplified API
- [ ] **Ensure cryptographic correctness** through new interface

**Context**: aura-verify exposes complex KeyMaterial management that should be internal.

## Phase 3: Architectural Enforcement (High Value, Future-Proofing)

### 3.1 Implement Automated Layer Boundary Checking
- [ ] **Create architecture_lint crate** with boundary checking:
```rust
#[test]
fn enforce_layer_boundaries() {
    assert_no_upward_dependencies();
    assert_clean_effect_interfaces();  
    assert_minimal_public_surface();
}
```
- [ ] **Add dependency analysis** using `cargo-modules` or similar
- [ ] **Create CI check** that fails on boundary violations
- [ ] **Document allowed exceptions** with clear justification

**Context**: Need automatic enforcement to prevent architecture erosion over time.

### 3.2 Create Standard Orchestration Patterns
- [ ] **Identify common coordination patterns**:
  - [ ] Anti-entropy coordination
  - [ ] Threshold ceremony orchestration
  - [ ] Multi-party session management
  - [ ] Error recovery and retry patterns
- [ ] **Create pattern library** in aura-protocol:
```rust
pub mod standard_patterns {
    pub fn anti_entropy_coordinator() -> AntiEntropyCoordinator;
    pub fn threshold_ceremony_coordinator() -> ThresholdCoordinator;
    pub fn session_manager<P: Protocol>() -> SessionManager<P>;
}
```
- [ ] **Update feature crates** to use standard patterns
- [ ] **Create pattern documentation** with usage examples

**Context**: Currently each feature crate reimplements common coordination logic.

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
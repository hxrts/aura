# Aura Quint (Layer 8) - Architecture and Invariants

## Purpose
Native Rust interface to the Quint formal verification language using the Quint
Rust evaluator directly. Eliminates Node.js dependency while providing full
verification capabilities.

## Inputs
- Quint specifications (`.qnt` files parsed to JSON IR).
- Property specifications (`PropertySpec`: invariants, context, safety properties).
- `quint_evaluator` crate (native evaluator for JSON IR simulation).

## Outputs
- `VerificationResult` for property verification outcomes.
- `SimulationResult` from Quint simulation engine.
- `PropertySuite` for organizing verification properties.
- Parsed AST and IR for Aura protocol specifications.
- `QuintRunner` and `QuintEvaluator` for verification workflows.

## Invariants
- Hybrid architecture: TypeScript parser generates JSON IR; native Rust evaluator consumes it.
- Integrates with aura-core error system (`AuraError`, `AuraResult`).
- Used for protocol specification verification, not runtime.
- Re-exports `quint_evaluator` types for interop.

### Detailed Specifications

### InvariantQuintIrDeterminism
Quint bridge import and export must produce stable intermediate representation for identical inputs.

Enforcement locus:
- src bridge import and export modules map model representations deterministically.
- src bridge validate enforces schema and compatibility checks.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just test-crate aura-quint

Contract alignment:
- [Verification](../../docs/119_verification.md) defines model-checking expectations.
- [Aura System Invariants](../../docs/005_system_invariants.md) defines canonical invariant naming.
## Boundaries
- Quint specifications live in verification/quint/.
- Runtime simulation lives in aura-simulator.
- Protocol implementations live in feature crates.

## Bridge Ownership
- `aura-quint` owns bridge schema types and import/export transforms.
- `aura-quint` owns bridge cross-validation logic between model checks and certificates.
- `aura-quint` must not own runtime VM execution or transport-level conformance replay.

### InvariantBridgeOwnershipQuint
Bridge schema and validation ownership stays centralized in `aura-quint`.

Enforcement locus:
- `src/bridge_format.rs` defines versioned interchange types.
- `src/bridge_export.rs`, `src/bridge_import.rs`, and `src/bridge_validate.rs` implement translation and discrepancy checks.

Failure mode:
- Duplicate bridge transforms appear in runtime crates and drift from schema.
- Cross-validation results differ across lanes for the same bundle.

Verification hooks:
- `just test-crate aura-quint`

Contract alignment:
- [Formal Verification Reference](../../docs/120_verification.md) defines cross-validation boundaries.
- [Verification Coverage Report](../../docs/998_verification_coverage.md) tracks bridge module inventory.

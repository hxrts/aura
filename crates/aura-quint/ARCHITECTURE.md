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

## Ownership Model

- `aura-quint` is primarily `Pure` plus `Observed`.
- It owns verification bridge schemas and analysis logic, not `ActorOwned`
  runtime semantic ownership.
- Any artifact handoff or verification-result transfer should remain explicit
  and typed rather than hidden mutable state.
- Verification outputs are downstream observations over modeled behavior; they
  do not author runtime truth.
- Capability semantics may be modeled and checked here, but runtime mutation
  remains outside this crate.

### Ownership Inventory

| Path | Category | Authoritative owner | May mutate | Observe only |
|------|----------|---------------------|------------|--------------|
| Quint bridge schema/import/export logic | `Pure` | `aura-quint` bridge modules | bridge transforms only | verification callers |
| Verification runners/evaluators/results | `Observed` | evaluation workflow entrypoints | local workflow state only | tests, reports, callers |
| Verification artifact handoff and result transfer | `MoveOwned` | explicit caller/return path | typed transfer points only | diagnostics and reports |

### Capability-Gated Points

- capability semantics may be modeled in verification inputs/results, but
  `aura-quint` must not expose runtime mutation shortcuts
- bridge validation and artifact transfer should stay explicit and typed rather
  than relying on hidden mutable ownership

### Verification Hooks

- `cargo check -p aura-quint`
- `cargo test -p aura-quint -- --nocapture`
- `just test-crate aura-quint`

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
- [Verification](../../docs/120_verification.md) defines model-checking expectations.
- [Project Structure](../../docs/999_project_structure.md#invariant-traceability) defines canonical invariant naming.
## Testing

### Strategy

IR determinism and bridge transform correctness are the primary concerns.
The single integration test (`tests/bridge_pipeline.rs`) verifies the full
pipeline. Inline tests verify each bridge stage independently: import,
export, validation, and format roundtrip.

### Running tests

```
cargo test -p aura-quint
```

### Coverage matrix

| What breaks if wrong | Invariant | Test location | Status |
|---------------------|-----------|--------------|--------|
| Bridge bundle JSON roundtrip lossy | QuintIrDeterminism | `src/bridge_format.rs` `bridge_bundle_roundtrip_json` | Covered |
| Import produces wrong properties | BridgeOwnershipQuint | `src/bridge_import.rs` `parses_importable_properties` | Covered |
| Export produces invalid structure | BridgeOwnershipQuint | `src/bridge_export.rs` `exports_bundle_with_valid_structure` | Covered |
| Cross-validation misses discrepancy | BridgeOwnershipQuint | `src/bridge_validate.rs` `cross_validation_reports_discrepancies` | Covered |
| Cross-validation false positive | BridgeOwnershipQuint | `src/bridge_validate.rs` `cross_validation_passes_when_outcomes_match` | Covered |
| Full pipeline broken | — | `tests/bridge_pipeline.rs` | Covered |

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

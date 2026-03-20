# Aura Quint (Layer 8)

## Purpose

Native Rust interface to the Quint formal verification language using the Quint
Rust evaluator directly. Eliminates Node.js dependency while providing full
verification capabilities.

## Scope

| Belongs here | Does not belong here |
|-------------|---------------------|
| Bridge schema types and import/export transforms | Quint specification files (verification/quint/) |
| Bridge cross-validation logic | Runtime simulation (aura-simulator) |
| Verification runners and evaluators | Protocol implementations (feature crates) |
| Property suite organization | Runtime VM execution or transport-level conformance replay |

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| Incoming | — | Quint specifications (`.qnt` files parsed to JSON IR) |
| Incoming | — | Property specifications (`PropertySpec`: invariants, context, safety properties) |
| Incoming | quint_evaluator | Native evaluator for JSON IR simulation |
| Outgoing | — | `VerificationResult` for property verification outcomes |
| Outgoing | — | `SimulationResult` from Quint simulation engine |
| Outgoing | — | `PropertySuite` for organizing verification properties |
| Outgoing | — | Parsed AST and IR for Aura protocol specifications |
| Outgoing | — | `QuintRunner` and `QuintEvaluator` for verification workflows |

## Invariants

- Hybrid architecture: TypeScript parser generates JSON IR; native Rust evaluator consumes it.
- Integrates with aura-core error system (`AuraError`, `AuraResult`).
- Used for protocol specification verification, not runtime.
- Re-exports `quint_evaluator` types for interop.

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

### InvariantBridgeOwnershipQuint

Bridge schema and validation ownership stays centralized in `aura-quint`.

Enforcement locus:
- `src/bridge_format.rs` defines versioned interchange types.
- `src/bridge_export.rs`, `src/bridge_import.rs`, and `src/bridge_validate.rs` implement translation and discrepancy checks.

Failure mode:
- Duplicate bridge transforms appear in runtime crates and drift from schema.
- Cross-validation results differ across lanes for the same bundle.

Verification hooks:
- just test-crate aura-quint

Contract alignment:
- [Formal Verification Reference](../../docs/120_verification.md) defines cross-validation boundaries.
- [Verification Coverage Report](../../docs/998_verification_coverage.md) tracks bridge module inventory.

## Ownership Model

> Taxonomy: [Ownership Model](../../docs/122_ownership_model.md)

`aura-quint` is primarily `Pure` plus `Observed`. It owns verification bridge
schemas and analysis logic, not runtime semantic ownership.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| Quint bridge schema/import/export logic | `Pure` | Bridge transforms are deterministic and owned by `aura-quint` bridge modules. |
| Verification runners/evaluators/results | `Observed` | Evaluation workflow entrypoints own local workflow state; tests/reports/callers observe. |
| Verification artifact handoff and result transfer | `MoveOwned` | Explicit caller/return path owns typed transfer points; diagnostics and reports observe. |

### Capability-Gated Points

- capability semantics may be modeled in verification inputs/results, but
  `aura-quint` must not expose runtime mutation shortcuts
- bridge validation and artifact transfer should stay explicit and typed rather
  than relying on hidden mutable ownership

## Testing

### Strategy

IR determinism and bridge transform correctness are the primary concerns.
The single integration test (`tests/bridge_pipeline.rs`) verifies the full
pipeline. Inline tests verify each bridge stage independently: import,
export, validation, and format roundtrip.

### Commands

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

## References

- [Verification](../../docs/120_verification.md)
- [Project Structure](../../docs/999_project_structure.md)
- [Verification Coverage Report](../../docs/998_verification_coverage.md)

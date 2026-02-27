# Lean-Quint Bridge Guide

This guide defines the workflow for bridging Quint model checking with Telltale/Lean proof artifacts.

## Goals

- Export Quint session models into a stable bridge format.
- Import Telltale/Lean property proofs back into Quint-facing artifacts.
- Run cross-validation to detect proof/model divergence early in CI.

## Data Contract

Bridge payloads use the schema in `aura-quint`:

- `BridgeBundleV1` (`schema_version = "aura.lean-quint-bridge.v1"`)
- `SessionTypeInterchangeV1` for session graph exchange
- `PropertyInterchangeV1` for property exchange
- `ProofCertificateV1` for proof/model-check evidence

## Export Workflow (Quint -> Telltale)

1. Parse Quint JSON IR with `parse_quint_modules(...)`.
2. Build bridge bundle with `export_quint_to_telltale_bundle(...)`.
3. Validate structural correctness with `validate_export_bundle(...)`.

## Import Workflow (Telltale/Lean -> Quint)

1. Select importable properties with `parse_telltale_properties(...)`.
2. Generate Quint invariant module text with `generate_quint_invariant_module(...)`.
3. Map certificates into Quint assertion comments with `map_certificates_to_quint_assertions(...)`.

## Cross-Validation Workflow

Use `run_cross_validation(...)` from `aura-quint`:

- Executes Quint checks through a `QuintModelCheckExecutor`.
- Compares Quint outcomes to bridge proof certificates.
- Emits a `CrossValidationReport` with explicit discrepancy entries.

CI lane:

```bash
just ci-lean-quint-bridge
```

Artifacts:

- `artifacts/lean-quint-bridge/bridge.log`
- `artifacts/lean-quint-bridge/report.json`

## Failure Handling

When cross-validation reports discrepancies:

1. Confirm property identity mapping (`property_id`) between model/proof pipelines.
2. Re-run the failing property in Quint and capture trace/counterexample.
3. Re-check proof certificate assumptions against current protocol model.
4. Do not merge until bridge mismatch is resolved or explicitly justified.

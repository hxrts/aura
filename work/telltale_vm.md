# Telltale VM Integration Plan for Aura Verification

## Objective

Aura should keep existing Lean and Quint domain verification for consensus and CRDT properties.
Aura should add Telltale VM parity as a runtime conformance and cross-validation layer.
The integration should improve simulator parity coverage without replacing domain proofs.

## Architecture Summary

### Current Verification Roles

Aura currently uses three verification lanes.
`verification/lean/` proves consensus and CRDT domain properties.
`verification/quint/` model checks protocol invariants and temporal properties.
`aura-agent` and CI conformance lanes validate runtime behavior and target parity.

### Target Verification Roles

Aura keeps domain proof ownership in existing Lean and Quint models.
Telltale VM adds executable parity checks for choreography runtime behavior.
The theorem-pack capability system remains an admission gate for runtime profiles.

### Design Constraints

Telltale VM parity must not weaken or remove current domain theorem coverage.
Simulator parity must remain deterministic and artifact driven.
Bridge formats and reports must be versioned and checked in CI.

## Component Model

### Domain Proof Layer

This layer remains unchanged in ownership.
`verification/lean/` and `verification/quint/` are authoritative for consensus and CRDT correctness claims.
Coverage here is measured by theorem and invariant inventories.

### Runtime Parity Layer

This layer uses Telltale VM traces and Aura replay artifacts.
It checks that runtime execution conforms to admitted profiles and expected observable envelopes.
Coverage here is measured by parity lanes, scenario contract bundles, and mismatch diagnostics.

### Bridge and Cross-Validation Layer

This layer maps claims and artifacts between Lean, Quint, Telltale, and Aura replay outputs.
It uses `aura-quint` bridge modules for schema, import/export, and discrepancy checks.
Coverage here is measured by bridge schema checks and CI bridge reports.

### Simulator Integration Layer

This layer extends `aura-simulator` with Telltale parity-aware replay workflows.
It keeps current deterministic effect composition and adds optional Telltale-backed comparison.
Coverage here is measured by deterministic replay parity and envelope-bounded differential checks.

## Data and Artifact Flows

### Flow A: Domain Verification

Quint and Lean run on protocol/domain models.
They emit invariant and theorem outcomes.
Coverage is recorded in verification coverage reporting.

### Flow B: Runtime Conformance

Telltale VM and Aura runtime execute equivalent choreography scenarios.
Both produce normalized conformance artifacts.
Differential comparison classifies mismatches with strict and envelope-bounded policies.

### Flow C: Cross-Validation

Bridge bundles package property claims and certificates.
Quint-side rechecks and bridge validators compare outcomes.
CI produces machine-readable reports for discrepancy triage.

## Non-Goals

This plan does not replace Aura consensus or CRDT theorem development with Telltale-only proofs.
This plan does not remove existing Quint model-checking lanes.
This plan does not collapse verification into a single toolchain.

## Implementation Tasks

## Phase 1: Architecture and Interfaces

- [x] Define the Telltale VM parity boundary in `aura-simulator`.
Success criteria: A documented interface exists for running parity comparison without changing default simulator behavior.
- [x] Define canonical artifact mapping between Aura conformance artifacts and Telltale VM outputs.
Success criteria: Mapping rules are documented and include required surfaces, normalization rules, and schema versioning.
- [x] Define bridge ownership across `aura-quint`, `aura-agent`, and `aura-simulator`.
Success criteria: Each crate has clear responsibilities and no duplicate bridge logic.

## Phase 2: Runtime and Simulator Integration

- [x] Add simulator entry points for Telltale-backed parity execution.
Success criteria: A simulator lane can run parity checks from deterministic scenarios and emit comparison artifacts.
- [x] Integrate parity comparison with `DifferentialTester` profiles.
Success criteria: Both `strict` and `envelope_bounded` policies can evaluate Telltale-vs-Aura outputs.
- [x] Add failure diagnostics for first mismatch with surface and step indexing.
Success criteria: Reports identify mismatch location and classification in a stable JSON payload.

## Phase 3: Bridge and Verification Pipeline

- [x] Expand `aura-quint` bridge use from schema tests to executable pipeline checks.
Success criteria: CI invokes bridge import/export and cross-validation with real fixture inputs.
- [x] Add deterministic fixtures for bridge cross-validation.
Success criteria: At least one positive and one negative fixture exist and are asserted in tests.
- [x] Add CI artifact emission for Telltale parity and bridge discrepancy reports.
Success criteria: CI stores reports under `artifacts/` with stable schema tags and replay metadata.

## Phase 4: Documentation Updates

- [x] Update [Verification Coverage Report](../docs/998_verification_coverage.md) with Telltale VM parity and bridge coverage sections.
Success criteria: The report lists Telltale parity lanes, bridge modules, coverage metrics, and artifact outputs.
- [x] Update [Testing Guide](../docs/805_testing_guide.md) with Telltale VM parity architecture and mismatch taxonomy.
Success criteria: The doc defines how Telltale parity relates to native and wasm lanes and differential envelopes.
- [x] Update [Simulation Infrastructure Reference](../docs/118_simulator.md) with simulator integration points for Telltale parity workflows.
Success criteria: The doc describes new simulator paths, configuration, and expected outputs.
- [x] Update [Formal Verification Reference](../docs/119_verification.md) with separation between domain proofs and runtime parity checks.
Success criteria: The doc explicitly distinguishes theorem/model claims from runtime conformance guarantees.
- [x] Update [Verification and MBT Guide](../docs/807_verification_guide.md) with operator workflows for running Telltale parity and bridge checks.
Success criteria: The guide includes runnable commands and failure triage workflow.
- [x] Update [Distributed Systems Contract](../docs/004_distributed_systems_contract.md) with a short section that classifies Telltale parity guarantees and limits.
Success criteria: The contract links runtime parity guarantees to existing safety and liveness assumptions without redefining domain invariants.

## Phase 5: Verification Coverage Automation

- [x] Update `scripts/check-verification-coverage.sh` to validate Telltale parity metrics and bridge pipeline coverage.
Success criteria: Script checks all new metrics referenced in `docs/998_verification_coverage.md` and fails on drift.
- [x] Add script checks for documented CI lanes that are now required by Telltale parity integration.
Success criteria: Missing lane entries in docs or justfile are detected as coverage mismatches.
- [x] Add script checks for Telltale parity artifact schema entries.
Success criteria: Script validates documented schema identifiers against checked-in references.

## Phase 6: Rollout and Exit Criteria

- [x] Run `just ci-conformance`, `just ci-lean-quint-bridge`, and the updated verification coverage script in one verification bundle.
Success criteria: All lanes pass and generated artifacts contain no undocumented schema or missing coverage entries.
- [x] Record final verification boundary statement in docs to prevent future proof-surface drift.
Success criteria: Documentation clearly states what Telltale parity verifies and what remains under domain Lean and Quint proof ownership.

## Definition of Done

The codebase has a documented and implemented Telltale VM parity layer integrated with simulator and CI.
Domain consensus and CRDT proofs remain owned by existing Aura Lean and Quint assets.
Documentation and automation scripts agree on verification inventory and fail on drift.

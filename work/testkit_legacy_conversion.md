# aura-testkit Legacy Verification Type Conversion Plan

## Goal

Refactor all `aura-testkit` verification call sites to use the new full-fidelity Lean types, then remove legacy compatibility types/exports from:

- `crates/aura-testkit/src/verification/mod.rs`
- `crates/aura-testkit/src/verification/lean_oracle.rs`
- related tests and consumers

## Scope

- In scope:
  - `aura-testkit` verification module and tests
  - crates/tests in this repo that import legacy verification types
  - docs/comments describing legacy compatibility
- Out of scope:
  - unrelated protocol/runtime refactors
  - changing Lean theorem definitions unless required by payload schema drift

## Definition of Done

- No legacy verification types are re-exported from `verification/mod.rs`.
- No repository call sites use legacy types (`Fact`, legacy `TimeStamp`, legacy `Ordering`, etc.).
- Differential and verification tests pass for updated type surface.
- Documentation reflects only the new full-fidelity type model.

---

## Phase 0: Baseline and Inventory

### Tasks

- [x] Create a complete inventory of legacy symbols and all call sites with `rg`.
- [x] Categorize each usage: migrate, replace with adapter, or delete.
- [x] Capture baseline test status before refactor.
- [x] Document migration mapping table (legacy type -> new type/API).

### Success Criteria

- [x] Inventory file/checklist is complete and reviewed.
- [x] No unknown legacy usage remains unclassified.
- [x] Baseline command outputs are saved (or summarized) for comparison.

### Verify + Commit Gate

- [x] Run:
  - `nix develop --command cargo test -p aura-testkit --lib`
  - `nix develop --command cargo test -p aura-testkit --tests`
- [x] Commit baseline inventory artifacts and plan updates:
  - Suggested commit: `testkit: inventory legacy verification type usage`

### Phase 0 Inventory Results

#### Legacy Symbols Defined

- `ComparePolicy`, `Fact`, `FlowChargeInput`, `FlowChargeResult`
- `JournalMergeInput`, `JournalMergeResult`, `JournalReduceInput`, `JournalReduceResult`
- `TimeStamp`, `Ordering`, `TimestampCompareInput`, `TimestampCompareResult`
- `LeanOracleResult`, `OracleVersion`

#### Call Site Classification

- `crates/aura-testkit/src/verification/mod.rs`
  - legacy re-exports: `MIGRATE+REMOVE` (remove after consumers are migrated)
- `crates/aura-testkit/src/verification/lean_oracle.rs`
  - legacy type definitions and methods (`verify_merge`, `verify_reduce`, `verify_charge`, `verify_compare`): `ADAPTER THEN REMOVE`
- `crates/aura-testkit/tests/lean_differential.rs`
  - direct imports and usage of legacy compare/flow structs: `MIGRATE`
  - dead-code legacy helpers (`legacy_fact_strategy`, `legacy_journal_strategy`, etc.): `DELETE`
- Workspace external usage
  - no non-`aura-testkit` consumers found importing these legacy symbols: `NONE`

#### Migration Mapping Table (Working)

| Legacy Symbol | Target Symbol / API | Planned Action |
| --- | --- | --- |
| `Fact` | `LeanFact` | Migrate tests and call sites |
| `TimeStamp` | `LeanTimeStamp` | Migrate compare tests/call sites |
| `Ordering` | `LeanTimestampOrdering` (new typed enum) | Introduce typed API, migrate, remove legacy |
| `ComparePolicy` | `LeanComparePolicy` (new typed policy) | Introduce typed API, migrate, remove legacy |
| `JournalMergeInput`/`JournalReduceInput` | `FullJournalMergeInput`/`FullJournalReduceInput` + `LeanJournal` | Keep full-fidelity path, remove legacy path |
| `JournalMergeResult`/`JournalReduceResult` | `LeanJournalMergeResult`/`LeanJournalReduceResult` | Migrate consumers |
| `FlowChargeInput`/`FlowChargeResult` | `LeanFlowChargeInput`/`LeanFlowChargeResult` (new typed structs) | Introduce typed API, migrate, remove legacy |
| `TimestampCompareInput`/`TimestampCompareResult` | `LeanTimestampCompareInput`/`LeanTimestampCompareResult` (new typed structs) | Introduce typed API, migrate, remove legacy |

#### Baseline Test Summary

- `cargo test -p aura-testkit --lib`: pass
  - result: `155 passed; 0 failed`
- `cargo test -p aura-testkit --tests`: pass
  - result: crate and integration test suite green (including `lean_differential` compiled in default mode)

---

## Phase 1: Introduce New-Type API Surface (No Breaking Removal Yet)

### Tasks

- [x] Add/normalize new typed request/response structs for operations still using legacy payloads (merge/reduce/charge/compare).
- [x] Add conversion helpers where needed (`legacy -> new`, temporary internal only).
- [x] Mark legacy types and methods as deprecated in rustdoc/comments.
- [x] Keep behavior unchanged while dual-surface exists.

### Success Criteria

- [x] All operational paths can be exercised via new typed APIs.
- [x] Legacy symbols are still present but clearly deprecated.
- [x] No behavior regressions in existing tests.

### Verify + Commit Gate

- [x] Run:
  - `nix develop --command cargo test -p aura-testkit --lib`
  - `nix develop --command cargo test -p aura-testkit --features lean --lib`
  - `nix develop --command cargo test -p aura-testkit --test lean_differential --features lean -- --ignored`
- [ ] Commit:
  - Suggested commit: `testkit: add new typed verification APIs and deprecate legacy surface`

### Phase 1 Notes

- Added canonical typed payloads in `lean_types`:
  - `LeanFlowChargeInput` / `LeanFlowChargeResult`
  - `LeanComparePolicy` / `LeanCompareTimeStamp`
  - `LeanTimestampCompareInput` / `LeanTimestampCompareResult` / `LeanTimestampOrdering`
- Added typed oracle APIs in `lean_oracle`:
  - `verify_flow_charge(...)`
  - `verify_timestamp_compare(...)`
- Legacy `verify_charge(...)` and `verify_compare(...)` now delegate through typed APIs.
- Legacy surface remains exported for compatibility, with deprecation notes in doc comments.
- Built Lean oracle binary (`just lean-oracle-build`) to run ignored differential tests during gate.

---

## Phase 2: Migrate All Internal Call Sites

### Tasks

- [ ] Update `crates/aura-testkit/tests/lean_differential.rs` to remove legacy type usage.
- [ ] Rewrite helper strategies using `LeanFact`, `LeanTimeStamp`, `LeanJournal`, etc.
- [ ] Update any remaining internal references to legacy compare/merge/reduce structures.
- [ ] Remove dead helper code that exists only for legacy paths.

### Success Criteria

- [ ] `aura-testkit` internal code and tests compile without importing legacy verification symbols.
- [ ] Differential tests validate the same semantics via new types.

### Verify + Commit Gate

- [ ] Run:
  - `nix develop --command cargo test -p aura-testkit --features lean --test lean_differential`
  - `nix develop --command cargo test -p aura-testkit --features lean`
- [ ] Commit:
  - Suggested commit: `testkit: migrate internal verification tests to full-fidelity Lean types`

---

## Phase 3: Migrate External Repo Call Sites

### Tasks

- [ ] Search workspace for imports from `aura_testkit::verification` legacy symbols.
- [ ] Update all consuming crates/tests/examples to new symbols.
- [ ] Ensure no public docs/snippets reference legacy verification types.

### Success Criteria

- [ ] Workspace-wide search returns zero usage of targeted legacy symbols.
- [ ] All downstream tests compile with new imports and APIs.

### Verify + Commit Gate

- [ ] Run:
  - `nix develop --command rg -n "\\b(ComparePolicy|Fact|TimeStamp|Ordering|JournalMergeInput|JournalReduceInput|FlowChargeInput|TimestampCompareInput)\\b" crates tests examples verification`
  - `nix develop --command just test-crate aura-testkit`
  - `nix develop --command just test`
- [ ] Commit:
  - Suggested commit: `testkit: migrate workspace verification call sites to new types`

---

## Phase 4: Remove Legacy Types and Re-exports

### Tasks

- [ ] Delete legacy type definitions and legacy-only methods from `lean_oracle.rs`.
- [ ] Remove legacy re-exports from `verification/mod.rs`.
- [ ] Remove compatibility comments referring to retained legacy support.
- [ ] Simplify module/docs to a single canonical type surface.

### Success Criteria

- [ ] Legacy verification symbols no longer exist in code.
- [ ] Public API exports only full-fidelity types.
- [ ] `cargo doc`/rustdoc comments no longer claim legacy compatibility.

### Verify + Commit Gate

- [ ] Run:
  - `nix develop --command cargo check -p aura-testkit --all-features`
  - `nix develop --command cargo test -p aura-testkit --all-features`
  - `nix develop --command just check-arch`
- [ ] Commit:
  - Suggested commit: `testkit: remove legacy verification types and re-exports`

---

## Phase 5: Final Hardening and Documentation Cleanup

### Tasks

- [ ] Update docs that mention legacy verification types (`docs/806_verification_guide.md`, `verification/README.md`, inline rustdoc).
- [ ] Add a short migration note/changelog entry for downstream users.
- [ ] Run full verification-related CI lanes locally where feasible.

### Success Criteria

- [ ] Documentation consistently describes only the new type model.
- [ ] Verification lanes are green.
- [ ] Migration note includes breaking-change guidance.

### Verify + Commit Gate

- [ ] Run:
  - `nix develop --command just ci-lean-quint-bridge`
  - `nix develop --command just ci-simulator-telltale-parity`
  - `nix develop --command just ci-dry-run`
- [ ] Commit:
  - Suggested commit: `docs: finalize legacy verification type migration notes`

---

## Final Validation Checklist

- [ ] `rg` confirms zero legacy symbol usage and definitions.
- [ ] `aura-testkit` lean differential tests pass on new types.
- [ ] Workspace tests and architecture checks pass.
- [ ] Commits are phase-separated and reversible.
- [ ] PR description includes:
  - migration mapping table
  - breaking changes
  - test evidence by phase

# Aura Macros (Layer 2) - Architecture and Invariants

## Purpose
Compile-time DSL parser for choreographies with Aura-specific annotations. Generates
type-safe Rust code for distributed protocols.

## Inputs
- Choreography protocol specifications (token streams).
- Domain fact enum definitions.
- Effect trait declarations and handler specs.
- Aura-specific annotations (`guard_capability`, `flow_cost`, `journal_facts`).

## Outputs
- `choreography!` macro: Full Telltale feature inheritance with Aura extensions.
- `DomainFact` derive macro: Canonical encoding with schema versioning.
- `aura_effect_handlers` macro: Mock/real handler variant boilerplate.
- `aura_handler_adapters` macro: AuraHandler trait adapters.
- `aura_test` attribute macro: Async test setup with tracing.
- `src/bin/arch_lints.rs`: repo-local Rust-native syntax lints used by
  `just lint-arch-syntax`.

## Invariants
- Depends only on aura-core (pure compile-time code generation).
- Is a proc-macro crate (no runtime code).
- All work happens at compile time.
- Uses empty extension registry (extensions handled by aura-macros itself).

## Ownership Model

- `aura-macros` is primarily `Pure`.
- It owns compile-time translation, not `ActorOwned` runtime lifecycle.
- Ownership transfer and capability requirements should appear in generated
  typed surfaces rather than being inferred from ad hoc runtime conventions.
- Macro output may expose `MoveOwned` or capability-gated contracts, but the
  macro crate does not own those lifecycles at runtime.
- `Observed` tooling may inspect expansions, not mutate semantic truth.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| proc-macro parsers and expanders in `src/` | `Pure` | Compile-time parsing and code generation only. |
| generated choreography/fact/handler surfaces | `Pure` producer | Macro output may encode `MoveOwned` and capability-gated contracts, but the macro crate does not own them at runtime. |
| Actor-owned runtime state | none | Proc-macro crates must not own runtime lifecycle or background tasks. |
| Observed-only surfaces | none | Macro inspection tooling lives outside the crate. |

### Capability-Gated Points

- generated typed capability surfaces and ownership contracts consumed by
  downstream crates

### Verification Hooks

- `cargo check -p aura-macros`
- `cargo test -p aura-macros -- --nocapture`

### Enforcement Split

- `aura-macros` is part of the compile-time enforcement path for boundary-shape
  rules that can be expressed through generated surfaces or Rust-native linting.
- `src/bin/arch_lints.rs` owns the Rust-native lint path for grep-heavy syntax
  rules that moved out of `scripts/check/arch.sh` in the architecture upgrade.
- `scripts/check/arch.sh` should not remain the primary enforcement path for a
  rule when macro expansion, visibility, or linting can reject the pattern
  earlier and more precisely.
- Repo-wide topology, docs/governance, and semantic integration checks still
  belong to `just check-arch`.

### Detailed Specifications

### InvariantChoreographyAnnotationProjection
Choreography annotations must project deterministically into runtime metadata.

Enforcement locus:
- src proc-macro parsing captures guard, flow, and leakage annotations.
- Expansion outputs remain compile-time only and avoid runtime side effects.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just test-crate aura-macros
- just lint-arch-syntax

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) defines annotation semantics for guards and leakage.
- [MPST and Choreography](../../docs/110_mpst_and_choreography.md) defines projection expectations.
## Testing

### Strategy

aura-macros is a proc-macro crate — all work happens at compile time. The
critical concern is that valid inputs compile and invalid inputs produce
clear errors. If a valid choreography is rejected or an invalid one is
silently accepted, the DSL contract is broken.

### Running tests

```
cargo test -p aura-macros --test compile_fail  # boundary tests
cargo test -p aura-macros --lib                # inline unit tests
```

To regenerate `.stderr` files after intentional changes:
```
TRYBUILD=overwrite cargo test -p aura-macros --test compile_fail
```

### Coverage matrix

| What breaks if wrong | Test file | Status |
|---------------------|----------|--------|
| Valid choreography annotations rejected | `boundaries/valid_annotations.rs` | covered (pass) |
| Valid ceremony facts rejected | `boundaries/ceremony_facts_valid.rs` | covered (pass) |
| Valid semantic_owner rejected | `boundaries/semantic_owner_valid.rs` | covered (pass) |
| Valid actor_owned rejected | `boundaries/actor_owned_valid.rs` | covered (pass) |
| Valid capability_boundary rejected | `boundaries/capability_boundary_valid.rs` | covered (pass) |
| Valid ownership_lifecycle rejected | `boundaries/ownership_lifecycle_valid.rs` | covered (pass) |
| Invalid flow_cost silently accepted | `boundaries/invalid_flow_cost.rs` | covered (compile_fail) |
| Invalid guard_capability accepted | `boundaries/invalid_guard_capability.rs` | covered (compile_fail) |
| Self-send accepted | `boundaries/incoherent_self_send.rs` | covered (compile_fail) |
| Missing namespace accepted | `boundaries/missing_namespace.rs` | covered (compile_fail) |
| semantic_owner missing context | `boundaries/semantic_owner_missing_context.rs` | covered (compile_fail) |
| semantic_owner missing owner | `boundaries/semantic_owner_missing_owner.rs` | covered (compile_fail) |
| semantic_owner missing category | `boundaries/semantic_owner_missing_category.rs` | covered (compile_fail) |
| semantic_owner missing terminal | `boundaries/semantic_owner_missing_terminal_path.rs` | covered (compile_fail) |
| actor_owned missing capacity | `boundaries/actor_owned_missing_capacity.rs` | covered (compile_fail) |
| actor_owned missing gate | `boundaries/actor_owned_missing_gate.rs` | covered (compile_fail) |
| actor_owned bypass without macro | `boundaries/actor_owned_bypass_without_macro.rs` | covered (compile_fail) |
| capability_boundary missing category | `boundaries/capability_boundary_missing_category.rs` | covered (compile_fail) |
| ownership_lifecycle invalid variant | `boundaries/ownership_lifecycle_invalid_variant.rs` | covered (compile_fail) |

## Boundaries
- No runtime code or effect implementations.
- Generated code uses types from aura-mpst for choreographies.
- No multi-party coordination (only generates code).

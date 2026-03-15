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

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) defines annotation semantics for guards and leakage.
- [MPST and Choreography](../../docs/110_mpst_and_choreography.md) defines projection expectations.
## Boundaries
- No runtime code or effect implementations.
- Generated code uses types from aura-mpst for choreographies.
- No multi-party coordination (only generates code).

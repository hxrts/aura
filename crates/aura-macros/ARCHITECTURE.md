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
- `choreography!` macro: Full rumpsteak-aura inheritance with Aura extensions.
- `DomainFact` derive macro: Canonical encoding with schema versioning.
- `aura_effect_handlers` macro: Mock/real handler variant boilerplate.
- `aura_handler_adapters` macro: AuraHandler trait adapters.
- `aura_test` attribute macro: Async test setup with tracing.

## Invariants
- Depends only on aura-core (pure compile-time code generation).
- Is a proc-macro crate (no runtime code).
- All work happens at compile time.
- Uses empty extension registry (extensions handled by aura-macros itself).

## Boundaries
- No runtime code or effect implementations.
- Generated code uses types from aura-mpst for choreographies.
- No multi-party coordination (only generates code).

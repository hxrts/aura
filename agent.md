# Aura Agent Refactor Plan

## Target Architecture
- Clear layering aligned to Layer 6 (`docs/001_system_architecture.md`): `agent-core` (public API, config/errors), `agent-runtime` (effect registry, builder, lifecycle, reliability, choreography adapter), `agent-handlers` (domain-specific handlers).
- Authority-first identity: all contexts derive device-local notions internally; no public `DeviceId` API.
- Single effect wiring surface: one registry/builder entrypoint; no ad-hoc wiring in handlers.
- Cohesive reliability + scheduling module; cohesive persistence utilities; cohesive choreography adapter for distributed flows.
- Explicit effect runtime objects per spec: `EffectExecutor`, `AuraEffectSystem`, `ContextManager`, `FlowBudgetManager`, `ReceiptManager` assembled via a single builder with explicit lifecycle.

## Success Criteria
- No file exceeds ~400 LOC; top-level public API surface under ~300 LOC.
- Zero backwards compatibility code, zero migration glue, zero legacy shims.
- Handlers use shared context/utility modules (persistence, reliability, choreography) instead of duplicating logic.
- Tests use shared support builders/mocks; no production stubs.
- Effect composition matches Layer-6 responsibilities: runtime orchestrates handlers, does not reimplement Layer-4 coordination or Layer-3 stateless effects.

## Tasks
- [ ] Define module boundaries and filenames for `agent-core`, `agent-runtime`, `agent-handlers` (or subcrates if split).
- [ ] Move effect registry/builder into a single module; delete redundant wiring paths.
- [ ] Extract authority-centric context type and replace public-facing `DeviceId` usage.
- [ ] Split `handlers/sessions.rs` into role-focused modules with shared helpers; remove duplication.
- [ ] Relocate or remove `runtime/coordinator_stub` and other test-only code behind feature-gated test support. Consider moving some test-only code to aura-testkit if appropriate.
- [ ] Consolidate reliability/propagation/backoff into one module; ensure handlers call it instead of custom loops.
- [ ] Consolidate persistence helpers (storage keys, effect API helpers) into a single utility module.
- [ ] Simplify `operations.rs` into declarative operations with typed inputs/outputs.
- [ ] Slim `agent.rs` to a thin façade delegating to the runtime; move wiring into the builder.
- [ ] Add shared test support module; excise production-time stubs.
- [ ] Ensure the runtime builder wires `EffectExecutor`, `ContextManager`, `FlowBudgetManager`, `ReceiptManager`, and lifecycle per the Layer-6 spec (no custom globals).

## Implementation Guidance
- Implementation must be concise, clean, and elegant.
- Zero backwards compatibility code, zero migration code, zero legacy code—prefer deletion over shims.
- Favor small modules and clear naming; keep comments minimal and explanatory when needed.

## Public API Principles
- Intentional surface: only expose the minimal constructors, config types, and operation entrypoints needed by consumers; keep everything else crate-private.
- Architecture-aligned: API shapes must adhere to the Layer-6 runtime composition contract in `docs/001_system_architecture.md` (effect boundaries explicit, choreography adapters isolated, no side-channel globals).
- Authority-first ergonomics: authority/context identifiers are first-class; device-local details are derived internally and never leaked in public types.
- Composable defaults: builders/constructors should express optional capabilities as additive traits or feature flags, not ad-hoc booleans.
- Choreography integration: expose adapters/hooks for choreographic protocols via the runtime, not bespoke networking paths.
- Zero legacy: no transitional aliases or compatibility shims in the public surface.

## Choreography & Macro Guidance
- Use `aura_macros::choreography!` for all multi-party coordination; avoid bespoke networking codepaths. Execute via the runtime’s choreography adapter so guard chains and journal coupling apply automatically.
- Generate handlers and implementations with `aura_macros::aura_effect_handlers!` / `aura_effect_implementations!` to remove boilerplate while keeping handlers stateless and single-party as required by the architecture layering.
- Bridge traits to the executor with `aura_macros::aura_handler_adapters!` instead of custom adapters to keep dispatch uniform.
- Replace the placeholder guard/flow hooks emitted by the choreography wrapper with real CapGuard → FlowGuard → JournalCoupler calls using runtime context; no stub validators in production.
- All nondeterminism and side effects (randomness, console/CLI I/O, file/network I/O) must enter through effect traits and be injected via the runtime builder; never access OS APIs directly from handlers.

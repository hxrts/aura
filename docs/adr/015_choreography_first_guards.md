# **ADR-015: Choreography-First Guard Architecture**

**Status**: Accepted
**Date**: 2024-12-XX
**Author**: Architecture Team

---

# 1. Context

Aura's guard system has evolved through multiple iterations. ADR-014 established pure guard evaluation with async effect interpretation. However, guard requirements were scattered across multiple locations:

* **aura-mpst**: Had its own `guards.rs` (291 lines), `leakage.rs` (495 lines), and `runtime.rs` (1428 lines) with duplicated implementations
* **aura-core**: Defined effect traits (`EffectCommand`, `EffectInterpreter`, `GuardOutcome`)
* **aura-protocol**: Had the canonical guard chain (`CapGuard`, `FlowGuard`, `JournalCoupler`)
* **choreography! macro**: Generated wrapper code but required manual effect wiring

This produced:

1. **Duplicated implementations** - Similar guard logic in multiple crates
2. **Disconnected specifications** - Choreographic annotations weren't automatically enforced
3. **Manual wiring** - Protocol authors had to manually connect annotations to effects
4. **Maintenance burden** - Changes required updates in multiple locations

---

# 2. Decision

**Make choreographic annotations the canonical source of truth for guard requirements.**

The `choreography!` macro now:

1. Parses guard annotations (`guard_capability`, `flow_cost`, `journal_facts`, `leak`, etc.)
2. Generates `EffectCommand` sequences from these annotations
3. Provides an `effect_bridge` module for runtime execution

Guard effects originate from two unified sources:

1. **Choreographic Annotations** (compile-time): Macro-generated `EffectCommand` sequences from DSL annotations
2. **Runtime Guard Chain** (send-site): Pure guards evaluating against `GuardSnapshot` at each send

Both produce `Vec<EffectCommand>` executed through the same `EffectInterpreter` infrastructure.

---

# 3. Architecture

## Effect Command Flow

```
┌─────────────────────────────────────────────────────────────────────┐
│ choreography! {                                                      │
│   Client[guard_capability = "send", flow_cost = 200]                │
│   -> Server: Request;                                               │
│ }                                                                    │
└───────────────────────────┬─────────────────────────────────────────┘
                            │ generates
                            ▼
┌─────────────────────────────────────────────────────────────────────┐
│ effect_bridge::annotation_to_commands()                              │
│ → Vec<EffectCommand> {                                               │
│     ChargeBudget { ... amount: 200 },                                │
│     StoreMetadata { key: "guard_validated", value: "send" },         │
│   }                                                                  │
└───────────────────────────┬─────────────────────────────────────────┘
                            │ + runtime guards
                            ▼
┌─────────────────────────────────────────────────────────────────────┐
│ EffectInterpreter::execute()                                         │
│ → Production / Simulation / Test                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## Supported Annotations

| Annotation | Description | Generated Effect |
|------------|-------------|------------------|
| `guard_capability = "cap"` | Capability requirement | `StoreMetadata` (audit trail) |
| `flow_cost = N` | Flow budget charge | `ChargeBudget` |
| `journal_facts = "fact"` | Journal fact recording | `StoreMetadata` (fact key) |
| `journal_merge = true` | Request journal merge | `StoreMetadata` (merge flag) |
| `audit_log = "event"` | Audit trail entry | `StoreMetadata` (audit key) |
| `leak = "External"` | Leakage tracking | `RecordLeakage` |

## Layer Responsibilities

| Layer | Crate | Responsibility |
|-------|-------|----------------|
| 1 | aura-core | `EffectCommand`, `EffectInterpreter`, `GuardOutcome` types |
| 2 | aura-mpst | Session type runtime (deprecated guard implementations) |
| 2 | aura-macros | Annotation parsing, `effect_bridge` generation |
| 3 | aura-effects | `ProductionEffectInterpreter` implementation |
| 4 | aura-protocol | Guard chain orchestration, `execute_guarded_choreography()` |
| 6 | aura-simulator | `SimulationEffectInterpreter` implementation |

---

# 4. Consequences

## Positive

* **Single source of truth**: Guard requirements defined in choreography DSL
* **Automatic enforcement**: Annotations generate executable effect commands
* **Unified execution**: Same `EffectCommand` types across macro and runtime guards
* **Testing parity**: Same effect model in production, simulation, and tests
* **Reduced duplication**: ~1000+ lines removed from aura-mpst

## Negative

* **Migration required**: Existing code using aura-mpst guard types must migrate
* **Learning curve**: Developers must understand both annotation and runtime guard sources

## Neutral

* **aura-mpst simplification**: Module reduced to session type runtime semantics only
* **Deprecation timeline**: Old APIs deprecated but kept for compatibility until v1.0

---

# 5. Implementation

## Phase 1: Consolidate Effect Types ✓
- Removed `aura-mpst::guards` module
- Added deprecation markers to `aura-mpst::runtime`
- Verified single source of `EffectCommand` in aura-core

## Phase 2: Macro Code Generation ✓
- Enhanced annotation parsing (journal_merge, audit_log, leak variants)
- Generated `effect_bridge` module with `annotation_to_commands()`
- Added `execute_commands()` for interpreter integration

## Phase 3: Effect Interpreters ✓
- `ProductionEffectInterpreter` handles all 6 command types
- `SimulationEffectInterpreter` provides deterministic execution
- `BorrowedEffectInterpreter` for protocol-layer integration

## Phase 4: Protocol Layer ✓
- Added `execute_effect_commands()` helper
- Added `execute_guarded_choreography()` for combined execution
- Updated guards module documentation

## Phase 5: Documentation ✓
- Updated docs/107_mpst_and_choreography.md
- Created ADR-015 (this document)

## Phase 6: Migration and Cleanup ✓
- Fixed test compilation errors in aura-protocol
- Updated PhysicalTimeEffects trait implementations
- Fixed TimeStamp and FlowBudgetView API usage
- Updated test macros to use tokio::test
- Verified core crates build successfully
- Created comprehensive integration tests (choreography_guards_integration.rs, 9 tests)
- Note: Deprecated code removal scheduled for v1.0

---

# 6. References

* ADR-014: Pure Guard Evaluation with Asynchronous Effect Interpretation
* docs/107_mpst_and_choreography.md: Multi-party Session Types and Choreography
* docs/003_information_flow_contract.md: Information Flow Contract
* docs/106_effect_system_and_runtime.md: Effect System and Runtime

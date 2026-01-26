# Generative Testing Guide

This guide explains Aura's approach to generative testing where formal specifications drive test case generation and execution.

## Philosophy

Generative testing bridges the gap between formal verification and empirical testing. Traditional testing verifies specific scenarios while formal verification proves properties over infinite state spaces. Generative testing occupies the middle ground: formal specifications generate test cases that execute against real implementations.

### Why Generative Testing?

**Limitation of traditional tests**: Hand-written tests capture known scenarios but miss unexpected interactions. Distributed protocols have combinatorial state spaces that manual testing cannot cover.

**Limitation of model checking alone**: Model checkers verify abstract state machines but cannot detect implementation bugs, performance issues, or environment interactions.

**Generative testing combines both**: Quint specifications define correct behavior and generate traces. The simulator executes those traces through real effect handlers, catching both specification violations and implementation defects.

### The Trust Chain

```
Quint Specification (formal model)
        │
        ▼ generates
   ITF Traces (execution scripts)
        │
        ▼ replayed through
   Aura Effect Handlers (real implementation)
        │
        ▼ produces
   Property Evaluations (pass/fail verdicts)
```

Each link in this chain adds verification value:
1. **Specification → Traces**: Validates state machine reachability
2. **Traces → Handlers**: Validates implementation matches model
3. **Handlers → Properties**: Validates invariants hold under execution

## Writing Quint Specs for Simulation

See `verification/quint/` for Aura-specific Quint syntax patterns.

### Key Principles

**Authority model**: Use `AuthorityId` (opaque identity) not `DeviceId` (internal structure). Specifications should model relational semantics without exposing device-level details.

**State machine design**: Each protocol should have clear phases with explicit transitions:

```quint
type Phase = Setup | Active | Completed | Failed

action transition(pid: ProtocolId, targetPhase: Phase): bool = all {
  currentPhase(pid) != Completed,
  currentPhase(pid) != Failed,
  validTransition(currentPhase(pid), targetPhase),
  phases' = phases.set(pid, targetPhase)
}
```

**Harness modules**: Every protocol spec should have a corresponding harness module that provides standardized entry points:

```quint
module harness_myprotocol {
  import protocol_myprotocol from "./protocol_myprotocol"

  action register(...): bool = ...  // Initialize protocol
  action complete(pid): bool = ...  // Signal completion
  action abort(pid): bool = ...     // Signal failure
}
```

### Mapping Quint Actions to Effects

The `ActionRegistry` in `aura-simulator/src/quint/` maps Quint action names to effect system handlers:

| Quint Action | Effect Handler | Notes |
|--------------|----------------|-------|
| `submitShare` | `CryptoEffects::frost_sign` | Guardian share contribution |
| `recordCommitment` | `JournalEffects::append_fact` | Commitment fact recording |
| `deliverShare` | `TransportEffects::send` | P2P share delivery |

## ITF Trace Interpretation

See [Simulation Guide](806_simulation_guide.md) §Generative Simulation for detailed trace format.

### Trace Structure

ITF (Informal Trace Format) traces contain:

```json
{
  "#meta": { "format": "ITF", "source": "quint" },
  "vars": ["phase", "participants", "shares"],
  "states": [
    { "#meta": { "index": 0 }, "phase": "Setup", ... },
    { "#meta": { "index": 1 }, "phase": "Active", ... }
  ]
}
```

### Replaying Traces

The `GenerativeSimulator` replays traces step-by-step:

1. **Load trace**: Parse ITF JSON, extract state sequence
2. **Initialize**: Map initial state to Aura runtime state
3. **Execute steps**: For each transition, invoke registered handlers
4. **Evaluate properties**: After each step, check invariants

### Non-deterministic Choices

ITF traces capture `nondet_picks` for reproducible non-determinism:

```json
{
  "#meta": { "index": 3, "nondet_picks": { "guardian": "alice" } },
  ...
}
```

The simulator injects these choices into `RandomEffects` to ensure trace replay produces identical results.

## Testing Workflow

### 1. Develop Specification

Write Quint spec in `verification/quint/` following authority model and syntax patterns:

```bash
quint typecheck verification/quint/protocol_myprotocol.qnt
```

### 2. Generate Traces

Use Quint simulator or Apalache model checker to generate traces:

```bash
quint run --main=harness_myprotocol --out-itf=trace.itf.json
```

### 3. Replay Through Simulator

Execute traces through generative simulator:

```rust
let simulator = GenerativeSimulator::new(effects)?;
let results = simulator.replay_trace(&trace).await?;
assert!(results.all_properties_passed());
```

### 4. Property Evaluation

Check specification properties against execution results:

- **Safety**: Bad states never reached
- **Liveness**: Good states eventually reached (bounded)
- **Invariants**: Properties hold in all observed states

## Property Categories

The `aura-quint` runner classifies properties by keyword:

| Category | Keywords | Examples |
|----------|----------|----------|
| Authorization | `grant`, `permit`, `guard` | `guardChainOrder`, `noCapabilityWidening` |
| Budget | `budget`, `charge`, `spent` | `chargeBeforeSend`, `spentWithinLimit` |
| Integrity | `attenuation`, `signature`, `chain` | `attenuationOnlyNarrows`, `receiptChainIntegrity` |

## Best Practices

1. **Start with invariants**: Define properties before actions. Clear invariants guide action design.

2. **Use unique variant names**: Quint requires globally unique sum type variants. Prefix with domain: `DkgSetup`, `RecoverySetup`.

3. **Test harnesses separately**: Verify harness modules parse before integrating with simulator.

4. **Incremental traces**: Start with short traces (3-5 steps) to debug action mappings before exhaustive exploration.

5. **Property isolation**: Test one property at a time during development. Combine for coverage testing.

## Related Documentation

- [Simulation Guide](806_simulation_guide.md) - Deterministic simulation and generative infrastructure
- [Testing Guide](805_testing_guide.md) - Unit, integration, and property testing
- [Effect System Guide](105_effect_system_and_runtime.md) - Effect trait architecture

# MPST and Choreography

This document describes the architecture of choreographic protocols in Aura. It explains how global protocols are defined, projected, and executed. It defines the structure of local session types, the integration with the [Effect System](103_effect_system.md), and the use of [guard chains](106_authorization.md) and journal coupling.

## 1. DSL and Projection

Aura defines global protocols using the `choreography!` macro. The macro parses a global specification into an abstract syntax tree. The macro produces code that represents the protocol as a choreographic structure. The source of truth for protocols is a `.choreo` file stored next to the Rust module that loads it.

Projection converts the global protocol into per-role local session types. Each local session type defines the exact sequence of sends and receives for a single role. Projection eliminates deadlocks and ensures that communication structure is correct.

```rust
choreography!(include_str!("example.choreo"));
```

Example file: `example.choreo`
```
module example exposing (Example)

protocol Example =
  roles A, B
  A -> B : Msg(data: Vec<u8>)
  B -> A : Ack(code: u32)
```

This snippet defines a global protocol with two roles. Projection produces a local type for `A` and a local type for `B`. Each local type enforces the required ordering at compile time.

## 2. Local Session Types

Local session types describe the allowed actions for a role. Each send and receive is represented as a typed operation. Local types prevent protocol misuse by ensuring that nodes follow the projected sequence.

Local session types embed type-level guarantees. These guarantees prevent message ordering errors. They prevent unmatched sends or receives. Each protocol execution must satisfy the session type.

```rust
type A_Local = Send<B, Msg, Receive<B, Ack, End>>;
```

This example shows the projected type for role `A`. The type describes that `A` must send `Msg` to `B` and then receive `Ack`.

## 3. Runtime Integration

Aura executes production choreographies through the Telltale protocol machine. The `choreography!` macro emits the global type, projected local types, role metadata, and composition metadata that the runtime uses to build protocol-machine code images. `AuraChoreoEngine` in `crates/aura-agent/src/runtime/choreo_engine.rs` is the production runtime surface.

Generated runners still expose role-specific execution helpers. Aura keeps those helpers for tests, focused migration utilities, and narrow tooling paths. They are not the production execution boundary.

Generated runtime artifacts also carry the data that production startup needs:
- `provide_message` for outbound payloads
- `select_branch` for choice decisions
- protocol id and determinism policy reference
- required capability keys
- link and delegation constraints
- operational-envelope selection inputs

These values are sourced from runtime state such as params, journal facts, UI inputs, and manifest-driven admission state.

Aura has one production choreography backend:
- protocol-machine backend (`AuraChoreoEngine`) for admitted Telltale runtime execution, replay, and parity checks.

The authoritative async ownership contract for how `aura-agent` hosts these sessions lives in `crates/aura-agent/ARCHITECTURE.md`.

That contract is intentionally split:

- actor services structure the long-lived host runtime
- move semantics define fragment, session, and endpoint ownership transfer

This distinction matters because `delegate` is not merely another actor message.

Direct generated-runner execution is test and migration support only.

Production runtime ownership is fragment-scoped. The admitted unit is one protocol fragment derived from the generated `CompositionManifest`. A manifest without `link` metadata yields one protocol fragment. A manifest with `link` metadata yields one fragment per linked bundle.

`delegate` and `link` define how ownership moves. Local runtime services claim fragment ownership through `AuraEffectSystem`. Runtime transfer goes through `ReconfigurationManager`. The runtime rejects ambiguous local ownership before a transfer reaches the protocol machine.

The host runtime may use actor services to supervise the surrounding work, but fragment ownership itself remains a singular move boundary with stale-owner rejection.

Owner record and capability are also distinct here:

- ownership answers which local runtime currently owns the fragment
- capability answers which fragment-scoped effects that owner may drive

Delegation must define both the ownership handoff and the capability scope that moves with it.

Host-side async code must preserve that ownership model. External network, timer, and callback work enters through canonical ingress and is routed to the current local owner before any session mutation occurs.

`VmBridgeEffects` is the synchronous host boundary for one fragment. Protocol-machine callbacks use it for session-local payload queues, blocked receive snapshots, and scheduler signals. Async transport, journal, and storage work stay outside the callback path in the host bridge loop.

## 4. Choreography Annotations and Effect Commands

Choreographies support annotations that modify runtime behavior. The `choreography!` macro extracts these annotations and generates `EffectCommand` sequences. This follows the choreography-first architecture where choreographic annotations are the canonical source of truth for guard requirements.

### Supported Annotations

| Annotation | Description | Generated Effect |
|------------|-------------|------------------|
| `guard_capability = "namespace:capability"` | Canonical capability requirement | `StoreMetadata` (audit trail) |
| `flow_cost = N` | Flow budget charge | `ChargeBudget` |
| `journal_facts = "fact"` | Journal fact recording | `StoreMetadata` (fact key) |
| `journal_merge = true` | Request journal merge | `StoreMetadata` (merge flag) |
| `audit_log = "event"` | Audit trail entry | `StoreMetadata` (audit key) |
| `leak = "External"` | Leakage tracking | `RecordLeakage` |

`guard_capability` is the string boundary for choreography DSL input. The macro
parses it into a validated `CapabilityName` and rejects legacy, unnamespaced,
or invalid values at compile time. Outside the DSL boundary, first-party Rust
code should use typed capability families rather than hand-written strings.

See [Choreography Development Guide](803_choreography_guide.md) for annotation syntax and usage, including protocol artifact requirements, dynamic reconfiguration, protocol evolution compatibility policy, termination budgets, effect command generation, macro output contracts, and effect interpreter integration.

## 5. Guard Chain Integration

Choreography annotations compile to `EffectCommand` sequences that feed the same guard chain used at runtime send sites (CapGuard, FlowGuard, JournalCoupler, LeakageTracker). Annotation-derived effects execute first, then runtime guards validate and charge budgets before transport. Guard evaluation is synchronous over a prepared `GuardSnapshot` and yields `EffectCommand` items interpreted asynchronously.

See [Choreography Development Guide](803_choreography_guide.md) for guard chain integration patterns.

## 6. Execution Modes

Aura supports multiple execution environments for the same choreography definitions. Production execution uses admitted VM sessions with real effect handlers. Simulation execution uses deterministic time and fault injection. Test utilities may use narrower runner surfaces when that improves isolation.

Each environment preserves the same protocol structure and admission semantics where applicable. Choreography execution also captures conformance artifacts for native/WASM parity testing. See [Test Infrastructure Reference](118_testkit.md) for artifact surfaces and effect classification.

## 7. Example Protocols

Anti-entropy protocols synchronize CRDT state. They run as choreographies that exchange state deltas. Session types ensure that the exchange pattern follows causal and structural rules.

FROST ceremonies use choreographies to coordinate threshold signing. These ceremonies use the guard chain to enforce authorization rules.

Aura Consensus uses choreographic notation for fast path and fallback flows. Consensus choreographies define execute, witness, and commit messages. Session types ensure evidence propagation and correctness.

```rust
choreography! {
    #[namespace = "sync"]
    protocol AntiEntropy {
        roles: A, B;
        A -> B: Delta(data: Vec<u8>);
        B -> A: Ack(data: Vec<u8>);
    }
}
```

This anti-entropy example illustrates a minimal synchronization protocol.

## 8. Operation Categories and Choreography Use

Not all multi-party operations require full choreographic specification. Aura classifies operations into categories that determine when choreography is necessary.

### 8.1 When to Use Choreography

Full choreography (Category C) is required for operations where partial execution is dangerous and all parties must agree before effects apply -- such as establishing or modifying cryptographic relationships. Operations within established cryptographic contexts (Category A) use CRDT fact emission without choreography. Operations affecting other users' policies (Category B) may use lightweight proposal/approval patterns.

See [Choreography Development Guide](803_choreography_guide.md) for the decision framework. See [Consensus - Operation Categories](108_consensus.md#17-operation-categories) for detailed categorization.

## 9. Choreography Inventory

The codebase contains choreographic protocols spanning core consensus, rendezvous, authentication, recovery, invitation, sync, and runtime coordination -- approximately 15 protocols across 7 domains.

See [Project Structure](999_project_structure.md) for the protocol inventory with locations and purposes.

## 10. Runtime Infrastructure

The runtime provides production choreographic execution through manifest-driven Telltale protocol-machine sessions.

### 10.1 ChoreographicEffects Trait

| Method | Purpose |
|--------|---------|
| `send_to_role_bytes` | Send message to specific role |
| `receive_from_role_bytes` | Receive message from specific role |
| `broadcast_bytes` | Broadcast to all roles |
| `start_session` | Initialize choreography session |
| `end_session` | Terminate choreography session |

`AuraVmEffectHandler` is the synchronous host boundary between the protocol machine and Aura runtime services. `AuraQueuedVmBridgeHandler` provides queued outbound payloads and branch decisions for role-scoped protocol-machine sessions.

### 10.2 Wiring a Choreography

Wiring a choreography involves storing the protocol in a `.choreo` file, generating artifacts via `choreography!`, opening an admitted protocol-machine session, and providing decision sources through the host bridge.

See [Choreography Development Guide](803_choreography_guide.md) for the wiring procedure.

### 10.5 Output and Flow Policy Integration Points

Aura binds choreography execution to protocol-machine output/flow gates at the runtime boundary.

`AuraVmEffectHandler` tags protocol-machine-observable operations with output-condition predicate hints so `OutputConditionPolicy` can enforce commit visibility rules. The hardening profile allow-list admits only known predicates (transport send/recv, protocol choice/step, guard acquire/release). Unknown predicates are rejected in CI profiles.

Flow constraints are enforced with `FlowPolicy::PredicateExpr(...)` derived from Aura role/category constraints. This keeps pre-send flow checks aligned with Aura's information-flow contract while preserving deterministic replay behavior.

Practical integration points:

1. Choreography annotations declare intent (`guard_capability`, `flow_cost`, `journal_facts`, `leak`).
2. Macro output emits `EffectCommand` sequences.
3. Snapshot builders evaluate typed capability candidates into an admitted
   frontier, and the guard chain evaluates commands and budgets at send sites.
4. Protocol-machine output/flow policies gate observable commits and cross-role message flow before transport effects execute.

Choreography-level guard semantics and protocol-machine-level hardening are additive, not competing. Annotations define required effects. Policies constrain which effects are allowed to become observable.

## 11. Summary

Aura uses choreographic programming to define global protocols. Projection produces local session types. Session types enforce structured communication. Handlers execute protocol steps using effect traits. Extension effects provide authorization, budgeting, and journal updates. Execution modes support testing, simulation, and production. Choreographies define distributed coordination for CRDT sync, FROST signing, and consensus.

Not all multi-party operations need choreography. Operations within established cryptographic contexts use optimistic CRDT facts. Choreography is reserved for Category C operations where partial state would be dangerous.

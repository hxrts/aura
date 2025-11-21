## Aura Quint Specifications Overview

### Goals

- Provide executable Quint models for every protocol lifecycle built on `protocol-core`.
- Mirror the complete control flow that the Rust lifecycles drive: descriptor registration, inputs, transitions, side effects, evidence, completion/abort.
- Enable the simulator to run “Spec-in-the-loop” sessions: drive lifecycles with real inputs and verify invariants on each step.
- Support generation of counter-examples and reproducible traces for debugging.
- Serve as documentation of expected behaviour across modules such as DKD, resharing, recovery, locking, counter, group, sessions, SBB, and journal.

### Components

- `protocol_core.qnt`: shared runtime utilities mirroring `ProtocolLifecycle`, `ProtocolInput`, `ProtocolEffect`, timers, evidence, typestate transitions.
- Protocol lifecycles (`protocol_dkg.qnt`, `protocol_resharing.qnt`, `protocol_recovery.qnt`, `protocol_locking.qnt`, `protocol_counter.qnt`, `protocol_groups.qnt`, `protocol_sessions.qnt`, `protocol_sbb.qnt`, `protocol_journal.qnt`) capturing per-protocol state machines and effects.
- Harness targets (`harness_dkg.qnt`, `harness_resharing.qnt`, `harness_recovery.qnt`, `harness_locking.qnt`, `harness_counter.qnt`, `harness_groups.qnt`) compose `protocol_core` with a specific lifecycle and expose helper actions (`register`, `complete`, `abort`, etc.) so the simulator can replay actual lifecycle steps using a single module entry point.
- Bridge bindings (pending implementation) that translate Rust trace events into Quint action invocations.

### Design Objectives

1. **Trace Fidelity** – Every LocalSignal/transition emitted by Rust lifecycles must have an equivalent action. Specs should model successful and aborted completions, and capture the generated outputs/effects.
2. **Effect Validation** – For each protocol, ensure emitted `ProtocolEffect`s match spec-defined expectations (e.g., counter reservations must produce `UpdateCounter`, group operations emit `Trace`).
3. **Outcome Typing** – Define typed representations of results (commit payloads, reserved ranges) so specs verify final payloads.
4. **Environment Constraints** – Tie effect_api updates, timers, evidence rehydration, and counter increments to invariants preventing invalid state (e.g., nonce uniqueness, active timers for non-final states).
5. **Composable Harness** – Provide standard action entry points (`recordInput`, `advance`, `signalAbort`, etc.) to let the simulator forward real inputs to the specs.
6. **Executable Specification** – When plugged into the simulator, the modules should allow `quint run/typecheck/verify` to act as an end-to-end specification runtime for any protocol built on `protocol-core`.

# Aura Guards (Layer 4) - Architecture and Invariants

## Purpose
Provide the guard chain that enforces authorization, flow budgets, leakage budgets, and
journal coupling for every network-visible send. Guards are pure evaluators that return
EffectCommands; no guard performs I/O directly.

## Inputs
- GuardSnapshot prepared from effect system state.
- GuardChain configuration (capability requirement, flow cost, leakage policy, journal facts).
- EffectContext metadata (authority/context/session).

## Outputs
- EffectCommands describing the required side effects (budget charge, journal commit, etc.).
- GuardDecision with optional receipt metadata.

## Invariants
- Charge-before-send: no transport side effects without successful guard evaluation.
- Authorization precedes budgeting: CapGuard runs before FlowGuard.
- Journal coupling is atomic with budget charge.
- Leakage accounting is recorded as journal facts (RelationalFact::LeakageEvent).
- Guards are pure and deterministic given the snapshot.

## Ownership Model

- `aura-guards` is primarily `Pure` guard evaluation.
- Any effectful execution around guards should remain outside the pure decision
  core and should not turn guards into `ActorOwned` semantic owners.
- Capability requirements are first-class input here and must not be bypassed
  by higher-layer shortcuts.
- Guard execution outcomes should participate in typed terminal failure rather
  than hidden fail-open or silent blocking behavior.
- `Observed` consumers may inspect decisions and receipts but not redefine them.

### Detailed Specifications

### InvariantSentMessagesHaveFacts
No observable network behavior may occur before capability validation, flow budget charging, and journal coupling succeed.

Enforcement locus:
- `src/guards/pure.rs`: `GuardChain` defines pure decision order.
- `src/guards/executor.rs`: `GuardChainExecutor` applies effect commands before transport send.
- `src/guards/capability_guard.rs`: authorization checks run before budget logic.
- `src/guards/flow.rs` and `src/guards/journal.rs`: flow charge and journal coupling are linked.

Failure mode:
- Unauthorized packets reach the network.
- Budget exhaustion fails open.
- Journal and transport diverge on send accounting.

Verification hooks:
- `just check-arch`
- `cargo test -p aura-protocol guard_chain_invariant`
- `cargo test -p aura-simulator invariant_tests`

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) defines guard-mediated observability.
- [Privacy and Information Flow Contract](../../docs/003_information_flow_contract.md) defines charge-before-send and flow budget invariants.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines `InvariantSentMessagesHaveFacts` and `InvariantFlowBudgetNonNegative`.

## Boundaries
- No direct transport or storage calls inside guards.
- Effect execution happens via interpreters (production, simulation, test).
- Time/random access only via effect traits in interpreter layer.

## Core + Orchestrator Rule
- Pure guard evaluation logic belongs in core/pure modules.
- Effectful execution and I/O belong in orchestrator/executor modules.

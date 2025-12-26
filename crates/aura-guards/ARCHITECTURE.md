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

## Boundaries
- No direct transport or storage calls inside guards.
- Effect execution happens via interpreters (production, simulation, test).
- Time/random access only via effect traits in interpreter layer.

## Core + Orchestrator Rule
- Pure guard evaluation logic belongs in core/pure modules.
- Effectful execution and I/O belong in orchestrator/executor modules.

# Aura Consensus (Layer 4) - Architecture and Invariants

## Purpose
Provide the strong-agreement protocol for single-operation consensus. This is the only
strong-agreement mechanism in Aura; all other coordination is CRDT/monotone.

## Inputs
- Prestate commitment and proposal payload.
- Witness set and threshold parameters.
- Effect traits for crypto, time, transport (provided by orchestrator).

## Outputs
- CommitFact representing the agreed proposal.
- Witness evidence suitable for journal insertion by callers.

## Invariants
- Single-shot consensus: one proposal bound to a specific prestate.
- CommitFact implies threshold agreement over the proposal and prestate.
- No direct journal/storage mutation inside the pure core.
- Effectful orchestration is isolated from core state machine.

## Boundaries
- Core state machine lives in `consensus/core` and is effect-free.
- Orchestrator functions require explicit effect traits.
- Callers are responsible for guard chain enforcement and journal insertion.

## Core + Orchestrator Rule
- New protocol logic must have a pure core module and a thin effectful orchestrator.

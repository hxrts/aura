# Aura Protocol (Layer 4) - Architecture and Invariants

## Purpose
Coordinate multi-party protocols and guard-chain enforcement. This crate provides
orchestration glue, not single-party effect implementations.

## Inputs
- Effect trait implementations assembled by higher layers (agent/simulator).
- Choreographic annotations and session types.
- Journal and authorization facts from domain crates.

## Outputs
- Guarded transport operations and protocol outcomes.
- Orchestrated consensus and anti-entropy flows.

## Invariants
- No production effect implementations live in Layer 4.
- Guard chain is enforced on every send.
- Journal facts and budgets are coupled atomically before transport.

## Boundaries
- No runtime composition or lifecycle management (Layer 6 responsibility).
- No application-specific protocol logic (Layer 5 responsibility).

## Core + Orchestrator Rule
- Any new protocol logic should be split into pure core and effectful orchestrator modules.

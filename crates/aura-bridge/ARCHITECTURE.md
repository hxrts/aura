# Aura Bridge (Layer 4) - Architecture and Invariants

## Purpose
Bridge handler systems via typed or type-erased adapters without changing effect
trait definitions.

## Inputs
- Typed handler implementations or dynamic handler registries.
- Effect command/parameter payloads (serialized).

## Outputs
- Bridged handler calls with stable serialization formats.

## Invariants
- Production serialization uses bincode; JSON is debug-only.
- Only one production type-erasure stack is supported; legacy bridge is deprecated.

## Boundaries
- Bridges do not own runtime lifecycles.
- No protocol-specific business logic belongs here.

## Core + Orchestrator Rule
- Keep serialization/layout logic pure; orchestration lives in higher layers.

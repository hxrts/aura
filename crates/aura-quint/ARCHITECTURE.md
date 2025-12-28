# Aura Quint (Layer 8) - Architecture and Invariants

## Purpose
Native Rust interface to the Quint formal verification language using the Quint
Rust evaluator directly. Eliminates Node.js dependency while providing full
verification capabilities.

## Inputs
- Quint specifications (`.qnt` files parsed to JSON IR).
- Property specifications (`PropertySpec`: invariants, context, safety properties).
- `quint_evaluator` crate (native evaluator for JSON IR simulation).

## Outputs
- `VerificationResult` for property verification outcomes.
- `SimulationResult` from Quint simulation engine.
- `PropertySuite` for organizing verification properties.
- Parsed AST and IR for Aura protocol specifications.
- `QuintRunner` and `QuintEvaluator` for verification workflows.

## Invariants
- Hybrid architecture: TypeScript parser generates JSON IR; native Rust evaluator consumes it.
- Integrates with aura-core error system (`AuraError`, `AuraResult`).
- Used for protocol specification verification, not runtime.
- Re-exports `quint_evaluator` types for interop.

## Boundaries
- Quint specifications live in verification/quint/.
- Runtime simulation lives in aura-simulator.
- Protocol implementations live in feature crates.

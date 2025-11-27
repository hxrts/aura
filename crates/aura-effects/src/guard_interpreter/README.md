# Guard Effect Interpreter

This module provides the `ProductionEffectInterpreter` that executes effect commands produced by pure guard evaluation in the Aura system.

## Overview

Per ADR-014, guards in Aura are pure functions that:
1. Take an immutable `GuardSnapshot` containing pre-fetched state
2. Return a `GuardOutcome` with authorization decision and effect commands
3. Never perform I/O or blocking operations directly

The `ProductionEffectInterpreter` bridges this pure evaluation model to real-world effects by asynchronously executing the commands.

## Effect Commands

The interpreter handles these primitive effect commands:

- **ChargeBudget**: Deduct flow budget for spam/DoS protection
- **AppendJournal**: Persist facts to the journal
- **RecordLeakage**: Track privacy metadata leakage
- **StoreMetadata**: Save key-value pairs to storage
- **SendEnvelope**: Transmit messages over the network
- **GenerateNonce**: Create cryptographic nonces

## Usage

```rust
use aura_effects::ProductionEffectInterpreter;
use std::sync::Arc;

// Create interpreter with all required effect handlers
let interpreter = ProductionEffectInterpreter::new(
    Arc::new(journal_handler),
    Arc::new(flow_budget_handler),
    Arc::new(leakage_handler),
    Arc::new(storage_handler),
    Arc::new(network_handler),
    Arc::new(random_handler),
    authority_id,
);

// Execute effect commands from guard evaluation
for cmd in guard_outcome.effects {
    let result = interpreter.execute(cmd).await?;
    // Handle result...
}
```

## Architecture

The interpreter follows Aura's layered architecture:
- **Layer 1 (aura-core)**: Defines `EffectInterpreter` trait and `EffectCommand` types
- **Layer 3 (aura-effects)**: Implements production interpreter with real I/O
- **Layer 4+ (aura-protocol, etc.)**: Uses interpreter in guard chain execution

This separation enables:
- Algebraic effects and clean composition
- Deterministic testing with mock interpreters
- WASM compatibility (pure guards can run in WASM)
- Clear separation between business logic and I/O
# Aura Protocol Middleware Architecture

A composable middleware architecture for Aura's distributed protocols, built on choreographic programming principles and designed for threshold cryptography coordination.

## Overview

The `aura-protocol` crate provides the foundational infrastructure for Aura's distributed protocols, featuring a clean middleware system that enables composable cross-cutting concerns like tracing, effects injection, error recovery, and metrics collection. The architecture is designed around choreographic programming patterns where protocols are written from a global viewpoint and automatically projected to local device behavior.

## Architecture

### Core Components

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   Application   │    │   Middleware    │    │   Base Handler  │
│                 │    │                 │    │                 │
│ Protocol Logic  │◄──►│ • Tracing       │◄──►│ • InMemory      │
│                 │    │ • Effects       │    │ • Network       │
│                 │    │ • Metrics       │    │ • Simulation    │
│                 │    │ • Recovery      │    │                 │
└─────────────────┘    └─────────────────┘    └─────────────────┘
        │                       │                       │
        └───────────────────────┼───────────────────────┘
                                │
                    ┌─────────────────┐
                    │     Effects     │
                    │                 │
                    │ • Cryptographic │
                    │ • Time/Schedule │
                    │ • Error Handling│
                    └─────────────────┘
```

### Key Abstractions

- **`AuraProtocolHandler`**: Core trait defining the protocol handler interface
- **Middleware**: Composable components that wrap handlers to add functionality
- **Effects**: Algebraic effects system for side-effect isolation and testability
- **BaseContext**: Execution environment providing access to ledger, transport, and cryptographic operations
- **Protocol Types**: Core types for instructions, errors, and coordination

## Quick Start

```rust
use aura_protocol::prelude::*;

// Create a base handler
let handler = InMemoryHandler::new();

// Compose middleware stack
let handler = TracingMiddleware::new(handler, tracer);
let handler = EffectsMiddleware::with_test_effects(handler, device_id, "test");
let handler = MetricsMiddleware::new(handler, metrics_collector);

// Use the composed handler
handler.start_session(participants, "dkd".to_string(), metadata).await?;
```

## Modules

### `middleware/`
Composable middleware system for cross-cutting concerns:

- **`TracingMiddleware`**: Distributed tracing and observability
- **`EffectsMiddleware`**: Automatic effects injection for protocols
- **`MetricsMiddleware`**: Performance and usage metrics collection
- **`SessionMiddleware`**: Session lifecycle management
- **`ErrorRecoveryMiddleware`**: Automatic error recovery and retry logic
- **`CapabilityMiddleware`**: Authorization and capability checking
- **`InstrumentationMiddleware`**: Protocol instrumentation and debugging
- **`EventWatcherMiddleware`**: Event monitoring and reaction

### `handlers/`
Base protocol handler implementations:

- **`InMemoryHandler`**: In-memory handler for testing and development
- **`NetworkHandler`**: Network-based handler for production environments
- **`SimulationHandler`**: Deterministic simulation handler for testing

### `effects/`
Algebraic effects system for side-effect isolation:

- **Cryptographic Effects**: Event signing, verification, key operations
- **Time Effects**: Scheduling, timeouts, cooperative yielding
- **Error Effects**: Unified error handling across protocols

### `execution/`
Protocol execution infrastructure:

- **`BaseContext`**: Common execution environment for all protocols
- **`Transport`**: Network transport abstraction
- **Protocol types**: Instructions, errors, and coordination primitives

### `protocols/`
Protocol-specific utilities and implementations:

- **`lottery`**: Distributed lottery for lock acquisition
- **`rendezvous`**: Participant discovery and connection establishment
- **`common`**: Shared protocol patterns and utilities

### `types/`
Core protocol types and utilities:

- **`IdentifierMapping`**: Safe bidirectional mapping between ID types
- **Protocol coordination types**: Errors, instructions, filters

## Effects System

The effects system provides clean separation between pure protocol logic and side effects:

```rust
use aura_protocol::effects::*;

// Protocol functions accept effects as parameters
fn execute_protocol_phase(
    state: ProtocolState,
    effects: &impl ProtocolEffects,
) -> AuraResult<ProtocolState> {
    // Use effects for side operations
    let signature = effects.sign_event(&event)?;
    let current_time = effects.current_epoch();
    
    // Pure logic using effect results
    Ok(state.with_signature(signature).at_time(current_time))
}
```

### Effect Categories

- **`SigningEffects`**: Cryptographic operations (signing, verification)
- **`TimeEffects`**: Time and scheduling operations
- **`ProtocolEffects`**: Unified interface combining all effect types

### Effect Implementations

- **`AuraEffectsAdapter`**: Bridges to `aura_crypto::Effects` system
- **`CombinedEffects`**: Composes multiple effect providers
- **Test Effects**: Deterministic effects for testing

## Middleware Composition

Middleware components are designed to be composable and layered:

```rust
// Start with base handler
let handler = InMemoryHandler::new();

// Add essential middleware
let handler = EffectsMiddleware::with_production_effects(handler, device_id);
let handler = SessionMiddleware::new(handler);

// Add observability middleware
let handler = TracingMiddleware::new(handler, tracer);
let handler = MetricsMiddleware::new(handler, collector);

// Add reliability middleware
let handler = ErrorRecoveryMiddleware::new(handler, retry_config);

// Add security middleware
let handler = CapabilityMiddleware::new(handler, authorizer);
```

## Protocol Coordination

The crate provides utilities for choreographic protocol coordination:

```rust
use aura_protocol::protocols::*;

// Deterministic participant ordering
let ordered = deterministic_participant_order(&session_id, &participants);

// Coordinator selection
let coordinator_idx = select_coordinator(&session_id, "dkd", &participants);

// Threshold calculations
let byzantine_threshold = byzantine_threshold(participant_count);
let majority_threshold = majority_threshold(participant_count);

// Distributed lottery for lock acquisition
let ticket = compute_lottery_ticket(&device_id, &last_event_hash, &effects);
let winner = determine_lock_winner(&lock_requests)?;
```

## Error Handling

Unified error handling across the protocol system:

```rust
use aura_protocol::effects::{AuraError, AuraResult, ErrorCode, ErrorSeverity};

// Protocol operations return consistent error types
pub fn protocol_operation() -> AuraResult<ProtocolResult> {
    // Errors are automatically converted and handled uniformly
    Ok(ProtocolResult::Success)
}
```

## Testing and Simulation

The architecture supports comprehensive testing through:

- **Deterministic Effects**: Controlled time, randomness, and cryptographic operations
- **Simulation Handler**: Deterministic protocol execution for testing
- **Middleware Testing**: Individual middleware components can be tested in isolation
- **Integration Testing**: Full protocol stacks can be tested end-to-end

```rust
// Create deterministic test environment
let effects = Effects::deterministic(seed, initial_time);
let handler = SimulationHandler::new();
let handler = EffectsMiddleware::new(handler, effects);

// Run deterministic tests
test_protocol_execution(&handler).await;
```

## Configuration

### Feature Flags

- `simulation`: Enables simulation handler and deterministic testing
- `test-utils`: Enables additional testing utilities and fault injection

### Dependencies

- `aura-crypto`: Cryptographic primitives and effects
- `aura-journal`: CRDT ledger and event types
- `aura-types`: Core Aura types and error handling
- `aura-protocol-types`: Protocol-specific type definitions
- `rumpsteak`: Choreographic programming framework (future integration)

## Examples

### Basic Protocol Handler Setup

```rust
use aura_protocol::prelude::*;

#[tokio::main]
async fn main() -> AuraResult<()> {
    // Create handler with middleware stack
    let handler = InMemoryHandler::new();
    let handler = EffectsMiddleware::with_production_effects(handler, device_id);
    let handler = TracingMiddleware::new(handler, tracing_subscriber::registry());
    
    // Start protocol session
    let session_id = handler.start_session(
        participants,
        "threshold_signing".to_string(),
        metadata
    ).await?;
    
    // Execute protocol operations
    handler.send_message(peer_id, message).await?;
    let response = handler.receive_message(peer_id).await?;
    
    Ok(())
}
```

### Custom Middleware

```rust
use aura_protocol::middleware::*;

pub struct CustomMiddleware<H> {
    inner: H,
    custom_state: CustomState,
}

#[async_trait]
impl<H: AuraProtocolHandler> AuraProtocolHandler for CustomMiddleware<H> {
    type DeviceId = H::DeviceId;
    type SessionId = H::SessionId;
    type Message = H::Message;
    
    async fn send_message(&mut self, to: Self::DeviceId, msg: Self::Message) -> ProtocolResult<()> {
        // Add custom logic before delegation
        self.custom_logic(&msg).await?;
        
        // Delegate to inner handler
        self.inner.send_message(to, msg).await
    }
    
    // Implement other required methods...
}
```

## Integration with Aura

This crate integrates with the broader Aura ecosystem:

- **`aura-crypto`**: Provides cryptographic effects and primitives
- **`aura-journal`**: Supplies CRDT ledger and event handling
- **`aura-agent`**: Uses protocol handlers for device-level operations
- **`aura-coordination`**: Implements specific protocol choreographies

## License

This crate is part of the Aura project and follows the same licensing terms.
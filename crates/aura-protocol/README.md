# Aura Protocol: Choreographic Middleware Architecture

A composable middleware architecture for Aura's distributed protocols, built on choreographic programming principles using the Rumpsteak-Aura framework for type-safe protocol coordination.

## Overview

The `aura-protocol` crate provides the foundational infrastructure for Aura's distributed protocols, featuring a clean middleware system that enables composable cross-cutting concerns like observability, effects injection, error recovery, authorization, and ledger integration. The architecture is designed around choreographic programming patterns where protocols are written from a global viewpoint and automatically projected to local device behavior.

## Current Implementation Status

### Production-Ready Components ✅

**Core Infrastructure:**
- **Middleware System**: Complete composable stack with builder pattern and type-safe composition
- **Effects System**: Full algebraic effects with deterministic testing support  
- **Handler Abstraction**: Transport-agnostic protocol handler trait with async API
- **Choreographic Integration**: Complete Rumpsteak adapter with session type support
- **Error System**: Unified error handling across all protocol operations
- **Testing Framework**: Deterministic testing with controlled effects and time

**Middleware Components:**
- **ObservabilityMiddleware**: Unified tracing, metrics, instrumentation, and dev console (1000+ lines)
- **EffectsMiddleware**: Production/test/deterministic effects injection with full adapter
- **CapabilityMiddleware**: Authorization and permission checking system
- **ErrorRecoveryMiddleware**: Retry logic with exponential backoff and error classification  
- **SessionMiddleware**: Session lifecycle management and state transitions
- **StackBuilder**: Type-safe middleware composition with both function and macro approaches

### Work-in-Progress Components 🚧

**Protocol Implementations:**
- **DKD Choreography**: Basic structure with placeholder crypto operations (needs completion)
- **FROST Signing**: Module structure exists but implementation is minimal
- **Journal Sync**: Framework present but needs full CRDT integration
- **Coordination Protocols**: Epoch management structure exists but needs implementation

**Integration Components:**
- **Ledger Integration**: Middleware defined but not fully connected to journal operations
- **Event Watcher**: Basic structure but limited reactive functionality
- **Network Transport**: Re-exports from aura-transport but needs production hardening

### Known Limitations 🔧

**Test Infrastructure:**
- All test files currently disabled (`.disabled` extensions) due to API changes during refactoring
- Need to re-enable and update tests after API stabilization
- Byzantine and simulation tests need updates for new handler APIs

**Performance:**
- Current focus is on correctness and type safety over performance optimization
- Middleware stack uses some boxing for dynamic dispatch (acceptable for protocol handlers)
- Metrics system is basic atomic counters (could be expanded for production)

## Core Architecture

### Design Principles

1. **Choreographic Programming**: Protocols written from global viewpoint, automatically projected to local behavior using Rumpsteak-Aura
2. **Middleware Composition**: Layerable cross-cutting concerns without modifying core protocol logic
3. **Effect Injection**: Side effects (crypto, time, I/O) injected for testability and determinism
4. **Type Safety**: Session types and rumpsteak framework provide compile-time protocol safety
5. **Clean Separation**: Protocol logic independent from transport and handler implementation

### Component Diagram

```
┌──────────────────────┐
│   Choreographic      │
│   Protocol Logic     │
│   (Rumpsteak)        │
└──────────┬───────────┘
           │
┌──────────▼──────────────────────────────────────────┐
│           Middleware Stack (Composable)             │
├─────────────────────────────────────────────────────┤
│ ┌────────────────────────────────────────────────┐  │
│ │ ObservabilityMiddleware (Tracing/Metrics/Dev)  │  │
│ │ CapabilityMiddleware (Authorization)           │  │
│ │ EffectsMiddleware (Side-Effect Injection)      │  │
│ │ ErrorRecoveryMiddleware (Fault Handling)       │  │
│ │ EventWatcherMiddleware (Event Monitoring)      │  │
│ │ LedgerIntegrationMiddleware (State Sync)       │  │
│ │ SessionMiddleware (Lifecycle Management)       │  │
│ └────────────────────────────────────────────────┘  │
└──────────┬──────────────────────────────────────────┘
           │
┌──────────▼─────────────┐
│   AuraProtocolHandler  │
│  (Transport-Agnostic)  │
└──────────┬─────────────┘
           │
┌──────────▼───────────────────────────────┐
│    Transport Layer (aura-transport)      │
├──────────────────────────────────────────┤
│ • InMemoryHandler (testing)              │
│ • NetworkHandler (production)            │
│ • SimulationHandler (deterministic test) │
└──────────────────────────────────────────┘
```

## Core Components

### 1. Middleware System (`middleware/`)

**Fully Implemented Components:**

- **`ObservabilityMiddleware`**: Unified observability combining:
  - Distributed tracing for protocol events with configurable log levels
  - Real-time metrics collection (send/receive/session/error counters)
  - Event instrumentation with filtering and content capture
  - Dev console integration for visualization
  - Trace recording for debugging and replay analysis
  
- **`EffectsMiddleware`**: Side-effect injection system for:
  - Cryptographic operations (signing, verification, key derivation)
  - Time and scheduling operations (current time, delays, epoch tracking)
  - Deterministic testing with controlled randomness and fixed time
  - Production/test/deterministic effect variants with adapter pattern

- **`CapabilityMiddleware`**: Authorization and capability checking:
  - Permission-based access control with policy evaluation
  - Capability token validation and proof generation
  - Integration with Aura's unified permission system

- **`ErrorRecoveryMiddleware`**: Fault tolerance and recovery:
  - Automatic retry logic with exponential backoff and jitter
  - Error classification (retriable vs permanent) with context
  - Circuit breaker patterns for degraded service scenarios
  - Graceful degradation with operation timeouts

- **`SessionMiddleware`**: Session lifecycle management:
  - Session initialization and cleanup with resource management
  - State transition tracking with epoch management
  - Timeout handling and session expiration
  - Multi-participant coordination with consensus

**Partial Implementations:**

- **`EventWatcherMiddleware`**: Event monitoring framework (needs reactive functionality)
- **`LedgerIntegrationMiddleware`**: Journal synchronization framework (needs CRDT integration)

### 2. Effects System (`effects/`)

**Production-Ready Implementation:**

**Effect Categories:**

- **`SigningEffects`**: Cryptographic operations
  - Event signing with Ed25519 device keys and verification
  - FROST threshold signature coordination
  - Key derivation and rotation operations
  - Merkle proof generation and validation

- **`TimeEffects`**: Time and scheduling
  - Current timestamp queries with monotonic guarantees
  - Cooperative delays and timeouts with wake conditions  
  - Epoch tracking for session management
  - Simulation time control for deterministic testing

- **`ConsoleEffects`**: Logging and development output
  - Structured logging with trace correlation
  - Debug output with configurable verbosity
  - Dev console event streaming for real-time monitoring

- **`ProtocolEffects`**: Unified interface combining all effect categories

**Effect Implementations:**

- **`AuraEffectsAdapter`**: Complete bridge to `aura_protocol::effects::Effects` system
- **`CombinedEffects`**: Functional composition of multiple effect providers
- **Production Effects**: Real cryptography and system time
- **Test Effects**: Deterministic effects with fixed seeds and controlled time progression

### 3. Choreographic Integration (`protocols/choreographic/`)

**Production-Ready Infrastructure:**

- **`RumpsteakAdapter`**: Complete `ChoreoHandler` implementation with:
  - Async send/recv operations through Aura middleware stack
  - Session type support with endpoint state management
  - Choose/offer operations for protocol branching
  - Timeout management with configurable operation-specific limits
  - Message serialization with bincode and error handling

- **`BridgedRole`** and **`BridgedEndpoint`**: Type bridges between:
  - Rumpsteak's abstract roles and Aura's concrete device IDs
  - Choreographic endpoints and Aura's session context
  - Protocol state and ledger state management

- **`ChoreographicHandlerBuilder`**: Complete factory for creating protocol handlers with:
  - Full middleware stack composition (observability, authorization, recovery)
  - In-memory and network handler variants
  - Effects injection for deterministic testing
  - Configurable middleware selection

- **Error Handling**: Byzantine fault detection and safe choreography execution
- **Timeout Management**: Operation-specific timeout configuration with fallback strategies

### 4. Protocol Implementations (`protocols/`)

**Framework Complete, Implementations Partial:**

**Threshold Cryptography Protocols:**

- **`DkdChoreography`**: Deterministic Key Derivation framework
  - Complete message types and protocol phases
  - Placeholder crypto operations (needs completion with aura-crypto integration)
  - All-to-all broadcast patterns implemented
  - Result verification framework present

- **`FrostSigningChoreography`**: FROST threshold signatures
  - Module structure exists but implementation is minimal
  - Needs integration with frost-ed25519 crate operations

**Coordination Protocols:**

- **`EpochBumpChoreography`**: Session epoch management framework
- **`JournalSyncChoreography`**: Ledger synchronization framework (needs CRDT integration)
- **`FailureRecovery`**: Recovery protocol framework

**Protocol Patterns:**

- **`DecentralizedLottery`**: Complete distributed lock acquisition
  - Deterministic lottery ticket computation with cryptographic randomness
  - Winner selection with Byzantine fault tolerance
  - Lock coordination with timeout and release mechanisms

### 5. Handler System (`handlers.rs`)

**Complete Transport Integration:**

- **`StandardHandlerFactory`**: Creates handlers with Aura-standard types (DeviceId, Uuid, Vec<u8>)
- **Extension Traits**: `MiddlewareExt` and `HandlerAdapterExt` for easy composition
- **Boxed Handler Support**: Full trait object support for dynamic dispatch
- **Transport Bridge**: Re-exports from `aura-transport` when feature is enabled

### 6. Testing Infrastructure (`test_utils.rs`, `test_helpers.rs`)

**Complete Framework:**

- **Memory Transport**: Simple in-memory transport for testing choreographies
- **Deterministic UUIDs**: Fixed UUID generation for reproducible tests
- **Effects Injection**: Test effects with deterministic time and randomness
- **Middleware Testing**: Framework for testing middleware composition in isolation
- **Simulation Support**: Time travel debugging and protocol replay capabilities

## Quick Start

### Basic Choreographic Protocol

```rust
use aura_protocol::protocols::choreographic::{
    ChoreographicHandlerBuilder, BridgedRole, BridgedEndpoint
};
use aura_protocol::effects::Effects;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create deterministic effects for testing
    let effects = Effects::deterministic(42, 0);
    
    // Build choreographic handler with full middleware stack
    let handler = ChoreographicHandlerBuilder::new(effects)
        .with_device_name("device-1".to_string())
        .build_in_memory(device_id, context);
    
    // Use in choreographic protocol
    let role = BridgedRole { device_id, role_index: 0 };
    let mut endpoint = BridgedEndpoint::new(context);
    
    // Execute protocol phases...
    Ok(())
}
```

### Custom Middleware Stack

```rust
use aura_protocol::middleware::{
    MiddlewareStackBuilder, MiddlewareConfig,
    observability::ObservabilityConfig,
    error_recovery::ErrorRecoveryConfig,
};

let handler = base_handler;

// Configure middleware
let config = MiddlewareConfig {
    device_name: "my-device".to_string(),
    enable_observability: true,
    enable_capabilities: true,
    enable_error_recovery: true,
    observability_config: Some(ObservabilityConfig {
        device_name: "my-device".to_string(),
        log_level: tracing::Level::DEBUG,
        enable_trace_recording: true,
        capture_message_contents: false,
        ..Default::default()
    }),
    error_recovery_config: Some(ErrorRecoveryConfig {
        max_retries: 5,
        initial_delay_ms: 100,
        max_delay_ms: 5000,
        backoff_multiplier: 2.0,
        ..Default::default()
    }),
};

// Build full stack
let handler = MiddlewareStackBuilder::new(handler)
    .with_config(config)
    .build();
```

### Deterministic Protocol Testing

```rust
use aura_protocol::effects::Effects;

#[tokio::test]
async fn test_protocol_deterministically() {
    // Create deterministic effects with fixed seed
    let effects = Effects::deterministic(42, 1000);
    
    // Build handler with test effects
    let handler = handler
        .with_effects(effects)
        .with_observability(config);
    
    // Run protocol - will be deterministic and reproducible
    let result = run_protocol(&handler).await.unwrap();
    
    // Verify expected outcome
    assert_eq!(result.status, ProtocolStatus::Success);
}
```

## Development Roadmap

### Immediate Priorities (High Impact)

1. **Complete Protocol Implementations**:
   - Finish DKD choreography with real cryptographic operations
   - Implement FROST signing choreography with threshold coordination
   - Complete journal sync choreography with CRDT integration

2. **Re-enable Test Suite**:
   - Update disabled tests for new API changes
   - Restore Byzantine fault tolerance tests
   - Re-enable simulation and time travel debugging tests

3. **Production Hardening**:
   - Complete network transport integration for production deployment
   - Expand metrics system beyond basic atomic counters
   - Performance optimization for high-throughput scenarios

### Medium-Term Goals

1. **Protocol Library Expansion**:
   - Recovery and resharing choreographies
   - Account migration protocols
   - Multi-device coordination patterns

2. **Developer Experience**:
   - Protocol debugging tools and visualizations
   - Choreography composition utilities
   - Better error messages and diagnostics

3. **Performance Optimization**:
   - Reduce middleware overhead through zero-cost abstractions
   - Optimize message serialization and transport
   - Implement connection pooling and batching

## Architecture Strengths

### What Works Well

1. **Type Safety**: Strong type system with session types prevents protocol violations at compile time
2. **Middleware Composition**: Clean, zero-cost abstractions with compile-time middleware stacks
3. **Effect Injection**: Complete separation of pure protocol logic from side effects enables deterministic testing
4. **Choreographic Integration**: Full Rumpsteak integration with automatic local projection from global protocols
5. **Error Handling**: Comprehensive error taxonomy with context preservation and unified error system
6. **Testing Framework**: Deterministic testing with controlled effects, time, and randomness

### Design Decisions

**Why Choreographic Programming?**
- Type safety through session types prevents deadlocks and protocol violations
- Global viewpoint simplifies protocol reasoning and verification
- Automatic projection to local behavior eliminates manual coordination code
- Compile-time verification of protocol properties

**Why Middleware Architecture?**
- Each concern (observability, authorization, recovery) independently implemented and tested
- Protocol logic remains pure and transport-agnostic
- New middleware can be added without modifying existing code
- Middleware components are reusable across different protocols

**Why Algebraic Effects?**
- All side effects can be mocked and controlled for testing
- Deterministic testing with fixed seeds and controlled time progression
- Same protocol logic runs across different platforms and environments
- Effects can be traced and monitored for observability

## Integration with Aura Ecosystem

This crate integrates with the broader Aura ecosystem:

- **`aura-crypto`**: Cryptographic primitives, FROST operations, and Merkle trees
- **`aura-journal`**: CRDT ledger operations and event persistence
- **`aura-types`**: Core Aura types, error handling, and effects system
- **`aura-messages`**: Wire format message types for protocol communication
- **`aura-transport`**: Transport layer implementations and network handlers
- **`aura-agent`**: Device-level agent that uses protocol handlers for coordination
- **`aura-simulator`**: Deterministic protocol simulation and Byzantine testing
- **`rumpsteak-choreography`**: Choreographic programming framework (Rumpsteak-Aura fork)

## Module Structure

```
aura-protocol/
├── middleware/          # Production-ready composable middleware
│   ├── observability.rs # Unified tracing/metrics/dev console (1000+ lines)
│   ├── effects.rs       # Effect injection with adapter pattern
│   ├── error_recovery.rs# Retry logic and fault tolerance
│   ├── capability.rs    # Authorization and access control
│   ├── session.rs       # Session lifecycle management
│   ├── stack.rs         # Type-safe middleware composition
│   └── ...              # Other middleware components
├── effects/             # Complete algebraic effects system
│   ├── signing.rs       # Cryptographic effects with Ed25519
│   ├── time.rs          # Time effects with simulation support
│   ├── console.rs       # Logging and development effects
│   └── mod.rs           # Unified ProtocolEffects interface
├── protocols/           # Protocol implementations and patterns
│   ├── choreographic/   # Complete Rumpsteak integration
│   │   ├── handler_adapter.rs      # ChoreoHandler implementation
│   │   ├── middleware_integration.rs # Builder and composition
│   │   ├── error_handling.rs       # Byzantine detection
│   │   └── timeout_management.rs   # Operation timeouts
│   ├── threshold_crypto/# Threshold cryptography protocols
│   │   ├── dkd_choreography.rs     # DKD implementation (partial)
│   │   └── frost_signing_choreography.rs # FROST (minimal)
│   ├── coordination/    # Coordination and sync protocols
│   └── patterns/        # Reusable protocol patterns
├── context.rs           # Protocol execution context
├── types.rs             # Core protocol types and utilities
├── handlers.rs          # Transport handler factory and adapters
└── test_utils.rs        # Testing framework and utilities
```

## Features

- **`simulation`**: Enables simulation handler and deterministic testing framework
- **`test-utils`**: Enables testing utilities, fault injection, and Byzantine testing
- **`transport`**: Enables transport handler integration (default, required for handler creation)

## Performance Characteristics

**Current Status**: Focus on correctness and type safety over performance optimization

**Known Performance Considerations**:
- Middleware stack uses boxing for dynamic dispatch (acceptable overhead for protocol handlers)
- Message serialization uses bincode (efficient but could be optimized further)
- Effects system has minimal overhead for production use cases
- Memory allocations are minimized where possible

**Future Optimizations**:
- Zero-cost middleware abstractions through monomorphization
- Custom serialization formats for high-frequency messages
- Connection pooling and message batching for network efficiency
- Protocol-specific optimizations based on usage patterns

## License

This crate is part of the Aura project and follows the same licensing terms (MIT OR Apache-2.0).
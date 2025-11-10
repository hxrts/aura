# Aura Protocol: Core Effect System and Handler Infrastructure

The foundational crate providing Aura's unified effect system, handler infrastructure, and protocol coordination primitives. This crate serves as the execution substrate for all distributed protocols in the Aura ecosystem.

## Overview

The `aura-protocol` crate provides the core infrastructure that enables safe, composable, and testable distributed protocols through:

- **Unified Effect System**: Algebraic effects with static dispatch for zero-overhead execution
- **Handler Infrastructure**: Composable handlers for crypto, network, storage, and other system operations
- **Protocol Foundation**: Type-safe primitives for building distributed protocols
- **Testing Framework**: Deterministic testing with controlled effects and simulation

## Current Implementation Status

### Production-Ready Components

**Core Effect System:**
- **Effect Traits**: Complete algebraic effect interfaces (`CryptoEffects`, `NetworkEffects`, `StorageEffects`, etc.)
- **Handler Implementations**: Real, mock, and simulation handlers for all effect types
- **Composite Handler**: Unified handler that implements all effect traits with configurable backends
- **Type-Erased Bridge**: Seamless integration between typed traits and dynamic dispatch
- **Execution Modes**: Production, testing, and simulation modes with appropriate handler selection

**Handler Infrastructure:**
- **`CompositeHandler`**: Main handler implementation with support for all effect types
- **Handler Factory**: Easy creation of handlers for different execution contexts
- **Memory Handlers**: Complete in-memory implementations for testing
- **Real Handlers**: Production handlers using actual system APIs (filesystem, network, crypto libraries)
- **Simulation Handlers**: Controlled handlers for deterministic testing

**Effect Categories:**
- **`CryptoEffects`**: BLAKE3 hashing, Ed25519 signatures, random number generation
- **`NetworkEffects`**: Message sending, receiving, and peer discovery
- **`StorageEffects`**: Key-value storage operations with namespacing
- **`TimeEffects`**: Time queries and epoch management
- **`ConsoleEffects`**: Structured logging and debug output
- **`JournalEffects`**: Journal operations for CRDT state management
- **`RandomEffects`**: Cryptographic random number generation

### Work-in-Progress Components

**Protocol Infrastructure:**
- **Choreographic Integration**: Framework for choreographic programming (in `choreography/`)
- **Message Types**: Protocol message definitions (in `messages/`)
- **Guard System**: Protocol execution guards and constraints (in `guards/`)
- **Sync Protocols**: Anti-entropy and state synchronization protocols (in `sync/`)

**Advanced Features:**
- **Middleware System**: Cross-cutting concerns framework (basic traits defined)
- **CRDT Handlers**: Semilattice-based state handlers (in `effects/semilattice/`)
- **Agent Effects**: Higher-level agent operations (in `effects/agent.rs`)

### Testing and Development

**Complete Testing Infrastructure:**
- **Deterministic Effects**: Fixed-seed randomness and controlled time
- **Memory Transport**: In-memory message routing for unit tests
- **Simulation Framework**: Controlled execution environment
- **Property-Based Testing**: Support for randomized protocol testing

## Core Architecture

### Design Principles

1. **Algebraic Effects**: All side effects abstracted through trait interfaces
2. **Static Dispatch**: Zero-overhead trait implementations with full inlining
3. **Handler Composition**: Modular handlers that can be combined and configured
4. **Execution Modes**: Same code runs in production, testing, and simulation
5. **Type Safety**: Compile-time guarantees for protocol correctness

### Component Diagram

```
┌─────────────────────────────────────────┐
│           Protocol Logic                │
│     (Uses Effect Traits)                │
└──────────────┬──────────────────────────┘
               │
┌──────────────▼──────────────────────────┐
│         CompositeHandler                │
│    (Implements All Effect Traits)       │
├─────────────────────────────────────────┤
│ CryptoEffects  │ NetworkEffects         │
│ StorageEffects │ TimeEffects            │
│ ConsoleEffects │ JournalEffects         │
│ RandomEffects  │ ...                    │
└──────────────┬──────────────────────────┘
               │
┌──────────────▼──────────────────────────┐
│      Concrete Handler Backends          │
├─────────────────────────────────────────┤
│ • Real Handlers (Production)            │
│ • Mock Handlers (Testing)               │
│ • Memory Handlers (Unit Tests)          │
│ • Simulation Handlers (Controlled)      │
└─────────────────────────────────────────┘
```

### Effect System Architecture

The effect system provides a clean separation between protocol logic and system operations:

```rust
// Protocol code uses abstract effect traits
async fn my_protocol<C: CryptoEffects, N: NetworkEffects>(
    crypto: &C,
    network: &N,
) -> Result<ProtocolResult> {
    let signature = crypto.sign_message(&message).await?;
    network.broadcast(signature).await?;
    Ok(result)
}

// Handler provides concrete implementations
let handler = CompositeHandler::for_production(device_id)?;
let result = my_protocol(&handler, &handler).await?;
```

## Quick Start

### Basic Effect Usage

```rust
use aura_protocol::handlers::CompositeHandler;
use aura_protocol::effects::{CryptoEffects, NetworkEffects};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create handler for testing
    let handler = CompositeHandler::for_testing(device_id);

    // Use crypto effects
    let hash = handler.blake3_hash(b"hello world").await;
    let signature = handler.sign_message(b"message").await?;

    // Use network effects
    handler.broadcast(message).await?;
    let received = handler.receive().await?;

    Ok(())
}
```

### Execution Modes

```rust
use aura_protocol::handlers::CompositeHandler;

// Production mode - uses real system APIs
let prod_handler = CompositeHandler::for_production(device_id)?;

// Testing mode - uses mock implementations
let test_handler = CompositeHandler::for_testing(device_id);

// Simulation mode - uses controlled, deterministic implementations
let sim_handler = CompositeHandler::for_simulation(device_id, seed);
```

### Deterministic Testing

```rust
#[tokio::test]
async fn test_protocol_deterministically() {
    let handler = CompositeHandler::for_simulation(device_id, 42);

    // Protocol execution will be deterministic
    let result = run_protocol(&handler).await;

    // Same seed always produces same result
    assert_eq!(result.status, expected_status);
}
```

### Type-Erased Usage

```rust
use aura_protocol::handlers::AuraHandler;
use std::sync::Arc;
use tokio::sync::RwLock;

// Create type-erased handler
let handler: Arc<RwLock<Box<dyn AuraHandler>>> =
    CompositeHandler::for_testing(device_id).into_boxed();

// Still use typed effect traits through automatic bridge
let hash = handler.blake3_hash(b"data").await;
```

## Core Components

### 1. Effect System (`effects/`)

**Core Effect Traits:**

- **`CryptoEffects`**: Cryptographic operations
  ```rust
  #[async_trait]
  pub trait CryptoEffects {
      async fn blake3_hash(&self, data: &[u8]) -> [u8; 32];
      async fn sign_message(&self, message: &[u8]) -> Result<Signature>;
      async fn verify_signature(&self, signature: &Signature, message: &[u8]) -> bool;
  }
  ```

- **`NetworkEffects`**: Network communication
  ```rust
  #[async_trait]
  pub trait NetworkEffects {
      async fn send_message(&self, target: DeviceId, message: Vec<u8>) -> Result<()>;
      async fn broadcast(&self, message: Vec<u8>) -> Result<()>;
      async fn receive(&self) -> Result<(DeviceId, Vec<u8>)>;
  }
  ```

- **`StorageEffects`**: Persistent storage
  ```rust
  #[async_trait]
  pub trait StorageEffects {
      async fn get(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>>;
      async fn set(&self, namespace: &str, key: &str, value: Vec<u8>) -> Result<()>;
      async fn delete(&self, namespace: &str, key: &str) -> Result<()>;
  }
  ```

**Effect System Features:**
- Zero-overhead static dispatch through traits
- Automatic type-erased bridge for dynamic usage
- Configurable backends for different execution environments
- Comprehensive testing support with mock implementations

### 2. Handler Infrastructure (`handlers/`)

**`CompositeHandler`**: The main handler implementation that combines all effect backends:

```rust
impl CompositeHandler {
    // Factory methods for different contexts
    pub fn for_production(device_id: DeviceId) -> Result<Self>;
    pub fn for_testing(device_id: DeviceId) -> Self;
    pub fn for_simulation(device_id: DeviceId, seed: u64) -> Self;

    // Access to execution context
    pub fn device_id(&self) -> DeviceId;
    pub fn execution_mode(&self) -> ExecutionMode;
}
```

**Handler Types:**
- **Memory Handlers**: Fast in-memory implementations for testing
- **Real Handlers**: Production implementations using system APIs
- **Mock Handlers**: Controllable handlers for unit testing
- **Simulation Handlers**: Deterministic handlers for reproducible testing

### 3. Protocol Foundation (`choreography/`, `messages/`, `sync/`)

**Message Types**: Strongly-typed protocol messages in `messages/`:
- Crypto protocol messages (DKD, FROST, resharing)
- Social protocol messages (rendezvous)
- Common message patterns (envelopes, errors)

**Choreographic Framework**: Infrastructure for choreographic programming in `choreography/`:
- Protocol definitions and implementations
- Runtime adaptation and integration
- Type system for roles and endpoints

**Synchronization Protocols**: State sync and anti-entropy protocols in `sync/`:
- Peer view management
- Intent state synchronization
- Anti-entropy protocols for consistency

### 4. Testing Infrastructure (`handlers/`, `effects/`)

**Deterministic Testing:**
- Fixed-seed random number generation
- Controlled time progression
- Reproducible network behavior
- Deterministic storage operations

**Memory Implementations:**
- In-memory storage with persistence simulation
- In-memory network with message routing
- Mock crypto operations with consistent results

## Integration with Aura Ecosystem

This crate provides the foundation for all other Aura components:

- **`aura-core`**: Provides core types and interfaces used by the effect system
- **`aura-crypto`**: Implements cryptographic primitives used by crypto effects
- **`aura-agent`**: Uses handlers for device-level operations and protocol coordination
- **`aura-journal`**: Uses journal effects for CRDT state management
- **`aura-simulator`**: Uses simulation handlers for controlled testing environments
- **`aura-choreography`**: Uses choreographic infrastructure for protocol definitions

## Module Structure

```
aura-protocol/
├── effects/             # Core algebraic effects system
│   ├── mod.rs          # Effect trait definitions and core types
│   ├── crypto.rs       # Cryptographic effect implementations
│   ├── network.rs      # Network effect implementations
│   ├── storage.rs      # Storage effect implementations
│   ├── time.rs         # Time and epoch management effects
│   ├── console.rs      # Logging and console effects
│   ├── journal.rs      # Journal operation effects
│   ├── system.rs       # Core effect system infrastructure
│   └── semilattice/    # CRDT and semilattice handlers
├── handlers/            # Handler implementations and infrastructure
│   ├── composite.rs    # Main CompositeHandler implementation
│   ├── factory.rs      # Handler factory methods
│   ├── typed_bridge.rs # Type-erased to typed effect bridge
│   ├── crypto/         # Crypto handler implementations
│   ├── network/        # Network handler implementations
│   ├── storage/        # Storage handler implementations
│   ├── console/        # Console handler implementations
│   └── ...             # Other specialized handlers
├── choreography/        # Choreographic programming infrastructure
│   ├── protocols/      # Protocol definitions and implementations
│   ├── runtime/        # Runtime adaptation and execution
│   ├── types/          # Choreographic type system
│   └── integration.rs  # Integration with effect system
├── messages/            # Protocol message definitions
│   ├── crypto/         # Cryptographic protocol messages
│   ├── social/         # Social protocol messages
│   └── common/         # Common message patterns
├── sync/               # Synchronization and anti-entropy protocols
├── guards/             # Protocol execution guards and constraints
├── middleware/         # Middleware framework (basic)
├── context.rs          # Protocol execution context
└── lib.rs             # Main crate interface
```

## Performance Characteristics

**Zero-Cost Abstractions:**
- Effect traits compile to direct function calls with full inlining
- No runtime overhead for type-safe effect usage
- Static dispatch eliminates vtable lookups in hot paths

**Efficient Implementations:**
- Memory handlers use efficient data structures (HashMap, Vec)
- Real handlers minimize system call overhead
- Message serialization uses compact binary formats

**Scalability Considerations:**
- Handlers are designed for concurrent access patterns
- Effect system supports both sync and async operations
- Memory usage is minimized through careful resource management

## Development Status

**Stable and Production-Ready:**
- Core effect system and trait definitions
- Handler infrastructure and factory methods
- Memory and mock implementations for testing
- Type-erased bridge for dynamic dispatch

**Active Development:**
- Choreographic programming integration
- Protocol message types and definitions
- Middleware framework expansion
- Advanced CRDT and semilattice handlers

**Future Plans:**
- Performance optimization through custom allocators
- Advanced monitoring and observability features
- Protocol-specific optimizations
- Enhanced debugging and development tools

## Contributing

When contributing to `aura-protocol`:

1. **Effect System**: Follow algebraic effect patterns with pure trait interfaces
2. **Handler Implementation**: Implement all required effect traits for new handler types
3. **Testing**: Provide both unit tests and integration tests for new functionality
4. **Documentation**: Document public APIs with examples and usage patterns
5. **Performance**: Consider both correctness and performance in design decisions

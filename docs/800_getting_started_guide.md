# Getting Started Guide

Aura enables building distributed applications using threshold cryptography and CRDT-based state management. This guide provides the essential steps to set up your development environment and build your first application.

The guide covers prerequisites, core concepts, initial application development, and basic testing patterns. You will learn how Aura's effect system separates application logic from infrastructure concerns.

See [Effect System Guide](801_effect_system_guide.md) for advanced handler patterns. See [CRDT Programming Guide](802_crdt_programming_guide.md) for state management details.

---

## Prerequisites

**Software:**
- Nix with flakes enabled
- Rust 1.75+

**Knowledge:**
- Rust programming and async/await patterns
- Distributed systems fundamentals
- Effect systems and algebraic data types

## Core Concepts

**Algebraic Effects** separate application logic from infrastructure implementation. For complete details on Aura's effect system architecture, see [System Architecture](002_system_architecture.md#1-unified-effect-system-architecture). For practical implementation patterns, see [Effect System Guide](801_effect_system_guide.md).

The effect system enables testing with mock handlers and production deployment with real infrastructure handlers while keeping application code identical across environments.

**CRDT-Based State** enables multiple devices to update data simultaneously without conflicts. For theoretical foundations, see [Theoretical Model](001_theoretical_model.md). For implementation patterns, see [CRDT Programming Guide](802_crdt_programming_guide.md).

CRDT properties ensure state converges to the same value on all devices. Operations can execute in any order and produce consistent results.

**Threshold Cryptography** distributes security across multiple devices using secret sharing. For detailed implementation patterns, see [Protocol Development Guide](803_protocol_development_guide.md).

Guardian devices must coordinate to produce valid signatures. The threshold configuration determines how many guardians must participate for operations to succeed.

## Development Environment Setup

**Enter Development Shell:**
```bash
nix develop
```

The Nix shell provides all required development tools including Rust toolchain, formatters, and testing utilities. This command must be run from the project root directory.

**Verify Environment:**
```bash
just build
just check
just test
```

These commands validate that the development environment is configured correctly. Build errors indicate missing dependencies or configuration issues.

**Available Development Commands:**
- `just fmt` formats all code according to project standards
- `just clippy` runs linting with warnings treated as errors
- `just docs` generates API documentation for all crates
- `just watch` rebuilds automatically when files change

The `just` command runner provides consistent build tasks across different operating systems. All project documentation assumes you are running within the Nix development shell.

## Application Dependencies

When building applications on Aura, choose dependencies based on your architecture layer:

**For Basic Applications** (runtime composition):
```toml
[dependencies]
aura-core = { path = "../aura-core" }      # Effect traits and types
aura-agent = { path = "../aura-agent" }    # Complete runtime
```

**For Effect System Libraries** (handler implementations):
```toml  
[dependencies]
aura-core = { path = "../aura-core" }      # Effect traits
aura-effects = { path = "../aura-effects" } # Standard implementations
```

**For Protocol Coordination** (distributed protocols):
```toml
[dependencies]
aura-core = { path = "../aura-core" }      # Effect traits
aura-effects = { path = "../aura-effects" } # Handler implementations  
aura-protocol = { path = "../aura-protocol" } # Coordination primitives
aura-mpst = { path = "../aura-mpst" }      # Choreographic DSL
```

**Never depend on:**
- `aura-protocol` just to get basic effect handlers (use `aura-effects`)
- Multiple crates for the same functionality (causes conflicts)

## First Application

**Create Application Structure:**
```rust
// Import foundational types from aura-core
use aura_core::{AccountId, DeviceId};
use aura_core::effects::JournalEffects;

// Import runtime composition from aura-agent  
use aura_agent::AuraAgent;

pub struct CounterApp {
    device_id: DeviceId,
    account_id: AccountId,
    agent: AuraAgent,
}

impl CounterApp {
    pub fn new(device_id: DeviceId, account_id: AccountId, agent: AuraAgent) -> Self {
        Self { device_id, account_id, agent }
    }
}
```

The application structure separates business logic from runtime composition. `AuraAgent` provides a complete runtime that assembles handlers and protocols. For complete effect system documentation, see [Effect System API](500_effect_system_api.md).

**Implement Core Logic:**
```rust
impl CounterApp {
    pub async fn increment(&self, amount: i64) -> Result<(), AppError> {
        // Use the agent's journal effects
        let entry = Entry::counter_increment(self.device_id, amount);

        self.agent
            .journal_write_entry(entry)
            .await
            .map_err(AppError::Journal)?;

        Ok(())
    }

    pub async fn get_current_value(&self) -> Result<i64, AppError> {
        let query = Query::all_counter_entries(self.account_id);
        let entries = self.agent
            .journal_read_entries(query)
            .await
            .map_err(AppError::Journal)?;

        let total = entries
            .iter()
            .filter_map(|entry| entry.counter_amount())
            .sum();

        Ok(total)
    }
}
```

Application methods use the agent's composed effect system to interact with the journal. The `AuraAgent` provides a complete runtime while hiding handler composition complexity. CRDT properties ensure counter values converge correctly across devices.

## Testing Patterns

**Unit Tests with Testing Runtime:**
```rust
#[tokio::test]
async fn test_counter_increment() {
    let device_id = DeviceId::new();
    let account_id = AccountId::new();
    
    // Use testing agent with deterministic handlers from aura-effects
    let agent = AuraAgent::for_testing(device_id)?;

    let app = CounterApp::new(device_id, account_id, agent);

    app.increment(5).await.unwrap();
    let value = app.get_current_value().await.unwrap();

    assert_eq!(value, 5);
}
```

Test effect systems provide deterministic behavior for unit tests. Mock handlers return predictable results without external dependencies.

**Integration Tests with Real Effects:**
```rust
#[tokio::test]
async fn test_cross_device_synchronization() {
    let account_id = AccountId::new();

    let device1 = DeviceId::new();
    let effects1 = AuraEffectSystem::for_integration_testing(device1);
    let app1 = CounterApp::new(device1, account_id, effects1);

    let device2 = DeviceId::new();
    let effects2 = AuraEffectSystem::for_integration_testing(device2);
    let app2 = CounterApp::new(device2, account_id, effects2);

    app1.increment(3).await.unwrap();
    app2.increment(7).await.unwrap();

    // Allow CRDT synchronization
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert_eq!(app1.get_current_value().await.unwrap(), 10);
    assert_eq!(app2.get_current_value().await.unwrap(), 10);
}
```

Integration tests validate CRDT synchronization between multiple devices. These tests use real journal handlers with temporary storage.

## Next Steps

**Learn Advanced Patterns:**
- Read [Effect System Guide](801_effect_system_guide.md) for custom effect implementation
- Read [CRDT Programming Guide](802_crdt_programming_guide.md) for complex state management
- Read [Protocol Development Guide](803_protocol_development_guide.md) for distributed coordination

**Build Real Applications:**
- Implement multi-user collaboration features using threshold signatures
- Add real-time synchronization using CRDT merge operations
- Deploy applications using the patterns in [Deployment Guide](804_deployment_guide.md)

The effect system architecture enables rapid iteration and testing. CRDT-based state provides natural distribution without complex synchronization protocols.

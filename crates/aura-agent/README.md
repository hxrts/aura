# Aura Agent

Device-side identity management with effect-based runtime composition.

## Architecture

The `aura-agent` crate follows the unified effect system architecture by **composing handlers into a runtime**. It does not define effects or implement core effect handlers - instead, it composes existing effect handlers from `aura-protocol` into agent-specific runtimes.

### Core Concept

```rust
// The agent composes handlers to build a runtime
let agent = AuraAgent::new()
    .with_storage_handler(secure_storage_handler)
    .with_auth_handler(biometric_auth_handler) 
    .with_session_handler(threshold_session_handler)
    .with_middleware(metrics_middleware)
    .with_middleware(validation_middleware)
    .build();

// The runtime executes agent operations through the composed handlers
agent.authenticate_device().await?;
agent.store_credential(key, value).await?;
agent.start_recovery_session().await?;
```

### Module Organization

```
src/
├── agent.rs         # AuraAgent - composes handlers into runtime
├── config.rs        # Configuration for runtime composition
├── errors.rs        # Agent-specific error types
├── effects/         # Agent-specific effect trait definitions
│   ├── agent.rs     # High-level agent effect traits
│   └── mod.rs       # Effect trait exports
├── handlers/        # Handler implementations for agent effects
│   ├── auth.rs      # AuthenticationHandler implementation
│   ├── journal.rs   # JournalHandler implementation  
│   ├── sessions.rs  # SessionHandler implementation
│   └── storage.rs   # StorageHandler implementation
└── middleware/      # Middleware that wraps handlers
    ├── metrics.rs   # Metrics collection middleware
    ├── tracing.rs   # Distributed tracing middleware
    └── validation.rs # Input validation middleware
```

### Effect System Integration

- **Effects** (defined in `aura-protocol`): Core capabilities like `CryptoEffects`, `StorageEffects`
- **Handlers** (implemented here): Agent-specific handlers that compose core effects into device workflows
- **Middleware** (implemented here): Agent-specific cross-cutting concerns
- **Runtime** (`AuraAgent`): Composes handlers + middleware into executable runtime

### Usage

```rust
use aura_agent::{AuraAgent, create_production_agent};

// Production runtime with real effect handlers
let agent = create_production_agent(device_id).await?;

// Testing runtime with mock handlers  
let agent = AuraAgent::for_testing(device_id);

// Custom runtime composition
let agent = AuraAgent::builder(device_id)
    .with_secure_storage()
    .with_biometric_auth()
    .with_metrics_middleware()
    .build().await?;
```

The agent runtime enables device-side operations like credential storage, biometric authentication, session management, and recovery ceremonies by composing the appropriate effect handlers with agent-specific business logic.
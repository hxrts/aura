# Aura Agent

Device-side identity management with effect-based runtime composition.

## Architecture

The `aura-agent` crate follows the unified effect system architecture by composing handlers into a runtime. It does not define effects or implement core effect handlers - instead, it composes existing effect handlers from `aura-protocol` into agent-specific runtimes.

### Core Concept

```rust
// The agent composes handlers to build a runtime using the effect system
let config = aura_protocol::effects::EffectSystemConfig::for_production(device_id)?
    .with_logging(true)
    .with_metrics(true);
let effects = aura_protocol::effects::AuraEffectSystem::new(config)?;
let agent = AuraAgent::new(effects, device_id);

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
│   ├── invitations.rs # InvitationHandler implementation  
│   ├── ota.rs       # OTA operations handler
│   ├── recovery.rs  # RecoveryHandler implementation
│   ├── sessions.rs  # SessionHandler implementation
│   └── storage.rs   # StorageHandler implementation
├── maintenance.rs   # Maintenance operations (GC, snapshots)
└── operations.rs    # Agent operation coordination
```

### Effect System Integration

- **Effects** (defined in `aura-protocol`): Core capabilities like `CryptoEffects`, `StorageEffects`
- **Handlers** (implemented here): Agent-specific handlers that compose core effects into device workflows
- **System Handlers** (from `aura-protocol`): Production-ready logging, metrics, and validation handlers
- **Runtime** (`AuraAgent`): Composes handlers through effect system into executable runtime

### Usage

```rust
use aura_agent::{AuraAgent, create_production_agent};

// Production runtime with real effect handlers
let agent = create_production_agent(device_id).await?;

// Testing runtime with mock handlers
let agent = AuraAgent::for_testing(device_id);

// Custom runtime composition using effect system
let config = aura_protocol::effects::EffectSystemConfig::for_production(device_id)?
    .with_logging(true)
    .with_metrics(true);
let effects = aura_protocol::effects::AuraEffectSystem::new(config)?;
let agent = AuraAgent::new(effects, device_id);
```

The agent runtime enables device-side operations like credential storage, biometric authentication, session management, and recovery ceremonies by composing the appropriate effect handlers with agent-specific business logic.

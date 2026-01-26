# Hello World Guide

This guide gets you running with Aura in 15 minutes. You will build a simple ping-pong protocol, deploy it locally, and interact with it using the CLI.

## Setup

Aura uses Nix for reproducible builds. Install Nix with flakes support.

Enter the development environment:

```bash
nix develop
```

This command activates all required tools and dependencies. The environment includes Rust, development tools, and build scripts.

Build the project:

```bash
just build
```

The build compiles all Aura components and generates the CLI binary. This takes a few minutes on the first run.

## Creating an Agent

Aura provides platform-specific builder presets for creating agents. The CLI preset is the simplest path for terminal applications.

```rust
use aura_agent::AgentBuilder;

// CLI preset - simplest path for terminal applications
let agent = AgentBuilder::cli()
    .data_dir("~/.aura")
    .testing_mode()
    .build()
    .await?;
```

The CLI preset provides sensible defaults for command-line tools. It uses file-based storage, real cryptographic operations, and TCP transport.

For custom environments that need explicit control over effect handlers, use the custom preset with typestate enforcement.

```rust
use std::sync::Arc;
use aura_agent::AgentBuilder;
use aura_effects::{
    RealCryptoHandler, FilesystemStorageHandler,
    PhysicalTimeHandler, RealRandomHandler, RealConsoleHandler,
};

// Custom preset - all effects must be provided
let agent = AgentBuilder::custom()
    .with_crypto(Arc::new(RealCryptoHandler::new()))
    .with_storage(Arc::new(FilesystemStorageHandler::new("~/.aura".into())))
    .with_time(Arc::new(PhysicalTimeHandler::new()))
    .with_random(Arc::new(RealRandomHandler::new()))
    .with_console(Arc::new(RealConsoleHandler::new()))
    .testing_mode()
    .build()
    .await?;
```

The custom preset uses Rust's type system to enforce that all required effects are provided before building. Attempting to call `build()` without providing all five required effects results in a compile error.

Platform-specific presets are available for iOS, Android, and Web/WASM. These require feature flags to enable.

```rust
// iOS preset (requires --features ios)
let agent = AgentBuilder::ios()
    .app_group("group.com.example.aura")
    .build()
    .await?;

// Android preset (requires --features android)
let agent = AgentBuilder::android()
    .application_id("com.example.aura")
    .use_strongbox(true)
    .build()
    .await?;

// Web preset (requires --features web)
let agent = AgentBuilder::web()
    .storage_prefix("aura_")
    .build()
    .await?;
```

See [Project Structure](999_project_structure.md) for details on the 8-layer architecture and effect handler organization.

## Hello World Protocol

Create a simple ping-pong choreography. This protocol demonstrates basic message exchange between two devices.

```rust
use aura_macros::choreography;
use aura_core::effects::{ConsoleEffects, NetworkEffects, TimeEffects};
use aura_core::time::PhysicalTime;
use serde::{Serialize, Deserialize};

/// Sealed supertrait for ping-pong effects
pub trait PingPongEffects: ConsoleEffects + NetworkEffects + TimeEffects {}
impl<T> PingPongEffects for T where T: ConsoleEffects + NetworkEffects + TimeEffects {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ping {
    pub message: String,
    pub timestamp: PhysicalTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pong {
    pub response: String,
    pub timestamp: PhysicalTime,
}

choreography! {
    #[namespace = "hello_world"]
    protocol HelloWorld {
        roles: Alice, Bob;

        Alice[guard_capability = "send_ping", flow_cost = 10]
        -> Bob: SendPing(Ping);

        Bob[guard_capability = "send_pong", flow_cost = 10, journal_facts = "pong_sent"]
        -> Alice: SendPong(Pong);
    }
}
```

The choreography defines a global protocol. Alice sends a ping to Bob. Bob responds with a pong. [Guard capabilities](104_authorization.md) control access and flow costs manage rate limiting.

Implement the Alice session:

```rust
pub async fn execute_alice_session<E: PingPongEffects>(
    effects: &E,
    ping_message: String,
    bob_device: aura_core::DeviceId,
) -> Result<Pong, HelloWorldError> {
    let ping = Ping {
        message: ping_message,
        timestamp: effects.current_timestamp().await,
    };

    let ping_bytes = serde_json::to_vec(&ping)?;
    effects.send_to_peer(bob_device.into(), ping_bytes).await?;

    let (peer_id, pong_bytes) = effects.receive().await?;
    let pong: Pong = serde_json::from_slice(&pong_bytes)?;

    Ok(pong)
}
```

Alice serializes the ping message and sends it to Bob. She then waits for Bob's response and deserializes the pong message. See [Effect System and Runtime](105_effect_system_and_runtime.md) for details on effect-based execution.

## Local Deployment

Initialize a local Aura account:

```bash
just init-account
```

This command creates a 2-of-3 threshold account configuration. The account uses three virtual devices with a threshold of two signatures for operations.

Check account status:

```bash
just status
```

The status command shows account health, device connectivity, and threshold configuration. All virtual devices should show as connected.

Test key derivation:

```bash
just test-dkd my_app signing_context
```

This command tests the Distributed Key Derivation (DKD) protocol. The protocol derives cryptographic keys across multiple devices without revealing private key shares.

## CLI Interaction

The Aura CLI provides commands for account management and protocol testing. These commands demonstrate core functionality.

View account information:

```bash
aura status --verbose
```

This shows detailed account state including journal facts, capability sets, and trust relationships. The journal contains all distributed state updates.

Run a threshold signature test:

```bash
aura threshold-test --message "hello world" --threshold 2
```

The threshold test coordinates signature generation across virtual devices. Two devices must participate to create a valid signature.

View recent protocol activity:

```bash
aura journal-query --limit 10
```

This command shows recent journal entries created by protocol execution. Each entry represents a state change with cryptographic verification.

## Testing Your Protocol

Create a test script for the hello world protocol:

```rust
use aura_macros::aura_test;
use aura_testkit::*;
use aura_agent::runtime::AuraEffectSystem;
use aura_agent::AgentConfig;

#[aura_test]
async fn test_hello_world_protocol() -> aura_core::AuraResult<()> {
    // Create test fixture with automatic tracing
    let fixture = create_test_fixture().await?;

    // Create simple effect systems for testing
    let alice_effects = AuraEffectSystem::testing(&AgentConfig::default());
    let bob_effects = AuraEffectSystem::testing(&AgentConfig::default());

    // Get device IDs for routing
    let alice_device = fixture.create_device_id();
    let bob_device = fixture.create_device_id();

    let ping_message = "Hello Bob!".to_string();

    // Run protocol sessions concurrently
    let (alice_result, bob_result) = tokio::join!(
        execute_alice_session(&alice_effects, ping_message.clone(), bob_device),
        execute_bob_session(&bob_effects, ping_message.clone())
    );

    assert!(alice_result.is_ok(), "Alice session failed");
    assert!(bob_result.is_ok(), "Bob session failed");

    let pong = alice_result?;
    assert!(pong.response.contains(&ping_message));

    Ok(())
}
```

This test creates stateless effect systems for Alice and Bob using testing configuration. The systems are context-free and provide deterministic behavior for testing protocol logic. For comprehensive testing approaches, see [Testing Guide](805_testing_guide.md).

Run the test:

```bash
cargo test test_hello_world_protocol
```

The test validates protocol correctness without requiring network infrastructure. Mock handlers provide deterministic behavior for testing.

## Understanding System Invariants

As you develop protocols, be aware of Aura's system invariants - properties that must always hold true:

- **Charge-Before-Send**: All messages pass through the guard chain, which evaluates over a prepared `GuardSnapshot` and emits `EffectCommand` items that the interpreter executes before any transport send
- **CRDT Convergence**: Identical facts always produce identical state
- **Context Isolation**: Information stays within relational context boundaries
- **Secure Channel Lifecycle**: Channels are epoch-bound and follow strict state transitions

See [System Invariants](005_system_invariants.md) for details. When developing, ensure your protocols respect these invariants to maintain system integrity.

## Next Steps

You now have a working Aura development environment. The hello world protocol demonstrates basic choreographic programming concepts.

Continue with [Core Systems Guide](802_core_systems_guide.md) to learn about effect systems, authentication, and capabilities. Learn advanced coordination patterns in [Coordination Systems Guide](803_coordination_guide.md). For detailed protocol development, see [MPST and Choreography](108_mpst_and_choreography.md).

Explore the simulation system in [Testing Guide](805_testing_guide.md) and [Simulation Guide](806_simulation_guide.md) for comprehensive protocol testing approaches.

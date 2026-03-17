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

For custom environments that need explicit control over effect handlers, use `AgentBuilder::custom()` with typestate enforcement. This requires providing all five core effects (crypto, storage, time, random, console) before `build()` is available.

Platform-specific presets are available for iOS (`AgentBuilder::ios()`), Android (`AgentBuilder::android()`), and Web/WASM (`AgentBuilder::web()`). These require feature flags to enable. See [Effects and Handlers Guide](802_effects_guide.md) for detailed builder examples.

See [Project Structure](999_project_structure.md) for details on the 8-layer architecture and effect handler organization.

## Ownership Declaration Before You Add New Parity-Critical Code

Before adding a new parity-critical module or workflow, declare its ownership
category in the crate `ARCHITECTURE.md`.

Use this rule:

- `Pure` for reducers, validators, and typed contracts
- `MoveOwned` for handles, owner tokens, and ownership transfer/handoff
- `ActorOwned` for long-lived mutable async state and coordinators
- `Observed` for rendering, harness reads, and diagnostics

Also declare:

- which capability gates parity-critical mutation/publication
- which module owns terminal lifecycle
- which timeout/backoff policy the owner consumes

If those points are not explicit, the new module is not ready to land.

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

The choreography defines a global protocol. Alice sends a ping to Bob. Bob responds with a pong. [Guard capabilities](106_authorization.md) control access and flow costs manage rate limiting.

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

Alice serializes the ping message and sends it to Bob. She then waits for Bob's response and deserializes the pong message. See [Effect System](103_effect_system.md) for details on effect-based execution.

## Local Deployment

Initialize a local Aura account:

```bash
just quickstart init
```

This command creates a 2-of-3 threshold account configuration. The account uses three virtual devices with a threshold of two signatures for operations.

Check account status:

```bash
just quickstart status
```

The status command shows account health, device connectivity, and threshold configuration. All virtual devices should show as connected.

Run quickstart smoke checks:

```bash
just quickstart smoke
```

This command runs a local end-to-end smoke flow (init, status, and threshold-signature checks) across multiple virtual devices.

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

    // Create deterministic test effect systems
    let alice_effects = AuraEffectSystem::simulation_for_named_test_with_salt(
        &AgentConfig::default(),
        "test_hello_world_protocol",
        0,
    )?;
    let bob_effects = AuraEffectSystem::simulation_for_named_test_with_salt(
        &AgentConfig::default(),
        "test_hello_world_protocol",
        1,
    )?;

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

This test creates deterministic, seeded effect systems for Alice and Bob using `simulation_for_named_test_with_salt(...)`. The identity + salt pair makes failures reproducible. For comprehensive testing approaches, see [Testing Guide](804_testing_guide.md).

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

See [Project Structure](999_project_structure.md#invariant-traceability) for details. When developing, ensure your protocols respect these invariants to maintain system integrity.

## Next Steps

You now have a working Aura development environment. The hello world protocol demonstrates basic choreographic programming concepts.

Continue with [Effects and Handlers Guide](802_effects_guide.md) to learn about effect systems, platform implementation, and handler patterns. Learn choreographic programming in [Choreography Guide](803_choreography_guide.md). For session type theory, see [MPST and Choreography](110_mpst_and_choreography.md).

Explore testing and simulation in [Testing Guide](804_testing_guide.md) and [Simulation Guide](805_simulation_guide.md).

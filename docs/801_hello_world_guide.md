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

## Hello World Protocol

Create a simple ping-pong choreography. This protocol demonstrates basic message exchange between two devices.

```rust
use aura_macros::aura_choreography;
use aura_core::effects::{ConsoleEffects, NetworkEffects, TimeEffects};
use serde::{Serialize, Deserialize};

/// Sealed supertrait for ping-pong effects
pub trait PingPongEffects: ConsoleEffects + NetworkEffects + TimeEffects {}
impl<T> PingPongEffects for T where T: ConsoleEffects + NetworkEffects + TimeEffects {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ping {
    pub message: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pong {
    pub response: String,
    pub timestamp: u64,
}

aura_choreography! {
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

The choreography defines a global protocol. Alice sends a ping to Bob. Bob responds with a pong. Guard capabilities control access and flow costs manage rate limiting.

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

Alice serializes the ping message and sends it to Bob. She then waits for Bob's response and deserializes the pong message.

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
#[tokio::test]
async fn test_hello_world_protocol() {
    let alice_device = aura_core::DeviceId::new();
    let bob_device = aura_core::DeviceId::new();
    
    let (alice_effects, bob_effects) = create_test_handlers(alice_device, bob_device);
    
    let ping_message = "Hello Bob!".to_string();
    
    let (alice_result, bob_result) = tokio::join!(
        execute_alice_session(&alice_effects, ping_message.clone(), bob_device),
        execute_bob_session(&bob_effects, ping_message.clone())
    );
    
    assert!(alice_result.is_ok());
    assert!(bob_result.is_ok());
    
    let pong = alice_result.unwrap();
    assert!(pong.response.contains(&ping_message));
}
```

This test creates mock handlers for Alice and Bob. The test runs both sessions concurrently and verifies successful message exchange.

Run the test:

```bash
cargo test test_hello_world_protocol
```

The test validates protocol correctness without requiring network infrastructure. Mock handlers provide deterministic behavior for testing.

## Next Steps

You now have a working Aura development environment. The hello world protocol demonstrates basic choreographic programming concepts.

Continue with [Core Systems Guide](802_core_systems_guide.md) to learn about effect systems, authentication, and capabilities. Learn advanced coordination patterns in [Coordination Systems Guide](803_coordination_systems_guide.md).

Explore the simulation system in [Simulation and Testing Guide](805_simulation_and_testing_guide.md) for comprehensive protocol testing approaches.
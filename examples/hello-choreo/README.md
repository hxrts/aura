# hello-choreo

Minimal two-role choreography example that illustrates choreographic protocol programming with the rumpsteak-aura DSL, session types, and guard chain integration concepts.

## What it shows

- Two-role choreography defined with the `choreography!` macro DSL
- Alice → Bob message send with Ping type
- Bob → Alice message send with Pong type
- Automatic session type projection from global choreography
- Guard chain concepts: CapGuard → FlowGuard → JournalCoupler
- Compile-time safety guarantees via session types
- Type-safe message ordering and deadlock-free communication

## Choreography Definition

The example defines a simple ping-pong protocol:

```rust
choreography! {
    protocol PingPong {
        roles: Alice, Bob;
        
        // Alice sends Ping to Bob
        Alice -> Bob: Ping(PingMessage);
        
        // Bob sends Pong back to Alice
        Bob -> Alice: Pong(PongMessage);
    }
}
```

This generates:
- **Alice's session type**: `Send<Ping> -> Recv<Pong> -> End`
- **Bob's session type**: `Recv<Ping> -> Send<Pong> -> End`

## Guard Chain Integration

The example documents how the guard chain would protect each message send:

1. **CapGuard**: Check that sender has required capability
   - Alice checks: `need(SEND_PING) ≤ Caps(Alice_ctx)`
   - Bob checks: `need(SEND_PONG) ≤ Caps(Bob_ctx)`

2. **FlowGuard**: Charge cost and verify budget
   - Alice charges: 100 units for Ping send
   - Bob charges: 100 units for Pong send
   - Returns receipt as proof of charge

3. **JournalCoupler**: Atomic merge and send
   - Merges journal fact atomically with network send
   - All-or-nothing semantics: both succeed or both fail
   - No partial state from failed operations

## Type Safety Guarantees

The choreography! macro provides:

- **Deadlock-free**: Session types prevent communication deadlocks
- **Message order**: Types enforce exact sequence of sends/receives
- **No race conditions**: Choreography projects to sequential local types
- **Compile-time verification**: Protocol violations caught at build time

## Full Implementation Pattern

This example documents the pattern for a full implementation:

```rust
// 1. Create handlers implementing ChoreoHandler for your transport
let alice_handler = YourTransportHandler::new(...);
let bob_handler = YourTransportHandler::new(...);

// 2. Integrate with AuraEffectSystem for guard chain + journal
let effects = AuraEffectSystem::new(...);

// 3. Call interpret to execute the choreography
interpret(choreography, alice_handler, bob_handler, effects).await?;

// 4. Use simulator for deterministic testing with injectable effects
let sim_effects = SimulatorEffects::new(...);
```

## Run

```bash
cargo run -p hello-choreo
```

Or from the workspace root:

```bash
./target/debug/hello-choreo
```

The example outputs:
1. Choreography definition (roles and message sends)
2. Generated session types for each role
3. Guard chain protection explanation
4. Type safety guarantees
5. Key invariants for distributed protocols

## Next Steps

To build a full choreography implementation:

1. **Define message types** that serialize for transport
2. **Implement ChoreoHandler** for your specific transport (TCP, WebSocket, etc.)
3. **Wire effects system** to handle CapGuard, FlowGuard, and JournalCoupler
4. **Test with simulator** using deterministic effect injection
5. **Deploy with real handlers** for production communication

See `docs/800_building_on_aura.md` and `docs/003_distributed_applications.md` for complete examples.

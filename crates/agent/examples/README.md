# Aura Agent Examples

Working examples demonstrating available agent functionality.

## Running Examples

```bash
cargo run --example <example_name>
```

## Available Examples

### [`agent_basic.rs`](agent_basic.rs)
Basic agent functionality using core available types.

- Device/Account ID creation
- DerivedIdentity construction  
- DeviceAttestation creation
- JSON serialization

### [`agent_state.rs`](agent_state.rs)
Agent state management with compile-time safety.

- State transitions (Uninitialized → Idle → Coordinating)
- State-based operation safety
- Identity derivation in correct state
- Mock agent implementation

### [`storage_secure.rs`](storage_secure.rs)
Secure storage interfaces and platform implementations.

- Multiple security levels (StrongBox, TEE, Software)
- Key share and secure data operations
- Platform-specific security features
- Device attestation with security capabilities

## Note

These examples use only the working types and avoid the coordination crate (which has compilation errors). They demonstrate the available agent functionality that can be used for development and testing.
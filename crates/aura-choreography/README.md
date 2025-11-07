# Aura Choreographic Protocol Implementations

This crate contains concrete implementations of Aura's distributed protocols using choreographic programming patterns following **[Protocol Guide](../../../../docs/405_protocol_guide.md)**. It builds on the unified effect system provided by `aura-protocol` and integrates seamlessly with existing Aura infrastructure.

**Architecture Context**: [Architecture Overview](../../../../docs/002_architecture.md) provides the layered stack design showing how choreographic protocols fit into Aura's overall architecture.

## Architecture Overview

Following the protocol guide layered architecture:

```text
Session Type Algebra → Choreographic DSL → Effect System → Semilattice Types
```

### Protocol Categories

- **DKD Protocols**: Deterministic key derivation choreographies
- **FROST Protocols**: Threshold signature choreographies  
- **Consensus Protocols**: Agreement and coordination choreographies
- **Semilattice Protocols**: CRDT synchronization choreographies

All protocols are implemented using the `rumpsteak-aura` choreography macro for compile-time verified session types and deadlock-free distributed execution.

## Quick Start

```rust,ignore
use aura_choreography::integration::{create_testing_adapter, create_choreography_endpoint};
use aura_choreography::protocols::dkd::execute_dkd;

// Create choreographic handler with unified effect system
let mut adapter = create_testing_adapter(device_id);

// Execute DKD protocol using the protocol guide patterns
let config = DkdConfig {
    participants: vec![device1_id, device2_id, device3_id],
    context: "user_key_derivation".to_string(),
    timeout_ms: 30000,
};

let result = execute_dkd(&mut adapter, config).await?;
assert!(result.success);
```

### Design Principles

Following the **[Protocol Guide](../../../../docs/405_protocol_guide.md)** design principles:

- **Start from global choreographic perspective**: Write protocols from a global viewpoint, automatically projected to device-specific actions
- **Use strongly-typed messages**: Clear semantics with version information in protocol messages
- **Model explicit failure modes**: Include timeout handling and Byzantine behavior in choreographies
- **Unified effect system integration**: All operations flow through the aura-protocol effect system

## Integration with Effect System

Choreographic protocols seamlessly integrate with Aura's unified effect system:

```
Choreographic Protocols (this crate)
    ↓ uses
Unified Effect System (aura-protocol)
    ↓ coordinates with  
Core Aura Crates (aura-crypto, aura-journal, aura-store, etc.)
```

### Unified Choreography Adapters

The integration layer provides factory functions for different deployment contexts:

```rust
// For testing with deterministic effects
let adapter = create_testing_adapter(device_id);

// For production deployment
let adapter = create_production_adapter(device_id);

// For simulation environments
let adapter = create_simulation_adapter(device_id);
```

## Module Structure

Following the protocol guide organization:

```
crates/aura-choreography/src/
├── lib.rs                            # Choreographic protocol exports
├── protocols/                        # Core protocol implementations
│   ├── dkd.rs                       # Deterministic key derivation 
│   ├── frost.rs                     # FROST threshold signatures
│   └── consensus.rs                 # Consensus and coordination
├── semilattice/                      # CRDT synchronization choreographies
├── integration/                      # Unified choreography adapters
├── runtime/                          # Choreography execution infrastructure  
├── types/                            # Rumpsteak-compatible type definitions
└── common/                           # Shared utilities
```

### Key Integration Points

- **Cryptography**: Uses `aura-crypto` primitives via effects system
- **State Management**: Integrates with `aura-journal` CRDT 
- **Authorization**: Leverages KeyJournal via capability middleware
- **Transport**: Uses established transport abstraction

## Protocol Implementation Examples

### DKD Choreography

```rust,ignore
use crate::protocols::dkd::{execute_dkd, DkdConfig};

// Configure DKD for 3-participant key derivation
let config = DkdConfig {
    participants: vec![device1, device2, device3],
    context: "application_key".to_string(),
    timeout_ms: 30000,
};

// Execute choreographic protocol with unified effects
let result = execute_dkd(&mut adapter, config).await?;
println!("Derived key: {:?}", result.derived_key);
```

### FROST Threshold Signing

```rust,ignore
use crate::protocols::frost::{execute_frost_signing, FrostConfig};

// Configure FROST for 2-of-3 threshold signature
let config = FrostConfig {
    participants: vec![device1, device2, device3],
    threshold: 2,
    message: "Hello, threshold world!".as_bytes().to_vec(),
    signing_package: signing_package_bytes,
};

// Execute threshold signing choreography
let result = execute_frost_signing(&mut adapter, config).await?;
println!("Threshold signature: {:?}", result.signature);
```

## Development and Testing

### Using DeepWiki MCP for Protocol Guide

This crate is designed to work with the **[Protocol Guide](../../../../docs/405_protocol_guide.md)**. For comprehensive documentation and examples, use the DeepWiki MCP server:

```bash
# Query protocol guide patterns
mcp_deepwiki_ask_question "How do I implement a choreographic consensus protocol?"

# Get complete protocol guide content
mcp_deepwiki_read_wiki_contents aura
```

### Testing with Deterministic Effects

All protocols support deterministic testing through the unified effect system:

```rust,ignore
use aura_choreography::integration::create_testing_adapter;

// Create deterministic test environment
let mut adapter = create_testing_adapter(device_id);
adapter.set_deterministic_seed(42);

// Test protocols with repeatable results
let result1 = execute_dkd(&mut adapter, config.clone()).await?;
adapter.reset_with_seed(42);
let result2 = execute_dkd(&mut adapter, config).await?;

assert_eq!(result1.derived_key, result2.derived_key);
```

## References

- **[Protocol Guide](../../../../docs/405_protocol_guide.md)** - Complete choreographic protocol design patterns
- **[Architecture Overview](../../../../docs/002_architecture.md)** - System architecture context  
- **[Effect System Documentation](../aura-protocol/README.md)** - Unified effect system details
- **[Rumpsteak-Aura DSL](../../work/rumpsteak-aura.md)** - Choreographic programming framework

For detailed implementation guidance, see the **[Protocol Guide](../../../../docs/405_protocol_guide.md)** which provides comprehensive patterns and examples for choreographic protocol development.
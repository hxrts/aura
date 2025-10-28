# Aura Agent Examples

This directory contains examples demonstrating the refactored agent architecture with clean separation of concerns and service delegation patterns.

## Running Examples

All examples can be run with:

```bash
cargo run --example <example_name>
```

## Examples Overview

### [`basic_agent.rs`](basic_agent.rs)
Demonstrates basic usage of the refactored DeviceAgent with clean 3-line service delegation methods.

**Key concepts:**
- Creating agents with default services
- Simple identity derivation (was 50+ lines, now 3 lines)
- Account bootstrap with service delegation
- Session statistics retrieval

**Run with:**
```bash
cargo run --example basic_agent
```

### [`capability_demo.rs`](capability_demo.rs)
Shows the different agent types and their capability-driven architecture patterns.

**Key concepts:**
- `CapabilityAgent` - Pure capability-driven with no external dependencies
- `IntegratedAgent` - Full system integration with transport and storage
- Direct service layer usage for custom integrations

**Run with:**
```bash
cargo run --example capability_demo
```

### [`service_delegation.rs`](service_delegation.rs)
Demonstrates the service delegation pattern that enables 3-line methods and clean separation of concerns.

**Key concepts:**
- Before vs After comparison (50+ lines → 3 lines)
- Service composition and dependency injection
- Error handling through service boundaries
- Independent service usage

**Run with:**
```bash
cargo run --example service_delegation
```

## Legacy Examples (Moved from src/)

The following examples were moved from the main source code to keep production code clean:

### [`refactoring_comparison.rs`](refactoring_comparison.rs)
Shows the detailed before/after comparison of the DeviceAgent refactoring, demonstrating how complex 50+ line methods were simplified to 3-line service delegation calls.

### [`enhanced_security_demo.rs`](enhanced_security_demo.rs)
Demonstrates enhanced security features with verification and replay protection that were part of the exploration phase.

### [`state_management_demo.rs`](state_management_demo.rs)
Shows proper CRDT event sourcing patterns that were developed during the state management improvements phase.

### [`transport_demo.rs`](transport_demo.rs)
Demonstrates transport layer replacement patterns and P2P networking concepts.

## Architecture Benefits Demonstrated

The examples showcase the key benefits of the refactored architecture:

### **Clean Separation of Concerns**
- **Core agents** contain only business logic (3-line methods)
- **Services** handle complex operations and domain logic
- **Infrastructure** manages platform-specific code
- **Clear boundaries** between each layer

### **Testability**
- Services can be mocked independently
- Clean dependency injection through `ServiceRegistry`
- No layer violations or complex interdependencies
- Each service has a single responsibility

### **Maintainability**
- Small, focused files (no more 1,800-line monoliths)
- Clear ownership of functionality by domain
- Easy to locate and modify specific features
- Consistent naming and organization patterns

### **Extensibility**
- New protocols easily added to `protocols/` directory
- Platform support added to `infrastructure/` without core changes
- Service implementations can be swapped or enhanced
- Clean APIs for integration with external systems

## Agent Types Comparison

| Agent Type | Dependencies | Use Cases | Example |
|------------|-------------|-----------|---------|
| **CapabilityAgent** | None (pure) | Testing, embedded, libraries | `basic_agent.rs` |
| **IntegratedAgent** | Transport + Storage | Full applications | `capability_demo.rs` |
| **DeviceAgent** (Refactored) | Services | High-level APIs | `service_delegation.rs` |

## Service Layer Architecture

```
Application Layer    → core/ (DeviceAgent, CapabilityAgent, IntegratedAgent)
                      ↓ (3-line delegation methods)
Business Logic Layer → services/ (IdentityService, AccountService, etc.)
                      ↓ (service calls)
Protocol Layer       → protocols/ (DKD, Recovery, Guardian management)
                      ↓ (protocol calls)
Infrastructure Layer → infrastructure/ (Storage, Transport, Security)
```

## Next Steps

1. **Try the examples** to understand the architecture
2. **Read the service layer code** in `src/services/`
3. **Explore the infrastructure** in `src/infrastructure/`
4. **Look at protocol implementations** in `src/protocols/`
5. **Check the error handling** in `src/error/`

For more information, see the main crate documentation and the individual module documentation.
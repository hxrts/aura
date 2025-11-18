# CLAUDE.md + AGENTS.md

## Project Overview

Aura is a threshold identity and encrypted storage platform built on relational security principles. It uses threshold cryptography and social recovery to eliminate the traditional choice between trusting a single device or a centralized entity.

**Architecture**: Choreographic programming with session types for coordinating distributed protocols. Uses algebraic effects for modular runtime composition. See `docs/999_project_structure.md` for a complete crate breakdown.

## Development Setup

**Required**: Nix with flakes enabled

```bash
nix develop                           # Enter development shell
# OR
echo "use flake" > .envrc && direnv allow  # Auto-activate with direnv
```

All commands below must be run within `nix develop`.

## Common Commands

### Build & Check
- `just build` - Build all crates
- `just check` - Check without building
- `just fmt` - Format code
- `just fmt-check` - Check formatting
- `just clippy` - Lint (warnings as errors)

### Hermetic Builds (crate2nix)
- `nix build` - Build with hermetic Nix (reproducible)
- `nix build .#aura-cli` - Build specific package
- `nix run` - Run aura CLI hermetically
- `nix flake check` - Run hermetic tests
- `crate2nix generate` - Regenerate Cargo.nix after dependency changes

### Testing
- `just test` - Run all tests (preferred)
- `just test-crate <name>` - Test specific crate
- `just ci` - Full CI checks (format, lint, test)
- `just smoke-test` - Phase 0 integration tests
- `cargo test --workspace -- --nocapture` - Tests with output

### Development Workflow
- `just watch` - Rebuild on changes
- `just watch-test` - Retest on changes
- `just clean` - Clean artifacts
- `just docs` - Generate documentation

### Phase 0 Demo
- `just init-account` - Initialize 2-of-3 threshold account
- `just status` - Show account status
- `just test-dkd <app_id> <context>` - Test key derivation

## Architecture Essentials

### 8-Layer Architecture

The codebase follows a strict 8-layer architecture with zero circular dependencies:

1. **Foundation** (`aura-core`): Single source of truth for all domain concepts and interfaces. Effect traits (`CryptoEffects`, `NetworkEffects`, `StorageEffects`, `TimeEffects`, `JournalEffects`, `ConsoleEffects`, `RandomEffects`, `TransportEffects`), domain types (`DeviceId`, `AccountId`, `SessionId`, `Capability`, `FlowBudget`), cryptographic utilities (key derivation, FROST types, merkle trees), semantic traits (`JoinSemilattice`, `MeetSemilattice`, `CvState`, `MvState`), unified error handling (`AuraError`), and reliability utilities (`RetryPolicy`, `RateLimiter`). No other Aura crate dependencies.

2. **Specification** (Domain Crates + `aura-mpst` + `aura-macros`): 
   - Domain crates (`aura-journal`, `aura-wot`, `aura-verify`, `aura-store`, `aura-transport`): Domain-specific types, semantics, and pure logic without effect handlers. `aura-journal` contains CRDT domain types and semilattice operations for distributed ledger state.
   - `aura-mpst` (runtime library): Session type extensions, runtime types, security policies (`LeakageTracker` with secure-by-default budget enforcement), extension registry. Used by 7 crates.
   - `aura-macros` (compile-time tool): Choreography DSL parser, annotation extraction (`guard_capability`, `flow_cost`, `journal_facts`), code generation. Parses choreography syntax with Aura-specific annotations, generates Rust code calling `rumpsteak-aura` for projection. Used by 2 feature crates.
   - **Note**: `aura-sync` demonstrates semantic library independence by intentionally removing `aura-macros` dependency while still using `aura-mpst` types via `aura-protocol`.

3. **Implementation** (`aura-effects`): Standard library of stateless, single-party, context-free effect handlers. Mock handlers (`MockCryptoHandler`, `MockNetworkHandler`, `InMemoryStorageHandler`, `MockTimeHandler`) for testing. Real handlers (`RealCryptoHandler`, `TcpNetworkHandler`, `FilesystemStorageHandler`, `RealTimeHandler`) for production. System handlers (`MonitoringSystemHandler`, `HealthCheckHandler`) for observability. Each handler implements one effect trait independently.

4. **Orchestration** (`aura-protocol`): Stateful coordination infrastructure for multi-party execution. Handler orchestration (`AuraHandlerAdapter`, `CompositeHandler`, `CrdtCoordinator`, `GuardChain`), capability evaluation (`CapabilityEvaluator`), cross-cutting effect implementations (circuit breakers), and reusable coordination patterns (`anti_entropy`, `consensus`, `snapshot`, `threshold_ceremony`).

5. **Feature/Protocol Implementation** (`aura-authenticate`, `aura-frost`, `aura-invitation`, `aura-recovery`, `aura-rendezvous`, `aura-storage`, `aura-sync`): Complete end-to-end protocol implementations. Each crate implements a specific protocol (authentication, FROST threshold signatures, peer invitations, recovery ceremonies, peer discovery, capability-based storage, journal synchronization). `aura-sync` includes choreographic protocols for tree synchronization and journal coordination migrated from `aura-journal`. Use choreography macros, compose handlers from `aura-effects`, use coordination from `aura-protocol`. No UI or main entry points - designed as reusable building blocks.

6. **Runtime Composition** (`aura-agent`, `aura-simulator`): Runtime libraries (NOT binaries) that assemble handlers and protocols. `aura-agent` for production runtime, `aura-simulator` for deterministic testing with controlled scheduling.

7. **User Interface** (`aura-cli`): Binaries with `main()` entry points that users actually run. Drives the `aura-agent` runtime, translating user actions into protocol operations.

8. **Testing & Development Tools** (`aura-testkit`, `aura-quint-api`): Cross-cutting test utilities (shared fixtures, scenario builders, property test helpers) and formal verification bridges.

**Where does my code go?** Use this decision matrix:
- **Stateless + single-party + context-free?** → `aura-effects`
- **Coordinates multiple handlers or multi-party?** → `aura-protocol`
- **Domain-specific types/logic?** → Domain crate
- **Complete protocol implementation?** → Feature crate
- **UI with main()?** → UI crate

### Key Concepts

- **Choreographic Programming**: Write protocols from global viewpoint, automatically projected to device-specific actions. Provides deadlock freedom and type-checked communication.
- **Session Types**: Compile-time safety for protocol state transitions. Typestate prevents invalid operations.
- **Unified State Model**: Single Journal CRDT for account, storage, and communication state. Atomic consistency across subsystems.
- **Threshold Cryptography**: FROST-based M-of-N signatures. No single device can compromise account.
- **Capability-Based Access Control**: Unified system for storage access, relay permissions, and trust evaluation. Supports both traditional capability-based authorization and **Biscuit tokens** for cryptographically verifiable, attenuated delegation.

## Distributed Protocols

### Choreography System Architecture

Aura uses a 3-layer choreographic programming system:

```
choreography! { ... }      # User writes choreography with Aura annotations
    ↓ [aura-macros proc macro]
ExtensionRegistry + rumpsteak session types generation
    ↓ [generated code calls]
aura-mpst runtime with typed extensions + guard chains + journal coupling
```

**Key Components:**

- **`aura-macros`** (proc macro crate): Parses choreography syntax with Aura-specific annotations (`guard_capability`, `flow_cost`, `journal_facts`) and generates calls to rumpsteak-aura + aura-mpst integration
- **`aura-mpst`** (runtime integration): Provides `ExtensionRegistry`, `AuraRuntime`, guard chains, journal coupling, and effect system bridge with rumpsteak session types
- **`rumpsteak-aura`** (session types foundation): Handles choreographic projection, session type safety, and distributed execution

### Choreography Guidelines

Aura uses rumpsteak-aura for choreographic protocol implementation with Aura-specific extensions:

- **DSL-based protocols**: Write protocols using string-based choreography DSL that's parsed at runtime via `rumpsteak_aura_choreography::compiler::parser::parse_choreography_str`
- **Automatic projection**: Global protocols project to local session types for each role, ensuring deadlock freedom
- **Effect-based execution**: Protocols execute through the effect system with `interpret()` using configurable handlers
- **Aura extensions**: Use `aura-macros` for domain-specific annotations (guard capabilities, flow costs, journal facts):

  ```rust
  use aura_macros::choreography;
  
  choreography! {
      #[namespace = "secure_request"]
      protocol SecureRequest {
          roles: Client, Server;
          
          Client[guard_capability = "send_request", flow_cost = 50]
          -> Server: SendRequest(RequestData);
          
          Server[guard_capability = "send_response", flow_cost = 30, 
                 journal_facts = "response_sent"]
          -> Client: SendResponse(ResponseData);
          
          // Messages with annotations but no flow_cost get default of 100
          Client[guard_capability = "acknowledge"]
          -> Server: Ack(AckData);
      }
  }
  ```

- **Protocol composition**: Compose choreographies through effect programs using `.then()` and `.ext()` for cross-cutting concerns
- **Testing support**: Use `InMemoryHandler` for testing, real transport handlers for production
- **Default flow costs**: Messages with role annotations (e.g., `guard_capability`, `journal_facts`) automatically receive a default `flow_cost = 100` if not explicitly specified. This ensures all annotated operations have flow budget tracking without requiring redundant cost declarations.

### Semilattice Guidelines

Aura uses semilattice CRDTs for distributed state management with eventual consistency guarantees:

- **Join semilattices** (⊔): Implement `Join` trait for types that accumulate knowledge through union operations:
  ```rust
  use aura_journal::semilattice::Join;
  
  impl Join for GCounterState {
      fn join(&self, other: &Self) -> Self {
          let mut merged = self.device_counts.clone();
          for (device_id, count) in &other.device_counts {
              let current = merged.get(device_id).copied().unwrap_or(0);
              merged.insert(*device_id, current.max(*count));
          }
          GCounterState { device_counts: merged }
      }
  }
  ```
  Operations must be associative, commutative, and idempotent. Used for `Journal.facts` and other monotonically growing state.

- **Meet semilattices** (⊓): Implement `Meet` trait for types that refine authority through intersection:
  ```rust
  use aura_wot::CapabilitySet;
  
  impl Meet for CapabilitySet {
      fn meet(&self, other: &Self) -> Self {
          let intersection = self.capabilities
              .intersection(&other.capabilities)
              .cloned()
              .collect();
          CapabilitySet { capabilities: intersection }
      }
  }
  ```
  Ensures conservative security - operations require explicit authorization from all sources. Used for `Journal.caps`.

- **Capability system**: Aura's authorization is built on meet-semilattice operations. Capabilities refine through `⊓` (intersection), ensuring that delegation can only restrict authority, never expand it. This provides mathematically guaranteed security - no privilege escalation is possible.

- **CRDT integration**: Use `CrdtCoordinator` builders to integrate with protocols:
  - `CrdtCoordinator::with_cv_state()` for state-based CRDTs
  - `CrdtCoordinator::with_delta_threshold()` for delta CRDTs (more efficient)
  - `CrdtCoordinator::with_mv_state()` for meet-semilattice constraints

- **Key property**: The Journal combines both: facts grow (⊔), capabilities shrink (⊓), providing atomic consistency across subsystems

## Authorization Systems

Aura provides two complementary authorization systems that can be used independently or together:

### Traditional Capability-Based Authorization

Located in `aura-wot` crate, provides semilattice-based capability evaluation:

```rust
use aura_wot::{Capability, CapabilitySet, evaluate_capabilities};

// Check if device has required capabilities
let required = CapabilitySet::single(Capability::Write);
if device_caps.contains_all(&required) {
    // Authorized to write
}
```

**Use Cases**: Device-to-device operations, local authorization decisions, simple capability checks
**Key Features**: Meet-semilattice properties, delegation chains, storage permissions

### Biscuit Token Authorization

Located in `aura-wot/src/biscuit_*` and `aura-protocol/src/authorization/biscuit_*`, provides cryptographically verifiable tokens:

```rust
use aura_wot::biscuit_token::{AccountAuthority, BiscuitTokenManager};
use aura_protocol::authorization::BiscuitAuthorizationBridge;

// Create root authority
let authority = AccountAuthority::new(account_id);
let device_token = authority.create_device_token(device_id)?;

// Create attenuated delegation
let manager = BiscuitTokenManager::new(device_id, device_token);
let read_token = manager.attenuate_read("documents/shared/")?;

// Verify authorization
let bridge = BiscuitAuthorizationBridge::new(authority.root_public_key(), device_id);
let result = bridge.authorize(&read_token, "read", &resource_scope)?;
```

**Use Cases**: Cross-network delegation, offline authorization, audit trails, fine-grained resource access
**Key Features**: Cryptographic verification, attenuation-only delegation, resource-specific authorization

### Integration with Guard System

Both authorization systems integrate with Aura's guard evaluation:

```rust
use aura_protocol::guards::biscuit_evaluator::BiscuitGuardEvaluator;

// Biscuit guards with flow budget
let evaluator = BiscuitGuardEvaluator::new(biscuit_bridge);
let result = evaluator.evaluate_guard(&token, "write", &resource, 50, &mut flow_budget)?;

// Traditional capability guards
if device_caps.contains(&Capability::Write) && flow_budget.can_charge(50) {
    // Execute operation
}
```

### Authorization Guidelines

1. **Choose the Right System**:
   - Use **traditional capabilities** for direct device operations and simple authorization
   - Use **Biscuit tokens** for delegation, cross-network authorization, and audit requirements

2. **Security Best Practices**:
   - Always use least-privilege principles (minimal required capabilities/token scope)
   - Implement flow budget limits to prevent abuse
   - Validate token integrity before authorization decisions
   - Use resource scopes for fine-grained access control

3. **Integration Patterns**:
   - Combine both systems for defense-in-depth (require both to authorize)
   - Use capabilities for local checks, Biscuit for remote delegation
   - Implement caching for performance-critical authorization paths

4. **Resource Scope Patterns**:
   ```rust
   // Storage access
   ResourceScope::Storage { 
       category: StorageCategory::Personal, 
       path: "documents/private/" 
   }
   
   // Journal operations
   ResourceScope::Journal { 
       account_id: account.to_string(), 
       operation: JournalOp::Write 
   }
   
   // Recovery operations
   ResourceScope::Recovery { 
       recovery_type: RecoveryType::DeviceKey 
   }
   ```

See `work/authorization_patterns.md` for comprehensive usage patterns and examples.

## Documentation

### Core Documentation

- **[Project Overview](docs/000_project_overview.md)**: Explains Aura's goals, constraints, and how threshold cryptography combined with choreographic protocols enables practical web-of-trust systems. Provides architecture overview and complete documentation index.

- **[Theoretical Model](docs/001_theoretical_model.md)**: Formal mathematical foundation including Aura calculus, algebraic type definitions, session type algebra, and CRDT semantics. Defines the theoretical guarantees for deadlock freedom, convergence, and information flow.

- **[System Architecture](docs/002_system_architecture.md)**: Complete implementation architecture covering the stateless effect system, CRDT coordination, choreographic protocols, and guard chains. Shows how theoretical concepts map to actual code organization.

- **[Information Flow](docs/003_information_flow.md)**: Privacy framework defining how information flows through trust boundaries with unified flow budgets. Covers threat model, privacy layers, and spam prevention through capability-based authorization.

### Developer Guides

- **[Coordination Systems Guide](docs/803_coordination_systems_guide.md)**: Practical guide to CRDT programming, ratchet tree operations, web-of-trust coordination, and flow budget management. Shows how to compose distributed protocols using session types.

- **[Advanced Choreography Guide](docs/804_advanced_choreography_guide.md)**: Comprehensive DSL syntax reference including message types, dynamic roles, choice constructs, and Aura-specific extensions. Covers protocol composition, error handling, and system layering techniques.

- **[Testing Guide](docs/805_testing_guide.md)**: Testing philosophy and practices including property-based testing, integration testing, and performance benchmarking. Demonstrates how to validate protocol properties like consistency, safety, and liveness.

### Reference

- **[Project Structure](docs/999_project_structure.md)**: Complete 8-layer architecture breakdown with crate responsibilities, dependency graph, and API reference. Essential for understanding where code belongs and how crates interact.

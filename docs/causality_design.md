# Aura Choreographic VM Design

## Purpose

Aura requires a verifiable execution substrate for distributed threshold cryptography protocols. The choreographic VM provides automatic protocol derivation, location-transparent operations, and zero-knowledge proof generation while maintaining compatibility with Aura's identity-centric architecture.

The VM serves three primary functions. First, it executes threshold signature protocols like FROST with multi-round coordination. Second, it synchronizes the Journal CRDT by treating it as an immutable event log with pure materialization. Third, it derives session types automatically from data access patterns to eliminate manual protocol specification.

The design prioritizes correctness and verifiability over raw performance. Protocols execute in a deterministic, traceable environment suitable for proof generation. Native operation recognition provides performance parity with hand-written Rust for critical paths while maintaining the verification benefits.

## Design Principles

### Choreography First

Protocols are specified from a global viewpoint describing the complete interaction. The VM performs local projection to generate device-specific implementations automatically. This eliminates the coordination bugs that arise from manually implementing distributed state machines.

A choreography describes what happens across all participants. The projection step determines what each participant must do locally. Session types ensure the projections are compatible and deadlock-free. The programmer writes one specification and gets correct implementations for all devices.

### Location Transparency

Computation and communication are unified operations that differ only by location parameters. A transform operation works identically whether both endpoints are local, both are remote, or one of each. The location determines execution strategy but not the programming model.

This symmetry eliminates code duplication. Current Aura has separate implementations for local operations and remote protocols. The VM provides a single abstraction that works everywhere. The compiler generates efficient code for each case without programmer intervention.

### Linear Resources

Every resource is created exactly once, used exactly once, and destroyed exactly once. The type system enforces this statically. Linear types prevent use-after-free, double-spend, and race conditions by making them unrepresentable.

Key shares are linear resources. They cannot be copied or reused. The VM tracks their lifecycle and ensures proper cleanup. This matches the security requirements for threshold cryptography where key material must have strict lifecycle management.

### Content Addressing

All data, code, and protocols are identified by the hash of their canonical serialization. This provides global deduplication, verifiable references, and cache-friendly protocols. Two devices computing the same value get the same identifier.

Protocol transforms are content-addressed. The DKD implementation has a deterministic EntityId. All devices load the same code. Updates create new EntityIds rather than modifying existing transforms. This immutability enables aggressive caching and simplifies verification.

### Effect Isolation

Side effects are isolated at the VM boundary through algebraic effect handlers. Protocol logic is pure and verifiable. Effects like secure storage, networking, and cryptography are handled by platform-specific implementations that satisfy a common interface.

The effect boundary is explicit and minimal. Everything inside the VM generates verifiable proofs. Everything outside is trusted platform code. This separation makes auditing practical and enables gradual migration to verified implementations.

## Architecture

### Three Layer Model

The VM implements three layers with distinct responsibilities.

Layer 0 is the register machine. It defines five fundamental instructions that capture all operations. The transform instruction applies morphisms. The alloc instruction creates resources. The consume instruction destroys resources. The compose instruction chains operations sequentially. The tensor instruction combines operations in parallel.

Layer 1 is the linear lambda calculus. It provides functional programming with strict resource tracking. Functions are first-class values. Application consumes both function and argument. Types ensure resources are used exactly once. This layer compiles to Layer 0 instructions.

Layer 2 is the choreographic language. It provides protocol specification, session types, and automatic projection. Programmers write at this level. The compiler generates Layer 1 code with derived session protocols. This layer provides the developer experience.

### Execution Model

The VM executes compiled protocols with deterministic semantics. Each protocol runs in an isolated environment with explicit resource tracking. The execution trace is recorded for verification and debugging.

Execution begins when the runtime loads a protocol by its `EntityId`. The protocol specifies inputs, outputs, and constraints. The runtime resolves dependencies, allocates resources, and begins instruction dispatch. Effects are routed to registered handlers. Results are content-addressed and stored.

The register file contains 32 general-purpose registers following RISC conventions. Each register tracks whether it contains a valid linear resource. Instructions validate resource linearity before execution. Violations cause immediate failure with detailed error reporting.

### Compilation Pipeline

Source code goes through several transformations before execution.

The choreography parser reads protocol specifications and builds an abstract syntax tree. The AST represents the global protocol view with all participants and their interactions.

The session type deriver analyzes data access patterns in the choreography. When code accesses data across device boundaries the deriver generates communication protocols automatically. It computes session types for each participant and verifies they are compatible duals.

The local projector takes the global choreography and generates per-device implementations. Each device gets a projected protocol containing only its relevant operations. The projector ensures coordination requirements are satisfied.

The lambda compiler translates projected protocols into linear lambda calculus. It applies optimizations like function inlining and dead code elimination. It generates explicit resource allocation and deallocation operations.

The instruction generator translates lambda terms into register machine instructions. It performs register allocation, instruction scheduling, and peephole optimization. The output is a sequence of the five fundamental instructions.

The native recognizer identifies transforms that match registered native implementations. It replaces interpretable sequences with native call instructions. This provides performance parity with hand-written code while maintaining verifiability through separate equivalence proofs.

## Core Components

### Register Machine

The register machine is a minimal instruction set designed for deterministic execution and proof generation.

Each instruction operates on the register file. Registers hold linear resources that are content-addressed. The machine tracks resource validity per register. Operations that violate linearity fail immediately.

The transform instruction applies a morphism to an input resource and produces an output resource. The morphism can be interpreted VM code or a native function. The instruction validates that the input is consumed and the output is fresh.

The alloc instruction creates a new resource given a type and initialization data. It allocates a register, marks it valid, and stores the resource. The resource is content-addressed and recorded in the trace.

The consume instruction destroys a resource and invalidates its register. The resource is marked consumed in the trace. Future attempts to use the register fail with a linearity violation error.

The compose instruction chains two morphisms sequentially. The output type of the first must match the input type of the second. The composition creates a new morphism that can be used in transform instructions.

The tensor instruction combines two morphisms to operate on paired resources. It creates a morphism that processes two inputs in parallel and produces two outputs. This enables concurrent operations with proper resource tracking.

### Session Types

Session types describe communication protocols between participants. They ensure type-safe message passing with deadlock freedom guarantees.

A session type is a protocol description. Send operations indicate outgoing messages with their types. Receive operations indicate incoming messages. Choice operations offer multiple branches. Selection operations choose among offered branches. Recursion enables repeated interactions.

Session type duality ensures compatibility. For every send there must be a corresponding receive. For every choice there must be a corresponding selection. The dual computation generates complementary protocols automatically. Duality violations indicate protocol errors.

Automatic derivation generates session types from choreography code. When code reads a field owned by another device the deriver inserts a receive operation. When code writes a field the deriver inserts a send operation. Access patterns determine communication protocols.

Substructural typing ensures resources are used correctly in protocols. Linear session channels are used exactly once. Affine channels are used at most once. Relevant channels must be used but can be copied. Unrestricted channels have no usage constraints.

### Effect System

Effects are pure data structures describing operations to be performed. They separate specification from implementation enabling testing, verification, and cross-platform support.

The effect trait defines the interface. Each effect declares required capabilities, read resources, and written resources. Effects also specify their domain and type. The trait is object-safe enabling dynamic dispatch.

Effect handlers provide implementations. The handler registry maps effect types to concrete implementations. At runtime the VM looks up handlers and invokes them with effect data. Handlers return results or errors that flow back to protocol code.

Secure storage effects handle key material. The store operation saves data to platform secure storage. The retrieve operation loads data. The delete operation removes data. Implementations use Keychain on macOS, Keystore on Android, and Secret Service on Linux.

Network effects handle message passing. The send operation delivers a message to a device. The receive operation waits for incoming messages with optional timeout. The transport layer handles connection management and retries.

Crypto effects handle performance-critical operations. Signing and verification operations use optimized native implementations. Encryption and decryption operations use platform crypto APIs when available. These effects may bypass the VM for performance.

Randomness effects provide entropy. The random bytes operation generates cryptographically secure random data. The random scalar operation generates field elements for FROST. Implementations use platform random number generators.

Time effects provide temporal operations. The now operation returns current time. The sleep operation delays execution. These enable timeouts, rate limiting, and temporal ordering.

### Content Addressing

Every value in the system has a deterministic content identifier computed from its canonical representation.

The EntityId type wraps a 32-byte hash. Computing an EntityId requires SSZ serialization followed by hashing with a pluggable hash function. SSZ provides deterministic encoding with merkleization support. The hash is the unique identifier.

The hash function is defined by a trait allowing different implementations. The initial implementation uses SHA-256 for compatibility and hardware acceleration. Future implementations may use ZK-friendly hash functions like Poseidon or Rescue for efficient proof generation.

```rust
pub trait HashFunction: Send + Sync {
    fn hash(&self, data: &[u8]) -> [u8; 32];
    fn name(&self) -> &'static str;
}

pub struct Sha256Hasher;

impl HashFunction for Sha256Hasher {
    fn hash(&self, data: &[u8]) -> [u8; 32] {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(data);
        hasher.finalize().into()
    }

    fn name(&self) -> &'static str {
        "sha256"
    }
}

pub struct EntityId {
    hash: [u8; 32],
    hash_fn: &'static str,
}

impl EntityId {
    pub fn from_ssz<T: SimpleSerialize>(value: &T, hasher: &dyn HashFunction) -> Self {
        let ssz_bytes = value.serialize();
        let hash = hasher.hash(&ssz_bytes);
        EntityId {
            hash,
            hash_fn: hasher.name(),
        }
    }
}
```

The hash function name is stored with the EntityId to enable future migration. When ZK-friendly hashing is added the system can support multiple hash functions simultaneously during transition.

Resources are content-addressed. Creating a resource computes its EntityId. The resource data and EntityId are stored together. References to resources use EntityIds. Loading a resource validates that its hash matches.

Transforms are content-addressed. A transform definition includes its code, type signature, and metadata. The EntityId is computed from this complete definition. Two semantically identical transforms have the same EntityId enabling global deduplication.

Protocols are content-addressed. A compiled protocol is a sequence of instructions with metadata. The EntityId identifies the complete protocol. Devices exchange protocol EntityIds to coordinate execution. The content addressing enables protocol caching and verification.

The content store maps EntityIds to data. It provides put and get operations with content verification. The store can be local or distributed. Devices fetch missing content by EntityId from peers or repositories.

### Native Operation Registry

The registry maps VM transform definitions to native implementations through verified isomorphisms.

Each entry pairs a transform EntityId with a native function. The native function must produce identical results to the VM interpretation. Property testing or formal verification establishes equivalence.

Recognition happens during compilation. The compiler checks each transform against the registry. When a match is found the compiler emits a native call instruction instead of interpretable code. The native function is invoked directly at runtime.

Cryptographic operations are primary candidates. HMAC-SHA256 has an optimized native implementation. Ed25519 signing and verification use ed25519-dalek, scalar operations use curve25519-dalek, which are orders of magnitude faster than interpretation.

The registry is extensible. Platform-specific optimizations can be added without changing protocol code. New native implementations are registered at initialization. The VM automatically uses them when available.

Verification of isomorphisms is critical. Ideally, property-based testing runs both implementations on thousands of random inputs and checks equivalence. For cryptographic operations, formal verification can prove mathematical equivalence. Failed verification prevents registration.

## Implementation Strategy

### Phase 1: Foundation

Build the register machine and instruction set. Define the five instructions with clear semantics. Implement the register file with linearity tracking. Create the instruction interpreter with execution tracing.

Implement content addressing using SSZ and SHA-256. Define the EntityId type and content addressing trait. Create the content store abstraction with put, get, and verify operations. Build a local file-based implementation.

Define the effect handler interface. Create the handler registry with registration and lookup. Implement mock handlers for testing that return deterministic results. Build the effect invocation machinery.

Implement basic type definitions. Define resource types, capability types, and constraint types. Create the serialization and deserialization infrastructure. Validate that types round-trip correctly.

Build tooling for inspection and debugging. Create a trace viewer that displays execution history. Implement resource tracking that shows allocation and consumption. Add register state visualization.

Expected duration is three weeks with one developer. The result is a working register machine that can execute simple programs with effects.

### Phase 2: Lambda Calculus

Implement the linear lambda calculus layer. Define term representations for values, variables, functions, and applications. Implement the type checker with linearity enforcement. Build the lambda reducer with proper resource management.

Create compilation from lambda terms to register instructions. Implement register allocation with linear resource tracking. Generate efficient instruction sequences. Add optimization passes for common patterns.

Build the standard library of functions. Implement arithmetic operations, comparison operations, and boolean logic. Add string operations, list operations, and record operations. Ensure all functions respect linearity.

Implement the native operation registry. Define the isomorphism verification interface. Create registration machinery with equivalence checking. Add property-based testing infrastructure.

Register critical native operations. Add cryptographic operations from aura-crypto. Include serialization operations. Implement common utility functions. Verify all registered operations against their lambda definitions.

Expected duration is four weeks. The result is a functional programming layer with native optimization that compiles to the register machine.

### Phase 3: Choreographic Language

Design the choreography syntax. Define constructs for protocol specification, participant declaration, and location expressions. Create syntax for local operations, remote operations, and coordination.

Implement the choreography parser. Build an AST representation of global protocols. Add validation for well-formedness. Create error reporting with source locations.

Build the session type deriver. Analyze choreography ASTs for cross-location data access. Generate send and receive operations for remote reads and writes. Compute session types for each participant. Verify duality of complementary roles.

Implement local projection. Take global choreographies and generate per-device implementations. Ensure coordination requirements are satisfied. Handle choice and recursion correctly. Validate projection preserves semantics.

Create the choreography compiler. Translate projected protocols into lambda calculus. Generate resource allocation and cleanup. Insert effect invocations. Optimize the resulting code.

Expected duration is five weeks. The result is a working choreographic language with automatic session type derivation and local projection.

### Phase 4: Aura Integration

Implement platform effect handlers. Create secure storage handlers for macOS Keychain, Linux Secret Service, and Android Keystore. Implement network handlers using Aura transport. Add crypto handlers for FROST operations. Create randomness and time handlers.

Build the type bridge between Aura and VM types. Implement `ToCausality` for DeviceId, KeyShare, AccountId, and Journal events. Implement `FromCausality` for all result types. Add serialization using SSZ. Validate round-trip conversion.

Create the protocol API layer. Implement `derive_key` for DKD. Implement `frost_sign` for threshold signatures. Implement `recover_shares` for recovery protocol. Implement `sync_journal` for CRDT synchronization. Each method creates an intent, compiles it, executes with handlers, and converts results.

Write protocol definitions in the choreographic language. Define DKD as a pure transform. Define FROST as a multi-round distributed protocol. Define recovery as a request-response protocol. Define Journal sync as a bidirectional exchange.

Register native operations for Aura cryptography. Add FROST operations from aura-crypto. Register DKD operations. Add signature operations. Verify all registrations against choreographic definitions.

Expected duration is four weeks. The result is full integration with Aura enabling VM execution of all protocols.

### Phase 5: Optimization and Polish

Implement protocol caching. Cache compiled protocols by `EntityId`. Share cached protocols across devices. Implement cache invalidation on updates. Measure cache hit rates.

Add execution optimizations. Implement instruction fusion for common patterns. Add constant folding and dead code elimination. Optimize register allocation. Measure performance improvements.

Build comprehensive testing. Create property tests for linearity enforcement. Add protocol correctness tests. Implement regression tests for all native operations. Create integration tests with mock effects.

Improve error messages and debugging. Add source location tracking through compilation. Implement detailed error explanations. Create interactive debugger. Add protocol replay from traces.

Write documentation and examples. Document the choreographic language syntax. Provide examples of common protocols. Explain the compilation pipeline. Create migration guides from native Aura protocols.

Expected duration is two weeks. The result is a production-ready VM with good performance and developer experience.

## Total Timeline

The complete implementation requires approximately 18 weeks with one developer or 9 weeks with two developers working in parallel. The phases are mostly sequential but some parallelization is possible.

Phase 1 foundation work is prerequisite for everything else. Phase 2 lambda calculus and Phase 3 choreographic language can overlap partially after register machine is complete. Phase 4 integration requires the choreographic compiler. Phase 5 optimization can happen concurrently with late integration work.

With careful planning the critical path is approximately 14 weeks. This delivers a choreographic VM specifically designed for Aura's needs with all essential features.

## Comparison with Adaptation

Adapting the existing Causality codebase would require similar effort but with ongoing friction.

The effect-centric model would need replacement with choreography. The incomplete runtime would need full implementation. Session type derivation would be new development. The resource lifecycle model would conflict with Aura's identity model.

Building a focused VM provides control over abstractions, optimization for Aura use cases, and alignment with the identity-centric architecture. The effort is comparable but the result is superior.

## Success Criteria

The VM is successful when all Aura protocols execute with these properties.

Correctness: Protocols produce identical results to native implementations. Session types prevent coordination bugs. Linearity prevents resource leaks.

Performance: DKD completes in under 100 microseconds. FROST signing completes within network latency bounds. Journal materialization processes 2000 events per second minimum.

Verifiability: All protocol steps generate verifiable traces. Native operations have verified isomorphisms. Effect handlers are auditable.

Developer experience: Protocols are written once in choreographic language. Session types are derived automatically. Errors provide actionable messages.

Integration: All Aura protocols migrate to VM. Agent layer uses VM runtime. Platform effects work on all supported platforms.

These criteria validate that the VM serves its intended purpose and provides value over the current native implementation approach.

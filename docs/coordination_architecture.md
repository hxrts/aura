# Coordination Crate Architecture

The coordination crate implements Aura's distributed protocol execution, providing choreographic programming abstractions that enable complex multi-party protocols to be written as linear async functions while maintaining deadlock free type safety.

## Core Architecture

The coordination crate employs a layered architecture built around choreographic programming principles. Protocols are expressed as global descriptions that execute locally on each device, eliminating the complexity of explicit message passing and state synchronization.

### Choreographic Programming Model

The crate implements protocols as choreographies: single async functions that describe multi-party protocols from a global viewpoint. Each device executes the same choreography but observes only its local role. This approach provides several guarantees:

The choreographic structure ensures deadlock freedom by construction. The global view eliminates races and coordination failures, while session types provide compile-time verification that protocols follow their intended communication patterns.

### Layer Organization

The architecture organizes into distinct layers with clear responsibilities:

- The **protocols layer** contains complete implementations of cryptographic protocols like DKD, resharing, and recovery. These are pure choreographic functions that yield instructions to coordinate distributed state and return structured results that include ledger mutations and cryptographic proofs.
- The **execution layer** provides the runtime infrastructure for protocol execution. It manages contexts, processes instructions, and coordinates between local execution and distributed state.
- The **session types layer** adds compile-time safety by tracking protocol state at the type level. Each protocol phase has its own type, preventing invalid state transitions.
- The **local runtime layer** manages multiple concurrent protocol sessions on a single device. It routes messages, manages session lifecycles, and provides the async API for protocol management.

## Protocol Execution Model

Protocols execute through an instruction-based model that abstracts away the complexity of distributed coordination.

### Instruction Processing

Protocols yield Instructions that describe their coordination needs:

```rust
pub enum Instruction {
    WriteToLedger(Event),
    AwaitEvent { filter: EventFilter, timeout_epochs: Option<u64> },
    AwaitThreshold { count: usize, filter: EventFilter, timeout_epochs: Option<u64> },
    GetLedgerState,
    RunSubProtocol { protocol_type: ProtocolType, config: ProtocolConfig },
}
```

The execution context processes these instructions by interacting with the underlying ledger, transport, and crypto services. Protocols remain pure functions that describe their coordination requirements without directly managing distributed state.

### Context Management

Protocol execution operates within validated contexts that provide secure access to device keys, ledger state, and network transport.

The `BaseContext` provides common functionality shared by all protocols: session management, device identification, ledger access, and effects injection. Protocol-specific contexts like `ResharingContext` and `RecoveryContext` extend this base with specialized capabilities.

The `ContextBuilder` validates device authorization, capability permissions, and threshold material availability before creating contexts. It loads real ledger state, verifies device keys match the crypto service, and ensures devices are not revoked. This ensures protocols execute only when they have the necessary cryptographic materials and permissions.

### Session Type Safety

Session types provide compile-time guarantees about protocol state transitions.

Each protocol phase has its own type that captures the available operations and required transitions. The `SessionTypedProtocol` wrapper tracks the current state at the type level, preventing invalid operations.

Runtime witnesses validate distributed conditions that cannot be checked at compile time. These include verifying that enough participants have contributed or that threshold conditions are met.

## Time and Effects Management

The coordination crate supports both deterministic simulation and production execution via management of external effects.

### Time Abstraction

The `TimeSource` trait abstracts time in a way that ensures identical behavior in simulation and production environments. Protocols use cooperative yielding rather than polling, specifying exact conditions for resumption.

The `WakeCondition` enumerated type captures the specific conditions that should resume protocol execution: new events arriving, reaching a specific epoch, or matching particular event patterns.

### Effect Injection

External effects like time, randomness, and I/O are injected as dependencies rather than used directly. This enables deterministic testing where the same sequence of inputs produces identical outputs across multiple runs.

The `Effects` struct bundles all external dependencies that protocols might need. This makes explicit the dependencies of external state protocols and enables precise control during testing.

## Event Coordination

Protocol coordination operates through a CRDT-based event system that provides eventual consistency without requiring centralized coordination.

### Unified Event Log

All protocol events are stored in a single AccountLedger CRDT. This provides atomic consistency across all protocol operations and eliminates the need to synchronize between multiple data structures.

Event filtering allows protocols to wait for specific types of events from particular participants. The filtering system supports complex queries that match event types, authors, and content patterns.

Protocols collect all events they emit during execution and return them as part of their structured results. This enables callers to understand exactly what ledger mutations occurred and integrate them appropriately.

### Threshold Coordination

Protocols often need to wait for contributions from multiple participants before proceeding. The threshold awaiting system tracks events from different participants and resumes execution when sufficient contributions are received.

This eliminates the need for protocols to manually track participant contributions or implement their own counting logic. The coordination infrastructure handles these concerns transparently.

## Local Runtime

Devices use the local runtime to participate in multiple concurrent protocol sessions.

### Session Management

Each protocol session runs independently with its own context and state. The runtime tracks session lifecycles, routes events between sessions, and manages resource allocation.

Sessions communicate through the shared event log rather than direct message passing. This eliminates the need for complex message routing and ensures all communication is properly logged and verified.

### Command Interface

The runtime provides an async command interface for starting, monitoring, and terminating protocol sessions. Commands return structured protocol results that include:

- Protocol-specific data (derived keys, recovered shares, new configurations)
- Complete ledger event history from the protocol execution
- Cryptographic proofs and threshold signatures
- Canonical commit payloads for ledger integration

The interface supports both immediate operations and long-running protocols. Clients can await protocol completion or monitor progress through event streams.

## Error Handling

The coordination crate employs a unified error handling strategy that provides detailed context while maintaining consistent APIs.

### Error Classification

Errors are classified by type and include sufficient context for debugging. Protocol errors include session identifiers and error classifications that help determine whether errors are recoverable.

The error system distinguishes between permanent failures that should abort protocols and transient issues that might resolve with retry or alternative approaches.

### Graceful Degradation

Protocols can recover from certain error conditions by adjusting their execution strategy. For example, if a participant fails to respond within the timeout period, protocols can continue with the remaining participants if they still meet the threshold requirements.

## Instrumentation and Observability

The coordination crate includes optional instrumentation that allows visibility into protocol execution.

### Dev Console Integration

The dev console feature enables real-time monitoring of protocol state, event flow, and participant interactions. This provides valuable debugging capabilities during development and testing.

Instrumentation is compile-time optional and adds no overhead to production builds. The instrumentation hooks are designed to capture protocol semantics rather than low-level implementation details.

### Execution Tracing

Protocol execution can be traced at multiple levels of detail. Traces capture the sequence of instructions, state transitions, and coordination events that occur during protocol execution.

Trace data is structured to support both human inspection and automated analysis. This enables protocol verification, performance analysis, and failure diagnosis.

## Module Dependencies

The coordination crate maintains clean dependencies with other system components.

External dependencies are accessed through trait interfaces, enabling testing with mock implementations and backend modularity.

The crate avoids circular dependencies by carefully layering abstractions. Higher-level protocol implementations depend on lower-level coordination primitives, but not vice versa.

Optional features like dev console instrumentation are isolated and add no dependencies to the core coordination functionality.

## Design Benefits

This architecture provides several key benefits for implementing distributed cryptographic protocols:

Type safety eliminates classes of coordination bugs at compile time. Session types ensure protocols follow their intended communication patterns and prevent invalid state transitions.

Choreographic programming eliminates deadlocks and races by construction. The global view makes it impossible to create circular dependencies or coordination failures.

Deterministic testing enables comprehensive protocol validation. The same protocol implementation can be thoroughly tested in simulation and deployed to production with confidence.

Clean layering enables independent evolution of protocol logic and coordination infrastructure. New protocols can be added without modifying the underlying execution system.

Effect injection and time abstraction ensure protocols are environment-independent. The same code works identically in simulation, testing, and various production environments.

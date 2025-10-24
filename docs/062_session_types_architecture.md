# 062 · Session Types and Choreographic Programming Architecture

## Overview

Aura uses session types to provide compile-time safety for distributed choreographic protocols. Session types encode protocol states in the type system, making invalid state transitions impossible at compile time while preserving the distributed, peer-to-peer nature of choreographic programming.

## Core Architecture

### Choreographic Programming Foundation

Choreographic programming remains the primary paradigm for defining distributed protocols in Aura. Protocols are specified as choreographies that describe global communication patterns between participants. Session types enhance this foundation by providing compile-time verification that local implementations correctly follow choreographic specifications.

### Local Session Runtime Per Device

Each device runs its own lightweight session runtime that manages active protocol instances. The runtime processes events from the transport layer, handles local commands, and coordinates with peer devices through existing transport abstractions. This maintains Aura's fully distributed design without requiring central coordinators.

### Type-Safe Protocol States

Protocol states are encoded in the type system using phantom types. Each protocol defines a sequence of states that represent phases of the choreographic execution:

```rust
ChoreographicProtocol<DkdChoreography, CommitmentPhase>
ChoreographicProtocol<DkdChoreography, RevealPhase>
ChoreographicProtocol<DkdChoreography, Completed>
```

Operations are only available in appropriate states, preventing invalid sequences at compile time.

## State Safety Model

### Local Typestate Safety

Session types provide compile-time guarantees for local device state transitions. A device can only perform operations valid for its current state in a given protocol. This prevents common errors like attempting to reveal before committing in threshold protocols.

### Runtime Witnesses for Distributed Invariants

Global distributed properties require runtime verification. Aura uses runtime witnesses - types that can only be constructed after verifying distributed conditions from journal evidence:

```rust
pub struct CollectedCommitments {
    threshold_met: bool,  // Private field ensures constructor verification
}

impl CollectedCommitments {
    pub fn verify_from_journal(events: &[Event], threshold: usize) -> Option<Self> {
        let commitment_count = count_valid_commitments(events);
        if commitment_count >= threshold {
            Some(CollectedCommitments { threshold_met: true })
        } else {
            None
        }
    }
}
```

State transitions that depend on distributed conditions require runtime witnesses as proof that conditions are met.

### Crash Recovery Through Evidence

Typestate values exist only in memory and disappear on crash. Protocol state is reconstructed from persistent journal evidence through rehydration:

```rust
pub trait ProtocolRehydration<P> {
    type State;
    type Evidence;

    fn rehydrate_from_journal(evidence: Self::Evidence) -> Option<P<Self::State>>;
    fn resolve_ambiguous_state(candidates: Vec<Self::State>) -> Self::State;
}
```

Rehydration chooses conservative states to ensure safety over liveness. All state reconstruction is based on cryptographically signed journal evidence.

## Protocol Implementation Pattern

### DKD Choreography Example

The Deterministic Key Derivation protocol demonstrates the session type pattern:

**Local States**: `Initializing`, `CommitmentPhase`, `RevealPhase`, `Finalizing`, `Completed`

**Runtime Witnesses**: `CollectedCommitments`, `VerifiedReveals` for threshold verification

**State Transitions**:
```rust
impl ChoreographicProtocol<DkdChoreography, CommitmentPhase> {
    pub fn transition_to_reveal(
        self,
        witness: CollectedCommitments
    ) -> ChoreographicProtocol<DkdChoreography, RevealPhase>
}
```

The choreographic specification defines the global protocol behavior. Session types ensure each device's local implementation follows the choreography correctly.

### Recovery Choreography Example

Guardian recovery follows the same pattern:

**Local States**: `Initiated`, `CollectingApprovals`, `CollectingShares`, `Reconstructing`, `Completed`

**Runtime Witnesses**: `ApprovalThresholdMet`, `SharesCollected` for guardian thresholds

This maintains the distributed recovery choreography while providing compile-time local safety.

## Communication Architecture

### Typed Channels

Local session runtimes communicate through typed channels that separate concerns:

**Command Channel**: Local user actions and timer events
**Event Channel**: Incoming messages from peer devices
**Effect Channel**: Outgoing operations like sending messages or storing data

### Choreographic Message Flow

1. Choreographic specification defines global protocol behavior
2. Local projection gives each device its view of the choreography
3. Inbound events arrive from peer devices via transport layer
4. Session runtime processes events using session-typed state machines
5. Choreographic state updates follow type-safe transitions
6. Effects are generated and sent to peer devices through transport

Session types ensure the local implementation correctly follows the choreographic specification at each step.

## Implementation Benefits

### Compile-Time Safety

Invalid state operations become compilation errors. Attempting to reveal before committing or send messages in wrong protocol phases is caught by the type system.

### Enhanced Choreographic Programming

Current implicit choreographic state becomes explicit in the type system. Multiple choreographic protocols can run concurrently without state interference. Choreographic protocol states can be tested in isolation.

### Improved Developer Experience

Protocol bugs are caught at compile time rather than runtime. Session types make valid operations obvious through the type system. Protocol state machines serve as executable specifications with IDE support.

### Distributed Coordination

Each device runs its own session runtime without requiring central coordinators. Peer-to-peer interactions are validated by session types. Protocol invariants are checked locally using type constraints while preserving choreographic design.

## Safety Guarantees and Limitations

### What Session Types Provide

Local typestate safety prevents invalid operations for current device state. State transition safety ensures valid local state machine progression. API boundaries make valid operations obvious to developers and tools.

### What Remains Runtime Verified

Distributed invariants like quorum thresholds require runtime verification. Cross-device coordination is verified through runtime witnesses backed by journal evidence. Global protocol progress is ensured through cryptographic evidence and CRDT properties.

### Crash Safety Design

Typestate is reconstructed from persistent journal evidence. Conservative rehydration ensures safety over liveness. Evidence-based state recovery prevents ledger corruption.

## Current Implementation Status

Session type infrastructure is implemented in the coordination crate. Core choreographic protocols including DKD and recovery have been enhanced with session types. Local session runtime provides per-device protocol coordination. Integration with existing transport and journal layers preserves choreographic communication patterns.

## Conclusion

Session types enhance Aura's choreographic programming model by providing compile-time safety for local protocol implementations while preserving the distributed, peer-to-peer nature of the system. This combination delivers both strong safety guarantees and practical distributed system resilience.

The approach maintains all existing choreographic specifications and communication patterns while adding type-level protection against common protocol implementation errors. This represents an evolution of choreographic programming toward greater safety and maintainability.

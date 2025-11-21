# Aura Protocol Quint Specifications

This directory contains formal specifications of Aura's core protocols written in Quint, a modern specification language for distributed systems.

## Overview

These specifications formally model the key protocols and invariants in the Aura system:

### Core Infrastructure
- **session_types.qnt**: Session type system providing compile-time safety for distributed protocols
- **journal_effect_api.qnt**: CRDT-based event effect_api with threshold signatures and causal ordering

### Cryptographic Protocols
- **dkd_protocol.qnt**: Deterministic Key Derivation (DKD) for threshold identity derivation
- **frost_protocol.qnt**: FROST threshold signatures for distributed signing

### Communication Protocols
- **transport_sbb.qnt**: Social Bulletin Board gossip protocol for envelope distribution
- **groups_cgka.qnt**: Continuous Group Key Agreement using BeeKEM for secure group messaging

## Key Properties Verified

### Safety Properties
- **Type Safety**: Invalid protocol state transitions are impossible
- **Byzantine Fault Tolerance**: Protocols tolerate up to threshold-1 malicious participants
- **Causal Consistency**: Events maintain proper causal ordering via Lamport clocks
- **Replay Protection**: Nonce-based protection prevents message replay attacks

### Liveness Properties
- **Progress**: Non-final protocols can always make progress with honest participants
- **Eventual Delivery**: Messages eventually reach all intended recipients
- **CRDT Convergence**: Identical event sets produce identical states

### Security Properties
- **Threshold Security**: Operations requiring threshold authorization cannot proceed without sufficient participants
- **Forward Secrecy**: Compromised keys cannot decrypt past messages
- **Post-Compromise Security**: Key updates heal from compromise
- **Unforgeability**: Signatures cannot be created without threshold participation

## Protocol Interactions

The specifications model how protocols interact:

1. **Session Types** provide the foundation for all protocol state machines
2. **DKD** and **FROST** use the journal for coordination and commitment
3. **Transport SBB** carries protocol messages between participants
4. **Journal Ledger** records all protocol events with threshold authorization
5. **Groups CGKA** builds on the transport for secure group communication

## Running the Specifications

To check these specifications with Quint:

```bash
# Check individual specifications
quint verify session_types.qnt
quint verify dkd_protocol.qnt
quint verify frost_protocol.qnt
quint verify journal_effect_api.qnt
quint verify transport_sbb.qnt
quint verify groups_cgka.qnt

# Run bounded model checking
quint test dkd_protocol.qnt --max-steps 20
quint test frost_protocol.qnt --max-steps 15

# Generate traces
quint run dkd_protocol.qnt --invariant byzantineFaultTolerance
```

## Key Insights from Formal Analysis

The formal specifications have revealed several important properties:

1. **Lamport Clocks Suffice**: The journal only needs Lamport clocks for causal ordering, not full vector clocks, due to the CRDT merge semantics

2. **Witness Requirements**: Runtime witnesses ensure distributed conditions are met before state transitions, preventing race conditions

3. **Threshold Composition**: The interplay between DKD threshold and FROST threshold enables flexible security policies

4. **Gossip Convergence**: The SBB transport achieves eventual consistency through epidemic gossip with exponential backoff

5. **CGKA Tree Invariants**: The BeeKEM tree structure maintains forward secrecy and post-compromise security through careful key evolution

## Future Work

- Model recovery and resharing protocols
- Add probabilistic analysis for gossip latency
- Verify cross-protocol security properties
- Generate implementation tests from specifications

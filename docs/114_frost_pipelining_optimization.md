# FROST Pipelined Commitment Optimization

This document describes the pipelined commitment optimization for FROST consensus that reduces steady-state consensus from 2 RTT (round-trip times) to 1 RTT.

## Overview

The FROST pipelining optimization improves consensus performance by bundling next-round nonce commitments with current-round signature shares. This allows the coordinator to start the next consensus round immediately without waiting for a separate nonce commitment phase.

### Standard FROST Consensus (2 RTT)

In standard FROST consensus, each round requires:

1. **Execute → NonceCommit** (1 RTT): Coordinator sends execute request, witnesses respond with nonce commitments
2. **SignRequest → SignShare** (1 RTT): Coordinator sends aggregated nonces, witnesses respond with signature shares

Total: **2 RTT per consensus**

### Pipelined FROST Consensus (1 RTT)

With pipelining optimization:

1. **Execute+SignRequest → SignShare+NextCommitment** (1 RTT): 
   - Coordinator sends execute request with cached commitments from previous round
   - Witnesses respond with signature share AND next-round nonce commitment

Total: **1 RTT per consensus** (after warm-up)

## Architecture

### Core Components

#### WitnessState (`consensus/witness_state.rs`)

Manages persistent nonce state for each witness:

```rust
pub struct WitnessState {
    /// Precomputed nonce for the next consensus round
    next_nonce: Option<(NonceCommitment, NonceToken)>,
    
    /// Current epoch to detect when cached commitments become stale
    epoch: Epoch,
    
    /// Witness identifier
    witness_id: AuthorityId,
}
```

Key methods:
- `get_next_commitment()`: Returns cached commitment if valid for current epoch
- `take_nonce()`: Consumes cached nonce for use in current round
- `set_next_nonce()`: Stores new nonce for future use
- `invalidate()`: Clears cached state on epoch change

#### Message Schema Updates (`consensus/choreography.rs`)

The `SignShare` message now includes optional next-round commitment:

```rust
SignShare {
    consensus_id: ConsensusId,
    share: PartialSignature,
    /// Optional commitment for the next consensus round (pipelining optimization)
    next_commitment: Option<NonceCommitment>,
    /// Epoch for commitment validation
    epoch: Epoch,
}
```

#### PipelinedConsensusOrchestrator (`consensus/frost_pipelining.rs`)

Orchestrates the optimization logic:

```rust
pub struct PipelinedConsensusOrchestrator {
    /// Manager for witness nonce states
    witness_states: WitnessStateManager,
    
    /// Current epoch
    current_epoch: Epoch,
    
    /// Threshold required for consensus
    threshold: u16,
}
```

Key methods:
- `run_consensus()`: Determines fast path vs slow path based on cached commitments
- `can_use_fast_path()`: Checks if sufficient cached commitments available
- `handle_epoch_change()`: Invalidates all cached state on epoch rotation

## Epoch Safety

All cached commitments are bound to epochs to prevent replay attacks:

1. **Epoch Binding**: Each commitment is tied to a specific epoch
2. **Automatic Invalidation**: Epoch changes invalidate all cached commitments
3. **Validation**: Witnesses reject commitments from wrong epochs

```rust
// Epoch change invalidates all cached nonces
if self.epoch != current_epoch {
    self.next_nonce = None;
    self.epoch = current_epoch;
    return None;
}
```

## Fallback Handling

The system gracefully falls back to 2 RTT when:

1. **Insufficient Cached Commitments**: Less than threshold witnesses have cached nonces
2. **Epoch Change**: All cached commitments become invalid
3. **Witness Failures**: Missing or invalid next_commitment in responses
4. **Initial Bootstrap**: First round after startup (no cached state)

```rust
if has_quorum {
    // Fast path: 1 RTT using cached commitments
    self.run_fast_path(...)
} else {
    // Slow path: 2 RTT standard consensus
    self.run_slow_path(...)
}
```

## Performance Impact

### Latency Reduction
- **Before**: 2 RTT per consensus
- **After**: 1 RTT per consensus (steady state)
- **Improvement**: 50% latency reduction

### Message Count
- **Before**: 4 messages per witness (Execute, NonceCommit, SignRequest, SignShare)
- **After**: 2 messages per witness (Execute+SignRequest, SignShare+NextCommitment)
- **Improvement**: 50% message reduction

### Trade-offs
- **Memory**: Small overhead for caching one nonce per witness
- **Complexity**: Additional state management and epoch tracking
- **Bootstrap**: First round still requires 2 RTT

## Implementation Guidelines

### Adding Pipelining to New Consensus Operations

1. **Update Message Schema**: Add `next_commitment` and `epoch` fields to response messages
2. **Generate Next Nonce**: During signature generation, also generate next-round nonce
3. **Cache Management**: Store next nonce in `WitnessState` for future use
4. **Epoch Handling**: Always validate epoch before using cached commitments

### Example: Witness Implementation

```rust
pub async fn handle_sign_request<R: RandomEffects + ?Sized>(
    &mut self,
    consensus_id: ConsensusId,
    aggregated_nonces: Vec<NonceCommitment>,
    current_epoch: Epoch,
    random: &R,
) -> Result<ConsensusMessage> {
    // Generate signature share
    let share = self.create_signature_share(consensus_id, aggregated_nonces)?;
    
    // Generate or retrieve next-round commitment
    let next_commitment = if let Some((commitment, _)) = self.witness_state.take_nonce(current_epoch) {
        // Use cached nonce
        Some(commitment)
    } else {
        // Generate fresh nonce
        let (nonces, commitment) = self.generate_nonce(random).await?;
        let token = NonceToken::from(nonces);
        
        // Cache for future
        self.witness_state.set_next_nonce(commitment.clone(), token, current_epoch);
        
        Some(commitment)
    };
    
    Ok(ConsensusMessage::SignShare {
        consensus_id,
        share,
        next_commitment,
        epoch: current_epoch,
    })
}
```

## Security Considerations

1. **Nonce Reuse Prevention**: Each nonce is used exactly once and tied to specific epoch
2. **Epoch Isolation**: Nonces from different epochs cannot be mixed
3. **Forward Security**: Epoch rotation provides natural forward security boundary
4. **Availability**: Fallback ensures consensus continues even without optimization

## Testing Strategy

### Unit Tests
- Epoch invalidation logic
- Nonce caching and retrieval
- Message serialization with new fields

### Integration Tests
- Fast path vs slow path selection
- Epoch transition handling
- Performance measurement

### Simulation Tests
- Network delay impact on 1 RTT vs 2 RTT
- Behavior under partial failures
- Convergence properties

## Future Enhancements

1. **Adaptive Thresholds**: Dynamically adjust quorum requirements based on cached state
2. **Predictive Caching**: Pre-generate multiple rounds of nonces during idle time
3. **Compression**: Batch multiple commitments in single message
4. **Cross-Context Optimization**: Share cached state across related consensus contexts

## References

- [FROST Paper](https://eprint.iacr.org/2020/852): Flexible Round-Optimized Schnorr Threshold Signatures
- [`docs/104_consensus.md`](104_consensus.md): Aura Consensus Protocol
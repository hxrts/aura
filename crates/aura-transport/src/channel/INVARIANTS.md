# Secure Channel Invariants

## Secure Channel Lifecycle Invariant

### Invariant Name
`SECURE_CHANNEL_LIFECYCLE`

### Description
Secure channels are strictly bound to epochs and relational contexts. Messages on stale channels (wrong epoch) must be rejected. Channel state transitions must follow the defined state machine.

### Enforcement Locus

1. **Channel State Machine**:
   - Module: `aura-transport/src/channel/state_machine.rs`
   - Type: `ChannelState` enum - `{Closed, Opening, Open(epoch), Closing}`
   - Function: `validate_transition()` - Enforces valid state transitions

2. **Epoch Binding**:
   - Module: `aura-transport/src/channel/epoch_binding.rs`
   - Function: `SecureChannel::validate_epoch()` - Checks message epoch
   - Property: Messages with epoch != channel.epoch are rejected

3. **Context Binding**:
   - Module: `aura-transport/src/channel/context_binding.rs`
   - Type: `ChannelIdentity(ContextId, AuthorityId, AuthorityId, Epoch)`
   - Property: Channel identity immutable after establishment

4. **Message Validation**:
   - Module: `aura-transport/src/channel/validation.rs`
   - Function: `validate_inbound_message()` - Full message validation
   - Checks: Epoch match, context match, state compatibility

### Failure Mode

**Observable Consequences**:
1. **Epoch Confusion Attack**: Stale messages accepted from previous epochs
2. **Channel Hijacking**: Messages routed to wrong context/peer
3. **State Corruption**: Invalid transitions lead to undefined behavior

**Attack Scenarios**:
- Adversary replays messages from previous epoch
- Malicious peer continues sending after epoch rotation
- Race condition during channel teardown/reestablishment

### Detection Method

1. **State Machine Tests**:
   ```rust
   #[test]
   fn test_invalid_transitions_rejected() {
       let mut fsm = ChannelFSM::new();
       
       // Cannot open already open channel
       fsm.transition(Open(epoch1));
       assert!(fsm.transition(Open(epoch2)).is_err());
       
       // Cannot send on closed channel
       fsm.transition(Closed);
       assert!(fsm.can_send().is_false());
   }
   ```

2. **Epoch Validation Tests**:
   ```rust
   #[test]
   fn test_epoch_mismatch_rejection() {
       let channel = SecureChannel::new(ctx, peer, epoch: 5);
       let stale_msg = Message { epoch: 4, ... };
       
       assert_eq!(
           channel.receive(stale_msg),
           Err(ChannelError::EpochMismatch { expected: 5, got: 4 })
       );
   }
   ```

3. **Simulator Scenarios**:
   - Test: `test_epoch_rotation_channel_teardown()`
   - Scenario: Epoch rotates, old channels must close
   - Expected: All messages on old epoch rejected

### Related Invariants
- `EPOCH_MONOTONICITY`: Epochs only increase
- `CHANNEL_UNIQUENESS`: At most one channel per (context, peer, epoch) tuple
- `RENDEZVOUS_EPOCH_SYNC`: Rendezvous descriptors include epoch

### Implementation Notes

Channel lifecycle follows strict FSM:

```rust
// State machine definition
enum ChannelState {
    Closed,
    Opening { 
        context: ContextId,
        peer: AuthorityId,
        epoch: Epoch,
    },
    Open {
        context: ContextId,
        peer: AuthorityId,  
        epoch: Epoch,
        established_at: TimeStamp,
    },
    Closing {
        reason: CloseReason,
    },
}

// CORRECT: Epoch-aware message handling
impl SecureChannel {
    pub async fn send(&mut self, msg: Message) -> Result<()> {
        match &self.state {
            ChannelState::Open { epoch, .. } => {
                if msg.epoch != *epoch {
                    return Err(ChannelError::EpochMismatch);
                }
                self.transport.send(msg).await
            }
            _ => Err(ChannelError::NotOpen),
        }
    }
    
    pub async fn handle_epoch_rotation(&mut self, new_epoch: Epoch) {
        // Channels MUST close on epoch rotation
        self.close(CloseReason::EpochRotation).await;
    }
}

// WRONG: Ignoring epoch
async fn bad_send(channel: &Channel, msg: Message) {
    // This ignores epoch validation!
    channel.transport.send(msg).await;
}
```

### State Transition Rules

Valid transitions:
- `Closed → Opening`: Initiate channel establishment
- `Opening → Open`: Handshake complete, epoch confirmed  
- `Opening → Closed`: Handshake failed
- `Open → Closing`: Graceful shutdown or epoch rotation
- `Closing → Closed`: Cleanup complete

Invalid transitions (must error):
- `Open → Open`: Cannot re-open
- `Closed → Open`: Must go through Opening
- `* → Opening` when already Opening/Open

### Verification

Channel tests:
```bash
cargo test -p aura-transport channel_lifecycle
cargo test -p aura-transport epoch_validation
```
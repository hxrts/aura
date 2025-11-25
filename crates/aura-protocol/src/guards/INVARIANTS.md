# Guard Chain Invariants

## Charge-Before-Send Invariant

### Invariant Name
`CHARGE_BEFORE_SEND`

### Description
No observable network behavior may occur without successful authorization, flow budget charging, and journal coupling. All transport sends must pass through the complete guard chain.

### Enforcement Locus

1. **Primary Enforcement**: Guard chain composition
   - Module: `aura-protocol/src/guards/mod.rs`
   - Function: `GuardChain::execute()`
   - Sequence: `CapGuard` → `FlowGuard` → `JournalCoupler` → `TransportEffects`

2. **Guard Components**:
   - `cap_guard.rs::CapGuard::check()` - Authorization via Biscuit/capability evaluation
   - `flow_guard.rs::FlowGuard::charge()` - Budget charging before send
   - `journal_coupler.rs::JournalCoupler::couple()` - Atomic fact commit

3. **Transport Integration**:
   - Module: `aura-protocol/src/handlers/sessions/shared.rs`
   - Function: All choreography send operations expand through guard chain
   - No direct `TransportEffects::send()` calls permitted outside guard chain

### Failure Mode

**Observable Consequences**:
1. **Authorization Bypass**: Unauthorized messages reach network, violating capability model
2. **Budget Violation**: Spam/DoS attacks possible without flow control
3. **State Inconsistency**: Journal facts not atomically coupled with sends

**Attack Scenarios**:
- Malicious peer exhausts flow budget then continues sending
- Unauthorized peer gains network access
- State divergence between journal and actual sends

### Detection Method

1. **arch-check**: 
   ```bash
   # Flag any TransportEffects::send not within guards/
   grep -r "TransportEffects::send" crates/ | grep -v "guards/"
   ```

2. **Simulator Tests**:
   - Test: `test_send_without_charge_fails_locally()`
   - Scenario: Attempt transport send with exhausted budget
   - Expected: Local failure, no network packet emitted

3. **Property Tests**:
   - Property: For every successful send, exactly one flow charge fact exists
   - Property: No send succeeds after budget exhaustion

### Related Invariants
- `BUDGET_MONOTONICITY`: Spent counters only increase
- `ATOMIC_JOURNAL_COUPLING`: Facts and sends commit together

### Implementation Notes

The guard chain is the sole path to network effects. Any bypass would break Aura's security model. The chain executes in strict order:

```rust
async fn guarded_send(&self, msg: Message) -> Result<()> {
    // 1. Authorization check (CapGuard)
    let cap_decision = self.cap_guard.check(&msg).await?;
    
    // 2. Flow budget charge (FlowGuard)  
    let receipt = self.flow_guard.charge(&msg).await?;
    
    // 3. Journal coupling (JournalCoupler)
    let facts = self.journal_coupler.couple(&msg, &receipt).await?;
    
    // 4. Only now can we send
    self.transport.send(msg).await
}
```
# Refactoring Status - Session Types and Protocol Contexts

**Date**: 2025-10-25  
**Status**: IN PROGRESS (Compilation Blocked)

## Overview

A major refactoring is underway to improve the protocol context architecture by:

1. Separating base context from protocol-specific contexts
2. Centralizing CLI command utilities to reduce code duplication
3. Implementing cleaner session type abstractions

## New Files Created

### Coordination Layer
- `crates/coordination/src/execution/base_context.rs` - Base context with common protocol fields
- `crates/coordination/src/execution/protocol_contexts.rs` - Protocol-specific context types (DkdContext, ResharingContext, RecoveryContext, etc.)

### CLI Layer
- `crates/cli/src/commands/common.rs` - Shared utilities for command handlers (agent creation, scope parsing, attribute parsing)

## Modified Files (Incomplete Migration)

The following files have been modified but the migration is incomplete, causing compilation errors:

### Agent Layer
- `crates/agent/src/dkd.rs`
- `crates/agent/src/integrated_agent.rs`

### CLI Layer
- `crates/cli/src/commands/auth.rs`
- `crates/cli/src/commands/authz.rs`
- `crates/cli/src/commands/network.rs`
- `crates/cli/src/commands/storage.rs`
- `crates/cli/src/commands/mod.rs`

### Coordination Layer
- `crates/coordination/src/execution/context.rs`
- `crates/coordination/src/execution/mod.rs`
- `crates/coordination/src/lib.rs`

### Transport Layer
- `crates/transport/src/stub.rs`
- `crates/transport/src/transport.rs`

## Current Compilation Errors

### 1. Private Field Access in DeviceAgent
**Location**: `crates/agent/src/dkd.rs:77-80`

```rust
error[E0616]: field `ledger` of struct `DeviceAgent` is private
error[E0616]: field `transport` of struct `DeviceAgent` is private
error[E0616]: field `effects` of struct `DeviceAgent` is private
error[E0616]: field `device_key_manager` of struct `DeviceAgent` is private
```

**Issue**: DKD code is trying to access private fields on `DeviceAgent`. Need to either:
- Add public getter methods to `DeviceAgent`
- Make fields pub(crate)
- Refactor DKD to use a different API

### 2. Missing AgentError::coordination Variant
**Location**: Multiple files in `crates/agent/src/integrated_agent.rs`

```rust
error[E0599]: no variant or associated item named `coordination` found for enum `AgentError`
```

**Issue**: Code references `AgentError::coordination()` but this variant doesn't exist. Need to:
- Add the `coordination` variant to `AgentError` enum
- Or use existing variants like `AgentError::orchestrator()`, `AgentError::dkd_failed()`, etc.

### 3. API Changes in CapabilityAgent
**Location**: `crates/agent/src/integrated_agent.rs`

```rust
error[E0609]: no field `ledger` on type `CapabilityAgent`
error[E0599]: no method named `threshold_sign` found for struct `CapabilityAgent`
```

**Issue**: `CapabilityAgent` API has changed:
- No longer has direct `ledger` field access
- `threshold_sign` method removed or renamed

### 4. Coordination Layer API Changes
**Location**: `crates/agent/src/dkd.rs:120`

```rust
error[E0599]: no method named `finalize_session` found for mutable reference `&mut DkdProtocol<'_>`
```

**Issue**: `DkdProtocol::finalize_session()` has been removed. Need to:
- Find the replacement API
- Update DKD code to use new session completion pattern

### 5. TimeSource Trait Mismatch
**Location**: `crates/agent/src/dkd.rs:96`

```rust
error[E0277]: the trait bound `SystemTimeSource: aura_coordination::execution::TimeSource` is not satisfied
```

**Issue**: `aura_crypto::SystemTimeSource` doesn't implement the coordination layer's `TimeSource` trait. Need to:
- Use `ProductionTimeSource` from coordination layer instead
- Or create adapter between the two types

### 6. Missing Transport Trait Import
**Location**: `crates/agent/src/integrated_agent.rs:119`

```rust
error[E0599]: no method named `connect` found for struct `Arc<aura_transport::StubTransport>`
```

**Issue**: `Transport` trait not in scope. Need to add:
```rust
use aura_transport::Transport;
```

### 7. PresenceTicket Field Name Changes
**Location**: `crates/agent/src/integrated_agent.rs:484`

```rust
error[E0609]: no field `epoch` on type `PresenceTicket`
```

**Issue**: `PresenceTicket` field renamed from `epoch` to `session_epoch`. Need to update all references.

### 8. Signature Serialization
**Location**: `crates/agent/src/integrated_agent.rs:486`

```rust
error[E0277]: the trait bound `Signature: AsRef<[u8]>` is not implemented for `Signature`
```

**Issue**: Need to convert `Signature` to bytes before hex encoding:
```rust
hex::encode(my_ticket.signature.to_bytes())
```

## Architecture Changes

### BaseContext Pattern

The new architecture separates protocol context into two layers:

```rust
// Base context with common fields
pub struct BaseContext {
    pub session_id: Uuid,
    pub device_id: Uuid,
    pub participants: Vec<DeviceId>,
    pub threshold: Option<usize>,
    pub(crate) ledger: Arc<RwLock<AccountLedger>>,
    pub(crate) transport: Arc<dyn Transport>,
    pub effects: Effects,
    pub(crate) time_source: Box<dyn TimeSource>,
    // ... event handling fields
}

// Protocol-specific context
pub struct DkdContext {
    base: BaseContext,
}

impl DkdContext {
    // Delegate common operations to base
    pub fn sign_event(&self, event: &Event) -> Result<Signature, ProtocolError> {
        self.base.sign_event(event)
    }
    
    // Protocol-specific operations
    pub async fn get_dkd_commitment_root(&self) -> Result<[u8; 32], ProtocolError> {
        self.base.get_dkd_commitment_root().await
    }
}
```

### Benefits
- Clear separation of concerns
- Protocol-specific contexts only expose relevant operations
- Easier to test individual protocols in isolation
- Reduced coupling between protocols

### Migration Required
- Update all protocol implementations to use new context types
- Update agent layer to create appropriate protocol contexts
- Ensure all field accesses go through proper getters

## CLI Command Utilities

The new `common.rs` module centralizes:

1. **Agent Creation**
   ```rust
   pub async fn create_agent(config: &Config) -> anyhow::Result<IntegratedAgent>
   ```

2. **Capability Scope Parsing**
   ```rust
   pub fn parse_capability_scope(scope_str: &str, resource: Option<&str>) -> anyhow::Result<CapabilityScope>
   ```

3. **Attribute Parsing**
   ```rust
   pub fn parse_attributes(attr_str: &str) -> anyhow::Result<BTreeMap<String, String>>
   ```

4. **Standard Error/Success Messages**
   ```rust
   pub mod errors { ... }
   pub mod success { ... }
   ```

## Next Steps to Complete Refactoring

1. **Add Missing AgentError Variant**
   - Add `Coordination` variant to `AgentError` enum in `crates/agent/src/error.rs`
   - Or map coordination errors to existing variants

2. **Fix DeviceAgent Field Access**
   - Add public getter methods for `ledger`, `transport`, `effects`, `device_key_manager`
   - Or refactor DKD to use higher-level APIs

3. **Update CapabilityAgent API Usage**
   - Find new way to access ledger state
   - Find replacement for `threshold_sign` method

4. **Update DKD Protocol Integration**
   - Replace `finalize_session()` calls with new API
   - Update to use correct `TimeSource` implementation

5. **Fix Import Statements**
   - Add `use aura_transport::Transport` where needed

6. **Update PresenceTicket Field References**
   - Change `ticket.epoch` to `ticket.session_epoch`

7. **Fix Signature Serialization**
   - Use `signature.to_bytes()` before hex encoding

8. **Update All Command Handlers**
   - Migrate CLI commands to use utilities from `common.rs`
   - Remove duplicated code

9. **Run Full Test Suite**
   - Fix any test failures caused by API changes
   - Add tests for new context types

10. **Update Documentation**
    - Update architecture docs to reflect new context pattern
    - Document new CLI utilities

## Testing Strategy

Once compilation is fixed:

1. Run `just check` to verify compilation
2. Run `just test` to verify all tests pass
3. Run `just clippy` to check for warnings
4. Run `just smoke-test` to verify Phase 0 functionality still works

## Estimated Completion Time

Based on the number and complexity of errors:
- **Fixing compilation errors**: 2-3 hours
- **Testing and verification**: 1-2 hours
- **Documentation updates**: 1 hour
- **Total**: 4-6 hours of focused work

## Risks

1. **API Compatibility**: Some agent APIs may have fundamentally changed, requiring larger rewrites
2. **Test Coverage**: Tests may need significant updates to match new architecture
3. **Integration Points**: Changes may ripple to other areas not yet discovered

## Related Work

This refactoring appears to be part of the broader session types initiative mentioned in the codebase. The goal is to provide:
- Compile-time safety for protocol state transitions
- Type-checked communication patterns
- Deadlock-free choreographic programming

The current blocking issues are primarily integration points where the old API is still being used with the new architecture.

# Agent Crate DRY (Don't Repeat Yourself) Analysis

## Executive Summary

The agent crate has significant code duplication, particularly in `agent/session.rs` (2384 lines). This analysis identifies key areas for refactoring to improve maintainability, testability, and code clarity.

## Major Issues

### 1. **Massive `session.rs` Module** (2384 lines)
**Problem**: Single file contains too many responsibilities
- Session state machine
- Bootstrap logic
- Identity derivation (DKD protocol)
- Data storage/retrieval with encryption
- Recovery and resharing protocols
- Capability verification
- Replication logic
- Quota management
- Multiple trait implementations

**Recommendation**: Split into submodules under `session/`

### 2. **Duplicate `verify_protocol_witness` Method**
**Location**: `agent/session.rs` lines 1029 and 1366

```rust
// Appears in both Idle and Failed state implementations
fn verify_protocol_witness(&self, witness: &ProtocolCompleted) -> Result<()> {
    // Identical code duplicated
}
```

**Fix**: Extract to shared helper in `AgentCore` or `session/common.rs`

### 3. **Repeated Session Runtime Command Pattern**
**Count**: 6 occurrences

```rust
// Pattern repeated throughout session.rs
let (command_sender, mut response_receiver) = {
    let mut session_runtime = self.inner.session_runtime.write().await;
    let command_sender = session_runtime.command_sender();
    let response_receiver = session_runtime.response_receiver();
    (command_sender, response_receiver)
};

command_sender.send(command).map_err(|_| {
    crate::error::AuraError::coordination_failed("Failed to send command")
})?;
```

**Locations**:
- Line 166-180: FROST DKG in bootstrap
- Line 430-445: FROST signing in derive_identity
- Line 974-987: Recovery initiation
- Line 1009-1022: Resharing initiation
- Line 1438-1451: Recovery (trait impl)
- Line 1468-1481: Resharing (trait impl)

**Fix**: Extract to helper method:
```rust
impl<T: Transport, S: Storage> AgentCore<T, S> {
    async fn send_session_command(&self, command: SessionCommand) -> Result<mpsc::Sender<SessionResponse>> {
        let (command_sender, response_receiver) = {
            let mut runtime = self.session_runtime.write().await;
            (runtime.command_sender(), runtime.response_receiver())
        };
        
        command_sender.send(command)
            .map_err(|_| AuraError::coordination_failed("Failed to send command"))?;
        
        Ok(response_receiver)
    }
}
```

### 4. **Repeated Response Waiting Pattern**
**Pattern**: Timeout loop waiting for session responses

```rust
// Appears multiple times with slight variations
loop {
    match tokio::time::timeout(Duration::from_secs(5), response_receiver.recv()).await {
        Ok(Some(response)) => match response {
            SessionResponse::SomeCompleted { .. } => { break result; }
            SessionResponse::Error { message } => { return Err(...); }
            _ => continue,
        },
        Ok(None) => return Err(...),
        Err(_) => return Err(...),
    }
}
```

**Locations**:
- Lines 189-226: FROST DKG waiting
- Lines 448-479: FROST signing waiting

**Fix**: Extract to generic helper:
```rust
async fn wait_for_response<F, R>(
    response_receiver: &mut mpsc::Receiver<SessionResponse>,
    timeout: Duration,
    matcher: F,
) -> Result<R>
where
    F: Fn(SessionResponse) -> Option<Result<R>>,
{
    loop {
        match tokio::time::timeout(timeout, response_receiver.recv()).await {
            Ok(Some(response)) => {
                if let Some(result) = matcher(response) {
                    return result;
                }
            }
            Ok(None) => return Err(AuraError::coordination_failed("Channel closed")),
            Err(_) => return Err(AuraError::coordination_failed("Timeout")),
        }
    }
}
```

### 5. **Duplicate `verify_storage_capability` Logic**
**Problem**: Capability verification logic appears in both:
- `traits.rs` line 168 (trait definition)
- `agent/traits.rs` line 175 (another definition)
- `session.rs` line 2186 (full implementation)

**Fix**: Consolidate to single implementation in `capabilities.rs` or `AgentCore`

### 6. **Repeated Metadata Storage Pattern**
**Pattern**: Creating JSON metadata, serializing, and storing

```rust
// Appears ~10 times throughout session.rs
let metadata = serde_json::json!({ ... });
let metadata_bytes = serde_json::to_vec(&metadata)
    .map_err(|e| AuraError::storage_failed(format!("...: {}", e)))?;
self.inner.storage.store(&key, &metadata_bytes).await?;
```

**Fix**: Extract to helper:
```rust
impl<T: Transport, S: Storage> AgentCore<T, S> {
    async fn store_json_metadata(&self, key: &str, value: serde_json::Value) -> Result<()> {
        let bytes = serde_json::to_vec(&value)
            .map_err(|e| AuraError::serialization_failed(format!("Metadata: {}", e)))?;
        self.storage.store(key, &bytes).await
    }
    
    async fn retrieve_json_metadata(&self, key: &str) -> Result<Option<serde_json::Value>> {
        match self.storage.retrieve(key).await? {
            Some(bytes) => Ok(Some(serde_json::from_slice(&bytes)
                .map_err(|e| AuraError::deserialization_failed(format!("Metadata: {}", e)))?)),
            None => Ok(None),
        }
    }
}
```

### 7. **Duplicate Trait Files**
**Problem**: `agent/traits.rs` and root `traits.rs` both define agent traits

**Current structure**:
- `crates/agent/src/traits.rs` - Main trait definitions (Agent, CoordinatingAgent, Transport, Storage, etc.)
- `crates/agent/src/agent/traits.rs` - Additional trait implementations

**Fix**: Consolidate to single location or clarify separation

### 8. **Repeated Error Construction**
**Pattern**: Same error creation patterns throughout

```rust
.map_err(|e| crate::error::AuraError::storage_failed(format!("...: {}", e)))?
.map_err(|e| crate::error::AuraError::serialization_failed(format!("...: {}", e)))?
.map_err(|e| crate::error::AuraError::coordination_failed(format!("...: {}", e)))?
```

**Fix**: Consider implementing `From` conversions or context helpers

### 9. **Repeated Encryption/Decryption Setup**
**Pattern**: Creating encryption contexts

```rust
// Appears in store_encrypted and retrieve_encrypted
let effects = aura_crypto::Effects::production();
let encryption_ctx = aura_crypto::EncryptionContext::new(&effects);
```

**Fix**: Extract to `AgentCore` method:
```rust
impl<T: Transport, S: Storage> AgentCore<T, S> {
    fn get_encryption_context(&self) -> EncryptionContext {
        EncryptionContext::new(&self.effects)
    }
}
```

### 10. **Session Status Filtering Pattern**
**Pattern**: Filtering session statuses by state

```rust
// Appears multiple times in Coordinating state
let active_sessions: Vec<_> = session_statuses
    .iter()
    .filter(|status| !status.is_final)
    .collect();
    
let failed_sessions: Vec<_> = session_statuses
    .iter()
    .filter(|status| matches!(status.status, SessionStatus::Failed | ...))
    .collect();
```

**Fix**: Extract to session status utilities module

## Proposed Refactoring

### Phase 1: Extract Session Helpers (Immediate)

Create `agent/session/helpers.rs`:
```rust
pub mod helpers {
    use super::*;
    
    /// Send a command to the session runtime
    pub async fn send_session_command<T, S>(
        core: &AgentCore<T, S>,
        command: SessionCommand,
    ) -> Result<mpsc::Receiver<SessionResponse>> { ... }
    
    /// Wait for a specific session response with timeout
    pub async fn wait_for_response<F, R>(
        receiver: &mut mpsc::Receiver<SessionResponse>,
        timeout: Duration,
        matcher: F,
    ) -> Result<R> { ... }
    
    /// Verify protocol completion witness
    pub fn verify_protocol_witness(
        device_id: DeviceId,
        witness: &ProtocolCompleted,
    ) -> Result<()> { ... }
}
```

### Phase 2: Restructure Session Module

```
agent/
├── session/
│   ├── mod.rs              # Module organization
│   ├── states.rs           # State types (Uninitialized, Idle, etc.)
│   ├── protocol.rs         # AgentProtocol struct
│   ├── bootstrap.rs        # Bootstrap implementation
│   ├── identity.rs         # Identity derivation (DKD)
│   ├── storage_ops.rs      # Store/retrieve data operations
│   ├── coordination.rs     # Recovery/resharing
│   ├── helpers.rs          # Shared helpers
│   └── trait_impls.rs      # Trait implementations
```

### Phase 3: Extract Capability Management

Move capability-related operations from `session.rs` to `capabilities.rs`:
- `verify_storage_capability`
- `grant_storage_capability`
- `revoke_storage_capability`
- `list_storage_capabilities`

### Phase 4: Extract Storage Operations

Create `agent/storage/` module:
```
agent/storage/
├── mod.rs              # Module organization
├── encrypted.rs        # Encrypted storage operations
├── metadata.rs         # Metadata management
├── replication.rs      # Data replication
├── quota.rs            # Quota management
└── integrity.rs        # Integrity verification
```

### Phase 5: Consolidate Core Logic

Move more logic to `AgentCore`:
- Encryption context creation
- JSON metadata helpers
- Session command helpers
- Storage key formatting

## Benefits of Refactoring

1. **Maintainability**: Smaller, focused modules easier to understand
2. **Testability**: Helper functions can be unit tested independently
3. **Reusability**: Common patterns extracted once
4. **Type Safety**: Generic helpers improve API consistency
5. **Performance**: Reduced code duplication = smaller binary
6. **Clarity**: Clear separation of concerns

## Implementation Priority

### High Priority (Do First)
1. Extract session command helpers → Immediate 40% reduction in duplication
2. Extract `verify_protocol_witness` → Quick win
3. Split `session.rs` into submodules → Major maintainability improvement

### Medium Priority
4. Extract metadata helpers
5. Consolidate capability verification
6. Extract encryption helpers

### Low Priority
7. Reorganize trait files
8. Extract storage operations module
9. Add more `From` conversions for errors

## Metrics

### Current State
- **Largest file**: `session.rs` (2384 lines)
- **Code duplication**: ~600 lines could be deduplicated
- **Helper patterns**: 10+ repeated patterns

### Target State
- **Largest file**: < 500 lines
- **Code duplication**: < 100 lines
- **Helper patterns**: 0 repeated patterns (all extracted)

## Next Steps

1. Start with Phase 1 (session helpers)
2. Test thoroughly after each extraction
3. Update documentation
4. Consider adding integration tests for refactored code
5. Use `just clippy` to catch any issues

## Related Files

- `crates/agent/src/agent/session.rs` - Primary refactoring target
- `crates/agent/src/agent/core.rs` - Target for new helper methods
- `crates/agent/src/agent/capabilities.rs` - Target for capability consolidation
- `crates/agent/src/traits.rs` - Trait consolidation target


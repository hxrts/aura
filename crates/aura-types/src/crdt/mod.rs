//! Universal CRDT (Conflict-free Replicated Data Type) patterns for Aura
//!
//! This module provides the foundational CRDT traits and patterns extracted
//! from the aura-journal implementation. These patterns enable consistent
//! state management across all Aura components with automatic conflict resolution.
//!
//! ## Design Principles
//!
//! 1. **Conflict-free**: All operations commute and converge to same state
//! 2. **Eventually Consistent**: All replicas converge to same state eventually
//! 3. **Partition Tolerant**: Operations work during network partitions
//! 4. **Causally Consistent**: Respects causality of operations
//! 5. **Universal**: Same patterns work for all Aura state management
//!
//! ## CRDT Types Supported
//!
//! - **Counters**: Increment-only and decrement counters (G-Counter, PN-Counter)
//! - **Sets**: Add-only and remove sets (G-Set, 2P-Set, OR-Set)
//! - **Maps**: Key-value maps with last-writer-wins or multi-value semantics
//! - **Text**: Collaborative text editing (via Automerge text CRDT)
//! - **Custom**: Application-specific CRDTs using composition
//!
//! ## Usage Pattern
//!
//! ```rust
//! use aura_types::crdt::{CrdtState, AutomergeCrdt};
//!
//! // Define CRDT state for a component
//! #[derive(CrdtState)]
//! #[crdt(automerge)]
//! struct ComponentState {
//!     #[crdt(counter)]
//!     epoch: u64,
//!     #[crdt(set)]
//!     devices: BTreeSet<DeviceId>,
//!     #[crdt(map)]
//!     metadata: BTreeMap<String, String>,
//! }
//!
//! // Use the state
//! let mut state = ComponentState::new()?;
//! state.increment_epoch()?;
//! state.add_to_devices(device_id)?;
//!
//! // Synchronize with remote state
//! let changes = state.get_changes(&remote_heads);
//! remote_state.apply_changes(changes)?;
//! ```

pub mod automerge;
pub mod traits;
pub mod types;

// Re-export core CRDT types from submodules
pub use automerge::{AutomergeCrdt, AutomergeDocument};
pub use traits::{CrdtState, CrdtOperation, CrdtValue};
pub use types::{CrdtError, ChangeHash, StateId, SyncMessage};


/// Builder for creating CRDT instances with different backends
pub struct CrdtBuilder {
    backend: CrdtBackend,
}

/// Supported CRDT backends
#[derive(Debug, Clone)]
pub enum CrdtBackend {
    /// Automerge backend (JSON-like documents)
    Automerge,
    /// Custom backend for specific use cases
    Custom(String),
}

impl CrdtBuilder {
    /// Create a new CRDT builder
    pub fn new() -> Self {
        Self {
            backend: CrdtBackend::Automerge,
        }
    }
    
    /// Use Automerge as the CRDT backend
    pub fn with_automerge(mut self) -> Self {
        self.backend = CrdtBackend::Automerge;
        self
    }
    
    /// Use a custom CRDT backend
    pub fn with_custom(mut self, backend_name: String) -> Self {
        self.backend = CrdtBackend::Custom(backend_name);
        self
    }
    
    /// Build an Automerge-based CRDT document
    pub fn build_automerge(self) -> Result<AutomergeCrdt, CrdtError> {
        match self.backend {
            CrdtBackend::Automerge => AutomergeCrdt::new(),
            _ => Err(CrdtError::UnsupportedBackend("Expected Automerge backend".to_string())),
        }
    }
}

impl Default for CrdtBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Sync protocol for CRDT state synchronization
pub struct CrdtSyncProtocol<T: CrdtState> {
    local_state: T,
    sync_states: std::collections::HashMap<String, SyncState<T::StateId>>,
}

/// Sync state for tracking synchronization with a peer
#[derive(Debug, Clone)]
pub struct SyncState<StateId> {
    last_sync_heads: Vec<StateId>,
}

impl<T: CrdtState> CrdtSyncProtocol<T> {
    /// Create a new sync protocol for the given state
    pub fn new(state: T) -> Self {
        Self {
            local_state: state,
            sync_states: std::collections::HashMap::new(),
        }
    }
    
    /// Generate sync message for a peer
    pub fn generate_sync_message(&mut self, peer_id: &str) -> Result<SyncMessage, T::Error> {
        let sync_state = self.sync_states.entry(peer_id.to_string()).or_insert_with(|| SyncState {
            last_sync_heads: Vec::new(),
        });
        
        let changes = self.local_state.get_changes(&sync_state.last_sync_heads[..]);
        
        Ok(SyncMessage {
            from_peer: "local".to_string(), // TODO: Use actual peer ID
            to_peer: peer_id.to_string(),
            changes: changes.into_iter().map(|_| vec![]).collect(), // TODO: Serialize changes
            state_id: self.local_state.get_state_id().into_iter().map(|_| ChangeHash::default()).collect(),
        })
    }
    
    /// Process incoming sync message
    pub fn receive_sync_message(&mut self, _message: SyncMessage) -> Result<(), T::Error> {
        // TODO: Deserialize and apply changes
        // For now, this is a placeholder
        Ok(())
    }
    
    /// Get the current state
    pub fn state(&self) -> &T {
        &self.local_state
    }
    
    /// Get mutable reference to the state
    pub fn state_mut(&mut self) -> &mut T {
        &mut self.local_state
    }
}
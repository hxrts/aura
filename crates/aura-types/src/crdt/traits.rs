//! Core CRDT traits extracted from aura-journal

/// Core CRDT state trait that all implementations must follow
pub trait CrdtState: Send + Sync {
    /// Type representing a change/operation in the CRDT
    type Change: Clone + Send + Sync;
    
    /// Type representing the state identifier (vector clock, heads, etc.)
    type StateId: Clone + Send + Sync;
    
    /// Error type for CRDT operations
    type Error: std::error::Error + Send + Sync + 'static;
    
    /// Apply a set of changes to this CRDT state
    fn apply_changes(&mut self, changes: impl IntoIterator<Item = Self::Change>) -> Result<(), Self::Error>;
    
    /// Get all changes since the specified state
    fn get_changes(&self, since: &[Self::StateId]) -> Vec<Self::Change>;
    
    /// Get the current state identifier (heads, vector clock, etc.)
    fn get_state_id(&self) -> Vec<Self::StateId>;
    
    /// Merge another CRDT state into this one, returning the changes applied
    fn merge_with(&mut self, other: &Self) -> Result<Vec<Self::Change>, Self::Error>;
    
    /// Serialize the entire CRDT state to bytes
    fn save(&self) -> Result<Vec<u8>, Self::Error>;
    
    /// Deserialize CRDT state from bytes
    fn load(data: &[u8]) -> Result<Self, Self::Error>
    where
        Self: Sized;
}

/// Trait for operations that can be applied to CRDT state
pub trait CrdtOperation {
    /// Type of the target CRDT state
    type State: CrdtState;
    
    /// Apply this operation to the CRDT state
    fn apply_to(&self, state: &mut Self::State) -> Result<Vec<<Self::State as CrdtState>::Change>, <Self::State as CrdtState>::Error>;
    
    /// Check if this operation is idempotent (can be safely applied multiple times)
    fn is_idempotent(&self) -> bool {
        false
    }
    
    /// Get a unique identifier for this operation (for deduplication)
    fn operation_id(&self) -> String;
}

/// Trait for values that can be stored in CRDT structures
pub trait CrdtValue: Clone + Send + Sync {
    /// Serialize the value to bytes
    fn to_bytes(&self) -> Result<Vec<u8>, super::CrdtError>;
    
    /// Deserialize the value from bytes
    fn from_bytes(data: &[u8]) -> Result<Self, super::CrdtError>
    where
        Self: Sized;
    
    /// Merge two values when there's a conflict (for LWW semantics, return the newer one)
    fn merge_with(&self, other: &Self) -> Self {
        // Default: last-writer-wins (return other)
        other.clone()
    }
}

// Implement CrdtValue for common types
impl CrdtValue for String {
    fn to_bytes(&self) -> Result<Vec<u8>, super::CrdtError> {
        Ok(self.as_bytes().to_vec())
    }
    
    fn from_bytes(data: &[u8]) -> Result<Self, super::CrdtError> {
        String::from_utf8(data.to_vec())
            .map_err(|e| super::CrdtError::SerializationFailed(e.to_string()))
    }
}

impl CrdtValue for u64 {
    fn to_bytes(&self) -> Result<Vec<u8>, super::CrdtError> {
        Ok(self.to_le_bytes().to_vec())
    }
    
    fn from_bytes(data: &[u8]) -> Result<Self, super::CrdtError> {
        if data.len() != 8 {
            return Err(super::CrdtError::SerializationFailed("Invalid u64 length".to_string()));
        }
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(data);
        Ok(u64::from_le_bytes(bytes))
    }
}

impl CrdtValue for serde_json::Value {
    fn to_bytes(&self) -> Result<Vec<u8>, super::CrdtError> {
        serde_json::to_vec(self)
            .map_err(|e| super::CrdtError::SerializationFailed(e.to_string()))
    }
    
    fn from_bytes(data: &[u8]) -> Result<Self, super::CrdtError> {
        serde_json::from_slice(data)
            .map_err(|e| super::CrdtError::SerializationFailed(e.to_string()))
    }
}
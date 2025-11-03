//! CRDT-related types and error definitions

/// CRDT operation errors
#[derive(Debug, thiserror::Error)]
pub enum CrdtError {
    /// CRDT operation failed with description
    #[error("CRDT operation failed: {0}")]
    OperationFailed(String),
    /// Serialization of CRDT state failed
    #[error("Serialization failed: {0}")]
    SerializationFailed(String),
    /// Deserialization of CRDT state failed
    #[error("Deserialization failed: {0}")]
    DeserializationFailed(String),
    /// Merge conflict detected during CRDT operation
    #[error("Merge conflict: {0}")]
    MergeConflict(String),
    /// Invalid CRDT state
    #[error("Invalid state: {0}")]
    InvalidState(String),
    /// Unsupported CRDT backend implementation
    #[error("Unsupported backend: {0}")]
    UnsupportedBackend(String),
    /// CRDT synchronization failed
    #[error("Sync failed: {0}")]
    SyncFailed(String),
}

/// Represents a change hash for CRDT operations
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChangeHash(pub [u8; 32]);

impl Default for ChangeHash {
    fn default() -> Self {
        ChangeHash([0u8; 32])
    }
}

impl From<[u8; 32]> for ChangeHash {
    fn from(hash: [u8; 32]) -> Self {
        ChangeHash(hash)
    }
}

impl AsRef<[u8]> for ChangeHash {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// State identifier for CRDT synchronization
pub type StateId = ChangeHash;

/// Sync message for CRDT state synchronization
#[derive(Debug, Clone)]
pub struct SyncMessage {
    /// Source peer identifier
    pub from_peer: String,
    /// Destination peer identifier
    pub to_peer: String,
    /// Serialized CRDT changes to be synchronized
    pub changes: Vec<Vec<u8>>,
    /// State identifiers for synchronization tracking
    pub state_id: Vec<ChangeHash>,
}

/// CRDT value wrapper that tracks metadata
#[derive(Debug, Clone)]
pub struct CrdtValueWithMetadata<T> {
    /// The actual CRDT value
    pub value: T,
    /// Timestamp when the value was created/modified
    pub timestamp: u64,
    /// Identifier of the actor who created/modified this value
    pub actor_id: String,
    /// Unique operation identifier
    pub operation_id: String,
}

impl<T> CrdtValueWithMetadata<T> {
    /// Create a new CRDT value with metadata
    pub fn new(value: T, timestamp: u64, actor_id: String, operation_id: String) -> Self {
        Self {
            value,
            timestamp,
            actor_id,
            operation_id,
        }
    }
}

/// Last-Writer-Wins (LWW) register
///
/// This is a CRDT that resolves concurrent writes by keeping the value
/// from the write with the highest timestamp. If timestamps are equal,
/// the actor ID is used as a tiebreaker.
#[derive(Debug, Clone)]
pub struct LwwRegister<T> {
    /// The current value with metadata
    value: Option<CrdtValueWithMetadata<T>>,
}

impl<T: Clone> LwwRegister<T> {
    /// Create a new empty LWW register
    pub fn new() -> Self {
        Self { value: None }
    }

    /// Set the value in the register with metadata
    pub fn set(&mut self, value: T, timestamp: u64, actor_id: String, operation_id: String) {
        let new_value = CrdtValueWithMetadata::new(value, timestamp, actor_id, operation_id);

        match &self.value {
            None => self.value = Some(new_value),
            Some(current) => {
                if new_value.timestamp > current.timestamp
                    || (new_value.timestamp == current.timestamp
                        && new_value.actor_id > current.actor_id)
                {
                    self.value = Some(new_value);
                }
            }
        }
    }

    /// Get the current value from the register
    pub fn get(&self) -> Option<&T> {
        self.value.as_ref().map(|v| &v.value)
    }

    /// Merge another register's value with this one, keeping the latest
    pub fn merge(&mut self, other: &Self) {
        if let Some(other_value) = &other.value {
            match &self.value {
                None => self.value = other.value.clone(),
                Some(self_value) => {
                    if other_value.timestamp > self_value.timestamp
                        || (other_value.timestamp == self_value.timestamp
                            && other_value.actor_id > self_value.actor_id)
                    {
                        self.value = other.value.clone();
                    }
                }
            }
        }
    }
}

impl<T: Clone> Default for LwwRegister<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Grow-only counter (G-Counter)
///
/// A CRDT that can only increase. Each actor maintains its own counter value,
/// and the total value is the sum of all actor counters. Merging keeps the
/// maximum value for each actor.
#[derive(Debug, Clone)]
pub struct GCounter {
    counters: std::collections::HashMap<String, u64>,
}

impl GCounter {
    /// Create a new empty grow-only counter
    pub fn new() -> Self {
        Self {
            counters: std::collections::HashMap::new(),
        }
    }

    /// Increment the counter for a specific actor by the given amount
    pub fn increment(&mut self, actor_id: String, amount: u64) {
        let current = self.counters.get(&actor_id).unwrap_or(&0);
        self.counters.insert(actor_id, current + amount);
    }

    /// Get the total value of the counter (sum of all actor counters)
    pub fn value(&self) -> u64 {
        self.counters.values().sum()
    }

    /// Merge another counter into this one, keeping the maximum value for each actor
    pub fn merge(&mut self, other: &Self) {
        for (actor_id, other_count) in &other.counters {
            let self_count = self.counters.get(actor_id).unwrap_or(&0);
            self.counters
                .insert(actor_id.clone(), (*self_count).max(*other_count));
        }
    }
}

impl Default for GCounter {
    fn default() -> Self {
        Self::new()
    }
}

/// Grow-only set (G-Set)
///
/// A CRDT that can only add elements, never remove them. Merging combines
/// the elements from both sets. Only supports elements that implement Clone,
/// Eq, and Hash traits.
#[derive(Debug, Clone)]
pub struct GSet<T: Clone + Eq + std::hash::Hash> {
    elements: std::collections::HashSet<T>,
}

impl<T: Clone + Eq + std::hash::Hash> GSet<T> {
    /// Create a new empty grow-only set
    pub fn new() -> Self {
        Self {
            elements: std::collections::HashSet::new(),
        }
    }

    /// Add an element to the set
    pub fn add(&mut self, element: T) {
        self.elements.insert(element);
    }

    /// Check if an element is in the set
    pub fn contains(&self, element: &T) -> bool {
        self.elements.contains(element)
    }

    /// Get an iterator over all elements in the set
    pub fn elements(&self) -> impl Iterator<Item = &T> {
        self.elements.iter()
    }

    /// Merge another set into this one, adding all elements from the other set
    pub fn merge(&mut self, other: &Self) {
        self.elements.extend(other.elements.iter().cloned());
    }
}

impl<T: Clone + Eq + std::hash::Hash> Default for GSet<T> {
    fn default() -> Self {
        Self::new()
    }
}

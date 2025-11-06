//! Core CRDT traits following the Aura session type algebra
//!
//! This module defines the fundamental trait interfaces for CRDTs as described in
//! docs/402_crdt_types.md. These traits enable conflict-free replicated data types
//! expressed through Aura's session type system.

/// Join semilattice trait for state-based CRDTs (CvRDT)
///
/// A join semilattice has a binary operation (join) that is:
/// - Commutative: a ⊔ b = b ⊔ a
/// - Associative: (a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)
/// - Idempotent: a ⊔ a = a
pub trait JoinSemilattice: Clone {
    /// Join this value with another, producing the least upper bound
    fn join(&self, other: &Self) -> Self;
}

/// Bottom element trait for lattices with a minimum
pub trait Bottom {
    /// Return the bottom element (minimum value)
    fn bottom() -> Self;
}

/// Convergent Replicated Data Type (CvRDT) - state-based CRDT
///
/// CvRDTs synchronize by exchanging full state and merging using join.
/// Convergence is guaranteed by semilattice laws.
pub trait CvState: JoinSemilattice + Bottom {}

/// Delta CRDT trait for incremental state synchronization
///
/// Delta CRDTs optimize bandwidth by transmitting deltas (partial updates)
/// rather than full states. Deltas can be joined and eventually folded into state.
pub trait Delta: Clone {
    /// Join this delta with another delta
    fn join_delta(&self, other: &Self) -> Self;
}

/// Delta production from state changes
pub trait DeltaProduce<S> {
    /// Compute the delta between old and new state
    fn delta_from(old: &S, new: &S) -> Self;
}

/// Causal operation trait for operation-based CRDTs (CmRDT)
///
/// CmRDTs propagate operations that are applied to local state.
/// Each operation carries a causal context (e.g., vector clock).
pub trait CausalOp {
    /// Operation identifier type (for deduplication)
    type Id: Clone;
    /// Causal context type (vector clock, dependency set, etc.)
    type Ctx: Clone;

    /// Get the operation identifier
    fn id(&self) -> Self::Id;
    /// Get the causal context
    fn ctx(&self) -> &Self::Ctx;
}

/// Commutative Replicated Data Type (CmRDT) - operation-based CRDT
///
/// CmRDTs apply operations that commute under causal delivery.
/// The apply method must be commutative for concurrent operations.
pub trait CmApply<Op> {
    /// Apply an operation to this state
    fn apply(&mut self, op: Op);
}

/// Deduplication trait for operation-based CRDTs
///
/// Tracks which operations have been seen to prevent duplicate application.
pub trait Dedup<I> {
    /// Check if an operation has been seen
    fn seen(&self, id: &I) -> bool;
    /// Mark an operation as seen
    fn mark_seen(&mut self, id: I);
}

/// Generic CRDT state trait (legacy interface, being phased out)
///
/// Note: New code should use JoinSemilattice + CvState or CmApply traits directly.
/// This trait is kept for backwards compatibility with existing code.
pub trait CrdtState: Send + Sync {
    /// Type representing a change/operation in the CRDT
    type Change: Clone + Send + Sync;

    /// Type representing the state identifier (vector clock, heads, etc.)
    type StateId: Clone + Send + Sync;

    /// Error type for CRDT operations
    type Error: std::error::Error + Send + Sync + 'static;

    /// Apply a set of changes to this CRDT state
    fn apply_changes(
        &mut self,
        changes: impl IntoIterator<Item = Self::Change>,
    ) -> Result<(), Self::Error>;

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

/// Trait for operations that can be applied to CRDT state (legacy)
///
/// Note: New code should use CausalOp + CmApply traits directly.
pub trait CrdtOperation {
    /// Type of the target CRDT state
    type State: CrdtState;

    /// Apply this operation to the CRDT state
    fn apply_to(
        &self,
        state: &mut Self::State,
    ) -> Result<Vec<<Self::State as CrdtState>::Change>, <Self::State as CrdtState>::Error>;

    /// Check if this operation is idempotent
    fn is_idempotent(&self) -> bool {
        false
    }

    /// Get a unique identifier for this operation
    fn operation_id(&self) -> String;
}

/// Trait for values that can be stored in CRDT structures (legacy)
pub trait CrdtValue: Clone + Send + Sync {
    /// Serialize the value to bytes
    fn to_bytes(&self) -> Result<Vec<u8>, super::CrdtError>;

    /// Deserialize the value from bytes
    fn from_bytes(data: &[u8]) -> Result<Self, super::CrdtError>
    where
        Self: Sized;

    /// Merge two values when there's a conflict
    fn merge_with(&self, other: &Self) -> Self {
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
            return Err(super::CrdtError::SerializationFailed(
                "Invalid u64 length".to_string(),
            ));
        }
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(data);
        Ok(u64::from_le_bytes(bytes))
    }
}

impl CrdtValue for serde_json::Value {
    fn to_bytes(&self) -> Result<Vec<u8>, super::CrdtError> {
        serde_json::to_vec(self).map_err(|e| super::CrdtError::SerializationFailed(e.to_string()))
    }

    fn from_bytes(data: &[u8]) -> Result<Self, super::CrdtError> {
        serde_json::from_slice(data)
            .map_err(|e| super::CrdtError::SerializationFailed(e.to_string()))
    }
}

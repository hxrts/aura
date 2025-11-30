//! Delta<T> - Incremental changes to collections
//!
//! `Delta<T>` represents incremental changes to a collection, enabling efficient
//! list updates without full re-renders. Used primarily for reactive list UIs
//! and incremental database subscriptions.

/// Represents an incremental change to a collection.
///
/// Delta enables efficient updates by describing what changed rather than
/// sending the entire collection. This is essential for:
/// - Smooth list animations in TUI
/// - Efficient database change subscriptions
/// - Reduced network bandwidth for sync
///
/// # Example
///
/// ```rust,ignore
/// use aura_core::reactive::{Delta, apply_delta};
///
/// let mut items = vec!["a", "b", "c"];
///
/// // Insert at position 1
/// let delta = Delta::Insert { index: 1, item: "x" };
/// apply_delta(&mut items, delta);
/// assert_eq!(items, vec!["a", "x", "b", "c"]);
///
/// // Remove at position 2
/// let delta = Delta::Remove { index: 2 };
/// apply_delta(&mut items, delta);
/// assert_eq!(items, vec!["a", "x", "c"]);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delta<T> {
    /// Replace the entire collection with new items.
    /// Used for initial load or when incremental updates aren't practical.
    Reset(Vec<T>),

    /// Insert an item at the specified index.
    /// Items at and after this index shift right.
    Insert {
        /// Position to insert at (0-indexed)
        index: usize,
        /// Item to insert
        item: T,
    },

    /// Remove the item at the specified index.
    /// Items after this index shift left.
    Remove {
        /// Position to remove from (0-indexed)
        index: usize,
    },

    /// Update the item at the specified index.
    /// The item is replaced in-place.
    Update {
        /// Position to update (0-indexed)
        index: usize,
        /// New item value
        item: T,
    },

    /// Apply multiple deltas in sequence.
    /// Deltas are applied in order, allowing complex changes
    /// to be expressed atomically.
    Batch(Vec<Delta<T>>),
}

impl<T> Delta<T> {
    /// Create a reset delta with the given items.
    pub fn reset(items: Vec<T>) -> Self {
        Delta::Reset(items)
    }

    /// Create an insert delta.
    pub fn insert(index: usize, item: T) -> Self {
        Delta::Insert { index, item }
    }

    /// Create a remove delta.
    pub fn remove(index: usize) -> Self {
        Delta::Remove { index }
    }

    /// Create an update delta.
    pub fn update(index: usize, item: T) -> Self {
        Delta::Update { index, item }
    }

    /// Create a batch delta.
    pub fn batch(deltas: Vec<Delta<T>>) -> Self {
        Delta::Batch(deltas)
    }

    /// Check if this is an empty batch or empty reset.
    pub fn is_empty(&self) -> bool {
        match self {
            Delta::Reset(items) => items.is_empty(),
            Delta::Batch(deltas) => deltas.is_empty(),
            _ => false,
        }
    }
}

/// Apply a delta to a mutable vector.
///
/// This modifies the vector in-place according to the delta.
///
/// # Panics
///
/// Panics if the index is out of bounds for the operation:
/// - Insert: index > len
/// - Remove: index >= len
/// - Update: index >= len
///
/// # Example
///
/// ```rust,ignore
/// use aura_core::reactive::{Delta, apply_delta};
///
/// let mut items = vec![1, 2, 3];
/// apply_delta(&mut items, Delta::Insert { index: 1, item: 10 });
/// assert_eq!(items, vec![1, 10, 2, 3]);
/// ```
pub fn apply_delta<T>(items: &mut Vec<T>, delta: Delta<T>) {
    match delta {
        Delta::Reset(new_items) => {
            *items = new_items;
        }
        Delta::Insert { index, item } => {
            items.insert(index, item);
        }
        Delta::Remove { index } => {
            items.remove(index);
        }
        Delta::Update { index, item } => {
            items[index] = item;
        }
        Delta::Batch(deltas) => {
            for d in deltas {
                apply_delta(items, d);
            }
        }
    }
}

/// Try to apply a delta to a mutable vector, returning an error if out of bounds.
///
/// This is the fallible version of `apply_delta` that returns an error
/// instead of panicking on invalid indices.
///
/// # Errors
///
/// Returns `DeltaError::IndexOutOfBounds` if the index is invalid for the operation.
pub fn try_apply_delta<T>(items: &mut Vec<T>, delta: Delta<T>) -> Result<(), DeltaError> {
    match delta {
        Delta::Reset(new_items) => {
            *items = new_items;
            Ok(())
        }
        Delta::Insert { index, item } => {
            if index > items.len() {
                Err(DeltaError::IndexOutOfBounds {
                    index,
                    len: items.len(),
                    operation: "insert",
                })
            } else {
                items.insert(index, item);
                Ok(())
            }
        }
        Delta::Remove { index } => {
            if index >= items.len() {
                Err(DeltaError::IndexOutOfBounds {
                    index,
                    len: items.len(),
                    operation: "remove",
                })
            } else {
                items.remove(index);
                Ok(())
            }
        }
        Delta::Update { index, item } => {
            if index >= items.len() {
                Err(DeltaError::IndexOutOfBounds {
                    index,
                    len: items.len(),
                    operation: "update",
                })
            } else {
                items[index] = item;
                Ok(())
            }
        }
        Delta::Batch(deltas) => {
            for d in deltas {
                try_apply_delta(items, d)?;
            }
            Ok(())
        }
    }
}

/// Error type for delta operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeltaError {
    /// Index was out of bounds for the operation.
    IndexOutOfBounds {
        /// The invalid index
        index: usize,
        /// The current length of the collection
        len: usize,
        /// The operation that failed
        operation: &'static str,
    },
}

impl std::fmt::Display for DeltaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeltaError::IndexOutOfBounds {
                index,
                len,
                operation,
            } => {
                write!(
                    f,
                    "delta {operation} failed: index {index} out of bounds for length {len}"
                )
            }
        }
    }
}

impl std::error::Error for DeltaError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delta_reset() {
        let mut items = vec![1, 2, 3];
        apply_delta(&mut items, Delta::reset(vec![10, 20]));
        assert_eq!(items, vec![10, 20]);
    }

    #[test]
    fn test_delta_insert() {
        let mut items = vec![1, 2, 3];
        apply_delta(&mut items, Delta::insert(1, 10));
        assert_eq!(items, vec![1, 10, 2, 3]);
    }

    #[test]
    fn test_delta_insert_at_end() {
        let mut items = vec![1, 2, 3];
        apply_delta(&mut items, Delta::insert(3, 10));
        assert_eq!(items, vec![1, 2, 3, 10]);
    }

    #[test]
    fn test_delta_remove() {
        let mut items = vec![1, 2, 3];
        apply_delta(&mut items, Delta::remove(1));
        assert_eq!(items, vec![1, 3]);
    }

    #[test]
    fn test_delta_update() {
        let mut items = vec![1, 2, 3];
        apply_delta(&mut items, Delta::update(1, 20));
        assert_eq!(items, vec![1, 20, 3]);
    }

    #[test]
    fn test_delta_batch() {
        let mut items = vec![1, 2, 3];
        apply_delta(
            &mut items,
            Delta::batch(vec![
                Delta::insert(0, 0),  // [0, 1, 2, 3]
                Delta::remove(3),     // [0, 1, 2]
                Delta::update(2, 20), // [0, 1, 20]
            ]),
        );
        assert_eq!(items, vec![0, 1, 20]);
    }

    #[test]
    fn test_delta_is_empty() {
        assert!(Delta::<i32>::reset(vec![]).is_empty());
        assert!(Delta::<i32>::batch(vec![]).is_empty());
        assert!(!Delta::<i32>::insert(0, 1).is_empty());
        assert!(!Delta::<i32>::remove(0).is_empty());
        assert!(!Delta::<i32>::update(0, 1).is_empty());
    }

    #[test]
    fn test_try_apply_delta_success() {
        let mut items = vec![1, 2, 3];
        assert!(try_apply_delta(&mut items, Delta::insert(1, 10)).is_ok());
        assert_eq!(items, vec![1, 10, 2, 3]);
    }

    #[test]
    fn test_try_apply_delta_insert_out_of_bounds() {
        let mut items = vec![1, 2, 3];
        let result = try_apply_delta(&mut items, Delta::insert(5, 10));
        assert!(matches!(
            result,
            Err(DeltaError::IndexOutOfBounds {
                index: 5,
                len: 3,
                operation: "insert"
            })
        ));
    }

    #[test]
    fn test_try_apply_delta_remove_out_of_bounds() {
        let mut items = vec![1, 2, 3];
        let result = try_apply_delta(&mut items, Delta::remove(3));
        assert!(matches!(
            result,
            Err(DeltaError::IndexOutOfBounds {
                index: 3,
                len: 3,
                operation: "remove"
            })
        ));
    }

    #[test]
    fn test_try_apply_delta_update_out_of_bounds() {
        let mut items = vec![1, 2, 3];
        let result = try_apply_delta(&mut items, Delta::update(5, 10));
        assert!(matches!(
            result,
            Err(DeltaError::IndexOutOfBounds {
                index: 5,
                len: 3,
                operation: "update"
            })
        ));
    }

    #[test]
    fn test_delta_error_display() {
        let err = DeltaError::IndexOutOfBounds {
            index: 5,
            len: 3,
            operation: "insert",
        };
        let msg = format!("{err}");
        assert!(msg.contains("insert"));
        assert!(msg.contains("5"));
        assert!(msg.contains("3"));
    }
}

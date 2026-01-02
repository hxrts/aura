//! View Delta Reduction Infrastructure
//!
//! This module provides extensible view-level reduction for turning journal facts
//! into application/UI-level deltas. Domain crates register their view reducers,
//! and the runtime scheduler uses the registry to dispatch facts appropriately.
//!
//! # Architecture
//!
//! View delta reduction is separate from journal-level reduction (`FactReducer`):
//! - **Journal reduction** (`aura-journal`): Facts → `RelationalBinding` for storage
//! - **View reduction** (this module): Facts → View Deltas for UI updates
//!
//! # Pattern
//!
//! Domain crates export:
//! 1. Fact type implementing `DomainFact` (in their crate)
//! 2. Delta type for view updates (e.g., `ChatDelta`)
//! 3. View reducer implementing `ViewDeltaReducer`
//!
//! # Example
//!
//! ```ignore
//! // In aura-chat/src/view.rs:
//! #[derive(Debug, Clone)]
//! pub enum ChatDelta {
//!     ChannelAdded { id: String, name: String },
//!     MessageAdded { channel_id: String, content: String },
//! }
//!
//! pub struct ChatViewReducer;
//!
//! impl ViewDeltaReducer for ChatViewReducer {
//!     fn handles_type(&self) -> &'static str { "chat" }
//!
//!     fn reduce_fact(
//!         &self,
//!         binding_type: &str,
//!         binding_data: &[u8],
//!         _own_authority: Option<AuthorityId>,
//!     ) -> Vec<ViewDelta> {
//!         if binding_type != "chat" { return vec![]; }
//!         ChatFact::from_bytes(binding_data)
//!             .map(|fact| ChatDelta::from(fact).into())
//!             .into_iter()
//!             .flatten()
//!             .collect()
//!     }
//! }
//!
//! // Registration at runtime (in aura-agent):
//! registry.register("chat", Box::new(ChatViewReducer));
//! ```

use aura_core::identifiers::AuthorityId;
use std::any::Any;
use std::collections::HashMap;
use std::fmt::Debug;

/// Type-erased view delta that can hold any domain-specific delta type.
///
/// Domain crates wrap their concrete delta types in this for the registry.
pub type ViewDelta = Box<dyn Any + Send + Sync>;

/// Trait for reducing journal facts to view deltas.
///
/// Domain crates implement this to define how their facts are transformed
/// into view-level deltas for UI updates.
pub trait ViewDeltaReducer: Send + Sync {
    /// Returns the fact type ID this reducer handles.
    ///
    /// This should match the `type_id()` from `DomainFact`.
    fn handles_type(&self) -> &'static str;

    /// Reduce a serialized fact to view deltas.
    ///
    /// # Arguments
    /// * `binding_type` - The type identifier from `RelationalFact::Generic`
    /// * `binding_data` - The serialized fact data
    /// * `own_authority` - The current user's authority ID for contextual reduction.
    ///   For example, determining inbound vs outbound invitations.
    ///
    /// # Returns
    /// A vector of view deltas. Returns empty if the binding type doesn't match
    /// or if reduction fails.
    fn reduce_fact(
        &self,
        binding_type: &str,
        binding_data: &[u8],
        own_authority: Option<AuthorityId>,
    ) -> Vec<ViewDelta>;
}

/// Trait for deltas that can be losslessly (or intentionally) compacted.
///
/// The compaction behavior is defined by `try_merge`, which should preserve
/// the effective outcome of applying the two deltas in-order.
pub trait ComposableDelta: Sized {
    /// Key used to determine whether two deltas are merge candidates.
    type Key: PartialEq;

    /// Return a key that identifies the logical target of this delta.
    fn key(&self) -> Self::Key;

    /// Attempt to merge `other` into `self`.
    ///
    /// Returns `true` if `other` was merged and can be discarded.
    /// Returns `false` if the deltas must remain separate.
    fn try_merge(&mut self, other: Self) -> bool;
}

/// Compact deltas while preserving relative order.
///
/// This is an order-aware compactor: it only merges with the most recent prior
/// delta for the same key, preserving sequential semantics.
pub fn compact_deltas<T: ComposableDelta + Clone>(deltas: Vec<T>) -> Vec<T> {
    let mut output: Vec<T> = Vec::with_capacity(deltas.len());

    for delta in deltas {
        let key = delta.key();
        if let Some(pos) = output.iter().rposition(|existing| existing.key() == key) {
            let mut existing = output.remove(pos);
            if existing.try_merge(delta.clone()) {
                output.insert(pos, existing);
                continue;
            }
            output.insert(pos, existing);
        }
        output.push(delta);
    }

    output
}

/// Registry for domain view reducers.
///
/// The runtime scheduler uses this to dispatch facts to appropriate reducers.
#[derive(Default)]
pub struct ViewDeltaRegistry {
    /// Map from type_id string to reducer
    reducers: HashMap<String, Box<dyn ViewDeltaReducer>>,
}

impl ViewDeltaRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a view delta reducer for a fact type.
    ///
    /// # Arguments
    /// * `type_id` - The fact type identifier (e.g., "chat", "invitation")
    /// * `reducer` - The reducer that handles this fact type
    pub fn register(&mut self, type_id: &str, reducer: Box<dyn ViewDeltaReducer>) {
        self.reducers.insert(type_id.to_string(), reducer);
    }

    /// Check if a type_id has a registered reducer.
    pub fn is_registered(&self, type_id: &str) -> bool {
        self.reducers.contains_key(type_id)
    }

    /// Get the reducer for a given type_id.
    pub fn get_reducer(&self, type_id: &str) -> Option<&dyn ViewDeltaReducer> {
        self.reducers.get(type_id).map(|r| r.as_ref())
    }

    /// Reduce a fact using the appropriate registered reducer.
    ///
    /// # Arguments
    /// * `binding_type` - The fact type identifier
    /// * `binding_data` - The serialized fact data
    /// * `own_authority` - The current user's authority for contextual reduction
    ///
    /// If no reducer is registered for the binding_type, returns empty.
    pub fn reduce(
        &self,
        binding_type: &str,
        binding_data: &[u8],
        own_authority: Option<AuthorityId>,
    ) -> Vec<ViewDelta> {
        if let Some(reducer) = self.reducers.get(binding_type) {
            reducer.reduce_fact(binding_type, binding_data, own_authority)
        } else {
            Vec::new()
        }
    }

    /// Get all registered type IDs.
    pub fn registered_types(&self) -> impl Iterator<Item = &str> {
        self.reducers.keys().map(|s| s.as_str())
    }
}

impl Debug for ViewDeltaRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewDeltaRegistry")
            .field(
                "registered_types",
                &self.reducers.keys().collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// Helper trait for domain crates to convert their deltas to ViewDelta.
///
/// This provides a convenient way to box domain deltas.
pub trait IntoViewDelta: Any + Send + Sync + Sized {
    /// Convert self into a type-erased ViewDelta.
    fn into_view_delta(self) -> ViewDelta {
        Box::new(self)
    }
}

// Blanket implementation for all compatible types
impl<T: Any + Send + Sync + Sized> IntoViewDelta for T {}

/// Helper to downcast a ViewDelta back to a concrete type.
///
/// # Example
/// ```ignore
/// let delta: ViewDelta = ChatDelta::ChannelAdded { ... }.into_view_delta();
/// if let Some(chat_delta) = downcast_delta::<ChatDelta>(&delta) {
///     // Use chat_delta
/// }
/// ```
pub fn downcast_delta<T: 'static>(delta: &ViewDelta) -> Option<&T> {
    delta.downcast_ref::<T>()
}

/// Helper to downcast and take ownership of a ViewDelta.
pub fn downcast_delta_owned<T: 'static>(delta: ViewDelta) -> Option<T> {
    delta.downcast::<T>().ok().map(|b| *b)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test delta type
    #[derive(Debug, Clone, PartialEq)]
    enum TestDelta {
        ItemAdded { id: String },
        ItemRemoved { id: String },
    }

    impl ComposableDelta for TestDelta {
        type Key = String;

        fn key(&self) -> Self::Key {
            match self {
                TestDelta::ItemAdded { id } | TestDelta::ItemRemoved { id } => id.clone(),
            }
        }

        fn try_merge(&mut self, other: Self) -> bool {
            match (self, other) {
                (TestDelta::ItemAdded { id }, TestDelta::ItemAdded { id: other_id }) => {
                    *id = other_id;
                    true
                }
                (TestDelta::ItemRemoved { id }, TestDelta::ItemRemoved { id: other_id }) => {
                    *id = other_id;
                    true
                }
                _ => false,
            }
        }
    }

    // Test reducer
    struct TestReducer;

    impl ViewDeltaReducer for TestReducer {
        fn handles_type(&self) -> &'static str {
            "test"
        }

        fn reduce_fact(
            &self,
            binding_type: &str,
            binding_data: &[u8],
            _own_authority: Option<AuthorityId>,
        ) -> Vec<ViewDelta> {
            if binding_type != "test" {
                return vec![];
            }

            // Simple: treat binding_data as an ID string
            if let Ok(id) = std::str::from_utf8(binding_data) {
                vec![TestDelta::ItemAdded { id: id.to_string() }.into_view_delta()]
            } else {
                vec![]
            }
        }
    }

    #[test]
    fn test_compact_deltas_merges_by_key() {
        let deltas = vec![
            TestDelta::ItemAdded { id: "a".to_string() },
            TestDelta::ItemAdded { id: "a".to_string() },
            TestDelta::ItemRemoved { id: "b".to_string() },
            TestDelta::ItemRemoved { id: "b".to_string() },
        ];

        let compacted = compact_deltas(deltas);
        assert_eq!(
            compacted,
            vec![
                TestDelta::ItemAdded { id: "a".to_string() },
                TestDelta::ItemRemoved { id: "b".to_string() },
            ]
        );
    }

    #[test]
    fn test_registry_registration() {
        let mut registry = ViewDeltaRegistry::new();
        registry.register("test", Box::new(TestReducer));

        assert!(registry.is_registered("test"));
        assert!(!registry.is_registered("unknown"));
    }

    #[test]
    fn test_registry_reduce() {
        let mut registry = ViewDeltaRegistry::new();
        registry.register("test", Box::new(TestReducer));

        let deltas = registry.reduce("test", b"item123", None);
        assert_eq!(deltas.len(), 1);

        let delta = downcast_delta::<TestDelta>(&deltas[0]).unwrap();
        assert_eq!(
            delta,
            &TestDelta::ItemAdded {
                id: "item123".to_string()
            }
        );
    }

    #[test]
    fn test_registry_reduce_unknown_type() {
        let registry = ViewDeltaRegistry::new();
        let deltas = registry.reduce("unknown", b"data", None);
        assert!(deltas.is_empty());
    }

    #[test]
    fn test_into_view_delta() {
        let delta = TestDelta::ItemRemoved {
            id: "xyz".to_string(),
        };
        let view_delta = delta.clone().into_view_delta();

        let recovered = downcast_delta::<TestDelta>(&view_delta).unwrap();
        assert_eq!(recovered, &delta);
    }

    #[test]
    fn test_downcast_owned() {
        let delta = TestDelta::ItemAdded {
            id: "abc".to_string(),
        };
        let view_delta = delta.clone().into_view_delta();

        let recovered = downcast_delta_owned::<TestDelta>(view_delta).unwrap();
        assert_eq!(recovered, delta);
    }
}

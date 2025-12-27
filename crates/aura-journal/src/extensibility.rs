//! Extensible fact type infrastructure
//!
//! This module provides traits and a registry for domain crates to register
//! their own fact types without modifying `aura-journal`. This follows the
//! Open/Closed Principle: the journal is open for extension but closed for
//! modification.
//!
//! # Architecture
//!
//! **Journal-level reduction** (`FactReducer`): Facts → `RelationalBinding`
//! - Used for storage and journal queries
//! - Registered via `FactRegistry`
//!
//! **View-level reduction** lives in `aura-composition` (`ViewDeltaReducer`):
//! - Used for TUI/application layer
//! - Registered via `ViewDeltaRegistry`
//! - See `aura_composition::reactive::view_delta` module
//!
//! # Example
//!
//! ```ignore
//! // In aura-chat/src/facts.rs:
//! #[derive(Debug, Clone, Serialize, Deserialize)]
//! pub enum ChatFact {
//!     ChannelCreated { channel_id: ChannelId, name: String },
//!     MessageSent { channel_id: ChannelId, content: String },
//! }
//!
//! impl DomainFact for ChatFact {
//!     fn type_id(&self) -> &'static str { "chat" }
//!     fn context_id(&self) -> ContextId { /* derive from channel_id */ }
//!     fn to_bytes(&self) -> Vec<u8> { serde_json::to_vec(self).unwrap() }
//!     fn from_bytes(bytes: &[u8]) -> Option<Self> { serde_json::from_slice(bytes).ok() }
//! }
//!
//! // Registration at runtime:
//! registry.register::<ChatFact>(Box::new(ChatFactReducer));
//! ```

use crate::reduction::{RelationalBinding, RelationalBindingType};
use aura_core::identifiers::ContextId;
use std::any::TypeId;
use std::collections::HashMap;
use std::fmt::Debug;

pub use aura_core::facts::{decode_domain_fact, encode_domain_fact, FactEncoding, FactEnvelope};

/// Trait for domain-specific fact types
///
/// Domain crates implement this trait for their fact enums to enable
/// extensible fact handling without modifying `aura-journal`.
pub trait DomainFact: Debug + Clone + Send + Sync + 'static {
    /// Returns the type identifier for this fact domain
    ///
    /// This should be a unique string like "chat", "invitation", "contact".
    /// Used to distinguish fact types when stored as `RelationalFact::Generic`.
    fn type_id(&self) -> &'static str;

    /// Returns the context ID this fact belongs to
    ///
    /// Facts are scoped to relational contexts for isolation.
    fn context_id(&self) -> ContextId;

    /// Serialize the fact to bytes for storage
    ///
    /// Typically uses serde_json or bincode.
    fn to_bytes(&self) -> Vec<u8>;

    /// Deserialize a fact from bytes
    ///
    /// Returns None if deserialization fails.
    fn from_bytes(bytes: &[u8]) -> Option<Self>
    where
        Self: Sized;

    /// Convert to a Generic relational fact for storage
    fn to_generic(&self) -> crate::fact::RelationalFact {
        crate::fact::RelationalFact::Generic {
            context_id: self.context_id(),
            binding_type: self.type_id().to_string(),
            binding_data: self.to_bytes(),
        }
    }
}

/// Trait for reducing domain facts to relational bindings
///
/// Each domain crate implements this to define how its facts
/// are reduced to `RelationalBinding` during journal reduction.
pub trait FactReducer: Send + Sync {
    /// Returns the type ID this reducer handles
    fn handles_type(&self) -> &'static str;

    /// Reduce a domain fact (serialized as bytes) to a relational binding
    ///
    /// # Arguments
    /// * `context_id` - The context this fact belongs to
    /// * `binding_type` - The binding type string from the Generic fact
    /// * `binding_data` - The serialized fact data
    ///
    /// # Returns
    /// A `RelationalBinding` if reduction succeeds, or None if this reducer
    /// doesn't handle this binding type.
    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding>;
}

/// Registry for domain fact types and their reducers
///
/// The runtime uses this registry to:
/// 1. Serialize domain facts to Generic relational facts
/// 2. Reduce Generic relational facts using registered reducers
#[derive(Default)]
pub struct FactRegistry {
    /// Map from type_id string to reducer
    reducers: HashMap<String, Box<dyn FactReducer>>,
    /// Map from TypeId to type_id string for reverse lookup
    type_ids: HashMap<TypeId, &'static str>,
}

impl FactRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a domain fact type with its reducer
    ///
    /// # Arguments
    /// * `type_id` - The string identifier for this fact type (e.g., "chat")
    /// * `reducer` - The reducer that handles this fact type
    pub fn register<F: DomainFact>(
        &mut self,
        type_id: &'static str,
        reducer: Box<dyn FactReducer>,
    ) {
        self.reducers.insert(type_id.to_string(), reducer);
        self.type_ids.insert(TypeId::of::<F>(), type_id);
    }

    /// Check if a type_id is registered
    pub fn is_registered(&self, type_id: &str) -> bool {
        self.reducers.contains_key(type_id)
    }

    /// Get the reducer for a given type_id
    pub fn get_reducer(&self, type_id: &str) -> Option<&dyn FactReducer> {
        self.reducers.get(type_id).map(|r| r.as_ref())
    }

    /// Reduce a Generic relational fact using the registered reducer
    ///
    /// If no reducer is registered for the binding_type, returns a
    /// default Generic binding.
    pub fn reduce_generic(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> RelationalBinding {
        if let Some(reducer) = self.reducers.get(binding_type) {
            if let Some(binding) = reducer.reduce(context_id, binding_type, binding_data) {
                return binding;
            }
        }

        // Fallback: return a generic binding
        RelationalBinding {
            binding_type: RelationalBindingType::Generic(binding_type.to_string()),
            context_id,
            data: binding_data.to_vec(),
        }
    }

    /// Get all registered type IDs
    pub fn registered_types(&self) -> impl Iterator<Item = &str> {
        self.reducers.keys().map(|s| s.as_str())
    }
}

impl Debug for FactRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FactRegistry")
            .field(
                "registered_types",
                &self.reducers.keys().collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// Helper to create a domain fact from a Generic relational fact
///
/// # Type Parameters
/// * `F` - The domain fact type to deserialize to
///
/// # Arguments
/// * `binding_type` - The binding type from the Generic fact
/// * `binding_data` - The serialized fact data
/// * `expected_type` - The expected type_id string
///
/// # Returns
/// The deserialized domain fact, or None if the type doesn't match
/// or deserialization fails.
pub fn parse_generic_fact<F: DomainFact>(
    binding_type: &str,
    binding_data: &[u8],
    expected_type: &str,
) -> Option<F> {
    if binding_type != expected_type {
        return None;
    }
    F::from_bytes(binding_data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::ContextId;
    use serde::{Deserialize, Serialize};

    // Test domain fact type
    #[derive(Debug, Clone, Serialize, Deserialize)]
    enum TestFact {
        Created { id: String },
        Updated { id: String, value: u32 },
    }

    impl DomainFact for TestFact {
        fn type_id(&self) -> &'static str {
            "test"
        }

        fn context_id(&self) -> ContextId {
            ContextId::new_from_entropy([42u8; 32])
        }

        fn to_bytes(&self) -> Vec<u8> {
            serde_json::to_vec(self).unwrap()
        }

        fn from_bytes(bytes: &[u8]) -> Option<Self> {
            serde_json::from_slice(bytes).ok()
        }
    }

    // Test reducer
    struct TestFactReducer;

    impl FactReducer for TestFactReducer {
        fn handles_type(&self) -> &'static str {
            "test"
        }

        fn reduce(
            &self,
            context_id: ContextId,
            binding_type: &str,
            binding_data: &[u8],
        ) -> Option<RelationalBinding> {
            if binding_type != "test" {
                return None;
            }

            let fact: TestFact = serde_json::from_slice(binding_data).ok()?;
            let id = match &fact {
                TestFact::Created { id } => id.clone(),
                TestFact::Updated { id, .. } => id.clone(),
            };

            Some(RelationalBinding {
                binding_type: RelationalBindingType::Generic("test".to_string()),
                context_id,
                data: id.into_bytes(),
            })
        }
    }

    #[test]
    fn test_domain_fact_serialization() {
        let fact = TestFact::Created {
            id: "abc".to_string(),
        };
        let bytes = fact.to_bytes();
        let restored = TestFact::from_bytes(&bytes);
        assert!(restored.is_some());
    }

    #[test]
    fn test_to_generic() {
        let fact = TestFact::Created {
            id: "abc".to_string(),
        };
        let generic = fact.to_generic();

        if let crate::fact::RelationalFact::Generic {
            binding_type,
            binding_data,
            ..
        } = generic
        {
            assert_eq!(binding_type, "test");
            let restored = TestFact::from_bytes(&binding_data);
            assert!(restored.is_some());
        } else {
            panic!("Expected Generic variant");
        }
    }

    #[test]
    fn test_registry() {
        let mut registry = FactRegistry::new();
        registry.register::<TestFact>("test", Box::new(TestFactReducer));

        assert!(registry.is_registered("test"));
        assert!(!registry.is_registered("unknown"));
    }

    #[test]
    fn test_registry_reduce() {
        let mut registry = FactRegistry::new();
        registry.register::<TestFact>("test", Box::new(TestFactReducer));

        let fact = TestFact::Created {
            id: "xyz".to_string(),
        };
        let context_id = fact.context_id();
        let bytes = fact.to_bytes();

        let binding = registry.reduce_generic(context_id, "test", &bytes);
        assert_eq!(binding.data, b"xyz".to_vec());
    }

    #[test]
    fn test_parse_generic_fact() {
        let fact = TestFact::Updated {
            id: "foo".to_string(),
            value: 42,
        };
        let bytes = fact.to_bytes();

        let restored: Option<TestFact> = parse_generic_fact("test", &bytes, "test");
        assert!(restored.is_some());

        let wrong_type: Option<TestFact> = parse_generic_fact("wrong", &bytes, "test");
        assert!(wrong_type.is_none());
    }
}

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

pub use crate::fact::ProtocolFactKey;
pub use aura_core::types::facts::{
    decode_domain_fact, encode_domain_fact, FactEncoding, FactEnvelope,
};

/// Trait for domain-specific fact types
///
/// Domain crates implement this trait for their fact enums to enable
/// extensible fact handling without modifying `aura-journal`.
///
/// # Typed Storage (No Stringly-Typed Vec<u8>)
///
/// Facts are stored using `FactEnvelope` which provides:
/// - **Type safety**: `envelope.type_id` is always valid
/// - **No double serialization**: Only `envelope.payload` is raw bytes
/// - **Validation at construction**: Size and type checks in envelope
pub trait DomainFact: Debug + Clone + Send + Sync + 'static {
    /// Returns the type identifier for this fact domain
    ///
    /// This should be a unique string like "chat", "invitation", "contact".
    /// Used to distinguish fact types when stored as `RelationalFact::Generic`.
    fn type_id(&self) -> &'static str;

    /// Returns the schema version for this fact type
    ///
    /// Increment when making breaking changes to the fact structure.
    /// Default is 1 for backwards compatibility.
    fn schema_version(&self) -> u16 {
        1
    }

    /// Returns the context ID this fact belongs to
    ///
    /// Facts are scoped to relational contexts for isolation.
    fn context_id(&self) -> ContextId;

    /// Create a typed `FactEnvelope` for this fact.
    ///
    /// This is the primary serialization method. The envelope contains:
    /// - `type_id`: From `self.type_id()`
    /// - `schema_version`: From `self.schema_version()`
    /// - `encoding`: DAG-CBOR (default)
    /// - `payload`: The serialized fact data
    fn to_envelope(&self) -> FactEnvelope;

    /// Deserialize a fact from a `FactEnvelope`.
    ///
    /// Returns None if:
    /// - The type_id doesn't match
    /// - Deserialization fails
    fn from_envelope(envelope: &FactEnvelope) -> Option<Self>
    where
        Self: Sized;

    /// Serialize to raw bytes (convenience wrapper for `to_envelope().payload`).
    ///
    /// This is the payload portion of the envelope, suitable for hashing or
    /// embedding in other structures.
    fn to_bytes(&self) -> Vec<u8> {
        self.to_envelope().payload
    }

    /// Deserialize from raw bytes (convenience wrapper for direct deserialization).
    ///
    /// Note: This bypasses type_id and schema_version validation. Prefer
    /// `from_envelope` when you have a full envelope with metadata.
    fn from_bytes(bytes: &[u8]) -> Option<Self>
    where
        Self: Sized + serde::de::DeserializeOwned,
    {
        aura_core::util::serialization::from_slice(bytes).ok()
    }

    /// Convert to a Generic relational fact for storage
    fn to_generic(&self) -> crate::fact::RelationalFact {
        crate::fact::RelationalFact::Generic {
            context_id: self.context_id(),
            envelope: self.to_envelope(),
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

    /// Reduce a domain fact from its envelope to a relational binding
    ///
    /// # Arguments
    /// * `context_id` - The context this fact belongs to
    /// * `envelope` - The typed fact envelope
    ///
    /// # Returns
    /// A `RelationalBinding` if reduction succeeds, or None if this reducer
    /// doesn't handle this fact type.
    fn reduce_envelope(
        &self,
        context_id: ContextId,
        envelope: &FactEnvelope,
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
    /// If no reducer is registered for the envelope's type_id, returns a
    /// default Generic binding.
    pub fn reduce_envelope(
        &self,
        context_id: ContextId,
        envelope: &FactEnvelope,
    ) -> RelationalBinding {
        let type_id = envelope.type_id.as_str();
        if let Some(reducer) = self.reducers.get(type_id) {
            if let Some(binding) = reducer.reduce_envelope(context_id, envelope) {
                return binding;
            }
        }

        // Fallback: return a generic binding with the envelope's payload
        RelationalBinding {
            binding_type: RelationalBindingType::Generic(type_id.to_string()),
            context_id,
            data: envelope.payload.clone(),
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

/// Helper to create a domain fact from a `FactEnvelope`
///
/// # Type Parameters
/// * `F` - The domain fact type to deserialize to
///
/// # Arguments
/// * `envelope` - The typed fact envelope
/// * `expected_type` - The expected type_id string
///
/// # Returns
/// The deserialized domain fact, or None if the type doesn't match
/// or deserialization fails.
pub fn parse_envelope<F: DomainFact>(envelope: &FactEnvelope, expected_type: &str) -> Option<F> {
    if envelope.type_id.as_str() != expected_type {
        return None;
    }
    F::from_envelope(envelope)
}

/// Validate a list of fact type IDs for duplicates.
pub fn validate_type_ids(type_ids: &[&str]) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for type_id in type_ids {
        if type_id.is_empty() {
            return Err("fact type id cannot be empty".to_string());
        }
        if !seen.insert(*type_id) {
            return Err(format!("duplicate fact type id: {type_id}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::ContextId;
    use serde::{Deserialize, Serialize};

    // Test domain fact type
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

        fn to_envelope(&self) -> FactEnvelope {
            // SAFETY: TestFact serialization is deterministic and should never fail in tests.
            #[allow(clippy::expect_used)]
            let payload =
                aura_core::util::serialization::to_vec(self).expect("TestFact serialization");
            FactEnvelope {
                type_id: aura_core::types::facts::FactTypeId::from(self.type_id()),
                schema_version: self.schema_version(),
                encoding: FactEncoding::DagCbor,
                payload,
            }
        }

        fn from_envelope(envelope: &FactEnvelope) -> Option<Self> {
            if envelope.type_id.as_str() != "test" {
                return None;
            }
            aura_core::util::serialization::from_slice(&envelope.payload).ok()
        }
    }

    // Test reducer
    struct TestFactReducer;

    impl FactReducer for TestFactReducer {
        fn handles_type(&self) -> &'static str {
            "test"
        }

        fn reduce_envelope(
            &self,
            context_id: ContextId,
            envelope: &FactEnvelope,
        ) -> Option<RelationalBinding> {
            if envelope.type_id.as_str() != "test" {
                return None;
            }

            let fact: TestFact = TestFact::from_envelope(envelope)?;
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
    fn test_domain_fact_envelope_roundtrip() {
        let fact = TestFact::Created {
            id: "abc".to_string(),
        };
        let envelope = fact.to_envelope();
        let restored = TestFact::from_envelope(&envelope);
        assert_eq!(restored, Some(fact));
    }

    #[test]
    fn test_to_generic() {
        let fact = TestFact::Created {
            id: "abc".to_string(),
        };
        let generic = fact.to_generic();

        if let crate::fact::RelationalFact::Generic { envelope, .. } = generic {
            assert_eq!(envelope.type_id.as_str(), "test");
            let restored = TestFact::from_envelope(&envelope);
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
    fn test_registry_reduce_envelope() {
        let mut registry = FactRegistry::new();
        registry.register::<TestFact>("test", Box::new(TestFactReducer));

        let fact = TestFact::Created {
            id: "xyz".to_string(),
        };
        let context_id = fact.context_id();
        let envelope = fact.to_envelope();

        let binding = registry.reduce_envelope(context_id, &envelope);
        assert_eq!(binding.data, b"xyz".to_vec());
    }

    #[test]
    fn test_parse_envelope() {
        let fact = TestFact::Updated {
            id: "foo".to_string(),
            value: 42,
        };
        let envelope = fact.to_envelope();

        let restored: Option<TestFact> = parse_envelope(&envelope, "test");
        assert!(restored.is_some());

        // Wrong type should fail
        let mut wrong_envelope = envelope;
        wrong_envelope.type_id = aura_core::types::facts::FactTypeId::from("wrong");
        let wrong_type: Option<TestFact> = parse_envelope(&wrong_envelope, "test");
        assert!(wrong_type.is_none());
    }
}

//! Context propagation handlers
//!
//! This module provides stateless helpers for creating `EffectContext` values.
//! It intentionally defers to the canonical context type in `aura-core` to
//! avoid introducing alternate identity models in Layer 3.
//!
//! # Key Characteristics
//!
//! - **Stateless**: Context is passed explicitly, no ambient state
//! - **Single-party**: Context for one operation at a time
//! - **Context-free**: No assumptions about execution environment
//!
//! # Usage
//!
//! ```rust,ignore
//! use aura_effects::context::StandardContextHandler;
//! use aura_core::{AuthorityId, ContextId, ExecutionMode};
//!
//! let context = StandardContextHandler::new()
//!     .create_effect_context(authority_id, context_id, ExecutionMode::Production);
//! ```

use aura_core::effects::ExecutionMode;
use aura_core::identifiers::{AuthorityId, ContextId};
use std::collections::HashMap;

// Re-export EffectContext from aura_core for convenience
pub use aura_core::context::EffectContext;

/// Standard context handler
///
/// Provides utilities for creating and managing execution contexts.
/// This is a stateless handler that follows Layer 3 principles.
#[derive(Debug, Clone)]
pub struct StandardContextHandler;

impl StandardContextHandler {
    /// Create a new context handler
    pub fn new() -> Self {
        Self
    }

    /// Create an effect context.
    pub fn create_effect_context(
        &self,
        authority_id: AuthorityId,
        context_id: ContextId,
        execution_mode: ExecutionMode,
    ) -> EffectContext {
        EffectContext::new(authority_id, context_id, execution_mode)
    }

    /// Convenience for creating a context with only an authority.
    pub fn create_effect_context_for_authority(&self, authority_id: AuthorityId) -> EffectContext {
        EffectContext::with_authority(authority_id)
    }

    /// Validate context for required metadata fields.
    pub fn validate_context(&self, context: &EffectContext, required_fields: &[&str]) -> bool {
        required_fields
            .iter()
            .all(|field| context.get_metadata(field).is_some())
    }

    /// Merge metadata from multiple contexts.
    pub fn merge_metadata(&self, contexts: &[&EffectContext]) -> HashMap<String, String> {
        let mut merged = HashMap::new();
        for context in contexts {
            for (key, value) in context.metadata() {
                merged.insert(key.clone(), value.clone());
            }
        }
        merged
    }
}

impl Default for StandardContextHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::{AuthorityId, ContextId};

    #[test]
    fn test_effect_context_creation() {
        let authority_id = AuthorityId::new_from_entropy([1u8; 32]);
        let context_id = ContextId::new_from_entropy([2u8; 32]);

        let context = EffectContext::new(authority_id, context_id, ExecutionMode::Testing);

        assert_eq!(context.authority_id(), authority_id);
        assert_eq!(context.context_id(), context_id);
    }

    #[test]
    fn test_metadata_helpers() {
        let authority_id = AuthorityId::new_from_entropy([3u8; 32]);
        let context_id = ContextId::new_from_entropy([4u8; 32]);

        let mut context = EffectContext::new(authority_id, context_id, ExecutionMode::Testing);
        context.set_metadata("key1", "value1");
        context.set_metadata("key2", "value2");

        assert_eq!(context.get_metadata("key1"), Some(&"value1".to_string()));
        assert_eq!(context.get_metadata("key2"), Some(&"value2".to_string()));
    }

    #[test]
    fn test_standard_context_handler() {
        let handler = StandardContextHandler::new();
        let authority_id = AuthorityId::new_from_entropy([5u8; 32]);
        let context_id = ContextId::new_from_entropy([6u8; 32]);

        let context =
            handler.create_effect_context(authority_id, context_id, ExecutionMode::Testing);

        assert_eq!(context.authority_id(), authority_id);
        assert_eq!(context.context_id(), context_id);
    }

    #[test]
    fn test_context_validation() {
        let handler = StandardContextHandler::new();
        let authority_id = AuthorityId::new_from_entropy([7u8; 32]);
        let context_id = ContextId::new_from_entropy([8u8; 32]);

        let mut context = EffectContext::new(authority_id, context_id, ExecutionMode::Testing);
        context.set_metadata("custom_field", "value");

        assert!(handler.validate_context(&context, &["custom_field"]));
        assert!(!handler.validate_context(&context, &["missing_field"]));
    }

    #[test]
    fn test_metadata_merging() {
        let handler = StandardContextHandler::new();
        let authority_id = AuthorityId::new_from_entropy([9u8; 32]);
        let context_id = ContextId::new_from_entropy([10u8; 32]);

        let mut context1 = EffectContext::new(authority_id, context_id, ExecutionMode::Testing);
        context1.set_metadata("key1", "value1");
        context1.set_metadata("shared", "from_context1");

        let mut context2 = EffectContext::new(authority_id, context_id, ExecutionMode::Testing);
        context2.set_metadata("key2", "value2");
        context2.set_metadata("shared", "from_context2");

        let merged = handler.merge_metadata(&[&context1, &context2]);

        assert_eq!(merged.get("key1"), Some(&"value1".to_string()));
        assert_eq!(merged.get("key2"), Some(&"value2".to_string()));
        assert_eq!(merged.get("shared"), Some(&"from_context2".to_string()));
    }
}

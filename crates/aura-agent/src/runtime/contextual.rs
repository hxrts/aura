//! Contextual execution utilities
//!
//! Provides utilities for context-aware execution and context propagation
//! across effect operations in the authority-centric runtime.

use super::EffectContext;
use aura_core::identifiers::{AuthorityId, ContextId};

/// Trait for types that can execute within a context
pub trait Contextual {
    /// Execute within the given context
    async fn execute_with_context(&self, context: &EffectContext) -> ContextResult;
}

/// Trait for types that can create child contexts
pub trait ContextProvider {
    /// Create a new context
    fn create_context(&self, authority_id: AuthorityId, context_id: ContextId) -> EffectContext;

    /// Create a child context from an existing one
    fn create_child_context(&self, parent: &EffectContext, context_id: ContextId) -> EffectContext;
}

/// Result of contextual execution
#[derive(Debug, Clone)]
pub enum ContextResult {
    Success,
    Error(String),
    BudgetExceeded(String),
}

impl ContextResult {
    pub fn is_success(&self) -> bool {
        matches!(self, ContextResult::Success)
    }

    pub fn is_error(&self) -> bool {
        matches!(
            self,
            ContextResult::Error(_) | ContextResult::BudgetExceeded(_)
        )
    }
}

/// Context-aware wrapper for operations
#[derive(Debug)]
pub struct ContextualWrapper<T> {
    inner: T,
}

impl<T> ContextualWrapper<T> {
    pub fn new(inner: T) -> Self {
        Self { inner }
    }

    pub fn inner(&self) -> &T {
        &self.inner
    }

    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl<T> Contextual for ContextualWrapper<T>
where
    T: Send + Sync,
{
    async fn execute_with_context(&self, _context: &EffectContext) -> ContextResult {
        // Stub implementation
        ContextResult::Success
    }
}

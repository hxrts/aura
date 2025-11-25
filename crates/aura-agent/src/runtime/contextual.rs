//! Contextual execution utilities
//!
//! Provides utilities for context-aware execution and context propagation
//! across effect operations in the authority-centric runtime.

use super::EffectContext;
use aura_core::identifiers::{AuthorityId, ContextId};

/// Trait for types that can execute within a context
#[allow(dead_code)] // Part of future contextual execution API
pub trait Contextual {
    /// Execute within the given context
    async fn execute_with_context(&self, context: &EffectContext) -> ContextResult;
}

/// Trait for types that can create child contexts
#[allow(dead_code)] // Part of future contextual execution API
pub trait ContextProvider {
    /// Create a new context
    fn create_context(&self, authority_id: AuthorityId, context_id: ContextId) -> EffectContext;

    /// Create a child context from an existing one
    fn create_child_context(&self, parent: &EffectContext, context_id: ContextId) -> EffectContext;
}

/// Result of contextual execution
#[derive(Debug, Clone)]
#[allow(dead_code)] // Part of future contextual execution API
pub enum ContextResult {
    Success,
    Error(String),
    BudgetExceeded(String),
}

impl ContextResult {
    #[allow(dead_code)] // Part of future contextual execution API
    pub fn is_success(&self) -> bool {
        matches!(self, ContextResult::Success)
    }

    #[allow(dead_code)] // Part of future contextual execution API
    pub fn is_error(&self) -> bool {
        matches!(
            self,
            ContextResult::Error(_) | ContextResult::BudgetExceeded(_)
        )
    }
}

/// Context-aware wrapper for operations
#[derive(Debug)]
#[allow(dead_code)] // Part of future contextual execution API
pub struct ContextualWrapper<T> {
    inner: T,
}

impl<T> ContextualWrapper<T> {
    #[allow(dead_code)] // Part of future contextual execution API
    pub fn new(inner: T) -> Self {
        Self { inner }
    }

    #[allow(dead_code)] // Part of future contextual execution API
    pub fn inner(&self) -> &T {
        &self.inner
    }

    #[allow(dead_code)] // Part of future contextual execution API
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

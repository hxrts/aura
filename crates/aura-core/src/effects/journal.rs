//! Journal effect interface for CRDT operations

use crate::{AuraError, Journal};
use async_trait::async_trait;

/// Pure trait for journal/CRDT operations
#[async_trait]
pub trait JournalEffects {
    /// Merge facts using join semilattice operation
    async fn merge_facts(&self, target: &Journal, delta: &Journal) -> Result<Journal, AuraError>;

    /// Refine capabilities using meet semilattice operation  
    async fn refine_caps(
        &self,
        target: &Journal,
        refinement: &Journal,
    ) -> Result<Journal, AuraError>;

    /// Get current journal state
    async fn get_journal(&self) -> Result<Journal, AuraError>;

    /// Persist journal state
    async fn persist_journal(&self, journal: &Journal) -> Result<(), AuraError>;
}

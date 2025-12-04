//! # Query Executor
//!
//! Executes TuiQuery instances and manages subscriptions to query results.
//! Maintains an in-memory journal built from fact streams for efficient queries.
//!
//! ## Architecture
//!
//! This executor handles queries for data stored in `RelationalFact`:
//! - Guardian bindings (`GuardianBinding`)
//! - Recovery grants (`RecoveryGrant`)
//!
//! For chat and invitation data, use the reactive view system instead:
//! - `ChatView` receives `ChatDelta` from `ChatViewReducer` in `aura-chat`
//! - `InvitationsView` receives `InvitationDelta` from `InvitationViewReducer` in `aura-invitation`
//!
//! See `views.rs` for the delta-based reactive views.

use aura_journal::fact::{Fact, FactContent, Journal, JournalNamespace, RelationalFact};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use super::queries::{
    Guardian, GuardianApproval, GuardianStatus, GuardiansQuery, RecoveryQuery, RecoveryState,
    RecoveryStatus,
};

/// Query executor that manages query execution and subscriptions
pub struct QueryExecutor {
    /// In-memory journal for queries (built from fact stream)
    journal: Arc<RwLock<Journal>>,
    /// Update broadcaster
    update_tx: broadcast::Sender<DataUpdate>,
}

/// Updates emitted when data changes
#[derive(Debug, Clone)]
pub enum DataUpdate {
    /// Guardians were updated
    GuardiansUpdated,
    /// Recovery state was updated
    RecoveryUpdated,
}

impl QueryExecutor {
    /// Create a new query executor with an in-memory journal
    ///
    /// The journal is initialized with a generic namespace.
    /// Facts should be added via `add_facts()` from the fact stream.
    pub fn new() -> Self {
        let (update_tx, _) = broadcast::channel(256);
        // Create a generic journal namespace for TUI queries
        // Full deployment ties this to the current user's authority
        let namespace =
            JournalNamespace::Authority(aura_core::AuthorityId::new_from_entropy([0u8; 32]));
        let journal = Journal::new(namespace);

        Self {
            journal: Arc::new(RwLock::new(journal)),
            update_tx,
        }
    }

    /// Add facts to the query executor's journal
    ///
    /// This should be called whenever new facts are committed.
    /// Typically connected to the FactStreamAdapter.
    pub async fn add_facts(&self, facts: Vec<Fact>) -> Result<(), String> {
        let mut journal = self.journal.write().await;
        for fact in facts {
            journal
                .add_fact(fact)
                .map_err(|e| format!("Failed to add fact: {}", e))?;
        }
        Ok(())
    }

    /// Execute a guardians query
    ///
    /// Extracts GuardianBinding facts from the journal and reduces them to Guardian types.
    pub async fn execute_guardians_query(
        &self,
        query: &GuardiansQuery,
    ) -> Result<Vec<Guardian>, String> {
        let journal = self.journal.read().await;

        // Extract all GuardianBinding facts
        let mut guardians: Vec<Guardian> = journal
            .iter_facts()
            .filter_map(|fact| match &fact.content {
                FactContent::Relational(RelationalFact::GuardianBinding {
                    account_id: _,
                    guardian_id,
                    binding_hash: _,
                }) => {
                    // Convert guardian_id to string
                    let guardian_id_str = format!("{:?}", guardian_id);

                    // Get timestamp from fact
                    let added_at = match &fact.timestamp {
                        aura_core::time::TimeStamp::PhysicalClock(physical) => physical.ts_ms,
                        _ => 0,
                    };

                    Some(Guardian {
                        authority_id: guardian_id_str.clone(),
                        name: format!(
                            "Guardian {}",
                            &guardian_id_str[..8.min(guardian_id_str.len())]
                        ),
                        status: GuardianStatus::Active, // Default to active
                        added_at,
                        last_seen: Some(added_at),
                        share_index: None, // Would be populated from FROST facts
                    })
                }
                _ => None,
            })
            .collect();

        // Apply query filters
        if let Some(status) = &query.status {
            guardians.retain(|g| g.status == *status);
        }

        if query.with_shares_only {
            guardians.retain(|g| g.share_index.is_some());
        }

        Ok(guardians)
    }

    /// Execute a recovery query
    ///
    /// Extracts RecoveryGrant facts from the journal and derives recovery status.
    /// Currently returns a basic status; full recovery session tracking would require
    /// additional fact types for recovery initiation and guardian approvals.
    pub async fn execute_recovery_query(
        &self,
        _query: &RecoveryQuery,
    ) -> Result<RecoveryStatus, String> {
        let journal = self.journal.read().await;

        // Extract RecoveryGrant facts
        let recovery_grants: Vec<_> = journal
            .iter_facts()
            .filter_map(|fact| match &fact.content {
                FactContent::Relational(RelationalFact::RecoveryGrant {
                    account_id: _,
                    guardian_id,
                    grant_hash: _,
                }) => Some(*guardian_id),
                _ => None,
            })
            .collect();

        // Also get guardian bindings to calculate threshold
        let guardian_bindings: Vec<_> = journal
            .iter_facts()
            .filter_map(|fact| match &fact.content {
                FactContent::Relational(RelationalFact::GuardianBinding {
                    guardian_id, ..
                }) => Some(*guardian_id),
                _ => None,
            })
            .collect();

        let total_guardians = guardian_bindings.len() as u32;
        let threshold = if total_guardians > 0 {
            total_guardians.div_ceil(2) // Simple majority (n+1)/2
        } else {
            0
        };

        // Build guardian approvals from grants
        let approvals: Vec<GuardianApproval> = recovery_grants
            .iter()
            .map(|guardian_id| GuardianApproval {
                guardian_id: format!("{:?}", guardian_id),
                guardian_name: format!("Guardian {:?}", guardian_id),
                approved: true,
                timestamp: None,
            })
            .collect();

        let approvals_received = approvals.len() as u32;

        // Determine recovery state
        let state = if approvals_received == 0 {
            RecoveryState::None
        } else if approvals_received >= threshold {
            RecoveryState::ThresholdMet
        } else {
            RecoveryState::Initiated
        };

        Ok(RecoveryStatus {
            session_id: None,
            state,
            approvals_received,
            threshold,
            total_guardians,
            approvals,
            started_at: None,
            expires_at: None,
            error: None,
        })
    }

    /// Subscribe to data updates
    pub fn subscribe(&self) -> broadcast::Receiver<DataUpdate> {
        self.update_tx.subscribe()
    }
}

impl Default for QueryExecutor {
    fn default() -> Self {
        Self::new()
    }
}

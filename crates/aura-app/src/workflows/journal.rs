//! Helpers for journal fact encoding and persistence.

use super::error::{fact_encoding, journal_op};
use aura_core::effects::JournalEffects;
use aura_core::{AuraError, FactValue, Journal};
use aura_journal::fact::{FactContent, RelationalFact};
use aura_maintenance::MaintenanceFact;

/// Encode FactContent into a FactValue with JSON serialization.
pub fn encode_fact_content(content: FactContent) -> Result<FactValue, AuraError> {
    serde_json::to_vec(&content)
        .map(FactValue::Bytes)
        .map_err(|e| fact_encoding(e).into())
}

/// Commit a maintenance fact to the local journal under `fact_key`, wrapped
/// as a relational fact in the fact's own context.
pub async fn persist_maintenance_fact<E: JournalEffects>(
    effects: &E,
    fact: &MaintenanceFact,
    fact_key: String,
) -> Result<(), AuraError> {
    let envelope = fact.to_envelope().map_err(fact_encoding)?;
    let content = FactContent::Relational(RelationalFact::Generic {
        context_id: fact.context_id(),
        envelope,
    });
    persist_fact_value(effects, fact_key, encode_fact_content(content)?).await
}

/// Maintenance facts held in the local journal, in journal key order.
/// Entries that are not maintenance facts are skipped.
pub async fn read_maintenance_facts<E: JournalEffects>(
    effects: &E,
) -> Result<Vec<MaintenanceFact>, AuraError> {
    let journal = effects
        .get_journal()
        .await
        .map_err(|e| journal_op("load journal", e))?;
    Ok(journal
        .facts
        .iter()
        .filter_map(|(_, value)| match value {
            FactValue::Bytes(bytes) => serde_json::from_slice::<FactContent>(bytes).ok(),
            _ => None,
        })
        .filter_map(|content| match content {
            FactContent::Relational(RelationalFact::Generic { envelope, .. }) => {
                MaintenanceFact::from_envelope(&envelope).ok()
            }
            _ => None,
        })
        .collect())
}

/// Merge a single fact into the journal and persist it.
pub async fn persist_fact_value<E: JournalEffects>(
    effects: &E,
    fact_key: String,
    fact_value: FactValue,
) -> Result<(), AuraError> {
    let mut delta = Journal::new();
    delta.facts.insert(fact_key, fact_value)?;

    let current = effects
        .get_journal()
        .await
        .map_err(|e| journal_op("load journal", e))?;
    let merged = effects
        .merge_facts(current, delta)
        .await
        .map_err(|e| journal_op("merge facts", e))?;
    effects
        .persist_journal(&merged)
        .await
        .map_err(|e| journal_op("persist journal", e))?;

    Ok(())
}

//! Helpers for journal fact encoding and persistence.

use super::error::{fact_encoding, journal_op};
use aura_core::effects::JournalEffects;
use aura_core::{AuraError, FactValue, Journal};
use aura_journal::fact::FactContent;

/// Encode FactContent into a FactValue with JSON serialization.
pub fn encode_fact_content(content: FactContent) -> Result<FactValue, AuraError> {
    serde_json::to_vec(&content)
        .map(FactValue::Bytes)
        .map_err(|e| fact_encoding(e).into())
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

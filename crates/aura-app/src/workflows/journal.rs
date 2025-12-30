//! Helpers for journal fact encoding and persistence.

use aura_core::effects::JournalEffects;
use aura_core::identifiers::ContextId;
use aura_core::{AuraError, FactValue, Journal};
use aura_journal::fact::{FactContent, RelationalFact};
use serde::Serialize;

/// Encode FactContent into a FactValue with JSON serialization.
pub fn encode_fact_content(content: FactContent) -> Result<FactValue, AuraError> {
    serde_json::to_vec(&content)
        .map(FactValue::Bytes)
        .map_err(|e| AuraError::agent(format!("Failed to encode fact content: {e}")))
}

/// Encode a generic relational fact payload into a FactValue.
pub fn encode_relational_generic<T: Serialize>(
    context_id: ContextId,
    kind: &str,
    payload: &T,
) -> Result<FactValue, AuraError> {
    let content = FactContent::Relational(RelationalFact::Generic {
        context_id,
        binding_type: kind.to_string(),
        binding_data: serde_json::to_vec(payload)
            .map_err(|e| AuraError::agent(format!("Failed to serialize fact payload: {e}")))?,
    });

    encode_fact_content(content)
}

/// Merge a single fact into the journal and persist it.
pub async fn persist_fact_value<E: JournalEffects>(
    effects: &E,
    fact_key: String,
    fact_value: FactValue,
) -> Result<(), AuraError> {
    let mut delta = Journal::new();
    delta.facts.insert(fact_key, fact_value);

    let current = effects
        .get_journal()
        .await
        .map_err(|e| AuraError::agent(format!("Failed to load journal: {e}")))?;
    let merged = effects
        .merge_facts(&current, &delta)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to merge journal facts: {e}")))?;
    effects
        .persist_journal(&merged)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to persist journal: {e}")))?;

    Ok(())
}

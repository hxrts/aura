//! Helpers for journal fact encoding and persistence.

use super::error::{fact_encoding, journal_op};
use aura_core::effects::JournalEffects;
use aura_core::types::identifiers::ContextId;
use aura_core::{AuraError, FactValue, Journal};
use aura_journal::fact::{FactContent, RelationalFact};
use serde::Serialize;

/// Encode FactContent into a FactValue with JSON serialization.
pub fn encode_fact_content(content: FactContent) -> Result<FactValue, AuraError> {
    serde_json::to_vec(&content)
        .map(FactValue::Bytes)
        .map_err(|e| fact_encoding(e).into())
}

/// Encode a generic relational fact payload into a FactValue.
pub fn encode_relational_generic<T: Serialize>(
    context_id: ContextId,
    kind: &str,
    payload: &T,
) -> Result<FactValue, AuraError> {
    let payload_bytes = serde_json::to_vec(payload).map_err(fact_encoding)?;

    let envelope = aura_core::types::facts::FactEnvelope {
        type_id: aura_core::types::facts::FactTypeId::from(kind),
        schema_version: 1,
        encoding: aura_core::types::facts::FactEncoding::Json,
        payload: payload_bytes,
    };

    let content = FactContent::Relational(RelationalFact::Generic {
        context_id,
        envelope,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    struct RejectEncoding;
    impl Serialize for RejectEncoding {
        fn serialize<S: serde::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("payload refused encoding"))
        }
    }

    #[test]
    fn relational_encoding_retains_actual_serializer_failure() {
        let error = encode_relational_generic(
            ContextId::new_from_entropy([9; 32]),
            "test.encoding",
            &RejectEncoding,
        )
        .unwrap_err();
        let workflow = error
            .source()
            .unwrap()
            .downcast_ref::<super::super::error::WorkflowError>()
            .unwrap();
        assert!(matches!(
            workflow,
            super::super::error::WorkflowError::FactEncoding { .. }
        ));
        let context = workflow
            .source()
            .unwrap()
            .downcast_ref::<AuraError>()
            .unwrap();
        assert_eq!(context.category(), "serialization");
        let codec = context
            .source()
            .unwrap()
            .downcast_ref::<serde_json::Error>()
            .unwrap();
        assert!(codec.is_data());
        assert_eq!(codec.to_string(), "payload refused encoding");
    }
}

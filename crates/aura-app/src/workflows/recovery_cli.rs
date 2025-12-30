//! CLI-oriented recovery helpers (journal + protocol wiring).

use std::sync::Arc;

use aura_core::effects::{JournalEffects, NetworkEffects, PhysicalTimeEffects, TimeEffects};
use aura_core::identifiers::ContextId;
use aura_core::{AuraError, FactValue, Hash32};
use aura_relational::RelationalContext;
use aura_recovery::recovery_protocol::{RecoveryProtocol, RecoveryProtocolHandler, RecoveryRequest};
use serde::Serialize;
use crate::workflows::journal::{encode_relational_generic, persist_fact_value};

/// Run the recovery protocol initiation sequence.
pub async fn initiate_recovery_protocol<E: PhysicalTimeEffects + NetworkEffects + JournalEffects>(
    effects: &E,
    account_authority: aura_core::AuthorityId,
    guardian_authorities: Vec<aura_core::AuthorityId>,
    threshold: u32,
    request: RecoveryRequest,
) -> Result<(), AuraError> {
    let recovery_context = Arc::new(RelationalContext::new(guardian_authorities.clone()));
    let recovery_protocol = RecoveryProtocol::new(
        recovery_context,
        account_authority,
        guardian_authorities,
        threshold,
    );
    let protocol_handler = RecoveryProtocolHandler::new(Arc::new(recovery_protocol));
    protocol_handler
        .handle_recovery_initiation(request, effects, effects, effects)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to initiate recovery protocol: {e}")))?;
    Ok(())
}

/// Write a generic recovery fact into the journal.
pub async fn record_recovery_fact<T: Serialize, E: JournalEffects>(
    effects: &E,
    context_id: ContextId,
    fact_key: String,
    kind: &str,
    payload: &T,
) -> Result<(), AuraError> {
    let fact_value = encode_relational_generic(context_id, kind, payload)?;
    persist_fact_value(effects, fact_key, fact_value).await?;

    Ok(())
}

/// List recovery-related fact keys for status reporting.
pub async fn list_recovery_fact_keys<E: JournalEffects>(
    effects: &E,
) -> Result<(Vec<String>, Vec<String>), AuraError> {
    let current = effects
        .get_journal()
        .await
        .map_err(|e| AuraError::agent(format!("Failed to get journal: {e}")))?;

    let recovery_facts: Vec<String> = current
        .facts
        .keys()
        .filter(|key| key.contains("recovery") || key == "emergency_recovery_initiated")
        .collect();
    let completed_facts: Vec<String> = current
        .facts
        .keys()
        .filter(|key| key == "emergency_recovery_completed")
        .collect();

    Ok((recovery_facts, completed_facts))
}

/// Record a recovery dispute fact after validating the dispute window.
pub async fn record_recovery_dispute<T: Serialize, E: JournalEffects + TimeEffects>(
    effects: &E,
    context_id: ContextId,
    evidence_id: &str,
    guardian_authority: aura_core::AuthorityId,
    dispute: &T,
) -> Result<String, AuraError> {
    let dispute_journal = effects
        .get_journal()
        .await
        .map_err(|e| AuraError::agent(format!("Failed to get journal: {e}")))?;

    let evidence_key = format!("recovery_evidence.{evidence_id}");
    if let Some(value) = dispute_journal.facts.get(&evidence_key) {
        let evidence_json: serde_json::Value = match value {
            FactValue::String(data) => serde_json::from_str(data),
            FactValue::Bytes(bytes) => serde_json::from_slice(bytes),
            _ => Ok(serde_json::Value::Null),
        }
        .map_err(|e| AuraError::agent(format!("Failed to parse evidence JSON: {e}")))?;

        if let Some(dispute_window_ends) = evidence_json
            .get("dispute_window_ends_at_ms")
            .and_then(|v| v.as_u64())
        {
            let current_time = effects.current_timestamp().await;
            if current_time > dispute_window_ends {
                return Err(AuraError::agent(format!(
                    "Dispute window has closed for evidence {evidence_id}"
                )));
            }
        }
    }

    let existing_dispute_key = format!("recovery_dispute.{evidence_id}.{guardian_authority}");
    if dispute_journal.facts.contains_key(&existing_dispute_key) {
        return Err(AuraError::agent(format!(
            "Guardian {guardian_authority} has already filed a dispute for evidence {evidence_id}"
        )));
    }

    let dispute_key = format!("recovery_dispute.{}.{}", evidence_id, guardian_authority);
    record_recovery_fact(
        effects,
        context_id,
        dispute_key.clone(),
        "recovery_dispute",
        dispute,
    )
    .await?;

    Ok(dispute_key)
}

/// Build a deterministic recovery request for protocol initiation.
pub fn build_protocol_request(
    account_authority: aura_core::AuthorityId,
    commitment: Hash32,
    new_public_key: Vec<u8>,
    justification: String,
) -> RecoveryRequest {
    RecoveryRequest {
        recovery_id: account_authority.to_string(),
        account_authority,
        new_tree_commitment: commitment,
        operation: aura_recovery::recovery_protocol::RecoveryOperation::ReplaceTree {
            new_public_key,
        },
        justification,
    }
}

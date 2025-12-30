//! Delta fact application for join-semilattice updates
//!
//! This module implements atomic delta fact application that integrates with
//! aura-journal's CRDT system. It ensures that protocol execution results in
//! monotonic fact accumulation following join-semilattice laws.

mod compensation;
mod conversion;
mod operation;
mod parsers;
mod validation;

use aura_core::TimeEffects;
use aura_core::{AuraError, AuraResult};
use serde_json::Value as JsonValue;
use tracing::{debug, error, info, instrument, warn};

// Re-export public types
pub use operation::JournalOperation;

use compensation::generate_compensation_fact;
use conversion::{convert_to_journal_operation, journal_from_json_fact};
use validation::{infer_fact_type, validate_delta_facts};

/// Apply delta facts to the journal atomically
///
/// This function implements the "join-only commits" principle from the formal model.
/// Facts are accumulated monotonically in the journal CRDT, ensuring convergence
/// across replicas while preserving join-semilattice properties.
///
/// Timing is captured via the tracing span (subscriber handles `Instant::now()`).
#[instrument(skip(effect_system), fields(fact_count = delta_facts.len()))]
pub async fn apply_delta_facts<E: aura_core::effects::JournalEffects + TimeEffects>(
    delta_facts: &[JsonValue],
    effect_system: &E,
) -> AuraResult<Vec<JsonValue>> {
    if delta_facts.is_empty() {
        debug!("No delta facts to apply");
        return Ok(Vec::new());
    }

    debug!("Starting atomic delta fact application");

    // Validate facts before application (fail fast)
    let validated_facts = validate_delta_facts(delta_facts)?;

    // Apply facts atomically to the journal
    let mut applied_facts = Vec::new();

    for (index, fact) in validated_facts.iter().enumerate() {
        match apply_single_fact(fact, effect_system).await {
            Ok(applied_fact) => {
                applied_facts.push(applied_fact);
                debug!(
                    fact_index = index,
                    fact_type = infer_fact_type(fact),
                    "Fact applied successfully"
                );
            }
            Err(e) => {
                error!(
                    fact_index = index,
                    error = %e,
                    "Failed to apply fact, rolling back"
                );

                // Attempt rollback (best effort)
                if let Err(rollback_error) =
                    rollback_applied_facts(&applied_facts, effect_system).await
                {
                    error!(
                        rollback_error = %rollback_error,
                        "Rollback failed - journal may be in inconsistent state"
                    );
                }

                return Err(AuraError::internal(format!(
                    "Delta application failed at fact {index}: {e}"
                )));
            }
        }
    }

    info!(
        facts_applied = applied_facts.len(),
        "Delta facts applied successfully"
    );

    Ok(applied_facts)
}

/// Apply a single fact to the journal
async fn apply_single_fact<E: aura_core::effects::JournalEffects + TimeEffects>(
    fact: &JsonValue,
    effect_system: &E,
) -> AuraResult<JsonValue> {
    // Convert the JSON fact to the appropriate journal operation
    let journal_operation = convert_to_journal_operation(fact)?;

    // Apply to journal via effect system
    effect_system
        .apply_journal_operation(journal_operation)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to apply journal operation: {e}")))?;

    // Return the applied fact (possibly with additional metadata)
    Ok(fact.clone())
}

/// Rollback applied facts (best effort)
async fn rollback_applied_facts<E: aura_core::effects::JournalEffects + TimeEffects>(
    applied_facts: &[JsonValue],
    effect_system: &E,
) -> AuraResult<()> {
    warn!(
        facts_to_rollback = applied_facts.len(),
        "Attempting to rollback applied facts"
    );

    // Note: In a true CRDT system, rollback is not possible due to monotonicity.
    // This function exists for completeness but in practice, the journal
    // should use compensation patterns rather than rollbacks.

    // Implement compensation patterns for failed operations
    for (index, fact) in applied_facts.iter().enumerate() {
        error!(
            fact_index = index,
            fact = %fact,
            "Fact applied but sequence failed - applying compensation"
        );

        // Generate compensation fact based on fact type
        if let Some(compensation_fact) = generate_compensation_fact(fact, effect_system).await? {
            tracing::info!("Applying compensation fact: {}", compensation_fact);
            if let Err(compensation_error) =
                merge_json_fact(effect_system, &compensation_fact).await
            {
                error!(
                    compensation_fact = %compensation_fact,
                    error = %compensation_error,
                    "Failed to apply compensation fact"
                );
            } else {
                info!(
                    compensation_fact = %compensation_fact,
                    "Applied compensation fact for failed operation"
                );
            }
        }
    }

    Ok(())
}

async fn merge_json_fact<E: aura_core::effects::JournalEffects + TimeEffects>(
    effect_system: &E,
    fact: &JsonValue,
) -> AuraResult<()> {
    let current = effect_system
        .get_journal()
        .await
        .map_err(|e| AuraError::internal(format!("Failed to load journal: {e}")))?;
    let delta = journal_from_json_fact(fact)?;

    let merged = effect_system
        .merge_facts(&current, &delta)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to merge journal: {e}")))?;

    effect_system
        .persist_journal(&merged)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to persist journal: {e}")))?;

    Ok(())
}

/// Extension trait for journal-capable effect systems to support journal operations
trait JournalOperationExt {
    async fn apply_journal_operation(&self, operation: JournalOperation) -> AuraResult<()>;
}

impl<E: aura_core::effects::JournalEffects + TimeEffects> JournalOperationExt for E {
    async fn apply_journal_operation(&self, operation: JournalOperation) -> AuraResult<()> {
        debug!(operation = ?operation, "Applying journal operation");

        // Apply operation via CRDT journal effects
        match operation {
            JournalOperation::RegisterDevice {
                device_id,
                metadata,
            } => {
                // Create device registration fact for journal merge
                let mut device_fact_map = serde_json::Map::new();
                device_fact_map.insert(
                    "type".to_string(),
                    JsonValue::String("device_registration".to_string()),
                );
                device_fact_map.insert("device_id".to_string(), JsonValue::String(device_id));
                device_fact_map.insert("metadata".to_string(), metadata);
                device_fact_map.insert(
                    "timestamp".to_string(),
                    JsonValue::Number(serde_json::Number::from(
                        self.physical_time()
                            .await
                            .map_err(|e| AuraError::internal(format!("time error: {e}")))?
                            .ts_ms,
                    )),
                );
                let device_fact = JsonValue::Object(device_fact_map);

                tracing::info!("Applying device registration fact: {}", device_fact);
                merge_json_fact(self, &device_fact).await?;
            }

            JournalOperation::GrantCapability {
                capability,
                target_device,
                expiry,
            } => {
                // Create capability grant fact
                let mut cap_fact = serde_json::Map::new();
                cap_fact.insert(
                    "type".to_string(),
                    JsonValue::String("capability_grant".to_string()),
                );
                cap_fact.insert("capability".to_string(), JsonValue::String(capability));
                cap_fact.insert(
                    "target_device".to_string(),
                    JsonValue::String(target_device),
                );
                if let Some(expiry_time) = expiry {
                    cap_fact.insert(
                        "expiry".to_string(),
                        JsonValue::Number(serde_json::Number::from(expiry_time)),
                    );
                }

                tracing::info!(
                    "Applying capability grant fact: {}",
                    JsonValue::Object(cap_fact.clone())
                );
                merge_json_fact(self, &JsonValue::Object(cap_fact)).await?;
            }

            JournalOperation::AttestSession {
                session_id,
                attestation,
            } => {
                // Create session attestation fact
                let mut session_fact_map = serde_json::Map::new();
                session_fact_map.insert(
                    "type".to_string(),
                    JsonValue::String("session_attestation".to_string()),
                );
                session_fact_map.insert("session_id".to_string(), JsonValue::String(session_id));
                session_fact_map.insert("attestation".to_string(), attestation);
                let session_fact = JsonValue::Object(session_fact_map);

                tracing::info!("Applying session attestation fact: {}", session_fact);
                merge_json_fact(self, &session_fact).await?;
            }

            JournalOperation::FinalizeIntent { intent_id, result } => {
                // Create intent finalization fact
                let mut intent_fact_map = serde_json::Map::new();
                intent_fact_map.insert(
                    "type".to_string(),
                    JsonValue::String("intent_finalization".to_string()),
                );
                intent_fact_map.insert("intent_id".to_string(), JsonValue::String(intent_id));
                intent_fact_map.insert("result".to_string(), result);
                let intent_fact = JsonValue::Object(intent_fact_map);

                tracing::info!("Applying intent finalization fact: {}", intent_fact);
                merge_json_fact(self, &intent_fact).await?;
            }
            _ => {
                tracing::warn!("Unsupported journal operation: {:?}", operation);
                return Err(AuraError::invalid("Unsupported journal operation"));
            }
        }

        Ok(())
    }
}

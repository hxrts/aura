//! Delta fact application for join-semilattice updates
//!
//! This module implements atomic delta fact application that integrates with
//! aura-journal's CRDT system. It ensures that protocol execution results in
//! monotonic fact accumulation following join-semilattice laws.

use crate::effects::{system::AuraEffectSystem, JournalEffects};
use aura_core::{AuraError, AuraResult};
use serde_json::Value as JsonValue;
use std::time::Instant;
use tracing::{debug, error, info, warn};

/// Apply delta facts to the journal atomically
///
/// This function implements the "join-only commits" principle from the formal model.
/// Facts are accumulated monotonically in the journal CRDT, ensuring convergence
/// across replicas while preserving join-semilattice properties.
pub async fn apply_delta_facts(
    delta_facts: &[JsonValue],
    effect_system: &mut AuraEffectSystem,
) -> AuraResult<Vec<JsonValue>> {
    if delta_facts.is_empty() {
        debug!("No delta facts to apply");
        return Ok(Vec::new());
    }

    let start_time = Instant::now();

    debug!(
        fact_count = delta_facts.len(),
        "Starting atomic delta fact application"
    );

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

                return Err(AuraError::internal(&format!(
                    "Delta application failed at fact {}: {}",
                    index, e
                )));
            }
        }
    }

    let application_time = start_time.elapsed();

    info!(
        facts_applied = applied_facts.len(),
        application_time_us = application_time.as_micros(),
        "Delta facts applied successfully"
    );

    Ok(applied_facts)
}

/// Validate delta facts before application
fn validate_delta_facts(facts: &[JsonValue]) -> AuraResult<&[JsonValue]> {
    for (index, fact) in facts.iter().enumerate() {
        if !is_valid_fact_format(fact) {
            return Err(AuraError::invalid(&format!(
                "Invalid fact format at index {}: {}",
                index, fact
            )));
        }

        if !preserves_monotonicity(fact) {
            return Err(AuraError::invalid(&format!(
                "Fact at index {} violates monotonicity: {}",
                index, fact
            )));
        }
    }

    debug!(
        validated_facts = facts.len(),
        "All delta facts passed validation"
    );

    Ok(facts)
}

/// Apply a single fact to the journal
async fn apply_single_fact(
    fact: &JsonValue,
    effect_system: &mut AuraEffectSystem,
) -> AuraResult<JsonValue> {
    // Convert the JSON fact to the appropriate journal operation
    let journal_operation = convert_to_journal_operation(fact)?;

    // Apply to journal via effect system
    effect_system
        .apply_journal_operation(journal_operation)
        .await
        .map_err(|e| AuraError::internal(&format!("Failed to apply journal operation: {}", e)))?;

    // Return the applied fact (possibly with additional metadata)
    Ok(fact.clone())
}

/// Convert JSON fact to journal operation
fn convert_to_journal_operation(fact: &JsonValue) -> AuraResult<JournalOperation> {
    // Parse the fact JSON to determine the operation type
    let fact_type = fact
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AuraError::invalid("Fact missing 'type' field"))?;

    match fact_type {
        "device_registration" => {
            // Parse device registration fact
            Ok(JournalOperation::RegisterDevice {
                device_id: parse_device_id_from_fact(fact)?,
                metadata: parse_metadata_from_fact(fact)?,
            })
        }
        "capability_grant" => {
            // Parse capability grant fact
            Ok(JournalOperation::GrantCapability {
                capability: parse_capability_from_fact(fact)?,
                target_device: parse_device_id_from_fact(fact)?,
                expiry: parse_expiry_from_fact(fact)?,
            })
        }
        "session_attestation" => {
            // Parse session attestation fact
            Ok(JournalOperation::AttestSession {
                session_id: parse_session_id_from_fact(fact)?,
                attestation: parse_attestation_from_fact(fact)?,
            })
        }
        "intent_finalization" => {
            // Parse intent finalization fact
            Ok(JournalOperation::FinalizeIntent {
                intent_id: parse_intent_id_from_fact(fact)?,
                result: parse_result_from_fact(fact)?,
            })
        }
        _ => Err(AuraError::invalid(&format!(
            "Unknown fact type: {}",
            fact_type
        ))),
    }
}

/// Rollback applied facts (best effort)
async fn rollback_applied_facts(
    applied_facts: &[JsonValue],
    effect_system: &mut AuraEffectSystem,
) -> AuraResult<()> {
    warn!(
        facts_to_rollback = applied_facts.len(),
        "Attempting to rollback applied facts"
    );

    // Note: In a true CRDT system, rollback is not possible due to monotonicity.
    // This function exists for completeness but in practice, the journal
    // should use compensation patterns rather than rollbacks.

    // TODO fix - For now, log the facts that would need compensation
    for (index, fact) in applied_facts.iter().enumerate() {
        error!(
            fact_index = index,
            fact = %fact,
            "Fact applied but sequence failed - consider compensation"
        );
    }

    // In the future, this could trigger compensation operations
    // or mark facts as "pending confirmation" with eventual cleanup

    Ok(())
}

// Validation helper functions

/// Check if the fact has valid JSON format for journal operations
fn is_valid_fact_format(fact: &JsonValue) -> bool {
    // Basic validation: must be an object with a type field
    fact.is_object() && fact.get("type").is_some()
}

/// Check if the fact preserves monotonicity (no negative facts)
fn preserves_monotonicity(fact: &JsonValue) -> bool {
    // Check that this is not a retraction or deletion operation
    if let Some(fact_type) = fact.get("type").and_then(|v| v.as_str()) {
        match fact_type {
            // These operations are additive (monotonic)
            "device_registration"
            | "capability_grant"
            | "session_attestation"
            | "intent_finalization" => true,

            // These operations would violate monotonicity
            "device_removal" | "capability_revocation" | "session_invalidation" => false,

            _ => {
                // Unknown types are assumed to be non-monotonic for safety
                warn!(
                    fact_type = fact_type,
                    "Unknown fact type, assuming non-monotonic"
                );
                false
            }
        }
    } else {
        false
    }
}

/// Infer the type of a fact for logging
fn infer_fact_type(fact: &JsonValue) -> &str {
    fact.get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
}

// Placeholder types for journal operations (these should match aura-journal types)

/// Journal operation types for fact application
#[derive(Debug, Clone)]
pub enum JournalOperation {
    RegisterDevice {
        device_id: String,
        metadata: JsonValue,
    },
    GrantCapability {
        capability: String,
        target_device: String,
        expiry: Option<u64>,
    },
    AttestSession {
        session_id: String,
        attestation: JsonValue,
    },
    FinalizeIntent {
        intent_id: String,
        result: JsonValue,
    },
}

// Parser functions for extracting data from facts (placeholders)

fn parse_device_id_from_fact(fact: &JsonValue) -> AuraResult<String> {
    fact.get("device_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AuraError::invalid("Missing or invalid device_id"))
}

fn parse_metadata_from_fact(fact: &JsonValue) -> AuraResult<JsonValue> {
    Ok(fact.get("metadata").cloned().unwrap_or(JsonValue::Null))
}

fn parse_capability_from_fact(fact: &JsonValue) -> AuraResult<String> {
    fact.get("capability")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AuraError::invalid("Missing or invalid capability"))
}

fn parse_expiry_from_fact(fact: &JsonValue) -> AuraResult<Option<u64>> {
    Ok(fact.get("expiry").and_then(|v| v.as_u64()))
}

fn parse_session_id_from_fact(fact: &JsonValue) -> AuraResult<String> {
    fact.get("session_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AuraError::invalid("Missing or invalid session_id"))
}

fn parse_attestation_from_fact(fact: &JsonValue) -> AuraResult<JsonValue> {
    fact.get("attestation")
        .cloned()
        .ok_or_else(|| AuraError::invalid("Missing attestation"))
}

fn parse_intent_id_from_fact(fact: &JsonValue) -> AuraResult<String> {
    fact.get("intent_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AuraError::invalid("Missing or invalid intent_id"))
}

fn parse_result_from_fact(fact: &JsonValue) -> AuraResult<JsonValue> {
    fact.get("result")
        .cloned()
        .ok_or_else(|| AuraError::invalid("Missing result"))
}

/// Extension trait for AuraEffectSystem to support journal operations
trait JournalOperationExt {
    async fn apply_journal_operation(&mut self, operation: JournalOperation) -> AuraResult<()>;
}

impl JournalOperationExt for AuraEffectSystem {
    async fn apply_journal_operation(&mut self, operation: JournalOperation) -> AuraResult<()> {
        // This would integrate with the actual journal effects
        // TODO fix - For now, log the operation
        debug!(operation = ?operation, "Applying journal operation");

        // TODO: Integrate with actual aura-journal CRDT operations
        // This would call the appropriate journal effects methods
        // Example: self.journal_effects().append_tree_op(converted_op).await?;

        Ok(())
    }
}

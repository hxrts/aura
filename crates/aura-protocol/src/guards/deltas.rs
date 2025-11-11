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

    // Implement compensation patterns for failed operations
    for (index, fact) in applied_facts.iter().enumerate() {
        error!(
            fact_index = index,
            fact = %fact,
            "Fact applied but sequence failed - applying compensation"
        );
        
        // Generate compensation fact based on fact type
        if let Some(compensation_fact) = generate_compensation_fact(fact)? {
            // Apply compensation fact to journal
            if let Err(compensation_error) = effect_system.append_fact(compensation_fact.clone()).await {
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

/// Generate compensation fact for a failed operation
fn generate_compensation_fact(fact: &JsonValue) -> AuraResult<Option<JsonValue>> {
    let fact_type = fact
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("unknown");
    
    match fact_type {
        "device_registration" => {
            // For device registration failures, mark device as inactive
            if let Some(device_id) = fact.get("device_id").and_then(|id| id.as_str()) {
                Ok(Some(JsonValue::Object([
                    ("type".to_string(), JsonValue::String("device_deactivation".to_string())),
                    ("device_id".to_string(), JsonValue::String(device_id.to_string())),
                    ("reason".to_string(), JsonValue::String("registration_compensation".to_string())),
                    ("timestamp".to_string(), JsonValue::Number(serde_json::Number::from(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs()
                    ))),
                ].into())))
            } else {
                Ok(None)
            }
        },
        
        "capability_grant" => {
            // For capability grant failures, revoke the granted capability
            if let (Some(capability), Some(target_device)) = (
                fact.get("capability").and_then(|c| c.as_str()),
                fact.get("target_device").and_then(|d| d.as_str())
            ) {
                Ok(Some(JsonValue::Object([
                    ("type".to_string(), JsonValue::String("capability_revocation".to_string())),
                    ("capability".to_string(), JsonValue::String(capability.to_string())),
                    ("target_device".to_string(), JsonValue::String(target_device.to_string())),
                    ("reason".to_string(), JsonValue::String("grant_compensation".to_string())),
                ].into())))
            } else {
                Ok(None)
            }
        },
        
        "session_attestation" => {
            // For session attestation failures, invalidate the session
            if let Some(session_id) = fact.get("session_id").and_then(|id| id.as_str()) {
                Ok(Some(JsonValue::Object([
                    ("type".to_string(), JsonValue::String("session_invalidation".to_string())),
                    ("session_id".to_string(), JsonValue::String(session_id.to_string())),
                    ("reason".to_string(), JsonValue::String("attestation_compensation".to_string())),
                ].into())))
            } else {
                Ok(None)
            }
        },
        
        "intent_finalization" => {
            // For intent finalization failures, mark intent as failed
            if let Some(intent_id) = fact.get("intent_id").and_then(|id| id.as_str()) {
                Ok(Some(JsonValue::Object([
                    ("type".to_string(), JsonValue::String("intent_failure".to_string())),
                    ("intent_id".to_string(), JsonValue::String(intent_id.to_string())),
                    ("reason".to_string(), JsonValue::String("finalization_compensation".to_string())),
                ].into())))
            } else {
                Ok(None)
            }
        },
        
        _ => {
            // For unknown fact types, create a generic compensation record
            warn!(fact_type = fact_type, "Unknown fact type for compensation");
            Ok(Some(JsonValue::Object([
                ("type".to_string(), JsonValue::String("operation_compensation".to_string())),
                ("original_fact".to_string(), fact.clone()),
                ("reason".to_string(), JsonValue::String("unknown_type_compensation".to_string())),
            ].into())))
        }
    }
}

/// Extension trait for AuraEffectSystem to support journal operations
trait JournalOperationExt {
    async fn apply_journal_operation(&mut self, operation: JournalOperation) -> AuraResult<()>;
}

impl JournalOperationExt for AuraEffectSystem {
    async fn apply_journal_operation(&mut self, operation: JournalOperation) -> AuraResult<()> {
        debug!(operation = ?operation, "Applying journal operation");

        // Apply operation via CRDT journal effects
        match operation {
            JournalOperation::RegisterDevice { device_id, metadata } => {
                // Create device registration fact for journal merge
                let device_fact = JsonValue::Object([
                    ("type".to_string(), JsonValue::String("device_registration".to_string())),
                    ("device_id".to_string(), JsonValue::String(device_id)),
                    ("metadata".to_string(), metadata),
                    ("timestamp".to_string(), JsonValue::Number(serde_json::Number::from(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs()
                    ))),
                ].into());
                
                self.append_fact(device_fact).await?;
            },
            
            JournalOperation::GrantCapability { capability, target_device, expiry } => {
                // Create capability grant fact
                let mut cap_fact = serde_json::Map::new();
                cap_fact.insert("type".to_string(), JsonValue::String("capability_grant".to_string()));
                cap_fact.insert("capability".to_string(), JsonValue::String(capability));
                cap_fact.insert("target_device".to_string(), JsonValue::String(target_device));
                if let Some(expiry_time) = expiry {
                    cap_fact.insert("expiry".to_string(), JsonValue::Number(serde_json::Number::from(expiry_time)));
                }
                
                self.append_fact(JsonValue::Object(cap_fact)).await?;
            },
            
            JournalOperation::AttestSession { session_id, attestation } => {
                // Create session attestation fact
                let session_fact = JsonValue::Object([
                    ("type".to_string(), JsonValue::String("session_attestation".to_string())),
                    ("session_id".to_string(), JsonValue::String(session_id)),
                    ("attestation".to_string(), attestation),
                ].into());
                
                self.append_fact(session_fact).await?;
            },
            
            JournalOperation::FinalizeIntent { intent_id, result } => {
                // Create intent finalization fact
                let intent_fact = JsonValue::Object([
                    ("type".to_string(), JsonValue::String("intent_finalization".to_string())),
                    ("intent_id".to_string(), JsonValue::String(intent_id)),
                    ("result".to_string(), result),
                ].into());
                
                self.append_fact(intent_fact).await?;
            },
        }

        Ok(())
    }
}

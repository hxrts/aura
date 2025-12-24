//! Fact validation for delta application
//!
//! Validation functions that ensure facts meet format requirements
//! and preserve monotonicity before application.

use aura_core::{AuraError, AuraResult};
use serde_json::Value as JsonValue;
use tracing::{debug, warn};

/// Validate delta facts before application
pub fn validate_delta_facts(facts: &[JsonValue]) -> AuraResult<&[JsonValue]> {
    for (index, fact) in facts.iter().enumerate() {
        if !is_valid_fact_format(fact) {
            return Err(AuraError::invalid(format!(
                "Invalid fact format at index {}: {}",
                index, fact
            )));
        }

        if !preserves_monotonicity(fact) {
            return Err(AuraError::invalid(format!(
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

/// Check if the fact has valid JSON format for journal operations
pub fn is_valid_fact_format(fact: &JsonValue) -> bool {
    // Basic validation: must be an object with a type field
    fact.is_object() && fact.get("type").is_some()
}

/// Check if the fact preserves monotonicity (no negative facts)
pub fn preserves_monotonicity(fact: &JsonValue) -> bool {
    // Check that this is not a retraction or deletion operation
    if let Some(fact_type) = fact.get("type").and_then(|v| v.as_str()) {
        match fact_type {
            // These operations are additive (monotonic)
            "device_registration"
            | "capability_grant"
            | "session_attestation"
            | "intent_finalization"
            | "guardian_enrollment"
            | "key_derivation"
            | "flow_budget_update"
            | "recovery_initiation"
            | "storage_commitment"
            | "ota_deployment" => true,

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
pub fn infer_fact_type(fact: &JsonValue) -> &str {
    fact.get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
}

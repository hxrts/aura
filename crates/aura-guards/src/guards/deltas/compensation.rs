//! Compensation fact generation
//!
//! Generates compensation facts for failed operations to maintain
//! consistency in the CRDT journal.

use aura_core::{AuraResult, TimeEffects};
use serde_json::Value as JsonValue;
use tracing::warn;

/// Generate compensation fact for a failed operation
pub async fn generate_compensation_fact<E: TimeEffects>(
    fact: &JsonValue,
    effects: &E,
) -> AuraResult<Option<JsonValue>> {
    let fact_type = fact
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("unknown");

    match fact_type {
        "device_registration" => {
            // For device registration failures, mark device as inactive
            if let Some(device_id) = fact.get("device_id").and_then(|id| id.as_str()) {
                let map = serde_json::Map::from_iter([
                    (
                        "type".to_string(),
                        JsonValue::String("device_deactivation".to_string()),
                    ),
                    (
                        "device_id".to_string(),
                        JsonValue::String(device_id.to_string()),
                    ),
                    (
                        "reason".to_string(),
                        JsonValue::String("registration_compensation".to_string()),
                    ),
                    (
                        "timestamp".to_string(),
                        JsonValue::Number(serde_json::Number::from(
                            effects
                                .physical_time()
                                .await
                                .map_err(|e| {
                                    aura_core::AuraError::internal(format!("time error: {e}"))
                                })?
                                .ts_ms,
                        )),
                    ),
                ]);
                Ok(Some(JsonValue::Object(map)))
            } else {
                Ok(None)
            }
        }

        "capability_grant" => {
            // For capability grant failures, revoke the granted capability
            if let (Some(capability), Some(target_device)) = (
                fact.get("capability").and_then(|c| c.as_str()),
                fact.get("target_device").and_then(|d| d.as_str()),
            ) {
                let map = serde_json::Map::from_iter([
                    (
                        "type".to_string(),
                        JsonValue::String("capability_revocation".to_string()),
                    ),
                    (
                        "capability".to_string(),
                        JsonValue::String(capability.to_string()),
                    ),
                    (
                        "target_device".to_string(),
                        JsonValue::String(target_device.to_string()),
                    ),
                    (
                        "reason".to_string(),
                        JsonValue::String("grant_compensation".to_string()),
                    ),
                ]);
                Ok(Some(JsonValue::Object(map)))
            } else {
                Ok(None)
            }
        }

        "session_attestation" => {
            // For session attestation failures, invalidate the session
            if let Some(session_id) = fact.get("session_id").and_then(|id| id.as_str()) {
                let map = serde_json::Map::from_iter([
                    (
                        "type".to_string(),
                        JsonValue::String("session_invalidation".to_string()),
                    ),
                    (
                        "session_id".to_string(),
                        JsonValue::String(session_id.to_string()),
                    ),
                    (
                        "reason".to_string(),
                        JsonValue::String("attestation_compensation".to_string()),
                    ),
                ]);
                Ok(Some(JsonValue::Object(map)))
            } else {
                Ok(None)
            }
        }

        "intent_finalization" => {
            // For intent finalization failures, mark intent as failed
            if let Some(intent_id) = fact.get("intent_id").and_then(|id| id.as_str()) {
                let map = serde_json::Map::from_iter([
                    (
                        "type".to_string(),
                        JsonValue::String("intent_failure".to_string()),
                    ),
                    (
                        "intent_id".to_string(),
                        JsonValue::String(intent_id.to_string()),
                    ),
                    (
                        "reason".to_string(),
                        JsonValue::String("finalization_compensation".to_string()),
                    ),
                ]);
                Ok(Some(JsonValue::Object(map)))
            } else {
                Ok(None)
            }
        }

        _ => {
            // For unknown fact types, create a generic compensation record
            warn!(fact_type = fact_type, "Unknown fact type for compensation");
            let map = serde_json::Map::from_iter([
                (
                    "type".to_string(),
                    JsonValue::String("operation_compensation".to_string()),
                ),
                ("original_fact".to_string(), fact.clone()),
                (
                    "reason".to_string(),
                    JsonValue::String("unknown_type_compensation".to_string()),
                ),
            ]);
            Ok(Some(JsonValue::Object(map)))
        }
    }
}

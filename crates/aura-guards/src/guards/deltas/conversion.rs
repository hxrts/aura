//! JSON fact conversion functions
//!
//! Functions for converting JSON facts to journal operations
//! and Fact/FactValue types.

use std::collections::BTreeSet;

use aura_core::{AuraError, AuraResult, Fact, FactValue, Journal};
use serde_json::Value as JsonValue;

use super::operation::JournalOperation;
use super::parsers::*;

/// Convert JSON fact to journal operation
pub fn convert_to_journal_operation(fact: &JsonValue) -> AuraResult<JournalOperation> {
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
        "guardian_enrollment" => {
            // Parse guardian enrollment fact
            let device_id = parse_device_id_from_fact(fact)?;
            let guardian_id = parse_guardian_id_from_fact(fact)?;
            let capabilities = parse_guardian_capabilities_from_fact(fact)?;

            Ok(JournalOperation::EnrollGuardian {
                guardian_id,
                device_id,
                capabilities,
            })
        }
        // NOTE: threshold_ceremony_completion facts should be handled by dedicated FROST
        // choreography crates, not the coordination layer.
        // This coordination layer no longer processes domain-specific threshold cryptography events
        "threshold_ceremony_completion" => Err(AuraError::invalid(
            "threshold_ceremony_completion facts belong to FROST choreography, not coordination layer",
        )),
        "key_derivation" => {
            // Parse key derivation fact (DKD operations)
            let derivation_id = parse_derivation_id_from_fact(fact)?;
            let context = parse_derivation_context_from_fact(fact)?;
            let derived_for = parse_device_id_from_fact(fact)?;

            Ok(JournalOperation::DeriveKey {
                derivation_id,
                context,
                derived_for,
            })
        }
        "flow_budget_update" => {
            // Parse flow budget update fact
            let context_id = parse_context_id_from_fact(fact)?;
            let peer_id = parse_device_id_from_fact(fact)?;
            let new_limit = parse_budget_limit_from_fact(fact)?;
            let cost = if fact.get("cost").is_some() {
                Some(parse_budget_cost_from_fact(fact)?)
            } else {
                None
            };

            Ok(JournalOperation::UpdateFlowBudget {
                context_id,
                peer_id,
                new_limit,
                cost,
            })
        }
        "recovery_initiation" => {
            // Parse recovery initiation fact
            let recovery_id = parse_recovery_id_from_fact(fact)?;
            let account_id = parse_account_id_from_fact(fact)?;
            let requester = parse_device_id_from_fact(fact)?;
            let guardians_required = parse_guardian_threshold_from_fact(fact)?;

            Ok(JournalOperation::InitiateRecovery {
                recovery_id,
                account_id,
                requester,
                guardians_required,
            })
        }
        "storage_commitment" => {
            // Parse storage commitment fact (content-addressed storage)
            let content_hash = parse_content_hash_from_fact(fact)?;
            let size = parse_content_size_from_fact(fact)?;
            let storage_nodes = parse_storage_nodes_from_fact(fact)?;

            Ok(JournalOperation::CommitStorage {
                content_hash,
                size,
                storage_nodes,
            })
        }
        "ota_deployment" => {
            // Parse OTA deployment fact
            let version = parse_ota_version_from_fact(fact)?;
            let target_epoch = parse_target_epoch_from_fact(fact)?;
            let deployment_hash = parse_deployment_hash_from_fact(fact)?;

            Ok(JournalOperation::DeployOta {
                version,
                target_epoch,
                deployment_hash,
            })
        }
        _ => Err(AuraError::invalid(format!(
            "Unknown fact type: {fact_type}"
        ))),
    }
}

/// Convert JSON value to FactValue
pub fn json_value_to_fact_value(value: &JsonValue) -> AuraResult<FactValue> {
    match value {
        JsonValue::String(s) => Ok(FactValue::String(s.clone())),
        JsonValue::Number(n) => n
            .as_i64()
            .map(FactValue::Number)
            .ok_or_else(|| AuraError::invalid("Numeric value out of range for i64")),
        JsonValue::Bool(b) => Ok(FactValue::Number(if *b { 1 } else { 0 })),
        JsonValue::Array(items) => {
            let mut set = BTreeSet::new();
            for item in items {
                set.insert(item.to_string());
            }
            Ok(FactValue::Set(set))
        }
        JsonValue::Object(map) => {
            let mut nested = Fact::new();
            for (key, nested_value) in map {
                let _ = nested.insert(key.clone(), json_value_to_fact_value(nested_value)?);
            }
            Ok(FactValue::Nested(Box::new(nested)))
        }
        JsonValue::Null => Ok(FactValue::String("null".to_string())),
    }
}

/// Create a Journal from a JSON fact
pub fn journal_from_json_fact(fact: &JsonValue) -> AuraResult<Journal> {
    let mut delta = Journal::default();
    let mut fact_record = Fact::new();

    match fact {
        JsonValue::Object(map) => {
            for (key, value) in map {
                let _ = fact_record.insert(key.clone(), json_value_to_fact_value(value)?);
            }
        }
        _ => {
            let _ = fact_record.insert("value", json_value_to_fact_value(fact)?);
        }
    }

    delta.merge_facts(fact_record);
    Ok(delta)
}

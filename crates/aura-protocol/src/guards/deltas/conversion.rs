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
            let device_id =
                parse_device_id_from_fact(fact).unwrap_or_else(|_| "unknown".to_string());
            let guardian_id =
                parse_guardian_id_from_fact(fact).unwrap_or_else(|_| "guardian".to_string());
            let capabilities = parse_guardian_capabilities_from_fact(fact).unwrap_or_else(|_| {
                vec![
                    "recovery:approve".to_string(),
                    "guardian:vote".to_string(),
                ]
            });

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
            let derivation_id =
                parse_derivation_id_from_fact(fact).unwrap_or_else(|_| "dkd_operation".to_string());
            let context =
                parse_derivation_context_from_fact(fact).unwrap_or_else(|_| "default".to_string());
            let derived_for =
                parse_device_id_from_fact(fact).unwrap_or_else(|_| "unknown".to_string());

            Ok(JournalOperation::DeriveKey {
                derivation_id,
                context,
                derived_for,
            })
        }
        "flow_budget_update" => {
            // Parse flow budget update fact
            let context_id = parse_context_id_from_fact(fact)
                .unwrap_or_else(|_| "default_context".to_string());
            let peer_id =
                parse_device_id_from_fact(fact).unwrap_or_else(|_| "unknown".to_string());
            let new_limit = parse_budget_limit_from_fact(fact).unwrap_or(10000);
            let cost = parse_budget_cost_from_fact(fact).ok();

            Ok(JournalOperation::UpdateFlowBudget {
                context_id,
                peer_id,
                new_limit,
                cost,
            })
        }
        "recovery_initiation" => {
            // Parse recovery initiation fact
            let recovery_id =
                parse_recovery_id_from_fact(fact).unwrap_or_else(|_| "recovery_session".to_string());
            let account_id =
                parse_account_id_from_fact(fact).unwrap_or_else(|_| "unknown_account".to_string());
            let requester =
                parse_device_id_from_fact(fact).unwrap_or_else(|_| "unknown".to_string());
            let guardians_required = parse_guardian_threshold_from_fact(fact).unwrap_or(2);

            Ok(JournalOperation::InitiateRecovery {
                recovery_id,
                account_id,
                requester,
                guardians_required,
            })
        }
        "storage_commitment" => {
            // Parse storage commitment fact (content-addressed storage)
            let content_hash =
                parse_content_hash_from_fact(fact).unwrap_or_else(|_| "unknown_hash".to_string());
            let size = parse_content_size_from_fact(fact).unwrap_or(0);
            let storage_nodes = parse_storage_nodes_from_fact(fact)
                .unwrap_or_else(|_| vec!["local_node".to_string()]);

            Ok(JournalOperation::CommitStorage {
                content_hash,
                size,
                storage_nodes,
            })
        }
        "ota_deployment" => {
            // Parse OTA deployment fact
            let version =
                parse_ota_version_from_fact(fact).unwrap_or_else(|_| "unknown_version".to_string());
            let target_epoch = parse_target_epoch_from_fact(fact).unwrap_or(1);
            let deployment_hash =
                parse_deployment_hash_from_fact(fact).unwrap_or_else(|_| "unknown_hash".to_string());

            Ok(JournalOperation::DeployOta {
                version,
                target_epoch,
                deployment_hash,
            })
        }
        _ => Err(AuraError::invalid(format!(
            "Unknown fact type: {}",
            fact_type
        ))),
    }
}

/// Convert JSON value to FactValue
pub fn json_value_to_fact_value(value: &JsonValue) -> FactValue {
    match value {
        JsonValue::String(s) => FactValue::String(s.clone()),
        JsonValue::Number(n) => FactValue::Number(n.as_i64().unwrap_or_default()),
        JsonValue::Bool(b) => FactValue::Number(if *b { 1 } else { 0 }),
        JsonValue::Array(items) => {
            let mut set = BTreeSet::new();
            for item in items {
                set.insert(item.to_string());
            }
            FactValue::Set(set)
        }
        JsonValue::Object(map) => {
            let mut nested = Fact::new();
            for (key, nested_value) in map {
                nested.insert(key.clone(), json_value_to_fact_value(nested_value));
            }
            FactValue::Nested(Box::new(nested))
        }
        JsonValue::Null => FactValue::String("null".to_string()),
    }
}

/// Create a Journal from a JSON fact
pub fn journal_from_json_fact(fact: &JsonValue) -> Journal {
    let mut delta = Journal::default();
    let mut fact_record = Fact::new();

    match fact {
        JsonValue::Object(map) => {
            for (key, value) in map {
                fact_record.insert(key.clone(), json_value_to_fact_value(value));
            }
        }
        _ => {
            fact_record.insert("value", json_value_to_fact_value(fact));
        }
    }

    delta.merge_facts(fact_record);
    delta
}

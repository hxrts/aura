//! JSON fact parsing functions
//!
//! Parser functions for extracting typed data from JSON facts.
//! Each function extracts a specific field from a fact JSON object.

use aura_core::{AuraError, AuraResult};
use serde_json::Value as JsonValue;

pub fn parse_device_id_from_fact(fact: &JsonValue) -> AuraResult<String> {
    fact.get("device_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AuraError::invalid("Missing or invalid device_id"))
}

pub fn parse_metadata_from_fact(fact: &JsonValue) -> AuraResult<JsonValue> {
    Ok(fact.get("metadata").cloned().unwrap_or(JsonValue::Null))
}

pub fn parse_capability_from_fact(fact: &JsonValue) -> AuraResult<String> {
    fact.get("capability")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AuraError::invalid("Missing or invalid capability"))
}

pub fn parse_expiry_from_fact(fact: &JsonValue) -> AuraResult<Option<u64>> {
    Ok(fact.get("expiry").and_then(|v| v.as_u64()))
}

pub fn parse_session_id_from_fact(fact: &JsonValue) -> AuraResult<String> {
    fact.get("session_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AuraError::invalid("Missing or invalid session_id"))
}

pub fn parse_attestation_from_fact(fact: &JsonValue) -> AuraResult<JsonValue> {
    fact.get("attestation")
        .cloned()
        .ok_or_else(|| AuraError::invalid("Missing attestation"))
}

pub fn parse_intent_id_from_fact(fact: &JsonValue) -> AuraResult<String> {
    fact.get("intent_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AuraError::invalid("Missing or invalid intent_id"))
}

pub fn parse_result_from_fact(fact: &JsonValue) -> AuraResult<JsonValue> {
    fact.get("result")
        .cloned()
        .ok_or_else(|| AuraError::invalid("Missing result"))
}

pub fn parse_account_id_from_fact(fact: &JsonValue) -> AuraResult<String> {
    fact.get("account_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AuraError::invalid("Missing account_id in fact"))
}

pub fn parse_derivation_id_from_fact(fact: &JsonValue) -> AuraResult<String> {
    fact.get("derivation_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AuraError::invalid("Missing derivation_id in fact"))
}

pub fn parse_context_id_from_fact(fact: &JsonValue) -> AuraResult<String> {
    fact.get("context_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AuraError::invalid("Missing context_id in fact"))
}

pub fn parse_recovery_id_from_fact(fact: &JsonValue) -> AuraResult<String> {
    fact.get("recovery_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AuraError::invalid("Missing recovery_id in fact"))
}

pub fn parse_content_hash_from_fact(fact: &JsonValue) -> AuraResult<String> {
    fact.get("content_hash")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AuraError::invalid("Missing content_hash in fact"))
}

pub fn parse_deployment_hash_from_fact(fact: &JsonValue) -> AuraResult<String> {
    fact.get("deployment_hash")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AuraError::invalid("Missing deployment_hash in fact"))
}

pub fn parse_guardian_id_from_fact(fact: &JsonValue) -> AuraResult<String> {
    fact.get("guardian_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AuraError::invalid("Missing guardian_id in fact"))
}

pub fn parse_guardian_capabilities_from_fact(fact: &JsonValue) -> AuraResult<Vec<String>> {
    fact.get("capabilities")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .ok_or_else(|| AuraError::invalid("Missing or invalid capabilities in fact"))
}

pub fn parse_derivation_context_from_fact(fact: &JsonValue) -> AuraResult<String> {
    fact.get("context")
        .or_else(|| fact.get("derivation_context"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AuraError::invalid("Missing derivation context in fact"))
}

pub fn parse_budget_limit_from_fact(fact: &JsonValue) -> AuraResult<u32> {
    fact.get("budget_limit")
        .or_else(|| fact.get("limit"))
        .and_then(|v| v.as_u64())
        .map(|n| n as u32)
        .ok_or_else(|| AuraError::invalid("Missing budget_limit in fact"))
}

pub fn parse_budget_cost_from_fact(fact: &JsonValue) -> AuraResult<u32> {
    fact.get("cost")
        .or_else(|| fact.get("flow_cost"))
        .and_then(|v| v.as_u64())
        .map(|n| n as u32)
        .ok_or_else(|| AuraError::invalid("Missing cost in fact"))
}

pub fn parse_guardian_threshold_from_fact(fact: &JsonValue) -> AuraResult<usize> {
    fact.get("guardians_required")
        .or_else(|| fact.get("threshold"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .ok_or_else(|| AuraError::invalid("Missing guardians_required in fact"))
}

pub fn parse_content_size_from_fact(fact: &JsonValue) -> AuraResult<u64> {
    fact.get("size")
        .or_else(|| fact.get("content_size"))
        .and_then(|v| v.as_u64())
        .ok_or_else(|| AuraError::invalid("Missing size in fact"))
}

pub fn parse_storage_nodes_from_fact(fact: &JsonValue) -> AuraResult<Vec<String>> {
    fact.get("storage_nodes")
        .or_else(|| fact.get("nodes"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .ok_or_else(|| AuraError::invalid("Missing or invalid storage_nodes in fact"))
}

pub fn parse_ota_version_from_fact(fact: &JsonValue) -> AuraResult<String> {
    fact.get("version")
        .or_else(|| fact.get("ota_version"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AuraError::invalid("Missing version in fact"))
}

pub fn parse_target_epoch_from_fact(fact: &JsonValue) -> AuraResult<u64> {
    fact.get("target_epoch")
        .or_else(|| fact.get("epoch"))
        .and_then(|v| v.as_u64())
        .ok_or_else(|| AuraError::invalid("Missing target_epoch in fact"))
}

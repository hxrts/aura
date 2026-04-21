//! JSON fact parsing functions
//!
//! Parser functions for extracting typed data from JSON facts.
//! Each function extracts a specific field from a fact JSON object.

use aura_core::{AuraError, AuraResult};
use serde_json::Value as JsonValue;

fn invalid_field(field: &str) -> AuraError {
    AuraError::invalid(field.to_string())
}

fn first_present<'a>(fact: &'a JsonValue, keys: &[&str]) -> Option<&'a JsonValue> {
    keys.iter().find_map(|key| fact.get(*key))
}

fn required_string_field(fact: &JsonValue, keys: &[&str], error: &str) -> AuraResult<String> {
    first_present(fact, keys)
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
        .ok_or_else(|| invalid_field(error))
}

fn required_u64_field(fact: &JsonValue, keys: &[&str], error: &str) -> AuraResult<u64> {
    first_present(fact, keys)
        .and_then(|value| value.as_u64())
        .ok_or_else(|| invalid_field(error))
}

fn optional_u64_field(fact: &JsonValue, keys: &[&str]) -> Option<u64> {
    first_present(fact, keys).and_then(|value| value.as_u64())
}

fn required_json_field(fact: &JsonValue, key: &str, error: &str) -> AuraResult<JsonValue> {
    fact.get(key).cloned().ok_or_else(|| invalid_field(error))
}

fn required_string_vec_field(
    fact: &JsonValue,
    keys: &[&str],
    error: &str,
) -> AuraResult<Vec<String>> {
    first_present(fact, keys)
        .and_then(|value| value.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|value| value.as_str().map(ToString::to_string))
                .collect()
        })
        .ok_or_else(|| invalid_field(error))
}

pub fn parse_device_id_from_fact(fact: &JsonValue) -> AuraResult<String> {
    required_string_field(fact, &["device_id"], "Missing or invalid device_id")
}

pub fn parse_metadata_from_fact(fact: &JsonValue) -> AuraResult<JsonValue> {
    Ok(fact.get("metadata").cloned().unwrap_or(JsonValue::Null))
}

pub fn parse_capability_from_fact(fact: &JsonValue) -> AuraResult<String> {
    required_string_field(fact, &["capability"], "Missing or invalid capability")
}

pub fn parse_expiry_from_fact(fact: &JsonValue) -> AuraResult<Option<u64>> {
    Ok(optional_u64_field(fact, &["expiry"]))
}

pub fn parse_session_id_from_fact(fact: &JsonValue) -> AuraResult<String> {
    required_string_field(fact, &["session_id"], "Missing or invalid session_id")
}

pub fn parse_attestation_from_fact(fact: &JsonValue) -> AuraResult<JsonValue> {
    required_json_field(fact, "attestation", "Missing attestation")
}

pub fn parse_intent_id_from_fact(fact: &JsonValue) -> AuraResult<String> {
    required_string_field(fact, &["intent_id"], "Missing or invalid intent_id")
}

pub fn parse_result_from_fact(fact: &JsonValue) -> AuraResult<JsonValue> {
    required_json_field(fact, "result", "Missing result")
}

pub fn parse_account_id_from_fact(fact: &JsonValue) -> AuraResult<String> {
    required_string_field(fact, &["account_id"], "Missing account_id in fact")
}

pub fn parse_derivation_id_from_fact(fact: &JsonValue) -> AuraResult<String> {
    required_string_field(fact, &["derivation_id"], "Missing derivation_id in fact")
}

pub fn parse_context_id_from_fact(fact: &JsonValue) -> AuraResult<String> {
    required_string_field(fact, &["context_id"], "Missing context_id in fact")
}

pub fn parse_recovery_id_from_fact(fact: &JsonValue) -> AuraResult<String> {
    required_string_field(fact, &["recovery_id"], "Missing recovery_id in fact")
}

pub fn parse_content_hash_from_fact(fact: &JsonValue) -> AuraResult<String> {
    required_string_field(fact, &["content_hash"], "Missing content_hash in fact")
}

pub fn parse_deployment_hash_from_fact(fact: &JsonValue) -> AuraResult<String> {
    required_string_field(
        fact,
        &["deployment_hash"],
        "Missing deployment_hash in fact",
    )
}

pub fn parse_guardian_id_from_fact(fact: &JsonValue) -> AuraResult<String> {
    required_string_field(fact, &["guardian_id"], "Missing guardian_id in fact")
}

pub fn parse_guardian_capabilities_from_fact(fact: &JsonValue) -> AuraResult<Vec<String>> {
    required_string_vec_field(
        fact,
        &["capabilities"],
        "Missing or invalid capabilities in fact",
    )
}

pub fn parse_derivation_context_from_fact(fact: &JsonValue) -> AuraResult<String> {
    required_string_field(
        fact,
        &["context", "derivation_context"],
        "Missing derivation context in fact",
    )
}

pub fn parse_budget_limit_from_fact(fact: &JsonValue) -> AuraResult<u32> {
    required_u64_field(
        fact,
        &["budget_limit", "limit"],
        "Missing budget_limit in fact",
    )
    .map(|value| value as u32)
}

pub fn parse_budget_cost_from_fact(fact: &JsonValue) -> AuraResult<u32> {
    required_u64_field(fact, &["cost", "flow_cost"], "Missing cost in fact")
        .map(|value| value as u32)
}

pub fn parse_guardian_threshold_from_fact(fact: &JsonValue) -> AuraResult<usize> {
    required_u64_field(
        fact,
        &["guardians_required", "threshold"],
        "Missing guardians_required in fact",
    )
    .map(|value| value as usize)
}

pub fn parse_content_size_from_fact(fact: &JsonValue) -> AuraResult<u64> {
    required_u64_field(fact, &["size", "content_size"], "Missing size in fact")
}

pub fn parse_storage_nodes_from_fact(fact: &JsonValue) -> AuraResult<Vec<String>> {
    required_string_vec_field(
        fact,
        &["storage_nodes", "nodes"],
        "Missing or invalid storage_nodes in fact",
    )
}

pub fn parse_ota_version_from_fact(fact: &JsonValue) -> AuraResult<String> {
    required_string_field(fact, &["version", "ota_version"], "Missing version in fact")
}

pub fn parse_target_epoch_from_fact(fact: &JsonValue) -> AuraResult<u64> {
    required_u64_field(
        fact,
        &["target_epoch", "epoch"],
        "Missing target_epoch in fact",
    )
}

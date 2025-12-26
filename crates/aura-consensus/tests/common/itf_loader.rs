//! ITF Trace Loader for Consensus Conformance Testing
//!
//! Loads Quint ITF (Interaction Trace Format) traces and converts them
//! to Rust consensus state for conformance testing.
//!
//! ## ITF Format
//! ITF is the Apalache trace format: https://apalache-mc.org/docs/adr/015adr-trace.html
//!
//! Key features:
//! - `#set`: Set values represented as `{"#set": [...]}`
//! - `#map`: Map values represented as `{"#map": [[k1, v1], [k2, v2], ...]}`
//! - `#bigint`: Large integers represented as `{"#bigint": "123"}`
//! - Tagged variants: `{"tag": "SomeTag", "value": {...}}`

use aura_core::AuraError;
use aura_consensus::core::{
    ConsensusPhase, ConsensusState, ShareData, ShareProposal,
};
use aura_consensus::core::state::PureCommitFact;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

/// Parsed ITF trace containing a sequence of states
#[derive(Debug, Clone)]
pub struct ITFTrace {
    /// Metadata about the trace
    pub meta: ITFMeta,
    /// Variable names in the trace
    pub vars: Vec<String>,
    /// Sequence of states
    pub states: Vec<ITFState>,
}

/// Trace metadata
#[derive(Debug, Clone)]
pub struct ITFMeta {
    pub format: String,
    pub source: String,
    pub status: String,
}

/// A single state in the trace
#[derive(Debug, Clone)]
pub struct ITFState {
    /// State index
    pub index: usize,
    /// Raw variables as JSON
    pub variables: HashMap<String, Value>,
    /// Parsed consensus instances
    pub instances: HashMap<String, ConsensusState>,
    /// Current epoch
    pub epoch: u64,
}

/// Load an ITF trace from a file
pub fn load_itf_trace(path: &Path) -> Result<ITFTrace, AuraError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| AuraError::invalid(format!("failed to read ITF file: {}", e)))?;

    parse_itf_trace(&content)
}

/// Parse an ITF trace from JSON string
pub fn parse_itf_trace(json: &str) -> Result<ITFTrace, AuraError> {
    let value: Value = serde_json::from_str(json)
        .map_err(|e| AuraError::invalid(format!("failed to parse ITF JSON: {}", e)))?;

    let meta = parse_meta(&value)?;
    let vars = parse_vars(&value)?;
    let states = parse_states(&value)?;

    Ok(ITFTrace { meta, vars, states })
}

fn parse_meta(value: &Value) -> Result<ITFMeta, AuraError> {
    let meta_obj = value
        .get("#meta")
        .and_then(|v| v.as_object())
        .ok_or_else(|| AuraError::invalid("missing #meta in ITF trace"))?;

    Ok(ITFMeta {
        format: meta_obj
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("ITF")
            .to_string(),
        source: meta_obj
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        status: meta_obj
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    })
}

fn parse_vars(value: &Value) -> Result<Vec<String>, AuraError> {
    let vars_arr = value
        .get("vars")
        .and_then(|v| v.as_array())
        .ok_or_else(|| AuraError::invalid("missing vars in ITF trace"))?;

    vars_arr
        .iter()
        .map(|v| {
            v.as_str()
                .map(String::from)
                .ok_or_else(|| AuraError::invalid("invalid var name in ITF trace"))
        })
        .collect()
}

fn parse_states(value: &Value) -> Result<Vec<ITFState>, AuraError> {
    let states_arr = value
        .get("states")
        .and_then(|v| v.as_array())
        .ok_or_else(|| AuraError::invalid("missing states in ITF trace"))?;

    states_arr.iter().map(parse_state).collect()
}

fn parse_state(value: &Value) -> Result<ITFState, AuraError> {
    let obj = value
        .as_object()
        .ok_or_else(|| AuraError::invalid("state is not an object"))?;

    let index = obj
        .get("#meta")
        .and_then(|m| m.get("index"))
        .and_then(|i| i.as_u64())
        .unwrap_or(0) as usize;

    let mut variables = HashMap::new();
    for (key, val) in obj {
        if key != "#meta" {
            variables.insert(key.clone(), val.clone());
        }
    }

    // Parse epoch
    let epoch = parse_bigint(variables.get("currentEpoch")).unwrap_or(0);

    // Parse instances
    let instances = parse_instances(variables.get("instances"))?;

    Ok(ITFState {
        index,
        variables,
        instances,
        epoch,
    })
}

/// Parse a Quint #bigint value
fn parse_bigint(value: Option<&Value>) -> Option<u64> {
    value.and_then(|v| {
        if let Some(n) = v.as_u64() {
            return Some(n);
        }
        v.get("#bigint")
            .and_then(|s| s.as_str())
            .and_then(|s| s.parse().ok())
    })
}

/// Parse a Quint #set value
fn parse_set_strings(value: Option<&Value>) -> BTreeSet<String> {
    value
        .and_then(|v| v.get("#set"))
        .and_then(|arr| arr.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Parse a Quint #map value
fn parse_map<F, T>(value: Option<&Value>, parse_val: F) -> HashMap<String, T>
where
    F: Fn(&Value) -> Option<T>,
{
    value
        .and_then(|v| v.get("#map"))
        .and_then(|arr| arr.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|pair| {
                    let pair_arr = pair.as_array()?;
                    if pair_arr.len() != 2 {
                        return None;
                    }
                    let key = pair_arr[0].as_str()?.to_string();
                    let val = parse_val(&pair_arr[1])?;
                    Some((key, val))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Parse consensus instances from ITF map
fn parse_instances(value: Option<&Value>) -> Result<HashMap<String, ConsensusState>, AuraError> {
    let empty_vec = vec![];
    let map_arr = value
        .and_then(|v| v.get("#map"))
        .and_then(|arr| arr.as_array())
        .unwrap_or(&empty_vec);

    let mut instances = HashMap::new();
    for pair in map_arr {
        let pair_arr = pair
            .as_array()
            .ok_or_else(|| AuraError::invalid("instance pair is not array"))?;
        if pair_arr.len() != 2 {
            continue;
        }

        let cid = pair_arr[0]
            .as_str()
            .ok_or_else(|| AuraError::invalid("instance cid is not string"))?
            .to_string();

        let inst = parse_instance(&pair_arr[1])?;
        instances.insert(cid, inst);
    }

    Ok(instances)
}

/// Parse a single consensus instance
fn parse_instance(value: &Value) -> Result<ConsensusState, AuraError> {
    let obj = value
        .as_object()
        .ok_or_else(|| AuraError::invalid("instance is not object"))?;

    let cid = obj
        .get("cid")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AuraError::invalid("missing cid"))?
        .to_string();

    let operation = obj
        .get("operation")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let prestate_hash = obj
        .get("prestateHash")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let threshold = parse_bigint(obj.get("threshold")).unwrap_or(1) as usize;

    let witnesses = parse_set_strings(obj.get("witnesses"));

    let initiator = obj
        .get("initiator")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let phase = parse_phase(obj.get("phase"))?;

    let proposals = parse_proposals(obj.get("proposals"))?;

    let commit_fact = parse_commit_fact(obj.get("commitFact"))?;

    let fallback_timer_active = obj
        .get("fallbackTimerActive")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let equivocators = parse_set_strings(obj.get("equivocators"));

    Ok(ConsensusState {
        cid,
        operation,
        prestate_hash,
        threshold,
        witnesses,
        initiator,
        phase,
        proposals,
        commit_fact,
        fallback_timer_active,
        equivocators,
    })
}

/// Parse consensus phase from tagged variant
fn parse_phase(value: Option<&Value>) -> Result<ConsensusPhase, AuraError> {
    let tag = value
        .and_then(|v| v.get("tag"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| AuraError::invalid("missing phase tag"))?;

    match tag {
        "ConsensusPending" => Ok(ConsensusPhase::Pending),
        "FastPathActive" => Ok(ConsensusPhase::FastPathActive),
        "FallbackActive" => Ok(ConsensusPhase::FallbackActive),
        "ConsensusCommitted" => Ok(ConsensusPhase::Committed),
        "ConsensusFailed" => Ok(ConsensusPhase::Failed),
        _ => Err(AuraError::invalid(format!("unknown phase: {}", tag))),
    }
}

/// Parse proposals set
fn parse_proposals(value: Option<&Value>) -> Result<Vec<ShareProposal>, AuraError> {
    let empty_vec = vec![];
    let set_arr = value
        .and_then(|v| v.get("#set"))
        .and_then(|arr| arr.as_array())
        .unwrap_or(&empty_vec);

    set_arr
        .iter()
        .map(|p| {
            let obj = p
                .as_object()
                .ok_or_else(|| AuraError::invalid("proposal is not object"))?;

            let witness = obj
                .get("witness")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let result_id = obj
                .get("resultId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let share = parse_share_data(obj.get("share"))?;

            Ok(ShareProposal {
                witness,
                result_id,
                share,
            })
        })
        .collect()
}

/// Parse share data
///
/// Handles two formats:
/// 1. Simple string: `"share": "share_value"` (Quint simplified format)
/// 2. Full object: `"share": {"shareValue": "...", "nonceBinding": "...", "dataBinding": "..."}`
fn parse_share_data(value: Option<&Value>) -> Result<ShareData, AuraError> {
    let val = value.ok_or_else(|| AuraError::invalid("missing share field"))?;

    // Handle simple string format
    if let Some(share_str) = val.as_str() {
        return Ok(ShareData {
            share_value: share_str.to_string(),
            nonce_binding: String::new(),
            data_binding: String::new(),
        });
    }

    // Handle full object format
    let obj = val
        .as_object()
        .ok_or_else(|| AuraError::invalid("share is not string or object"))?;

    let share_value = obj
        .get("shareValue")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let nonce_binding = obj
        .get("nonceBinding")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Parse data binding - may be nested object
    let data_binding = obj
        .get("dataBinding")
        .and_then(|v| {
            if let Some(db_obj) = v.as_object() {
                Some(format!(
                    "{}:{}:{}",
                    db_obj.get("bindCid").and_then(|x| x.as_str()).unwrap_or(""),
                    db_obj.get("bindRid").and_then(|x| x.as_str()).unwrap_or(""),
                    db_obj
                        .get("bindPHash")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                ))
            } else {
                v.as_str().map(String::from)
            }
        })
        .unwrap_or_default();

    Ok(ShareData {
        share_value,
        nonce_binding,
        data_binding,
    })
}

/// Parse optional commit fact
fn parse_commit_fact(value: Option<&Value>) -> Result<Option<PureCommitFact>, AuraError> {
    let tag = value
        .and_then(|v| v.get("tag"))
        .and_then(|v| v.as_str())
        .unwrap_or("None");

    if tag == "None" {
        return Ok(None);
    }

    let inner = value
        .and_then(|v| v.get("value"))
        .and_then(|v| v.as_object())
        .ok_or_else(|| AuraError::invalid("commit fact value is not object"))?;

    let cid = inner
        .get("cid")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let result_id = inner
        .get("rid")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let prestate_hash = inner
        .get("prestateHash")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let signature = inner
        .get("signature")
        .and_then(|v| v.get("sigValue"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Ok(Some(PureCommitFact {
        cid,
        result_id,
        signature,
        prestate_hash,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_TRACE: &str = r##"{
        "#meta": {"format": "ITF", "source": "test.qnt", "status": "ok"},
        "vars": ["instances", "currentEpoch"],
        "states": [
            {
                "#meta": {"index": 0},
                "instances": {"#map": []},
                "currentEpoch": {"#bigint": "0"}
            }
        ]
    }"##;

    #[test]
    fn test_parse_minimal_trace() {
        let trace = parse_itf_trace(MINIMAL_TRACE).unwrap();
        assert_eq!(trace.meta.format, "ITF");
        assert_eq!(trace.vars.len(), 2);
        assert_eq!(trace.states.len(), 1);
        assert_eq!(trace.states[0].epoch, 0);
    }

    #[test]
    fn test_parse_instance() {
        let trace_json = r##"{
            "#meta": {"format": "ITF", "source": "test.qnt", "status": "ok"},
            "vars": ["instances"],
            "states": [
                {
                    "#meta": {"index": 0},
                    "instances": {
                        "#map": [
                            ["cns1", {
                                "cid": "cns1",
                                "operation": "update_policy",
                                "prestateHash": "pre123",
                                "threshold": {"#bigint": "2"},
                                "witnesses": {"#set": ["w1", "w2", "w3"]},
                                "initiator": "w1",
                                "phase": {"tag": "FastPathActive", "value": {"#tup": []}},
                                "proposals": {"#set": []},
                                "commitFact": {"tag": "None", "value": {"#tup": []}},
                                "fallbackTimerActive": false,
                                "equivocators": {"#set": []}
                            }]
                        ]
                    }
                }
            ]
        }"##;

        let trace = parse_itf_trace(trace_json).unwrap();
        let state = &trace.states[0];
        assert_eq!(state.instances.len(), 1);

        let inst = state.instances.get("cns1").unwrap();
        assert_eq!(inst.cid, "cns1");
        assert_eq!(inst.operation, "update_policy");
        assert_eq!(inst.threshold, 2);
        assert_eq!(inst.witnesses.len(), 3);
        assert_eq!(inst.phase, ConsensusPhase::FastPathActive);
    }

    #[test]
    fn test_parse_bigint() {
        assert_eq!(parse_bigint(Some(&serde_json::json!(42))), Some(42));
        assert_eq!(
            parse_bigint(Some(&serde_json::json!({"#bigint": "123"}))),
            Some(123)
        );
        assert_eq!(parse_bigint(None), None);
    }

    #[test]
    fn test_parse_set_strings() {
        let set_json = serde_json::json!({"#set": ["a", "b", "c"]});
        let set = parse_set_strings(Some(&set_json));
        assert_eq!(set.len(), 3);
        assert!(set.contains("a"));
        assert!(set.contains("b"));
        assert!(set.contains("c"));
    }
}

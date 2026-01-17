//! QuintMappable implementations for consensus core types
//!
//! This module provides bidirectional conversion between Rust consensus types
//! and their Quint JSON representations for ITF trace conformance testing.
//!
//! ## Quint Type Correspondence
//! - `ConsensusPhase` ↔ `ConsensusPhase` sum type
//! - `PathSelection` ↔ `PathSelection` sum type
//! - `ShareData` ↔ `ShareData` record type
//! - `ShareProposal` ↔ `ShareProposal` record type
//! - `ConsensusState` ↔ `ConsensusInstance` record type
//! - `PureCommitFact` ↔ `CommitFact` record type

use aura_core::effects::quint::QuintMappable;
use aura_core::Result;
use serde_json::{json, Value};

use super::super::state::{
    ConsensusPhase, ConsensusState, ConsensusThreshold, PathSelection, PureCommitFact, ShareData,
    ShareProposal,
};
use crate::types::ConsensusId;
use aura_core::{hash, AuthorityId, Hash32, OperationId};
use std::str::FromStr;

fn parse_hash32(value: &str) -> Result<Hash32> {
    if value.len() == 64 {
        return Hash32::from_hex(value)
            .map_err(|e| aura_core::AuraError::invalid(format!("invalid Hash32: {e}")));
    }
    Ok(Hash32::from_bytes(value.as_bytes()))
}

fn parse_consensus_id(value: &str) -> Result<ConsensusId> {
    let raw = value.strip_prefix("consensus:").unwrap_or(value);
    if raw.len() == 64 {
        let hash = Hash32::from_hex(raw)
            .map_err(|e| aura_core::AuraError::invalid(format!("invalid ConsensusId: {e}")))?;
        return Ok(ConsensusId(hash));
    }
    Ok(ConsensusId(Hash32::from_bytes(raw.as_bytes())))
}

fn parse_authority_id(value: &str) -> Result<AuthorityId> {
    AuthorityId::from_str(value)
        .or_else(|_| Ok(AuthorityId::new_from_entropy(hash::hash(value.as_bytes()))))
}

fn parse_operation_id(value: &str) -> Result<OperationId> {
    OperationId::from_str(value)
        .or_else(|_| Ok(OperationId::new_from_entropy(hash::hash(value.as_bytes()))))
}

impl QuintMappable for ConsensusPhase {
    fn to_quint(&self) -> Value {
        // Quint sum type represented as tagged variant
        match self {
            ConsensusPhase::Pending => json!({ "tag": "ConsensusPending" }),
            ConsensusPhase::FastPathActive => json!({ "tag": "FastPathActive" }),
            ConsensusPhase::FallbackActive => json!({ "tag": "FallbackActive" }),
            ConsensusPhase::Committed => json!({ "tag": "ConsensusCommitted" }),
            ConsensusPhase::Failed => json!({ "tag": "ConsensusFailed" }),
        }
    }

    fn from_quint(value: &Value) -> Result<Self> {
        let tag = value
            .get("tag")
            .and_then(|v| v.as_str())
            .ok_or_else(|| aura_core::AuraError::invalid("expected tagged ConsensusPhase"))?;

        match tag {
            "ConsensusPending" => Ok(ConsensusPhase::Pending),
            "FastPathActive" => Ok(ConsensusPhase::FastPathActive),
            "FallbackActive" => Ok(ConsensusPhase::FallbackActive),
            "ConsensusCommitted" => Ok(ConsensusPhase::Committed),
            "ConsensusFailed" => Ok(ConsensusPhase::Failed),
            _ => Err(aura_core::AuraError::invalid(format!(
                "unknown ConsensusPhase tag: {tag}"
            ))),
        }
    }

    fn quint_type_name() -> &'static str {
        "ConsensusPhase"
    }
}

impl QuintMappable for PathSelection {
    fn to_quint(&self) -> Value {
        match self {
            PathSelection::FastPath => json!({ "tag": "FastPath" }),
            PathSelection::SlowPath => json!({ "tag": "SlowPath" }),
        }
    }

    fn from_quint(value: &Value) -> Result<Self> {
        let tag = value
            .get("tag")
            .and_then(|v| v.as_str())
            .ok_or_else(|| aura_core::AuraError::invalid("expected tagged PathSelection"))?;

        match tag {
            "FastPath" => Ok(PathSelection::FastPath),
            "SlowPath" => Ok(PathSelection::SlowPath),
            _ => Err(aura_core::AuraError::invalid(format!(
                "unknown PathSelection tag: {tag}"
            ))),
        }
    }

    fn quint_type_name() -> &'static str {
        "PathSelection"
    }
}

impl QuintMappable for ShareData {
    fn to_quint(&self) -> Value {
        // Quint: type ShareData = { shareValue: str, nonceBinding: str, dataBinding: DataBinding }
        json!({
            "shareValue": self.share_value,
            "nonceBinding": self.nonce_binding,
            "dataBinding": {
                "bindCid": "",  // Simplified - in pure model we use string
                "bindRid": "",
                "bindPHash": self.data_binding
            }
        })
    }

    fn from_quint(value: &Value) -> Result<Self> {
        let obj = value
            .as_object()
            .ok_or_else(|| aura_core::AuraError::invalid("expected object for ShareData"))?;

        let share_value = obj
            .get("shareValue")
            .and_then(|v| v.as_str())
            .ok_or_else(|| aura_core::AuraError::invalid("missing shareValue"))?
            .to_string();

        let nonce_binding = obj
            .get("nonceBinding")
            .and_then(|v| v.as_str())
            .ok_or_else(|| aura_core::AuraError::invalid("missing nonceBinding"))?
            .to_string();

        // Handle dataBinding which is a nested record
        let data_binding = if let Some(db_obj) = obj.get("dataBinding").and_then(|v| v.as_object())
        {
            // Try to extract bindPHash, fall back to concatenation
            db_obj
                .get("bindPHash")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        } else {
            String::new()
        };

        Ok(ShareData {
            share_value,
            nonce_binding,
            data_binding,
        })
    }

    fn quint_type_name() -> &'static str {
        "ShareData"
    }
}

impl QuintMappable for ShareProposal {
    fn to_quint(&self) -> Value {
        // Quint: type ShareProposal = { witness: AuthorityId, resultId: ResultId, prestateHash: PrestateHash, share: ShareData }
        json!({
            "witness": self.witness.to_string(),
            "resultId": self.result_id.to_hex(),
            "prestateHash": "", // Pure model doesn't track this separately
            "share": self.share.to_quint()
        })
    }

    fn from_quint(value: &Value) -> Result<Self> {
        let obj = value
            .as_object()
            .ok_or_else(|| aura_core::AuraError::invalid("expected object for ShareProposal"))?;

        let witness = obj
            .get("witness")
            .and_then(|v| v.as_str())
            .ok_or_else(|| aura_core::AuraError::invalid("missing witness"))?;
        let witness = parse_authority_id(witness)?;

        let result_id = obj
            .get("resultId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| aura_core::AuraError::invalid("missing resultId"))?;
        let result_id = parse_hash32(result_id)?;

        let share_value = obj
            .get("share")
            .ok_or_else(|| aura_core::AuraError::invalid("missing share"))?;
        let share = ShareData::from_quint(share_value)?;

        Ok(ShareProposal {
            witness,
            result_id,
            share,
        })
    }

    fn quint_type_name() -> &'static str {
        "ShareProposal"
    }
}

impl QuintMappable for PureCommitFact {
    fn to_quint(&self) -> Value {
        // Quint: type CommitFact = { cid: ConsensusId, rid: ResultId, prestateHash: PrestateHash, signature: ThresholdSignature, attesters: Set[AuthorityId] }
        json!({
            "cid": self.cid.0.to_hex(),
            "rid": self.result_id.to_hex(),
            "prestateHash": self.prestate_hash.to_hex(),
            "signature": {
                "sigValue": self.signature,
                "boundCid": self.cid.0.to_hex(),
                "boundRid": self.result_id.to_hex(),
                "boundPHash": self.prestate_hash.to_hex(),
                "signerSet": []  // Simplified in pure model
            },
            "attesters": []  // Simplified in pure model
        })
    }

    fn from_quint(value: &Value) -> Result<Self> {
        let obj = value
            .as_object()
            .ok_or_else(|| aura_core::AuraError::invalid("expected object for CommitFact"))?;

        let cid = obj
            .get("cid")
            .and_then(|v| v.as_str())
            .ok_or_else(|| aura_core::AuraError::invalid("missing cid"))?;
        let cid = parse_consensus_id(cid)?;

        let result_id = obj
            .get("rid")
            .and_then(|v| v.as_str())
            .ok_or_else(|| aura_core::AuraError::invalid("missing rid"))?;
        let result_id = parse_hash32(result_id)?;

        let prestate_hash = obj
            .get("prestateHash")
            .and_then(|v| v.as_str())
            .ok_or_else(|| aura_core::AuraError::invalid("missing prestateHash"))?;
        let prestate_hash = parse_hash32(prestate_hash)?;

        // Extract signature from nested object
        let signature = if let Some(sig_obj) = obj.get("signature").and_then(|v| v.as_object()) {
            sig_obj
                .get("sigValue")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        } else {
            String::new()
        };

        Ok(PureCommitFact {
            cid,
            result_id,
            signature,
            prestate_hash,
        })
    }

    fn quint_type_name() -> &'static str {
        "CommitFact"
    }
}

impl QuintMappable for ConsensusState {
    fn to_quint(&self) -> Value {
        // Quint: type ConsensusInstance = { cid, operation, prestateHash, threshold, witnesses, initiator, phase, proposals, commitFact, fallbackTimerActive, equivocators }
        let witnesses: Vec<Value> = self.witnesses.iter().map(|w| json!(w)).collect();
        let proposals: Vec<Value> = self.proposals.iter().map(|p| p.to_quint()).collect();
        let equivocators: Vec<Value> = self.equivocators.iter().map(|e| json!(e)).collect();

        let commit_fact = match &self.commit_fact {
            Some(cf) => json!({ "tag": "Some", "value": cf.to_quint() }),
            None => json!({ "tag": "None" }),
        };

        json!({
            "cid": self.cid.0.to_hex(),
            "operation": self.operation.to_string(),
            "prestateHash": self.prestate_hash.to_hex(),
            "threshold": self.threshold.get(),
            "witnesses": witnesses,
            "initiator": self.initiator.to_string(),
            "phase": self.phase.to_quint(),
            "proposals": proposals,
            "commitFact": commit_fact,
            "fallbackTimerActive": self.fallback_timer_active,
            "equivocators": equivocators
        })
    }

    fn from_quint(value: &Value) -> Result<Self> {
        let obj = value.as_object().ok_or_else(|| {
            aura_core::AuraError::invalid("expected object for ConsensusInstance")
        })?;

        let cid = obj
            .get("cid")
            .and_then(|v| v.as_str())
            .ok_or_else(|| aura_core::AuraError::invalid("missing cid"))?;
        let cid = parse_consensus_id(cid)?;

        let operation = obj
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or_else(|| aura_core::AuraError::invalid("missing operation"))?;
        let operation = parse_operation_id(operation)?;

        let prestate_hash = obj
            .get("prestateHash")
            .and_then(|v| v.as_str())
            .ok_or_else(|| aura_core::AuraError::invalid("missing prestateHash"))?;
        let prestate_hash = parse_hash32(prestate_hash)?;

        let threshold_value = obj
            .get("threshold")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| aura_core::AuraError::invalid("missing threshold"))?;
        let threshold_value = u16::try_from(threshold_value)
            .map_err(|_| aura_core::AuraError::invalid("threshold must fit in u16"))?;
        let threshold = ConsensusThreshold::new(threshold_value)
            .ok_or_else(|| aura_core::AuraError::invalid("threshold must be >= 1"))?;

        let witnesses: std::collections::BTreeSet<AuthorityId> = obj
            .get("witnesses")
            .and_then(|v| v.as_array())
            .ok_or_else(|| aura_core::AuraError::invalid("missing witnesses"))?
            .iter()
            .filter_map(|v| v.as_str())
            .map(parse_authority_id)
            .collect::<Result<std::collections::BTreeSet<_>>>()?;

        let initiator = obj
            .get("initiator")
            .and_then(|v| v.as_str())
            .ok_or_else(|| aura_core::AuraError::invalid("missing initiator"))?;
        let initiator = parse_authority_id(initiator)?;

        let phase_value = obj
            .get("phase")
            .ok_or_else(|| aura_core::AuraError::invalid("missing phase"))?;
        let phase = ConsensusPhase::from_quint(phase_value)?;

        let proposals: Vec<ShareProposal> = obj
            .get("proposals")
            .and_then(|v| v.as_array())
            .ok_or_else(|| aura_core::AuraError::invalid("missing proposals"))?
            .iter()
            .map(ShareProposal::from_quint)
            .collect::<Result<Vec<_>>>()?;

        let commit_fact = if let Some(cf_obj) = obj.get("commitFact") {
            let tag = cf_obj.get("tag").and_then(|v| v.as_str()).unwrap_or("None");
            if tag == "Some" {
                let cf_value = cf_obj
                    .get("value")
                    .ok_or_else(|| aura_core::AuraError::invalid("missing commitFact value"))?;
                Some(PureCommitFact::from_quint(cf_value)?)
            } else {
                None
            }
        } else {
            None
        };

        let fallback_timer_active = obj
            .get("fallbackTimerActive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let equivocators: std::collections::BTreeSet<AuthorityId> = obj
            .get("equivocators")
            .and_then(|v| v.as_array())
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|v| v.as_str())
            .map(parse_authority_id)
            .collect::<Result<_>>()?;

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

    fn quint_type_name() -> &'static str {
        "ConsensusInstance"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn test_authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn test_hash(seed: u8) -> Hash32 {
        Hash32::new([seed; 32])
    }

    fn test_consensus_id(seed: u8) -> ConsensusId {
        ConsensusId(Hash32::new([seed; 32]))
    }

    fn test_operation(seed: u8) -> OperationId {
        OperationId::new_from_entropy([seed; 32])
    }

    #[test]
    fn test_consensus_phase_roundtrip() {
        let phases = [
            ConsensusPhase::Pending,
            ConsensusPhase::FastPathActive,
            ConsensusPhase::FallbackActive,
            ConsensusPhase::Committed,
            ConsensusPhase::Failed,
        ];

        for phase in phases {
            let quint = phase.to_quint();
            let restored = ConsensusPhase::from_quint(&quint).unwrap();
            assert_eq!(phase, restored);
        }
    }

    #[test]
    fn test_path_selection_roundtrip() {
        let paths = [PathSelection::FastPath, PathSelection::SlowPath];

        for path in paths {
            let quint = path.to_quint();
            let restored = PathSelection::from_quint(&quint).unwrap();
            assert_eq!(path, restored);
        }
    }

    #[test]
    fn test_share_data_roundtrip() {
        let share = ShareData {
            share_value: "share123".to_string(),
            nonce_binding: "nonce456".to_string(),
            data_binding: "binding789".to_string(),
        };

        let quint = share.to_quint();
        let restored = ShareData::from_quint(&quint).unwrap();
        assert_eq!(share.share_value, restored.share_value);
        assert_eq!(share.nonce_binding, restored.nonce_binding);
    }

    #[test]
    fn test_share_proposal_roundtrip() {
        let proposal = ShareProposal {
            witness: test_authority(1),
            result_id: test_hash(9),
            share: ShareData {
                share_value: "share".to_string(),
                nonce_binding: "nonce".to_string(),
                data_binding: "binding".to_string(),
            },
        };

        let quint = proposal.to_quint();
        let restored = ShareProposal::from_quint(&quint).unwrap();
        assert_eq!(proposal.witness, restored.witness);
        assert_eq!(proposal.result_id, restored.result_id);
    }

    #[test]
    fn test_commit_fact_roundtrip() {
        let cf = PureCommitFact {
            cid: test_consensus_id(1),
            result_id: test_hash(9),
            signature: "sig".to_string(),
            prestate_hash: test_hash(3),
        };

        let quint = cf.to_quint();
        let restored = PureCommitFact::from_quint(&quint).unwrap();
        assert_eq!(cf.cid, restored.cid);
        assert_eq!(cf.result_id, restored.result_id);
        assert_eq!(cf.prestate_hash, restored.prestate_hash);
    }

    #[test]
    fn test_consensus_state_roundtrip() {
        let mut witnesses = BTreeSet::new();
        witnesses.insert(test_authority(1));
        witnesses.insert(test_authority(2));
        witnesses.insert(test_authority(3));

        let state = ConsensusState {
            cid: test_consensus_id(1),
            operation: test_operation(2),
            prestate_hash: test_hash(3),
            threshold: ConsensusThreshold::new(2).expect("threshold"),
            witnesses,
            initiator: test_authority(1),
            phase: ConsensusPhase::FastPathActive,
            proposals: vec![],
            commit_fact: None,
            fallback_timer_active: false,
            equivocators: BTreeSet::new(),
        };

        let quint = state.to_quint();
        let restored = ConsensusState::from_quint(&quint).unwrap();

        assert_eq!(state.cid, restored.cid);
        assert_eq!(state.operation, restored.operation);
        assert_eq!(state.prestate_hash, restored.prestate_hash);
        assert_eq!(state.threshold, restored.threshold);
        assert_eq!(state.witnesses, restored.witnesses);
        assert_eq!(state.phase, restored.phase);
    }

    #[test]
    fn test_consensus_state_with_commit_fact() {
        let mut witnesses = BTreeSet::new();
        witnesses.insert(test_authority(1));
        witnesses.insert(test_authority(2));

        let state = ConsensusState {
            cid: test_consensus_id(1),
            operation: test_operation(2),
            prestate_hash: test_hash(3),
            threshold: ConsensusThreshold::new(2).expect("threshold"),
            witnesses,
            initiator: test_authority(1),
            phase: ConsensusPhase::Committed,
            proposals: vec![],
            commit_fact: Some(PureCommitFact {
                cid: test_consensus_id(1),
                result_id: test_hash(9),
                signature: "sig".to_string(),
                prestate_hash: test_hash(3),
            }),
            fallback_timer_active: false,
            equivocators: BTreeSet::new(),
        };

        let quint = state.to_quint();
        let restored = ConsensusState::from_quint(&quint).unwrap();

        assert!(restored.commit_fact.is_some());
        assert_eq!(
            state.commit_fact.as_ref().unwrap().cid,
            restored.commit_fact.as_ref().unwrap().cid
        );
    }
}

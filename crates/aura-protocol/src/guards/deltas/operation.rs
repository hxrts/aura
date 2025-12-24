//! Journal operation types for delta fact application
//!
//! Local types for journal operations that mirror aura-journal types for guard processing.

use serde_json::Value as JsonValue;

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
    FormRelationship {
        relationship_id: String,
        device_a: String,
        device_b: String,
        trust_level: String,
    },
    EnrollGuardian {
        guardian_id: String,
        device_id: String,
        capabilities: Vec<String>,
    },
    DeriveKey {
        derivation_id: String,
        context: String,
        derived_for: String,
    },
    UpdateFlowBudget {
        context_id: String,
        peer_id: String,
        new_limit: u32,
        cost: Option<u32>,
    },
    InitiateRecovery {
        recovery_id: String,
        account_id: String,
        requester: String,
        guardians_required: usize,
    },
    CommitStorage {
        content_hash: String,
        size: u64,
        storage_nodes: Vec<String>,
    },
    DeployOta {
        version: String,
        target_epoch: u64,
        deployment_hash: String,
    },
}

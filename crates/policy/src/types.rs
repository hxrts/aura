// Core policy types

use aura_journal::{AccountId, DeviceId};
use serde::{Deserialize, Serialize};

/// Risk tier for operations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskTier {
    /// Low-risk operations (read-only, etc.)
    Low,
    /// Medium-risk operations (write data, add peer)
    Medium,
    /// High-risk operations (add device, modify guardians)
    High,
    /// Critical operations (remove device, change threshold)
    Critical,
}

/// Device posture - security state of a device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevicePosture {
    pub device_id: DeviceId,
    pub device_type: DeviceType,
    pub is_hardware_backed: bool,
    pub has_secure_boot: bool,
    pub is_jailbroken: bool,
    pub last_attestation: Option<u64>,
}

/// Device type for policy evaluation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceType {
    /// Hardware-backed native device
    Native,
    /// Guardian device
    Guardian,
    /// Browser-based device
    Browser,
}

impl From<aura_journal::DeviceType> for DeviceType {
    fn from(dt: aura_journal::DeviceType) -> Self {
        match dt {
            aura_journal::DeviceType::Native => DeviceType::Native,
            aura_journal::DeviceType::Guardian => DeviceType::Guardian,
            aura_journal::DeviceType::Browser => DeviceType::Browser,
        }
    }
}

/// Operation being requested
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    pub operation_type: OperationType,
    pub risk_tier: RiskTier,
    pub resource: Option<String>,
}

/// Types of operations
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationType {
    // Identity operations
    AddDevice,
    RemoveDevice,
    AddGuardian,
    RemoveGuardian,
    BumpEpoch,
    
    // Storage operations
    StoreObject,
    FetchObject,
    DeleteObject,
    ReplicateChunk,
    
    // Threshold operations
    InitiateDkg,
    SignThreshold,
    ReshareKeys,
}

impl Operation {
    pub fn add_device() -> Self {
        Operation {
            operation_type: OperationType::AddDevice,
            risk_tier: RiskTier::High,
            resource: None,
        }
    }
    
    pub fn remove_device(device_id: DeviceId) -> Self {
        Operation {
            operation_type: OperationType::RemoveDevice,
            risk_tier: RiskTier::Critical,
            resource: Some(format!("device:{}", device_id)),
        }
    }
    
    pub fn add_guardian() -> Self {
        Operation {
            operation_type: OperationType::AddGuardian,
            risk_tier: RiskTier::High,
            resource: None,
        }
    }
    
    pub fn store_object(cid: &str) -> Self {
        Operation {
            operation_type: OperationType::StoreObject,
            risk_tier: RiskTier::Low,
            resource: Some(format!("object:{}", cid)),
        }
    }
    
    pub fn fetch_object(cid: &str) -> Self {
        Operation {
            operation_type: OperationType::FetchObject,
            risk_tier: RiskTier::Low,
            resource: Some(format!("object:{}", cid)),
        }
    }
}

/// Policy decision context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyContext {
    pub account_id: AccountId,
    pub requester: DeviceId,
    pub device_posture: DevicePosture,
    pub operation: Operation,
    pub guardians_count: u32,
    pub active_devices_count: u32,
    pub session_epoch: u64,
}

/// Policy decision result
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    /// Allow the operation
    Allow,
    /// Deny the operation with reason
    Deny(String),
    /// Allow with additional requirements
    AllowWithConstraints(Vec<Constraint>),
}

/// Constraints that must be satisfied
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Constraint {
    /// Requires threshold signature
    RequiresThresholdSignature,
    /// Requires specific number of guardians to approve
    RequiresGuardianApprovals(u32),
    /// Requires cooldown period (seconds)
    RequiresCooldown(u64),
    /// Requires device attestation
    RequiresAttestation,
}


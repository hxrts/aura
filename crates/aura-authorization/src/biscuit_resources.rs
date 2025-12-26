//! Resource scope definitions for Biscuit tokens.
//!
//! Defines the hierarchical resource model
//! (storage, journal, relay, recovery, admin)
//! and provides Datalog pattern generation for
//! capability-based access control.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResourceScope {
    Storage {
        category: StorageCategory,
        path: String,
    },
    Journal {
        account_id: String,
        operation: JournalOp,
    },
    Relay {
        channel_id: String,
    },
    Recovery {
        recovery_type: RecoveryType,
    },
    Admin {
        operation: AdminOperation,
    },
}

impl ResourceScope {
    pub fn to_datalog_pattern(&self) -> String {
        match self {
            ResourceScope::Storage { category, path } => {
                format!(
                    "resource(\"/storage/{}/{}\"), resource_type(\"storage\")",
                    category.as_str(),
                    path
                )
            }
            ResourceScope::Journal {
                account_id,
                operation,
            } => {
                format!(
                    "resource(\"/journal/{}/{}\"), resource_type(\"journal\")",
                    account_id,
                    operation.as_str()
                )
            }
            ResourceScope::Relay { channel_id } => {
                format!(
                    "resource(\"/relay/{}\"), resource_type(\"relay\")",
                    channel_id
                )
            }
            ResourceScope::Recovery { recovery_type } => {
                format!(
                    "resource(\"/recovery/{}\"), resource_type(\"recovery\")",
                    recovery_type.as_str()
                )
            }
            ResourceScope::Admin { operation } => {
                format!(
                    "resource(\"/admin/{}\"), resource_type(\"admin\")",
                    operation.as_str()
                )
            }
        }
    }

    pub fn resource_pattern(&self) -> String {
        match self {
            ResourceScope::Storage { category, path } => {
                format!("/storage/{}/{}", category.as_str(), path)
            }
            ResourceScope::Journal {
                account_id,
                operation,
            } => {
                format!("/journal/{}/{}", account_id, operation.as_str())
            }
            ResourceScope::Relay { channel_id } => {
                format!("/relay/{}", channel_id)
            }
            ResourceScope::Recovery { recovery_type } => {
                format!("/recovery/{}", recovery_type.as_str())
            }
            ResourceScope::Admin { operation } => {
                format!("/admin/{}", operation.as_str())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageCategory {
    Personal,
    Shared,
    Public,
}

impl StorageCategory {
    pub fn as_str(&self) -> &str {
        match self {
            StorageCategory::Personal => "personal",
            StorageCategory::Shared => "shared",
            StorageCategory::Public => "public",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JournalOp {
    Read,
    Write,
    Sync,
    Snapshot,
}

impl JournalOp {
    pub fn as_str(&self) -> &str {
        match self {
            JournalOp::Read => "read",
            JournalOp::Write => "write",
            JournalOp::Sync => "sync",
            JournalOp::Snapshot => "snapshot",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryType {
    DeviceKey,
    AccountAccess,
    GuardianSet,
    EmergencyFreeze,
}

impl RecoveryType {
    pub fn as_str(&self) -> &str {
        match self {
            RecoveryType::DeviceKey => "device_key",
            RecoveryType::AccountAccess => "account_access",
            RecoveryType::GuardianSet => "guardian_set",
            RecoveryType::EmergencyFreeze => "emergency_freeze",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AdminOperation {
    AddGuardian,
    RemoveGuardian,
    ModifyThreshold,
    RevokeDevice,
}

impl AdminOperation {
    pub fn as_str(&self) -> &str {
        match self {
            AdminOperation::AddGuardian => "add_guardian",
            AdminOperation::RemoveGuardian => "remove_guardian",
            AdminOperation::ModifyThreshold => "modify_threshold",
            AdminOperation::RevokeDevice => "revoke_device",
        }
    }
}

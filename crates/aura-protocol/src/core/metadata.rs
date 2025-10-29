//! Shared protocol metadata enumerations and descriptors.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Protocol type enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProtocolType {
    /// Counter reservation protocol.
    Counter,
    /// Deterministic Key Derivation protocol.
    Dkd,
    /// Distributed group lifecycle (DKG + messaging).
    Group,
    /// Key resharing protocol.
    Resharing,
    /// Account recovery protocol.
    Recovery,
    /// Resource locking protocol.
    Locking,
    /// Lock acquisition protocol.
    LockAcquisition,
    /// FROST Distributed Key Generation.
    FrostDkg,
    /// FROST Threshold Signing.
    FrostSigning,
    /// Storage operations protocol.
    Storage,
}

impl fmt::Display for ProtocolType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolType::Counter => write!(f, "counter"),
            ProtocolType::Dkd => write!(f, "dkd"),
            ProtocolType::Group => write!(f, "group"),
            ProtocolType::Resharing => write!(f, "resharing"),
            ProtocolType::Recovery => write!(f, "recovery"),
            ProtocolType::Locking => write!(f, "locking"),
            ProtocolType::LockAcquisition => write!(f, "lock-acquisition"),
            ProtocolType::FrostDkg => write!(f, "frost-dkg"),
            ProtocolType::FrostSigning => write!(f, "frost-signing"),
            ProtocolType::Storage => write!(f, "storage"),
        }
    }
}

impl ProtocolType {
    /// Retrieve all protocol types.
    pub fn all() -> &'static [ProtocolType] {
        &[
            ProtocolType::Counter,
            ProtocolType::Dkd,
            ProtocolType::Group,
            ProtocolType::Resharing,
            ProtocolType::Recovery,
            ProtocolType::Locking,
            ProtocolType::LockAcquisition,
            ProtocolType::FrostDkg,
            ProtocolType::FrostSigning,
            ProtocolType::Storage,
        ]
    }

    /// Whether protocol supports threshold operations.
    pub fn supports_threshold(&self) -> bool {
        matches!(
            self,
            ProtocolType::Counter
                | ProtocolType::Group
                | ProtocolType::Dkd
                | ProtocolType::Resharing
                | ProtocolType::Recovery
                | ProtocolType::FrostDkg
                | ProtocolType::FrostSigning
                | ProtocolType::Storage
        )
    }

    /// Whether protocol mutates account state.
    pub fn modifies_account_state(&self) -> bool {
        matches!(
            self,
            ProtocolType::Counter
                | ProtocolType::Group
                | ProtocolType::Dkd
                | ProtocolType::Resharing
                | ProtocolType::Recovery
                | ProtocolType::FrostDkg
                | ProtocolType::Storage
        )
    }

    /// Typical duration category.
    pub fn duration_category(&self) -> ProtocolDuration {
        match self {
            ProtocolType::Counter => ProtocolDuration::Short,
            ProtocolType::Dkd => ProtocolDuration::Short,
            ProtocolType::Group => ProtocolDuration::Medium,
            ProtocolType::Resharing => ProtocolDuration::Medium,
            ProtocolType::Recovery => ProtocolDuration::Long,
            ProtocolType::Locking => ProtocolDuration::Short,
            ProtocolType::LockAcquisition => ProtocolDuration::Short,
            ProtocolType::FrostDkg => ProtocolDuration::Short,
            ProtocolType::FrostSigning => ProtocolDuration::Short,
            ProtocolType::Storage => ProtocolDuration::Medium,
        }
    }
}

/// Operation type enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum OperationType {
    /// Counter reservation operation.
    Counter,
    /// Deterministic Key Derivation operation.
    Dkd,
    /// Distributed group operation (DKG, membership, messaging).
    Group,
    /// Key resharing operation.
    Resharing,
    /// Account recovery operation.
    Recovery,
    /// Resource locking operation.
    Locking,
    /// Threshold signing operation.
    Signing,
    /// Storage operation.
    Storage,
}

impl fmt::Display for OperationType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OperationType::Counter => write!(f, "counter"),
            OperationType::Dkd => write!(f, "dkd"),
            OperationType::Group => write!(f, "group"),
            OperationType::Resharing => write!(f, "resharing"),
            OperationType::Recovery => write!(f, "recovery"),
            OperationType::Locking => write!(f, "locking"),
            OperationType::Signing => write!(f, "signing"),
            OperationType::Storage => write!(f, "storage"),
        }
    }
}

impl From<ProtocolType> for OperationType {
    fn from(protocol: ProtocolType) -> Self {
        match protocol {
            ProtocolType::Counter => OperationType::Counter,
            ProtocolType::Dkd => OperationType::Dkd,
            ProtocolType::Group => OperationType::Group,
            ProtocolType::Resharing => OperationType::Resharing,
            ProtocolType::Recovery => OperationType::Recovery,
            ProtocolType::Locking => OperationType::Locking,
            ProtocolType::LockAcquisition => OperationType::Locking,
            ProtocolType::FrostDkg => OperationType::Dkd,
            ProtocolType::FrostSigning => OperationType::Signing,
            ProtocolType::Storage => OperationType::Storage,
        }
    }
}

/// Protocol duration categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProtocolDuration {
    /// Seconds to minutes.
    Short,
    /// Minutes to hours.
    Medium,
    /// Hours to days.
    Long,
}

impl fmt::Display for ProtocolDuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolDuration::Short => write!(f, "short"),
            ProtocolDuration::Medium => write!(f, "medium"),
            ProtocolDuration::Long => write!(f, "long"),
        }
    }
}

/// Protocol priority levels.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default,
)]
pub enum ProtocolPriority {
    /// Background operations.
    Low,
    /// Standard operations.
    #[default]
    Normal,
    /// Important operations.
    High,
    /// Security or recovery operations.
    Critical,
}

impl fmt::Display for ProtocolPriority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolPriority::Low => write!(f, "low"),
            ProtocolPriority::Normal => write!(f, "normal"),
            ProtocolPriority::High => write!(f, "high"),
            ProtocolPriority::Critical => write!(f, "critical"),
        }
    }
}

/// Protocol execution modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ProtocolMode {
    /// Execute synchronously.
    Synchronous,
    /// Execute asynchronously.
    #[default]
    Asynchronous,
    /// Requires user interaction.
    Interactive,
}

impl fmt::Display for ProtocolMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolMode::Synchronous => write!(f, "synchronous"),
            ProtocolMode::Asynchronous => write!(f, "asynchronous"),
            ProtocolMode::Interactive => write!(f, "interactive"),
        }
    }
}

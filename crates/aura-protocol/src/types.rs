//! Protocol orchestration types
//!
//! Types for protocol coordination, session management, and execution control.
//! These types represent orchestration-level concerns (Layer 4).

use serde::{Deserialize, Serialize};
use std::fmt;

/// Protocol type enumeration
///
/// Identifies the type of protocol being executed in a session and is used for
/// dispatch/analytics across the platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProtocolType {
    /// Deterministic Key Derivation protocol
    Dkd,
    /// Counter reservation protocol
    Counter,
    /// Key resharing protocol for threshold updates
    Resharing,
    /// Resource locking protocol
    Locking,
    /// Lock acquisition protocol
    LockAcquisition,
    /// Effect API compaction protocol for state optimization
    Compaction,
}

impl fmt::Display for ProtocolType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolType::Dkd => write!(f, "dkd"),
            ProtocolType::Counter => write!(f, "counter"),
            ProtocolType::Resharing => write!(f, "resharing"),
            ProtocolType::Locking => write!(f, "locking"),
            ProtocolType::LockAcquisition => write!(f, "lock-acquisition"),
            ProtocolType::Compaction => write!(f, "compaction"),
        }
    }
}

impl ProtocolType {
    /// Get all protocol types
    pub fn all() -> &'static [ProtocolType] {
        &[
            ProtocolType::Dkd,
            ProtocolType::Counter,
            ProtocolType::Resharing,
            ProtocolType::Locking,
            ProtocolType::LockAcquisition,
            ProtocolType::Compaction,
        ]
    }

    /// Check if this protocol supports threshold operations
    pub fn supports_threshold(&self) -> bool {
        matches!(
            self,
            ProtocolType::Dkd | ProtocolType::Counter | ProtocolType::Resharing
        )
    }

    /// Check if this protocol modifies account state
    pub fn modifies_account_state(&self) -> bool {
        matches!(
            self,
            ProtocolType::Dkd
                | ProtocolType::Counter
                | ProtocolType::Resharing
                | ProtocolType::Compaction
        )
    }

    /// Get the typical duration category for this protocol
    pub fn duration_category(&self) -> ProtocolDuration {
        match self {
            ProtocolType::Dkd => ProtocolDuration::Short,
            ProtocolType::Counter => ProtocolDuration::Short,
            ProtocolType::Resharing => ProtocolDuration::Medium,
            ProtocolType::Locking => ProtocolDuration::Short,
            ProtocolType::LockAcquisition => ProtocolDuration::Short,
            ProtocolType::Compaction => ProtocolDuration::Medium,
        }
    }
}

/// Protocol duration categories
///
/// Indicates the expected duration category for different protocols.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProtocolDuration {
    /// Short duration (seconds to minutes)
    Short,
    /// Medium duration (minutes to hours)
    Medium,
    /// Long duration (hours to days)
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

/// Protocol priority levels
///
/// Indicates the priority level for protocol execution and resource allocation.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default,
)]
pub enum ProtocolPriority {
    /// Low priority - background operations
    Low,
    /// Normal priority - standard operations
    #[default]
    Normal,
    /// High priority - important operations
    High,
    /// Critical priority - security or recovery operations
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

/// Protocol execution mode
///
/// Indicates how a protocol should be executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ProtocolMode {
    /// Synchronous execution - wait for completion
    Synchronous,
    /// Asynchronous execution - run in background
    #[default]
    Asynchronous,
    /// Interactive execution - requires user interaction
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

/// Protocol session status enumeration for coordination
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProtocolSessionStatus {
    /// Session is initializing
    Initializing,
    /// Session is actively running
    Active,
    /// Session completed successfully
    Completed,
    /// Session failed with error
    Failed(String),
    /// Session was terminated
    Terminated,
}

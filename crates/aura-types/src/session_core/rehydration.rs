//! Protocol Rehydration for Crash Recovery
//!
//! This module provides utilities for reconstructing protocol state after crashes
//! or restarts by analyzing journal evidence.

#![allow(clippy::result_large_err)] // ErrorContext provides valuable debugging info

use crate::{errors::SessionError, SessionProtocol};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

/// Rehydration evidence from journal for crash recovery
#[derive(Debug, Clone)]
pub struct RehydrationEvidence<Event>
where
    Event: Send + Sync + Clone + std::fmt::Debug + 'static,
{
    /// Events from the journal relevant to this protocol
    pub events: Vec<Event>,
    /// Protocol session ID
    pub session_id: uuid::Uuid,
    /// Last known state from journal
    pub last_state: Option<String>,
}

/// Manager for protocol rehydration operations
#[derive(Debug)]
pub struct RehydrationManager {
    /// Device ID for this rehydration manager
    device_id: uuid::Uuid,
    /// Cache of rehydrated protocols
    protocol_cache: HashMap<Uuid, Box<dyn ProtocolEvidence>>,
}

impl RehydrationManager {
    /// Create a new rehydration manager
    pub fn new(device_id: uuid::Uuid) -> Self {
        Self {
            device_id,
            protocol_cache: HashMap::new(),
        }
    }

    /// Rehydrate a protocol from journal evidence
    pub fn rehydrate_protocol<P, E>(
        &mut self,
        evidence: RehydrationEvidence<E>,
    ) -> Result<P, CrashRecoveryError>
    where
        E: Send + Sync + Clone + std::fmt::Debug + 'static,
        P: SessionProtocol + StateRecovery<Evidence = RehydrationEvidence<E>>,
    {
        // Validate evidence is sufficient
        if !P::validate_rehydration_evidence(&evidence) {
            return Err(CrashRecoveryError::InsufficientEvidence {
                protocol_id: evidence.session_id,
                missing_data: "Required events or state information missing".to_string(),
            });
        }

        // Attempt rehydration
        let protocol =
            P::rehydrate_from_evidence(self.device_id, evidence.clone()).map_err(|e| {
                CrashRecoveryError::RehydrationFailed {
                    protocol_id: evidence.session_id,
                    error: e.to_string(),
                }
            })?;

        // Cache the evidence for future use
        self.protocol_cache
            .insert(evidence.session_id, Box::new(evidence));

        Ok(protocol)
    }

    /// Get cached evidence for a protocol
    pub fn get_cached_evidence(&self, protocol_id: Uuid) -> Option<&dyn ProtocolEvidence> {
        self.protocol_cache.get(&protocol_id).map(|e| e.as_ref())
    }

    /// Clear the protocol cache
    pub fn clear_cache(&mut self) {
        self.protocol_cache.clear();
    }
}

/// Trait for evidence that can be used for protocol rehydration
pub trait ProtocolEvidence: Send + Sync + std::fmt::Debug {
    /// Get the protocol session ID
    fn protocol_id(&self) -> Uuid;

    /// Get the last known state, if available
    fn last_known_state(&self) -> Option<&str>;

    /// Validate that this evidence is complete
    fn is_complete(&self) -> bool;
}

impl<Event> ProtocolEvidence for RehydrationEvidence<Event>
where
    Event: Send + Sync + Clone + std::fmt::Debug + 'static,
{
    fn protocol_id(&self) -> Uuid {
        self.session_id
    }

    fn last_known_state(&self) -> Option<&str> {
        self.last_state.as_deref()
    }

    fn is_complete(&self) -> bool {
        !self.events.is_empty()
    }
}

/// Trait for protocols that support state recovery from evidence
pub trait StateRecovery: SessionProtocol {
    /// The type of evidence required for rehydration
    type Evidence: ProtocolEvidence;

    /// Rehydrate the protocol from journal evidence
    fn rehydrate_from_evidence(
        device_id: uuid::Uuid,
        evidence: Self::Evidence,
    ) -> Result<Self, SessionError>;

    /// Validate that evidence is sufficient for rehydration
    fn validate_rehydration_evidence(evidence: &Self::Evidence) -> bool;

    /// Extract the current state from evidence
    fn extract_state_from_evidence(evidence: &Self::Evidence) -> Option<String>;

    /// Check if rehydration is possible for this protocol type
    fn supports_rehydration() -> bool {
        true
    }
}

/// Errors that can occur during crash recovery
#[derive(Error, Debug, Clone)]
pub enum CrashRecoveryError {
    /// Insufficient evidence to rehydrate protocol
    #[error("Insufficient evidence for protocol {protocol_id}: {missing_data}")]
    InsufficientEvidence {
        /// Identifier of the protocol that lacks sufficient evidence
        protocol_id: Uuid,
        /// Description of what data is missing
        missing_data: String,
    },

    /// Protocol rehydration failed
    #[error("Failed to rehydrate protocol {protocol_id}: {error}")]
    RehydrationFailed {
        /// Identifier of the protocol that failed to rehydrate
        protocol_id: Uuid,
        /// Error message describing the failure
        error: String,
    },

    /// Conflicting evidence found
    #[error("Conflicting evidence for protocol {protocol_id}: {details}")]
    ConflictingEvidence {
        /// Identifier of the protocol with conflicting evidence
        protocol_id: Uuid,
        /// Details about the conflicting evidence
        details: String,
    },

    /// Evidence corruption detected
    #[error("Evidence corruption detected for protocol {protocol_id}")]
    CorruptedEvidence {
        /// Identifier of the protocol with corrupted evidence
        protocol_id: Uuid,
    },

    /// Journal is inconsistent
    #[error("Journal inconsistency detected: {details}")]
    JournalInconsistency {
        /// Details about the journal inconsistency
        details: String,
    },

    /// Protocol version mismatch
    #[error("Protocol version mismatch for {protocol_id}: expected {expected}, found {found}")]
    VersionMismatch {
        /// Identifier of the protocol with version mismatch
        protocol_id: Uuid,
        /// Expected protocol version
        expected: String,
        /// Actual protocol version found
        found: String,
    },
}

/// Configuration for rehydration operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RehydrationConfig {
    /// Maximum age of events to consider (in seconds)
    pub max_event_age_seconds: u64,
    /// Whether to allow partial rehydration
    pub allow_partial_rehydration: bool,
    /// Timeout for rehydration operations (in milliseconds)
    pub rehydration_timeout_ms: u64,
    /// Whether to validate all witnesses during rehydration
    pub validate_witnesses: bool,
}

impl Default for RehydrationConfig {
    fn default() -> Self {
        Self {
            max_event_age_seconds: 24 * 60 * 60, // 24 hours
            allow_partial_rehydration: false,
            rehydration_timeout_ms: 30_000, // 30 seconds
            validate_witnesses: true,
        }
    }
}

/// Utility for analyzing journal evidence
#[derive(Debug)]
pub struct EvidenceAnalyzer {
    #[allow(dead_code)]
    config: RehydrationConfig,
}

impl EvidenceAnalyzer {
    /// Create a new evidence analyzer
    pub fn new(config: RehydrationConfig) -> Self {
        Self { config }
    }

    /// Analyze evidence quality for a protocol
    pub fn analyze_evidence_quality<E>(&self, evidence: &RehydrationEvidence<E>) -> EvidenceQuality
    where
        E: Send + Sync + Clone + std::fmt::Debug + 'static,
    {
        let event_count = evidence.events.len();
        let has_state = evidence.last_state.is_some();

        if event_count == 0 {
            EvidenceQuality::Insufficient
        } else if has_state && event_count >= 3 {
            EvidenceQuality::Excellent
        } else if has_state || event_count >= 2 {
            EvidenceQuality::Good
        } else {
            EvidenceQuality::Poor
        }
    }

    /// Check if evidence contains basic issues (simplified for generic use)
    pub fn basic_evidence_check<E>(&self, evidence: &RehydrationEvidence<E>) -> Vec<String>
    where
        E: Send + Sync + Clone + std::fmt::Debug + 'static,
    {
        let mut issues = Vec::new();

        // Check for empty evidence
        if evidence.events.is_empty() {
            issues.push("No events provided in evidence".to_string());
        }

        // Check for invalid session ID
        if evidence.session_id.is_nil() {
            issues.push("Invalid session ID (nil UUID)".to_string());
        }

        issues
    }
}

/// Quality assessment of rehydration evidence
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvidenceQuality {
    /// Evidence is insufficient for rehydration
    Insufficient,
    /// Evidence is present but low quality
    Poor,
    /// Evidence is adequate for rehydration
    Good,
    /// Evidence is high quality with complete information
    Excellent,
}

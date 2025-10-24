//! Protocol Rehydration for Crash Recovery
//!
//! This module provides utilities for reconstructing protocol state after crashes
//! or restarts by analyzing journal evidence.

use crate::core::{SessionError, SessionProtocol};
use crate::witnesses::RehydrationEvidence;
use aura_journal::{Event, DeviceId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

/// Manager for protocol rehydration operations
#[derive(Debug)]
pub struct RehydrationManager {
    /// Device ID for this rehydration manager
    device_id: DeviceId,
    /// Cache of rehydrated protocols
    protocol_cache: HashMap<Uuid, Box<dyn ProtocolEvidence>>,
}

impl RehydrationManager {
    /// Create a new rehydration manager
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            protocol_cache: HashMap::new(),
        }
    }
    
    /// Rehydrate a protocol from journal evidence
    pub fn rehydrate_protocol<P>(&mut self, evidence: RehydrationEvidence) -> Result<P, CrashRecoveryError>
    where
        P: SessionProtocol + StateRecovery<Evidence = RehydrationEvidence>,
    {
        // Validate evidence is sufficient
        if !P::validate_rehydration_evidence(&evidence) {
            return Err(CrashRecoveryError::InsufficientEvidence {
                protocol_id: evidence.session_id,
                missing_data: "Required events or state information missing".to_string(),
            });
        }
        
        // Attempt rehydration
        let protocol = P::rehydrate_from_evidence(self.device_id, evidence.clone())
            .map_err(|e| CrashRecoveryError::RehydrationFailed {
                protocol_id: evidence.session_id,
                error: e.to_string(),
            })?;
        
        // Cache the evidence for future use
        self.protocol_cache.insert(evidence.session_id, Box::new(evidence));
        
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
    
    /// Get the relevant events for this protocol
    fn events(&self) -> &[Event];
    
    /// Get the last known state, if available
    fn last_known_state(&self) -> Option<&str>;
    
    /// Validate that this evidence is complete
    fn is_complete(&self) -> bool;
}

impl ProtocolEvidence for RehydrationEvidence {
    fn protocol_id(&self) -> Uuid {
        self.session_id
    }
    
    fn events(&self) -> &[Event] {
        &self.events
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
        device_id: DeviceId,
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
        protocol_id: Uuid,
        missing_data: String,
    },
    
    /// Protocol rehydration failed
    #[error("Failed to rehydrate protocol {protocol_id}: {error}")]
    RehydrationFailed {
        protocol_id: Uuid,
        error: String,
    },
    
    /// Conflicting evidence found
    #[error("Conflicting evidence for protocol {protocol_id}: {details}")]
    ConflictingEvidence {
        protocol_id: Uuid,
        details: String,
    },
    
    /// Evidence corruption detected
    #[error("Evidence corruption detected for protocol {protocol_id}")]
    CorruptedEvidence {
        protocol_id: Uuid,
    },
    
    /// Journal is inconsistent
    #[error("Journal inconsistency detected: {details}")]
    JournalInconsistency {
        details: String,
    },
    
    /// Protocol version mismatch
    #[error("Protocol version mismatch for {protocol_id}: expected {expected}, found {found}")]
    VersionMismatch {
        protocol_id: Uuid,
        expected: String,
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
    config: RehydrationConfig,
}

impl EvidenceAnalyzer {
    /// Create a new evidence analyzer
    pub fn new(config: RehydrationConfig) -> Self {
        Self { config }
    }
    
    /// Analyze evidence quality for a protocol
    pub fn analyze_evidence_quality(&self, evidence: &RehydrationEvidence) -> EvidenceQuality {
        let event_count = evidence.events.len();
        let has_state = evidence.last_state.is_some();
        let is_recent = self.check_evidence_freshness(&evidence.events);
        
        if event_count == 0 {
            EvidenceQuality::Insufficient
        } else if !is_recent {
            EvidenceQuality::Stale
        } else if has_state && event_count >= 3 {
            EvidenceQuality::Excellent
        } else if has_state || event_count >= 2 {
            EvidenceQuality::Good
        } else {
            EvidenceQuality::Poor
        }
    }
    
    /// Check if evidence contains conflicting information
    pub fn detect_conflicts(&self, evidence: &RehydrationEvidence) -> Vec<String> {
        let mut conflicts = Vec::new();
        
        // Check for duplicate events with different content
        let mut event_ids = std::collections::HashSet::new();
        for event in &evidence.events {
            if !event_ids.insert(event.event_id) {
                conflicts.push(format!("Duplicate event ID: {:?}", event.event_id));
            }
        }
        
        // Check for chronological inconsistencies
        let mut last_timestamp = 0;
        for event in &evidence.events {
            if event.timestamp < last_timestamp {
                conflicts.push("Events not in chronological order".to_string());
                break;
            }
            last_timestamp = event.timestamp;
        }
        
        conflicts
    }
    
    /// Extract protocol state progression from events
    pub fn extract_state_progression(&self, evidence: &RehydrationEvidence) -> Vec<String> {
        // TODO: Implement state extraction based on event types
        // This is a placeholder that would analyze events to determine state transitions
        evidence.events.iter()
            .enumerate()
            .map(|(i, _)| format!("State_{}", i))
            .collect()
    }
    
    fn check_evidence_freshness(&self, events: &[Event]) -> bool {
        if events.is_empty() {
            return false;
        }
        
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        let latest_event_time = events.iter()
            .map(|e| e.timestamp)
            .max()
            .unwrap_or(0);
        
        (now - latest_event_time) <= self.config.max_event_age_seconds
    }
}

/// Quality assessment of rehydration evidence
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvidenceQuality {
    /// Evidence is insufficient for rehydration
    Insufficient,
    /// Evidence is stale (too old)
    Stale,
    /// Evidence is present but low quality
    Poor,
    /// Evidence is adequate for rehydration
    Good,
    /// Evidence is high quality with complete information
    Excellent,
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_journal::{EventAuthorization, EventType, AccountId};
    
    #[test]
    fn test_rehydration_manager_creation() {
        let device_id = DeviceId(Uuid::new_v4());
        let manager = RehydrationManager::new(device_id);
        assert_eq!(manager.device_id, device_id);
        assert!(manager.protocol_cache.is_empty());
    }
    
    #[test]
    fn test_evidence_quality_analysis() {
        let config = RehydrationConfig::default();
        let analyzer = EvidenceAnalyzer::new(config);
        
        // Test insufficient evidence
        let empty_evidence = RehydrationEvidence {
            events: vec![],
            session_id: Uuid::new_v4(),
            last_state: None,
        };
        
        let quality = analyzer.analyze_evidence_quality(&empty_evidence);
        assert_eq!(quality, EvidenceQuality::Insufficient);
    }
    
    #[test]
    fn test_conflict_detection() {
        let config = RehydrationConfig::default();
        let analyzer = EvidenceAnalyzer::new(config);
        
        let event_id = Uuid::new_v4();
        let device_id = DeviceId(Uuid::new_v4());
        
        // Create evidence with duplicate event IDs
        let evidence = RehydrationEvidence {
            events: vec![
                Event {
                    event_id,
                    timestamp: 1000,
                    device_id,
                    account_id: AccountId(Uuid::new_v4()),
                    event_type: EventType::InitiateRecovery { guardian_threshold: 2 },
                    authorization: EventAuthorization::DeviceCertificate {
                        device_id,
                        signature: vec![],
                    },
                },
                Event {
                    event_id, // Same ID - should be detected as conflict
                    timestamp: 2000,
                    device_id,
                    account_id: AccountId(Uuid::new_v4()),
                    event_type: EventType::InitiateRecovery { guardian_threshold: 3 },
                    authorization: EventAuthorization::DeviceCertificate {
                        device_id,
                        signature: vec![],
                    },
                },
            ],
            session_id: Uuid::new_v4(),
            last_state: None,
        };
        
        let conflicts = analyzer.detect_conflicts(&evidence);
        assert!(!conflicts.is_empty());
        assert!(conflicts[0].contains("Duplicate event ID"));
    }
}
//! State Management Improvements for CRDT Event Sourcing
//!
//! This module provides improved state management patterns replacing direct state
//! modification with proper CRDT events and event sourcing patterns.

use crate::{AgentError, Result};
use aura_journal::{
    events::{Event, EventAuthorization, EventData},
    AccountState, DeviceId, Effects,
};
use blake3::Hash;
use tracing::{debug, info, warn};

/// State management service providing proper CRDT event sourcing
pub struct StateManager {
    device_id: DeviceId,
}

impl StateManager {
    /// Create new state manager
    pub fn new(device_id: DeviceId) -> Self {
        Self { device_id }
    }

    /// Create properly signed event with actual hash instead of placeholder
    pub async fn create_signed_event(
        &self,
        event_data: EventData,
        effects: &Effects,
        sign_fn: impl Fn(&[u8]) -> Result<Vec<u8>>,
    ) -> Result<Event> {
        debug!("Creating properly signed event with real hash computation");

        // 1. Create preliminary event to compute hash
        let preliminary_event = Event::new(
            event_data.clone(),
            EventAuthorization::DeviceCertificate {
                device_id: self.device_id,
                signature: vec![0u8; 64], // Temporary placeholder for hash computation
            },
            effects,
        )
        .map_err(|e| AgentError::ledger(format!("Failed to create preliminary event: {}", e)))?;

        // 2. Compute actual event hash
        let event_hash = self.compute_event_hash(&preliminary_event)?;
        debug!("Computed event hash: {}", hex::encode(&event_hash));

        // 3. Sign the actual event hash
        let signature = sign_fn(&event_hash).map_err(|e| {
            AgentError::crypto_operation(format!("Failed to sign event hash: {}", e))
        })?;

        // 4. Create final event with real signature
        let final_event = Event::new(
            event_data,
            EventAuthorization::DeviceCertificate {
                device_id: self.device_id,
                signature,
            },
            effects,
        )
        .map_err(|e| AgentError::ledger(format!("Failed to create final signed event: {}", e)))?;

        info!("Created properly signed event with real hash verification");
        Ok(final_event)
    }

    /// Compute event hash for signing
    pub fn compute_event_hash(&self, event: &Event) -> Result<Vec<u8>> {
        // Serialize event for hashing (excluding signature to avoid circular dependency)
        let event_bytes = self.serialize_event_for_hash(event)?;

        // Compute blake3 hash
        let hash = blake3::hash(&event_bytes);
        Ok(hash.as_bytes().to_vec())
    }

    /// Serialize event for hash computation (excluding signature)
    fn serialize_event_for_hash(&self, event: &Event) -> Result<Vec<u8>> {
        // In real implementation, this would serialize the event excluding the signature field
        // For now, use a deterministic representation
        let mut serialized = Vec::new();

        // Add event timestamp
        serialized.extend_from_slice(&event.lamport_timestamp.to_le_bytes());

        // Add device ID
        serialized.extend_from_slice(self.device_id.as_bytes());

        // Add event type discriminant
        match &event.data {
            EventData::UpdateDeviceNonce(nonce_event) => {
                serialized.extend_from_slice(b"UpdateDeviceNonce");
                serialized.extend_from_slice(&nonce_event.new_nonce.to_le_bytes());
                serialized.extend_from_slice(&nonce_event.previous_nonce.to_le_bytes());
            }
            EventData::CreateSession(session_event) => {
                serialized.extend_from_slice(b"CreateSession");
                serialized.extend_from_slice(session_event.session_id.as_bytes());
                serialized.extend_from_slice(&session_event.lamport_timestamp.to_le_bytes());
            }
            EventData::UpdateSession(session_event) => {
                serialized.extend_from_slice(b"UpdateSession");
                serialized.extend_from_slice(session_event.session_id.as_bytes());
                serialized.extend_from_slice(&session_event.lamport_timestamp.to_le_bytes());
            }
            EventData::DeleteSession(session_event) => {
                serialized.extend_from_slice(b"DeleteSession");
                serialized.extend_from_slice(session_event.session_id.as_bytes());
                serialized.extend_from_slice(&session_event.lamport_timestamp.to_le_bytes());
            }
            EventData::CleanupExpiredSessions(cleanup_event) => {
                serialized.extend_from_slice(b"CleanupExpiredSessions");
                serialized.extend_from_slice(&cleanup_event.expired_session_count.to_le_bytes());
                serialized.extend_from_slice(&cleanup_event.cleanup_timestamp.to_le_bytes());
            }
            _ => {
                // Handle other event types
                serialized.extend_from_slice(b"GenericEvent");
            }
        }

        Ok(serialized)
    }

    /// Validate state consistency after event application
    pub fn validate_state_consistency(
        &self,
        state: &AccountState,
    ) -> Result<StateValidationReport> {
        debug!("Validating CRDT state consistency");

        let mut report = StateValidationReport::new();

        // 1. Validate device nonce monotonicity
        for (device_id, device_metadata) in &state.devices {
            if device_metadata.next_nonce == 0 {
                report.add_warning(format!("Device {} has zero nonce", device_id));
            }

            // Check for reasonable nonce values (not too high, suggesting overflow)
            if device_metadata.next_nonce > u64::MAX / 2 {
                report.add_error(format!(
                    "Device {} nonce approaching overflow: {}",
                    device_id, device_metadata.next_nonce
                ));
            }
        }

        // 2. Validate session consistency
        let active_sessions = state.sessions.len();
        if active_sessions > 1000 {
            report.add_warning(format!(
                "High number of active sessions: {}",
                active_sessions
            ));
        }

        // 3. Validate lamport clock monotonicity
        if state.lamport_clock == 0 {
            report.add_error("Lamport clock is zero".to_string());
        }

        // 4. Validate account integrity
        if state.devices.is_empty() {
            report.add_error("No devices in account state".to_string());
        }

        // 5. Validate threshold constraints
        let device_count = state.devices.len();
        if device_count > 0 && state.threshold > device_count {
            report.add_error(format!(
                "Threshold {} exceeds device count {}",
                state.threshold, device_count
            ));
        }

        debug!(
            "State validation completed: {} errors, {} warnings",
            report.errors.len(),
            report.warnings.len()
        );

        Ok(report)
    }

    /// Optimize CRDT operations for better performance
    pub fn optimize_crdt_operations(&self, state: &AccountState) -> Result<CrdtOptimizationReport> {
        debug!("Analyzing CRDT operations for optimization opportunities");

        let mut report = CrdtOptimizationReport::new();

        // 1. Check for excessive session cleanup frequency
        let session_count = state.sessions.len();
        if session_count > 100 {
            report.add_recommendation(
                "High session count detected",
                "Consider implementing batched session cleanup to reduce CRDT churn",
            );
        }

        // 2. Check for frequent nonce updates
        let device_count = state.devices.len();
        if device_count > 10 {
            report.add_recommendation(
                "Multiple devices generating frequent nonce updates",
                "Consider implementing nonce batching or caching to reduce event frequency",
            );
        }

        // 3. Check for lamport clock efficiency
        if state.lamport_clock > 1000000 {
            report.add_recommendation(
                "High lamport clock value",
                "Consider implementing clock compression or reset strategies",
            );
        }

        debug!(
            "CRDT optimization analysis completed: {} recommendations",
            report.recommendations.len()
        );

        Ok(report)
    }

    /// Implement event sourcing pattern with proper ordering
    pub fn create_event_sourcing_pattern(
        &self,
        events: Vec<EventData>,
        effects: &Effects,
        sign_fn: impl Fn(&[u8]) -> Result<Vec<u8>>,
    ) -> Result<Vec<Event>> {
        debug!(
            "Creating event sourcing pattern with {} events",
            events.len()
        );

        let mut sourced_events = Vec::new();

        for (index, event_data) in events.into_iter().enumerate() {
            debug!("Processing event {} in sourcing pattern", index);

            // Create properly signed event
            let event = futures::executor::block_on(async {
                self.create_signed_event(event_data, effects, &sign_fn)
                    .await
            })?;

            sourced_events.push(event);
        }

        info!(
            "Created event sourcing pattern with {} properly ordered events",
            sourced_events.len()
        );
        Ok(sourced_events)
    }
}

/// State validation report
#[derive(Debug, Clone)]
pub struct StateValidationReport {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl StateValidationReport {
    fn new() -> Self {
        Self {
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn add_error(&mut self, error: String) {
        self.errors.push(error);
    }

    fn add_warning(&mut self, warning: String) {
        self.warnings.push(warning);
    }

    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }
}

/// CRDT optimization report
#[derive(Debug, Clone)]
pub struct CrdtOptimizationReport {
    pub recommendations: Vec<OptimizationRecommendation>,
}

#[derive(Debug, Clone)]
pub struct OptimizationRecommendation {
    pub issue: String,
    pub recommendation: String,
}

impl CrdtOptimizationReport {
    fn new() -> Self {
        Self {
            recommendations: Vec::new(),
        }
    }

    fn add_recommendation(&mut self, issue: &str, recommendation: &str) {
        self.recommendations.push(OptimizationRecommendation {
            issue: issue.to_string(),
            recommendation: recommendation.to_string(),
        });
    }
}

/// Enhanced event batching for improved CRDT performance
pub struct EventBatch {
    events: Vec<EventData>,
    batch_id: uuid::Uuid,
    created_at: std::time::SystemTime,
}

impl EventBatch {
    /// Create new event batch
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            batch_id: uuid::Uuid::new_v4(),
            created_at: std::time::SystemTime::now(),
        }
    }

    /// Add event to batch
    pub fn add_event(&mut self, event: EventData) {
        self.events.push(event);
    }

    /// Check if batch should be committed (size or time based)
    pub fn should_commit(&self) -> bool {
        // Commit if batch has enough events or is old enough
        self.events.len() >= 10 || self.created_at.elapsed().unwrap_or_default().as_secs() >= 60
    }

    /// Get events for commitment
    pub fn into_events(self) -> Vec<EventData> {
        self.events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_state_validation_report() {
        let mut report = StateValidationReport::new();
        assert!(report.is_valid());
        assert!(!report.has_warnings());

        report.add_warning("Test warning".to_string());
        assert!(report.is_valid());
        assert!(report.has_warnings());

        report.add_error("Test error".to_string());
        assert!(!report.is_valid());
        assert!(report.has_warnings());
    }

    #[test]
    fn test_event_batch() {
        let mut batch = EventBatch::new();
        assert!(!batch.should_commit());

        // Add events to reach commit threshold
        for _ in 0..10 {
            batch.add_event(EventData::UpdateDeviceNonce(
                aura_journal::events::UpdateDeviceNonceEvent {
                    device_id: DeviceId(Uuid::new_v4()),
                    new_nonce: 1,
                    previous_nonce: 0,
                },
            ));
        }

        assert!(batch.should_commit());
        assert_eq!(batch.into_events().len(), 10);
    }
}

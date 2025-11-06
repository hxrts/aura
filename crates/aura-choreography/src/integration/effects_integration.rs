//! Effects system integration for choreographic protocols

// use aura_protocol::effects::ProtocolEffects; // TODO: Re-enable when ProtocolEffects is available
use aura_protocol::effects::Effects;
use rumpsteak_choreography::ChoreographyError;
use std::time::Duration;
use uuid::Uuid;

/// Extended effects adapter for choreographic protocols
pub struct ChoreographicEffectsAdapter {
    device_id: Uuid,
    effects: Effects,
    protocol_name: String,
}

impl ChoreographicEffectsAdapter {
    pub fn new(device_id: Uuid, effects: Effects, protocol_name: String) -> Self {
        Self { device_id, effects, protocol_name }
    }

    /// Record choreographic event for tracing
    pub async fn record_choreographic_event(
        &self,
        event_type: &str,
        participant: Uuid,
        phase: &str,
    ) -> Result<(), ChoreographyError> {
        tracing::info!(
            protocol = %self.protocol_name,
            event_type = %event_type,
            participant = %participant,
            phase = %phase,
            "Choreographic event recorded"
        );
        Ok(())
    }

    /// Record message send for visualization
    pub async fn record_message_send(
        &self,
        from: Uuid,
        to: Uuid,
        message_type: &str,
        size_bytes: usize,
    ) -> Result<(), ChoreographyError> {
        tracing::debug!(
            protocol = %self.protocol_name,
            from = %from,
            to = %to,
            message_type = %message_type,
            size_bytes = %size_bytes,
            "Message sent"
        );
        Ok(())
    }

    /// Record protocol phase transition
    pub async fn record_phase_transition(
        &self,
        from_phase: &str,
        to_phase: &str,
        participant: Uuid,
    ) -> Result<(), ChoreographyError> {
        tracing::info!(
            protocol = %self.protocol_name,
            from_phase = %from_phase,
            to_phase = %to_phase,
            participant = %participant,
            "Phase transition"
        );
        Ok(())
    }

    /// Record Byzantine behavior detection
    pub async fn record_byzantine_behavior(
        &self,
        participant: Uuid,
        behavior_type: &str,
        evidence: &str,
    ) -> Result<(), ChoreographyError> {
        tracing::warn!(
            protocol = %self.protocol_name,
            participant = %participant,
            behavior_type = %behavior_type,
            evidence = %evidence,
            "Byzantine behavior detected"
        );
        Ok(())
    }

    /// Get underlying effects for crypto operations
    pub fn effects(&self) -> &Effects {
        &self.effects
    }

    /// Get device ID
    pub fn device_id(&self) -> Uuid {
        self.device_id
    }

    /// Check if running in simulation mode
    pub fn is_simulation(&self) -> bool {
        // For now, assume we're always in simulation mode for choreographic protocols
        true
    }

    /// Yield until condition with timeout
    pub async fn yield_with_timeout<F>(
        &self,
        condition: F,
        timeout: Duration,
    ) -> Result<(), ChoreographyError>
    where
        F: Fn() -> bool + Send + 'static,
    {
        let deadline = tokio::time::Instant::now() + timeout;
        
        while !condition() {
            if tokio::time::Instant::now() >= deadline {
                return Err(ChoreographyError::Timeout(timeout));
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        
        Ok(())
    }
}

/// Helper trait for choreographic protocol effects
pub trait ChoreographicEffects {
    /// Record a protocol step for visualization
    fn record_step(&self, step_name: &str, data: &[u8]) -> Result<(), ChoreographyError>;
    
    /// Get deterministic delay for timing obfuscation
    fn get_delay(&self, base_ms: u64, jitter_ms: u64) -> Duration;
    
    /// Check protocol timeout
    fn check_timeout(&self, start_time: tokio::time::Instant, timeout: Duration) -> Result<(), ChoreographyError>;
}

impl ChoreographicEffects for ChoreographicEffectsAdapter {
    fn record_step(&self, step_name: &str, data: &[u8]) -> Result<(), ChoreographyError> {
        tracing::debug!(
            protocol = %self.protocol_name,
            step = %step_name,
            data_len = data.len(),
            "Protocol step recorded"
        );
        Ok(())
    }

    fn get_delay(&self, base_ms: u64, jitter_ms: u64) -> Duration {
        let jitter = if jitter_ms > 0 {
            let random_bytes = self.effects().random_bytes(4);
            let random_val = u32::from_le_bytes([random_bytes[0], random_bytes[1], random_bytes[2], random_bytes[3]]);
            (random_val as u64) % jitter_ms
        } else {
            0
        };
        
        Duration::from_millis(base_ms + jitter)
    }

    fn check_timeout(&self, start_time: tokio::time::Instant, timeout: Duration) -> Result<(), ChoreographyError> {
        if start_time.elapsed() > timeout {
            Err(ChoreographyError::Timeout(timeout))
        } else {
            Ok(())
        }
    }
}
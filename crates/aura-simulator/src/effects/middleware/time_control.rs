//! Time control middleware implementation
//!
//! Provides time manipulation capabilities for simulation including time acceleration,
//! pause/resume, and deterministic time control.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use aura_protocol::handlers::{
    AuraContext, AuraHandler, AuraHandlerError, EffectType, ExecutionMode,
};
use aura_core::identifiers::DeviceId;
use aura_core::sessions::LocalSessionType;

/// Time control middleware for simulation effect system
pub struct TimeControlMiddleware {
    device_id: DeviceId,
    time_seed: u64,
    execution_mode: ExecutionMode,
    current_time: Duration,
    time_acceleration: f64,
    is_paused: bool,
}

impl TimeControlMiddleware {
    /// Create new time control middleware
    pub fn new(device_id: DeviceId, time_seed: u64) -> Self {
        Self {
            device_id,
            time_seed,
            execution_mode: ExecutionMode::Simulation { seed: time_seed },
            current_time: Duration::ZERO,
            time_acceleration: 1.0,
            is_paused: false,
        }
    }

    /// Create for simulation mode
    pub fn for_simulation(device_id: DeviceId, seed: u64) -> Self {
        Self::new(device_id, seed)
    }

    /// Check if this middleware handles time control effects
    fn handles_effect(&self, effect_type: EffectType) -> bool {
        matches!(effect_type, EffectType::TimeControl)
    }

    /// Get the device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Set time acceleration factor
    pub fn set_acceleration(&mut self, factor: f64) {
        self.time_acceleration = factor;
    }

    /// Pause time
    pub fn pause(&mut self) {
        self.is_paused = true;
    }

    /// Resume time
    pub fn resume(&mut self) {
        self.is_paused = false;
    }

    /// Get current simulated time
    pub fn current_time(&self) -> Duration {
        self.current_time
    }

    /// Advance time by duration
    pub fn advance_time(&mut self, duration: Duration) {
        if !self.is_paused {
            self.current_time += duration;
        }
    }
}

#[async_trait]
impl AuraHandler for TimeControlMiddleware {
    async fn execute_effect(
        &mut self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        _ctx: &mut AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError> {
        if !self.handles_effect(effect_type) {
            return Err(AuraHandlerError::UnsupportedEffect { effect_type });
        }

        match operation {
            "get_current_time" => {
                let time_millis = self.current_time.as_millis() as u64;
                Ok(serde_json::to_vec(&time_millis).unwrap_or_default())
            }
            "advance_time" => {
                let milliseconds = if parameters.is_empty() {
                    1000 // Default 1 second
                } else {
                    String::from_utf8_lossy(parameters)
                        .parse::<u64>()
                        .unwrap_or(1000)
                };
                self.advance_time(Duration::from_millis(milliseconds));
                Ok(serde_json::to_vec(&self.current_time.as_millis()).unwrap_or_default())
            }
            "set_acceleration" => {
                let factor = if parameters.is_empty() {
                    1.0
                } else {
                    String::from_utf8_lossy(parameters)
                        .parse::<f64>()
                        .unwrap_or(1.0)
                };
                self.set_acceleration(factor);
                Ok(serde_json::to_vec(&self.time_acceleration).unwrap_or_default())
            }
            "pause" => {
                self.pause();
                Ok(serde_json::to_vec(&self.is_paused).unwrap_or_default())
            }
            "resume" => {
                self.resume();
                Ok(serde_json::to_vec(&self.is_paused).unwrap_or_default())
            }
            "is_paused" => Ok(serde_json::to_vec(&self.is_paused).unwrap_or_default()),
            "get_acceleration" => {
                Ok(serde_json::to_vec(&self.time_acceleration).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnsupportedOperation {
                effect_type,
                operation: operation.to_string(),
            }),
        }
    }

    async fn execute_session(
        &mut self,
        _session: LocalSessionType,
        _ctx: &mut AuraContext,
    ) -> Result<(), AuraHandlerError> {
        // Time control doesn't handle sessions directly
        Ok(())
    }

    fn supports_effect(&self, effect_type: EffectType) -> bool {
        self.handles_effect(effect_type)
    }

    fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_time_control_creation() {
        let device_id = DeviceId::new();
        let middleware = TimeControlMiddleware::for_simulation(device_id, 42);

        assert_eq!(middleware.device_id(), device_id);
        assert_eq!(middleware.time_seed, 42);
        assert_eq!(
            middleware.execution_mode(),
            ExecutionMode::Simulation { seed: 42 }
        );
        assert_eq!(middleware.current_time(), Duration::ZERO);
        assert_eq!(middleware.time_acceleration, 1.0);
        assert!(!middleware.is_paused);
    }

    #[tokio::test]
    async fn test_time_effect_support() {
        let device_id = DeviceId::new();
        let middleware = TimeControlMiddleware::for_simulation(device_id, 42);

        assert!(middleware.supports_effect(EffectType::TimeControl));
        assert!(!middleware.supports_effect(EffectType::Crypto));
        assert!(!middleware.supports_effect(EffectType::FaultInjection));
    }

    #[tokio::test]
    async fn test_time_operations() {
        let device_id = DeviceId::new();
        let mut middleware = TimeControlMiddleware::for_simulation(device_id, 42);
        let mut ctx = AuraContext::new(device_id);

        // Test get current time
        let result = middleware
            .execute_effect(EffectType::TimeControl, "get_current_time", b"", &mut ctx)
            .await;
        assert!(result.is_ok());

        // Test advance time
        let result = middleware
            .execute_effect(
                EffectType::TimeControl,
                "advance_time",
                b"5000", // 5 seconds
                &mut ctx,
            )
            .await;
        assert!(result.is_ok());
        assert_eq!(middleware.current_time(), Duration::from_millis(5000));

        // Test pause
        let result = middleware
            .execute_effect(EffectType::TimeControl, "pause", b"", &mut ctx)
            .await;
        assert!(result.is_ok());
        assert!(middleware.is_paused);

        // Test resume
        let result = middleware
            .execute_effect(EffectType::TimeControl, "resume", b"", &mut ctx)
            .await;
        assert!(result.is_ok());
        assert!(!middleware.is_paused);
    }

    #[test]
    fn test_time_manipulation() {
        let device_id = DeviceId::new();
        let mut middleware = TimeControlMiddleware::for_simulation(device_id, 42);

        // Test time advancement
        middleware.advance_time(Duration::from_secs(10));
        assert_eq!(middleware.current_time(), Duration::from_secs(10));

        // Test pause
        middleware.pause();
        middleware.advance_time(Duration::from_secs(5));
        assert_eq!(middleware.current_time(), Duration::from_secs(10)); // No change when paused

        // Test resume
        middleware.resume();
        middleware.advance_time(Duration::from_secs(5));
        assert_eq!(middleware.current_time(), Duration::from_secs(15));

        // Test acceleration
        middleware.set_acceleration(2.0);
        assert_eq!(middleware.time_acceleration, 2.0);
    }
}

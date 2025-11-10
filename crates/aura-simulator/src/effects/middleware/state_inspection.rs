//! State inspection middleware implementation
//!
//! Provides state inspection capabilities for simulation debugging
//! including state capture, diff analysis, and snapshot management.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::time::SystemTime;

use aura_protocol::handlers::{
    AuraContext, AuraHandler, AuraHandlerError, EffectType, ExecutionMode,
};
use aura_core::identifiers::DeviceId;
use aura_core::sessions::LocalSessionType;

/// State capture parameters
#[derive(Debug, Serialize, Deserialize)]
pub struct StateCaptureParams {
    /// Optional custom identifier for the snapshot
    pub snapshot_id: Option<String>,
    /// Additional metadata to attach to the snapshot
    pub metadata: HashMap<String, serde_json::Value>,
}

/// State snapshot information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    /// Unique identifier for this snapshot
    pub id: String,
    /// When the snapshot was created
    pub timestamp: SystemTime,
    /// ID of the device that created this snapshot
    pub device_id: DeviceId,
    /// Captured state data from the context
    pub state_data: HashMap<String, serde_json::Value>,
    /// Additional metadata attached to this snapshot
    pub metadata: HashMap<String, serde_json::Value>,
}

/// State diff result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateDiffResult {
    /// ID of the first snapshot being compared
    pub snapshot_a: String,
    /// ID of the second snapshot being compared
    pub snapshot_b: String,
    /// List of differences found between the snapshots
    pub differences: Vec<String>,
}

/// State inspection middleware for simulation debugging
pub struct StateInspectionMiddleware {
    device_id: DeviceId,
    execution_mode: ExecutionMode,
    snapshots: VecDeque<StateSnapshot>,
    snapshot_counter: u64,
    max_snapshots: usize,
}

impl StateInspectionMiddleware {
    /// Create new state inspection middleware
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            execution_mode: ExecutionMode::Simulation { seed: 0 },
            snapshots: VecDeque::new(),
            snapshot_counter: 0,
            max_snapshots: 100,
        }
    }

    /// Create for simulation mode
    pub fn for_simulation(device_id: DeviceId, seed: u64) -> Self {
        Self {
            device_id,
            execution_mode: ExecutionMode::Simulation { seed },
            snapshots: VecDeque::new(),
            snapshot_counter: 0,
            max_snapshots: 100,
        }
    }

    /// Check if this middleware handles state inspection effects
    fn handles_effect(&self, effect_type: EffectType) -> bool {
        matches!(effect_type, EffectType::StateInspection)
    }

    /// Generate snapshot ID
    fn generate_snapshot_id(&mut self) -> String {
        self.snapshot_counter += 1;
        format!("snapshot_{}_{}", self.device_id, self.snapshot_counter)
    }

    /// Extract state from context
    fn extract_state_from_context(&self, ctx: &AuraContext) -> HashMap<String, serde_json::Value> {
        let mut state = HashMap::new();

        // Add basic context info
        state.insert("device_id".to_string(), serde_json::json!(ctx.device_id));
        state.insert(
            "execution_mode".to_string(),
            serde_json::json!(ctx.execution_mode),
        );

        // Add session info if available
        if let Some(session_id) = &ctx.session_id {
            state.insert("session_id".to_string(), serde_json::json!(session_id));
        }

        // Add timestamp
        state.insert(
            "timestamp".to_string(),
            serde_json::json!(SystemTime::now()),
        );

        state
    }

    /// Capture state snapshot
    fn capture_snapshot(&mut self, ctx: &AuraContext, custom_id: Option<String>) -> StateSnapshot {
        let snapshot_id = custom_id.unwrap_or_else(|| self.generate_snapshot_id());
        let state_data = self.extract_state_from_context(ctx);

        let snapshot = StateSnapshot {
            id: snapshot_id,
            timestamp: SystemTime::now(),
            device_id: self.device_id,
            state_data,
            metadata: HashMap::new(),
        };

        // Add snapshot to history
        self.snapshots.push_back(snapshot.clone());

        // Trim snapshot history if needed
        while self.snapshots.len() > self.max_snapshots {
            self.snapshots.pop_front();
        }

        snapshot
    }

    /// Get state diff between snapshots
    fn get_state_diff(
        &self,
        snapshot_a: &str,
        snapshot_b: &str,
    ) -> Result<StateDiffResult, AuraHandlerError> {
        let snap_a = self
            .snapshots
            .iter()
            .find(|s| s.id == snapshot_a)
            .ok_or_else(|| AuraHandlerError::ContextError {
                message: "Snapshot not found".to_string(),
            })?;

        let snap_b = self
            .snapshots
            .iter()
            .find(|s| s.id == snapshot_b)
            .ok_or_else(|| AuraHandlerError::ContextError {
                message: "Snapshot not found".to_string(),
            })?;

        let mut differences = Vec::new();

        // Simple diff - just compare keys that differ
        for key in snap_a.state_data.keys() {
            if snap_a.state_data.get(key) != snap_b.state_data.get(key) {
                differences.push(format!("Changed: {}", key));
            }
        }

        for key in snap_b.state_data.keys() {
            if !snap_a.state_data.contains_key(key) {
                differences.push(format!("Added: {}", key));
            }
        }

        Ok(StateDiffResult {
            snapshot_a: snapshot_a.to_string(),
            snapshot_b: snapshot_b.to_string(),
            differences,
        })
    }

    /// Get device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Get snapshots
    pub fn snapshots(&self) -> &VecDeque<StateSnapshot> {
        &self.snapshots
    }
}

#[async_trait]
impl AuraHandler for StateInspectionMiddleware {
    async fn execute_effect(
        &mut self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        ctx: &mut AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError> {
        if !self.handles_effect(effect_type) {
            return Err(AuraHandlerError::UnsupportedEffect { effect_type });
        }

        match operation {
            "capture_state" => {
                let params: StateCaptureParams = if parameters.is_empty() {
                    StateCaptureParams {
                        snapshot_id: None,
                        metadata: HashMap::new(),
                    }
                } else {
                    bincode::deserialize(parameters).map_err(|_| {
                        AuraHandlerError::ContextError {
                            message: "Failed to deserialize capture parameters".to_string(),
                        }
                    })?
                };

                let snapshot = self.capture_snapshot(ctx, params.snapshot_id);
                serde_json::to_vec(&snapshot).map_err(|_| AuraHandlerError::ContextError {
                    message: "Failed to serialize snapshot".to_string(),
                })
            }
            "get_state_diff" => {
                if parameters.len() < 2 {
                    return Err(AuraHandlerError::ContextError {
                        message: "Need two snapshot IDs for diff".to_string(),
                    });
                }

                let snapshot_ids = String::from_utf8_lossy(parameters);
                let parts: Vec<&str> = snapshot_ids.split(',').collect();
                if parts.len() != 2 {
                    return Err(AuraHandlerError::ContextError {
                        message: "Expected two comma-separated snapshot IDs".to_string(),
                    });
                }

                let diff = self.get_state_diff(parts[0], parts[1])?;
                serde_json::to_vec(&diff).map_err(|_| AuraHandlerError::ContextError {
                    message: "Failed to serialize diff".to_string(),
                })
            }
            "list_snapshots" => {
                let snapshot_ids: Vec<String> =
                    self.snapshots.iter().map(|s| s.id.clone()).collect();
                serde_json::to_vec(&snapshot_ids).map_err(|_| AuraHandlerError::ContextError {
                    message: "Failed to serialize snapshot list".to_string(),
                })
            }
            "clear_snapshots" => {
                let count = self.snapshots.len();
                self.snapshots.clear();
                self.snapshot_counter = 0;

                serde_json::to_vec(&count).map_err(|_| AuraHandlerError::ContextError {
                    message: "Failed to serialize clear count".to_string(),
                })
            }
            _ => Err(AuraHandlerError::UnknownOperation {
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
        // State inspection doesn't handle sessions directly
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
    async fn test_state_inspection_creation() {
        let device_id = DeviceId::new();
        let middleware = StateInspectionMiddleware::for_simulation(device_id, 42);

        assert_eq!(middleware.device_id(), device_id);
        assert_eq!(
            middleware.execution_mode(),
            ExecutionMode::Simulation { seed: 42 }
        );
    }

    #[tokio::test]
    async fn test_state_effect_support() {
        let device_id = DeviceId::new();
        let middleware = StateInspectionMiddleware::for_simulation(device_id, 42);

        assert!(middleware.supports_effect(EffectType::StateInspection));
        assert!(!middleware.supports_effect(EffectType::Crypto));
        assert!(!middleware.supports_effect(EffectType::Network));
    }

    #[tokio::test]
    async fn test_state_operations() {
        let device_id = DeviceId::new();
        let mut middleware = StateInspectionMiddleware::for_simulation(device_id, 42);
        let mut ctx = AuraContext::for_testing(device_id);

        // Test capture state
        let result = middleware
            .execute_effect(EffectType::StateInspection, "capture_state", b"", &mut ctx)
            .await;
        assert!(result.is_ok());

        // Test list snapshots
        let result = middleware
            .execute_effect(EffectType::StateInspection, "list_snapshots", b"", &mut ctx)
            .await;
        assert!(result.is_ok());

        // Test clear snapshots
        let result = middleware
            .execute_effect(
                EffectType::StateInspection,
                "clear_snapshots",
                b"",
                &mut ctx,
            )
            .await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_snapshot_management() {
        let device_id = DeviceId::new();
        let mut middleware = StateInspectionMiddleware::for_simulation(device_id, 42);
        let ctx = AuraContext::for_testing(device_id);

        // Capture snapshot
        let snapshot = middleware.capture_snapshot(&ctx, Some("test_snapshot".to_string()));
        assert_eq!(snapshot.id, "test_snapshot");
        assert_eq!(middleware.snapshots().len(), 1);

        // Capture another
        middleware.capture_snapshot(&ctx, None);
        assert_eq!(middleware.snapshots().len(), 2);

        // Test max snapshots limit
        middleware.max_snapshots = 2;
        middleware.capture_snapshot(&ctx, None);
        assert_eq!(middleware.snapshots().len(), 2); // Should trim oldest
    }
}

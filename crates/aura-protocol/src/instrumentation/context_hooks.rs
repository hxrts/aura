//! ProtocolContext Instrumentation Hooks
//!
//! Provides hooks for instrumenting ProtocolContext operations
//! with dev console trace recording.

use super::recorder::TraceRecorder;
use std::sync::{Arc, Mutex};
use uuid::Uuid;
use DeviceId;

/// Instrumentation hooks for ProtocolContext
///
/// These hooks allow non-intrusive observation of protocol execution
/// for debugging and visualization in the dev console.
pub struct InstrumentationHooks {
    /// Trace recorder for events
    recorder: Arc<Mutex<TraceRecorder>>,
}

impl InstrumentationHooks {
    /// Create new instrumentation hooks
    pub fn new() -> Self {
        Self {
            recorder: Arc::new(Mutex::new(TraceRecorder::new())),
        }
    }

    /// Create hooks with shared recorder
    pub fn with_recorder(recorder: Arc<Mutex<TraceRecorder>>) -> Self {
        Self { recorder }
    }

    /// Enable instrumentation recording
    pub fn enable(&self) {
        if let Ok(mut recorder) = self.recorder.lock() {
            recorder.enable();
        }
    }

    /// Disable instrumentation recording
    pub fn disable(&self) {
        if let Ok(mut recorder) = self.recorder.lock() {
            recorder.disable();
        }
    }

    /// Record protocol start event
    pub fn on_protocol_start(&self, session_id: Uuid, device_id: DeviceId, protocol_name: &str) {
        if let Ok(mut recorder) = self.recorder.lock() {
            recorder.record_protocol_event(
                session_id,
                device_id,
                &format!("{}_start", protocol_name),
                Some(serde_json::json!({
                    "protocol": protocol_name,
                    "phase": "initialization"
                })),
            );
        }
    }

    /// Record protocol phase transition
    pub fn on_phase_transition(
        &self,
        session_id: Uuid,
        device_id: DeviceId,
        protocol_name: &str,
        from_phase: &str,
        to_phase: &str,
    ) {
        if let Ok(mut recorder) = self.recorder.lock() {
            recorder.record_protocol_event(
                session_id,
                device_id,
                &format!("{}_phase_transition", protocol_name),
                Some(serde_json::json!({
                    "protocol": protocol_name,
                    "from_phase": from_phase,
                    "to_phase": to_phase
                })),
            );
        }
    }

    /// Record event emission
    pub fn on_event_emit(
        &self,
        session_id: Uuid,
        device_id: DeviceId,
        event_type: &str,
        event_size: usize,
    ) {
        if let Ok(mut recorder) = self.recorder.lock() {
            recorder.record_protocol_event(
                session_id,
                device_id,
                "event_emit",
                Some(serde_json::json!({
                    "event_type": event_type,
                    "size_bytes": event_size
                })),
            );
        }
    }

    /// Record event awaiting
    pub fn on_event_await_start(
        &self,
        session_id: Uuid,
        device_id: DeviceId,
        event_pattern: &str,
        threshold: Option<usize>,
    ) {
        if let Ok(mut recorder) = self.recorder.lock() {
            recorder.record_protocol_event(
                session_id,
                device_id,
                "event_await_start",
                Some(serde_json::json!({
                    "pattern": event_pattern,
                    "threshold": threshold
                })),
            );
        }
    }

    /// Record event awaiting completion
    pub fn on_event_await_complete(
        &self,
        session_id: Uuid,
        device_id: DeviceId,
        event_pattern: &str,
        received_count: usize,
        success: bool,
    ) {
        if let Ok(mut recorder) = self.recorder.lock() {
            recorder.record_protocol_event(
                session_id,
                device_id,
                "event_await_complete",
                Some(serde_json::json!({
                    "pattern": event_pattern,
                    "received_count": received_count,
                    "success": success
                })),
            );
        }
    }

    /// Record protocol completion
    pub fn on_protocol_complete(
        &self,
        session_id: Uuid,
        device_id: DeviceId,
        protocol_name: &str,
        success: bool,
        result_summary: Option<serde_json::Value>,
    ) {
        if let Ok(mut recorder) = self.recorder.lock() {
            recorder.record_protocol_event(
                session_id,
                device_id,
                &format!("{}_complete", protocol_name),
                Some(serde_json::json!({
                    "protocol": protocol_name,
                    "success": success,
                    "result": result_summary
                })),
            );
        }
    }

    /// Record protocol error
    pub fn on_protocol_error(
        &self,
        session_id: Uuid,
        device_id: DeviceId,
        protocol_name: &str,
        error_type: &str,
        error_message: &str,
    ) {
        if let Ok(mut recorder) = self.recorder.lock() {
            recorder.record_error(
                device_id,
                &format!("{}_{}", protocol_name, error_type),
                error_message,
                Some(serde_json::json!({
                    "session_id": session_id,
                    "protocol": protocol_name,
                    "error_type": error_type
                })),
            );
        }
    }

    /// Get access to the underlying recorder for advanced operations
    pub fn recorder(&self) -> Arc<Mutex<TraceRecorder>> {
        Arc::clone(&self.recorder)
    }

    /// Export current trace data
    pub fn export_trace(&self) -> Vec<u8> {
        if let Ok(recorder) = self.recorder.lock() {
            recorder.export_trace()
        } else {
            Vec::new()
        }
    }
}

impl Default for InstrumentationHooks {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for InstrumentationHooks {
    fn clone(&self) -> Self {
        Self {
            recorder: Arc::clone(&self.recorder),
        }
    }
}

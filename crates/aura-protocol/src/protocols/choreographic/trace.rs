//! Choreography tracing integration
//!
//! This module integrates choreographic protocol events with Aura's trace system,
//! enabling visualization and debugging of distributed protocol execution.

#[allow(unused_imports)]
use super::{BridgedRole, ChoreoEvent};
use aura_types::DeviceId;
use serde::{Deserialize, Serialize};

// Define types that were missing
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProtocolPhase {
    Initialized,
    Started,
    Completed,
    Failed,
}

#[derive(Debug, Clone)]
pub struct ProtocolTrace {
    pub protocol_id: Uuid,
    pub device_id: DeviceId,
    pub start_time: std::time::Instant,
    pub events: Vec<TraceEvent>,
    pub phase_transitions: Vec<(ProtocolPhase, std::time::Instant)>,
    pub current_phase: ProtocolPhase,
    pub metadata: HashMap<String, String>,
}

impl ProtocolTrace {
    pub fn transition_phase(&mut self, phase: ProtocolPhase) {
        self.phase_transitions
            .push((self.current_phase.clone(), std::time::Instant::now()));
        self.current_phase = phase;
    }

    pub fn add_event(&mut self, event: TraceEvent) {
        self.events.push(event);
    }
}

#[derive(Debug, Clone)]
pub enum TraceEvent {
    PhaseTransition {
        from: ProtocolPhase,
        to: ProtocolPhase,
        timestamp: std::time::Instant,
    },
    MessageSent {
        to: DeviceId,
        message_type: String,
        size: usize,
    },
    MessageReceived {
        from: DeviceId,
        message_type: String,
        size: usize,
    },
    Error {
        error_type: String,
        message: String,
    },
    Custom {
        event_type: String,
        data: serde_json::Value,
    },
}
use std::collections::HashMap;
use std::time::Instant;
use uuid::Uuid;

/// Choreography trace context
#[derive(Debug, Clone)]
pub struct ChoreoTraceContext {
    /// Protocol ID for this choreography
    protocol_id: Uuid,

    /// Protocol type (e.g., "DKD", "FROST")
    protocol_type: String,

    /// Start time of the protocol
    start_time: Instant,

    /// Participants in the protocol
    participants: Vec<DeviceId>,

    /// Active traces
    traces: HashMap<DeviceId, ProtocolTrace>,
}

impl ChoreoTraceContext {
    /// Create a new choreography trace context
    pub fn new(protocol_type: String, participants: Vec<DeviceId>) -> Self {
        let protocol_id = Uuid::new_v4();
        let start_time = Instant::now();

        // Initialize traces for each participant
        let mut traces = HashMap::new();
        for &device_id in &participants {
            traces.insert(
                device_id,
                ProtocolTrace {
                    protocol_id,
                    device_id,
                    start_time,
                    events: Vec::new(),
                    phase_transitions: Vec::new(),
                    current_phase: ProtocolPhase::Initialized,
                    metadata: HashMap::new(),
                },
            );
        }

        Self {
            protocol_id,
            protocol_type,
            start_time,
            participants,
            traces,
        }
    }

    /// Convert choreography events to trace events
    pub fn record_choreo_event(&mut self, event: &ChoreoEvent) {
        match event {
            ChoreoEvent::ProtocolStarted {
                participants: _, ..
            } => {
                // Update all participant traces to Started phase
                for trace in self.traces.values_mut() {
                    trace.transition_phase(ProtocolPhase::Started);
                    trace.add_event(TraceEvent::PhaseTransition {
                        from: ProtocolPhase::Initialized,
                        to: ProtocolPhase::Started,
                        timestamp: std::time::Instant::now(),
                    });
                }
            }

            ChoreoEvent::MessageSent {
                from,
                to,
                message_type,
                payload_size,
                timestamp: _,
            } => {
                // Record send event in sender's trace
                if let Some(trace) = self.traces.get_mut(&DeviceId::from(from.device_id)) {
                    trace.add_event(TraceEvent::MessageSent {
                        to: DeviceId::from(to.device_id),
                        message_type: message_type.clone(),
                        size: *payload_size,
                    });
                }
            }

            ChoreoEvent::MessageReceived {
                from,
                to,
                message_type,
                timestamp: _,
            } => {
                // Record receive event in receiver's trace
                if let Some(trace) = self.traces.get_mut(&DeviceId::from(to.device_id)) {
                    trace.add_event(TraceEvent::MessageReceived {
                        from: DeviceId::from(from.device_id),
                        message_type: message_type.clone(),
                        size: 0, // Size not available in receive event
                    });
                }
            }

            ChoreoEvent::ChoiceMade {
                from,
                to,
                label,
                timestamp: _,
            } => {
                // Record choice as a special message
                if let Some(trace) = self.traces.get_mut(&DeviceId::from(from.device_id)) {
                    trace.add_event(TraceEvent::MessageSent {
                        to: DeviceId::from(to.device_id),
                        message_type: format!("Choice: {}", label),
                        size: label.len(),
                    });
                }
            }

            ChoreoEvent::ProtocolCompleted {
                success,
                duration_ms,
                ..
            } => {
                // Update all participant traces to Completed phase
                let final_phase = if *success {
                    ProtocolPhase::Completed
                } else {
                    ProtocolPhase::Failed
                };

                for trace in self.traces.values_mut() {
                    trace.transition_phase(final_phase.clone());
                    trace.add_event(TraceEvent::PhaseTransition {
                        from: trace.current_phase.clone(),
                        to: final_phase.clone(),
                        timestamp: std::time::Instant::now(),
                    });

                    // Add protocol completion metadata
                    trace
                        .metadata
                        .insert("duration_ms".to_string(), duration_ms.to_string());
                    trace
                        .metadata
                        .insert("success".to_string(), success.to_string());
                }
            }

            ChoreoEvent::ProtocolFailed { error, .. } => {
                // Update all participant traces to Failed phase
                for trace in self.traces.values_mut() {
                    trace.transition_phase(ProtocolPhase::Failed);
                    trace.add_event(TraceEvent::Error {
                        error_type: "ProtocolFailed".to_string(),
                        message: error.clone(),
                    });
                }
            }

            ChoreoEvent::CoordinatorElected {
                coordinator,
                participants,
                timestamp,
            } => {
                // Record coordinator election in all participant traces
                for participant in participants {
                    if let Some(trace) = self.traces.get_mut(&DeviceId::from(participant.device_id))
                    {
                        let event_type = if participant.device_id == coordinator.device_id {
                            "Elected as coordinator"
                        } else {
                            "Coordinator elected"
                        };

                        trace.add_event(TraceEvent::Custom {
                            event_type: event_type.to_string(),
                            data: {
                                let mut data = HashMap::new();
                                data.insert(
                                    "coordinator_id".to_string(),
                                    coordinator.device_id.to_string(),
                                );
                                data.insert("timestamp".to_string(), timestamp.to_string());
                                serde_json::to_value(data).unwrap_or(serde_json::Value::Null)
                            },
                        });
                    }
                }
            }

            ChoreoEvent::EpochBumped {
                old_epoch,
                new_epoch,
                reason,
                timestamp,
            } => {
                // Record epoch bump in all participant traces
                for trace in self.traces.values_mut() {
                    trace.add_event(TraceEvent::Custom {
                        event_type: "EpochBumped".to_string(),
                        data: {
                            let mut data = HashMap::new();
                            data.insert("old_epoch".to_string(), old_epoch.to_string());
                            data.insert("new_epoch".to_string(), new_epoch.to_string());
                            data.insert("reason".to_string(), reason.clone());
                            data.insert("timestamp".to_string(), timestamp.to_string());
                            serde_json::to_value(data).unwrap_or(serde_json::Value::Null)
                        },
                    });
                }
            }
        }
    }

    /// Get traces for all participants
    pub fn get_traces(&self) -> Vec<ProtocolTrace> {
        self.traces.values().cloned().collect()
    }

    /// Get trace for a specific participant
    pub fn get_participant_trace(&self, device_id: DeviceId) -> Option<&ProtocolTrace> {
        self.traces.get(&device_id)
    }

    /// Export traces for visualization
    pub fn export_for_visualization(&self) -> ChoreoTraceExport {
        ChoreoTraceExport {
            protocol_id: self.protocol_id,
            protocol_type: self.protocol_type.clone(),
            start_time: self.start_time,
            participants: self.participants.clone(),
            traces: self.get_traces(),
            metadata: {
                let mut meta = HashMap::new();
                meta.insert(
                    "participant_count".to_string(),
                    self.participants.len().to_string(),
                );
                meta.insert("protocol_type".to_string(), self.protocol_type.clone());
                meta
            },
        }
    }
}

/// Exported choreography trace data for visualization
#[derive(Debug, Clone)]
pub struct ChoreoTraceExport {
    pub protocol_id: Uuid,
    pub protocol_type: String,
    pub start_time: Instant,
    pub participants: Vec<DeviceId>,
    pub traces: Vec<ProtocolTrace>,
    pub metadata: HashMap<String, String>,
}

/// Integration with existing trace recording system
pub struct ChoreoTraceRecorder {
    /// Active trace contexts
    contexts: HashMap<Uuid, ChoreoTraceContext>,
}

impl Default for ChoreoTraceRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl ChoreoTraceRecorder {
    pub fn new() -> Self {
        Self {
            contexts: HashMap::new(),
        }
    }

    /// Start recording a new choreography
    pub fn start_choreography(
        &mut self,
        protocol_id: Uuid,
        protocol_type: String,
        participants: Vec<DeviceId>,
    ) -> Uuid {
        let context = ChoreoTraceContext::new(protocol_type, participants);
        self.contexts.insert(protocol_id, context);
        protocol_id
    }

    /// Record a choreography event
    pub fn record_event(&mut self, protocol_id: Uuid, event: ChoreoEvent) {
        if let Some(context) = self.contexts.get_mut(&protocol_id) {
            context.record_choreo_event(&event);
        }
    }

    /// Get traces for a protocol
    pub fn get_traces(&self, protocol_id: Uuid) -> Option<Vec<ProtocolTrace>> {
        self.contexts.get(&protocol_id).map(|ctx| ctx.get_traces())
    }

    /// Export all traces for visualization
    pub fn export_all(&self) -> Vec<ChoreoTraceExport> {
        self.contexts
            .values()
            .map(|ctx| ctx.export_for_visualization())
            .collect()
    }

    /// Clear completed protocols
    pub fn clear_completed(&mut self) {
        self.contexts.retain(|_, ctx| {
            // Keep protocols that haven't completed yet
            ctx.traces.values().any(|trace| {
                !matches!(
                    trace.current_phase,
                    ProtocolPhase::Completed | ProtocolPhase::Failed
                )
            })
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_choreo_trace_context() {
        let participants = vec![DeviceId::new(), DeviceId::new(), DeviceId::new()];
        let mut context = ChoreoTraceContext::new("DKD".to_string(), participants.clone());

        // Record protocol start
        let event = ChoreoEvent::ProtocolStarted {
            protocol_type: "DKD".to_string(),
            participants: participants
                .iter()
                .map(|&id| BridgedRole {
                    device_id: id.into(),
                    role_index: 0,
                })
                .collect(),
            timestamp: 0,
        };

        context.record_choreo_event(&event);

        // Check that all participants have transitioned to Started phase
        for trace in context.get_traces() {
            assert_eq!(trace.current_phase, ProtocolPhase::Started);
            assert_eq!(trace.events.len(), 1);
        }
    }

    #[test]
    fn test_message_event_recording() {
        let device1 = DeviceId::new();
        let device2 = DeviceId::new();
        let participants = vec![device1, device2];

        let mut context = ChoreoTraceContext::new("FROST".to_string(), participants);

        // Record message send
        let send_event = ChoreoEvent::MessageSent {
            from: BridgedRole {
                device_id: device1.into(),
                role_index: 0,
            },
            to: BridgedRole {
                device_id: device2.into(),
                role_index: 1,
            },
            message_type: "CommitmentShare".to_string(),
            payload_size: 128,
            timestamp: 100,
        };

        context.record_choreo_event(&send_event);

        // Check sender's trace
        let sender_trace = context.get_participant_trace(device1).unwrap();
        assert_eq!(sender_trace.events.len(), 1);
        match &sender_trace.events[0] {
            TraceEvent::MessageSent { to, .. } => {
                assert_eq!(*to, device2);
            }
            _ => panic!("Expected MessageSent event"),
        }
    }

    #[test]
    fn test_choreo_trace_recorder() {
        let mut recorder = ChoreoTraceRecorder::new();
        let participants = vec![DeviceId::new(), DeviceId::new()];

        // Start a choreography
        let protocol_id =
            recorder.start_choreography(Uuid::new_v4(), "DKD".to_string(), participants.clone());

        // Record some events
        recorder.record_event(
            protocol_id,
            ChoreoEvent::ProtocolStarted {
                protocol_type: "DKD".to_string(),
                participants: participants
                    .iter()
                    .map(|&id| BridgedRole {
                        device_id: id.into(),
                        role_index: 0,
                    })
                    .collect(),
                timestamp: 0,
            },
        );

        // Get traces
        let traces = recorder.get_traces(protocol_id).unwrap();
        assert_eq!(traces.len(), 2); // One trace per participant
    }
}

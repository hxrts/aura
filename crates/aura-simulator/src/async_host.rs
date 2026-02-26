//! Async simulator host boundary with deterministic request/resume semantics.

use crate::handlers::{SimulationFaultHandler, SimulationScenarioHandler};
use aura_core::effects::{ByzantineType, ChaosEffects};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use thiserror::Error;

/// Async host request envelope for simulator middleware actions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AsyncHostRequest {
    /// Register protocol/participants in scenario middleware.
    SetupChoreography {
        /// Protocol name.
        protocol: String,
        /// Participants involved in the protocol.
        participants: Vec<String>,
    },
    /// Apply network middleware condition.
    ApplyNetworkCondition {
        /// Condition identifier (e.g. "partition", "delay").
        condition: String,
        /// Participants affected.
        participants: Vec<String>,
        /// Duration of condition in ticks.
        duration_ticks: u64,
    },
    /// Inject a middleware fault for a participant.
    InjectFault {
        /// Fault target.
        participant: String,
        /// Fault behavior class.
        behavior: String,
    },
    /// Execute simulator property verification pass.
    VerifyAllProperties,
    /// Advance deterministic scenario time.
    WaitTicks {
        /// Tick delta.
        ticks: u64,
    },
}

/// Normalized async host response.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AsyncHostResponse {
    /// Request accepted and applied.
    Ack,
    /// Request rejected with deterministic reason text.
    Rejected {
        /// Rejection reason.
        reason: String,
    },
}

/// Replay artifact entry for async host boundary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AsyncHostTranscriptEntry {
    /// Monotone request sequence id.
    pub sequence: u64,
    /// Request payload.
    pub request: AsyncHostRequest,
    /// Normalized response payload.
    pub response: AsyncHostResponse,
}

#[derive(Clone, Debug)]
struct PendingRequest {
    sequence: u64,
    request: AsyncHostRequest,
}

/// Errors produced by async host bridge processing and replay.
#[derive(Debug, Error)]
pub enum AsyncHostError {
    /// No request available for resume.
    #[error("async host queue is empty")]
    QueueEmpty,

    /// Replay sequence mismatch.
    #[error("replay sequence mismatch: expected {expected}, got {actual}")]
    ReplaySequenceMismatch { expected: u64, actual: u64 },

    /// Replay response mismatch.
    #[error("replay response mismatch at sequence {sequence}")]
    ReplayResponseMismatch {
        /// Sequence id where mismatch occurred.
        sequence: u64,
        /// Expected response.
        expected: AsyncHostResponse,
        /// Actual response.
        actual: AsyncHostResponse,
    },
}

/// Async simulator host request/resume bridge.
///
/// This bridge keeps VM-side semantics deterministic by:
/// - assigning monotone sequence IDs at submit time
/// - processing requests strictly FIFO during resume
/// - recording normalized request/response transcript entries for replay
pub struct AsyncSimulatorHostBridge {
    scenario_handler: Arc<SimulationScenarioHandler>,
    fault_handler: Arc<SimulationFaultHandler>,
    queue: VecDeque<PendingRequest>,
    transcript: Vec<AsyncHostTranscriptEntry>,
    next_sequence: u64,
}

impl AsyncSimulatorHostBridge {
    /// Create a bridge with default simulator handlers for a deterministic seed.
    pub fn new(seed: u64) -> Self {
        Self::with_handlers(
            Arc::new(SimulationScenarioHandler::new(seed)),
            Arc::new(SimulationFaultHandler::new(seed)),
        )
    }

    /// Create a bridge from explicit simulator handlers.
    pub fn with_handlers(
        scenario_handler: Arc<SimulationScenarioHandler>,
        fault_handler: Arc<SimulationFaultHandler>,
    ) -> Self {
        Self {
            scenario_handler,
            fault_handler,
            queue: VecDeque::new(),
            transcript: Vec::new(),
            next_sequence: 0,
        }
    }

    /// Submit a request and return the deterministic sequence ID.
    pub fn submit(&mut self, request: AsyncHostRequest) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.queue.push_back(PendingRequest { sequence, request });
        sequence
    }

    /// Number of requests currently queued.
    pub fn pending_len(&self) -> usize {
        self.queue.len()
    }

    /// Immutable replay artifact view.
    pub fn transcript(&self) -> &[AsyncHostTranscriptEntry] {
        &self.transcript
    }

    /// Consume and return replay artifacts.
    pub fn take_transcript(&mut self) -> Vec<AsyncHostTranscriptEntry> {
        std::mem::take(&mut self.transcript)
    }

    /// Resume processing for the next queued request.
    pub async fn resume_next(&mut self) -> Result<AsyncHostTranscriptEntry, AsyncHostError> {
        let pending = self.queue.pop_front().ok_or(AsyncHostError::QueueEmpty)?;
        let response = apply_request(
            self.scenario_handler.as_ref(),
            self.fault_handler.as_ref(),
            &pending.request,
        )
        .await;
        let entry = AsyncHostTranscriptEntry {
            sequence: pending.sequence,
            request: pending.request,
            response,
        };
        self.transcript.push(entry.clone());
        Ok(entry)
    }

    /// Replay a recorded transcript and assert deterministic equivalence.
    pub async fn replay_expected(
        &mut self,
        expected: &[AsyncHostTranscriptEntry],
    ) -> Result<(), AsyncHostError> {
        for entry in expected {
            let seq = self.submit(entry.request.clone());
            if seq != entry.sequence {
                return Err(AsyncHostError::ReplaySequenceMismatch {
                    expected: entry.sequence,
                    actual: seq,
                });
            }
            let actual = self.resume_next().await?;
            if actual.response != entry.response {
                return Err(AsyncHostError::ReplayResponseMismatch {
                    sequence: entry.sequence,
                    expected: entry.response.clone(),
                    actual: actual.response,
                });
            }
        }
        Ok(())
    }
}

async fn apply_request(
    scenario_handler: &SimulationScenarioHandler,
    fault_handler: &SimulationFaultHandler,
    request: &AsyncHostRequest,
) -> AsyncHostResponse {
    let outcome = match request {
        AsyncHostRequest::SetupChoreography {
            protocol,
            participants,
        } => scenario_handler.setup_choreography(protocol, participants.clone()),
        AsyncHostRequest::ApplyNetworkCondition {
            condition,
            participants,
            duration_ticks,
        } => scenario_handler.apply_network_condition(
            condition,
            participants.clone(),
            *duration_ticks,
        ),
        AsyncHostRequest::InjectFault {
            participant,
            behavior,
        } => {
            let scenario_result = scenario_handler.inject_fault(participant, behavior);
            if let Err(err) = scenario_result {
                Err(err)
            } else {
                let byzantine = map_byzantine_behavior(behavior);
                match fault_handler
                    .inject_byzantine_behavior(vec![participant.clone()], byzantine)
                    .await
                {
                    Ok(()) => Ok(()),
                    Err(err) => Err(aura_core::effects::TestingError::EventRecordingError {
                        event_type: "inject_fault".to_string(),
                        reason: err.to_string(),
                    }),
                }
            }
        }
        AsyncHostRequest::VerifyAllProperties => scenario_handler.verify_all_properties(),
        AsyncHostRequest::WaitTicks { ticks } => scenario_handler.wait_ticks(*ticks),
    };

    match outcome {
        Ok(()) => AsyncHostResponse::Ack,
        Err(err) => AsyncHostResponse::Rejected {
            reason: err.to_string(),
        },
    }
}

fn map_byzantine_behavior(behavior: &str) -> ByzantineType {
    match behavior.to_ascii_lowercase().as_str() {
        "equivocation" => ByzantineType::Equivocation,
        "invalid_signature" | "invalid-signature" => ByzantineType::InvalidSignature,
        "protocol_violation" | "protocol-violation" => ByzantineType::ProtocolViolation,
        "random" => ByzantineType::Random,
        _ => ByzantineType::Silent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn run_sync_host(
        seed: u64,
        requests: &[AsyncHostRequest],
    ) -> Vec<AsyncHostTranscriptEntry> {
        let scenario = SimulationScenarioHandler::new(seed);
        let fault = SimulationFaultHandler::new(seed);
        let mut transcript = Vec::new();
        for (index, request) in requests.iter().enumerate() {
            let response = apply_request(&scenario, &fault, request).await;
            transcript.push(AsyncHostTranscriptEntry {
                sequence: index as u64,
                request: request.clone(),
                response,
            });
        }
        transcript
    }

    async fn run_async_host(
        seed: u64,
        requests: &[AsyncHostRequest],
    ) -> Vec<AsyncHostTranscriptEntry> {
        let mut bridge = AsyncSimulatorHostBridge::new(seed);
        for request in requests {
            bridge.submit(request.clone());
        }
        let mut produced = Vec::new();
        while bridge.pending_len() > 0 {
            produced.push(bridge.resume_next().await.expect("resume succeeds"));
        }
        produced
    }

    fn representative_requests() -> Vec<AsyncHostRequest> {
        vec![
            AsyncHostRequest::SetupChoreography {
                protocol: "guardian_setup".to_string(),
                participants: vec!["account".to_string(), "guardian-1".to_string()],
            },
            AsyncHostRequest::ApplyNetworkCondition {
                condition: "partition".to_string(),
                participants: vec!["guardian-1".to_string()],
                duration_ticks: 5,
            },
            AsyncHostRequest::InjectFault {
                participant: "guardian-1".to_string(),
                behavior: "equivocation".to_string(),
            },
            AsyncHostRequest::VerifyAllProperties,
            AsyncHostRequest::WaitTicks { ticks: 10 },
        ]
    }

    #[tokio::test]
    async fn async_host_parity_matches_sync_host_on_representative_suite() {
        let requests = representative_requests();
        let sync_trace = run_sync_host(42, &requests).await;
        let async_trace = run_async_host(42, &requests).await;
        assert_eq!(async_trace, sync_trace);
    }

    #[tokio::test]
    async fn async_host_replay_matches_recorded_transcript() {
        let requests = representative_requests();
        let recorded = run_async_host(99, &requests).await;

        let mut bridge = AsyncSimulatorHostBridge::new(99);
        bridge
            .replay_expected(&recorded)
            .await
            .expect("replay should match");
    }

    #[tokio::test]
    async fn async_host_replay_detects_mismatch() {
        let requests = representative_requests();
        let mut recorded = run_async_host(7, &requests).await;
        recorded[0].response = AsyncHostResponse::Rejected {
            reason: "forced mismatch".to_string(),
        };

        let mut bridge = AsyncSimulatorHostBridge::new(7);
        let result = bridge.replay_expected(&recorded).await;
        assert!(matches!(
            result,
            Err(AsyncHostError::ReplayResponseMismatch { .. })
        ));
    }
}

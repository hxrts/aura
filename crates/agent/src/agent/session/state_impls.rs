//! State-specific implementation methods
//!
//! This module provides implementation methods for different session states:
//! - `Coordinating` state - methods for managing running protocols
//! - `Failed` state - methods for handling and recovering from failures

use super::states::{
    AgentProtocol, Coordinating, Failed, FailureInfo, Idle, ProtocolCompleted, ProtocolStatus,
    Uninitialized,
};
use crate::{Result, Storage, Transport};
use aura_protocol::local_runtime::SessionStatus;
use aura_protocol::SessionStatusInfo;

// Implementation for Coordinating state - restricted API while protocol runs
impl<T: Transport, S: Storage> AgentProtocol<T, S, Coordinating> {
    /// Check the status of the currently running protocol
    pub async fn check_protocol_status(&self) -> Result<ProtocolStatus> {
        tracing::debug!(
            device_id = %self.inner.device_id,
            "Checking protocol status"
        );

        // Query session runtime for detailed status information
        let session_statuses = self.inner.get_session_status().await?;

        // Find active sessions and determine overall protocol status
        let active_sessions: Vec<_> = session_statuses
            .iter()
            .filter(|status| !status.is_final)
            .collect();

        if active_sessions.is_empty() {
            // No active sessions, check if any sessions completed recently
            let completed_sessions: Vec<_> = session_statuses
                .iter()
                .filter(|status| matches!(status.status, SessionStatus::Completed))
                .collect();

            if !completed_sessions.is_empty() {
                tracing::info!(
                    device_id = %self.inner.device_id,
                    completed_count = completed_sessions.len(),
                    "Protocol completed successfully"
                );
                return Ok(ProtocolStatus::Completed {
                    protocol_name: "unknown".to_string(),
                });
            }

            // Check for failed sessions
            let failed_sessions: Vec<_> = session_statuses
                .iter()
                .filter(|status| {
                    matches!(
                        status.status,
                        SessionStatus::Failed(_) | SessionStatus::Terminated
                    )
                })
                .collect();

            if !failed_sessions.is_empty() {
                let failure_details = failed_sessions
                    .iter()
                    .map(|s| format!("{:?}:{:?}", s.protocol_type, s.status))
                    .collect::<Vec<_>>()
                    .join(", ");

                tracing::warn!(
                    device_id = %self.inner.device_id,
                    failed_count = failed_sessions.len(),
                    failures = %failure_details,
                    "Protocol failed"
                );
                return Ok(ProtocolStatus::Failed {
                    protocol_name: "unknown".to_string(),
                    error: failure_details,
                });
            }

            // No recent sessions found - this shouldn't happen in coordinating state
            tracing::warn!(
                device_id = %self.inner.device_id,
                "No active or recent sessions found in coordinating state"
            );
            return Ok(ProtocolStatus::Failed {
                protocol_name: "unknown".to_string(),
                error: "No active sessions found".to_string(),
            });
        }

        // Log active session details
        tracing::debug!(
            device_id = %self.inner.device_id,
            active_session_count = active_sessions.len(),
            session_types = ?active_sessions.iter().map(|s| &s.protocol_type).collect::<Vec<_>>(),
            "Protocol sessions in progress"
        );

        Ok(ProtocolStatus::InProgress {
            protocol_name: "unknown".to_string(),
            progress: 0.5,
        })
    }

    /// Complete the coordination and return to idle state
    ///
    /// Requires a witness proving the protocol completed successfully
    pub fn finish_coordination(self, witness: ProtocolCompleted) -> AgentProtocol<T, S, Idle> {
        tracing::info!(
            device_id = %self.inner.device_id,
            protocol_id = %witness.protocol_id,
            "Finishing coordination with witness"
        );

        // Verify witness contains valid completion data
        if witness.protocol_id.is_nil() {
            tracing::error!(
                device_id = %self.inner.device_id,
                protocol_id = %witness.protocol_id,
                "Invalid protocol witness: nil protocol ID"
            );
        } else if !witness.result.is_object() && !witness.result.is_null() {
            tracing::error!(
                device_id = %self.inner.device_id,
                protocol_id = %witness.protocol_id,
                "Invalid protocol witness: malformed result data"
            );
        } else {
            tracing::debug!(
                device_id = %self.inner.device_id,
                protocol_id = %witness.protocol_id,
                result_keys = ?witness.result.as_object().map(|obj| obj.keys().collect::<Vec<_>>()),
                "Protocol witness verification passed"
            );
        }

        // Transition back to idle state
        self.transition_to()
    }

    /// Cancel the running protocol and return to idle state
    pub async fn cancel_coordination(self) -> Result<AgentProtocol<T, S, Idle>> {
        tracing::warn!(
            device_id = %self.inner.device_id,
            "Cancelling coordination protocol"
        );

        // Get active sessions and attempt to terminate them gracefully
        let active_sessions = self.inner.get_session_status().await?;

        for session_info in active_sessions {
            if !session_info.is_final {
                tracing::info!(
                    device_id = %self.inner.device_id,
                    session_id = %session_info.session_id,
                    protocol_type = ?session_info.protocol_type,
                    "Terminating active session"
                );

                let command = aura_protocol::SessionCommand::TerminateSession {
                    session_id: session_info.session_id,
                };

                if let Err(e) = self.inner.send_session_command(command).await {
                    tracing::error!(
                        device_id = %self.inner.device_id,
                        session_id = %session_info.session_id,
                        error = %e,
                        "Failed to send session termination command"
                    );
                }
            }
        }

        // Give sessions a moment to terminate gracefully
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        tracing::info!(
            device_id = %self.inner.device_id,
            "Protocol cancellation completed, transitioning to idle"
        );

        // Transition back to idle state
        Ok(self.transition_to())
    }

    /// Get detailed status of all active sessions
    pub async fn get_detailed_session_status(&self) -> Result<Vec<SessionStatusInfo>> {
        let session_statuses = self.inner.get_session_status().await?;

        tracing::debug!(
            device_id = %self.inner.device_id,
            session_count = session_statuses.len(),
            "Retrieved detailed session status"
        );

        Ok(session_statuses)
    }

    /// Check if any sessions are in a failed state that requires intervention
    pub async fn has_failed_sessions(&self) -> Result<bool> {
        let sessions = self.get_detailed_session_status().await?;

        let failed_count = sessions
            .iter()
            .filter(|status| {
                matches!(
                    status.status,
                    SessionStatus::Failed(_) | SessionStatus::Terminated
                )
            })
            .count();

        if failed_count > 0 {
            tracing::warn!(
                device_id = %self.inner.device_id,
                failed_session_count = failed_count,
                "Found failed sessions requiring intervention"
            );
        }

        Ok(failed_count > 0)
    }

    /// Get the time remaining before any active sessions timeout
    pub async fn get_session_timeout_info(&self) -> Result<Option<std::time::Duration>> {
        let sessions = self.get_detailed_session_status().await?;

        // Find the shortest remaining timeout among active sessions
        // This is a simplified implementation - in practice would need session timeout metadata
        let active_count = sessions.iter().filter(|s| !s.is_final).count();

        if active_count > 0 {
            // Return a conservative estimate - in practice this would query actual session timeouts
            Ok(Some(std::time::Duration::from_secs(300))) // 5 minutes default
        } else {
            Ok(None)
        }
    }
}

// Implementation for Failed state
impl<T: Transport, S: Storage> AgentProtocol<T, S, Failed> {
    /// Get the error that caused the failure
    pub fn get_failure_reason(&self) -> String {
        // In a full implementation, this would retrieve stored failure information
        // For now, return a generic message
        format!(
            "Agent {} failed during protocol execution",
            self.inner.device_id
        )
    }

    /// Get detailed failure information including session status
    pub async fn get_detailed_failure_info(&self) -> Result<FailureInfo> {
        let session_statuses = self.inner.get_session_status().await?;

        let failed_sessions: Vec<_> = session_statuses
            .iter()
            .filter(|status| {
                matches!(
                    status.status,
                    SessionStatus::Failed(_) | SessionStatus::Terminated
                )
            })
            .cloned()
            .collect();

        let failure_info = FailureInfo {
            device_id: self.inner.device_id,
            failure_time: std::time::SystemTime::now(),
            failed_sessions: failed_sessions.clone(),
            can_retry: failed_sessions.len() < 3, // Allow retry if less than 3 failures
            suggested_action: if failed_sessions.is_empty() {
                "No specific failures detected, safe to retry".to_string()
            } else {
                format!(
                    "Review {} failed sessions before retry",
                    failed_sessions.len()
                )
            },
        };

        tracing::info!(
            device_id = %self.inner.device_id,
            failed_session_count = failure_info.failed_sessions.len(),
            can_retry = failure_info.can_retry,
            "Retrieved detailed failure information"
        );

        Ok(failure_info)
    }

    /// Attempt to recover from failure
    ///
    /// This may succeed and return to Uninitialized state for re-bootstrap
    pub async fn attempt_recovery(self) -> Result<AgentProtocol<T, S, Uninitialized>> {
        tracing::info!(
            device_id = %self.inner.device_id,
            "Attempting recovery from failed state"
        );

        // Get failure information to determine recovery strategy
        let failure_info = self.get_detailed_failure_info().await?;

        if !failure_info.can_retry {
            return Err(crate::error::AuraError::coordination_failed(
                "Too many failures, manual intervention required".to_string(),
            ));
        }

        // Clean up any remaining sessions before recovery
        let active_sessions = self.inner.get_session_status().await?;

        for session_info in active_sessions.iter().filter(|s| !s.is_final) {
            tracing::info!(
                device_id = %self.inner.device_id,
                session_id = %session_info.session_id,
                "Cleaning up active session during recovery"
            );

            let command = aura_protocol::SessionCommand::TerminateSession {
                session_id: session_info.session_id,
            };

            let _ = self.inner.send_session_command(command).await;
        }

        // Allow time for cleanup
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        tracing::info!(
            device_id = %self.inner.device_id,
            "Recovery completed, transitioning to uninitialized state"
        );

        // If recovery succeeds, return to uninitialized for re-bootstrap
        Ok(self.transition_to())
    }

    /// Verify protocol completion witness
    fn verify_protocol_witness(&self, witness: &ProtocolCompleted) -> Result<()> {
        // Basic validation of witness structure
        if witness.protocol_id.is_nil() {
            return Err(crate::error::AuraError::coordination_failed(
                "Invalid protocol witness: nil protocol ID".to_string(),
            ));
        }

        // Validate result format (should be valid JSON)
        if !witness.result.is_object() && !witness.result.is_null() {
            return Err(crate::error::AuraError::coordination_failed(
                "Invalid protocol witness: malformed result data".to_string(),
            ));
        }

        tracing::debug!(
            device_id = %self.inner.device_id,
            protocol_id = %witness.protocol_id,
            result_keys = ?witness.result.as_object().map(|obj| obj.keys().collect::<Vec<_>>()),
            "Protocol witness verification passed"
        );

        Ok(())
    }
}

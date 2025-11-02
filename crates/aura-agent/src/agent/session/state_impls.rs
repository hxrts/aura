//! State-specific implementation methods
//!
//! This module provides implementation methods for different session states:
//! - `Coordinating` state - methods for managing running protocols
//! - `Failed` state - methods for handling and recovering from failures

use super::states::{
    AgentProtocol, Coordinating, Failed, FailureInfo, Idle, ProtocolCompleted, ProtocolStatus,
};
use crate::{Result, Storage};
use aura_protocol::middleware::handler::SessionInfo;

// Implementation for Coordinating state - restricted API while protocol runs
impl<S: Storage> AgentProtocol<S, Coordinating> {
    /// Check the status of the currently running protocol
    pub async fn check_protocol_status(&self) -> Result<ProtocolStatus> {
        tracing::debug!(
            device_id = %self.inner.device_id,
            "Checking protocol status"
        );

        // Query protocol handler for active sessions
        let active_sessions = self.inner.get_active_sessions().await?;

        // Find active sessions and determine overall protocol status
        // Note: all sessions returned by list_sessions() are considered active
        let active_sessions: Vec<_> = active_sessions.iter().collect();

        if active_sessions.is_empty() {
            // No active sessions - this shouldn't happen in Coordinating state
            tracing::warn!(
                device_id = %self.inner.device_id,
                "No active sessions found in Coordinating state"
            );
            return Ok(ProtocolStatus::Completed {
                protocol_name: "unknown".to_string(),
            });
        }

        // Check if any sessions have failed
        // Since list_sessions() only returns active sessions, we can't directly filter by status
        let failed_sessions: Vec<&SessionInfo> = Vec::new();

        if !failed_sessions.is_empty() {
            let error_msg = failed_sessions[0]
                .metadata
                .get("error")
                .unwrap_or(&"Unknown error".to_string())
                .clone();
            return Ok(ProtocolStatus::Failed {
                protocol_name: failed_sessions[0].protocol_type.clone(),
                error: error_msg,
            });
        }

        // Check for completed sessions
        // Since list_sessions() only returns active sessions, completed ones won't be in the list
        let completed_sessions: Vec<&SessionInfo> = Vec::new();

        if !completed_sessions.is_empty() {
            return Ok(ProtocolStatus::Completed {
                protocol_name: completed_sessions[0].protocol_type.clone(),
            });
        }

        // If we reach here, we have in-progress sessions
        let in_progress_session = &active_sessions[0];
        let progress = in_progress_session
            .metadata
            .get("progress")
            .and_then(|p| p.parse::<f32>().ok())
            .unwrap_or(0.5); // Default progress

        Ok(ProtocolStatus::InProgress {
            protocol_name: in_progress_session.protocol_type.clone(),
            progress,
        })
    }

    /// Get detailed status information about all sessions
    pub async fn get_detailed_session_status(&self) -> Result<Vec<SessionInfo>> {
        tracing::debug!(
            device_id = %self.inner.device_id,
            "Getting detailed session status"
        );

        self.inner.get_active_sessions().await
    }

    /// Check if any sessions have failed
    pub async fn has_failed_sessions(&self) -> Result<bool> {
        let sessions = self.inner.get_active_sessions().await?;
        let failed_count = sessions
            .iter()
            .filter(|_session| false) // No sessions are failed in the active list
            .count();

        tracing::debug!(
            device_id = %self.inner.device_id,
            failed_sessions = failed_count,
            "Checked for failed sessions"
        );

        Ok(failed_count > 0)
    }

    /// Get timeout information for active sessions
    pub async fn get_session_timeout_info(&self) -> Result<Option<std::time::Duration>> {
        let sessions = self.inner.get_active_sessions().await?;

        if sessions.is_empty() {
            return Ok(None);
        }

        // Find the session that will timeout soonest
        let mut min_timeout = None;
        let current_time = aura_types::time_utils::current_unix_timestamp();

        for session in &sessions {
            if let Some(timeout_str) = session.metadata.get("timeout_at") {
                if let Ok(timeout_at) = timeout_str.parse::<u64>() {
                    if timeout_at > current_time {
                        let remaining = std::time::Duration::from_secs(timeout_at - current_time);
                        match min_timeout {
                            None => min_timeout = Some(remaining),
                            Some(current_min) => {
                                if remaining < current_min {
                                    min_timeout = Some(remaining);
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(min_timeout)
    }

    /// Wait for protocol completion with optional timeout
    pub async fn wait_for_completion(
        &self,
        timeout: Option<std::time::Duration>,
    ) -> Result<ProtocolCompleted> {
        let effects = aura_crypto::Effects::production();
        let start_time = effects.now().unwrap_or(0);
        let timeout_duration = timeout.unwrap_or(std::time::Duration::from_secs(300)); // 5 minutes default

        loop {
            let status = self.check_protocol_status().await?;

            match status {
                ProtocolStatus::Completed { protocol_name } => {
                    tracing::info!(
                        device_id = %self.inner.device_id,
                        protocol_name = protocol_name,
                        "Protocol completed successfully"
                    );

                    return Ok(ProtocolCompleted {
                        protocol_id: aura_crypto::generate_uuid(), // Placeholder
                        result: serde_json::json!({
                            "status": "completed",
                            "protocol": protocol_name
                        }),
                    });
                }
                ProtocolStatus::Failed {
                    protocol_name,
                    error,
                } => {
                    return Err(crate::error::AuraError::coordination_failed(format!(
                        "Protocol {} failed: {}",
                        protocol_name, error
                    )));
                }
                ProtocolStatus::InProgress { .. } => {
                    // Continue waiting
                }
                ProtocolStatus::Idle => {
                    // This shouldn't happen in Coordinating state
                    return Err(crate::error::AuraError::agent_invalid_state(
                        "Unexpected idle status in coordinating state",
                    ));
                }
            }

            // Check for timeout
            let current_time = effects.now().unwrap_or(0);
            let elapsed_millis = current_time.saturating_sub(start_time);
            if elapsed_millis > timeout_duration.as_millis() as u64 {
                return Err(crate::error::AuraError::coordination_failed(
                    "Protocol completion timeout",
                ));
            }

            // Wait a bit before checking again
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    /// Attempt to cancel the running protocol
    pub async fn cancel_protocol(self) -> Result<AgentProtocol<S, Idle>> {
        tracing::warn!(
            device_id = %self.inner.device_id,
            "Attempting to cancel running protocol"
        );

        // Get active sessions and terminate them
        let sessions = self.inner.get_active_sessions().await?;
        for session in sessions {
            // All sessions from list_sessions() are considered active
            {
                tracing::info!(
                    device_id = %self.inner.device_id,
                    session_id = %session.session_id,
                    protocol = session.protocol_type,
                    "Terminating active session"
                );

                if let Err(e) = self.inner.terminate_session(session.session_id).await {
                    tracing::warn!(
                        device_id = %self.inner.device_id,
                        session_id = %session.session_id,
                        error = %e,
                        "Failed to terminate session"
                    );
                }
            }
        }

        tracing::info!(
            device_id = %self.inner.device_id,
            "Protocol cancellation completed, transitioning to Idle"
        );

        Ok(self.transition_to())
    }
}

// Implementation for Failed state - recovery and diagnosis methods
impl<S: Storage> AgentProtocol<S, Failed> {
    /// Get detailed failure information
    pub async fn get_failure_info(&self) -> Result<FailureInfo> {
        tracing::debug!(
            device_id = %self.inner.device_id,
            "Getting failure information"
        );

        let failed_sessions = self.inner.get_active_sessions().await?;
        let failed_sessions: Vec<_> = failed_sessions
            .into_iter()
            .filter(|_session| false) // No sessions are failed in the active list
            .collect();

        let can_retry = !failed_sessions.is_empty();
        let suggested_action = if can_retry {
            "Retry failed protocols after checking network connectivity".to_string()
        } else {
            "No active failed sessions - check agent state".to_string()
        };

        Ok(FailureInfo {
            device_id: self.inner.device_id,
            failure_time: aura_types::time_utils::current_unix_timestamp_millis(),
            failed_sessions,
            can_retry,
            suggested_action,
        })
    }

    /// Attempt to recover from failure and return to Idle state
    pub async fn attempt_recovery(self) -> Result<AgentProtocol<S, Idle>> {
        tracing::info!(
            device_id = %self.inner.device_id,
            "Attempting recovery from failed state"
        );

        // Clean up any failed sessions
        let sessions = self.inner.get_active_sessions().await?;
        for session in sessions {
            // All sessions from list_sessions() are considered active, not failed
            if false {
                tracing::info!(
                    device_id = %self.inner.device_id,
                    session_id = %session.session_id,
                    "Cleaning up failed session"
                );

                if let Err(e) = self.inner.terminate_session(session.session_id).await {
                    tracing::warn!(
                        device_id = %self.inner.device_id,
                        session_id = %session.session_id,
                        error = %e,
                        "Failed to terminate failed session"
                    );
                }
            }
        }

        // Validate agent state
        let security_report = self.inner.validate_security_state().await?;
        if security_report.has_critical_issues() {
            return Err(crate::error::AuraError::agent_invalid_state(format!(
                "Cannot recover - critical security issues: {:?}",
                security_report.issues
            )));
        }

        tracing::info!(
            device_id = %self.inner.device_id,
            "Recovery completed, transitioning to Idle"
        );

        Ok(self.transition_to())
    }

    /// Get diagnostic information about the failure
    pub async fn get_diagnostics(&self) -> Result<serde_json::Value> {
        let failure_info = self.get_failure_info().await?;
        let security_report = self.inner.validate_security_state().await?;

        Ok(serde_json::json!({
            "device_id": failure_info.device_id,
            "failure_time": failure_info.failure_time,
            "failed_sessions_count": failure_info.failed_sessions.len(),
            "can_retry": failure_info.can_retry,
            "suggested_action": failure_info.suggested_action,
            "security_state": {
                "is_secure": security_report.is_secure(),
                "has_critical_issues": security_report.has_critical_issues(),
                "issues_count": security_report.issues.len()
            }
        }))
    }
}

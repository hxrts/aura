//! Session Establishment Choreography
//!
//! This module implements distributed session ticket creation and validation
//! using choreographic programming principles.

use crate::{AuraError, AuraResult};
use aura_core::{AccountId, DeviceId};
use aura_mpst::{AuraRuntime, CapabilityGuard, JournalAnnotation};
use aura_verify::session::{SessionScope, SessionTicket};
use aura_verify::{Ed25519Signature, IdentityProof, VerifiedIdentity};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::Mutex;

/// Session establishment request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEstablishmentRequest {
    /// Device requesting the session
    pub device_id: DeviceId,
    /// Account context
    pub account_id: AccountId,
    /// Verified identity from authentication
    pub verified_identity: VerifiedIdentity,
    /// Requested session scope
    pub requested_scope: SessionScope,
    /// Session duration in seconds
    pub duration_seconds: u64,
}

/// Session establishment response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEstablishmentResponse {
    /// Created session ticket
    pub session_ticket: Option<SessionTicket>,
    /// Participating devices
    pub participants: Vec<DeviceId>,
    /// Success indicator
    pub success: bool,
    /// Error message if any
    pub error: Option<String>,
}

/// Message types for session establishment choreography
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionMessage {
    /// Request session creation
    SessionRequest {
        /// Device requesting session
        device_id: DeviceId,
        /// Account context
        account_id: AccountId,
        /// Verified identity
        verified_identity: VerifiedIdentity,
        /// Requested scope
        scope: SessionScope,
        /// Duration in seconds
        duration: u64,
    },

    /// Session creation proposal
    SessionProposal {
        /// Proposed session ticket
        session_ticket: SessionTicket,
        /// Session ID for tracking
        session_id: String,
        /// Expiry timestamp
        expires_at: u64,
    },

    /// Session approval/rejection
    SessionApproval {
        /// Session ID being approved
        session_id: String,
        /// Device approving
        approver_id: DeviceId,
        /// Approval decision
        approved: bool,
        /// Reason if rejected
        reason: Option<String>,
    },

    /// Final session establishment result
    SessionEstablished {
        /// Session ID
        session_id: String,
        /// Final session ticket
        session_ticket: Option<SessionTicket>,
        /// Success status
        success: bool,
        /// Error if failed
        error: Option<String>,
    },
}

/// Roles in session establishment choreography
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionRole {
    /// Device requesting the session
    Requester,
    /// Device approving session creation
    Approver(u32),
    /// Coordinator managing session establishment
    Coordinator,
}

impl SessionRole {
    /// Get the name of this role
    pub fn name(&self) -> String {
        match self {
            SessionRole::Requester => "Requester".to_string(),
            SessionRole::Approver(id) => format!("Approver_{}", id),
            SessionRole::Coordinator => "Coordinator".to_string(),
        }
    }
}

/// Session establishment choreography state
#[derive(Debug)]
pub struct SessionEstablishmentState {
    /// Current request being processed
    current_request: Option<SessionEstablishmentRequest>,
    /// Pending session proposals by session ID
    pending_sessions: HashMap<String, SessionTicket>,
    /// Approvals collected by session ID
    approvals: HashMap<String, Vec<(DeviceId, bool, Option<String>)>>,
    /// Established sessions
    established_sessions: HashMap<String, SessionTicket>,
}

impl SessionEstablishmentState {
    /// Create new state
    pub fn new() -> Self {
        Self {
            current_request: None,
            pending_sessions: HashMap::new(),
            approvals: HashMap::new(),
            established_sessions: HashMap::new(),
        }
    }

    /// Add a session proposal
    pub fn add_session_proposal(&mut self, session_id: String, ticket: SessionTicket) {
        self.pending_sessions.insert(session_id, ticket);
    }

    /// Add approval for a session
    pub fn add_approval(
        &mut self,
        session_id: String,
        device_id: DeviceId,
        approved: bool,
        reason: Option<String>,
    ) {
        self.approvals
            .entry(session_id)
            .or_insert_with(Vec::new)
            .push((device_id, approved, reason));
    }

    /// Check if session has sufficient approvals
    pub fn has_sufficient_approvals(&self, session_id: &str, required_approvals: usize) -> bool {
        self.approvals
            .get(session_id)
            .map(|approvals| {
                approvals
                    .iter()
                    .filter(|(_, approved, _)| *approved)
                    .count()
                    >= required_approvals
            })
            .unwrap_or(false)
    }

    /// Finalize session establishment
    pub fn establish_session(&mut self, session_id: String) -> Option<SessionTicket> {
        if let Some(ticket) = self.pending_sessions.remove(&session_id) {
            self.established_sessions.insert(session_id, ticket.clone());
            Some(ticket)
        } else {
            None
        }
    }
}

/// Session establishment choreography
#[derive(Debug)]
pub struct SessionEstablishmentChoreography {
    /// Local device role
    role: SessionRole,
    /// Choreography state
    state: Mutex<SessionEstablishmentState>,
    /// MPST runtime
    runtime: AuraRuntime,
}

impl SessionEstablishmentChoreography {
    /// Create new session establishment choreography
    pub fn new(role: SessionRole, runtime: AuraRuntime) -> Self {
        Self {
            role,
            state: Mutex::new(SessionEstablishmentState::new()),
            runtime,
        }
    }

    /// Execute the choreography
    pub async fn execute(
        &self,
        request: SessionEstablishmentRequest,
        effect_system: &aura_protocol::effects::system::AuraEffectSystem,
    ) -> AuraResult<SessionEstablishmentResponse> {
        let mut state = self.state.lock().await;
        state.current_request = Some(request.clone());
        drop(state);

        match self.role {
            SessionRole::Requester => self.execute_requester(request, effect_system).await,
            SessionRole::Approver(_) => self.execute_approver(effect_system).await,
            SessionRole::Coordinator => self.execute_coordinator(effect_system).await,
        }
    }

    /// Execute as session requester
    #[allow(clippy::disallowed_methods)]
    async fn execute_requester(
        &self,
        request: SessionEstablishmentRequest,
        _effect_system: &aura_protocol::effects::system::AuraEffectSystem,
    ) -> AuraResult<SessionEstablishmentResponse> {
        tracing::info!(
            "Executing session establishment as requester for device: {}",
            request.device_id
        );

        // Apply capability guard
        let session_cap = aura_core::Cap::with_permissions(vec![
            "session:establish".to_string(),
            "network:send".to_string(),
            "network:receive".to_string(),
        ]);
        let guard = CapabilityGuard::new(session_cap.clone());

        // For MVP, grant session permissions to authenticated devices
        let device_capabilities = session_cap; // Placeholder
        guard.enforce(&device_capabilities).map_err(|e| {
            AuraError::invalid(format!(
                "Insufficient capabilities for session establishment: {}",
                e
            ))
        })?;

        // Generate session ID
        let _session_id = uuid::Uuid::new_v4().to_string();

        // Session establishment would involve:
        // 1. Sending session request to approvers using AuraHandlerAdapter
        // 2. Receiving session proposals
        // 3. Evaluating proposals
        // 4. Finalizing session with selected approver(s)
        //
        // This requires session ticket generation and multi-party agreement
        tracing::warn!(
            "Session establishment requires multi-party coordination - placeholder implementation"
        );

        // Apply journal annotation
        let journal_annotation =
            JournalAnnotation::add_facts("Session establishment request".to_string());
        tracing::info!("Applied journal annotation: {:?}", journal_annotation);

        Ok(SessionEstablishmentResponse {
            session_ticket: None,
            participants: vec![request.device_id],
            success: false,
            error: Some("Session establishment requires multi-party coordination".to_string()),
        })
    }

    /// Execute as session approver
    async fn execute_approver(
        &self,
        _effect_system: &aura_protocol::effects::system::AuraEffectSystem,
    ) -> AuraResult<SessionEstablishmentResponse> {
        tracing::info!("Executing session establishment as approver");

        // Approver role is passive - awaits session requests from requester
        tracing::warn!("Approver role is passive - awaits session requests");

        Ok(SessionEstablishmentResponse {
            session_ticket: None,
            participants: Vec::new(),
            success: false,
            error: Some("Approver role is passive - awaits session requests".to_string()),
        })
    }

    /// Execute as coordinator
    async fn execute_coordinator(
        &self,
        _effect_system: &aura_protocol::effects::system::AuraEffectSystem,
    ) -> AuraResult<SessionEstablishmentResponse> {
        tracing::info!("Executing session establishment as coordinator");

        // Coordinator manages session establishment across multiple approvers
        // For single approver scenarios, requester handles coordination directly
        tracing::warn!(
            "Coordinator role not fully implemented - requester handles single approver"
        );

        Ok(SessionEstablishmentResponse {
            session_ticket: None,
            participants: Vec::new(),
            success: false,
            error: Some("Coordinator role requires multi-approver scenario".to_string()),
        })
    }
}

/// Session establishment coordinator
#[derive(Debug)]
pub struct SessionEstablishmentCoordinator {
    /// Local runtime
    runtime: AuraRuntime,
    /// Current choreography
    choreography: Option<SessionEstablishmentChoreography>,
}

impl SessionEstablishmentCoordinator {
    /// Create new coordinator
    pub fn new(runtime: AuraRuntime) -> Self {
        Self {
            runtime,
            choreography: None,
        }
    }

    /// Establish a session using choreography
    pub async fn establish_session(
        &mut self,
        request: SessionEstablishmentRequest,
        effect_system: &aura_protocol::effects::system::AuraEffectSystem,
    ) -> AuraResult<SessionEstablishmentResponse> {
        tracing::info!(
            "Starting session establishment for device: {}",
            request.device_id
        );

        // Create choreography with requester role
        let choreography =
            SessionEstablishmentChoreography::new(SessionRole::Requester, self.runtime.clone());

        // Execute the choreography with effect system
        let result = choreography.execute(request, effect_system).await;

        // Store choreography for potential follow-up operations
        self.choreography = Some(choreography);

        result
    }

    /// Get the current runtime
    pub fn runtime(&self) -> &AuraRuntime {
        &self.runtime
    }

    /// Check if a choreography is currently active
    pub fn has_active_choreography(&self) -> bool {
        self.choreography.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{AccountId, Cap, DeviceId, Journal};
    use aura_verify::session::{SessionScope, SessionTicket};
    use uuid::Uuid;

    #[tokio::test]
    async fn test_session_state_creation() {
        let mut state = SessionEstablishmentState::new();

        let session_id = "test_session".to_string();
        let device_id = DeviceId::new();

        // Create a dummy session ticket
        let ticket = SessionTicket {
            session_id: Uuid::new_v4(),
            issuer_device_id: device_id,
            issued_at: 1000,
            expires_at: 4600, // 1 hour later
            scope: SessionScope::Dkd {
                app_id: "test-app".to_string(),
                context: "test-context".to_string(),
            },
            nonce: [1u8; 16],
        };

        state.add_session_proposal(session_id.clone(), ticket);
        state.add_approval(session_id.clone(), device_id, true, None);

        assert!(state.has_sufficient_approvals(&session_id, 1));
        assert!(state.establish_session(session_id).is_some());
    }

    #[tokio::test]
    async fn test_session_coordinator() {
        let device_id = DeviceId::new();
        let runtime = AuraRuntime::new(device_id, Cap::top(), Journal::new());

        let mut coordinator = SessionEstablishmentCoordinator::new(runtime);
        assert!(!coordinator.has_active_choreography());

        let request = SessionEstablishmentRequest {
            device_id,
            account_id: AccountId::new(),
            verified_identity: VerifiedIdentity {
                proof: IdentityProof::Device {
                    device_id,
                    signature: Ed25519Signature::from_slice(&[0u8; 64]).unwrap(),
                },
                message_hash: [0u8; 32],
            },
            requested_scope: SessionScope::Dkd {
                app_id: "test-app".to_string(),
                context: "test-context".to_string(),
            },
            duration_seconds: 3600,
        };

        // Note: This will return Ok with success=false since choreography is not fully implemented
        let result = coordinator.establish_session(request).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(!response.success);
        assert!(coordinator.has_active_choreography());
    }
}

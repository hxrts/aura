//! Local Session Runtime for Per-Device Protocol Coordination
//!
//! This module provides a local session runtime that replaces implicit choreographic
//! execution with explicit per-device session management. The runtime coordinates
//! multiple protocols simultaneously while maintaining session type safety.

use crate::execution::ProtocolError;
use crate::session_types::new_session_typed_agent;
use crate::LifecycleScheduler;
use aura_crypto::Effects;
use aura_journal::{Event, EventType, InitiateDkdSessionEvent};
// Import from protocol-types
pub use aura_protocol_types::{
    DkdResult, SessionCommand, SessionId, SessionProtocolType, SessionResponse, SessionStatus,
    SessionStatusInfo, TransportEvent, TransportSession,
};
use aura_types::{AccountId, DeviceId};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Types of peer events for session notification
#[derive(Debug, Clone)]
enum PeerEventType {
    Connected,
    Disconnected,
}

/// Message envelope for routing between sessions
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SessionMessage {
    /// Target session ID
    session_id: Uuid,
    /// Event type for validation
    event_type: EventType,
    /// Message payload
    payload: Vec<u8>,
    /// Sender peer ID
    sender: String,
}

/// Active session tracking
#[derive(Debug)]
struct ActiveSession {
    session_id: Uuid,
    protocol_type: SessionProtocolType,
    status: SessionStatus,
    transport_session: Option<Box<dyn TransportSession>>,
    sender: mpsc::UnboundedSender<Event>,
}

/// Local Session Runtime for per-device coordination
///
/// Manages multiple concurrent protocol sessions while maintaining session type safety.
/// Replaces implicit choreographic execution with explicit session management.
pub struct LocalSessionRuntime {
    /// Device identity
    device_id: DeviceId,
    /// Account identity
    account_id: AccountId,
    /// Active protocol sessions
    sessions: Arc<RwLock<BTreeMap<Uuid, ActiveSession>>>,
    /// Session status subscribers
    status_subscribers: Arc<RwLock<Vec<mpsc::UnboundedSender<SessionStatusInfo>>>>,
    /// Lifecycle scheduler for protocol execution
    scheduler: Arc<Mutex<LifecycleScheduler>>,
    /// Effects for deterministic testing
    effects: Effects,
    /// Transport message router
    transport_router: Option<Arc<dyn TransportSession>>,
    /// Session command receiver
    command_receiver: Arc<Mutex<Option<mpsc::UnboundedReceiver<SessionCommand>>>>,
    /// Session command sender (for runtime control)
    command_sender: mpsc::UnboundedSender<SessionCommand>,
}

impl LocalSessionRuntime {
    /// Create a new local session runtime
    pub fn new(
        device_id: DeviceId,
        account_id: AccountId,
        scheduler: LifecycleScheduler,
        effects: Effects,
    ) -> Self {
        let (command_sender, command_receiver) = mpsc::unbounded_channel();

        Self {
            device_id,
            account_id,
            sessions: Arc::new(RwLock::new(BTreeMap::new())),
            status_subscribers: Arc::new(RwLock::new(Vec::new())),
            scheduler: Arc::new(Mutex::new(scheduler)),
            effects,
            transport_router: None,
            command_receiver: Arc::new(Mutex::new(Some(command_receiver))),
            command_sender,
        }
    }

    /// Create a new local session runtime with generated key (helper for config)
    pub fn new_with_generated_key(
        device_id: DeviceId,
        account_id: AccountId,
        effects: Effects,
    ) -> Self {
        let scheduler = LifecycleScheduler::new();
        Self::new(device_id, account_id, scheduler, effects)
    }

    /// Send a command to the runtime
    pub async fn send_command(
        &self,
        command: SessionCommand,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.command_sender
            .send(command)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
    }

    /// Start a new session
    pub async fn start_session(
        &self,
        command: SessionCommand,
    ) -> Result<SessionResponse, ProtocolError> {
        let session_id = Uuid::new_v4();

        match command {
            SessionCommand::StartDkd {
                app_id,
                context_label,
                participants,
                threshold,
            } => {
                info!(
                    "Starting DKD session {} for app_id={}, context={}",
                    session_id, app_id, context_label
                );

                // Create DKD session event
                let _dkd_event = InitiateDkdSessionEvent {
                    session_id,
                    context_id: format!("{}:{}", app_id, context_label).into_bytes(),
                    participants: participants.clone(),
                    threshold: threshold.unwrap_or(2) as u16,
                    start_epoch: 0,     // TODO: Get from effects
                    ttl_in_epochs: 100, // TODO: Make configurable
                };

                // Create active session entry
                let (sender, _receiver) = mpsc::unbounded_channel();
                let active_session = ActiveSession {
                    session_id,
                    protocol_type: SessionProtocolType::Dkd,
                    status: SessionStatus::Initializing,
                    transport_session: None,
                    sender,
                };

                // Store session
                {
                    let mut sessions = self.sessions.write().await;
                    sessions.insert(session_id, active_session);
                }

                // Notify status change
                self.notify_status_change(SessionStatusInfo {
                    session_id,
                    protocol_type: SessionProtocolType::Dkd,
                    status: SessionStatus::Initializing,
                    participants: participants,
                    is_final: false,
                })
                .await;

                Ok(SessionResponse::SessionStarted {
                    session_id,
                    session_type: SessionProtocolType::Dkd,
                })
            }

            SessionCommand::StartAgent => {
                info!("Starting Agent session {}", session_id);

                // Create agent session using session types
                let _agent = new_session_typed_agent(self.device_id);

                // Create active session entry
                let (sender, _receiver) = mpsc::unbounded_channel();
                let active_session = ActiveSession {
                    session_id,
                    protocol_type: SessionProtocolType::Agent,
                    status: SessionStatus::Active,
                    transport_session: None,
                    sender,
                };

                // Store session
                {
                    let mut sessions = self.sessions.write().await;
                    sessions.insert(session_id, active_session);
                }

                // Notify status change
                self.notify_status_change(SessionStatusInfo {
                    session_id,
                    protocol_type: SessionProtocolType::Agent,
                    status: SessionStatus::Active,
                    participants: vec![self.device_id],
                    is_final: false,
                })
                .await;

                Ok(SessionResponse::SessionStarted {
                    session_id,
                    session_type: SessionProtocolType::Agent,
                })
            }

            SessionCommand::TerminateSession { session_id } => {
                info!("Terminating session {}", session_id);

                // Remove session
                {
                    let mut sessions = self.sessions.write().await;
                    if let Some(_session) = sessions.remove(&session_id) {
                        // Notify termination
                        self.notify_status_change(SessionStatusInfo {
                            session_id,
                            protocol_type: SessionProtocolType::Agent, // Default, should be from session
                            status: SessionStatus::Terminated,
                            participants: vec![],
                            is_final: true,
                        })
                        .await;

                        Ok(SessionResponse::SessionStarted {
                            session_id,
                            session_type: SessionProtocolType::Agent,
                        })
                    } else {
                        Err(ProtocolError::new(format!(
                            "Session not found: {}",
                            session_id
                        )))
                    }
                }
            }

            _ => {
                warn!("Unhandled session command: {:?}", command);
                Err(ProtocolError::new("Command not implemented".to_string()))
            }
        }
    }

    /// Subscribe to session status updates
    pub async fn subscribe_status(&self) -> mpsc::UnboundedReceiver<SessionStatusInfo> {
        let (sender, receiver) = mpsc::unbounded_channel();

        {
            let mut subscribers = self.status_subscribers.write().await;
            subscribers.push(sender);
        }

        receiver
    }

    /// Get current sessions
    pub async fn list_sessions(&self) -> Vec<SessionStatusInfo> {
        let sessions = self.sessions.read().await;
        sessions
            .values()
            .map(|session| SessionStatusInfo {
                session_id: session.session_id,
                protocol_type: session.protocol_type.clone(),
                status: session.status.clone(),
                participants: vec![], // TODO: Store participants in ActiveSession
                is_final: matches!(
                    session.status,
                    SessionStatus::Completed | SessionStatus::Failed(_) | SessionStatus::Terminated
                ),
            })
            .collect()
    }

    /// Run the session runtime event loop
    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Starting LocalSessionRuntime for device {}", self.device_id);

        // Take the command receiver
        let mut command_receiver = {
            let mut receiver_guard = self.command_receiver.lock().await;
            receiver_guard
                .take()
                .ok_or("Command receiver already taken")?
        };

        // Main event loop
        loop {
            tokio::select! {
                // Handle session commands
                command = command_receiver.recv() => {
                    match command {
                        Some(cmd) => {
                            debug!("Processing command: {:?}", cmd);
                            if let Err(e) = self.handle_command(cmd).await {
                                error!("Command handling error: {:?}", e);
                            }
                        }
                        None => {
                            warn!("Command channel closed, stopping runtime");
                            break;
                        }
                    }
                }

                // Add other event sources here (transport, timers, etc.)
            }
        }

        info!("LocalSessionRuntime stopped");
        Ok(())
    }

    /// Handle a session command
    async fn handle_command(&self, command: SessionCommand) -> Result<(), ProtocolError> {
        match self.start_session(command).await {
            Ok(response) => {
                debug!("Command handled successfully: {:?}", response);
                Ok(())
            }
            Err(e) => {
                error!("Command handling failed: {:?}", e);
                Err(e)
            }
        }
    }

    /// Notify status subscribers
    async fn notify_status_change(&self, status_info: SessionStatusInfo) {
        let subscribers = self.status_subscribers.read().await;
        for subscriber in subscribers.iter() {
            if let Err(_) = subscriber.send(status_info.clone()) {
                // Subscriber disconnected, will be cleaned up on next write
            }
        }
    }

    /// Get device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Get account ID
    pub fn account_id(&self) -> AccountId {
        self.account_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_crypto::Effects;
    use aura_types::{AccountIdExt, DeviceIdExt};

    #[tokio::test]
    async fn test_session_runtime_creation() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);
        let account_id = AccountId::new_with_effects(&effects);
        let scheduler = LifecycleScheduler::new();

        let runtime = LocalSessionRuntime::new(device_id, account_id, scheduler, effects);

        assert_eq!(runtime.device_id(), device_id);
        assert_eq!(runtime.account_id(), account_id);
    }

    #[tokio::test]
    async fn test_start_dkd_session() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);
        let account_id = AccountId::new_with_effects(&effects);
        let scheduler = LifecycleScheduler::new();

        let runtime = LocalSessionRuntime::new(device_id, account_id, scheduler, effects);

        let command = SessionCommand::StartDkd {
            app_id: "test-app".to_string(),
            context_label: "test-context".to_string(),
            participants: vec![device_id],
            threshold: Some(1),
        };

        let response = runtime.start_session(command).await.unwrap();

        match response {
            SessionResponse::SessionStarted {
                session_id,
                session_type,
            } => {
                // Verify session was created
                let sessions = runtime.list_sessions().await;
                assert_eq!(sessions.len(), 1);
                assert_eq!(sessions[0].session_id, session_id);
                assert!(matches!(
                    sessions[0].protocol_type,
                    SessionProtocolType::Dkd
                ));
                assert!(matches!(session_type, SessionProtocolType::Dkd));
            }
            _ => panic!("Expected SessionStarted response"),
        }
    }

    #[tokio::test]
    async fn test_status_subscription() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);
        let account_id = AccountId::new_with_effects(&effects);
        let scheduler = LifecycleScheduler::new();

        let runtime = LocalSessionRuntime::new(device_id, account_id, scheduler, effects);
        let mut status_receiver = runtime.subscribe_status().await;

        let command = SessionCommand::StartAgent;
        let response = runtime.start_session(command).await.unwrap();

        // Should receive status update
        let status = status_receiver.recv().await.unwrap();

        match response {
            SessionResponse::SessionStarted {
                session_id,
                session_type,
            } => {
                assert_eq!(status.session_id, session_id);
                assert!(matches!(status.protocol_type, SessionProtocolType::Agent));
                assert!(matches!(status.status, SessionStatus::Active));
                assert!(matches!(session_type, SessionProtocolType::Agent));
            }
            _ => panic!("Expected SessionStarted response"),
        }
    }
}

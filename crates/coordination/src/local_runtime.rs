//! Local Session Runtime for Choreographic Protocols
//!
//! This module provides a per-device session runtime that manages active protocol
//! instances and coordinates with peer devices through the transport layer.

use crate::channels::*;
use aura_session_types::*;
use aura_journal::{AccountId, DeviceId, Event};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Per-device session runtime managing active protocol instances
///
/// The local session runtime coordinates multiple concurrent protocols while ensuring
/// session type safety and maintaining choreographic communication patterns.
#[derive(Clone)]
pub struct LocalSessionRuntime {
    /// Device identifier for this runtime
    device_id: DeviceId,
    /// Account identifier
    account_id: AccountId,
    /// Active protocol sessions
    active_sessions: Arc<RwLock<HashMap<Uuid, ActiveSession>>>,
    /// Command channel for external requests
    command_sender: mpsc::UnboundedSender<SessionCommand>,
    /// Event channel for protocol events
    event_sender: mpsc::UnboundedSender<SessionEvent>,
    /// Effect channel for system effects
    effect_sender: mpsc::UnboundedSender<SessionEffect>,
    /// Protocol channel registry
    channels: ChannelRegistry,
}

/// Active protocol session with type-erased state
pub struct ActiveSession {
    /// Session identifier
    pub session_id: Uuid,
    /// Protocol type
    pub protocol_type: SessionProtocolType,
    /// Current session state (type-erased)
    pub current_state: String,
    /// Can this session be safely terminated
    pub can_terminate: bool,
    /// Is this session in a final state
    pub is_final: bool,
    /// Session start time
    pub started_at: u64,
    /// Last activity time
    pub last_activity: u64,
}

impl LocalSessionRuntime {
    /// Create a new local session runtime for a device
    pub fn new(device_id: DeviceId, account_id: AccountId) -> Self {
        let (command_sender, _command_receiver) = mpsc::unbounded_channel();
        let (event_sender, _event_receiver) = mpsc::unbounded_channel();
        let (effect_sender, _effect_receiver) = mpsc::unbounded_channel();

        Self {
            device_id,
            account_id,
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
            command_sender,
            event_sender,
            effect_sender,
            channels: ChannelRegistry::new(),
        }
    }

    /// Start the session runtime event loop
    pub async fn run(&self) -> Result<(), RuntimeError> {
        info!(
            "Starting local session runtime for device {}",
            self.device_id
        );

        // Create channels for the main event loop
        let (command_tx, mut command_rx) = mpsc::unbounded_channel::<SessionCommand>();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<SessionEvent>();
        let (effect_tx, mut effect_rx) = mpsc::unbounded_channel::<SessionEffect>();

        // Store senders for external use
        let mut command_sender = self.command_sender.clone();
        let mut event_sender = self.event_sender.clone();
        let mut effect_sender = self.effect_sender.clone();

        // Main event loop
        loop {
            tokio::select! {
                // Handle incoming commands
                Some(command) = command_rx.recv() => {
                    debug!("Processing command: {:?}", command);
                    
                    match self.handle_command(command, &event_tx, &effect_tx).await {
                        Ok(_) => {
                            debug!("Command processed successfully");
                        }
                        Err(e) => {
                            error!("Error processing command: {}", e);
                            // Emit error event
                            let error_event = SessionEvent::RuntimeError {
                                session_id: None,
                                error: e.to_string(),
                                recoverable: true,
                            };
                            let _ = event_tx.send(error_event);
                        }
                    }
                }
                
                // Handle protocol events
                Some(event) = event_rx.recv() => {
                    debug!("Processing event: {:?}", event);
                    
                    match self.handle_event(event.clone(), &effect_tx).await {
                        Ok(_) => {
                            debug!("Event processed successfully");
                        }
                        Err(e) => {
                            error!("Error processing event: {}", e);
                        }
                    }
                    
                    // Forward event to external listeners
                    let _ = event_sender.send(event);
                }
                
                // Handle effects
                Some(effect) = effect_rx.recv() => {
                    debug!("Processing effect: {:?}", effect);
                    
                    match self.handle_effect(effect, &event_tx).await {
                        Ok(_) => {
                            debug!("Effect processed successfully");
                        }
                        Err(e) => {
                            error!("Error processing effect: {}", e);
                            // Emit error event
                            let error_event = SessionEvent::RuntimeError {
                                session_id: None,
                                error: e.to_string(),
                                recoverable: false,
                            };
                            let _ = event_tx.send(error_event);
                        }
                    }
                }
                
                // Handle shutdown or break condition
                else => {
                    info!("Event loop shutting down");
                    break;
                }
            }
        }

        info!("Local session runtime stopped for device {}", self.device_id);
        Ok(())
    }

    /// Handle incoming commands
    async fn handle_command(
        &self,
        command: SessionCommand,
        event_tx: &mpsc::UnboundedSender<SessionEvent>,
        effect_tx: &mpsc::UnboundedSender<SessionEffect>,
    ) -> Result<(), RuntimeError> {
        match command {
            SessionCommand::StartProtocol { protocol_type, session_id, config } => {
                self.handle_start_protocol(protocol_type, session_id, config, event_tx).await
            }
            
            SessionCommand::SendToPeers { session_id, recipients, message } => {
                self.handle_send_to_peers(session_id, recipients, message, effect_tx).await
            }
            
            SessionCommand::ProcessEvent { session_id, event } => {
                self.handle_process_event(session_id, event, event_tx, effect_tx).await
            }
            
            SessionCommand::RequestTransition { session_id, target_state, witness_data } => {
                self.handle_request_transition(session_id, target_state, witness_data, event_tx).await
            }
            
            SessionCommand::AbortProtocol { session_id, reason } => {
                self.handle_abort_protocol(session_id, reason, event_tx).await
            }
            
            SessionCommand::QueryStatus { session_id, response_channel } => {
                self.handle_query_status(session_id, response_channel).await
            }
            
            SessionCommand::Shutdown => {
                info!("Received shutdown command");
                Err(RuntimeError::RuntimeError("Shutdown requested".to_string()))
            }
        }
    }

    /// Handle protocol events
    async fn handle_event(
        &self,
        event: SessionEvent,
        effect_tx: &mpsc::UnboundedSender<SessionEffect>,
    ) -> Result<(), RuntimeError> {
        match event {
            SessionEvent::ProtocolStarted { session_id, protocol_type, initial_state } => {
                debug!("Protocol {} started in state {}", session_id, initial_state);
                Ok(())
            }
            
            SessionEvent::StateTransition { session_id, from_state, to_state, timestamp } => {
                self.handle_state_transition(session_id, from_state, to_state, timestamp).await
            }
            
            SessionEvent::MessageReceived { session_id, from_device, message } => {
                self.handle_message_received(session_id, from_device, message, effect_tx).await
            }
            
            SessionEvent::ProtocolCompleted { session_id, result } => {
                self.handle_protocol_completed(session_id, result).await
            }
            
            SessionEvent::ProtocolFailed { session_id, error, final_state } => {
                self.handle_protocol_failed(session_id, error, final_state).await
            }
            
            SessionEvent::RuntimeError { session_id, error, recoverable } => {
                warn!("Runtime error in session {:?}: {} (recoverable: {})", session_id, error, recoverable);
                Ok(())
            }
        }
    }

    /// Handle effects
    async fn handle_effect(
        &self,
        effect: SessionEffect,
        event_tx: &mpsc::UnboundedSender<SessionEvent>,
    ) -> Result<(), RuntimeError> {
        match effect {
            SessionEffect::WriteEvent { event, callback } => {
                // TODO: Implement actual journal writing
                debug!("Writing event to journal: {:?}", event);
                if let Some(callback) = callback {
                    let _ = callback.send(Ok(()));
                }
                Ok(())
            }
            
            SessionEffect::SendMessage { recipients, message, callback } => {
                // TODO: Implement actual message sending via transport
                debug!("Sending message to {:?}: {} bytes", recipients, message.len());
                if let Some(callback) = callback {
                    let _ = callback.send(Ok(()));
                }
                Ok(())
            }
            
            SessionEffect::StoreState { session_id, state_data, callback } => {
                // TODO: Implement state persistence
                debug!("Storing state for session {}: {} bytes", session_id, state_data.len());
                if let Some(callback) = callback {
                    let _ = callback.send(Ok(()));
                }
                Ok(())
            }
            
            SessionEffect::LoadState { session_id, callback } => {
                // TODO: Implement state loading
                debug!("Loading state for session {}", session_id);
                let _ = callback.send(Err("State loading not implemented".to_string()));
                Ok(())
            }
            
            SessionEffect::TriggerRehydration { session_id, from_epoch, callback } => {
                self.handle_rehydration(session_id, from_epoch, callback).await
            }
            
            SessionEffect::ScheduleAction { delay_ms, action } => {
                self.handle_schedule_action(delay_ms, *action, event_tx).await
            }
            
            SessionEffect::NotifyExternal { notification_type, data } => {
                // TODO: Implement external notifications
                debug!("External notification {}: {} bytes", notification_type, data.len());
                Ok(())
            }
        }
    }

    /// Create a new DKD protocol session
    pub async fn start_dkd_session(
        &self,
        app_id: String,
        context: String,
    ) -> Result<Uuid, RuntimeError> {
        let session_id = Uuid::new_v4();
        debug!(
            "Starting DKD session {} for app_id='{}', context='{}'",
            session_id, app_id, context
        );

        // Create DKD protocol in initial state
        let dkd_protocol = new_dkd_protocol(self.device_id, app_id.clone(), context.clone())
            .map_err(|e| RuntimeError::ProtocolError(e))?;
        
        // Wrap in state enum
        let dkd_state = DkdProtocolState::InitializationPhase(dkd_protocol);

        // Register session
        let session = ActiveSession {
            session_id,
            protocol_type: SessionProtocolType::DKD,
            current_state: dkd_state.state_name().to_string(),
            can_terminate: dkd_state.can_terminate(),
            is_final: false, // Will be true in CompletionPhase or Failure states
            started_at: 0,    // TODO: Use effects for timestamp
            last_activity: 0, // TODO: Use effects for timestamp
        };

        let mut sessions = self.active_sessions.write().await;
        sessions.insert(session_id, session);

        info!(
            "Started DKD session {} in state {}",
            session_id,
            dkd_protocol.current_state_name()
        );
        Ok(session_id)
    }

    /// Create a new recovery protocol session
    pub async fn start_recovery_session(
        &self,
        guardian_threshold: usize,
        cooldown_period: u64,
    ) -> Result<Uuid, RuntimeError> {
        let session_id = Uuid::new_v4();
        debug!(
            "Starting recovery session {} with threshold={}, cooldown={}",
            session_id, guardian_threshold, cooldown_period
        );

        // Create recovery protocol in initial state
        let recovery_protocol = new_session_typed_recovery(
            Uuid::new_v4(),
            self.device_id,
            vec![], // TODO: Provide actual guardian IDs
            guardian_threshold as u16,
            Some(cooldown_period),
        );

        // Register session
        let session = ActiveSession {
            session_id,
            protocol_type: SessionProtocolType::Recovery,
            current_state: recovery_protocol.current_state_name().to_string(),
            can_terminate: recovery_protocol.can_terminate(),
            is_final: recovery_protocol.is_final(),
            started_at: 0,    // TODO: Use effects for timestamp
            last_activity: 0, // TODO: Use effects for timestamp
        };

        let mut sessions = self.active_sessions.write().await;
        sessions.insert(session_id, session);

        info!(
            "Started recovery session {} in state {}",
            session_id,
            recovery_protocol.current_state_name()
        );
        Ok(session_id)
    }

    /// Create a new agent session
    pub async fn start_agent_session(&self) -> Result<Uuid, RuntimeError> {
        let session_id = Uuid::new_v4();
        debug!("Starting agent session {}", session_id);

        // Create agent protocol in initial state
        let agent_protocol = new_session_typed_agent(self.device_id);

        // Register session
        let session = ActiveSession {
            session_id,
            protocol_type: SessionProtocolType::Agent,
            current_state: agent_protocol.current_state_name().to_string(),
            can_terminate: agent_protocol.can_terminate(),
            is_final: agent_protocol.is_final(),
            started_at: 0,    // TODO: Use effects for timestamp
            last_activity: 0, // TODO: Use effects for timestamp
        };

        let mut sessions = self.active_sessions.write().await;
        sessions.insert(session_id, session);

        info!(
            "Started agent session {} in state {}",
            session_id,
            agent_protocol.current_state_name()
        );
        Ok(session_id)
    }

    /// Get status of active sessions
    pub async fn get_session_status(&self) -> Vec<SessionStatus> {
        let sessions = self.active_sessions.read().await;
        sessions
            .values()
            .map(|session| SessionStatus {
                session_id: session.session_id,
                protocol_type: session.protocol_type.clone(),
                current_state: session.current_state.clone(),
                can_terminate: session.can_terminate,
                is_final: session.is_final,
                started_at: session.started_at,
                last_activity: session.last_activity,
            })
            .collect()
    }

    /// Terminate a session
    pub async fn terminate_session(&self, session_id: Uuid) -> Result<(), RuntimeError> {
        let mut sessions = self.active_sessions.write().await;

        if let Some(session) = sessions.get(&session_id) {
            if !session.can_terminate {
                return Err(RuntimeError::SessionNotTerminable(format!(
                    "Session {} in state {} cannot be terminated",
                    session_id, session.current_state
                )));
            }

            sessions.remove(&session_id);
            info!("Terminated session {}", session_id);
            Ok(())
        } else {
            Err(RuntimeError::SessionNotFound(session_id))
        }
    }

    /// Send a command to the runtime
    pub async fn send_command(&self, command: SessionCommand) -> Result<(), RuntimeError> {
        self.command_sender
            .send(command)
            .map_err(|_| RuntimeError::ChannelClosed("Command channel closed".to_string()))?;
        Ok(())
    }

    /// Send an event to the runtime
    pub async fn send_event(&self, event: SessionEvent) -> Result<(), RuntimeError> {
        self.event_sender
            .send(event)
            .map_err(|_| RuntimeError::ChannelClosed("Event channel closed".to_string()))?;
        Ok(())
    }

    /// Send an effect from the runtime
    pub async fn send_effect(&self, effect: SessionEffect) -> Result<(), RuntimeError> {
        self.effect_sender
            .send(effect)
            .map_err(|_| RuntimeError::ChannelClosed("Effect channel closed".to_string()))?;
        Ok(())
    }

    /// Get channel registry for protocol communication
    pub fn channels(&self) -> &ChannelRegistry {
        &self.channels
    }

    // ========== Specific Command Handlers ==========

    async fn handle_start_protocol(
        &self,
        protocol_type: SessionProtocolType,
        session_id: Uuid,
        config: ProtocolConfig,
        event_tx: &mpsc::UnboundedSender<SessionEvent>,
    ) -> Result<(), RuntimeError> {
        debug!("Starting protocol {:?} with session {}", protocol_type, session_id);

        // Create protocol based on type
        let (current_state, can_terminate, is_final) = match protocol_type {
            SessionProtocolType::DKD => {
                // Extract app_id and context from config if available
                let app_id = config.parameters.get("app_id").unwrap_or(&"default".to_string()).clone();
                let context = config.parameters.get("context").unwrap_or(&"default".to_string()).clone();
                
                let protocol = new_dkd_protocol(self.device_id, app_id, context)
                    .map_err(|e| RuntimeError::ProtocolError(e))?;
                let state = DkdProtocolState::InitializationPhase(protocol);
                (state.state_name().to_string(), state.can_terminate(), false)
            }
            SessionProtocolType::Recovery => {
                let protocol = rehydrate_recovery_session(&[]).unwrap_or_else(|| {
                    // Create new recovery session as fallback
                    RecoverySessionState::RecoveryInitialized(new_session_typed_recovery(
                        Uuid::new_v4(),
                        self.device_id,
                        vec![],                   // TODO: Use actual guardians
                        2,                        // TODO: Use actual threshold
                        Some(48),                 // TODO: Use actual cooldown
                    ))
                });
                (protocol.state_name().to_string(), protocol.can_terminate(), false)
            }
            SessionProtocolType::Agent => {
                let protocol = rehydrate_agent_session(&[]).unwrap_or_else(|| {
                    AgentSessionState::AgentIdle(new_session_typed_agent(self.device_id))
                });
                (protocol.state_name().to_string(), protocol.can_terminate(), false)
            }
            _ => {
                return Err(RuntimeError::ProtocolError(format!("Unsupported protocol type: {:?}", protocol_type)));
            }
        };

        // Register the session
        let session = ActiveSession {
            session_id,
            protocol_type: protocol_type.clone(),
            current_state: current_state.clone(),
            can_terminate,
            is_final,
            started_at: 0, // TODO: Use effects for timestamp
            last_activity: 0, // TODO: Use effects for timestamp
        };

        let mut sessions = self.active_sessions.write().await;
        sessions.insert(session_id, session);

        // Emit protocol started event
        let event = SessionEvent::ProtocolStarted {
            session_id,
            protocol_type,
            initial_state: current_state,
        };
        let _ = event_tx.send(event);

        Ok(())
    }

    async fn handle_send_to_peers(
        &self,
        session_id: Uuid,
        recipients: Vec<DeviceId>,
        message: Vec<u8>,
        effect_tx: &mpsc::UnboundedSender<SessionEffect>,
    ) -> Result<(), RuntimeError> {
        // Verify session exists
        {
            let sessions = self.active_sessions.read().await;
            if !sessions.contains_key(&session_id) {
                return Err(RuntimeError::SessionNotFound(session_id));
            }
        }

        // Create send message effect
        let effect = SessionEffect::SendMessage {
            recipients,
            message,
            callback: None,
        };
        let _ = effect_tx.send(effect);

        Ok(())
    }

    async fn handle_process_event(
        &self,
        session_id: Uuid,
        event: Event,
        event_tx: &mpsc::UnboundedSender<SessionEvent>,
        effect_tx: &mpsc::UnboundedSender<SessionEffect>,
    ) -> Result<(), RuntimeError> {
        // Process the journal event in the context of the session
        debug!("Processing journal event for session {}: {:?}", session_id, event);
        
        // TODO: Implement actual event processing based on session state
        // This would involve state transitions, validation, etc.
        
        Ok(())
    }

    async fn handle_request_transition(
        &self,
        session_id: Uuid,
        target_state: String,
        witness_data: Vec<u8>,
        event_tx: &mpsc::UnboundedSender<SessionEvent>,
    ) -> Result<(), RuntimeError> {
        let mut sessions = self.active_sessions.write().await;
        
        if let Some(session) = sessions.get_mut(&session_id) {
            let from_state = session.current_state.clone();
            
            // TODO: Validate transition and witness data
            session.current_state = target_state.clone();
            session.last_activity = 0; // TODO: Use effects for timestamp

            // Emit state transition event
            let event = SessionEvent::StateTransition {
                session_id,
                from_state,
                to_state: target_state,
                timestamp: 0, // TODO: Use effects for timestamp
            };
            let _ = event_tx.send(event);

            Ok(())
        } else {
            Err(RuntimeError::SessionNotFound(session_id))
        }
    }

    async fn handle_abort_protocol(
        &self,
        session_id: Uuid,
        reason: String,
        event_tx: &mpsc::UnboundedSender<SessionEvent>,
    ) -> Result<(), RuntimeError> {
        let mut sessions = self.active_sessions.write().await;
        
        if let Some(session) = sessions.remove(&session_id) {
            // Emit protocol failed event
            let event = SessionEvent::ProtocolFailed {
                session_id,
                error: reason,
                final_state: Some(session.current_state),
            };
            let _ = event_tx.send(event);

            Ok(())
        } else {
            Err(RuntimeError::SessionNotFound(session_id))
        }
    }

    async fn handle_query_status(
        &self,
        session_id: Uuid,
        response_channel: ResponseChannel<ProtocolStatus>,
    ) -> Result<(), RuntimeError> {
        let sessions = self.active_sessions.read().await;
        
        if let Some(session) = sessions.get(&session_id) {
            let status = ProtocolStatus {
                current_state: session.current_state.clone(),
                can_terminate: session.can_terminate,
                is_final: session.is_final,
                progress: 0.5, // TODO: Calculate actual progress
                last_activity: session.last_activity,
                participants: vec![], // TODO: Get actual participants
            };

            let _ = response_channel.send(status);
            Ok(())
        } else {
            Err(RuntimeError::SessionNotFound(session_id))
        }
    }

    // ========== Specific Event Handlers ==========

    async fn handle_state_transition(
        &self,
        session_id: Uuid,
        from_state: String,
        to_state: String,
        timestamp: u64,
    ) -> Result<(), RuntimeError> {
        let mut sessions = self.active_sessions.write().await;
        
        if let Some(session) = sessions.get_mut(&session_id) {
            session.current_state = to_state;
            session.last_activity = timestamp;
            debug!("Session {} transitioned from {} to {}", session_id, from_state, session.current_state);
        }

        Ok(())
    }

    async fn handle_message_received(
        &self,
        session_id: Uuid,
        from_device: DeviceId,
        message: Vec<u8>,
        effect_tx: &mpsc::UnboundedSender<SessionEffect>,
    ) -> Result<(), RuntimeError> {
        debug!("Received message for session {} from device {}: {} bytes", session_id, from_device, message.len());
        
        // TODO: Process message based on session state and protocol
        // This would involve protocol-specific message handling
        
        Ok(())
    }

    async fn handle_protocol_completed(
        &self,
        session_id: Uuid,
        result: SessionProtocolResult,
    ) -> Result<(), RuntimeError> {
        let mut sessions = self.active_sessions.write().await;
        
        if let Some(session) = sessions.get_mut(&session_id) {
            session.is_final = true;
            debug!("Protocol {} completed with result type: {}", session_id, result.result_type);
        }

        Ok(())
    }

    async fn handle_protocol_failed(
        &self,
        session_id: Uuid,
        error: String,
        final_state: Option<String>,
    ) -> Result<(), RuntimeError> {
        let mut sessions = self.active_sessions.write().await;
        
        if let Some(session) = sessions.remove(&session_id) {
            error!("Protocol {} failed: {} (final state: {:?})", session_id, error, final_state);
        }

        Ok(())
    }

    // ========== Specific Effect Handlers ==========

    async fn handle_rehydration(
        &self,
        session_id: Uuid,
        from_epoch: u64,
        callback: ResponseChannel<Result<String, String>>,
    ) -> Result<(), RuntimeError> {
        debug!("Triggering rehydration for session {} from epoch {}", session_id, from_epoch);
        
        // TODO: Implement actual rehydration logic
        let _ = callback.send(Err("Rehydration not implemented".to_string()));
        
        Ok(())
    }

    async fn handle_schedule_action(
        &self,
        delay_ms: u64,
        action: SessionCommand,
        event_tx: &mpsc::UnboundedSender<SessionEvent>,
    ) -> Result<(), RuntimeError> {
        debug!("Scheduling action with delay {}ms: {:?}", delay_ms, action);
        
        // TODO: Implement proper action scheduling
        // For now, just execute immediately
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
            debug!("Executing scheduled action: {:?}", action);
            // TODO: Send the action back to the command handler
        });
        
        Ok(())
    }

    /// Rehydrate sessions from journal evidence after restart
    pub async fn rehydrate_from_journal(
        &self,
        session_evidence: Vec<(Uuid, SessionProtocolType, Vec<u8>)>,
    ) -> Result<usize, RuntimeError> {
        let mut rehydrated_count = 0;
        let mut sessions = self.active_sessions.write().await;

        for (session_id, protocol_type, evidence) in session_evidence {
            debug!(
                "Rehydrating session {} of type {:?}",
                session_id, protocol_type
            );

            // TODO: Implement protocol-specific rehydration
            match protocol_type {
                SessionProtocolType::DKD => {
                    if let Some(dkd_protocol) = rehydrate_dkd_protocol(&evidence) {
                        let session = ActiveSession {
                            session_id,
                            protocol_type,
                            current_state: dkd_protocol.state_name().to_string(),
                            can_terminate: dkd_protocol.can_terminate(),
                            is_final: false, // DkdProtocolState doesn't have is_final method
                            started_at: 0,    // TODO: Extract from evidence
                            last_activity: 0, // TODO: Extract from evidence
                        };
                        sessions.insert(session_id, session);
                        rehydrated_count += 1;
                    }
                }
                SessionProtocolType::Recovery => {
                    if let Some(recovery_protocol) = rehydrate_recovery_session(&evidence) {
                        let session = ActiveSession {
                            session_id,
                            protocol_type,
                            current_state: recovery_protocol.state_name().to_string(),
                            can_terminate: recovery_protocol.can_terminate(),
                            is_final: recovery_protocol.is_final(),
                            started_at: 0,    // TODO: Extract from evidence
                            last_activity: 0, // TODO: Extract from evidence
                        };
                        sessions.insert(session_id, session);
                        rehydrated_count += 1;
                    }
                }
                SessionProtocolType::Agent => {
                    if let Some(agent_protocol) = rehydrate_agent_session(&evidence) {
                        let session = ActiveSession {
                            session_id,
                            protocol_type,
                            current_state: agent_protocol.state_name().to_string(),
                            can_terminate: agent_protocol.can_terminate(),
                            is_final: agent_protocol.is_final(),
                            started_at: 0,    // TODO: Extract from evidence
                            last_activity: 0, // TODO: Extract from evidence
                        };
                        sessions.insert(session_id, session);
                        rehydrated_count += 1;
                    }
                }
                _ => {
                    warn!(
                        "Unsupported protocol type for rehydration: {:?}",
                        protocol_type
                    );
                }
            }
        }

        info!(
            "Rehydrated {} sessions from journal evidence",
            rehydrated_count
        );
        Ok(rehydrated_count)
    }
}

/// Session status information
#[derive(Debug, Clone)]
pub struct SessionStatus {
    pub session_id: Uuid,
    pub protocol_type: SessionProtocolType,
    pub current_state: String,
    pub can_terminate: bool,
    pub is_final: bool,
    pub started_at: u64,
    pub last_activity: u64,
}

/// Errors that can occur in the local session runtime
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("Session not found: {0}")]
    SessionNotFound(Uuid),

    #[error("Session not terminable: {0}")]
    SessionNotTerminable(String),

    #[error("Channel closed: {0}")]
    ChannelClosed(String),

    #[error("Protocol error: {0}")]
    ProtocolError(String),

    #[error("Rehydration error: {0}")]
    RehydrationError(String),

    #[error("Session runtime error: {0}")]
    RuntimeError(String),
}

// Helper functions for rehydration (placeholder implementations)
fn rehydrate_dkd_protocol(_evidence: &[u8]) -> Option<DkdProtocolState> {
    // TODO: Implement actual rehydration from journal evidence
    // TODO: Implement actual rehydration from journal evidence
    None
}

fn rehydrate_recovery_session(_evidence: &[u8]) -> Option<RecoverySessionState> {
    // TODO: Implement actual rehydration from journal evidence
    let protocol = new_session_typed_recovery(
        Uuid::new_v4(),
        DeviceId(Uuid::new_v4()), // TODO: Use actual device ID
        vec![],                   // TODO: Use actual guardians
        2,                        // TODO: Use actual threshold
        Some(48),                 // TODO: Use actual cooldown
    );
    Some(RecoverySessionState::RecoveryInitialized(protocol))
}

fn rehydrate_agent_session(_evidence: &[u8]) -> Option<AgentSessionState> {
    // TODO: Implement actual rehydration from journal evidence
    let protocol = new_session_typed_agent(DeviceId(Uuid::new_v4())); // TODO: Use actual device ID
    Some(AgentSessionState::AgentIdle(protocol))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_crypto::Effects;

    #[tokio::test]
    async fn test_runtime_creation() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);
        let account_id = AccountId::new_with_effects(&effects);

        let runtime = LocalSessionRuntime::new(device_id, account_id);

        // Should start with no active sessions
        let status = runtime.get_session_status().await;
        assert!(status.is_empty());
    }

    #[tokio::test]
    async fn test_dkd_session_lifecycle() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);
        let account_id = AccountId::new_with_effects(&effects);

        let runtime = LocalSessionRuntime::new(device_id, account_id);

        // Start a DKD session
        let session_id = runtime
            .start_dkd_session("test-app".to_string(), "test-context".to_string())
            .await
            .unwrap();

        // Should have one active session
        let status = runtime.get_session_status().await;
        assert_eq!(status.len(), 1);
        assert_eq!(status[0].session_id, session_id);
        assert_eq!(status[0].protocol_type, SessionProtocolType::DKD);

        // Terminate the session
        runtime.terminate_session(session_id).await.unwrap();

        // Should have no active sessions
        let status = runtime.get_session_status().await;
        assert!(status.is_empty());
    }

    #[tokio::test]
    async fn test_multiple_concurrent_sessions() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);
        let account_id = AccountId::new_with_effects(&effects);

        let runtime = LocalSessionRuntime::new(device_id, account_id);

        // Start multiple sessions
        let dkd_session = runtime
            .start_dkd_session("test-app".to_string(), "test-context".to_string())
            .await
            .unwrap();

        let recovery_session = runtime.start_recovery_session(3, 3600).await.unwrap();
        let agent_session = runtime.start_agent_session().await.unwrap();

        // Should have three active sessions
        let status = runtime.get_session_status().await;
        assert_eq!(status.len(), 3);

        // Verify each session type
        let session_types: std::collections::HashSet<_> =
            status.iter().map(|s| s.protocol_type.clone()).collect();

        assert!(session_types.contains(&SessionProtocolType::DKD));
        assert!(session_types.contains(&SessionProtocolType::Recovery));
        assert!(session_types.contains(&SessionProtocolType::Agent));
    }

    #[tokio::test]
    async fn test_event_loop_basic() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);
        let account_id = AccountId::new_with_effects(&effects);

        let runtime = LocalSessionRuntime::new(device_id, account_id);

        // Start the event loop in a background task
        let runtime_clone = runtime.clone();
        let event_loop_handle = tokio::spawn(async move {
            // Run for a short time then exit
            tokio::time::timeout(
                tokio::time::Duration::from_millis(100),
                runtime_clone.run()
            ).await
        });

        // Send a command
        let command = SessionCommand::StartProtocol {
            protocol_type: SessionProtocolType::DKD,
            session_id: Uuid::new_v4(),
            config: ProtocolConfig {
                parameters: std::collections::HashMap::new(),
                participants: vec![],
                timeout_epochs: None,
                context: vec![],
            },
        };

        let _ = runtime.send_command(command).await;

        // Wait for event loop to process or timeout
        let _result = event_loop_handle.await;

        // The event loop should have processed the command
        // In a real implementation, we'd check the session was created
    }

    #[tokio::test]
    async fn test_session_not_found_error() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);
        let account_id = AccountId::new_with_effects(&effects);

        let runtime = LocalSessionRuntime::new(device_id, account_id);

        // Try to terminate non-existent session
        let fake_id = Uuid::new_v4();
        let result = runtime.terminate_session(fake_id).await;

        assert!(matches!(result, Err(RuntimeError::SessionNotFound(_))));
    }
}

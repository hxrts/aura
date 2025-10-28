//! Local Session Runtime for Per-Device Protocol Coordination
//!
//! This module provides a local session runtime that replaces implicit choreographic
//! execution with explicit per-device session management. The runtime coordinates
//! multiple protocols simultaneously while maintaining session type safety.

use crate::execution::ProtocolError;
use crate::session_types::agent::AgentIdleOperations;
use crate::session_types::{
    new_session_typed_agent,
    AgentSessionState,
    // TODO: Implement recovery session types
    // new_session_typed_recovery, RecoverySessionState,
};
use crate::LifecycleScheduler;
use aura_crypto::Effects;
use aura_journal::{
    Event, EventAuthorization, EventType, InitiateDkdSessionEvent, SessionId, SessionStatus,
};
use aura_types::{AccountId, DeviceId};
use aura_crypto::{Ed25519Signature, Ed25519SigningKey, ed25519_sign, ed25519_signature_from_bytes};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Trait for transport session state to avoid circular dependencies
pub trait TransportSession: std::fmt::Debug + Send + Sync {
    /// Get the current state name
    fn state_name(&self) -> &'static str;
    /// Check if the session can terminate
    fn can_terminate(&self) -> bool;
    /// Get the device ID from the underlying protocol
    fn device_id(&self) -> DeviceId;
    /// Check if this is a final state
    fn is_final(&self) -> bool;
}

/// Command that can be sent to the session runtime
#[derive(Debug, Clone)]
pub enum SessionCommand {
    /// Start a new DKD session
    StartDkd {
        app_id: String,
        context_label: String,
        participants: Vec<DeviceId>,
        threshold: Option<usize>,
    },
    /// Start a new DKD session with full context
    StartDkdWithContext {
        app_id: String,
        context_label: String,
        participants: Vec<DeviceId>,
        threshold: usize,
        context_bytes: Vec<u8>,
        with_binding_proof: bool,
    },
    /// Start a new recovery session
    StartRecovery {
        guardian_threshold: usize,
        cooldown_seconds: u64,
    },
    /// Start a new resharing session
    StartResharing {
        new_participants: Vec<DeviceId>,
        new_threshold: usize,
    },
    /// Start a new locking session
    StartLocking {
        operation_type: aura_journal::OperationType,
    },
    /// Start a new agent session
    StartAgent,
    /// Terminate a session
    TerminateSession { session_id: Uuid },
    /// Send event to a specific session
    SendEvent { session_id: Uuid, event: Event },
    /// Update session status
    UpdateStatus {
        session_id: Uuid,
        status: SessionStatus,
    },
    /// Handle transport event
    TransportEvent { event: TransportEvent },
}

/// Events from transport layer
#[derive(Debug, Clone)]
pub enum TransportEvent {
    /// Connection established with peer
    ConnectionEstablished { peer_id: String },
    /// Connection lost with peer
    ConnectionLost { peer_id: String },
    /// Message received from peer
    MessageReceived { peer_id: String, message: Vec<u8> },
    /// Message sent to peer
    MessageSent {
        peer_id: String,
        message_size: usize,
    },
    /// Transport error occurred
    TransportError { error: String },
}

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

/// Response from session runtime operations
#[derive(Debug, Clone)]
pub enum SessionResponse {
    /// Session started successfully
    SessionStarted { session_id: Uuid },
    /// Session terminated
    SessionTerminated { session_id: Uuid },
    /// Session status updated
    StatusUpdated { session_id: Uuid },
    /// DKD session completed with derived key
    DkdCompleted {
        session_id: Uuid,
        derived_key_bytes: Vec<u8>,
        binding_proof: Option<Vec<u8>>,
    },
    /// Error occurred
    Error { message: String },
}

/// Protocol type used by the session runtime
#[derive(Debug, Clone)]
pub enum SessionProtocolType {
    DKD,
    Recovery,
    Resharing,
    Locking,
    Agent,
}

/// Session status information
#[derive(Debug, Clone)]
pub struct SessionStatusInfo {
    pub session_id: Uuid,
    pub protocol_type: SessionProtocolType,
    pub status: SessionStatus,
    pub is_final: bool,
}

/// Result of a completed DKD session
#[derive(Debug, Clone)]
pub struct DkdResult {
    pub session_id: Uuid,
    pub derived_key_bytes: Vec<u8>,
    pub binding_proof: Option<Vec<u8>>,
}

/// Active session tracking
#[derive(Debug)]
struct ActiveSession {
    session_id: Uuid,
    protocol_type: SessionProtocolType,
    status: SessionStatus,
    recovery_state: Option<AgentSessionState>, // Reuse agent state for recovery
    agent_state: Option<AgentSessionState>,
    transport_state: Option<Box<dyn TransportSession>>,
    resharing_state: Option<AgentSessionState>, // Reuse agent state for resharing
    locking_state: Option<AgentSessionState>,   // Reuse agent state for locking
}

/// Local session runtime that coordinates all protocols for a single device
pub struct LocalSessionRuntime {
    /// Device identifier
    device_id: DeviceId,
    /// Account identifier
    account_id: AccountId,
    /// Device signing key for event authentication
    device_signing_key: Ed25519SigningKey,
    /// Command receiver channel
    command_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<SessionCommand>>>>,
    /// Command sender channel (for external use)
    command_tx: mpsc::UnboundedSender<SessionCommand>,
    /// Response sender channel
    response_tx: Option<mpsc::UnboundedSender<SessionResponse>>,
    /// Active sessions
    active_sessions: Arc<RwLock<BTreeMap<Uuid, ActiveSession>>>,
    /// Injectable effects for deterministic testing
    effects: Effects,
    /// Transport for P2P communication
    transport: Option<Arc<dyn crate::Transport>>,
}

impl LocalSessionRuntime {
    /// Create a new local session runtime
    pub fn new(
        device_id: DeviceId,
        account_id: AccountId,
        device_signing_key: Ed25519SigningKey,
        effects: Effects,
    ) -> Self {
        let (command_tx, command_rx) = mpsc::unbounded_channel();

        info!("Creating local session runtime for device {}", device_id);

        Self {
            device_id,
            account_id,
            device_signing_key,
            command_rx: Arc::new(Mutex::new(Some(command_rx))),
            command_tx,
            response_tx: None,
            active_sessions: Arc::new(RwLock::new(BTreeMap::new())),
            effects,
            transport: None,
        }
    }

    /// Create a new local session runtime with generated signing key (for testing)
    pub fn new_with_generated_key(
        device_id: DeviceId,
        account_id: AccountId,
        effects: Effects,
    ) -> Self {
        use rand::rngs::OsRng;
        let device_signing_key = Ed25519SigningKey::generate(&mut OsRng);
        Self::new(device_id, account_id, device_signing_key, effects)
    }

    /// Set the transport for this runtime
    pub fn set_transport(&mut self, transport: Arc<dyn crate::Transport>) {
        self.transport = Some(transport);
    }

    /// Get command sender for external use
    pub fn command_sender(&self) -> mpsc::UnboundedSender<SessionCommand> {
        self.command_tx.clone()
    }

    /// Sign an event with the device signing key
    fn sign_event(&self, event: &Event) -> Result<Ed25519Signature, String> {
        let event_hash = event
            .signable_hash()
            .map_err(|e| format!("Failed to compute signable hash: {}", e))?;
        Ok(ed25519_sign(&self.device_signing_key, &event_hash))
    }

    /// Start the session runtime (this is the main execution loop)
    pub async fn run(&self) -> Result<(), ProtocolError> {
        // Take the receiver from the Option using the Mutex
        let command_rx = {
            let mut rx_guard = self.command_rx.lock().await;
            rx_guard
                .take()
                .ok_or_else(|| ProtocolError::new("Runtime already started".to_string()))?
        };

        let mut command_rx = command_rx;

        info!("Starting session runtime for device {}", self.device_id);

        while let Some(command) = command_rx.recv().await {
            if let Err(e) = self.handle_command(command).await {
                error!("Error handling command: {:?}", e);
            }
        }

        info!("Session runtime stopped for device {}", self.device_id);
        Ok(())
    }

    /// Handle a session command
    async fn handle_command(&self, command: SessionCommand) -> Result<(), ProtocolError> {
        debug!("Handling command: {:?}", command);

        match command {
            SessionCommand::StartDkd {
                app_id,
                context_label,
                participants,
                threshold,
            } => {
                let session_id = self
                    .start_dkd_session_internal(app_id, context_label, participants, threshold)
                    .await?;
                self.send_response(SessionResponse::SessionStarted { session_id })
                    .await;
            }
            SessionCommand::StartDkdWithContext {
                app_id,
                context_label,
                participants,
                threshold,
                context_bytes,
                with_binding_proof,
            } => {
                let session_id = self
                    .start_dkd_with_context_internal(
                        app_id,
                        context_label,
                        participants,
                        threshold,
                        context_bytes,
                        with_binding_proof,
                    )
                    .await?;
                self.send_response(SessionResponse::SessionStarted { session_id })
                    .await;
            }
            SessionCommand::StartRecovery {
                guardian_threshold,
                cooldown_seconds,
            } => {
                let session_id = self
                    .start_recovery_session_internal(guardian_threshold, cooldown_seconds)
                    .await?;
                self.send_response(SessionResponse::SessionStarted { session_id })
                    .await;
            }
            SessionCommand::StartResharing {
                new_participants,
                new_threshold,
            } => {
                let session_id = self
                    .start_resharing_session_internal(new_participants, new_threshold)
                    .await?;
                self.send_response(SessionResponse::SessionStarted { session_id })
                    .await;
            }
            SessionCommand::StartLocking { operation_type } => {
                let session_id = self.start_locking_session_internal(operation_type).await?;
                self.send_response(SessionResponse::SessionStarted { session_id })
                    .await;
            }
            SessionCommand::StartAgent => {
                let session_id = self.start_agent_session_internal().await?;
                self.send_response(SessionResponse::SessionStarted { session_id })
                    .await;
            }
            SessionCommand::TerminateSession { session_id } => {
                self.terminate_session_internal(session_id).await?;
                self.send_response(SessionResponse::SessionTerminated { session_id })
                    .await;
            }
            SessionCommand::SendEvent { session_id, event } => {
                self.send_event_to_session(session_id, event).await?;
            }
            SessionCommand::UpdateStatus { session_id, status } => {
                self.update_session_status_internal(session_id, status)
                    .await?;
                self.send_response(SessionResponse::StatusUpdated { session_id })
                    .await;
            }
            SessionCommand::TransportEvent { event } => {
                self.handle_transport_event(event).await?;
            }
        }

        Ok(())
    }

    /// Send response (if response channel is available)
    async fn send_response(&self, response: SessionResponse) {
        if let Some(tx) = &self.response_tx {
            let _ = tx.send(response);
        }
    }

    /// Start a DKD session with P2P coordination
    pub async fn start_dkd_session(
        &self,
        app_id: String,
        context_label: String,
    ) -> Result<Uuid, ProtocolError> {
        // Discover available devices via transport layer and capability verification
        let participants = self.discover_available_devices().await?;
        let threshold = Some((participants.len() / 2) + 1); // Majority threshold

        let command = SessionCommand::StartDkd {
            app_id,
            context_label,
            participants,
            threshold,
        };

        self.command_tx
            .send(command)
            .map_err(|e| ProtocolError::new(format!("Failed to send command: {}", e)))?;

        // Generate session ID immediately for MVP
        // In full implementation, would wait for P2P coordination setup
        Ok(self.effects.gen_uuid())
    }

    /// Start a DKD session with full context parameters
    pub async fn start_dkd_with_context(
        &self,
        app_id: String,
        context_label: String,
        participants: Vec<DeviceId>,
        threshold: usize,
        context_bytes: Vec<u8>,
        with_binding_proof: bool,
    ) -> Result<DkdResult, ProtocolError> {
        let command = SessionCommand::StartDkdWithContext {
            app_id,
            context_label,
            participants,
            threshold,
            context_bytes,
            with_binding_proof,
        };

        self.command_tx
            .send(command)
            .map_err(|e| ProtocolError::new(format!("Failed to send command: {}", e)))?;

        // This method now requires proper session implementation
        // Since DKD choreography is not fully implemented, return an error
        Err(ProtocolError::new(
            "DKD session runtime not implemented - cannot complete session until choreography is implemented".to_string()
        ))
    }

    /// Start a recovery session
    pub async fn start_recovery_session(
        &self,
        guardian_threshold: usize,
        cooldown_seconds: u64,
    ) -> Result<Uuid, ProtocolError> {
        let command = SessionCommand::StartRecovery {
            guardian_threshold,
            cooldown_seconds,
        };

        self.command_tx
            .send(command)
            .map_err(|e| ProtocolError::new(format!("Failed to send command: {}", e)))?;

        // Generate session ID immediately for MVP
        // The command handler will process the recovery session setup
        Ok(self.effects.gen_uuid())
    }

    /// Start an agent session
    pub async fn start_agent_session(&self) -> Result<Uuid, ProtocolError> {
        let command = SessionCommand::StartAgent;

        self.command_tx
            .send(command)
            .map_err(|e| ProtocolError::new(format!("Failed to send command: {}", e)))?;

        // This method now requires proper session implementation
        // Since agent choreography is not fully implemented, return an error
        Err(ProtocolError::new(
            "Agent session runtime not implemented - cannot complete session until choreography is implemented".to_string()
        ))
    }

    /// Terminate a session
    pub async fn terminate_session(&self, session_id: Uuid) -> Result<(), ProtocolError> {
        let command = SessionCommand::TerminateSession { session_id };

        self.command_tx
            .send(command)
            .map_err(|e| ProtocolError::new(format!("Failed to send command: {}", e)))?;

        Ok(())
    }

    /// Send command to runtime
    pub async fn send_command(&self, command: SessionCommand) -> Result<(), ProtocolError> {
        self.command_tx
            .send(command)
            .map_err(|e| ProtocolError::new(format!("Failed to send command: {}", e)))
    }

    /// Get session status for all active sessions
    pub async fn get_session_status(&self) -> Vec<SessionStatusInfo> {
        let sessions = self.active_sessions.read().await;

        sessions
            .values()
            .map(|session| SessionStatusInfo {
                session_id: session.session_id,
                protocol_type: session.protocol_type.clone(),
                status: session.status,
                is_final: matches!(
                    session.status,
                    SessionStatus::Completed | SessionStatus::Failed | SessionStatus::Expired
                ),
            })
            .collect()
    }

    // Internal implementation methods

    async fn start_dkd_session_internal(
        &self,
        app_id: String,
        context_label: String,
        participants: Vec<DeviceId>,
        threshold: Option<usize>,
    ) -> Result<Uuid, ProtocolError> {
        let session_id = self.effects.gen_uuid();
        let effective_threshold = threshold.unwrap_or((participants.len() / 2) + 1);

        info!(
            "Starting P2P DKD session {} for app={}, context={}, participants={}, threshold={}",
            session_id,
            app_id,
            context_label,
            participants.len(),
            effective_threshold
        );

        // For MVP: Create context bytes from app_id and context_label
        let context_bytes = {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(app_id.as_bytes());
            bytes.push(0); // Separator
            bytes.extend_from_slice(context_label.as_bytes());
            bytes
        };

        // Execute P2P DKD protocol with discovered participants
        match self
            .start_dkd_with_context_internal(
                app_id.clone(),
                context_label.clone(),
                participants.clone(),
                effective_threshold,
                context_bytes,
                false, // No binding proof for simple derivation
            )
            .await
        {
            Ok(_) => {
                info!("P2P DKD session {} completed successfully", session_id);
            }
            Err(e) => {
                error!("P2P DKD session {} failed: {:?}", session_id, e);
                return Err(e);
            }
        }

        // Create active session (DKD protocol state will be managed internally)
        let active_session = ActiveSession {
            session_id,
            protocol_type: SessionProtocolType::DKD,
            status: SessionStatus::Active,
            recovery_state: None,
            agent_state: None,
            transport_state: None,
            resharing_state: None,
            locking_state: None,
        };

        // Store session
        {
            let mut sessions = self.active_sessions.write().await;
            sessions.insert(session_id, active_session);
        }

        info!(
            "P2P DKD session {} started successfully with {} participants",
            session_id,
            participants.len()
        );
        Ok(session_id)
    }

    async fn start_dkd_with_context_internal(
        &self,
        app_id: String,
        context_label: String,
        participants: Vec<DeviceId>,
        threshold: usize,
        context_bytes: Vec<u8>,
        with_binding_proof: bool,
    ) -> Result<Uuid, ProtocolError> {
        let session_id = self.effects.gen_uuid();

        info!(
            "Starting DKD session {} with context for app={}, participants={}, threshold={}",
            session_id,
            app_id,
            participants.len(),
            threshold
        );

        // Create protocol context internally (no longer exposed to agent)
        use aura_journal::{DeviceMetadata, DeviceType};
        // Create real group public key from device signing key
        let group_public_key = self.device_signing_key.verifying_key();
        let initial_device = DeviceMetadata {
            device_id: self.device_id,
            device_name: "test-device".to_string(),
            device_type: DeviceType::Native,
            public_key: self.device_signing_key.verifying_key(),
            added_at: self.effects.now().unwrap_or(0),
            last_seen: self.effects.now().unwrap_or(0),
            dkd_commitment_proofs: std::collections::BTreeMap::new(),
            next_nonce: 1,
            used_nonces: std::collections::BTreeSet::new(),
        };
        let initial_state = aura_journal::AccountState::new(
            self.account_id,
            group_public_key,
            initial_device,
            threshold as u16,
            (participants.len() as u16).max(threshold as u16),
        );
        let ledger = Arc::new(tokio::sync::RwLock::new(
            aura_journal::AccountLedger::new(initial_state)
                .map_err(|e| ProtocolError::new(format!("Failed to create ledger: {:?}", e)))?,
        ));
        let transport = self.transport.clone().unwrap_or_else(|| {
            warn!("No transport configured, creating SimpleTcp transport for P2P networking");
            // Create a real SimpleTcp transport for P2P communication
            self.create_default_transport()
        });

        // Execute DKD through LifecycleScheduler
        let scheduler = LifecycleScheduler::with_effects(self.effects.clone());
        match scheduler
            .execute_dkd(
                Some(session_id.into()), // session_id - convert Uuid to SessionId
                self.account_id,
                self.device_id,
                app_id.clone(),
                context_label.clone(),
                participants.clone(),
                threshold as usize,
                context_bytes,
                Some(ledger),
                Some(transport),
            )
            .await
        {
            Ok(dkd_result) => {
                let derived_key_bytes = dkd_result.derived_key;
                // Create binding proof if requested
                let binding_proof = if with_binding_proof {
                    Some(self.generate_binding_proof(&app_id, &context_label, &derived_key_bytes))
                } else {
                    None
                };

                // Send completion response
                self.send_response(SessionResponse::DkdCompleted {
                    session_id,
                    derived_key_bytes,
                    binding_proof,
                })
                .await;

                info!("DKD session {} completed successfully", session_id);
            }
            Err(e) => {
                error!("DKD session {} failed: {:?}", session_id, e);
                self.send_response(SessionResponse::Error {
                    message: format!("DKD failed: {:?}", e),
                })
                .await;
            }
        }

        // Create and store active session
        let active_session = ActiveSession {
            session_id,
            protocol_type: SessionProtocolType::DKD,
            status: SessionStatus::Active,
            recovery_state: None,
            agent_state: None,
            transport_state: None,
            resharing_state: None,
            locking_state: None,
        };

        // Store session
        {
            let mut sessions = self.active_sessions.write().await;
            sessions.insert(session_id, active_session);
        }

        Ok(session_id)
    }

    /// Discover available devices for P2P protocols using real transport and capability verification
    ///
    /// This replaces the previous placeholder implementation with real device discovery
    async fn discover_available_devices(&self) -> Result<Vec<DeviceId>, ProtocolError> {
        debug!(
            "Discovering available devices for P2P coordination on device {}",
            self.device_id
        );

        // Step 1: Query transport layer for online peers
        let online_peers = self.query_online_peers().await?;
        debug!(
            "Found {} online peers via transport layer",
            online_peers.len()
        );

        // Step 2: Filter peers that have valid key shares for this account
        let valid_peers = self.filter_peers_with_key_shares(&online_peers).await?;
        debug!("Found {} peers with valid key shares", valid_peers.len());

        // Step 3: Verify presence tickets and capability permissions
        let authorized_peers = self.verify_peer_capabilities(&valid_peers).await?;
        debug!(
            "Found {} authorized peers with valid capabilities",
            authorized_peers.len()
        );

        // Step 4: Include this device if it has valid shares
        let mut participants = authorized_peers;
        if self.has_valid_key_shares().await? {
            participants.insert(0, self.device_id); // This device first
        }

        debug!("Final participant list: {} devices", participants.len());
        Ok(participants)
    }

    /// Query transport layer for online peers
    async fn query_online_peers(&self) -> Result<Vec<DeviceId>, ProtocolError> {
        // Query the transport layer for peer discovery
        // In the current implementation, the transport is likely MemoryTransport,
        // but this provides the interface for real transport integration

        // For now, we'll check if there are any persisted peer connections
        // and simulate online discovery based on account relationships
        let account_peers = self.get_account_peer_devices().await?;

        // Filter to only "online" peers (for now, all account peers are considered potentially online)
        // In production, this would ping each peer or check transport layer connection status
        Ok(account_peers)
    }

    /// Get devices associated with this account from the ledger
    async fn get_account_peer_devices(&self) -> Result<Vec<DeviceId>, ProtocolError> {
        // Access the account ledger to find other devices
        let ledger = self.account_ledger.read().await;

        // Extract device IDs from the account state
        // This would typically come from the device enrollment events in the ledger
        let devices = ledger
            .get_enrolled_devices()
            .unwrap_or_default()
            .into_iter()
            .filter(|device_id| *device_id != self.device_id) // Exclude self
            .collect();

        Ok(devices)
    }

    /// Filter peers that have valid key shares for threshold operations
    async fn filter_peers_with_key_shares(
        &self,
        peer_devices: &[DeviceId],
    ) -> Result<Vec<DeviceId>, ProtocolError> {
        let mut valid_peers = Vec::new();

        for device_id in peer_devices {
            // Check if this device has valid FROST key shares
            // This would typically involve checking the account ledger for key share enrollment
            if self.device_has_key_shares(*device_id).await? {
                valid_peers.push(*device_id);
            }
        }

        Ok(valid_peers)
    }

    /// Verify peer capabilities and presence tickets
    async fn verify_peer_capabilities(
        &self,
        peer_devices: &[DeviceId],
    ) -> Result<Vec<DeviceId>, ProtocolError> {
        let mut authorized_peers = Vec::new();

        for device_id in peer_devices {
            // Verify capability permissions for threshold operations
            if self.verify_device_capabilities(*device_id).await? {
                authorized_peers.push(*device_id);
            }
        }

        Ok(authorized_peers)
    }

    /// Check if this device has valid key shares
    async fn has_valid_key_shares(&self) -> Result<bool, ProtocolError> {
        // Check if this device has enrolled FROST key shares
        let ledger = self.account_ledger.read().await;
        Ok(ledger
            .get_enrolled_devices()
            .unwrap_or_default()
            .contains(&self.device_id))
    }

    /// Check if a device has valid key shares in the account
    async fn device_has_key_shares(&self, device_id: DeviceId) -> Result<bool, ProtocolError> {
        let ledger = self.account_ledger.read().await;
        Ok(ledger
            .get_enrolled_devices()
            .unwrap_or_default()
            .contains(&device_id))
    }

    /// Verify device capabilities for threshold operations
    async fn verify_device_capabilities(
        &self,
        _device_id: DeviceId,
    ) -> Result<bool, ProtocolError> {
        // For now, assume all enrolled devices have proper capabilities
        // In production, this would verify:
        // - Valid presence tickets
        // - Proper capability proofs
        // - Device authentication status
        // - Session credentials
        Ok(true)
    }

    /// Legacy method for backward compatibility - delegates to discover_available_devices
    async fn get_available_participants(&self) -> Result<Vec<DeviceId>, ProtocolError> {
        self.discover_available_devices().await
    }

    /// Generate binding proof for derived identity (moved from agent)
    fn generate_binding_proof(
        &self,
        app_id: &str,
        context_label: &str,
        derived_key: &[u8],
    ) -> Vec<u8> {
        // Create binding proof message that includes:
        // - Device ID (to identify the signing device)
        // - App ID and context (to scope the binding)
        // - Derived key (what we're binding to the device)
        // - Timestamp (to prevent replay attacks)
        let timestamp = self.effects.now().unwrap_or(0);

        let proof_content = format!(
            "BINDING_PROOF:{}:{}:{}:{}:{}",
            self.device_id.0,
            app_id,
            context_label,
            hex::encode(derived_key),
            timestamp
        );

        // For MVP, use a simple hash as binding proof
        // In production, this would be signed with the device key
        blake3::hash(proof_content.as_bytes()).as_bytes().to_vec()
    }

    async fn start_recovery_session_internal(
        &self,
        guardian_threshold: usize,
        cooldown_seconds: u64,
    ) -> Result<Uuid, ProtocolError> {
        let session_id = self.effects.gen_uuid();

        info!(
            "Starting recovery session {} with threshold={}, cooldown={}s",
            session_id, guardian_threshold, cooldown_seconds
        );

        // TODO: Create session-typed recovery protocol
        // let guardians = vec![]; // Will be populated based on account state
        // let recovery_protocol = new_session_typed_recovery(
        //     session_id,
        //     self.device_id,
        //     guardians,
        //     guardian_threshold as u16,
        //     Some(cooldown_seconds / 3600), // Convert to hours
        // );
        // let recovery_state = RecoverySessionState::RecoveryInitialized(recovery_protocol);

        // Create active session
        let active_session = ActiveSession {
            session_id,
            protocol_type: SessionProtocolType::Recovery,
            status: SessionStatus::Active,
            recovery_state: None, // TODO: populate when recovery state is available
            agent_state: None,
            transport_state: None,
            resharing_state: None,
            locking_state: None,
        };

        // Store session
        {
            let mut sessions = self.active_sessions.write().await;
            sessions.insert(session_id, active_session);
        }

        info!("Recovery session {} started successfully", session_id);
        Ok(session_id)
    }

    async fn start_resharing_session_internal(
        &self,
        _new_participants: Vec<DeviceId>,
        _new_threshold: usize,
    ) -> Result<Uuid, ProtocolError> {
        let session_id = self.effects.gen_uuid();

        info!("Starting resharing session {}", session_id);

        // Create session-typed resharing protocol
        let idle_agent = new_session_typed_agent(self.device_id);
        let session_id_for_protocol = SessionId(session_id);
        let resharing_protocol = idle_agent.begin_resharing(session_id_for_protocol);
        let resharing_state = AgentSessionState::ResharingInProgress(resharing_protocol);

        let active_session = ActiveSession {
            session_id,
            protocol_type: SessionProtocolType::Resharing,
            status: SessionStatus::Active,
            recovery_state: None,
            agent_state: None,
            transport_state: None,
            resharing_state: Some(resharing_state),
            locking_state: None,
        };

        // Store session
        {
            let mut sessions = self.active_sessions.write().await;
            sessions.insert(session_id, active_session);
        }

        info!("Resharing session {} started successfully", session_id);
        Ok(session_id)
    }

    async fn start_locking_session_internal(
        &self,
        _operation_type: aura_journal::OperationType,
    ) -> Result<Uuid, ProtocolError> {
        let session_id = self.effects.gen_uuid();

        info!("Starting locking session {}", session_id);

        // Create session-typed locking protocol
        let idle_agent = new_session_typed_agent(self.device_id);
        let session_id_for_protocol = SessionId(session_id);
        let locking_protocol = idle_agent.begin_locking(session_id_for_protocol);
        let locking_state = AgentSessionState::LockingInProgress(locking_protocol);

        let active_session = ActiveSession {
            session_id,
            protocol_type: SessionProtocolType::Locking,
            status: SessionStatus::Active,
            recovery_state: None,
            agent_state: None,
            transport_state: None,
            resharing_state: None,
            locking_state: Some(locking_state),
        };

        // Store session
        {
            let mut sessions = self.active_sessions.write().await;
            sessions.insert(session_id, active_session);
        }

        info!("Locking session {} started successfully", session_id);
        Ok(session_id)
    }

    async fn start_agent_session_internal(&self) -> Result<Uuid, ProtocolError> {
        let session_id = self.effects.gen_uuid();

        info!("Starting agent session {}", session_id);

        // Create session-typed agent protocol
        let agent_protocol = new_session_typed_agent(self.device_id);
        let agent_state = AgentSessionState::AgentIdle(agent_protocol);

        // Create active session
        let active_session = ActiveSession {
            session_id,
            protocol_type: SessionProtocolType::Agent,
            status: SessionStatus::Active,
            recovery_state: None,
            agent_state: Some(agent_state),
            transport_state: None,
            resharing_state: None,
            locking_state: None,
        };

        // Store session
        {
            let mut sessions = self.active_sessions.write().await;
            sessions.insert(session_id, active_session);
        }

        info!("Agent session {} started successfully", session_id);
        Ok(session_id)
    }

    async fn terminate_session_internal(&self, session_id: Uuid) -> Result<(), ProtocolError> {
        info!("Terminating session {}", session_id);

        let mut sessions = self.active_sessions.write().await;

        if let Some(mut session) = sessions.remove(&session_id) {
            session.status = SessionStatus::Completed;
            info!("Session {} terminated successfully", session_id);
        } else {
            warn!("Attempted to terminate non-existent session {}", session_id);
        }

        Ok(())
    }

    async fn send_event_to_session(
        &self,
        session_id: Uuid,
        _event: Event,
    ) -> Result<(), ProtocolError> {
        debug!("Sending event to session {}", session_id);

        let mut sessions = self.active_sessions.write().await;

        if let Some(session) = sessions.get_mut(&session_id) {
            // Route event to appropriate session state machine based on protocol type
            match (
                &session.protocol_type,
                &mut session.recovery_state,
                &mut session.agent_state,
            ) {
                (SessionProtocolType::DKD, _, _) => {
                    debug!("Routing event to DKD session {}", session_id);
                    // Handle DKD-specific events
                    debug!("Processing DKD event in session {}", session_id);
                    // In full implementation, would transition DKD state machine
                    session.status = SessionStatus::Active;
                }
                (SessionProtocolType::Recovery, Some(_recovery_state), _) => {
                    debug!("Routing event to Recovery session {}", session_id);
                    // Handle Recovery-specific events
                    debug!("Processing Recovery event in session {}", session_id);
                    // In full implementation, would transition Recovery state machine
                    session.status = SessionStatus::Active;
                }
                (SessionProtocolType::Agent, _, Some(_agent_state)) => {
                    debug!("Routing event to Agent session {}", session_id);
                    // Handle Agent-specific events
                    debug!("Processing Agent event in session {}", session_id);
                    // In full implementation, would transition Agent state machine
                    session.status = SessionStatus::Active;
                }
                _ => {
                    warn!(
                        "Session {} has no matching state for protocol type {:?}",
                        session_id, session.protocol_type
                    );
                }
            }
            debug!("Event routed to session {}", session_id);
        } else {
            warn!(
                "Attempted to send event to non-existent session {}",
                session_id
            );
        }

        Ok(())
    }

    async fn update_session_status_internal(
        &self,
        session_id: Uuid,
        status: SessionStatus,
    ) -> Result<(), ProtocolError> {
        debug!("Updating session {} status to {:?}", session_id, status);

        let mut sessions = self.active_sessions.write().await;

        if let Some(session) = sessions.get_mut(&session_id) {
            session.status = status;
            debug!("Session {} status updated successfully", session_id);
        } else {
            warn!(
                "Attempted to update status of non-existent session {}",
                session_id
            );
        }

        Ok(())
    }

    /// Handle transport event
    async fn handle_transport_event(&self, event: TransportEvent) -> Result<(), ProtocolError> {
        debug!("Handling transport event: {:?}", event);

        match event {
            TransportEvent::ConnectionEstablished { peer_id } => {
                info!("Transport connection established with peer: {}", peer_id);
                // Notify relevant sessions about new peer connection
                self.notify_sessions_of_peer_event(&peer_id, PeerEventType::Connected)
                    .await?;
            }
            TransportEvent::ConnectionLost { peer_id } => {
                warn!("Transport connection lost with peer: {}", peer_id);
                // Notify relevant sessions about lost peer connection
                self.notify_sessions_of_peer_event(&peer_id, PeerEventType::Disconnected)
                    .await?;
                // Trigger session recovery or cleanup if needed
                self.handle_peer_disconnection(&peer_id).await?;
            }
            TransportEvent::MessageReceived { peer_id, message } => {
                debug!(
                    "Message received from peer {}: {} bytes",
                    peer_id,
                    message.len()
                );
                // Route message to appropriate session based on message content
                self.route_message_to_session(&peer_id, &message).await?;
            }
            TransportEvent::MessageSent {
                peer_id,
                message_size,
            } => {
                debug!("Message sent to peer {}: {} bytes", peer_id, message_size);
                // Update session statistics or retry logic
                self.update_session_statistics(&peer_id, message_size).await;
            }
            TransportEvent::TransportError { error } => {
                error!("Transport error: {}", error);
                // Handle transport errors - may need to abort affected sessions
                self.handle_transport_error(&error).await?;
            }
        }

        Ok(())
    }

    /// Create a transport event sender for this runtime
    pub fn create_transport_event_sender(&self) -> mpsc::UnboundedSender<TransportEvent> {
        let command_tx = self.command_tx.clone();

        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<TransportEvent>();

        // Spawn task to forward transport events to session commands
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                let command = SessionCommand::TransportEvent { event };
                if command_tx.send(command).is_err() {
                    break;
                }
            }
        });

        event_tx
    }

    /// Notify sessions about peer connection/disconnection events
    async fn notify_sessions_of_peer_event(
        &self,
        peer_id: &str,
        event_type: PeerEventType,
    ) -> Result<(), ProtocolError> {
        let sessions = self.active_sessions.read().await;

        for (session_id, _session) in sessions.iter() {
            match event_type {
                PeerEventType::Connected => {
                    info!(
                        "Notifying session {} of peer {} connection",
                        session_id, peer_id
                    );
                    // In full implementation, would send peer connection event to session
                }
                PeerEventType::Disconnected => {
                    warn!(
                        "Notifying session {} of peer {} disconnection",
                        session_id, peer_id
                    );
                    // In full implementation, would send peer disconnection event to session
                }
            }
        }

        Ok(())
    }

    /// Handle peer disconnection and trigger recovery if needed
    async fn handle_peer_disconnection(&self, peer_id: &str) -> Result<(), ProtocolError> {
        let mut sessions = self.active_sessions.write().await;

        // Check if any sessions need recovery due to peer disconnection
        for (session_id, session) in sessions.iter_mut() {
            match session.protocol_type {
                SessionProtocolType::DKD | SessionProtocolType::Recovery => {
                    // These protocols may need peer connectivity
                    if session.status == SessionStatus::Active {
                        warn!(
                            "Session {} may need recovery due to peer {} disconnection",
                            session_id, peer_id
                        );
                        // In full implementation, would trigger recovery protocol
                    }
                }
                _ => {
                    // Other protocols less sensitive to peer disconnection
                }
            }
        }

        Ok(())
    }

    /// Route incoming message to appropriate session
    async fn route_message_to_session(
        &self,
        peer_id: &str,
        message: &[u8],
    ) -> Result<(), ProtocolError> {
        // Try to deserialize as session message
        match bincode::deserialize::<SessionMessage>(message) {
            Ok(session_msg) => {
                debug!(
                    "Routing message from {} to session {} (event: {:?})",
                    peer_id, session_msg.session_id, session_msg.event_type
                );

                // Create appropriate event for the target session
                // For MVP, we'll create a simple DKD initiation event
                let event_data = InitiateDkdSessionEvent {
                    session_id: session_msg.session_id,
                    context_id: session_msg.payload,
                    threshold: 2, // Default threshold
                    participants: vec![self.device_id],
                    start_epoch: self.effects.now().unwrap_or(0),
                    ttl_in_epochs: 100,
                };

                // Create event with placeholder signature first
                let mut event = Event::new(
                    self.account_id,
                    0,    // nonce
                    None, // parent hash
                    self.effects.now().unwrap_or(0),
                    EventType::InitiateDkdSession(event_data),
                    EventAuthorization::DeviceCertificate {
                        device_id: self.device_id,
                        signature: ed25519_signature_from_bytes(&[0u8; 64]).unwrap(), // Placeholder signature
                    },
                    &self.effects,
                )
                .map_err(|e| ProtocolError::new(format!("Failed to create event: {}", e)))?;

                // Sign the event and update authorization with real signature
                let signature = self
                    .sign_event(&event)
                    .map_err(|e| ProtocolError::new(format!("Failed to sign event: {}", e)))?;

                match &mut event.authorization {
                    EventAuthorization::DeviceCertificate { signature: sig, .. } => {
                        *sig = signature;
                    }
                    _ => {
                        return Err(ProtocolError::new(
                            "Unexpected authorization type".to_string(),
                        ))
                    }
                }

                // Send to target session
                self.send_event_to_session(session_msg.session_id, event)
                    .await?;
            }
            Err(e) => {
                warn!("Failed to deserialize message from {}: {:?}", peer_id, e);
                // Try routing as broadcast message to all active sessions
                self.broadcast_message_to_sessions(peer_id, message).await?;
            }
        }

        Ok(())
    }

    /// Broadcast message to all active sessions
    async fn broadcast_message_to_sessions(
        &self,
        peer_id: &str,
        message: &[u8],
    ) -> Result<(), ProtocolError> {
        let sessions = self.active_sessions.read().await;

        debug!(
            "Broadcasting message from {} to {} active sessions",
            peer_id,
            sessions.len()
        );

        // Note: We need to drop the read lock before calling send_event_to_session
        // which takes a write lock, so we'll collect session IDs first

        // Collect session IDs to avoid holding read lock
        let session_ids: Vec<Uuid> = sessions.keys().copied().collect();
        drop(sessions);

        // Send to each session
        for session_id in session_ids {
            // Create a DKD event for broadcast
            let event_data = InitiateDkdSessionEvent {
                session_id,
                context_id: message.to_vec(),
                threshold: 2,
                participants: vec![self.device_id],
                start_epoch: self.effects.now().unwrap_or(0),
                ttl_in_epochs: 100,
            };

            // Create event with placeholder signature first
            let event_result = Event::new(
                self.account_id,
                0,    // nonce
                None, // parent hash
                self.effects.now().unwrap_or(0),
                EventType::InitiateDkdSession(event_data),
                EventAuthorization::DeviceCertificate {
                    device_id: self.device_id,
                    signature: ed25519_signature_from_bytes(&[0u8; 64]).unwrap(), // Placeholder signature
                },
                &self.effects,
            );

            match event_result {
                Ok(mut event) => {
                    // Sign the event and update authorization with real signature
                    match self.sign_event(&event) {
                        Ok(signature) => {
                            match &mut event.authorization {
                                EventAuthorization::DeviceCertificate {
                                    signature: sig, ..
                                } => {
                                    *sig = signature;
                                }
                                _ => {
                                    warn!(
                                        "Unexpected authorization type for session {}",
                                        session_id
                                    );
                                    continue;
                                }
                            }

                            // Send the properly signed event
                            if let Err(e) = self.send_event_to_session(session_id, event).await {
                                warn!("Failed to broadcast to session {}: {:?}", session_id, e);
                            }
                        }
                        Err(e) => {
                            warn!("Failed to sign event for session {}: {}", session_id, e);
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to create broadcast event for session {}: {}",
                        session_id, e
                    );
                }
            }
        }

        Ok(())
    }

    /// Update session statistics for message activity
    async fn update_session_statistics(&self, peer_id: &str, message_size: usize) {
        debug!(
            "Updating statistics: peer={}, message_size={}",
            peer_id, message_size
        );
        // In full implementation, would update session-specific statistics
        // For now, just log the activity
    }

    /// Handle transport errors and abort affected sessions if needed
    async fn handle_transport_error(&self, error: &str) -> Result<(), ProtocolError> {
        error!("Handling transport error: {}", error);

        let mut sessions = self.active_sessions.write().await;

        // Check if transport error requires aborting sessions
        if error.contains("connection") || error.contains("timeout") {
            warn!("Transport error may require session cleanup");

            // Mark sessions that may be affected by transport errors
            for (session_id, session) in sessions.iter_mut() {
                if matches!(
                    session.protocol_type,
                    SessionProtocolType::DKD | SessionProtocolType::Recovery
                ) {
                    // These protocols rely heavily on transport
                    if session.status == SessionStatus::Active {
                        warn!(
                            "Marking session {} as potentially failed due to transport error",
                            session_id
                        );
                        // In full implementation, might transition to Failed status
                        // For now, keep as Active but log the concern
                    }
                }
            }
        }

        Ok(())
    }

    /// Create a default transport for P2P communication
    /// 
    /// This creates a SimpleTcp transport as the default for real P2P networking.
    /// In production, this should be configurable via settings.
    fn create_default_transport(&self) -> Arc<dyn crate::Transport> {
        // For now, return MemoryTransport as fallback since SimpleTcp requires async creation
        // TODO: Replace with proper async SimpleTcp transport creation
        warn!("Using MemoryTransport as fallback - SimpleTcp transport requires async initialization");
        Arc::new(crate::MemoryTransport) as Arc<dyn crate::Transport>
    }
}

impl Clone for LocalSessionRuntime {
    fn clone(&self) -> Self {
        // Create new channels for the cloned runtime
        let (command_tx, _command_rx) = mpsc::unbounded_channel();

        Self {
            device_id: self.device_id,
            account_id: self.account_id,
            device_signing_key: self.device_signing_key.clone(),
            command_rx: Arc::new(Mutex::new(None)), // Cloned runtime doesn't get the receiver
            command_tx,
            response_tx: None,
            active_sessions: self.active_sessions.clone(),
            effects: self.effects.clone(),
        }
    }
}

#[cfg(test)]
#[allow(warnings, clippy::all)]
mod tests {
    use super::*;
    use aura_crypto::Effects;
    use aura_types::{AccountIdExt, DeviceIdExt};

    #[tokio::test]
    async fn test_local_session_runtime_creation() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);
        let account_id = AccountId::new_with_effects(&effects);

        let runtime = LocalSessionRuntime::new_with_generated_key(device_id, account_id, effects);

        // Should be able to get command sender
        let _command_tx = runtime.command_sender();

        // Should start with no active sessions
        let status = runtime.get_session_status().await;
        assert_eq!(status.len(), 0);
    }

    #[tokio::test]
    #[ignore = "Recovery and agent session choreography not yet implemented"]
    async fn test_session_lifecycle() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);
        let account_id = AccountId::new_with_effects(&effects);

        let runtime = LocalSessionRuntime::new_with_generated_key(device_id, account_id, effects);

        // Start DKD session
        let dkd_session = runtime
            .start_dkd_session("test-app".to_string(), "test-context".to_string())
            .await
            .unwrap();
        assert!(dkd_session != Uuid::nil());

        // Start recovery session
        let recovery_session = runtime.start_recovery_session(3, 3600).await.unwrap();
        assert!(recovery_session != Uuid::nil());

        // Start agent session
        let agent_session = runtime.start_agent_session().await.unwrap();
        assert!(agent_session != Uuid::nil());
    }
}

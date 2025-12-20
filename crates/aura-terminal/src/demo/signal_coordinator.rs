//! # Signal-Based Demo Coordinator
//!
//! Coordinates demo mode using signals instead of AuraEvent.
//!
//! ## Architecture
//!
//! The signal coordinator:
//! 1. Subscribes to AppCore signals to detect Bob's actions
//! 2. Routes actions to simulated agents (Alice/Carol)
//! 3. Updates signals with agent responses

use std::sync::Arc;

use async_lock::RwLock;
use aura_app::signal_defs::{CHAT_SIGNAL, RECOVERY_SIGNAL};
use aura_app::views::chat::Message as ChatMessage;
use aura_app::views::recovery::{Guardian, GuardianStatus, RecoveryProcessStatus};
use aura_app::views::{ChatState, RecoveryState};
use aura_app::AppCore;
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::AuthorityId;
use tokio::sync::mpsc;

use super::{AgentEvent, AgentResponse, SimulatedBridge};

/// Coordinates demo mode using signals instead of events
pub struct DemoSignalCoordinator {
    /// AppCore for signal access
    app_core: Arc<RwLock<AppCore>>,

    /// Bob's authority ID (for filtering messages)
    bob_authority: AuthorityId,

    /// Bridge to simulated agents
    sim_bridge: Arc<SimulatedBridge>,

    /// Channel to receive agent responses
    response_rx: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<(AuthorityId, AgentResponse)>>>,

    /// Track last seen message count to detect new messages
    last_message_count: Arc<tokio::sync::Mutex<usize>>,

    /// Track last recovery state to detect changes
    last_recovery_state: Arc<tokio::sync::Mutex<Option<String>>>,
}

impl DemoSignalCoordinator {
    /// Create a new signal coordinator
    pub fn new(
        app_core: Arc<RwLock<AppCore>>,
        bob_authority: AuthorityId,
        sim_bridge: Arc<SimulatedBridge>,
        response_rx: mpsc::UnboundedReceiver<(AuthorityId, AgentResponse)>,
    ) -> Self {
        Self {
            app_core,
            bob_authority,
            sim_bridge,
            response_rx: Arc::new(tokio::sync::Mutex::new(response_rx)),
            last_message_count: Arc::new(tokio::sync::Mutex::new(0)),
            last_recovery_state: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    /// Start the coordinator tasks
    ///
    /// Returns handles to the spawned tasks for lifecycle management.
    pub fn start(self: Arc<Self>) -> (tokio::task::JoinHandle<()>, tokio::task::JoinHandle<()>) {
        let coordinator_clone = self.clone();
        let action_detector = tokio::spawn(async move {
            coordinator_clone.run_action_detector().await;
        });

        let coordinator_clone = self.clone();
        let response_handler = tokio::spawn(async move {
            coordinator_clone.run_response_handler().await;
        });

        (action_detector, response_handler)
    }

    /// Run the action detector loop
    ///
    /// Subscribes to signals and detects Bob's actions to forward to agents.
    async fn run_action_detector(&self) {
        // Subscribe to chat signal to detect new messages from Bob
        let mut chat_stream = {
            let core = self.app_core.read().await;
            core.subscribe(&*CHAT_SIGNAL)
        };

        // Subscribe to recovery signal to detect recovery initiation
        let mut recovery_stream = {
            let core = self.app_core.read().await;
            core.subscribe(&*RECOVERY_SIGNAL)
        };

        loop {
            tokio::select! {
                // Check for new chat messages
                Ok(chat_state) = chat_stream.recv() => {
                    self.handle_chat_update(&chat_state).await;
                }

                // Check for recovery state changes
                Ok(recovery_state) = recovery_stream.recv() => {
                    self.handle_recovery_update(&recovery_state).await;
                }
            }
        }
    }

    /// Handle chat state updates - detect new messages from Bob
    async fn handle_chat_update(&self, chat_state: &ChatState) {
        let current_count = chat_state.messages.len();
        let mut last_count = self.last_message_count.lock().await;

        if current_count > *last_count {
            // New messages - check if any are from Bob
            for msg in chat_state.messages.iter().skip(*last_count) {
                if msg.sender_id == self.bob_authority {
                    // Use sender_name as best-effort contact label; channel id as fallback
                    let channel = chat_state
                        .selected_channel_id
                        .as_ref()
                        .map(|id| id.to_string())
                        .unwrap_or_else(|| msg.channel_id.to_string());

                    let agent_event = AgentEvent::MessageReceived {
                        from: self.bob_authority,
                        channel,
                        content: msg.content.clone(),
                    };
                    self.sim_bridge.send_agent_event(agent_event);
                    tracing::debug!("Demo: Forwarded Bob's message to agents");
                }
            }
            *last_count = current_count;
        }
    }

    /// Handle recovery state updates - detect recovery initiation
    async fn handle_recovery_update(&self, recovery_state: &RecoveryState) {
        let mut last_state = self.last_recovery_state.lock().await;

        // Check if a new recovery session started
        if let Some(ref active) = recovery_state.active_recovery {
            let session_id = &active.id;

            // Only forward if this is a new session
            if last_state.as_ref() != Some(session_id)
                && matches!(
                    active.status,
                    RecoveryProcessStatus::Initiated | RecoveryProcessStatus::WaitingForApprovals
                )
            {
                // Bob initiated recovery - forward to agents
                let context_id = crate::ids::context_id("demo-recovery-context");
                let agent_event = AgentEvent::RecoveryRequested {
                    account: self.bob_authority,
                    session_id: session_id.clone(),
                    context_id,
                };
                self.sim_bridge.send_agent_event(agent_event);
                tracing::info!("Demo: Forwarded recovery request to agents");
                *last_state = Some(session_id.clone());
            }
        } else {
            // No active recovery - reset tracking
            *last_state = None;
        }
    }

    /// Run the response handler loop
    ///
    /// Processes agent responses and updates signals.
    async fn run_response_handler(&self) {
        loop {
            // Try to receive a response
            let response = {
                let mut rx = self.response_rx.lock().await;
                rx.recv().await
            };

            match response {
                Some((authority_id, response)) => {
                    self.handle_agent_response(authority_id, response).await;
                }
                None => {
                    // Channel closed
                    tracing::info!("Demo: Agent response channel closed");
                    break;
                }
            }
        }
    }

    /// Handle an agent response by updating ViewState
    ///
    /// Uses AppCore methods to update ViewState. Signal forwarding
    /// automatically propagates changes to ReactiveEffects signals.
    async fn handle_agent_response(&self, authority_id: AuthorityId, response: AgentResponse) {
        match response {
            AgentResponse::SendMessage { channel, content } => {
                // Add message via ViewState
                // Signal forwarding automatically propagates to CHAT_SIGNAL
                let channel_id = channel.parse().unwrap_or_default();
                let new_message = ChatMessage {
                    id: crate::ids::uuid("demo-msg").to_string(),
                    channel_id,
                    sender_id: authority_id,
                    sender_name: self.get_agent_name(&authority_id),
                    content,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                    is_own: false,
                    reply_to: None,
                    is_read: false,
                };

                let core = self.app_core.read().await;
                core.add_chat_message(new_message);
                tracing::debug!("Demo: Added agent message to chat");
            }

            AgentResponse::ApproveRecovery {
                session_id,
                account: _,
            } => {
                // Update recovery via ViewState
                // Signal forwarding automatically propagates to RECOVERY_SIGNAL
                let core = self.app_core.read().await;
                // Check if this is the active recovery session
                let recovery = core.views().snapshot().recovery;
                if let Some(ref active) = recovery.active_recovery {
                    if active.id == session_id && !active.approved_by.contains(&authority_id) {
                        core.add_recovery_approval(authority_id.clone());

                        // Log approval status
                        let updated_recovery = core.views().snapshot().recovery;
                        if let Some(ref active) = updated_recovery.active_recovery {
                            tracing::info!(
                                "Demo: Guardian {} approved recovery ({}/{})",
                                authority_id,
                                active.approvals_received,
                                active.approvals_required
                            );
                            if active.approvals_received >= active.approvals_required {
                                tracing::info!("Demo: Recovery threshold reached!");
                            }
                        }
                    }
                }
            }

            AgentResponse::AcceptGuardianBinding {
                account,
                context_id,
            } => {
                let guardian_name = self.get_agent_name(&authority_id);
                tracing::info!(
                    "Demo: Guardian {} ({}) accepted binding for {} in {}",
                    guardian_name,
                    authority_id,
                    account,
                    context_id
                );

                // Update ViewState to mark contact as guardian
                // Signal forwarding automatically propagates to CONTACTS_SIGNAL
                let core = self.app_core.read().await;
                // Check if contact exists first
                let contacts = core.views().snapshot().contacts;
                if contacts.contacts.iter().any(|c| c.id == authority_id) {
                    core.set_contact_guardian_status(&authority_id, true);
                    tracing::info!("Demo: Updated contact {} is_guardian=true", authority_id);

                    // Update ViewState to add guardian to recovery state
                    // Signal forwarding automatically propagates to RECOVERY_SIGNAL
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;

                    core.add_guardian(Guardian {
                        id: authority_id.clone(),
                        name: guardian_name,
                        status: GuardianStatus::Active,
                        added_at: now,
                        last_seen: Some(now),
                    });
                    tracing::info!("Demo: Added {} to guardians list", authority_id);
                } else {
                    tracing::warn!(
                        "Demo: Contact {} not found ({} contacts present)",
                        authority_id,
                        contacts.contacts.len()
                    );
                }
            }
        }
    }

    /// Get display name for an agent authority
    fn get_agent_name(&self, authority_id: &AuthorityId) -> String {
        // Heuristic mapping; demo bridge does not yet expose contacts
        let id_str = authority_id.to_string();
        if id_str.contains("alice") || id_str.ends_with("01") {
            "Alice".to_string()
        } else if id_str.contains("carol") || id_str.ends_with("02") {
            "Carol".to_string()
        } else {
            format!("Agent-{}", &id_str[..8.min(id_str.len())])
        }
    }
}

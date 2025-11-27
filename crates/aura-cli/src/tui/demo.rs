//! # Demo Interface
//!
//! Main demo orchestration interface for Bob's recovery journey.

use aura_core::PhysicalTimeEffects;
use aura_effects::time::PhysicalTimeHandler;
use std::collections::HashMap;
use tokio::sync::mpsc;

use super::{
    app::{DemoEvent, TuiApp},
    state::{AppState, DemoPhase},
};
use aura_chat::{ChatGroupId, ChatMessage, ChatMessageId};
use aura_core::{
    time::{PhysicalTime, TimeStamp},
    AuthorityId,
};

/// Demo orchestration interface
pub struct DemoInterface {
    /// TUI application
    app: TuiApp,
    /// Demo event receiver
    demo_rx: mpsc::UnboundedReceiver<DemoEvent>,
    /// Demo event sender for TUI
    #[allow(dead_code)]
    demo_tx: mpsc::UnboundedSender<DemoEvent>,
    /// Mock authorities for demo
    authorities: HashMap<String, AuthorityId>,
    /// Mock messages for restoration
    saved_messages: Vec<ChatMessage>,
}

impl DemoInterface {
    /// Create new demo interface
    pub fn new() -> Self {
        let (demo_tx, demo_rx) = mpsc::unbounded_channel();
        let mut app = TuiApp::new();
        app.set_demo_sender(demo_tx.clone());

        Self {
            app,
            demo_rx,
            demo_tx,
            authorities: HashMap::new(),
            saved_messages: Vec::new(),
        }
    }

    /// Run the demo interface
    pub async fn run(&mut self) -> anyhow::Result<()> {
        // Initialize demo data
        self.initialize_demo_data().await?;

        // Start TUI in background
        let app_handle = {
            let mut app = std::mem::take(&mut self.app);
            tokio::spawn(async move {
                if let Err(e) = app.run().await {
                    tracing::error!("TUI error: {}", e);
                }
            })
        };

        // Handle demo events
        while let Some(event) = self.demo_rx.recv().await {
            if let Err(e) = self.handle_demo_event(event).await {
                tracing::error!("Demo event error: {}", e);
            }
        }

        // Wait for TUI to finish
        let _ = app_handle.await;
        Ok(())
    }

    /// Initialize demo data
    async fn initialize_demo_data(&mut self) -> anyhow::Result<()> {
        // Create mock authorities
        self.authorities
            .insert("bob".to_string(), AuthorityId::new());
        self.authorities
            .insert("alice".to_string(), AuthorityId::new());
        self.authorities
            .insert("charlie".to_string(), AuthorityId::new());

        // Create sample messages for restoration demo
        let group_id = ChatGroupId::from_uuid(uuid::Uuid::new_v4());
        let timestamp = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1000,
            uncertainty: None,
        });

        self.saved_messages = vec![
            ChatMessage::new_text(
                ChatMessageId::from_uuid(uuid::Uuid::new_v4()),
                group_id.clone(),
                AuthorityId::new(),
                "Hey everyone! Welcome to our group chat.".to_string(),
                timestamp.clone(),
            ),
            ChatMessage::new_text(
                ChatMessageId::from_uuid(uuid::Uuid::new_v4()),
                group_id.clone(),
                AuthorityId::new(),
                "This is so cool! Secure messaging with social recovery.".to_string(),
                timestamp.clone(),
            ),
            ChatMessage::new_text(
                ChatMessageId::from_uuid(uuid::Uuid::new_v4()),
                group_id,
                AuthorityId::new(),
                "I love how we can recover our data if something goes wrong!".to_string(),
                timestamp.clone(),
            ),
        ];

        tracing::info!("Demo data initialized");
        Ok(())
    }

    /// Handle demo events from TUI
    async fn handle_demo_event(&mut self, event: DemoEvent) -> anyhow::Result<()> {
        match event {
            DemoEvent::AdvancePhase => {
                self.advance_phase().await?;
            }
            DemoEvent::SendMessage(content) => {
                self.send_message(content).await?;
            }
            DemoEvent::InitiateRecovery => {
                self.initiate_recovery().await?;
            }
            DemoEvent::GuardianApproval(guardian_id) => {
                self.guardian_approves(guardian_id).await?;
            }
            DemoEvent::RestoreMessages => {
                self.restore_messages().await?;
            }
            DemoEvent::Reset => {
                self.reset_demo().await?;
            }
        }
        Ok(())
    }

    /// Advance to next demo phase
    async fn advance_phase(&mut self) -> anyhow::Result<()> {
        // This would integrate with the actual Aura system
        // For demo purposes, we simulate the operations

        match self.app.state().current_phase {
            DemoPhase::Welcome => {
                self.setup_bob_onboarding().await?;
            }
            DemoPhase::BobOnboarding => {
                self.setup_guardians().await?;
            }
            DemoPhase::GuardianSetup => {
                self.start_group_chat().await?;
            }
            DemoPhase::GroupChat => {
                self.simulate_data_loss().await?;
            }
            DemoPhase::DataLoss => {
                // Wait for manual recovery initiation
            }
            DemoPhase::Recovery => {
                // Recovery happens via guardian approvals
            }
            DemoPhase::Restoration => {
                // Complete the demo
                self.complete_demo().await?;
            }
            DemoPhase::Completed => {
                // Demo is done
            }
        }

        Ok(())
    }

    /// Setup Bob's onboarding
    async fn setup_bob_onboarding(&mut self) -> anyhow::Result<()> {
        if let Some(bob_authority) = self.authorities.get("bob").copied() {
            self.app.state_mut().set_bob_authority(bob_authority);
        }

        // Simulate onboarding delay
        PhysicalTimeHandler::new().sleep_ms(500).await.ok();

        tracing::info!("Bob onboarding completed");
        Ok(())
    }

    /// Setup guardians (Alice & Charlie)
    async fn setup_guardians(&mut self) -> anyhow::Result<()> {
        if let Some(alice_authority) = self.authorities.get("alice").copied() {
            self.app.state_mut().setup_alice_guardian(alice_authority);
        }

        // Simulate setup delay
        PhysicalTimeHandler::new().sleep_ms(300).await.ok();

        if let Some(charlie_authority) = self.authorities.get("charlie").copied() {
            self.app
                .state_mut()
                .setup_charlie_guardian(charlie_authority);
        }

        tracing::info!("Guardians setup completed");
        Ok(())
    }

    /// Start group chat
    async fn start_group_chat(&mut self) -> anyhow::Result<()> {
        let group_id = ChatGroupId::from_uuid(uuid::Uuid::new_v4());
        self.app.state_mut().create_chat_group(group_id.clone());

        // Add some initial messages
        let initial_messages = vec![
            "Welcome to our secure group chat!",
            "This is powered by Aura's threshold identity system.",
            "Let's chat securely with social recovery!",
        ];

        for content in initial_messages {
            let message = ChatMessage::new_text(
                ChatMessageId::from_uuid(uuid::Uuid::new_v4()),
                group_id.clone(),
                self.authorities["bob"],
                content.to_string(),
                TimeStamp::PhysicalClock(PhysicalTime {
                    ts_ms: 2000,
                    uncertainty: None,
                }),
            );
            self.app.state_mut().add_message(message);
        }

        tracing::info!("Group chat started");
        Ok(())
    }

    /// Simulate data loss
    async fn simulate_data_loss(&mut self) -> anyhow::Result<()> {
        self.app.state_mut().simulate_data_loss();
        tracing::info!("Data loss simulated");
        Ok(())
    }

    /// Send a message in group chat
    async fn send_message(&mut self, content: String) -> anyhow::Result<()> {
        if let Some(group_id) = self.app.state().chat_group.as_ref() {
            let message = ChatMessage::new_text(
                ChatMessageId::from_uuid(uuid::Uuid::new_v4()),
                group_id.clone(),
                self.authorities["bob"],
                content,
                TimeStamp::PhysicalClock(PhysicalTime {
                    ts_ms: 3000,
                    uncertainty: None,
                }),
            );

            self.app.state_mut().add_message(message);
        }

        Ok(())
    }

    /// Initiate recovery process
    async fn initiate_recovery(&mut self) -> anyhow::Result<()> {
        self.app.state_mut().initiate_recovery();
        tracing::info!("Recovery process initiated");
        Ok(())
    }

    /// Guardian approves recovery
    async fn guardian_approves(&mut self, guardian_id: AuthorityId) -> anyhow::Result<()> {
        self.app.state_mut().guardian_approves_recovery(guardian_id);

        // Check if threshold reached
        let recovery = &self.app.state().recovery_progress;
        if recovery.approvals >= recovery.threshold {
            // Automatically trigger restoration
            self.restore_messages().await?;
        }

        tracing::info!("Guardian {} approved recovery", guardian_id);
        Ok(())
    }

    /// Restore messages after recovery
    async fn restore_messages(&mut self) -> anyhow::Result<()> {
        let restored_messages = self.saved_messages.clone();
        self.app.state_mut().complete_recovery(restored_messages);
        tracing::info!("Messages restored");
        Ok(())
    }

    /// Complete the demo
    async fn complete_demo(&mut self) -> anyhow::Result<()> {
        self.app
            .state_mut()
            .add_status("🎉 Demo journey completed successfully!".to_string());
        tracing::info!("Demo completed");
        Ok(())
    }

    /// Reset the demo
    async fn reset_demo(&mut self) -> anyhow::Result<()> {
        *self.app.state_mut() = AppState::new();
        self.initialize_demo_data().await?;
        tracing::info!("Demo reset");
        Ok(())
    }

    /// Get reference to the TUI app
    pub fn app(&self) -> &TuiApp {
        &self.app
    }

    /// Get mutable reference to the TUI app
    pub fn app_mut(&mut self) -> &mut TuiApp {
        &mut self.app
    }
}

impl Default for DemoInterface {
    fn default() -> Self {
        Self::new()
    }
}

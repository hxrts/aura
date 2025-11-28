#![allow(deprecated)]
//! # Application State for TUI Demo
//!
//! Manages the state of Bob's demo journey through the Aura system.

use aura_chat::{ChatGroupId, ChatMessage};
use aura_core::identifiers::AuthorityId;
use aura_core::time::{PhysicalTime, TimeStamp};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Application state for the TUI demo interface
#[derive(Debug, Clone)]
pub struct AppState {
    /// Current demo phase
    pub current_phase: DemoPhase,
    /// Bob's authority ID
    pub bob_authority: Option<AuthorityId>,
    /// Alice's authority ID (guardian)
    pub alice_authority: Option<AuthorityId>,
    /// Charlie's authority ID (guardian)
    pub charlie_authority: Option<AuthorityId>,
    /// Active chat group ID
    pub chat_group: Option<ChatGroupId>,
    /// Message history for display
    pub message_history: Vec<ChatMessage>,
    /// Recovery progress
    pub recovery_progress: RecoveryProgress,
    /// Demo status messages
    pub status_messages: Vec<String>,
    /// Guardian states
    pub guardian_states: HashMap<AuthorityId, GuardianState>,
}

/// Different phases of Bob's demo journey
#[derive(Debug, Clone, PartialEq)]
pub enum DemoPhase {
    /// Welcome screen
    Welcome,
    /// Bob's initial onboarding
    BobOnboarding,
    /// Setting up guardians (Alice & Charlie)
    GuardianSetup,
    /// Active group chat phase
    GroupChat,
    /// Data loss simulation
    DataLoss,
    /// Guardian recovery process
    Recovery,
    /// Message history restoration
    Restoration,
    /// Demo completed
    Completed,
}

/// Recovery progress tracking
#[derive(Debug, Clone, Default)]
pub struct RecoveryProgress {
    /// Whether recovery has been initiated
    pub initiated: bool,
    /// Guardian approval count
    pub approvals: u32,
    /// Required approvals (threshold)
    pub threshold: u32,
    /// Recovery completion percentage
    pub completion_percent: u32,
    /// Current recovery step description
    pub current_step: String,
}

/// Guardian state for tracking demo coordination
#[derive(Debug, Clone)]
pub struct GuardianState {
    /// Guardian authority ID
    pub authority_id: AuthorityId,
    /// Guardian name for display
    pub name: String,
    /// Whether guardian is online
    pub online: bool,
    /// Whether guardian has approved recovery
    pub approved_recovery: bool,
    /// Last activity timestamp (PhysicalClock ms)
    pub last_activity: TimeStamp,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            current_phase: DemoPhase::Welcome,
            bob_authority: None,
            alice_authority: None,
            charlie_authority: None,
            chat_group: None,
            message_history: Vec::new(),
            recovery_progress: RecoveryProgress::default(),
            status_messages: Vec::new(),
            guardian_states: HashMap::new(),
        }
    }
}

impl AppState {
    /// Create new app state
    pub fn new() -> Self {
        Self::default()
    }

    /// Move to next phase of the demo
    pub fn advance_phase(&mut self) {
        use DemoPhase::*;
        self.current_phase = match self.current_phase {
            Welcome => BobOnboarding,
            BobOnboarding => GuardianSetup,
            GuardianSetup => GroupChat,
            GroupChat => DataLoss,
            DataLoss => Recovery,
            Recovery => Restoration,
            Restoration => Completed,
            Completed => Completed, // Stay at completed
        };
    }

    /// Add a status message to the demo log
    pub fn add_status(&mut self, message: impl Into<String>) {
        self.status_messages.push(message.into());
        // Keep only last 50 messages
        if self.status_messages.len() > 50 {
            self.status_messages.remove(0);
        }
    }

    /// Set Bob's authority
    pub fn set_bob_authority(&mut self, authority: AuthorityId) {
        self.bob_authority = Some(authority);
        self.add_status(format!("Bob's authority created: {}", authority));
    }

    /// Set up Alice as guardian
    pub fn setup_alice_guardian(&mut self, authority: AuthorityId) {
        self.alice_authority = Some(authority);
        self.guardian_states.insert(
            authority,
            GuardianState {
                authority_id: authority,
                name: "Alice".to_string(),
                online: true,
                approved_recovery: false,
                last_activity: Self::now_ts(),
            },
        );
        self.add_status("Alice setup as guardian".to_string());
    }

    /// Set up Charlie as guardian
    pub fn setup_charlie_guardian(&mut self, authority: AuthorityId) {
        self.charlie_authority = Some(authority);
        self.guardian_states.insert(
            authority,
            GuardianState {
                authority_id: authority,
                name: "Charlie".to_string(),
                online: true,
                approved_recovery: false,
                last_activity: Self::now_ts(),
            },
        );
        self.add_status("Charlie setup as guardian".to_string());
    }

    /// Create chat group
    pub fn create_chat_group(&mut self, group_id: ChatGroupId) {
        self.chat_group = Some(group_id.clone());
        self.add_status(format!("Chat group created: {}", group_id));
    }

    /// Add message to history
    pub fn add_message(&mut self, message: ChatMessage) {
        self.message_history.push(message);
        // Keep only last 100 messages for display
        if self.message_history.len() > 100 {
            self.message_history.remove(0);
        }
    }

    /// Simulate data loss
    pub fn simulate_data_loss(&mut self) {
        self.add_status("WARNING: Simulating complete device data loss...".to_string());
        self.current_phase = DemoPhase::DataLoss;
        // Clear message history to simulate data loss
        self.message_history.clear();
    }

    /// Initiate recovery process
    pub fn initiate_recovery(&mut self) {
        self.recovery_progress.initiated = true;
        self.recovery_progress.threshold = 2; // 2-of-3 threshold
        self.recovery_progress.current_step = "Contacting guardians...".to_string();
        self.add_status("Recovery process initiated".to_string());
        self.current_phase = DemoPhase::Recovery;
    }

    /// Guardian approves recovery
    pub fn guardian_approves_recovery(&mut self, guardian_id: AuthorityId) {
        let guardian_name = if let Some(guardian) = self.guardian_states.get(&guardian_id) {
            guardian.name.clone()
        } else {
            return;
        };

        if let Some(guardian) = self.guardian_states.get_mut(&guardian_id) {
            if !guardian.approved_recovery {
                guardian.approved_recovery = true;
                self.recovery_progress.approvals += 1;

                // Update progress
                self.recovery_progress.completion_percent =
                    (self.recovery_progress.approvals * 100) / self.recovery_progress.threshold;

                if self.recovery_progress.approvals >= self.recovery_progress.threshold {
                    self.recovery_progress.current_step =
                        "Threshold reached! Restoring data...".to_string();
                    self.recovery_progress.completion_percent = 100;
                    self.add_status("Recovery threshold reached!".to_string());
                }
            }
        }

        self.add_status(format!("{} approved recovery", guardian_name));
    }

    /// Complete recovery and restore messages
    pub fn complete_recovery(&mut self, restored_messages: Vec<ChatMessage>) {
        self.message_history = restored_messages;
        self.current_phase = DemoPhase::Restoration;
        self.add_status(format!(
            "Recovery complete! Restored {} messages",
            self.message_history.len()
        ));
    }

    /// Get phase description for UI
    pub fn phase_description(&self) -> &'static str {
        use DemoPhase::*;
        match self.current_phase {
            Welcome => "Welcome to Aura - Threshold Identity Demo",
            BobOnboarding => "Bob's Onboarding - Creating Identity",
            GuardianSetup => "Guardian Setup - Alice & Charlie",
            GroupChat => "Group Chat - Messaging with Friends",
            DataLoss => "Data Loss - Device Failure Simulation",
            Recovery => "Guardian Recovery - Social Key Recovery",
            Restoration => "Data Restoration - Message History Recovery",
            Completed => "Demo Complete - Bob's Journey Finished",
        }
    }

    /// Get recovery status description
    pub fn recovery_status(&self) -> String {
        if self.recovery_progress.initiated {
            format!(
                "{} ({}/{})",
                self.recovery_progress.current_step,
                self.recovery_progress.approvals,
                self.recovery_progress.threshold
            )
        } else {
            "Not initiated".to_string()
        }
    }

    fn now_ts() -> TimeStamp {
        #[allow(clippy::disallowed_methods)]
        let ts_ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
        TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms,
            uncertainty: None,
        })
    }
}

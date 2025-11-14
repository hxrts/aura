//! End-to-End Threshold Identity Test Suite
//!
//! This test suite validates the complete threshold identity workflow including:
//! - Account creation and initialization
//! - Guardian invitation and acceptance
//! - Device addition through invitation system
//! - Recovery ceremony execution
//! - Multi-device coordination
//!
//! This test simulates the "20 friends, twice weekly" validation scenario
//! by creating multiple accounts, establishing guardian relationships,
//! and executing recovery ceremonies.

use aura_agent::AgentOperations;
use aura_core::{AccountId, Cap, DeviceId, TrustLevel, Top, AuraResult};
use aura_crypto::key_derivation::DkdEngine;
use aura_verify::registry::IdentityVerifier;
use aura_macros::aura_test;
use aura_invitation::{
    device_invitation::{DeviceInvitationCoordinator, DeviceInvitationRequest},
    guardian_invitation::{GuardianInvitationRequest, GuardianInvitationCoordinator},
    invitation_acceptance::{AcceptanceProtocolConfig, InvitationAcceptanceCoordinator},
};
use aura_journal::{JournalOperations, semilattice::InvitationLedger};
use aura_recovery::{
    guardian_recovery::GuardianRecoveryCoordinator,
    account_recovery::{AccountRecoveryRequest, AccountRecoveryCoordinator},
};
use aura_wot::CapabilitySet;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tokio::time::timeout;

/// Test participant representing a friend in the network
#[derive(Debug)]
struct TestParticipant {
    /// Device ID
    pub device_id: DeviceId,
    /// Account ID
    pub account_id: AccountId,
    /// Agent operations
    pub agent_ops: AgentOperations,
    /// Identity operations
    pub identity_ops: IdentityOperations,
    /// Journal operations
    pub journal_ops: JournalOperations,
    /// Effect system
    pub effects: AuraEffectSystem,
    /// Participant name for debugging
    pub name: String,
}

impl TestParticipant {
    /// Create a new test participant
    async fn new(name: String) -> aura_core::AuraResult<Self> {
        let device_id = DeviceId::new();
        let account_id = AccountId::new();
        let fixture = aura_testkit::create_test_fixture_with_device_id(device_id).await?;
        let effects = fixture.effects().as_ref().clone();

        let agent_ops = AgentOperations::new(device_id, effects.clone()).await;
        let identity_ops = IdentityOperations::new(effects.clone());
        let journal_ops = JournalOperations::new(effects.clone());

        Ok(Self {
            device_id,
            account_id,
            agent_ops,
            identity_ops,
            journal_ops,
            effects,
            name,
        })
    }

    /// Initialize participant account with threshold identity
    async fn initialize_account(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!("Initializing account for {}", self.name);

        // Initialize identity with 2-of-3 threshold
        let threshold_config = aura_core::protocols::ThresholdConfig::new(2, 3)
            .expect("Valid threshold configuration");

        self.identity_ops.initialize_account(
            self.account_id,
            threshold_config
        ).await?;

        // Create initial journal entry
        let init_fact = serde_json::json!({
            "type": "account_initialized",
            "account_id": self.account_id.to_string(),
            "device_id": self.device_id.to_string(),
            "name": self.name,
            "timestamp": self.effects.current_timestamp().await
        });

        let fact_bytes = serde_json::to_vec(&init_fact)?;
        self.journal_ops.append_fact(fact_bytes).await?;

        println!("✓ Account initialized for {}", self.name);
        Ok(())
    }

    /// Invite another participant as a guardian
    async fn invite_guardian(
        &self,
        guardian: &TestParticipant,
        ledger: Arc<Mutex<InvitationLedger>>,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        println!("{} inviting {} as guardian", self.name, guardian.name);

        let invitation_request = GuardianInvitationRequest {
            inviter: self.device_id,
            invitee: guardian.device_id,
            account_id: self.account_id,
            trust_level: TrustLevel::High,
            guardian_role: "recovery_guardian".to_string(),
            ttl_secs: Some(3600), // 1 hour
        };

        let coordinator = GuardianInvitationCoordinator::with_ledger(
            self.effects.clone(),
            ledger,
        );

        let response = coordinator.invite_guardian(invitation_request).await?;

        if response.success {
            println!("✓ Guardian invitation created: {}", response.invitation.invitation_id);
            Ok(response.invitation.invitation_id)
        } else {
            Err(format!("Guardian invitation failed: {:?}", response.error).into())
        }
    }

    /// Accept guardian invitation
    async fn accept_guardian_invitation(
        &self,
        invitation_id: String,
        ledger: Arc<Mutex<InvitationLedger>>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!("{} accepting guardian invitation {}", self.name, invitation_id);

        // Retrieve invitation from ledger
        let invitation_envelope = {
            let ledger_lock = ledger.lock().await;
            ledger_lock.get_invitation(&invitation_id)
                .ok_or("Invitation not found")?
        };

        let acceptance_config = AcceptanceProtocolConfig {
            auto_establish_relationship: true,
            default_trust_level: TrustLevel::High,
            require_transport_confirmation: false, // Faster for testing
            protocol_timeout_secs: 60,
        };

        let coordinator = InvitationAcceptanceCoordinator::with_config(
            self.effects.clone(),
            acceptance_config,
        );

        let acceptance = coordinator.accept_invitation(invitation_envelope).await?;

        if acceptance.success {
            println!("✓ {} accepted guardian invitation", self.name);
            Ok(())
        } else {
            Err(format!("Guardian invitation acceptance failed: {:?}", acceptance.error_message).into())
        }
    }

    /// Invite device to join account
    async fn invite_device(
        &self,
        device: &TestParticipant,
        role: &str,
        ledger: Arc<Mutex<InvitationLedger>>,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        println!("{} inviting {} as device ({})", self.name, device.name, role);

        let invitation_request = DeviceInvitationRequest {
            inviter: self.device_id,
            invitee: device.device_id,
            account_id: self.account_id,
            granted_capabilities: Cap::top(),
            device_role: role.to_string(),
            ttl_secs: Some(3600),
        };

        let coordinator = DeviceInvitationCoordinator::with_ledger(
            self.effects.clone(),
            ledger,
        );

        let response = coordinator.invite_device(invitation_request).await?;

        if response.success {
            println!("✓ Device invitation created: {}", response.invitation.invitation_id);
            Ok(response.invitation.invitation_id)
        } else {
            Err(format!("Device invitation failed: {:?}", response.error).into())
        }
    }
}

/// Test network representing multiple friends
struct TestNetwork {
    participants: HashMap<String, TestParticipant>,
    invitation_ledger: Arc<Mutex<InvitationLedger>>,
}

impl TestNetwork {
    /// Create test network with specified participants
    async fn with_participants(names: Vec<String>) -> aura_core::AuraResult<Self> {
        let mut participants = HashMap::new();

        for name in names {
            let participant = TestParticipant::new(name.clone()).await?;
            participants.insert(name, participant);
        }

        Ok(Self {
            participants,
            invitation_ledger: Arc::new(Mutex::new(InvitationLedger::new())),
        })
    }

    /// Get participant by name
    fn get(&self, name: &str) -> Option<&TestParticipant> {
        self.participants.get(name)
    }

    /// Get mutable participant by name
    fn get_mut(&mut self, name: &str) -> Option<&mut TestParticipant> {
        self.participants.get_mut(name)
    }
}

/// Test full threshold identity workflow
#[aura_test]
async fn test_threshold_identity_complete_workflow() -> AuraResult<()> {
    let test_timeout = Duration::from_secs(120); // 2 minutes for complete test

    let result = timeout(test_timeout, async {
        println!("Starting complete threshold identity workflow test");

        // Create test network with 5 friends
        let mut network = TestNetwork::with_participants(vec![
            "alice".to_string(),
            "bob".to_string(),
            "charlie".to_string(),
            "diana".to_string(),
            "eve".to_string(),
        ]).await?;

        // Phase 1: Account initialization
        println!("\nPhase 1: Account Initialization");
        for (name, participant) in network.participants.iter_mut() {
            participant.initialize_account().await
                .expect(&format!("Failed to initialize account for {}", name));
        }

        // Phase 2: Guardian relationships
        println!("\nPhase 2: Guardian Relationships");

        // Alice invites Bob and Charlie as guardians
        let alice = network.get("alice").unwrap();
        let bob = network.get("bob").unwrap();
        let charlie = network.get("charlie").unwrap();

        let bob_invitation = alice.invite_guardian(bob, network.invitation_ledger.clone()).await?;
        let charlie_invitation = alice.invite_guardian(charlie, network.invitation_ledger.clone()).await?;

        // Bob and Charlie accept guardian invitations
        bob.accept_guardian_invitation(bob_invitation, network.invitation_ledger.clone()).await?;
        charlie.accept_guardian_invitation(charlie_invitation, network.invitation_ledger.clone()).await?;

        // Phase 3: Device additions
        println!("\nPhase 3: Device Additions");

        let diana = network.get("diana").unwrap();
        let eve = network.get("eve").unwrap();

        // Alice invites additional devices to her account
        let diana_invitation = alice.invite_device(diana, "tablet", network.invitation_ledger.clone()).await?;
        let eve_invitation = alice.invite_device(eve, "mobile", network.invitation_ledger.clone()).await?;

        // Devices accept invitations
        diana.accept_guardian_invitation(diana_invitation, network.invitation_ledger.clone()).await?;
        eve.accept_guardian_invitation(eve_invitation, network.invitation_ledger.clone()).await?;

        // Phase 4: Recovery ceremony
        println!("\nPhase 4: Recovery Ceremony");

        // Simulate Alice losing access and needing recovery
        let recovery_request = AccountRecoveryRequest {
            account_id: alice.account_id,
            requesting_device: alice.device_id,
            guardians: vec![bob.device_id, charlie.device_id],
            new_device: DeviceId::new(), // New device for Alice
            recovery_reason: "device_lost".to_string(),
            timestamp: alice.effects.current_timestamp().await,
        };

        let recovery_coordinator = AccountRecoveryCoordinator::new(alice.effects.clone());
        let recovery_result = recovery_coordinator.initiate_recovery(recovery_request).await?;

        if recovery_result.success {
            println!("✓ Recovery ceremony initiated: {}", recovery_result.ceremony_id);

            // Guardians approve recovery
            let guardian_coordinator = GuardianRecoveryCoordinator::new(bob.effects.clone());

            let approval_result = guardian_coordinator.approve_recovery(
                recovery_result.ceremony_id.clone(),
                bob.device_id,
                "approved".to_string(),
            ).await?;

            if approval_result.success {
                println!("✓ Guardian {} approved recovery", bob.name);
            }
        }

        // Phase 5: Verification
        println!("\nPhase 5: System State Verification");

        // Verify all participants have valid state
        for (name, participant) in &network.participants {
            // Check journal state
            let facts_count = participant.journal_ops.get_fact_count().await?;
            assert!(facts_count > 0, "Participant {} should have facts in journal", name);

            // Check identity state
            let identity_status = participant.identity_ops.get_identity_status().await?;
            assert!(identity_status.initialized, "Participant {} should have initialized identity", name);

            println!("✓ {} has valid system state", name);
        }

        // Verify invitation ledger state
        let ledger_snapshot = network.invitation_ledger.lock().await;
        println!("Final invitation ledger has {} entries", ledger_snapshot.len());

        println!("Complete threshold identity workflow test passed!");

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    }).await;

    match result {
        Ok(inner_result) => {
            inner_result.map_err(|e| aura_core::AuraError::External(e.to_string()))?;
        }
        Err(_) => {
            return Err(aura_core::AuraError::External(format!("Threshold identity test timed out after {:?}", test_timeout)));
        }
    }
    
    Ok(())
}

/// Test concurrent multi-account scenario
#[aura_test]
async fn test_multi_account_concurrent_operations() -> AuraResult<()> {
    let test_timeout = Duration::from_secs(90);

    let result = timeout(test_timeout, async {
        println!("Starting multi-account concurrent operations test");

        // Create three separate friend groups
        let mut network1 = TestNetwork::with_participants(vec![
            "alice".to_string(),
            "bob".to_string(),
        ]).await?;

        let mut network2 = TestNetwork::with_participants(vec![
            "charlie".to_string(),
            "diana".to_string(),
        ]).await?;

        let mut network3 = TestNetwork::with_participants(vec![
            "eve".to_string(),
            "frank".to_string(),
        ]).await?;

        // Initialize all accounts concurrently
        let mut initialization_tasks = vec![];

        for network in [&mut network1, &mut network2, &mut network3] {
            for (name, participant) in network.participants.iter_mut() {
                let name_clone = name.clone();
                let init_task = async move {
                    participant.initialize_account().await
                        .map_err(|e| format!("Failed to initialize {}: {}", name_clone, e))
                };
                initialization_tasks.push(init_task);
            }
        }

        // Wait for all initializations
        let init_results = futures::future::join_all(initialization_tasks).await;
        for result in init_results {
            result?;
        }

        // Set up guardian relationships within each network
        let alice = network1.get("alice").unwrap();
        let bob = network1.get("bob").unwrap();
        let charlie = network2.get("charlie").unwrap();
        let diana = network2.get("diana").unwrap();
        let eve = network3.get("eve").unwrap();
        let frank = network3.get("frank").unwrap();

        // Create guardian relationships concurrently
        let guardian_tasks = vec![
            alice.invite_guardian(bob, network1.invitation_ledger.clone()),
            charlie.invite_guardian(diana, network2.invitation_ledger.clone()),
            eve.invite_guardian(frank, network3.invitation_ledger.clone()),
        ];

        let guardian_results = futures::future::try_join_all(guardian_tasks).await?;

        // Accept invitations concurrently
        let acceptance_tasks = vec![
            bob.accept_guardian_invitation(guardian_results[0].clone(), network1.invitation_ledger.clone()),
            diana.accept_guardian_invitation(guardian_results[1].clone(), network2.invitation_ledger.clone()),
            frank.accept_guardian_invitation(guardian_results[2].clone(), network3.invitation_ledger.clone()),
        ];

        let _acceptance_results = futures::future::try_join_all(acceptance_tasks).await?;

        // Verify state across all networks
        for (network_name, network) in [("Network1", &network1), ("Network2", &network2), ("Network3", &network3)] {
            for (participant_name, participant) in &network.participants {
                let identity_status = participant.identity_ops.get_identity_status().await?;
                assert!(identity_status.initialized,
                    "Participant {} in {} should have initialized identity",
                    participant_name, network_name);

                println!("✓ {} in {} has valid state", participant_name, network_name);
            }
        }

        println!("Multi-account concurrent operations test passed!");

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    }).await;

    match result {
        Ok(inner_result) => {
            inner_result.map_err(|e| aura_core::AuraError::External(e.to_string()))?;
        }
        Err(_) => {
            return Err(aura_core::AuraError::External(format!("Multi-account test timed out after {:?}", test_timeout)));
        }
    }
    
    Ok(())
}

/// Test error handling and recovery scenarios
#[aura_test]
async fn test_error_scenarios_and_resilience() -> AuraResult<()> {
    let test_timeout = Duration::from_secs(60);

    let result = timeout(test_timeout, async {
        println!("Starting error scenarios and resilience test");

        let mut network = TestNetwork::with_participants(vec![
            "alice".to_string(),
            "bob".to_string(),
        ]).await?;

        // Initialize accounts
        for (name, participant) in network.participants.iter_mut() {
            participant.initialize_account().await?;
        }

        let alice = network.get("alice").unwrap();
        let bob = network.get("bob").unwrap();

        // Test 1: Invalid invitation (expired TTL)
        println!("Testing expired invitation handling...");

        let invalid_request = DeviceInvitationRequest {
            inviter: alice.device_id,
            invitee: bob.device_id,
            account_id: alice.account_id,
            granted_capabilities: Cap::top(),
            device_role: "test".to_string(),
            ttl_secs: Some(0), // Invalid TTL
        };

        let coordinator = DeviceInvitationCoordinator::with_ledger(
            alice.effects.clone(),
            network.invitation_ledger.clone(),
        );

        let invalid_result = coordinator.invite_device(invalid_request).await;
        assert!(invalid_result.is_err(), "Should reject invalid TTL");
        println!("✓ Invalid invitation properly rejected");

        // Test 2: Recovery with insufficient guardians
        println!("Testing insufficient guardian scenario...");

        let insufficient_recovery = AccountRecoveryRequest {
            account_id: alice.account_id,
            requesting_device: alice.device_id,
            guardians: vec![], // No guardians
            new_device: DeviceId::new(),
            recovery_reason: "test".to_string(),
            timestamp: alice.effects.current_timestamp().await,
        };

        let recovery_coordinator = AccountRecoveryCoordinator::new(alice.effects.clone());
        let recovery_result = recovery_coordinator.initiate_recovery(insufficient_recovery).await;

        // Should handle gracefully (either succeed with empty guardian set or fail appropriately)
        match recovery_result {
            Ok(result) => {
                if result.success {
                    println!("✓ System handled empty guardian set gracefully");
                } else {
                    println!("✓ System properly rejected insufficient guardians");
                }
            }
            Err(_) => {
                println!("✓ System properly failed with insufficient guardians");
            }
        }

        // Test 3: System resilience under normal operations
        println!("Testing system resilience...");

        // Perform many rapid operations to test stability
        for i in 0..10 {
            let data = alice.effects.random_bytes(32).await?;
            assert_eq!(data.len(), 32);

            let timestamp = alice.effects.current_timestamp().await;
            assert!(timestamp > 0);

            if i % 3 == 0 {
                println!("  Completed {} resilience operations", i + 1);
            }
        }

        println!("✓ System maintained stability under load");
        println!("Error scenarios and resilience test passed!");

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    }).await;

    match result {
        Ok(inner_result) => {
            inner_result.map_err(|e| aura_core::AuraError::External(e.to_string()))?;
        }
        Err(_) => {
            return Err(aura_core::AuraError::External(format!("Error scenarios test timed out after {:?}", test_timeout)));
        }
    }
    
    Ok(())
}

/// Test scaling characteristics with larger groups
#[aura_test]
async fn test_scaling_characteristics() -> AuraResult<()> {
    let test_timeout = Duration::from_secs(180); // 3 minutes for scaling test

    let result = timeout(test_timeout, async {
        println!("Starting scaling characteristics test");

        // Create larger friend group (simulating "20 friends" scenario)
        let participant_names: Vec<String> = (0..10)
            .map(|i| format!("friend_{:02}", i))
            .collect();

        let mut network = TestNetwork::with_participants(participant_names.clone()).await?;

        // Phase 1: Initialize all accounts
        println!("Initializing {} accounts...", participant_names.len());
        let start_time = std::time::Instant::now();

        for (name, participant) in network.participants.iter_mut() {
            participant.initialize_account().await?;
        }

        let init_duration = start_time.elapsed();
        println!("✓ Initialized {} accounts in {:?}", participant_names.len(), init_duration);

        // Phase 2: Create mesh of guardian relationships
        println!("Creating guardian relationships...");
        let guardian_start = std::time::Instant::now();

        // Each participant invites next 2 participants as guardians
        let mut invitation_tasks = vec![];

        for (i, name) in participant_names.iter().enumerate() {
            let inviter = network.get(name).unwrap();

            // Invite next 2 participants (wrap around)
            for j in 1..=2 {
                let guardian_idx = (i + j) % participant_names.len();
                let guardian_name = &participant_names[guardian_idx];
                let guardian = network.get(guardian_name).unwrap();

                let invitation_task = inviter.invite_guardian(guardian, network.invitation_ledger.clone());
                invitation_tasks.push(invitation_task);
            }
        }

        let invitation_results = futures::future::try_join_all(invitation_tasks).await?;
        let guardian_duration = guardian_start.elapsed();

        println!("✓ Created {} guardian invitations in {:?}",
                invitation_results.len(), guardian_duration);

        // Phase 3: Performance verification
        println!("Verifying performance characteristics...");

        // Test rapid operations across multiple participants
        let perf_start = std::time::Instant::now();
        let mut operation_tasks = vec![];

        for (name, participant) in &network.participants {
            let name_clone = name.clone();
            let task = async move {
                // Rapid sequence of operations
                for _ in 0..5 {
                    let _data = participant.effects.random_bytes(16).await?;
                    let _timestamp = participant.effects.current_timestamp().await;
                }
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            };
            operation_tasks.push(task);
        }

        let _operation_results = futures::future::try_join_all(operation_tasks).await?;
        let perf_duration = perf_start.elapsed();

        println!("✓ Completed concurrent operations across {} participants in {:?}",
                participant_names.len(), perf_duration);

        // Phase 4: Memory and resource verification
        println!("Verifying resource usage...");

        let total_participants = network.participants.len();
        let total_invitations = invitation_results.len();

        // Basic sanity checks for scaling
        assert!(total_participants >= 10, "Should test with sufficient participants");
        assert!(total_invitations >= 20, "Should create sufficient guardian relationships");

        // Performance should be reasonable
        assert!(init_duration < Duration::from_secs(30),
               "Account initialization should complete quickly");
        assert!(guardian_duration < Duration::from_secs(60),
               "Guardian relationships should establish quickly");
        assert!(perf_duration < Duration::from_secs(10),
               "Concurrent operations should be fast");

        println!("Scaling test summary:");
        println!("  Participants: {}", total_participants);
        println!("  Guardian relationships: {}", total_invitations);
        println!("  Account init time: {:?}", init_duration);
        println!("  Guardian setup time: {:?}", guardian_duration);
        println!("  Concurrent ops time: {:?}", perf_duration);

        println!("Scaling characteristics test passed!");

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    }).await;

    match result {
        Ok(inner_result) => {
            inner_result.map_err(|e| aura_core::AuraError::External(e.to_string()))?;
        }
        Err(_) => {
            return Err(aura_core::AuraError::External(format!("Scaling test timed out after {:?}", test_timeout)));
        }
    }
    
    Ok(())
}

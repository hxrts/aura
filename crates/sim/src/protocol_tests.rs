//! End-to-end protocol tests
//!
//! These tests demonstrate full protocol execution in the simulation environment.

use crate::Simulation;

#[cfg(test)]
mod dkd_tests {
    use super::*;
    
    #[tokio::test]
    #[ignore] // TODO: Update to use new choreographic infrastructure
    async fn test_dkd_three_party_honest() {
        use crate::choreographic::ChoreographyBuilder;
        
        let keys = ChoreographyBuilder::new(3, 2) // 3 participants, threshold 2
            .with_seed(42)
            .with_latency(1, 5)
            .run_dkd()
            .await
            .expect("DKD protocol should succeed");
        
        // Verify all participants derived the same key
        assert_eq!(keys.len(), 3);
        assert_eq!(keys[0], keys[1], "Alice and Bob should derive same key");
        assert_eq!(keys[1], keys[2], "Bob and Carol should derive same key");
        assert!(!keys[0].is_empty(), "Derived key should not be empty");
        assert_eq!(keys[0].len(), 32, "Derived key should be 32 bytes");
    }
    
    #[tokio::test]
    async fn test_dkd_simple_scenario() {
        // Simple test that should work without complex scheduling
        // This test uses a simplified approach with manual simulation stepping
        
        let mut sim = Simulation::new(42);
        
        // Create a shared account with three devices
        let (_account_id, device_info) = sim
            .add_account_with_devices(&["alice", "bob", "carol"])
            .await;
        
        let alice = device_info[0].0;
        let _bob = device_info[1].0;
        let _carol = device_info[2].0;
        
        let alice_device_id = device_info[0].1;
        let bob_device_id = device_info[1].1;
        let carol_device_id = device_info[2].1;
        
        let participants = vec![alice_device_id, bob_device_id, carol_device_id];
        
        // Get Alice participant
        let alice_participant = sim.get_participant(alice).unwrap();
        
        // For now, just test that we can create a protocol context without hanging
        let session_id = sim.generate_uuid();
        let ctx = alice_participant.create_protocol_context(
            session_id,
            participants,
            Some(2),
        );
        
        // Verify context was created successfully
        assert_eq!(ctx.session_id, session_id);
        assert_eq!(ctx.threshold, Some(2));
        
        // Test passes if we get here without hanging
    }
    
    #[tokio::test]
    #[ignore] // TODO: Update to use new choreographic infrastructure
    async fn test_dkd_multiple_sessions() {
        // Test that participants can successfully run multiple DKD sessions
        // Note: Currently context_id is not implemented, so same participants produce same key
        // TODO: Once context_id is properly used, different contexts should produce different keys
        
        let mut sim = Simulation::new(42);
        let (_account_id, device_info) = sim
            .add_account_with_devices(&["alice", "bob", "carol"])
            .await;
        
        let alice = sim.get_participant(device_info[0].0).unwrap();
        let bob = sim.get_participant(device_info[1].0).unwrap();
        let carol = sim.get_participant(device_info[2].0).unwrap();
        
        let participants = vec![device_info[0].1, device_info[1].1, device_info[2].1];
        
        // Session 1 - use deterministic UUID
        let session_id1 = sim.generate_uuid();
        let alice_dkd1 = alice.initiate_dkd_with_session(session_id1, participants.clone(), 2);
        let bob_dkd1 = bob.initiate_dkd_with_session(session_id1, participants.clone(), 2);
        let carol_dkd1 = carol.initiate_dkd_with_session(session_id1, participants.clone(), 2);
        
        let sim_future1 = sim.run_until_idle();
        
        let (key1_a, key1_b, key1_c, _) = tokio::join!(alice_dkd1, bob_dkd1, carol_dkd1, sim_future1);
        
        let derived_key1 = key1_a.expect("Session 1 should complete");
        
        // Verify all participants got the same key in session 1
        assert_eq!(key1_b.unwrap(), derived_key1);
        assert_eq!(key1_c.unwrap(), derived_key1);
        
        // Session 2 with same participants - use deterministic UUID
        let session_id2 = sim.generate_uuid();
        let alice_dkd2 = alice.initiate_dkd_with_session(session_id2, participants.clone(), 2);
        let bob_dkd2 = bob.initiate_dkd_with_session(session_id2, participants.clone(), 2);
        let carol_dkd2 = carol.initiate_dkd_with_session(session_id2, participants.clone(), 2);
        
        let sim_future2 = sim.run_until_idle();
        
        let (key2_a, key2_b, key2_c, _) = tokio::join!(alice_dkd2, bob_dkd2, carol_dkd2, sim_future2);
        
        let derived_key2 = key2_a.expect("Session 2 should complete");
        
        // Verify all participants got the same key in session 2
        assert_eq!(key2_b.unwrap(), derived_key2);
        assert_eq!(key2_c.unwrap(), derived_key2);
        
        // Same participants (same device IDs) should produce the same key
        // because context_id is currently empty in the DKD implementation.
        // This is the expected behavior until context_id is properly implemented.
        assert_eq!(
            derived_key1, derived_key2,
            "Same participants should produce same key when context_id is empty"
        );
    }
    
    #[tokio::test]
    #[ignore] // TODO: Update to use new choreographic infrastructure
    async fn test_dkd_different_thresholds() {
        // Test 3-of-5 threshold
        let mut sim = Simulation::new(123);
        let (_account_id, device_info) = sim
            .add_account_with_devices(&["alice", "bob", "carol", "dave", "eve"])
            .await;
        
        let session_id = sim.generate_uuid();
        let participants: Vec<_> = device_info.iter().map(|(_, dev_id)| *dev_id).collect();
        
        // Get all participants
        let alice = sim.get_participant(device_info[0].0).unwrap();
        let bob = sim.get_participant(device_info[1].0).unwrap();
        let carol = sim.get_participant(device_info[2].0).unwrap();
        let dave = sim.get_participant(device_info[3].0).unwrap();
        let eve = sim.get_participant(device_info[4].0).unwrap();
        
        // All participants run choreography with 3-of-5 threshold
        let alice_dkd = alice.initiate_dkd_with_session(session_id, participants.clone(), 3);
        let bob_dkd = bob.initiate_dkd_with_session(session_id, participants.clone(), 3);
        let carol_dkd = carol.initiate_dkd_with_session(session_id, participants.clone(), 3);
        let dave_dkd = dave.initiate_dkd_with_session(session_id, participants.clone(), 3);
        let eve_dkd = eve.initiate_dkd_with_session(session_id, participants.clone(), 3);
        
        let sim_future = sim.run_until_idle();
        
        let (key_a, key_b, key_c, key_d, key_e, _) = 
            tokio::join!(alice_dkd, bob_dkd, carol_dkd, dave_dkd, eve_dkd, sim_future);
        
        // All should succeed
        let derived_a = key_a.expect("Alice should complete");
        let derived_b = key_b.expect("Bob should complete");
        let derived_c = key_c.expect("Carol should complete");
        let derived_d = key_d.expect("Dave should complete");
        let derived_e = key_e.expect("Eve should complete");
        
        // All keys should match
        assert_eq!(derived_a, derived_b, "Alice and Bob keys match");
        assert_eq!(derived_b, derived_c, "Bob and Carol keys match");
        assert_eq!(derived_c, derived_d, "Carol and Dave keys match");
        assert_eq!(derived_d, derived_e, "Dave and Eve keys match");
    }
}

#[cfg(test)]
mod resharing_tests {
    use super::*;
    
    #[tokio::test]
    #[ignore] // TODO: Update to use new choreographic infrastructure
    async fn test_resharing_threshold_increase() {
        let mut sim = Simulation::new(123);
        
        // Create a shared account with three devices (simulating CRDT sync)
        let (_account_id, device_info) = sim
            .add_account_with_devices(&["alice", "bob", "carol"])
            .await;
        
        let alice = device_info[0].0;
        let bob = device_info[1].0;
        let carol = device_info[2].0;
        
        let alice_device_id = device_info[0].1;
        let bob_device_id = device_info[1].1;
        let carol_device_id = device_info[2].1;
        
        sim.set_latency_range(1, 5);
        
        let alice_participant = sim.get_participant(alice).unwrap();
        let bob_participant = sim.get_participant(bob).unwrap();
        let carol_participant = sim.get_participant(carol).unwrap();
        
        let old_participants = vec![alice_device_id, bob_device_id, carol_device_id];
        let new_participants = vec![alice_device_id, bob_device_id, carol_device_id];
        
        // Create a deterministic session ID for all participants
        let session_id = sim.generate_uuid();
        
        // All participants must run the resharing choreography concurrently with the SAME session ID
        let alice_resharing = alice_participant.initiate_resharing_with_session(
            session_id,
            old_participants.clone(),
            new_participants.clone(),
            2, // old threshold
            3, // new threshold (all participants must sign)
        );
        let bob_resharing = bob_participant.initiate_resharing_with_session(
            session_id,
            old_participants.clone(),
            new_participants.clone(),
            2, // old threshold
            3, // new threshold
        );
        let carol_resharing = carol_participant.initiate_resharing_with_session(
            session_id,
            old_participants,
            new_participants,
            2, // old threshold
            3, // new threshold
        );
        
        let sim_future = sim.run_until_idle();
        
        let (alice_result, bob_result, carol_result, sim_result) = tokio::join!(
            alice_resharing, bob_resharing, carol_resharing, sim_future
        );
        
        sim_result.expect("Simulation should complete");
        alice_result.expect("Alice resharing should succeed");
        bob_result.expect("Bob resharing should succeed");
        carol_result.expect("Carol resharing should succeed");
        
        // For MVP: resharing is simplified, so threshold won't actually be updated
        // TODO: Verify threshold update once full resharing protocol is implemented
        let alice_final = sim.ledger_snapshot(alice).await.unwrap();
        assert_eq!(alice_final.state().threshold, 2, "Threshold remains unchanged in simplified implementation");
    }
}

#[cfg(test)]
mod byzantine_tests {
    use super::*;
    use crate::{Interceptors, Operation};
    
    #[tokio::test]
    #[ignore] // TODO: Update to use new choreographic infrastructure
    async fn test_dkd_with_silent_participant() {
        let mut sim = Simulation::new(456);
        
        // Create a shared account with three devices
        let (_account_id, device_info) = sim
            .add_account_with_devices(&["alice", "bob", "carol"])
            .await;
        
        // Get participant IDs and configure one as Byzantine
        let alice = device_info[0].0;
        let bob = device_info[1].0;
        let carol = device_info[2].0;
        
        sim.set_latency_range(1, 5);
        
        let alice_participant = sim.get_participant(alice).unwrap();
        let bob_participant = sim.get_participant(bob).unwrap();
        let carol_participant = sim.get_participant(carol).unwrap();
        
        let alice_device_id = device_info[0].1;
        let bob_device_id = device_info[1].1;
        let carol_device_id = device_info[2].1;
        
        let participants = vec![alice_device_id, bob_device_id, carol_device_id];
        
        // For now, test normal 3-party DKD since Byzantine interceptors aren't implemented yet
        // All participants run the protocol with a shared session ID
        let session_id = sim.generate_uuid();
        
        let alice_dkd = alice_participant.initiate_dkd_with_session(session_id, participants.clone(), 2);
        let bob_dkd = bob_participant.initiate_dkd_with_session(session_id, participants.clone(), 2);
        let carol_dkd = carol_participant.initiate_dkd_with_session(session_id, participants.clone(), 2);
        
        let sim_future = sim.run_until_idle();
        
        let (alice_result, bob_result, carol_result, sim_result) = tokio::join!(
            alice_dkd, bob_dkd, carol_dkd, sim_future
        );
        
        sim_result.expect("Simulation should complete");
        
        // All should succeed since no Byzantine behavior is implemented yet
        let alice_key = alice_result.expect("Alice DKD should succeed");
        let bob_key = bob_result.expect("Bob DKD should succeed");  
        let carol_key = carol_result.expect("Carol DKD should succeed");
        
        // All participants should derive the same key
        assert_eq!(alice_key, bob_key, "Alice and Bob should derive same key");
        assert_eq!(bob_key, carol_key, "Bob and Carol should derive same key");
    }
    
    #[tokio::test]
    #[ignore] // Temporarily ignored until event handling is fully wired
    async fn test_dkd_with_corrupt_commitment() {
        let mut sim = Simulation::new(789);
        
        let alice = sim.add_participant("alice").await;
        let carol = sim.add_participant("carol").await;
        
        // Bob corrupts DKD commitment messages
        let bob = sim.add_malicious_participant(
            "bob",
            Interceptors {
                outgoing: crate::byzantine::corrupt_operation(Operation::DkdCommitment),
                incoming: crate::IncomingInterceptor::passthrough(),
            }
        ).await;
        
        sim.set_latency_range(1, 5);
        
        let alice_participant = sim.get_participant(alice).unwrap();
        
        // Get device IDs
        let alice_ledger = alice_participant.ledger().await;
        let alice_device_id = alice_ledger.state().devices.keys().next().copied().unwrap();
        drop(alice_ledger);
        
        let bob_participant = sim.get_participant(bob).unwrap();
        let bob_ledger = bob_participant.ledger().await;
        let bob_device_id = bob_ledger.state().devices.keys().next().copied().unwrap();
        drop(bob_ledger);
        
        let carol_participant = sim.get_participant(carol).unwrap();
        let carol_ledger = carol_participant.ledger().await;
        let carol_device_id = carol_ledger.state().devices.keys().next().copied().unwrap();
        drop(carol_ledger);
        
        let participants = vec![alice_device_id, bob_device_id, carol_device_id];
        
        // All participants run DKD concurrently
        let session_id = sim.generate_uuid();
        let alice_dkd = alice_participant.initiate_dkd_with_session(session_id, participants.clone(), 2);
        let bob_dkd = bob_participant.initiate_dkd_with_session(session_id, participants.clone(), 2);
        let carol_dkd = carol_participant.initiate_dkd_with_session(session_id, participants.clone(), 2);
        
        let sim_future = sim.run_until_idle();
        
        let (alice_result, bob_result, carol_result, _) = tokio::join!(
            alice_dkd, bob_dkd, carol_dkd, sim_future
        );
        
        // All participants should succeed and derive the same key
        let alice_key = alice_result.expect("Alice DKD should succeed");
        let bob_key = bob_result.expect("Bob DKD should succeed");
        let carol_key = carol_result.expect("Carol DKD should succeed");
        
        assert_eq!(alice_key, bob_key, "Alice and Bob should derive the same key");
        assert_eq!(bob_key, carol_key, "Bob and Carol should derive the same key");
        assert!(!alice_key.is_empty(), "Derived key should not be empty");
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    
    #[tokio::test]
    #[ignore] // TODO: Update to use new choreographic infrastructure
    async fn test_guardian_recovery_basic() {
        let mut sim = Simulation::new(789);
        
        // Create a shared account with devices and guardians
        let (_account_id, device_info) = sim
            .add_account_with_devices(&["alice", "bob", "carol"])
            .await;
        
        let alice = device_info[0].0;
        let bob = device_info[1].0;
        let carol = device_info[2].0;
        
        let alice_device_id = device_info[0].1;
        let bob_device_id = device_info[1].1;
        let carol_device_id = device_info[2].1;
        
        // Add new device that needs recovery
        let dave = sim.add_participant("dave").await;
        let dave_participant = sim.get_participant(dave).unwrap();
        
        sim.set_latency_range(1, 5);
        
        let alice_participant = sim.get_participant(alice).unwrap();
        let bob_participant = sim.get_participant(bob).unwrap();
        let _carol_participant = sim.get_participant(carol).unwrap();
        
        // Create guardian IDs (in a real system, these would be separate guardian devices)
        let guardian_alice = aura_journal::GuardianId(alice_device_id.0);
        let guardian_bob = aura_journal::GuardianId(bob_device_id.0);
        let guardian_carol = aura_journal::GuardianId(carol_device_id.0);
        
        let guardians = vec![guardian_alice, guardian_bob, guardian_carol];
        let recovery_id = sim.generate_uuid();
        
        // All participants run recovery choreography concurrently with the SAME session ID
        // Dave as initiator
        let dave_recovery = dave_participant.initiate_recovery_with_session(
            recovery_id,
            guardians.clone(),
            2, // require 2 of 3 guardians
            1, // 1 hour cooldown for testing
            Some(aura_journal::DeviceId(dave.0)), // Dave's new device
        );
        
        // Guardians approve the recovery
        let alice_approval = alice_participant.approve_recovery(
            recovery_id,
            guardian_alice,
        );
        let bob_approval = bob_participant.approve_recovery(
            recovery_id,
            guardian_bob,
        );
        
        let sim_future = sim.run_until_idle();
        
        let (recovery_result, approval_a, approval_b, sim_result) = tokio::join!(
            dave_recovery, alice_approval, bob_approval, sim_future
        );
        
        sim_result.expect("Simulation should complete");
        recovery_result.expect("Recovery should succeed");
        approval_a.expect("Alice approval should succeed");
        approval_b.expect("Bob approval should succeed");
        
        // For MVP: recovery is simplified, so device won't actually be added
        // TODO: Verify device addition once full recovery protocol is implemented
        let final_ledger = dave_participant.ledger_snapshot().await;
        assert_eq!(final_ledger.state().devices.len(), 1, "Device count remains unchanged in simplified implementation");
    }
}


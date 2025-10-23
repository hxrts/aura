// Initialize a new account with session-based DKG ceremony

use aura_agent::IdentityConfig;
use aura_coordination::{KeyShare, ParticipantId};
use aura_journal::{
    AccountId, DeviceId, DeviceMetadata, DeviceType, AccountState, AccountLedger, 
    Session, ProtocolType, SessionStatus, ParticipantId as JournalParticipantId
};
use frost_ed25519 as frost;
use tracing::{info, debug};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

pub async fn run(participants: u16, threshold: u16, output_dir: &str) -> anyhow::Result<()> {
    info!("Initializing new Aura account with session-based genesis DKG");
    info!("Configuration: {}-of-{} threshold with {} participants", threshold, participants, participants);
    
    // Create output directory
    std::fs::create_dir_all(output_dir)?;
    
    // Validate parameters
    if participants < 2 {
        return Err(anyhow::anyhow!("Minimum 2 participants required"));
    }
    
    if threshold > participants {
        return Err(anyhow::anyhow!("Threshold cannot exceed participant count"));
    }
    
    // Step 1: Setup - Create empty in-memory CRDT document
    info!("Step 1: Setting up in-memory CRDT for session-based genesis");
    
    // Create account and device IDs for participants
    let account_id = AccountId::new();
    let mut device_ids = Vec::new();
    let mut device_metadatas = Vec::new();
    
    // Generate placeholder group public key (will be replaced by DKG)
    let placeholder_key = ed25519_dalek::VerifyingKey::from_bytes(&[0u8; 32])?;
    
    // Create device metadata for all participants
    for i in 0..participants {
        let device_id = DeviceId::new();
        let device = DeviceMetadata {
            device_id,
            device_name: format!("Device {}", i + 1),
            device_type: DeviceType::Native,
            public_key: placeholder_key, // Will be updated after DKG
            added_at: current_timestamp(),
            last_seen: current_timestamp(),
            dkd_commitment_proofs: std::collections::BTreeMap::new(),
        };
        device_ids.push(device_id);
        device_metadatas.push(device);
    }
    
    // Create initial account state with first device
    let mut initial_state = AccountState::new(
        account_id,
        placeholder_key,
        device_metadatas[0].clone(),
        threshold,
        participants,
    );
    
    // Add remaining devices to state
    for device in device_metadatas.iter().skip(1) {
        initial_state.add_device(device.clone()).map_err(|e| anyhow::anyhow!("Failed to add device: {:?}", e))?;
    }
    
    // Create shared ledger
    let shared_ledger = Arc::new(RwLock::new(AccountLedger::new(initial_state)?));
    
    // Step 2: Create Genesis Session
    info!("Step 2: Creating Genesis DKG session");
    
    let session_id = Uuid::new_v4();
    let genesis_participants: Vec<JournalParticipantId> = device_ids.iter()
        .map(|device_id| JournalParticipantId::Device(*device_id))
        .collect();
    
    let genesis_session = Session::new(
        session_id,
        ProtocolType::GenesisDkg,
        genesis_participants,
        1, // Start epoch
        100, // TTL in epochs - genesis can take time
        current_timestamp(),
    );
    
    // Add genesis session to ledger
    {
        let mut ledger = shared_ledger.write().await;
        ledger.add_session(genesis_session);
        debug!("Added genesis session {} to ledger", session_id);
    }
    
    // Step 3: Instantiate Participants (DeviceAgent instances)
    info!("Step 3: Instantiating {} DeviceAgent instances", participants);
    
    // For MVP, we simulate the P2P DKG by using a trusted dealer
    // In production, this would be a true distributed protocol
    let (frost_shares, pubkey_package) = {
        use rand::thread_rng;
        let mut rng = thread_rng();
        
        frost::keys::generate_with_dealer(
            threshold,
            participants,
            frost::keys::IdentifierList::Default,
            &mut rng,
        )?
    };
    
    // Convert FROST shares to our KeyShare format - only save primary device config
    let (_frost_participant_id, secret_share) = frost_shares.into_iter().next().unwrap();
    let primary_device_id = device_ids[0];
    let key_package = frost::keys::KeyPackage::try_from(secret_share)?;
    
    // Convert FROST identifier to u16 (FROST identifiers start from 1)
    // For simplicity, use the first participant (ID = 1)
    let participant_id = ParticipantId::from_u16_unchecked(1);
    
    let primary_key_share = KeyShare {
        participant_id,
        share: key_package,
        threshold,
        total_participants: participants,
    };
    
    let primary_config = IdentityConfig {
        device_id: primary_device_id,
        account_id,
        participant_id,
        share_path: format!("{}/key_share_0.cbor", output_dir),
        threshold,
        total_participants: participants,
    };
    
    // Step 4: Automatic Execution - Simulate the DKG completion
    info!("Step 4: Executing genesis DKG protocol (simulated for MVP)");
    
    // In a real implementation, DeviceAgent instances would see the genesis session
    // and automatically execute the DKG choreography. For MVP, we simulate success.
    
    // Update session status to Active
    {
        let mut ledger = shared_ledger.write().await;
        ledger.update_session_status(session_id, SessionStatus::Active)?;
        debug!("Updated genesis session status to Active");
    }
    
    // Simulate DKG completion by updating the group public key
    let frost_vk = pubkey_package.verifying_key();
    let group_public_key = ed25519_dalek::VerifyingKey::from_bytes(&frost_vk.serialize())?;
    
    // Update ledger with final group public key
    // Note: In production, this would be done through proper event application
    // For MVP init command, we directly modify the state
    {
        let mut ledger = shared_ledger.write().await;
        // We need to use a different approach since state_mut() is only available in tests
        // For now, we'll recreate the ledger with updated state
        let mut updated_state = ledger.state().clone();
        updated_state.group_public_key = group_public_key;
        
        // Update all device public keys to the group public key
        for device in updated_state.devices.values_mut() {
            device.public_key = group_public_key;
        }
        
        // Replace the ledger with updated state
        *ledger = AccountLedger::new(updated_state)?;
        
        debug!("Updated group public key in account state");
    }
    
    // Step 5: Completion - Mark session as completed
    info!("Step 5: Completing genesis session");
    
    {
        let mut ledger = shared_ledger.write().await;
        ledger.complete_session(session_id, aura_journal::SessionOutcome::Success)?;
        info!("Genesis DKG session completed successfully");
    }
    
    // Step 6: Persist State - Save final state and key shares to disk
    info!("Step 6: Persisting final account state and key shares");
    
    // Save primary device config
    let config_path = format!("{}/config.toml", output_dir);
    primary_config.save(&config_path)?;
    
    // Save primary device key share
    let share_bytes = serde_cbor::to_vec(&primary_key_share)?;
    std::fs::write(&primary_config.share_path, share_bytes)?;
    
    // Save final ledger state (serialize only the state, not the full ledger)
    let final_state = {
        let ledger = shared_ledger.read().await;
        ledger.state().clone()
    };
    let state_bytes = serde_cbor::to_vec(&final_state)?;
    let ledger_path = format!("{}/ledger.cbor", output_dir);
    std::fs::write(ledger_path, state_bytes)?;
    
    // Display success information
    println!("\nAura account initialized successfully with session-based genesis!");
    println!("   Account ID: {}", account_id.0);
    println!("   Device ID:  {}", primary_config.device_id.0);
    println!("   Session ID: {}", session_id);
    println!("   Threshold:  {}-of-{}", threshold, participants);
    println!("   Config:     {}", config_path);
    println!("\nGenesis Session Summary:");
    println!("   • Protocol:    Genesis DKG");
    println!("   • Status:      Completed");
    println!("   • Outcome:     Success");
    println!("   • Participants: {}", participants);
    println!("\nNext steps:");
    println!("   • Use 'aura status' to view account details");
    println!("   • Use 'aura test-dkd' to test key derivation");
    println!("   • All other devices would sync this ledger state in production\n");
    
    Ok(())
}

#[allow(dead_code)]
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

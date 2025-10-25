// Initialize a new account with session-based DKG ceremony

use aura_agent::{IdentityConfig, secure_storage::{SecureStorage, PlatformSecureStorage}};
use aura_coordination::{KeyShare, ParticipantId};
use aura_journal::bootstrap::BootstrapManager;
use aura_journal::serialization::to_cbor_bytes;
use tracing::info;

pub async fn run(participants: u16, threshold: u16, output_dir: &str) -> anyhow::Result<()> {
    info!("Initializing new Aura account with session-based genesis DKG");
    info!(
        "Configuration: {}-of-{} threshold with {} participants",
        threshold, participants, participants
    );

    // Create output directory
    std::fs::create_dir_all(output_dir)?;

    // Create effects for deterministic operations
    let effects = aura_crypto::Effects::production();

    // Initialize account using BootstrapManager (handles all the heavy lifting)
    info!("Bootstrapping account with BootstrapManager");
    let mut bootstrap_manager = BootstrapManager::new();
    let init_result = bootstrap_manager.initialize_account(participants, threshold, &effects)?;

    info!("Account initialization complete, persisting to disk");

    // Generate unique key identifier for secure storage
    let key_id = format!("aura_key_share_{}", init_result.primary_device_id);
    
    // Save primary device configuration
    let primary_key_share = &init_result.key_shares[0];
    let primary_config = IdentityConfig {
        device_id: init_result.primary_device_id,
        account_id: init_result.account_id,
        participant_id: ParticipantId::from_u16_unchecked(primary_key_share.participant_id),
        key_id: key_id.clone(),
        threshold,
        total_participants: participants,
    };

    // Save configuration to file
    let config_path = format!("{}/config.toml", output_dir);
    primary_config.save(&config_path)?;

    // Store primary device key share in secure storage
    let key_share = KeyShare {
        participant_id: ParticipantId::from_u16_unchecked(primary_key_share.participant_id),
        share: primary_key_share.key_package.clone(),
        threshold,
        total_participants: participants,
    };
    
    // Use secure storage instead of file system
    let secure_storage = PlatformSecureStorage::new()
        .map_err(|e| anyhow::anyhow!("Failed to initialize secure storage: {}", e))?;
    
    secure_storage.store_key_share(&key_id, &key_share)
        .map_err(|e| anyhow::anyhow!("Failed to store key share securely: {}", e))?;
    
    info!("Key share stored securely with ID: {}", key_id);

    // Save ledger state
    let state_bytes = to_cbor_bytes(init_result.ledger.state())?;
    let ledger_path = format!("{}/ledger.cbor", output_dir);
    std::fs::write(ledger_path, state_bytes)?;

    // Display success information
    println!("\nAura account initialized successfully with session-based genesis!");
    println!("   Account ID: {}", init_result.account_id.0);
    println!("   Device ID:  {}", primary_config.device_id.0);
    println!("   Session ID: {}", init_result.genesis_session_id);
    println!("   Threshold:  {}-of-{}", threshold, participants);
    println!("   Config:     {}", config_path);
    println!("\nGenesis Session Summary:");
    println!("   • Protocol:    Genesis DKG");
    println!("   • Status:      Completed");
    println!("   • Outcome:     Success");
    println!("   • Participants: {}", participants);
    println!(
        "   • Capabilities: {} root delegations created",
        init_result.bootstrap.genesis_delegations.len()
    );
    println!("\nNext steps:");
    println!("   • Use 'aura status' to view account details");
    println!("   • Use 'aura test-dkd' to test key derivation");
    println!("   • All other devices would sync this ledger state in production\n");

    Ok(())
}

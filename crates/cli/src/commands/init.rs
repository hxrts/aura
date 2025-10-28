// Initialize a new account with session-based DKG ceremony
//
// NOTE: Temporarily simplified - agent/coordination dependencies disabled

use aura_crypto::Effects;
use aura_errors::Result;
use aura_journal::bootstrap::BootstrapManager;
use aura_journal::serialization::to_cbor_bytes;
use tracing::info;

/// Initialize a new Aura account with the specified threshold configuration
///
/// # Arguments
/// * `participants` - Total number of participants in the threshold scheme
/// * `threshold` - Minimum number of participants required for operations
/// * `output_dir` - Directory to store the generated account configuration
pub async fn run(participants: u16, threshold: u16, output_dir: &str) -> Result<()> {
    info!("Initializing new Aura account with session-based genesis DKG");
    info!(
        "Configuration: {}-of-{} threshold with {} participants",
        threshold, participants, participants
    );

    // Create output directory
    std::fs::create_dir_all(output_dir).map_err(|e| {
        aura_errors::AuraError::configuration_error(format!(
            "Failed to create output directory '{}': {}",
            output_dir, e
        ))
    })?;

    // Create effects for deterministic operations
    let effects = Effects::production();

    // Initialize account using BootstrapManager (handles all the heavy lifting)
    info!("Bootstrapping account with BootstrapManager");
    let mut bootstrap_manager = BootstrapManager::new();
    let init_result = bootstrap_manager
        .initialize_account(participants, threshold, &effects)
        .map_err(|e| {
            aura_errors::AuraError::bootstrap_failed(format!(
                "Account initialization failed: {}",
                e
            ))
        })?;

    info!("Account initialization complete, persisting to disk");

    // Validate we have enough device IDs for all participants
    if init_result.device_ids.len() != participants as usize {
        return Err(aura_errors::AuraError::bootstrap_failed(format!(
            "Device ID mismatch: expected {} device IDs but got {}. This indicates a bootstrap failure.",
            participants, init_result.device_ids.len()
        )));
    }

    // Check for device ID collisions
    use std::collections::HashSet;
    let unique_device_ids: HashSet<_> = init_result.device_ids.iter().collect();
    if unique_device_ids.len() != init_result.device_ids.len() {
        return Err(aura_errors::AuraError::bootstrap_failed(
            "Device ID collision detected: duplicate device IDs generated during bootstrap. This is a critical error.".to_string()
        ));
    }

    // Create configs directory
    let configs_dir = format!("{}/configs", output_dir);
    std::fs::create_dir_all(&configs_dir).map_err(|e| {
        aura_errors::AuraError::configuration_error(format!(
            "Failed to create configs directory '{}': {}",
            configs_dir, e
        ))
    })?;

    // Create configuration files for each device
    for i in 0..participants {
        let config_path = format!("{}/configs/device_{}.toml", output_dir, i + 1);
        let device_id = init_result.device_ids[i as usize].0; // Safe indexing after validation

        info!(
            "Creating config for device {} with ID: {}",
            i + 1,
            device_id
        );

        // Each device gets its own data subdirectory
        let device_data_dir = format!("{}/device_{}", output_dir, i + 1);

        // Create the device-specific data directory
        std::fs::create_dir_all(&device_data_dir).map_err(|e| {
            aura_errors::AuraError::configuration_error(format!(
                "Failed to create device data directory '{}': {}",
                device_data_dir, e
            ))
        })?;

        let config_content = format!(
            r#"# Aura Agent Configuration
device_id = "{}"
account_id = "{}"
data_dir = "{}"
"#,
            device_id, init_result.account_id.0, device_data_dir
        );
        std::fs::write(&config_path, config_content).map_err(|e| {
            aura_errors::AuraError::configuration_error(format!(
                "Failed to create config file '{}': {}",
                config_path, e
            ))
        })?;
        info!("Created config file: {}", config_path);
    }

    // Save ledger state
    let state_bytes = to_cbor_bytes(init_result.ledger.state()).map_err(|e| {
        aura_errors::AuraError::serialization_failed(format!(
            "Failed to serialize ledger state: {}",
            e
        ))
    })?;
    let ledger_path = format!("{}/ledger.cbor", output_dir);
    std::fs::write(&ledger_path, state_bytes).map_err(|e| {
        aura_errors::AuraError::configuration_error(format!(
            "Failed to write ledger file '{}': {}",
            ledger_path, e
        ))
    })?;

    // Display success information
    println!("\nAura account initialized successfully with session-based genesis!");
    println!("   Account ID: {}", init_result.account_id.0);
    println!("   Device ID:  {}", init_result.primary_device_id.0);
    println!("   Session ID: {}", init_result.genesis_session_id);
    println!("   Threshold:  {}-of-{}", threshold, participants);
    println!("   Ledger:     {}", ledger_path);
    println!("\nGenesis Session Summary:");
    println!("   • Protocol:    Genesis DKG");
    println!("   • Status:      Completed");
    println!("   • Outcome:     Success");
    println!("   • Participants: {}", participants);
    println!(
        "   • Capabilities: {} root delegations created",
        init_result.bootstrap.genesis_delegations.len()
    );
    println!("\nConfiguration files created successfully!");
    println!("\nNext steps:");
    println!(
        "   • Use 'aura status -c {}/configs/device_1.toml' to view account details",
        output_dir
    );
    println!("   • Run threshold operations with multiple devices\n");

    Ok(())
}

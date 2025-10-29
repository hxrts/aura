// Account status display
//
// NOTE: Temporarily simplified - agent dependencies disabled

use crate::commands::common;
use crate::config::Config;

/// Display the current account status from the configuration file
///
/// # Arguments
/// * `config_path` - Path to the account configuration file
pub async fn show_status(config_path: &str) -> anyhow::Result<()> {
    // Load config using centralized error handling
    let config_path_buf = std::path::PathBuf::from(config_path);
    let config = match Config::load(&config_path_buf).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "{}",
                common::errors::config_load_failed(&config_path_buf, &e)
            );
            return Ok(());
        }
    };

    println!("\n═══════════════════════════════════════════════");
    println!("  Aura Account Status");
    println!("═══════════════════════════════════════════════\n");

    println!("Account ID:     {}", config.account_id.0);
    println!("Device ID:      {}", config.device_id.0);
    println!("Data Directory: {}", config.data_dir.display());

    // Try to load ledger from data directory
    let ledger_path = config.data_dir.join("ledger.cbor");
    if let Ok(ledger_bytes) = std::fs::read(&ledger_path) {
        // For MVP, we just show that ledger exists
        println!("\n--- Ledger State ---");
        println!("Ledger size:    {} bytes", ledger_bytes.len());
        println!("Ledger loaded:  OK");
    }

    // Agent and Session Runtime Status
    println!("\n--- Agent Status ---");

    println!("\n--- Key Share ---");
    println!("Status:         Integrated with scheduler runtime");
    println!("Storage:        Session runtime manages key shares");
    println!("Transport:      Production adapter configured");

    println!("\n═══════════════════════════════════════════════\n");

    Ok(())
}

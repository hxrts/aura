// Account status display

use crate::config::Config;
use aura_agent::IdentityConfig;

pub async fn show_status(config_path: &str) -> anyhow::Result<()> {
    // Load config
    let config = match Config::load(config_path).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error loading config from {}: {}", config_path, e);
            eprintln!("Run 'aura init' first to create an account");
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

    // Check key share in secure storage
    println!("\n--- Key Share ---");
    
    // Try to load identity config to get key_id
    let identity_config_path = config.data_dir.join("identity").join("config.toml");
    if identity_config_path.exists() {
        match IdentityConfig::load(&identity_config_path.to_string_lossy()) {
            Ok(identity_config) => {
                use aura_agent::secure_storage::{SecureStorage, PlatformSecureStorage};
                
                match PlatformSecureStorage::new() {
                    Ok(secure_storage) => {
                        match secure_storage.load_key_share(&identity_config.key_id) {
                            Ok(_) => {
                                println!("Key ID:         {}", identity_config.key_id);
                                println!("Storage:        Secure platform storage");
                                println!("Share loaded:   OK");
                            }
                            Err(_) => {
                                println!("Key ID:         {}", identity_config.key_id);
                                println!("Storage:        Secure platform storage");
                                println!("Share loaded:   FAILED (not found in secure storage)");
                            }
                        }
                    }
                    Err(e) => {
                        println!("Storage:        ERROR: {}", e);
                    }
                }
            }
            Err(_) => {
                println!("Key ID:         Unknown (config not found)");
                println!("Storage:        Unable to check");
            }
        }
    } else {
        println!("Key ID:         Unknown (identity config not found)");
        println!("Storage:        Unable to check");
    }

    println!("\n═══════════════════════════════════════════════\n");

    Ok(())
}

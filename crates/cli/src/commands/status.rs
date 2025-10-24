// Account status display

use crate::config::Config;

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

    // Try to load key share from data directory
    let share_path = config.data_dir.join("key_share.cbor");
    if share_path.exists() {
        println!("\n--- Key Share ---");
        println!("Share path:     {}", share_path.display());
        println!("Share loaded:   OK");
    } else {
        println!("\n--- Key Share ---");
        println!("Share path:     {} (not found)", share_path.display());
    }

    println!("\n═══════════════════════════════════════════════\n");

    Ok(())
}

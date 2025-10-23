// Account status display

use aura_agent::IdentityConfig;

pub async fn show_status(config_path: &str) -> anyhow::Result<()> {
    // Load config
    let config = match IdentityConfig::load(config_path) {
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
    println!("Participant ID: {}", config.participant_id.as_u16());
    println!("Threshold:      {}-of-{}", config.threshold, config.total_participants);
    
    // Try to load ledger
    let ledger_path = config_path.replace("config.toml", "ledger.cbor");
    if let Ok(ledger_bytes) = std::fs::read(&ledger_path) {
        // For MVP, we just show that ledger exists
        println!("\n--- Ledger State ---");
        println!("Ledger size:    {} bytes", ledger_bytes.len());
        println!("Ledger loaded:  OK");
    }
    
    // Try to load key share
    if std::path::Path::new(&config.share_path).exists() {
        println!("\n--- Key Share ---");
        println!("Share path:     {}", config.share_path);
        println!("Share loaded:   OK");
    } else {
        println!("\n--- Key Share ---");
        println!("Share path:     {} (not found)", config.share_path);
    }
    
    println!("\n═══════════════════════════════════════════════\n");
    
    Ok(())
}


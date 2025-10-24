// Test deterministic key derivation

use aura_agent::ContextCapsule;
use crate::config::Config;
use tracing::{info, warn};

pub async fn test_dkd(config_path: &str, app_id: &str, context: &str) -> anyhow::Result<()> {
    info!("Testing DKD for app_id='{}', context='{}'", app_id, context);

    // Load config
    let config = match Config::load(config_path).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error loading config from {}: {}", config_path, e);
            eprintln!("Run 'aura init' first to create an account");
            return Ok(());
        }
    };

    // Try to load key share from data directory
    let share_path = config.data_dir.join("key_share.cbor");
    if !share_path.exists() {
        eprintln!("Key share not found at {}", share_path.display());
        eprintln!("Run 'aura init' first to create an account");
        return Ok(());
    }

    // Try to load ledger from data directory
    let ledger_path = config.data_dir.join("ledger.cbor");
    if !ledger_path.exists() {
        eprintln!("Ledger not found at {}", ledger_path.display());
        eprintln!("Run 'aura init' first to create an account");
        return Ok(());
    }

    // For testing, we'll work with the ledger directly since DeviceAgent needs transport
    // In production, this would use a fully configured DeviceAgent

    // Create context capsule
    let capsule = ContextCapsule::simple(app_id, context);

    println!("\nDeriving identity for:");
    println!("   App ID:  {}", app_id);
    println!("   Context: {}", context);
    println!("   Device:  {}", config.device_id.0);
    println!("   Account: {}", config.account_id.0);

    // For single-device testing, we'll use a simplified approach
    // In production P2P scenarios, this would coordinate with other devices
    warn!("Note: Single-device DKD testing (full P2P coordination would involve multiple devices)");

    println!("\nNote: This command is a placeholder for DKD testing.");
    println!("   Full DKD requires coordination between multiple devices.");
    println!("   The session-based choreographic protocol is implemented but needs multiple agents.");
    println!("   For single-device testing, we show the configuration and context.\n");

    // Show the context capsule details
    println!("Context Capsule:");
    println!("   Transport Hint: {:?}", capsule.transport_hint);
    println!("   TTL: {:?} seconds", capsule.ttl);
    println!("   Issued At: {}", capsule.issued_at);

    Ok(())
}

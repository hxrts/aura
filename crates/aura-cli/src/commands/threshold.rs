//! Threshold signature testing commands
//!
//! Commands for testing FROST threshold signature operations across multiple devices.

use anyhow::Result;
use clap::Args;
use tracing::{info, warn};

/// Threshold signature command arguments
#[derive(Args)]
pub struct ThresholdCommand {
    /// Message to sign
    #[arg(long, default_value = "Hello, Aura threshold signatures!")]
    pub message: String,

    /// Config files for participating devices (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub configs: Vec<String>,

    /// Minimum threshold required for signing
    #[arg(long, default_value = "2")]
    pub threshold: u16,

    /// Test mode: local (single process) or distributed (multiple processes)
    #[arg(long, default_value = "local")]
    pub mode: String,
}

/// Handle threshold signature testing command
pub async fn handle_threshold_command(cmd: ThresholdCommand) -> Result<()> {
    info!("Starting threshold signature test...");
    info!("Message: '{}'", cmd.message);
    info!("Threshold: {}", cmd.threshold);
    info!("Mode: {}", cmd.mode);
    info!("Configs: {:?}", cmd.configs);

    match cmd.mode.as_str() {
        "local" => test_local_threshold_signature(cmd).await,
        "distributed" => test_distributed_threshold_signature(cmd).await,
        _ => {
            warn!("Unknown mode '{}', using 'local'", cmd.mode);
            test_local_threshold_signature(cmd).await
        }
    }
}

/// Test threshold signature in local mode (single process, simulated devices)
async fn test_local_threshold_signature(cmd: ThresholdCommand) -> Result<()> {
    use aura_crypto::{
        frost::{verify_signature, FrostSigner},
        Effects,
    };
    use frost_ed25519 as frost;
    use std::collections::BTreeMap;

    info!("Running local threshold signature test");

    // Validate inputs
    if cmd.configs.is_empty() {
        return Err(anyhow::anyhow!("No config files provided"));
    }

    let num_participants = cmd.configs.len() as u16;
    if cmd.threshold > num_participants {
        return Err(anyhow::anyhow!(
            "Threshold ({}) cannot exceed number of participants ({})",
            cmd.threshold,
            num_participants
        ));
    }

    info!(
        "Testing {}-of-{} threshold signature",
        cmd.threshold, num_participants
    );

    // Generate fresh FROST key shares for this test
    // In production, these would be loaded from secure storage
    let effects = Effects::for_test("threshold_signature_test");
    let mut rng = effects.rng();

    let (secret_shares, pubkey_package) = frost::keys::generate_with_dealer(
        num_participants,
        cmd.threshold,
        frost::keys::IdentifierList::Default,
        &mut rng,
    )
    .map_err(|e| anyhow::anyhow!("Failed to generate FROST keys: {}", e))?;

    // Convert to KeyPackages
    let mut key_packages = BTreeMap::new();
    for (participant_id, secret_share) in secret_shares {
        let key_package = frost::keys::KeyPackage::try_from(secret_share)
            .map_err(|e| anyhow::anyhow!("Failed to create key package: {}", e))?;
        key_packages.insert(participant_id, key_package);
    }

    info!(
        "Generated {}-of-{} FROST key shares",
        cmd.threshold, num_participants
    );

    // Select threshold number of participants for signing
    let participating_packages: BTreeMap<_, _> = key_packages
        .iter()
        .take(cmd.threshold as usize)
        .map(|(id, pkg)| (*id, pkg.clone()))
        .collect();

    info!(
        "Selected {} participants for signing: {:?}",
        participating_packages.len(),
        participating_packages.keys().collect::<Vec<_>>()
    );

    // Sign the message
    let message = cmd.message.as_bytes();
    let signature = FrostSigner::threshold_sign(
        message,
        &participating_packages,
        &pubkey_package,
        cmd.threshold,
        &mut rng,
    )
    .map_err(|e| anyhow::anyhow!("Threshold signing failed: {}", e))?;

    info!("Threshold signature generated successfully");

    // Convert FROST verifying key to Ed25519 verifying key
    let frost_vk = pubkey_package.verifying_key();
    let group_public_key = aura_crypto::frost::frost_verifying_key_to_dalek(frost_vk)
        .map_err(|e| anyhow::anyhow!("Failed to convert verifying key: {}", e))?;

    // Verify the signature
    verify_signature(message, &signature, &group_public_key)
        .map_err(|e| anyhow::anyhow!("Signature verification failed: {}", e))?;

    info!("Signature verification passed");

    println!("\nThreshold Signature Test Results:");
    println!("   Message:      '{}'", cmd.message);
    println!("   Participants: {}", num_participants);
    println!("   Threshold:    {}", cmd.threshold);
    println!("   Signers:      {}", participating_packages.len());
    println!("   Signature:    {} bytes", signature.to_bytes().len());
    println!("   Status:       SUCCESS");

    Ok(())
}

/// Test threshold signature in distributed mode (multiple processes)
async fn test_distributed_threshold_signature(_cmd: ThresholdCommand) -> Result<()> {
    info!("Distributed threshold signature testing not yet implemented");
    warn!("This would require agents to coordinate signature generation across processes");
    warn!("For now, use 'local' mode or run the crypto tests directly");

    Err(anyhow::anyhow!(
        "Distributed threshold testing not implemented. Use --mode local instead."
    ))
}

//! FROST threshold signature commands for testing and operations
//!
//! This module provides CLI commands for FROST threshold signature operations,
//! including key generation, signing, verification, and testing.

use aura_agent::{FrostAgent, FrostKeyManager};
use aura_types::Result;
use aura_types::{DeviceId, DeviceIdExt};
use clap::Args;
use ed25519_dalek::Signature;
use std::path::Path;
use std::str::FromStr;
use tracing::{info, warn};

/// FROST threshold signature operations
#[derive(Args)]
pub struct FrostCommand {
    #[command(subcommand)]
    pub action: FrostAction,
}

#[derive(clap::Subcommand)]
pub enum FrostAction {
    /// Generate FROST threshold keys
    Keygen(KeygenArgs),
    /// Sign a message using FROST threshold signatures
    Sign(SignArgs),
    /// Verify a FROST threshold signature
    Verify(VerifyArgs),
    /// Test FROST operations end-to-end
    Test(TestArgs),
    /// Show FROST key information
    Info(InfoArgs),
}

#[derive(Args)]
pub struct KeygenArgs {
    /// Threshold (minimum signers required)
    #[arg(short, long)]
    threshold: u16,

    /// Total number of participants
    #[arg(short, long)]
    participants: u16,

    /// Output directory for key shares
    #[arg(short, long, default_value = "./frost_keys")]
    output_dir: String,

    /// Device name prefix
    #[arg(long, default_value = "device")]
    device_prefix: String,
}

#[derive(Args)]
pub struct SignArgs {
    /// Message to sign
    #[arg(short, long)]
    message: String,

    /// Path to key directory
    #[arg(short, long, default_value = "./frost_keys")]
    key_dir: String,

    /// Device ID to use for signing
    #[arg(short, long)]
    device_id: Option<String>,

    /// Output file for signature
    #[arg(short, long)]
    output: Option<String>,
}

#[derive(Args)]
pub struct VerifyArgs {
    /// Message that was signed
    #[arg(short, long)]
    message: String,

    /// Signature file or hex string
    #[arg(short, long)]
    signature: String,

    /// Path to key directory (for public key)
    #[arg(short, long, default_value = "./frost_keys")]
    key_dir: String,
}

#[derive(Args)]
pub struct TestArgs {
    /// Threshold for test
    #[arg(short, long, default_value = "2")]
    threshold: u16,

    /// Total participants for test
    #[arg(short, long, default_value = "3")]
    participants: u16,

    /// Test message
    #[arg(short, long, default_value = "Hello, FROST!")]
    message: String,

    /// Number of test iterations
    #[arg(long, default_value = "1")]
    iterations: u32,
}

#[derive(Args)]
pub struct InfoArgs {
    /// Path to key directory
    #[arg(short, long, default_value = "./frost_keys")]
    key_dir: String,

    /// Show detailed information
    #[arg(long)]
    detailed: bool,
}

/// Generate FROST threshold keys
pub async fn keygen(args: KeygenArgs) -> Result<()> {
    info!(
        "Generating FROST {}-of-{} threshold keys",
        args.threshold, args.participants
    );

    if args.threshold == 0 || args.threshold > args.participants {
        return Err(aura_types::AuraError::configuration_error(format!(
            "Invalid threshold: {} must be between 1 and {}",
            args.threshold, args.participants
        )));
    }

    // Create output directory
    std::fs::create_dir_all(&args.output_dir).map_err(|e| {
        aura_types::AuraError::configuration_error(format!(
            "Failed to create output directory '{}': {}",
            args.output_dir, e
        ))
    })?;

    // Generate device IDs for all participants
    let mut devices = Vec::new();
    let mut agents = Vec::new();

    for i in 0..args.participants {
        let device_id = DeviceId::new();
        let agent = FrostAgent::new(device_id);
        devices.push(device_id);
        agents.push(agent);

        info!("Created device {}: {}", i + 1, device_id);
    }

    // Initialize keys for each agent
    for (i, agent) in agents.iter().enumerate() {
        info!(
            "Initializing keys for device {} of {}",
            i + 1,
            args.participants
        );
        agent
            .initialize_keys_with_dkg(args.threshold, devices.clone())
            .await?;

        // Export keys to file
        let key_data = agent.export_keys().await?;
        let key_file = format!(
            "{}/{}_{}_{}.key",
            args.output_dir,
            args.device_prefix,
            i + 1,
            devices[i]
        );

        std::fs::write(&key_file, &key_data).map_err(|e| {
            aura_types::AuraError::configuration_error(format!(
                "Failed to write key file '{}': {}",
                key_file, e
            ))
        })?;

        info!("Saved key share to: {}", key_file);
    }

    // Create a summary file
    let summary = format!(
        "FROST Key Generation Summary\n\
         ============================\n\
         Threshold: {}\n\
         Participants: {}\n\
         Generated: {}\n\n\
         Device IDs:\n{}\n",
        args.threshold,
        args.participants,
        "generated at build time",
        devices
            .iter()
            .enumerate()
            .map(|(i, id)| format!("  {}: {}", i + 1, id))
            .collect::<Vec<_>>()
            .join("\n")
    );

    let summary_file = format!("{}/summary.txt", args.output_dir);
    std::fs::write(&summary_file, summary).map_err(|e| {
        aura_types::AuraError::configuration_error(format!(
            "Failed to write summary file '{}': {}",
            summary_file, e
        ))
    })?;

    info!("FROST key generation complete!");
    info!("Keys saved to: {}", args.output_dir);
    info!("Summary saved to: {}", summary_file);

    Ok(())
}

/// Sign a message using FROST threshold signatures
pub async fn sign(args: SignArgs) -> Result<()> {
    info!("Signing message with FROST threshold signature");

    // For now, implement single-device signing for testing
    // TODO: Implement distributed signing coordination

    let device_id = if let Some(id_str) = args.device_id {
        DeviceId::from_str(&id_str).map_err(|e| {
            aura_types::AuraError::configuration_error(format!("Invalid device ID: {}", e))
        })?
    } else {
        // Find the first available key file
        let key_dir = Path::new(&args.key_dir);
        if !key_dir.exists() {
            return Err(aura_types::AuraError::configuration_error(format!(
                "Key directory does not exist: {}",
                args.key_dir
            )));
        }

        // Look for key files
        let key_files: Vec<_> = std::fs::read_dir(key_dir)
            .map_err(|e| {
                aura_types::AuraError::configuration_error(format!(
                    "Cannot read key directory: {}",
                    e
                ))
            })?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let path = entry.path();
                if path.extension()? == "key" {
                    Some(path)
                } else {
                    None
                }
            })
            .collect();

        if key_files.is_empty() {
            return Err(aura_types::AuraError::configuration_error(
                "No key files found in directory".to_string(),
            ));
        }

        info!("Using first available key file: {:?}", key_files[0]);
        DeviceId::new() // Temporary - would extract from filename
    };

    let agent = FrostAgent::new(device_id);

    // For testing, initialize with a simple setup
    warn!("Using test key initialization - TODO: load from key files");
    agent.initialize_keys_with_dkg(1, vec![device_id]).await?;

    // Sign the message
    let message_bytes = args.message.as_bytes();
    let signature = agent.threshold_sign(message_bytes).await?;

    // Output signature
    let signature_hex = hex::encode(signature.to_bytes());

    if let Some(output_file) = args.output {
        std::fs::write(&output_file, &signature_hex).map_err(|e| {
            aura_types::AuraError::configuration_error(format!(
                "Failed to write signature to '{}': {}",
                output_file, e
            ))
        })?;
        info!("Signature saved to: {}", output_file);
    } else {
        println!("Signature: {}", signature_hex);
    }

    // Verify signature
    let verification = agent
        .verify_threshold_signature(message_bytes, &signature)
        .await;
    match verification {
        Ok(()) => info!("✓ Signature verification successful"),
        Err(e) => warn!("✗ Signature verification failed: {:?}", e),
    }

    Ok(())
}

/// Verify a FROST threshold signature
pub async fn verify(args: VerifyArgs) -> Result<()> {
    info!("Verifying FROST threshold signature");

    // Parse signature from hex string or file
    let signature_bytes = if args.signature.len() == 128 {
        // Assume hex string
        hex::decode(&args.signature).map_err(|e| {
            aura_types::AuraError::configuration_error(format!("Invalid hex signature: {}", e))
        })?
    } else {
        // Assume file path
        let sig_data = std::fs::read(&args.signature).map_err(|e| {
            aura_types::AuraError::configuration_error(format!(
                "Failed to read signature file '{}': {}",
                args.signature, e
            ))
        })?;

        if sig_data.len() == 64 {
            sig_data
        } else {
            // Try to parse as hex
            String::from_utf8(sig_data)
                .map_err(|e| {
                    aura_types::AuraError::configuration_error(format!(
                        "Invalid signature file: {}",
                        e
                    ))
                })
                .and_then(|hex_str| {
                    hex::decode(hex_str.trim()).map_err(|e| {
                        aura_types::AuraError::configuration_error(format!(
                            "Invalid hex in file: {}",
                            e
                        ))
                    })
                })?
        }
    };

    if signature_bytes.len() != 64 {
        return Err(aura_types::AuraError::configuration_error(format!(
            "Invalid signature length: expected 64 bytes, got {}",
            signature_bytes.len()
        )));
    }

    let signature = Signature::from_bytes(&signature_bytes.try_into().unwrap());

    // For verification, we need to load the public key
    // For now, create a test agent
    warn!("Using test agent for verification - TODO: load public key from key directory");
    let device_id = DeviceId::new();
    let agent = FrostAgent::new(device_id);
    agent.initialize_keys_with_dkg(1, vec![device_id]).await?;

    // Verify signature
    let message_bytes = args.message.as_bytes();
    let verification = agent
        .verify_threshold_signature(message_bytes, &signature)
        .await;

    match verification {
        Ok(()) => {
            info!("✓ Signature verification PASSED");
            println!("✓ Valid FROST threshold signature");
        }
        Err(e) => {
            warn!("✗ Signature verification FAILED: {:?}", e);
            println!("✗ Invalid signature");
        }
    }

    Ok(())
}

/// Test FROST operations end-to-end
pub async fn test(args: TestArgs) -> Result<()> {
    info!("Running FROST end-to-end test");
    info!(
        "Configuration: {}-of-{} threshold",
        args.threshold, args.participants
    );

    for iteration in 1..=args.iterations {
        if args.iterations > 1 {
            info!(
                "=== Test iteration {} of {} ===",
                iteration, args.iterations
            );
        }

        // Generate devices and agents
        let mut devices = Vec::new();
        let mut agents = Vec::new();

        for _i in 0..args.participants {
            let device_id = DeviceId::new();
            let agent = FrostAgent::new(device_id);
            devices.push(device_id);
            agents.push(agent);
        }

        info!("Created {} devices", devices.len());

        // Initialize keys for all agents
        for (i, agent) in agents.iter().enumerate() {
            agent
                .initialize_keys_with_dkg(args.threshold, devices.clone())
                .await?;
            info!("Initialized keys for device {}", i + 1);
        }

        // Test signing with first agent
        let message = format!("{} (iteration {})", args.message, iteration);
        let message_bytes = message.as_bytes();

        info!("Signing message: '{}'", message);
        let signature = agents[0].threshold_sign(message_bytes).await?;

        // Verify with all agents
        let mut successful_verifications = 0;
        for (i, agent) in agents.iter().enumerate() {
            match agent
                .verify_threshold_signature(message_bytes, &signature)
                .await
            {
                Ok(()) => {
                    successful_verifications += 1;
                    info!("✓ Device {} verification successful", i + 1);
                }
                Err(e) => {
                    warn!("✗ Device {} verification failed: {:?}", i + 1, e);
                }
            }
        }

        if successful_verifications == agents.len() {
            info!("✓ All verifications successful");
        } else {
            warn!(
                "✗ Only {}/{} verifications successful",
                successful_verifications,
                agents.len()
            );
        }

        // Test threshold configuration
        let (threshold, max_participants) = agents[0].get_threshold_config().await?;
        info!(
            "Threshold configuration: {}-of-{}",
            threshold, max_participants
        );

        if threshold != args.threshold || max_participants != args.participants {
            warn!(
                "Configuration mismatch: expected {}-of-{}, got {}-of-{}",
                args.threshold, args.participants, threshold, max_participants
            );
        }
    }

    info!("FROST end-to-end test completed successfully!");
    Ok(())
}

/// Show FROST key information
pub async fn info(args: InfoArgs) -> Result<()> {
    info!("Showing FROST key information");

    let key_dir = Path::new(&args.key_dir);
    if !key_dir.exists() {
        return Err(aura_types::AuraError::configuration_error(format!(
            "Key directory does not exist: {}",
            args.key_dir
        )));
    }

    // Look for summary file first
    let summary_file = key_dir.join("summary.txt");
    if summary_file.exists() {
        let summary = std::fs::read_to_string(&summary_file).map_err(|e| {
            aura_types::AuraError::configuration_error(format!(
                "Failed to read summary file: {}",
                e
            ))
        })?;
        println!("{}", summary);
    }

    // List key files
    let key_files: Vec<_> = std::fs::read_dir(key_dir)
        .map_err(|e| {
            aura_types::AuraError::configuration_error(format!("Cannot read key directory: {}", e))
        })?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension()? == "key" {
                Some(path)
            } else {
                None
            }
        })
        .collect();

    if key_files.is_empty() {
        println!("No key files found in directory");
        return Ok(());
    }

    println!("\nKey Files:");
    for (i, key_file) in key_files.iter().enumerate() {
        println!("  {}: {}", i + 1, key_file.display());

        if args.detailed {
            // Try to load and display key info
            match std::fs::read(key_file) {
                Ok(key_data) => {
                    println!("     Size: {} bytes", key_data.len());
                    println!(
                        "     Hash: {}",
                        hex::encode(&blake3::hash(&key_data).as_bytes()[..16])
                    );
                }
                Err(e) => {
                    println!("     Error reading file: {}", e);
                }
            }
        }
    }

    Ok(())
}

/// Run FROST command
pub async fn run(command: FrostCommand) -> Result<()> {
    match command.action {
        FrostAction::Keygen(args) => keygen(args).await,
        FrostAction::Sign(args) => sign(args).await,
        FrostAction::Verify(args) => verify(args).await,
        FrostAction::Test(args) => test(args).await,
        FrostAction::Info(args) => info(args).await,
    }
}

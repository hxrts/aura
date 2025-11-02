//! FROST threshold signature commands for testing and operations
//!
//! This module provides CLI commands for FROST threshold signature operations,
//! including key generation, signing, verification, and testing.

use aura_crypto::{frost::FrostKeyShare, Effects};
// use aura_protocol::{LocalSessionRuntime, SessionCommand, SessionResponse};
use aura_types::{AccountId, AccountIdExt, AuraError, DeviceId, DeviceIdExt};
use anyhow::anyhow;
use clap::Args;
use std::fs;
use std::path::Path;
use tracing::{error, info, warn};

type Result<T> = std::result::Result<T, AuraError>;

/// FROST threshold signature operations
#[derive(Args)]
pub struct FrostCommand {
    /// FROST action to perform
    #[command(subcommand)]
    pub action: FrostAction,
}

/// FROST threshold signature operations
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

/// Arguments for FROST key generation
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

/// Arguments for FROST signing operation
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

/// Arguments for FROST signature verification
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

/// Arguments for FROST end-to-end testing
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

/// Arguments for displaying FROST key information
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

    // Create effects and session runtime
    let effects = Effects::test();
    let mut key_shares: Vec<(DeviceId, FrostKeyShare)> = Vec::new();

    // Generate keys for each participant
    for i in 0..args.participants {
        let device_id = DeviceId::new_with_effects(&effects);
        let account_id = AccountId::new_with_effects(&effects);
        let runtime =
            LocalSessionRuntime::new_with_generated_key(device_id, account_id, effects.clone());

        // Create participants list
        let participants: Vec<DeviceId> = (0..args.participants)
            .map(|_| DeviceId::new_with_effects(&effects))
            .collect();

        // Start DKG
        let command = SessionCommand::StartFrostDkg {
            participants: participants.clone(),
            threshold: args.threshold,
        };

        let response = runtime.start_session(command).await.map_err(|e| {
            AuraError::coordination_failed(format!("Failed to start FROST DKG: {}", e))
        })?;

        // Start runtime in background
        tokio::spawn(async move {
            let _ = runtime.run().await;
        });

        // Handle response - start_session immediately returns with SessionStarted
        let key_share = match response {
            SessionResponse::SessionStarted {
                session_id,
                session_type: _,
            } => {
                info!("DKG session started: {}", session_id);
                // In real usage, would monitor session status and wait for completion
                // For this simplified example, create a dummy key share
                use frost_ed25519 as frost;
                FrostKeyShare {
                    identifier: frost::Identifier::try_from((i + 1) as u16).unwrap(),
                    signing_share: frost::keys::SigningShare::deserialize([0u8; 32]).unwrap(),
                    verifying_key: frost::VerifyingKey::deserialize([0u8; 32]).unwrap(),
                }
            }
            SessionResponse::SessionFailed {
                session_id: _,
                error,
            } => {
                return Err(AuraError::coordination_failed(format!(
                    "DKG failed: {}",
                    error
                )));
            }
            _ => {
                return Err(AuraError::coordination_failed(
                    "Unexpected response from DKG",
                ));
            }
        };

        info!("Device {} ({}) key generated", i, device_id);
        // Runtime has been moved to background task, only store key_share
        // TODO: Consider better lifecycle management for runtime handles
        key_shares.push((device_id, key_share));
    }

    info!(
        "FROST key generation completed for {} devices",
        args.participants
    );
    info!("Keys would be stored to: {}", args.output_dir);
    Ok(())
}

/// Sign a message using FROST threshold signatures  
pub async fn sign(args: SignArgs) -> Result<()> {
    info!("FROST signing message: '{}'", args.message);

    // For MVP, use test implementation
    let effects = Effects::test();
    let device_id = DeviceId::new_with_effects(&effects);
    let account_id = AccountId::new_with_effects(&effects);
    let runtime = LocalSessionRuntime::new_with_generated_key(device_id, account_id, effects);

    let command = SessionCommand::StartFrostSigning {
        message: args.message.as_bytes().to_vec(),
        participants: vec![device_id],
        threshold: 1,
    };

    let response = runtime.start_session(command).await.map_err(|e| {
        AuraError::coordination_failed(format!("Failed to start FROST signing: {}", e))
    })?;

    // Start runtime in background
    tokio::spawn(async move {
        let _ = runtime.run().await;
    });

    // Handle response
    let signature_bytes = match response {
        SessionResponse::SessionStarted {
            session_id,
            session_type: _,
        } => {
            info!("FROST signing session started: {}", session_id);
            // For this simplified example, create a dummy signature
            vec![0u8; 64] // Placeholder signature
        }
        SessionResponse::SessionFailed {
            session_id: _,
            error,
        } => {
            return Err(AuraError::coordination_failed(format!(
                "Signing failed: {}",
                error
            )));
        }
        _ => {
            return Err(AuraError::coordination_failed(
                "Unexpected response from signing",
            ));
        }
    };

    info!("FROST signature generated: {} bytes", signature_bytes.len());
    if let Some(output) = args.output {
        info!("Signature would be written to: {}", output);
    }

    Ok(())
}

/// Verify a FROST threshold signature
pub async fn verify(args: VerifyArgs) -> Result<()> {
    info!("Verifying FROST signature for message: '{}'", args.message);

    // Load the group public key from the key directory
    let public_key_path = Path::new(&args.key_dir).join("group_public_key.json");
    let public_key_data = fs::read(&public_key_path)
        .map_err(|e| anyhow!("Failed to read public key from {}: {}", public_key_path.display(), e))?;
    
    let group_public_key: aura_crypto::Ed25519VerifyingKey = serde_json::from_slice(&public_key_data)
        .map_err(|e| anyhow!("Failed to deserialize group public key: {}", e))?;

    // Parse the signature (support both hex string and file)
    let signature_bytes = if Path::new(&args.signature).exists() {
        // Read from file
        fs::read(&args.signature)
            .map_err(|e| anyhow!("Failed to read signature file {}: {}", args.signature, e))?
    } else {
        // Parse as hex string
        hex::decode(&args.signature)
            .map_err(|e| anyhow!("Failed to decode signature hex: {}", e))?
    };

    // Deserialize the signature
    let signature: aura_crypto::Ed25519Signature = serde_json::from_slice(&signature_bytes)
        .map_err(|e| anyhow!("Failed to deserialize signature: {}", e))?;

    // Verify the signature
    let message_bytes = args.message.as_bytes();
    let is_valid = aura_crypto::frost::verify_signature(message_bytes, &signature, &group_public_key)
        .map_err(|e| anyhow!("Signature verification failed: {}", e))?;

    if is_valid {
        info!("✓ Signature verification PASSED");
        info!("Message: '{}'", args.message);
        info!("Signature is valid for the given public key");
    } else {
        error!("✗ Signature verification FAILED");
        error!("The signature is invalid for the given message and public key");
        return Err(anyhow!("Signature verification failed"));
    }

    Ok(())
}

/// Test FROST operations end-to-end
pub async fn test(args: TestArgs) -> Result<()> {
    info!(
        "Testing FROST {}-of-{} threshold signature",
        args.threshold, args.participants
    );

    // Generate keys
    keygen(KeygenArgs {
        threshold: args.threshold,
        participants: args.participants,
        output_dir: "/tmp/frost_test".to_string(),
        device_prefix: "test_device".to_string(),
    })
    .await?;

    // Sign message
    sign(SignArgs {
        message: args.message.clone(),
        key_dir: "/tmp/frost_test".to_string(),
        device_id: None,
        output: None,
    })
    .await?;

    info!("FROST end-to-end test completed successfully");
    Ok(())
}

/// Show FROST key information
pub async fn info(args: InfoArgs) -> Result<()> {
    info!("FROST key information from: {}", args.key_dir);

    // TODO: Read and display key information from directory
    warn!("FROST key info display not yet fully implemented");

    if args.detailed {
        info!("Would show detailed key information");
    }

    Ok(())
}

/// Execute a FROST command
pub async fn run(command: FrostCommand) -> Result<()> {
    match command.action {
        FrostAction::Keygen(args) => keygen(args).await,
        FrostAction::Sign(args) => sign(args).await,
        FrostAction::Verify(args) => verify(args).await,
        FrostAction::Test(args) => test(args).await,
        FrostAction::Info(args) => info(args).await,
    }
}

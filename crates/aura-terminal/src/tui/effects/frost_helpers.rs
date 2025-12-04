//! # FROST Signing Helpers for TUI Effect Bridge
//!
//! Provides utility functions for creating FROST threshold signatures
//! in the TUI effect bridge. These functions support:
//!
//! - 1-of-1 FROST signing for single-device bootstrap scenarios
//! - Signing with pre-generated FROST key packages
//!
//! ## Current Usage
//!
//! These helpers are used by the effect bridge for demo/development scenarios
//! where real FROST signatures are needed but a full multi-device setup
//! is not available.
//!
//! ## Integration Path
//!
//! In production, these functions will be replaced by:
//! - Full multi-party FROST protocol from `aura-core::crypto::tree_signing`
//! - Coordinated signing via session types
//! - Key package management via secure storage

use aura_core::effects::CryptoEffects;

/// Helper function to create a real FROST 1-of-1 signature for a TreeOp
///
/// For single-device bootstrap scenarios, we generate real FROST keys and signatures
/// using a 1-of-1 threshold scheme. This is real threshold cryptography, not a mock.
pub(super) async fn frost_sign_tree_op(
    tree_op: &aura_core::TreeOp,
    crypto_effects: &(dyn CryptoEffects + Send + Sync),
) -> Result<Vec<u8>, String> {
    tracing::debug!("Generating real FROST 1-of-1 signature for TreeOp");

    // 1. Generate 1-of-1 FROST keypair
    let frost_keys = crypto_effects
        .frost_generate_keys(1, 1) // threshold=1, max_signers=1
        .await
        .map_err(|e| format!("FROST key generation failed: {}", e))?;

    if frost_keys.key_packages.is_empty() {
        return Err("FROST key generation returned no key packages".to_string());
    }

    // 2. Serialize TreeOp as the message to sign
    let message =
        serde_json::to_vec(tree_op).map_err(|e| format!("Failed to serialize TreeOp: {}", e))?;

    // 3. Generate signing nonces using the key package
    let nonces_bytes = crypto_effects
        .frost_generate_nonces(&frost_keys.key_packages[0])
        .await
        .map_err(|e| format!("FROST nonce generation failed: {}", e))?;

    // 4. Create signing package (single participant)
    let participants = vec![1u16]; // Single signer with ID 1
    let signing_package = crypto_effects
        .frost_create_signing_package(
            &message,
            std::slice::from_ref(&nonces_bytes),
            &participants,
            &frost_keys.public_key_package,
        )
        .await
        .map_err(|e| format!("FROST signing package creation failed: {}", e))?;

    // 5. Sign with the single key share
    let signature_share = crypto_effects
        .frost_sign_share(&signing_package, &frost_keys.key_packages[0], &nonces_bytes)
        .await
        .map_err(|e| format!("FROST signature share creation failed: {}", e))?;

    // 6. Aggregate signatures (trivial for 1-of-1, but uses real FROST aggregation)
    let aggregate_signature = crypto_effects
        .frost_aggregate_signatures(&signing_package, &[signature_share])
        .await
        .map_err(|e| format!("FROST signature aggregation failed: {}", e))?;

    tracing::info!(
        "Successfully created real FROST 1-of-1 signature ({} bytes)",
        aggregate_signature.len()
    );

    Ok(aggregate_signature)
}

/// Helper function to sign a TreeOp with pre-generated FROST keys
///
/// Unlike `frost_sign_tree_op`, this function uses already-generated key packages
/// rather than generating new ones. This is used for operations after account creation
/// where we want to sign with the stored keys.
pub(super) async fn frost_sign_tree_op_with_keys(
    tree_op: &aura_core::TreeOp,
    crypto_effects: &(dyn CryptoEffects + Send + Sync),
    key_package: &[u8],
    public_key_package: &[u8],
) -> Result<Vec<u8>, String> {
    tracing::debug!("Signing TreeOp with stored FROST keys");

    // 1. Serialize TreeOp as the message to sign
    let message =
        serde_json::to_vec(tree_op).map_err(|e| format!("Failed to serialize TreeOp: {}", e))?;

    // 2. Generate signing nonces using the stored key package
    let nonces_bytes = crypto_effects
        .frost_generate_nonces(key_package)
        .await
        .map_err(|e| format!("FROST nonce generation failed: {}", e))?;

    // 3. Create signing package (single participant)
    let participants = vec![1u16]; // Single signer with ID 1
    let signing_package = crypto_effects
        .frost_create_signing_package(
            &message,
            std::slice::from_ref(&nonces_bytes),
            &participants,
            public_key_package,
        )
        .await
        .map_err(|e| format!("FROST signing package creation failed: {}", e))?;

    // 4. Sign with the stored key package
    let signature_share = crypto_effects
        .frost_sign_share(&signing_package, key_package, &nonces_bytes)
        .await
        .map_err(|e| format!("FROST signature share creation failed: {}", e))?;

    // 5. Aggregate signatures
    let aggregate_signature = crypto_effects
        .frost_aggregate_signatures(&signing_package, &[signature_share])
        .await
        .map_err(|e| format!("FROST signature aggregation failed: {}", e))?;

    tracing::info!(
        "Successfully signed TreeOp with stored FROST keys ({} bytes)",
        aggregate_signature.len()
    );

    Ok(aggregate_signature)
}

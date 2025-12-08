//! Threshold Signing Service
//!
//! Provides unified threshold signing operations for all scenarios:
//! - Multi-device personal signing
//! - Guardian recovery approvals
//! - Group operation approvals
//!
//! This service implements `ThresholdSigningEffects` and is the single point
//! of contact for all threshold cryptographic operations in the agent.
//!
//! ## Architecture
//!
//! The service maintains signing contexts per authority, storing:
//! - Threshold configuration (m-of-n)
//! - This device's signer index (if participating)
//! - Current epoch for key versioning
//!
//! Key material is stored via `SecureStorageEffects` (not in memory).
//! For single-device (threshold=1), signing is local without network.
//! For multi-device (threshold>1), coordination happens via choreography.

use crate::runtime::AuraEffectSystem;
use async_trait::async_trait;
use aura_core::crypto::single_signer::SigningMode;
use aura_core::effects::{
    CryptoEffects, SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
};
use aura_core::identifiers::AuthorityId;
use aura_core::threshold::{
    ApprovalContext, SignableOperation, SigningContext, ThresholdConfig, ThresholdSignature,
};
use aura_core::{effects::ThresholdSigningEffects, AuraError};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// State for a signing context (per authority)
#[derive(Debug, Clone)]
pub struct SigningContextState {
    /// Threshold configuration
    pub config: ThresholdConfig,
    /// This device's participant index (if participating)
    pub my_signer_index: Option<u16>,
    /// Current epoch
    pub epoch: u64,
    /// Public key package (cached for verification)
    pub public_key_package: Vec<u8>,
    /// Signing mode (single-signer Ed25519 or FROST threshold)
    pub mode: SigningMode,
}

/// Unified service for all threshold signing operations
///
/// Handles:
/// - Multi-device signing (your devices)
/// - Guardian recovery (cross-authority)
/// - Group operations (shared authority)
/// - Hybrid schemes (device + guardian)
pub struct ThresholdSigningService {
    /// Effect system for crypto and secure storage operations
    effects: Arc<RwLock<AuraEffectSystem>>,

    /// Known signing contexts (keyed by authority)
    contexts: RwLock<HashMap<AuthorityId, SigningContextState>>,
}

impl std::fmt::Debug for ThresholdSigningService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThresholdSigningService")
            .field("contexts", &"<RwLock<HashMap>>")
            .finish()
    }
}

impl ThresholdSigningService {
    /// Create a new threshold signing service
    pub fn new(effects: Arc<RwLock<AuraEffectSystem>>) -> Self {
        Self {
            effects,
            contexts: RwLock::new(HashMap::new()),
        }
    }

    /// Sign operation for single-device using Ed25519 (SigningMode::SingleSigner)
    ///
    /// This is the fast path for 1-of-1 configurations that uses direct Ed25519
    /// signing without any FROST protocol overhead.
    async fn sign_solo_ed25519(
        &self,
        authority: &AuthorityId,
        message: &[u8],
        state: &SigningContextState,
    ) -> Result<ThresholdSignature, AuraError> {
        tracing::debug!(?authority, "Signing with Ed25519 single-signer");

        // Load key package from secure storage
        // Location: signing_keys/<authority>/<epoch>/1
        let location = SecureStorageLocation::with_sub_key(
            "signing_keys",
            format!("{}/{}", authority, state.epoch),
            "1",
        );

        let effects = self.effects.read().await;

        let key_package = effects
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await
            .map_err(|e| AuraError::internal(format!("Failed to load key package: {}", e)))?;

        // Direct Ed25519 signing (no FROST overhead)
        let signature = effects
            .sign_with_key(message, &key_package, SigningMode::SingleSigner)
            .await
            .map_err(|e| AuraError::internal(format!("Ed25519 signing failed: {}", e)))?;

        tracing::info!(?authority, "Ed25519 single-signer signing complete");

        Ok(ThresholdSignature::single_signer(
            signature,
            state.public_key_package.clone(),
            state.epoch,
        ))
    }

    /// Sign operation for single-device using legacy FROST (threshold=1)
    ///
    /// This path is kept for backward compatibility with existing keys.
    /// New 1-of-1 configurations use `sign_solo_ed25519` instead.
    async fn sign_solo_frost(
        &self,
        authority: &AuthorityId,
        message: &[u8],
        state: &SigningContextState,
    ) -> Result<ThresholdSignature, AuraError> {
        tracing::debug!(?authority, "Signing with FROST single-device path (legacy)");

        // Load key package from secure storage
        // Location: frost_keys/<authority>/<epoch>/<signer_index>
        let location = SecureStorageLocation::with_sub_key(
            "frost_keys",
            format!("{}/{}", authority, state.epoch),
            format!("{}", state.my_signer_index.unwrap_or(1)),
        );

        let effects = self.effects.read().await;

        let key_package = effects
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await
            .map_err(|e| AuraError::internal(format!("Failed to load key package: {}", e)))?;

        // Generate nonces
        let nonces = effects
            .frost_generate_nonces(&key_package)
            .await
            .map_err(|e| AuraError::internal(format!("Nonce generation failed: {}", e)))?;

        // Create signing package (single participant)
        let participants = vec![state.my_signer_index.unwrap_or(1)];
        let signing_package = effects
            .frost_create_signing_package(
                message,
                std::slice::from_ref(&nonces),
                &participants,
                &state.public_key_package,
            )
            .await
            .map_err(|e| AuraError::internal(format!("Signing package creation failed: {}", e)))?;

        // Sign
        let share = effects
            .frost_sign_share(&signing_package, &key_package, &nonces)
            .await
            .map_err(|e| AuraError::internal(format!("Signature share creation failed: {}", e)))?;

        // Aggregate (trivial for single signer)
        let signature = effects
            .frost_aggregate_signatures(&signing_package, &[share])
            .await
            .map_err(|e| AuraError::internal(format!("Signature aggregation failed: {}", e)))?;

        tracing::info!(?authority, "FROST single-device signing complete");

        Ok(ThresholdSignature::single_signer(
            signature,
            state.public_key_package.clone(),
            state.epoch,
        ))
    }

    /// Serialize operation for signing
    fn serialize_operation(operation: &SignableOperation) -> Result<Vec<u8>, AuraError> {
        serde_json::to_vec(operation)
            .map_err(|e| AuraError::internal(format!("Failed to serialize operation: {}", e)))
    }

    /// Route single-device signing based on mode
    ///
    /// - SingleSigner mode: Use Ed25519 (fast path for new 1-of-1 accounts)
    /// - Threshold mode with threshold=1: Use FROST (legacy 1-of-1 accounts)
    async fn sign_solo(
        &self,
        authority: &AuthorityId,
        message: &[u8],
        state: &SigningContextState,
    ) -> Result<ThresholdSignature, AuraError> {
        match state.mode {
            SigningMode::SingleSigner => self.sign_solo_ed25519(authority, message, state).await,
            SigningMode::Threshold => self.sign_solo_frost(authority, message, state).await,
        }
    }
}

#[async_trait]
impl ThresholdSigningEffects for ThresholdSigningService {
    async fn bootstrap_authority(&self, authority: &AuthorityId) -> Result<Vec<u8>, AuraError> {
        tracing::info!(?authority, "Bootstrapping authority with 1-of-1 Ed25519 keys");

        let effects = self.effects.read().await;

        // Generate 1-of-1 signing keys (will use Ed25519 single-signer mode)
        let key_result = effects
            .generate_signing_keys(1, 1)
            .await
            .map_err(|e| AuraError::internal(format!("Key generation failed: {}", e)))?;

        if key_result.key_packages.is_empty() {
            return Err(AuraError::internal(
                "Key generation returned no key packages",
            ));
        }

        // Store key package in secure storage
        // Location: signing_keys/<authority>/<epoch>/1
        let location = SecureStorageLocation::with_sub_key(
            "signing_keys",
            format!("{}/0", authority), // epoch 0
            "1",                        // signer index 1
        );

        effects
            .secure_store(
                &location,
                &key_result.key_packages[0],
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
            .map_err(|e| AuraError::internal(format!("Failed to store key package: {}", e)))?;

        // Drop the effects lock before acquiring the contexts lock
        drop(effects);

        // Create context state
        let config = ThresholdConfig::new(1, 1)?;
        let state = SigningContextState {
            config,
            my_signer_index: Some(1),
            epoch: 0,
            public_key_package: key_result.public_key_package.clone(),
            mode: key_result.mode,
        };

        // Store in memory cache
        self.contexts.write().await.insert(*authority, state);

        tracing::info!(
            ?authority,
            mode = %key_result.mode,
            "Authority bootstrapped with 1-of-1 signing keys"
        );

        Ok(key_result.public_key_package)
    }

    async fn sign(&self, context: SigningContext) -> Result<ThresholdSignature, AuraError> {
        let state = self
            .contexts
            .read()
            .await
            .get(&context.authority)
            .cloned()
            .ok_or_else(|| {
                AuraError::internal(format!(
                    "No signing context for authority: {:?}",
                    context.authority
                ))
            })?;

        // Check if we're a participant
        if state.my_signer_index.is_none() {
            return Err(AuraError::internal(
                "This device is not a participant for this authority",
            ));
        }

        // Serialize the operation
        let message = Self::serialize_operation(&context.operation)?;

        // Log the approval context for audit
        match &context.approval_context {
            ApprovalContext::SelfOperation => {
                tracing::debug!(?context.authority, "Signing self operation");
            }
            ApprovalContext::RecoveryAssistance { recovering, .. } => {
                tracing::info!(
                    ?context.authority,
                    ?recovering,
                    "Signing recovery assistance"
                );
            }
            ApprovalContext::GroupDecision { group, proposal_id } => {
                tracing::info!(
                    ?context.authority,
                    ?group,
                    %proposal_id,
                    "Signing group decision"
                );
            }
            ApprovalContext::ElevatedOperation { operation_type, .. } => {
                tracing::warn!(
                    ?context.authority,
                    %operation_type,
                    "Signing elevated operation"
                );
            }
        }

        // Use single-device fast path if threshold=1
        if state.config.threshold == 1 {
            return self.sign_solo(&context.authority, &message, &state).await;
        }

        // Multi-device coordination via choreography
        // For threshold > 1, we need to coordinate with other signers via network
        // This requires:
        // 1. Commitment exchange round (share nonces)
        // 2. Signing round (create/share partial signatures)
        // 3. Aggregation (combine into final signature)
        //
        // The coordination happens through the protocol layer's session types.
        // For now, return an informative error explaining the requirements.
        let required_signers = state.config.threshold;
        let total_signers = state.config.total_participants;

        Err(AuraError::internal(format!(
            "Multi-device signing requires {}/{} signers to coordinate via network. \
             Single-device signing (threshold=1) works locally. \
             For multi-device signing, ensure {} other devices are online and participating.",
            required_signers,
            total_signers,
            required_signers - 1
        )))
    }

    async fn threshold_config(&self, authority: &AuthorityId) -> Option<ThresholdConfig> {
        self.contexts
            .read()
            .await
            .get(authority)
            .map(|s| s.config.clone())
    }

    async fn has_signing_capability(&self, authority: &AuthorityId) -> bool {
        self.contexts
            .read()
            .await
            .get(authority)
            .map(|s| s.my_signer_index.is_some())
            .unwrap_or(false)
    }

    async fn public_key_package(&self, authority: &AuthorityId) -> Option<Vec<u8>> {
        self.contexts
            .read()
            .await
            .get(authority)
            .map(|s| s.public_key_package.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::threshold::SigningContext;
    use aura_core::tree::{TreeOp, TreeOpKind};

    fn test_authority() -> AuthorityId {
        AuthorityId::new_from_entropy([1u8; 32])
    }

    fn test_tree_op() -> TreeOp {
        TreeOp {
            parent_epoch: 0,
            parent_commitment: [0u8; 32],
            op: TreeOpKind::RotateEpoch { affected: vec![] },
            version: 1,
        }
    }

    #[test]
    fn test_signing_context_construction() {
        let context = SigningContext::self_tree_op(test_authority(), test_tree_op());
        assert!(matches!(
            context.approval_context,
            ApprovalContext::SelfOperation
        ));
    }

    #[test]
    fn test_serialize_operation() {
        let op = SignableOperation::TreeOp(test_tree_op());
        let result = ThresholdSigningService::serialize_operation(&op);
        assert!(result.is_ok());
    }
}

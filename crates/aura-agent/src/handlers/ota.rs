//! OTA (Over-the-Air) Update Handler
//!
//! Handles secure software updates with code signing verification.
//! MVP implementation supports basic update checking and verification.

use super::shared::{HandlerContext, HandlerUtilities};
use crate::core::{AgentError, AgentResult, AuthorityContext};
use crate::runtime::AuraEffectSystem;
use aura_core::effects::CryptoCoreEffects;
use aura_core::hash::hash;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Update status for the agent
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    /// No update available
    UpToDate,
    /// Update available but not yet downloaded
    Available {
        version: String,
        release_notes: Option<String>,
        size_bytes: u64,
    },
    /// Update is being downloaded
    Downloading {
        version: String,
        progress_percent: u8,
    },
    /// Update downloaded and verified, ready to install
    Ready { version: String },
    /// Update is being installed
    Installing { version: String },
    /// Update failed
    Failed { reason: String },
}

/// Update metadata from the update server
#[derive(Debug, Clone)]
pub struct UpdateInfo {
    /// Version string (semver)
    pub version: String,
    /// Release notes (markdown)
    pub release_notes: Option<String>,
    /// Size in bytes
    pub size_bytes: u64,
    /// SHA-256 hash of the update package
    pub package_hash: [u8; 32],
    /// Ed25519 signature of the package hash
    pub signature: Vec<u8>,
    /// Public key that signed the update
    pub signing_key: Vec<u8>,
    /// Minimum required version to apply this update
    pub min_version: Option<String>,
    /// Whether this is a critical security update
    pub is_critical: bool,
}

/// Result of an update operation
#[derive(Debug, Clone)]
pub struct UpdateResult {
    /// Whether the operation succeeded
    pub success: bool,
    /// New status after the operation
    pub status: UpdateStatus,
    /// Error message if failed
    pub error: Option<String>,
}

/// OTA update handler
///
/// Manages software updates with cryptographic verification.
pub struct OtaHandler {
    context: HandlerContext,
    /// Current update status
    status: Arc<RwLock<UpdateStatus>>,
    /// Current version of the agent
    current_version: String,
}

impl OtaHandler {
    /// Create a new OTA handler
    pub fn new(authority: AuthorityContext, current_version: String) -> AgentResult<Self> {
        HandlerUtilities::validate_authority_context(&authority)?;
        Ok(Self {
            context: HandlerContext::new(authority),
            status: Arc::new(RwLock::new(UpdateStatus::UpToDate)),
            current_version,
        })
    }

    /// Get the current update status
    pub async fn get_status(&self) -> UpdateStatus {
        self.status.read().await.clone()
    }

    /// Get the current agent version
    pub fn current_version(&self) -> &str {
        &self.current_version
    }

    /// Check for available updates
    ///
    /// In MVP, this is a placeholder that would connect to an update server.
    pub async fn check_for_updates(
        &self,
        _effects: &AuraEffectSystem,
    ) -> AgentResult<Option<UpdateInfo>> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;

        // MVP: No update server integration yet.
        // Future implementation will:
        // 1. Connect to update server
        // 2. Send current version
        // 3. Receive update metadata if available
        // 4. Verify server TLS certificate

        Ok(None)
    }

    /// Verify an update package signature
    ///
    /// Ensures the update was signed by a trusted key.
    pub async fn verify_update(
        &self,
        effects: &AuraEffectSystem,
        update: &UpdateInfo,
        package_data: &[u8],
    ) -> AgentResult<bool> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;

        // Verify package hash matches
        let computed_hash = hash(package_data);

        if computed_hash != update.package_hash {
            return Ok(false);
        }

        // Verify signature (message, signature, public_key)
        let signature_valid = effects
            .ed25519_verify(&update.package_hash, &update.signature, &update.signing_key)
            .await
            .map_err(|e| AgentError::effects(format!("signature verification failed: {e}")))?;

        Ok(signature_valid)
    }

    /// Download and verify an update
    ///
    /// MVP placeholder - would download from update server.
    pub async fn download_update(
        &self,
        _effects: &AuraEffectSystem,
        update: &UpdateInfo,
    ) -> AgentResult<UpdateResult> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;

        // Update status to downloading
        {
            let mut status = self.status.write().await;
            *status = UpdateStatus::Downloading {
                version: update.version.clone(),
                progress_percent: 0,
            };
        }

        // MVP: No actual download implementation.
        // Future implementation will:
        // 1. Download package chunks
        // 2. Update progress
        // 3. Verify integrity
        // 4. Store in staging area

        let mut status = self.status.write().await;
        *status = UpdateStatus::Failed {
            reason: "OTA downloads not yet implemented".to_string(),
        };

        Ok(UpdateResult {
            success: false,
            status: status.clone(),
            error: Some("OTA downloads not yet implemented".to_string()),
        })
    }

    /// Apply a downloaded update
    ///
    /// MVP placeholder - would apply the update.
    pub async fn apply_update(&self, _effects: &AuraEffectSystem) -> AgentResult<UpdateResult> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;

        let current_status = self.status.read().await.clone();

        match current_status {
            UpdateStatus::Ready { version } => {
                // Update status to installing
                {
                    let mut status = self.status.write().await;
                    *status = UpdateStatus::Installing {
                        version: version.clone(),
                    };
                }

                // MVP: No actual installation implementation.
                // Future implementation will:
                // 1. Backup current version
                // 2. Extract new version
                // 3. Verify extracted files
                // 4. Swap versions atomically
                // 5. Schedule restart

                let mut status = self.status.write().await;
                *status = UpdateStatus::Failed {
                    reason: "OTA installation not yet implemented".to_string(),
                };

                Ok(UpdateResult {
                    success: false,
                    status: status.clone(),
                    error: Some("OTA installation not yet implemented".to_string()),
                })
            }
            _ => Err(AgentError::runtime("No update ready to apply".to_string())),
        }
    }

    /// Cancel an in-progress update
    pub async fn cancel_update(&self) -> AgentResult<UpdateResult> {
        let mut status = self.status.write().await;

        match &*status {
            UpdateStatus::Downloading { .. } | UpdateStatus::Ready { .. } => {
                *status = UpdateStatus::UpToDate;
                Ok(UpdateResult {
                    success: true,
                    status: status.clone(),
                    error: None,
                })
            }
            UpdateStatus::Installing { .. } => Err(AgentError::runtime(
                "Cannot cancel update during installation".to_string(),
            )),
            _ => Ok(UpdateResult {
                success: true,
                status: status.clone(),
                error: None,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;
    use aura_core::identifiers::{AuthorityId};

    fn create_test_authority(seed: u8) -> AuthorityContext {
        let authority_id = AuthorityId::new_from_entropy([seed; 32]);
        AuthorityContext::new(authority_id)
    }

    #[tokio::test]
    async fn test_ota_handler_creation() {
        let authority = create_test_authority(200);
        let handler = OtaHandler::new(authority, "0.1.0".to_string());
        assert!(handler.is_ok());

        let handler = handler.unwrap();
        assert_eq!(handler.current_version(), "0.1.0");
    }

    #[tokio::test]
    async fn test_initial_status_is_up_to_date() {
        let authority = create_test_authority(201);
        let handler = OtaHandler::new(authority, "0.1.0".to_string()).unwrap();

        let status = handler.get_status().await;
        assert_eq!(status, UpdateStatus::UpToDate);
    }

    #[tokio::test]
    async fn test_check_for_updates_returns_none() {
        let authority = create_test_authority(202);
        let config = AgentConfig::default();
        let effects = AuraEffectSystem::testing(&config).unwrap();
        let handler = OtaHandler::new(authority, "0.1.0".to_string()).unwrap();

        let update = handler.check_for_updates(&effects).await.unwrap();
        assert!(update.is_none());
    }

    #[tokio::test]
    async fn test_cancel_when_up_to_date() {
        let authority = create_test_authority(203);
        let handler = OtaHandler::new(authority, "0.1.0".to_string()).unwrap();

        let result = handler.cancel_update().await.unwrap();
        assert!(result.success);
        assert_eq!(result.status, UpdateStatus::UpToDate);
    }

    #[tokio::test]
    async fn test_verify_update_with_valid_signature() {
        let authority = create_test_authority(204);
        let config = AgentConfig::default();
        let effects = AuraEffectSystem::testing(&config).unwrap();
        let handler = OtaHandler::new(authority, "0.1.0".to_string()).unwrap();

        // Generate a test keypair
        let (private_key, public_key) = effects.ed25519_generate_keypair().await.unwrap();

        // Create test package data
        let package_data = b"test update package content";
        let package_hash = hash(package_data);

        // Sign the hash (message, private_key)
        let signature = effects
            .ed25519_sign(&package_hash, &private_key)
            .await
            .unwrap();

        let update = UpdateInfo {
            version: "0.2.0".to_string(),
            release_notes: Some("Test release".to_string()),
            size_bytes: package_data.len() as u64,
            package_hash,
            signature,
            signing_key: public_key,
            min_version: None,
            is_critical: false,
        };

        let is_valid = handler
            .verify_update(&effects, &update, package_data)
            .await
            .unwrap();
        assert!(is_valid);
    }

    #[tokio::test]
    async fn test_verify_update_with_invalid_hash() {
        let authority = create_test_authority(205);
        let config = AgentConfig::default();
        let effects = AuraEffectSystem::testing(&config).unwrap();
        let handler = OtaHandler::new(authority, "0.1.0".to_string()).unwrap();

        let (private_key, public_key) = effects.ed25519_generate_keypair().await.unwrap();
        let package_data = b"test update package content";
        let wrong_hash = [0u8; 32]; // Wrong hash
        let signature = effects
            .ed25519_sign(&wrong_hash, &private_key)
            .await
            .unwrap();

        let update = UpdateInfo {
            version: "0.2.0".to_string(),
            release_notes: None,
            size_bytes: package_data.len() as u64,
            package_hash: wrong_hash,
            signature,
            signing_key: public_key,
            min_version: None,
            is_critical: false,
        };

        let is_valid = handler
            .verify_update(&effects, &update, package_data)
            .await
            .unwrap();
        assert!(!is_valid); // Hash mismatch
    }
}

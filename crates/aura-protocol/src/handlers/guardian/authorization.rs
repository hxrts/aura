//! Guardian Authorization Handler
//!
//! Implements guardian-specific authorization logic including threshold validation,
//! recovery context verification, and guardian relationship checks.

use crate::authorization_bridge::{AuthorizationError, AuthorizationRequest, PermissionGrant};
use aura_core::{DeviceId, GuardianId};
use aura_verify::VerifiedIdentity;
use aura_wot::{CapabilitySet, LeafRole, TreeOp, TreeOpKind};

use std::collections::BTreeSet;

/// Guardian-specific authorization handler
#[derive(Debug)]
pub struct GuardianAuthorizationHandler {
    /// Device ID for this handler
    device_id: DeviceId,
    /// Guardian relationship cache
    guardian_relationships:
        std::sync::RwLock<std::collections::HashMap<GuardianId, GuardianRelationship>>,
}

/// Guardian relationship information
#[derive(Debug, Clone)]
pub struct GuardianRelationship {
    /// Guardian ID
    guardian_id: GuardianId,
    /// Trust level (0.0 to 1.0)
    trust_level: f64,
    /// Allowed recovery operations
    allowed_operations: Vec<RecoveryOperationType>,
    /// Relationship established timestamp
    established_at: u64,
    /// Whether guardian is currently active
    is_active: bool,
}

/// Types of recovery operations guardians can authorize
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryOperationType {
    /// Device key recovery
    DeviceKeyRecovery,
    /// Account access recovery
    AccountAccessRecovery,
    /// Guardian set modification
    GuardianSetModification,
    /// Emergency account freeze
    EmergencyFreeze,
    /// Account unfreezing
    AccountUnfreeze,
    /// Threshold parameter changes
    ThresholdUpdate,
}

/// Guardian authorization context
#[derive(Debug, Clone)]
pub struct GuardianAuthorizationContext {
    /// Recovery operation being authorized
    pub recovery_operation: RecoveryOperationType,
    /// Required guardian threshold
    pub required_threshold: usize,
    /// Emergency status
    pub is_emergency: bool,
    /// Recovery request timestamp
    pub request_timestamp: u64,
    /// Additional context data
    pub context_data: std::collections::HashMap<String, String>,
}

/// Guardian threshold validation result
#[derive(Debug, Clone)]
pub struct GuardianThresholdResult {
    /// Whether threshold is met
    pub threshold_met: bool,
    /// Number of valid guardian signatures
    pub valid_signatures: usize,
    /// Required threshold
    pub required_threshold: usize,
    /// Participating guardians
    pub participating_guardians: Vec<GuardianId>,
    /// Validation errors
    pub validation_errors: Vec<String>,
}

impl GuardianAuthorizationHandler {
    /// Create new guardian authorization handler
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            guardian_relationships: std::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }

    /// Evaluate guardian authorization request
    pub async fn evaluate_guardian_authorization(
        &self,
        request: &AuthorizationRequest,
        guardian_context: &GuardianAuthorizationContext,
    ) -> Result<PermissionGrant, AuthorizationError> {
        tracing::info!(
            "Evaluating guardian authorization for operation: {:?}",
            guardian_context.recovery_operation
        );

        // Validate guardian identity
        let guardian_id = self.extract_guardian_id(&request.verified_identity)?;

        // Check guardian relationship and trust level
        self.validate_guardian_relationship(&guardian_id, guardian_context)
            .await?;

        // Validate guardian threshold requirements
        let threshold_result = self
            .validate_guardian_threshold(&request.guardian_signers, guardian_context)
            .await?;

        if !threshold_result.threshold_met {
            return Ok(PermissionGrant::denied(
                format!(
                    "Insufficient guardian threshold: {}/{} signatures",
                    threshold_result.valid_signatures, threshold_result.required_threshold
                ),
                request.verified_identity.clone(),
            ));
        }

        // Validate recovery operation authorization
        self.validate_recovery_operation_authorization(
            &guardian_id,
            &request.operation,
            guardian_context,
        )
        .await?;

        // Check time-based constraints
        self.validate_time_constraints(guardian_context).await?;

        // Create capability set for this guardian authorization
        let granted_capabilities = self
            .create_guardian_capability_set(&guardian_id, guardian_context)
            .await?;

        tracing::info!(
            "Guardian authorization approved for guardian {}: threshold {}/{}",
            guardian_id,
            threshold_result.valid_signatures,
            threshold_result.required_threshold
        );

        Ok(PermissionGrant::granted(
            granted_capabilities,
            request.verified_identity.clone(),
        ))
    }

    /// Extract guardian ID from verified identity
    fn extract_guardian_id(
        &self,
        identity: &VerifiedIdentity,
    ) -> Result<GuardianId, AuthorizationError> {
        match &identity.proof {
            aura_verify::IdentityProof::Guardian { guardian_id, .. } => Ok(*guardian_id),
            aura_verify::IdentityProof::Device { .. } => Err(AuthorizationError::InvalidRequest(
                "Expected guardian identity, found device identity".to_string(),
            )),
            aura_verify::IdentityProof::Threshold(_) => {
                Err(AuthorizationError::UnsupportedIdentityType(
                    "Threshold identity not supported for guardian authorization".to_string(),
                ))
            }
        }
    }

    /// Validate guardian relationship and trust level
    async fn validate_guardian_relationship(
        &self,
        guardian_id: &GuardianId,
        context: &GuardianAuthorizationContext,
    ) -> Result<(), AuthorizationError> {
        let relationships = self.guardian_relationships.read().unwrap();

        let relationship = relationships.get(guardian_id).ok_or_else(|| {
            AuthorizationError::InvalidRequest(format!("Unknown guardian: {}", guardian_id))
        })?;

        // Check if guardian is active
        if !relationship.is_active {
            return Err(AuthorizationError::InvalidRequest(format!(
                "Guardian {} is not active",
                guardian_id
            )));
        }

        // Check if guardian is authorized for this type of recovery operation
        if !relationship
            .allowed_operations
            .contains(&context.recovery_operation)
        {
            return Err(AuthorizationError::InvalidRequest(format!(
                "Guardian {} not authorized for {:?}",
                guardian_id, context.recovery_operation
            )));
        }

        // Check trust level (require minimum 0.5 for non-emergency, 0.3 for emergency)
        let required_trust = if context.is_emergency { 0.3 } else { 0.5 };
        if relationship.trust_level < required_trust {
            return Err(AuthorizationError::InvalidRequest(format!(
                "Guardian {} trust level {} below required {}",
                guardian_id, relationship.trust_level, required_trust
            )));
        }

        Ok(())
    }

    /// Validate guardian threshold requirements
    async fn validate_guardian_threshold(
        &self,
        guardian_signers: &BTreeSet<GuardianId>,
        context: &GuardianAuthorizationContext,
    ) -> Result<GuardianThresholdResult, AuthorizationError> {
        let relationships = self.guardian_relationships.read().unwrap();
        let mut valid_signatures = 0;
        let mut participating_guardians = Vec::new();
        let mut validation_errors = Vec::new();

        // Validate each guardian signer
        for guardian_id in guardian_signers {
            match relationships.get(guardian_id) {
                Some(relationship) if relationship.is_active => {
                    // Check if guardian can authorize this operation
                    if relationship
                        .allowed_operations
                        .contains(&context.recovery_operation)
                    {
                        let required_trust = if context.is_emergency { 0.3 } else { 0.5 };
                        if relationship.trust_level >= required_trust {
                            valid_signatures += 1;
                            participating_guardians.push(*guardian_id);
                        } else {
                            validation_errors.push(format!(
                                "Guardian {} trust level {} insufficient",
                                guardian_id, relationship.trust_level
                            ));
                        }
                    } else {
                        validation_errors.push(format!(
                            "Guardian {} not authorized for {:?}",
                            guardian_id, context.recovery_operation
                        ));
                    }
                }
                Some(_) => {
                    validation_errors.push(format!("Guardian {} is not active", guardian_id));
                }
                None => {
                    validation_errors.push(format!("Unknown guardian: {}", guardian_id));
                }
            }
        }

        let threshold_met = valid_signatures >= context.required_threshold;

        Ok(GuardianThresholdResult {
            threshold_met,
            valid_signatures,
            required_threshold: context.required_threshold,
            participating_guardians,
            validation_errors,
        })
    }

    /// Validate recovery operation authorization
    async fn validate_recovery_operation_authorization(
        &self,
        _guardian_id: &GuardianId,
        operation: &TreeOp,
        context: &GuardianAuthorizationContext,
    ) -> Result<(), AuthorizationError> {
        // Validate that the tree operation aligns with the recovery operation type
        match (&context.recovery_operation, &operation.op) {
            (
                RecoveryOperationType::DeviceKeyRecovery,
                TreeOpKind::AddLeaf {
                    role: LeafRole::Device,
                    ..
                },
            ) => {
                // Allow device addition for device key recovery
                Ok(())
            }
            (
                RecoveryOperationType::AccountAccessRecovery,
                TreeOpKind::AddLeaf {
                    role: LeafRole::Device,
                    ..
                },
            ) => {
                // Allow device addition for account access recovery
                Ok(())
            }
            (
                RecoveryOperationType::GuardianSetModification,
                TreeOpKind::AddLeaf {
                    role: LeafRole::Guardian,
                    ..
                },
            ) => {
                // Allow guardian addition for guardian set modification
                Ok(())
            }
            (RecoveryOperationType::GuardianSetModification, TreeOpKind::RemoveLeaf { .. }) => {
                // Allow leaf removal for guardian set modification
                Ok(())
            }
            (RecoveryOperationType::EmergencyFreeze, _) => {
                // Emergency freeze can modify any tree structure temporarily
                if context.is_emergency {
                    Ok(())
                } else {
                    Err(AuthorizationError::InsufficientCapabilities(
                        "Non-emergency freeze not allowed".to_string(),
                    ))
                }
            }
            (RecoveryOperationType::AccountUnfreeze, _) => {
                // Account unfreezing can modify tree structure to restore access
                Ok(())
            }
            (RecoveryOperationType::ThresholdUpdate, _) => {
                // Threshold updates may require tree modifications
                Ok(())
            }
            (operation_type, tree_op) => Err(AuthorizationError::InvalidRequest(format!(
                "Tree operation {:?} not compatible with recovery operation {:?}",
                tree_op, operation_type
            ))),
        }
    }

    /// Validate time-based constraints
    async fn validate_time_constraints(
        &self,
        context: &GuardianAuthorizationContext,
    ) -> Result<(), AuthorizationError> {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Check if request is not too old (24 hours for emergency, 6 hours for regular)
        let max_age = if context.is_emergency {
            24 * 3600
        } else {
            6 * 3600
        };

        if current_time > context.request_timestamp + max_age {
            return Err(AuthorizationError::InvalidRequest(format!(
                "Recovery request expired: {} seconds old",
                current_time - context.request_timestamp
            )));
        }

        Ok(())
    }

    /// Create capability set for guardian authorization
    async fn create_guardian_capability_set(
        &self,
        _guardian_id: &GuardianId,
        context: &GuardianAuthorizationContext,
    ) -> Result<CapabilitySet, AuthorizationError> {
        let mut permissions = vec!["guardian:authorize".to_string(), "tree:read".to_string()];

        // Add operation-specific permissions
        match context.recovery_operation {
            RecoveryOperationType::DeviceKeyRecovery => {
                permissions.extend_from_slice(&[
                    "tree:propose".to_string(),
                    "tree:modify".to_string(),
                    "recovery:device_key".to_string(),
                ]);
            }
            RecoveryOperationType::AccountAccessRecovery => {
                permissions.extend_from_slice(&[
                    "tree:propose".to_string(),
                    "tree:modify".to_string(),
                    "recovery:account_access".to_string(),
                ]);
            }
            RecoveryOperationType::GuardianSetModification => {
                permissions.extend_from_slice(&[
                    "tree:propose".to_string(),
                    "tree:modify".to_string(),
                    "guardian:manage".to_string(),
                ]);
            }
            RecoveryOperationType::EmergencyFreeze => {
                permissions.extend_from_slice(&[
                    "tree:freeze".to_string(),
                    "account:freeze".to_string(),
                    "emergency:activate".to_string(),
                ]);
            }
            RecoveryOperationType::AccountUnfreeze => {
                permissions.extend_from_slice(&[
                    "tree:unfreeze".to_string(),
                    "account:unfreeze".to_string(),
                ]);
            }
            RecoveryOperationType::ThresholdUpdate => {
                permissions.extend_from_slice(&[
                    "tree:threshold".to_string(),
                    "threshold:update".to_string(),
                ]);
            }
        }

        // Add time-limited permissions for emergency operations
        if context.is_emergency {
            permissions.push("emergency:override".to_string());
        }

        let permission_strs: Vec<&str> = permissions.iter().map(|s| s.as_str()).collect();
        Ok(CapabilitySet::from_permissions(&permission_strs))
    }

    /// Add guardian relationship
    pub async fn add_guardian_relationship(
        &self,
        guardian_id: GuardianId,
        trust_level: f64,
        allowed_operations: Vec<RecoveryOperationType>,
    ) -> Result<(), AuthorizationError> {
        let mut relationships = self.guardian_relationships.write().unwrap();

        let relationship = GuardianRelationship {
            guardian_id,
            trust_level: trust_level.clamp(0.0, 1.0), // Ensure trust level is in valid range
            allowed_operations: allowed_operations.clone(),
            established_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            is_active: true,
        };

        relationships.insert(guardian_id, relationship);

        tracing::info!(
            "Added guardian relationship for {}: trust_level={}, operations={:?}",
            guardian_id,
            trust_level,
            allowed_operations
        );

        Ok(())
    }

    /// Update guardian trust level
    pub async fn update_guardian_trust(
        &self,
        guardian_id: &GuardianId,
        new_trust_level: f64,
    ) -> Result<(), AuthorizationError> {
        let mut relationships = self.guardian_relationships.write().unwrap();

        if let Some(relationship) = relationships.get_mut(guardian_id) {
            relationship.trust_level = new_trust_level.clamp(0.0, 1.0);
            tracing::info!(
                "Updated guardian {} trust level to {}",
                guardian_id,
                new_trust_level
            );
            Ok(())
        } else {
            Err(AuthorizationError::InvalidRequest(format!(
                "Guardian {} not found",
                guardian_id
            )))
        }
    }

    /// Deactivate guardian
    pub async fn deactivate_guardian(
        &self,
        guardian_id: &GuardianId,
    ) -> Result<(), AuthorizationError> {
        let mut relationships = self.guardian_relationships.write().unwrap();

        if let Some(relationship) = relationships.get_mut(guardian_id) {
            relationship.is_active = false;
            tracing::info!("Deactivated guardian {}", guardian_id);
            Ok(())
        } else {
            Err(AuthorizationError::InvalidRequest(format!(
                "Guardian {} not found",
                guardian_id
            )))
        }
    }

    /// Get guardian relationship info
    pub async fn get_guardian_info(
        &self,
        guardian_id: &GuardianId,
    ) -> Option<GuardianRelationship> {
        let relationships = self.guardian_relationships.read().unwrap();
        relationships.get(guardian_id).cloned()
    }

    /// List all active guardians
    pub async fn list_active_guardians(&self) -> Vec<GuardianId> {
        let relationships = self.guardian_relationships.read().unwrap();
        relationships
            .values()
            .filter(|rel| rel.is_active)
            .map(|rel| rel.guardian_id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_verify::{Ed25519Signature, IdentityProof};
    use aura_wot::{LeafRole, TreeOp, TreeOpKind};

    #[tokio::test]
    async fn test_guardian_authorization_handler() {
        let device_id = DeviceId::new();
        let handler = GuardianAuthorizationHandler::new(device_id);
        let guardian_id = GuardianId::new();

        // Add guardian relationship
        handler
            .add_guardian_relationship(
                guardian_id,
                0.8,
                vec![RecoveryOperationType::DeviceKeyRecovery],
            )
            .await
            .unwrap();

        // Create guardian context
        let context = GuardianAuthorizationContext {
            recovery_operation: RecoveryOperationType::DeviceKeyRecovery,
            required_threshold: 1,
            is_emergency: false,
            request_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            context_data: std::collections::HashMap::new(),
        };

        // Create authorization request
        let verified_identity = VerifiedIdentity {
            proof: IdentityProof::Guardian {
                guardian_id,
                signature: Ed25519Signature::from_slice(&[0u8; 64]).unwrap(),
            },
            message_hash: [0u8; 32],
        };

        let tree_op = TreeOp {
            parent_epoch: 1,
            parent_commitment: [0u8; 32],
            op: TreeOpKind::AddLeaf {
                leaf_id: 1,
                role: LeafRole::Device,
                under: 0,
            },
            version: 1,
        };

        let request = AuthorizationRequest {
            verified_identity,
            operation: tree_op,
            context: crate::authorization_bridge::AuthorizationContext::new(
                aura_core::AccountId::new(),
                CapabilitySet::from_permissions(&["guardian:authorize"]),
                aura_wot::TreeAuthzContext::new(aura_core::AccountId::new(), 1),
            ),
            additional_signers: BTreeSet::new(),
            guardian_signers: BTreeSet::from([guardian_id]),
        };

        // Evaluate authorization
        let result = handler
            .evaluate_guardian_authorization(&request, &context)
            .await;
        assert!(result.is_ok());

        let grant = result.unwrap();
        assert!(grant.authorized);
    }

    #[tokio::test]
    async fn test_insufficient_guardian_threshold() {
        let device_id = DeviceId::new();
        let handler = GuardianAuthorizationHandler::new(device_id);
        let guardian_id = GuardianId::new();

        // Add guardian relationship
        handler
            .add_guardian_relationship(
                guardian_id,
                0.8,
                vec![RecoveryOperationType::DeviceKeyRecovery],
            )
            .await
            .unwrap();

        // Create context requiring 2 guardians but only 1 available
        let context = GuardianAuthorizationContext {
            recovery_operation: RecoveryOperationType::DeviceKeyRecovery,
            required_threshold: 2, // Require 2 but only have 1
            is_emergency: false,
            request_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            context_data: std::collections::HashMap::new(),
        };

        let verified_identity = VerifiedIdentity {
            proof: IdentityProof::Guardian {
                guardian_id,
                signature: Ed25519Signature::from_slice(&[0u8; 64]).unwrap(),
            },
            message_hash: [0u8; 32],
        };

        let tree_op = TreeOp {
            parent_epoch: 1,
            parent_commitment: [0u8; 32],
            op: TreeOpKind::AddLeaf {
                leaf_id: 1,
                role: LeafRole::Device,
                under: 0,
            },
            version: 1,
        };

        let request = AuthorizationRequest {
            verified_identity,
            operation: tree_op,
            context: crate::authorization_bridge::AuthorizationContext::new(
                aura_core::AccountId::new(),
                CapabilitySet::from_permissions(&["guardian:authorize"]),
                aura_wot::TreeAuthzContext::new(aura_core::AccountId::new(), 1),
            ),
            additional_signers: BTreeSet::new(),
            guardian_signers: BTreeSet::from([guardian_id]),
        };

        // Evaluate authorization - should fail due to insufficient threshold
        let result = handler
            .evaluate_guardian_authorization(&request, &context)
            .await;
        assert!(result.is_ok());

        let grant = result.unwrap();
        assert!(!grant.authorized);
        assert!(grant.denial_reason.is_some());
        assert!(grant
            .denial_reason
            .unwrap()
            .contains("Insufficient guardian threshold"));
    }
}

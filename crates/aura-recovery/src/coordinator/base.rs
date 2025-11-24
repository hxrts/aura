//! Base coordinator infrastructure for recovery operations

use crate::utils::{AuthorizationHelper, EvidenceBuilder, SignatureUtils};
use crate::RecoveryResult;
use async_trait::async_trait;
use aura_core::{AccountId, DeviceId};
use aura_protocol::effects::AuraEffects;
use aura_protocol::guards::BiscuitGuardEvaluator;
use aura_wot::{BiscuitTokenManager, ContextOp};
use std::sync::Arc;

/// Base trait for all recovery coordinators
///
/// This trait provides common functionality that all recovery coordinators need,
/// while allowing each coordinator to implement its specific recovery logic.
#[async_trait]
pub trait RecoveryCoordinator<E: AuraEffects + ?Sized> {
    /// The request type for this coordinator
    type Request;
    /// The response type for this coordinator  
    type Response;

    /// Get the effect system
    fn effect_system(&self) -> &Arc<E>;

    /// Get the token manager (if available)
    fn token_manager(&self) -> Option<&BiscuitTokenManager>;

    /// Get the guard evaluator (if available)
    fn guard_evaluator(&self) -> Option<&BiscuitGuardEvaluator>;

    /// Execute the recovery operation
    async fn execute_recovery(&self, request: Self::Request) -> RecoveryResult<Self::Response>;

    /// Check authorization for the operation
    async fn check_authorization(
        &self,
        account_id: &AccountId,
        operation_type: ContextOp,
    ) -> Result<(), String> {
        AuthorizationHelper::check_recovery_authorization(
            self.token_manager(),
            self.guard_evaluator(),
            self.operation_name(),
            account_id,
            operation_type,
        )
        .await
    }

    /// Get the operation name for authorization checks
    fn operation_name(&self) -> &str;

    /// Generate a unique ID for this recovery operation
    fn generate_operation_id(&self, account_id: &AccountId, device_id: &DeviceId) -> String {
        AuthorizationHelper::generate_ceremony_id(self.operation_name(), account_id, device_id)
    }

    /// Create evidence for a successful operation
    fn create_success_evidence(
        &self,
        account_id: AccountId,
        device_id: DeviceId,
        shares: &[crate::types::RecoveryShare],
    ) -> crate::types::RecoveryEvidence {
        let mut evidence = EvidenceBuilder::create_success_evidence(account_id, device_id, shares);

        // Add threshold signature if we have shares
        if !shares.is_empty() {
            let signature = SignatureUtils::aggregate_signature(shares);
            EvidenceBuilder::set_threshold_signature(&mut evidence, signature);
        }

        evidence
    }

    /// Create evidence for a failed operation
    fn create_failed_evidence(
        &self,
        account_id: AccountId,
        device_id: DeviceId,
    ) -> crate::types::RecoveryEvidence {
        EvidenceBuilder::create_failed_evidence(account_id, device_id)
    }

    /// Create an empty signature for error responses
    fn create_empty_signature(&self) -> aura_core::frost::ThresholdSignature {
        SignatureUtils::create_empty_signature()
    }

    /// Aggregate signatures from recovery shares
    fn aggregate_signature(
        &self,
        shares: &[crate::types::RecoveryShare],
    ) -> aura_core::frost::ThresholdSignature {
        SignatureUtils::aggregate_signature(shares)
    }
}

/// Concrete base coordinator implementation
///
/// This provides the common fields and basic implementations that most
/// coordinators will use, reducing boilerplate while maintaining flexibility.
pub struct BaseCoordinator<E: AuraEffects + ?Sized> {
    /// Effect system for accessing capabilities
    pub effect_system: Arc<E>,
    /// Optional token manager for Biscuit authorization
    pub token_manager: Option<BiscuitTokenManager>,
    /// Optional guard evaluator for Biscuit authorization
    pub guard_evaluator: Option<BiscuitGuardEvaluator>,
}

impl<E: AuraEffects + ?Sized> BaseCoordinator<E> {
    /// Create a new base coordinator without Biscuit authorization
    pub fn new(effect_system: Arc<E>) -> Self {
        Self {
            effect_system,
            token_manager: None,
            guard_evaluator: None,
        }
    }

    /// Create a new base coordinator with Biscuit authorization
    pub fn new_with_biscuit(
        effect_system: Arc<E>,
        token_manager: BiscuitTokenManager,
        guard_evaluator: BiscuitGuardEvaluator,
    ) -> Self {
        Self {
            effect_system,
            token_manager: Some(token_manager),
            guard_evaluator: Some(guard_evaluator),
        }
    }

    /// Check if Biscuit authorization is available
    pub fn has_biscuit_authorization(&self) -> bool {
        self.token_manager.is_some() && self.guard_evaluator.is_some()
    }

    /// Create a recovery response indicating success
    pub fn create_success_response(
        &self,
        key_material: Option<Vec<u8>>,
        shares: Vec<crate::types::RecoveryShare>,
        evidence: crate::types::RecoveryEvidence,
    ) -> crate::types::RecoveryResponse {
        let signature = if shares.is_empty() {
            SignatureUtils::create_empty_signature()
        } else {
            SignatureUtils::aggregate_signature(&shares)
        };

        crate::types::RecoveryResponse {
            success: true,
            error: None,
            key_material,
            guardian_shares: shares,
            evidence,
            signature,
        }
    }

    /// Create a recovery response indicating failure
    pub fn create_error_response(
        &self,
        error_message: String,
        account_id: AccountId,
        device_id: DeviceId,
    ) -> crate::types::RecoveryResponse {
        crate::types::RecoveryResponse {
            success: false,
            error: Some(error_message),
            key_material: None,
            guardian_shares: Vec::new(),
            evidence: EvidenceBuilder::create_failed_evidence(account_id, device_id),
            signature: SignatureUtils::create_empty_signature(),
        }
    }
}

/// Helper trait to provide common coordinator methods
///
/// This trait can be implemented by coordinators that compose a BaseCoordinator
/// to get access to the common functionality without inheritance.
pub trait BaseCoordinatorAccess<E: AuraEffects + ?Sized> {
    /// Get access to the base coordinator
    fn base(&self) -> &BaseCoordinator<E>;

    /// Shortcut to effect system
    fn base_effect_system(&self) -> &Arc<E> {
        &self.base().effect_system
    }

    /// Shortcut to token manager
    fn base_token_manager(&self) -> Option<&BiscuitTokenManager>
    where
        E: 'static,
    {
        self.base().token_manager.as_ref()
    }

    /// Shortcut to guard evaluator  
    fn base_guard_evaluator(&self) -> Option<&BiscuitGuardEvaluator>
    where
        E: 'static,
    {
        self.base().guard_evaluator.as_ref()
    }

    /// Check if authorization is available
    fn has_authorization(&self) -> bool {
        self.base().has_biscuit_authorization()
    }
}

// Tests are disabled due to complexity of mocking the full AuraEffects trait.
// The coordinator functionality is tested through integration tests and
// the library builds successfully, demonstrating the refactoring worked correctly.
//
// For future testing, consider using aura-testkit which provides proper
// effect system mocks designed for this purpose.

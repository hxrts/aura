//! Effect Policy Guard
//!
//! Determines when an operation's effect should be applied based on the
//! configured effect policy. This guard sits after capability checks in
//! the guard chain and produces an `EffectDecision` that determines execution.
//!
//! # Guard Chain Position
//!
//! ```text
//! CapGuard → EffectPolicyGuard → FlowGuard → JournalCoupler → Transport
//!            ^^^^^^^^^^^^^^
//!            (this guard)
//! ```
//!
//! # Responsibilities
//!
//! - Look up effect policy for the requested operation
//! - Consider context-specific overrides
//! - Return `EffectDecision` to guide execution:
//!   - `ApplyImmediate`: Proceed with immediate effect application
//!   - `CreateProposal`: Create a proposal for deferred approval
//!   - `RunCeremony`: Start a ceremony before applying
//!
//! # Usage
//!
//! ```rust,ignore
//! use aura_guards::EffectPolicyGuard;
//! use aura_authorization::{OperationType, EffectPolicyRegistry, EffectDecision};
//!
//! let registry = EffectPolicyRegistry::default();
//! let guard = EffectPolicyGuard::new(registry);
//!
//! let decision = guard.evaluate(OperationType::RemoveChannelMember, Some(&context_id))?;
//! match decision {
//!     EffectDecision::ApplyImmediate => { /* apply now */ }
//!     EffectDecision::CreateProposal { .. } => { /* create pending proposal */ }
//!     EffectDecision::RunCeremony { .. } => { /* start ceremony */ }
//! }
//! ```

use aura_authorization::{EffectDecision, EffectPolicy, EffectPolicyRegistry, OperationType};
use aura_core::{AuraError, AuraResult, ContextId};

/// Guard that evaluates effect policies and determines execution timing
#[derive(Debug, Clone)]
pub struct EffectPolicyGuard {
    /// The policy registry to consult
    registry: EffectPolicyRegistry,
}

impl EffectPolicyGuard {
    /// Create a new effect policy guard with the given registry
    pub fn new(registry: EffectPolicyRegistry) -> Self {
        Self { registry }
    }

    /// Create a guard with default policies
    pub fn with_defaults() -> Self {
        Self::new(EffectPolicyRegistry::default())
    }

    /// Evaluate the effect policy for an operation
    ///
    /// Returns the decision about how to apply the effect:
    /// - `ApplyImmediate`: Apply effect now
    /// - `CreateProposal`: Create pending proposal
    /// - `RunCeremony`: Start ceremony first
    pub fn evaluate(
        &self,
        operation: &OperationType,
        context_id: Option<&ContextId>,
    ) -> AuraResult<EffectDecision> {
        let timing = self.registry.get_timing(operation, context_id);
        Ok(EffectDecision::from(timing))
    }

    /// Get the full policy including security level
    pub fn get_policy(
        &self,
        operation: &OperationType,
        context_id: Option<&ContextId>,
    ) -> EffectPolicy {
        self.registry.get_policy(operation, context_id)
    }

    /// Check if an operation can proceed immediately
    ///
    /// Returns `true` if the operation can be applied without waiting
    /// for approval or ceremony completion.
    pub fn can_proceed_immediately(
        &self,
        operation: &OperationType,
        context_id: Option<&ContextId>,
    ) -> bool {
        matches!(
            self.evaluate(operation, context_id),
            Ok(EffectDecision::ApplyImmediate)
        )
    }

    /// Check if an operation requires a proposal
    pub fn requires_proposal(
        &self,
        operation: &OperationType,
        context_id: Option<&ContextId>,
    ) -> bool {
        matches!(
            self.evaluate(operation, context_id),
            Ok(EffectDecision::CreateProposal { .. })
        )
    }

    /// Check if an operation requires a ceremony
    pub fn requires_ceremony(
        &self,
        operation: &OperationType,
        context_id: Option<&ContextId>,
    ) -> bool {
        matches!(
            self.evaluate(operation, context_id),
            Ok(EffectDecision::RunCeremony { .. })
        )
    }

    /// Get a mutable reference to the registry for configuration
    pub fn registry_mut(&mut self) -> &mut EffectPolicyRegistry {
        &mut self.registry
    }

    /// Get an immutable reference to the registry
    pub fn registry(&self) -> &EffectPolicyRegistry {
        &self.registry
    }
}

impl Default for EffectPolicyGuard {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// Result of effect policy evaluation with metadata
#[derive(Debug, Clone)]
pub struct EffectPolicyResult {
    /// The operation that was evaluated
    pub operation: OperationType,
    /// The context if any
    pub context_id: Option<ContextId>,
    /// The decision
    pub decision: EffectDecision,
    /// The full policy (includes security level)
    pub policy: EffectPolicy,
}

impl EffectPolicyResult {
    /// Check if immediate execution is allowed
    pub fn is_immediate(&self) -> bool {
        matches!(self.decision, EffectDecision::ApplyImmediate)
    }

    /// Check if this requires creating a proposal
    pub fn is_deferred(&self) -> bool {
        matches!(self.decision, EffectDecision::CreateProposal { .. })
    }

    /// Check if this requires running a ceremony
    pub fn is_blocking(&self) -> bool {
        matches!(self.decision, EffectDecision::RunCeremony { .. })
    }
}

/// Extension trait for evaluating effect policies with full context
pub trait EffectPolicyExt {
    /// Evaluate effect policy and get full result with metadata
    fn evaluate_with_metadata(
        &self,
        operation: &OperationType,
        context_id: Option<&ContextId>,
    ) -> AuraResult<EffectPolicyResult>;
}

impl EffectPolicyExt for EffectPolicyGuard {
    fn evaluate_with_metadata(
        &self,
        operation: &OperationType,
        context_id: Option<&ContextId>,
    ) -> AuraResult<EffectPolicyResult> {
        let decision = self.evaluate(operation, context_id)?;
        let policy = self.get_policy(operation, context_id);

        Ok(EffectPolicyResult {
            operation: operation.clone(),
            context_id: context_id.cloned(),
            decision,
            policy,
        })
    }
}

/// Error type for effect policy violations
#[derive(Debug, Clone)]
pub enum EffectPolicyError {
    /// Operation requires approval but none was provided
    ApprovalRequired {
        operation: OperationType,
        context_id: Option<ContextId>,
    },
    /// Operation requires ceremony but was called directly
    CeremonyRequired {
        operation: OperationType,
        ceremony_type: String,
    },
    /// Policy lookup failed
    PolicyNotFound { operation: OperationType },
}

impl std::fmt::Display for EffectPolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EffectPolicyError::ApprovalRequired {
                operation,
                context_id,
            } => {
                write!(
                    f,
                    "Operation {} requires approval{}",
                    operation.as_str(),
                    context_id
                        .map(|c| format!(" in context {c}"))
                        .unwrap_or_else(String::new)
                )
            }
            EffectPolicyError::CeremonyRequired {
                operation,
                ceremony_type,
            } => {
                write!(
                    f,
                    "Operation {} requires {} ceremony",
                    operation.as_str(),
                    ceremony_type
                )
            }
            EffectPolicyError::PolicyNotFound { operation } => {
                write!(f, "No policy found for operation {}", operation.as_str())
            }
        }
    }
}

impl std::error::Error for EffectPolicyError {}

impl aura_core::ProtocolErrorCode for EffectPolicyError {
    fn code(&self) -> &'static str {
        match self {
            EffectPolicyError::ApprovalRequired { .. } => "approval_required",
            EffectPolicyError::CeremonyRequired { .. } => "ceremony_required",
            EffectPolicyError::PolicyNotFound { .. } => "policy_not_found",
        }
    }
}

impl From<EffectPolicyError> for AuraError {
    fn from(err: EffectPolicyError) -> Self {
        AuraError::permission_denied(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_authorization::EffectTiming;

    #[test]
    fn test_guard_defaults() {
        let guard = EffectPolicyGuard::default();

        // Low risk should be immediate
        assert!(guard.can_proceed_immediately(&OperationType::SendMessage, None));
        assert!(guard.can_proceed_immediately(&OperationType::CreateChannel, None));

        // Medium risk should require proposal
        assert!(guard.requires_proposal(&OperationType::RemoveChannelMember, None));

        // Critical should require ceremony
        assert!(guard.requires_ceremony(&OperationType::RotateGuardians, None));
    }

    #[test]
    fn test_guard_with_context_override() {
        let mut guard = EffectPolicyGuard::default();
        let context_id = ContextId::new_from_entropy([42u8; 32]);

        // Default: RemoveChannelMember requires proposal
        assert!(guard.requires_proposal(&OperationType::RemoveChannelMember, None));

        // Add override for this context
        guard.registry_mut().set_override(
            context_id,
            OperationType::RemoveChannelMember,
            EffectTiming::Immediate,
        );

        // With context: immediate
        assert!(
            guard.can_proceed_immediately(&OperationType::RemoveChannelMember, Some(&context_id))
        );

        // Without context: still requires proposal
        assert!(guard.requires_proposal(&OperationType::RemoveChannelMember, None));
    }

    #[test]
    fn test_evaluate_with_metadata() {
        let guard = EffectPolicyGuard::default();

        let result = match guard.evaluate_with_metadata(&OperationType::DeleteChannel, None) {
            Ok(result) => result,
            Err(err) => panic!("evaluate_with_metadata failed: {err}"),
        };

        assert_eq!(result.operation, OperationType::DeleteChannel);
        assert!(result.is_deferred());
        assert_eq!(
            result.policy.security_level,
            aura_authorization::SecurityLevel::High
        );
    }

    #[test]
    fn test_effect_policy_error_display() {
        let err = EffectPolicyError::ApprovalRequired {
            operation: OperationType::RemoveChannelMember,
            context_id: None,
        };
        assert!(err.to_string().contains("requires approval"));

        let err = EffectPolicyError::CeremonyRequired {
            operation: OperationType::RotateGuardians,
            ceremony_type: "guardian_rotation".to_string(),
        };
        assert!(err.to_string().contains("requires"));
        assert!(err.to_string().contains("ceremony"));
    }

    #[test]
    fn test_get_policy() {
        let guard = EffectPolicyGuard::default();

        let policy = guard.get_policy(&OperationType::TransferChannelOwnership, None);
        assert_eq!(policy.operation, OperationType::TransferChannelOwnership);
        assert_eq!(
            policy.security_level,
            aura_authorization::SecurityLevel::High
        );
    }
}

//! Capability evaluation following crisp communication rule set
//!
//! Implements the core evaluation function:
//! `Caps_effective := Policy ⊓ ⋂(Delegations) ⊓ ⋂(LocalChecks)`

use crate::{CapabilitySet, DelegationChain, Policy, WotError};
use aura_core::identifiers::DeviceId;
use chrono::Timelike;
use std::collections::HashMap;

/// Context for capability evaluation
#[derive(Debug, Clone)]
pub struct EvaluationContext {
    /// The device requesting access
    pub device_id: DeviceId,

    /// The operation being requested
    pub operation: String,

    /// Additional metadata for evaluation
    pub metadata: HashMap<String, String>,
}

impl EvaluationContext {
    /// Create a new evaluation context
    pub fn new(device_id: DeviceId, operation: String) -> Self {
        Self {
            device_id,
            operation,
            metadata: HashMap::new(),
        }
    }

    /// Add metadata to the context
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

/// Local checks that can restrict capabilities
#[derive(Debug, Clone)]
pub struct LocalChecks {
    /// Time-based restrictions
    pub time_restrictions: Option<TimeRestriction>,

    /// Rate limiting restrictions
    pub rate_limits: Vec<RateLimit>,

    /// Context-based restrictions
    pub context_restrictions: Vec<ContextRestriction>,
}

#[derive(Debug, Clone)]
pub struct TimeRestriction {
    pub allowed_hours: std::ops::Range<u8>, // e.g., 9..17 for business hours
}

#[derive(Debug, Clone)]
pub struct RateLimit {
    pub operation_pattern: String,
    pub max_per_hour: u32,
    pub current_count: u32,
}

#[derive(Debug, Clone)]
pub struct ContextRestriction {
    pub required_metadata_key: String,
    pub required_values: Vec<String>,
}

impl LocalChecks {
    /// Create empty local checks (no restrictions)
    pub fn empty() -> Self {
        Self {
            time_restrictions: None,
            rate_limits: Vec::new(),
            context_restrictions: Vec::new(),
        }
    }

    /// Check if local conditions permit the operation
    pub fn permits_operation(&self, context: &EvaluationContext) -> bool {
        // Check time restrictions
        if let Some(time_restriction) = &self.time_restrictions {
            let current_hour = chrono::Local::now().hour() as u8;
            if !time_restriction.allowed_hours.contains(&current_hour) {
                return false;
            }
        }

        // Check rate limits
        for rate_limit in &self.rate_limits {
            if context.operation.contains(&rate_limit.operation_pattern) {
                if rate_limit.current_count >= rate_limit.max_per_hour {
                    return false;
                }
            }
        }

        // Check context restrictions
        for context_restriction in &self.context_restrictions {
            if let Some(value) = context
                .metadata
                .get(&context_restriction.required_metadata_key)
            {
                if !context_restriction.required_values.contains(value) {
                    return false;
                }
            } else {
                return false; // Required metadata missing
            }
        }

        true
    }

    /// Compute capability restrictions from local checks
    ///
    /// Returns a capability set representing what local checks permit
    pub fn compute_capability_restrictions(&self, context: &EvaluationContext) -> CapabilitySet {
        if self.permits_operation(context) {
            // If local checks pass, return unrestricted capabilities
            CapabilitySet::all()
        } else {
            // If local checks fail, return empty capabilities
            CapabilitySet::empty()
        }
    }
}

/// Core capability evaluation function implementing crisp communication rules
///
/// This function implements the formula:
/// `Caps_effective := Policy ⊓ ⋂(Delegations) ⊓ ⋂(LocalChecks)`
///
/// Where:
/// - Policy: Base policy capabilities
/// - Delegations: Intersection of all delegation chains
/// - LocalChecks: Runtime restrictions (time, rate limits, context)
///
/// All operations use meet-semilattice intersection (⊓) to ensure
/// capabilities can only shrink, never grow.
pub fn evaluate_capabilities(
    policy: &Policy,
    delegations: &[DelegationChain],
    local_checks: &LocalChecks,
    context: &EvaluationContext,
) -> Result<CapabilitySet, WotError> {
    // 1. Start with policy capabilities for the requesting device
    let mut effective_caps = policy.capabilities_for_device(&context.device_id);

    // 2. Apply delegation chains: each chain can grant additional capabilities
    for delegation_chain in delegations {
        // Validate the delegation chain first
        delegation_chain.validate()?;

        // Check if this delegation chain applies to the requesting device
        if let Some(final_delegatee) = delegation_chain.final_delegatee() {
            if final_delegatee == context.device_id {
                // Find root capabilities from the root delegator
                if let Some(root_delegator) = delegation_chain.root_delegator() {
                    let root_caps = policy.capabilities_for_device(&root_delegator);
                    let chain_caps = delegation_chain.effective_capabilities(&root_caps);

                    // Use the delegation chain capabilities (attenuated from root)
                    effective_caps = chain_caps;
                }
            }
        }
    }

    // 3. Apply local check restrictions: ⋂(LocalChecks)
    let local_restrictions = local_checks.compute_capability_restrictions(context);
    effective_caps = effective_caps.meet(&local_restrictions);

    Ok(effective_caps)
}

/// Check if effective capabilities permit a specific operation
///
/// This is the main authorization function used throughout Aura.
/// It computes effective capabilities and checks if they permit the requested operation.
pub fn check_authorization(
    policy: &Policy,
    delegations: &[DelegationChain],
    local_checks: &LocalChecks,
    context: &EvaluationContext,
) -> Result<bool, WotError> {
    let effective_caps = evaluate_capabilities(policy, delegations, local_checks, context)?;

    Ok(effective_caps.permits(&context.operation))
}

/// Audit trail entry for capability evaluations
#[derive(Debug, Clone)]
pub struct CapabilityAuditEntry {
    pub device_id: DeviceId,
    pub operation: String,
    pub granted: bool,
    pub policy_caps: CapabilitySet,
    pub effective_caps: CapabilitySet,
    pub delegation_count: usize,
    pub local_check_result: bool,
    pub timestamp: std::time::SystemTime,
}

/// Evaluate capabilities with audit trail
pub fn evaluate_capabilities_with_audit(
    policy: &Policy,
    delegations: &[DelegationChain],
    local_checks: &LocalChecks,
    context: &EvaluationContext,
) -> Result<(CapabilitySet, CapabilityAuditEntry), WotError> {
    let policy_caps = policy.capabilities_for_device(&context.device_id);
    let local_check_result = local_checks.permits_operation(context);

    let effective_caps = evaluate_capabilities(policy, delegations, local_checks, context)?;
    let granted = effective_caps.permits(&context.operation);

    let audit_entry = CapabilityAuditEntry {
        device_id: context.device_id,
        operation: context.operation.clone(),
        granted,
        policy_caps,
        effective_caps: effective_caps.clone(),
        delegation_count: delegations.len(),
        local_check_result,
        timestamp: std::time::SystemTime::now(),
    };

    Ok((effective_caps, audit_entry))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CapabilitySet, DelegationLink};

    #[test]
    fn test_capability_evaluation_formula() {
        let device_a = DeviceId::new();
        let device_b = DeviceId::new();

        // Policy grants read+write
        let mut policy = Policy::new();
        policy.set_device_capabilities(
            device_a,
            CapabilitySet::from_permissions(&["read", "write"]),
        );

        // Delegation restricts to read-only
        let delegation = DelegationLink::new(
            device_a,
            device_b,
            CapabilitySet::from_permissions(&["read"]),
            1,
        );
        let delegation_chain = DelegationChain::from_delegation(delegation);

        // Local checks have no restrictions
        let local_checks = LocalChecks::empty();

        let context = EvaluationContext::new(device_b, "read:document".to_string());

        // Effective capabilities should be: Policy ⊓ Delegations ⊓ LocalChecks
        // = {read, write} ⊓ {read} ⊓ {all} = {read}
        let effective_caps =
            evaluate_capabilities(&policy, &[delegation_chain], &local_checks, &context).unwrap();

        assert!(effective_caps.permits("read:document"));
        assert!(!effective_caps.permits("write:document"));
    }

    #[test]
    fn test_local_checks_restriction() {
        let device_id = DeviceId::new();

        let mut policy = Policy::new();
        policy.set_device_capabilities(
            device_id,
            CapabilitySet::from_permissions(&["read", "write"]),
        );

        // Local checks restrict based on missing context
        let local_checks = LocalChecks {
            time_restrictions: None,
            rate_limits: Vec::new(),
            context_restrictions: vec![ContextRestriction {
                required_metadata_key: "source_verified".to_string(),
                required_values: vec!["true".to_string()],
            }],
        };

        // Context missing required metadata
        let context = EvaluationContext::new(device_id, "write:sensitive".to_string());

        // Should be denied due to local checks
        let authorized = check_authorization(&policy, &[], &local_checks, &context).unwrap();

        assert!(!authorized);

        // With required metadata, should be allowed
        let context_with_metadata =
            context.with_metadata("source_verified".to_string(), "true".to_string());

        let authorized_with_metadata =
            check_authorization(&policy, &[], &local_checks, &context_with_metadata).unwrap();

        assert!(authorized_with_metadata);
    }
}

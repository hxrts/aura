//! Access control decision logic

use crate::capability::CapabilityToken;
use crate::policy::{evaluate_policy, PolicyContext, PolicyEvaluation};
use crate::{Action, AuthorizationError, Resource, Result, Subject};
use aura_authentication::AuthenticationContext;
use aura_types::CapabilityId;
use std::time::SystemTime;

/// Final access control decision
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessDecision {
    /// Access is granted
    Allow {
        /// Basis for the decision
        basis: AccessBasis,
    },

    /// Access is denied
    Deny {
        /// Reason for denial
        reason: String,
    },

    /// Access requires additional verification
    RequiresVerification {
        /// What verification is needed
        verification_type: VerificationType,
    },
}

/// Basis for granting access
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessBasis {
    /// Direct authority from the authority graph
    DirectAuthority,

    /// Valid capability token
    Capability {
        capability_id: CapabilityId,
        delegation_chain_length: u16,
    },

    /// Threshold authorization
    ThresholdAuthorization {
        participating_devices: u16,
        threshold: u16,
    },

    /// Administrative override
    AdminOverride,
}

/// Type of additional verification required
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationType {
    /// Requires threshold signatures
    ThresholdSignature {
        required_signatures: u16,
        current_signatures: u16,
    },

    /// Requires specific capability
    SpecificCapability { required_capability: String },

    /// Requires guardian approval
    GuardianApproval { required_guardians: u16 },

    /// Requires time delay (for high-impact operations)
    TimeDelay { delay_hours: u32 },
}

/// Access control request
#[derive(Debug, Clone)]
pub struct AccessRequest {
    /// Subject requesting access
    pub subject: Subject,

    /// Action being requested
    pub action: Action,

    /// Resource being accessed
    pub resource: Resource,

    /// Capability tokens provided with the request
    pub capabilities: Vec<CapabilityToken>,

    /// Additional context
    pub context: std::collections::HashMap<String, String>,

    /// Request timestamp
    pub timestamp: SystemTime,
}

/// Make an access control decision
pub fn make_access_decision(
    request: &AccessRequest,
    auth_context: &AuthenticationContext,
    policy_context: &PolicyContext,
) -> Result<AccessDecision> {
    // First check if the subject is authenticated
    verify_subject_authentication(&request.subject, auth_context)?;

    // Validate any provided capability tokens
    let validated_capabilities = validate_capability_tokens(&request.capabilities, auth_context)?;

    // Create an updated policy context with validated capabilities
    let updated_policy_context = PolicyContext {
        current_time: request.timestamp,
        authority_graph: policy_context.authority_graph.clone(),
        capabilities: validated_capabilities,
        context_data: policy_context.context_data.clone(),
    };

    // Evaluate the authorization policy
    let policy_result = evaluate_policy(
        &request.subject,
        &request.action,
        &request.resource,
        &updated_policy_context,
    )?;

    // Convert policy evaluation to access decision
    convert_policy_to_decision(policy_result, request, &updated_policy_context)
}

/// Verify that the subject is properly authenticated
fn verify_subject_authentication(
    subject: &Subject,
    auth_context: &AuthenticationContext,
) -> Result<()> {
    match subject {
        Subject::Device(device_id) => {
            // Verify device is registered and has valid credentials
            auth_context
                .verify_device_authentication(device_id)
                .map_err(AuthorizationError::AuthenticationError)?;
        }

        Subject::Guardian(guardian_id) => {
            // Convert UUID to GuardianId for authentication
            let guardian_id = aura_types::GuardianId(*guardian_id);
            auth_context
                .verify_guardian_authentication(&guardian_id)
                .map_err(AuthorizationError::AuthenticationError)?;
        }

        Subject::Session { session_id, issuer } => {
            // Verify session ticket is valid and issued by the claimed device
            auth_context
                .verify_session_authentication(session_id, issuer)
                .map_err(AuthorizationError::AuthenticationError)?;
        }

        Subject::ThresholdGroup {
            participants,
            threshold,
        } => {
            // Verify all participants are authenticated
            for device_id in participants {
                auth_context
                    .verify_device_authentication(device_id)
                    .map_err(AuthorizationError::AuthenticationError)?;
            }

            // Verify threshold configuration is valid
            if (*threshold as usize) > participants.len() {
                return Err(AuthorizationError::InvalidDelegationChain(
                    "Threshold exceeds participant count".to_string(),
                ));
            }
        }
    }

    Ok(())
}

/// Validate capability tokens and their delegation chains
fn validate_capability_tokens(
    capabilities: &[CapabilityToken],
    auth_context: &AuthenticationContext,
) -> Result<Vec<CapabilityToken>> {
    let mut validated = Vec::new();

    for capability in capabilities {
        // Verify the capability signature (simplified)
        let capability_data = serde_json::to_vec(capability).map_err(|e| {
            AuthorizationError::SerializationError(format!("Failed to serialize capability: {}", e))
        })?;
        auth_context
            .verify_capability_signature(&capability_data)
            .map_err(AuthorizationError::AuthenticationError)?;

        // Check if capability is valid (includes expiration and condition checks)
        #[allow(clippy::disallowed_methods)]
        let current_timestamp = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        capability.is_valid(current_timestamp)?;

        validated.push(capability.clone());
    }

    Ok(validated)
}

/// Convert policy evaluation result to access decision
fn convert_policy_to_decision(
    policy_result: PolicyEvaluation,
    request: &AccessRequest,
    policy_context: &PolicyContext,
) -> Result<AccessDecision> {
    match policy_result {
        PolicyEvaluation::Allow => {
            // Determine the basis for allowing access
            let basis = determine_access_basis(request, policy_context)?;
            Ok(AccessDecision::Allow { basis })
        }

        PolicyEvaluation::Deny => Ok(AccessDecision::Deny {
            reason: "Access denied by authorization policy".to_string(),
        }),

        PolicyEvaluation::Inconclusive => {
            // Check if this action requires special verification
            if requires_special_verification(&request.action, &request.resource) {
                let verification_type =
                    determine_verification_type(&request.action, &request.resource)?;
                Ok(AccessDecision::RequiresVerification { verification_type })
            } else {
                Ok(AccessDecision::Deny {
                    reason: "No authorization policy grants access".to_string(),
                })
            }
        }

        PolicyEvaluation::Conditional { requirements } => {
            // Convert requirements to verification types
            let verification_type = convert_requirements_to_verification(&requirements)?;
            Ok(AccessDecision::RequiresVerification { verification_type })
        }
    }
}

/// Determine the basis for granting access
fn determine_access_basis(
    request: &AccessRequest,
    policy_context: &PolicyContext,
) -> Result<AccessBasis> {
    // Check if access is granted via capability
    for capability in &request.capabilities {
        if capability.grants_access_to(&request.resource)
            && capability.allows_action(&request.action)
        {
            let chain_length = capability.delegation_depth as u16;

            return Ok(AccessBasis::Capability {
                capability_id: capability.id,
                delegation_chain_length: chain_length,
            });
        }
    }

    // Check if access is granted via direct authority
    if policy_context
        .authority_graph
        .has_direct_authority(&request.subject, &request.resource)?
    {
        return Ok(AccessBasis::DirectAuthority);
    }

    // Default to threshold authorization
    Ok(AccessBasis::ThresholdAuthorization {
        participating_devices: 1,
        threshold: 1,
    })
}

/// Check if an action requires special verification
fn requires_special_verification(action: &Action, resource: &Resource) -> bool {
    match action {
        Action::Admin | Action::Delete => true,
        Action::Delegate => matches!(resource, Resource::CapabilityDelegation { .. }),
        _ => false,
    }
}

/// Determine the type of verification required
fn determine_verification_type(action: &Action, _resource: &Resource) -> Result<VerificationType> {
    match action {
        Action::Admin => Ok(VerificationType::ThresholdSignature {
            required_signatures: 2,
            current_signatures: 0,
        }),

        Action::Delete => Ok(VerificationType::TimeDelay { delay_hours: 24 }),

        Action::Delegate => Ok(VerificationType::SpecificCapability {
            required_capability: "delegation_authority".to_string(),
        }),

        _ => Ok(VerificationType::ThresholdSignature {
            required_signatures: 1,
            current_signatures: 0,
        }),
    }
}

/// Convert policy requirements to verification type
fn convert_requirements_to_verification(requirements: &[String]) -> Result<VerificationType> {
    for requirement in requirements {
        match requirement.as_str() {
            "explicit_high_impact_capability" => {
                return Ok(VerificationType::SpecificCapability {
                    required_capability: requirement.clone(),
                });
            }
            req if req.starts_with("threshold_") => {
                // Parse threshold requirement
                if let Some(count_str) = req.strip_prefix("threshold_") {
                    if let Ok(count) = count_str.parse::<u16>() {
                        return Ok(VerificationType::ThresholdSignature {
                            required_signatures: count,
                            current_signatures: 0,
                        });
                    }
                }
            }
            req if req.starts_with("guardian_") => {
                // Parse guardian requirement
                if let Some(count_str) = req.strip_prefix("guardian_") {
                    if let Ok(count) = count_str.parse::<u16>() {
                        return Ok(VerificationType::GuardianApproval {
                            required_guardians: count,
                        });
                    }
                }
            }
            _ => continue,
        }
    }

    // Default to threshold signature
    Ok(VerificationType::ThresholdSignature {
        required_signatures: 1,
        current_signatures: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_access_decision_conversion() {
        let policy_result = PolicyEvaluation::Allow;
        let request = AccessRequest {
            subject: Subject::Device(aura_types::DeviceId::new()),
            action: Action::Read,
            resource: Resource::Account(aura_types::AccountId::new()),
            capabilities: vec![],
            context: std::collections::HashMap::new(),
            timestamp: SystemTime::UNIX_EPOCH,
        };
        #[allow(clippy::disallowed_method)]
        let policy_context = PolicyContext {
            current_time: SystemTime::now(),
            authority_graph: crate::policy::AuthorityGraph::new(),
            capabilities: vec![],
            context_data: std::collections::HashMap::new(),
        };

        let decision =
            convert_policy_to_decision(policy_result, &request, &policy_context).unwrap();
        assert!(matches!(decision, AccessDecision::Allow { .. }));
    }

    #[test]
    fn test_requires_special_verification() {
        assert!(requires_special_verification(
            &Action::Admin,
            &Resource::Account(aura_types::AccountId::new())
        ));
        assert!(requires_special_verification(
            &Action::Delete,
            &Resource::Account(aura_types::AccountId::new())
        ));
        assert!(!requires_special_verification(
            &Action::Read,
            &Resource::Account(aura_types::AccountId::new())
        ));
    }
}

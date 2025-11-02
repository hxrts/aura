//! Policy evaluation logic

use crate::capability::CapabilityToken;
use crate::policy::AuthorityGraph;
use crate::{Action, Resource, Result, Subject};
use std::time::SystemTime;

/// Result of policy evaluation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyEvaluation {
    /// Access is explicitly allowed
    Allow,

    /// Access is explicitly denied
    Deny,

    /// Policy is inconclusive - further evaluation needed
    Inconclusive,

    /// Conditional access based on additional requirements
    Conditional { requirements: Vec<String> },
}

/// Policy evaluation context
#[derive(Debug, Clone)]
pub struct PolicyContext {
    /// Current time for expiration checks
    pub current_time: SystemTime,

    /// Authority graph for capability verification
    pub authority_graph: AuthorityGraph,

    /// Active capability tokens
    pub capabilities: Vec<CapabilityToken>,

    /// Additional context data
    pub context_data: std::collections::HashMap<String, String>,
}

/// Evaluate access policy for a subject performing an action on a resource
pub fn evaluate_policy(
    subject: &Subject,
    action: &Action,
    resource: &Resource,
    context: &PolicyContext,
) -> Result<PolicyEvaluation> {
    // Check for explicit denials first
    if is_explicitly_denied(subject, action, resource, context)? {
        return Ok(PolicyEvaluation::Deny);
    }

    // Check capability-based access
    if let Some(evaluation) = evaluate_capability_access(subject, action, resource, context)? {
        return Ok(evaluation);
    }

    // Check authority graph permissions
    if let Some(evaluation) = evaluate_authority_permissions(subject, action, resource, context)? {
        return Ok(evaluation);
    }

    // Default to inconclusive - let higher-level logic decide
    Ok(PolicyEvaluation::Inconclusive)
}

/// Check if access is explicitly denied by policy
fn is_explicitly_denied(
    subject: &Subject,
    action: &Action,
    resource: &Resource,
    _context: &PolicyContext,
) -> Result<bool> {
    // Example denial policies
    match (subject, action, resource) {
        // Sessions cannot perform admin actions
        (Subject::Session { .. }, Action::Admin, _) => Ok(true),

        // Devices cannot delete other devices
        (Subject::Device(device_id), Action::Delete, Resource::Device(target_device_id))
            if device_id != target_device_id =>
        {
            Ok(true)
        }

        // Default: no explicit denial
        _ => Ok(false),
    }
}

/// Evaluate access based on capability tokens
fn evaluate_capability_access(
    subject: &Subject,
    action: &Action,
    resource: &Resource,
    context: &PolicyContext,
) -> Result<Option<PolicyEvaluation>> {
    // Find relevant capabilities for this subject
    let relevant_caps: Vec<_> = context
        .capabilities
        .iter()
        .filter(|cap| capability_applies_to_subject(cap, subject))
        .collect();

    if relevant_caps.is_empty() {
        return Ok(None);
    }

    // Check if any capability grants the requested access
    for capability in relevant_caps {
        if capability_grants_access(capability, action, resource, context)? {
            // Verify capability is still valid
            if verify_capability_validity(capability, context)? {
                return Ok(Some(PolicyEvaluation::Allow));
            }
        }
    }

    Ok(None)
}

/// Evaluate access based on authority graph
fn evaluate_authority_permissions(
    subject: &Subject,
    action: &Action,
    resource: &Resource,
    context: &PolicyContext,
) -> Result<Option<PolicyEvaluation>> {
    // Check if subject has direct authority over resource
    if context
        .authority_graph
        .has_direct_authority(subject, resource)?
    {
        // Some actions require additional permissions even with authority
        match action {
            Action::Admin | Action::Delete => {
                // High-impact actions require explicit capability
                Ok(Some(PolicyEvaluation::Conditional {
                    requirements: vec!["explicit_high_impact_capability".to_string()],
                }))
            }
            _ => Ok(Some(PolicyEvaluation::Allow)),
        }
    } else {
        Ok(None)
    }
}

/// Check if a capability applies to the given subject
fn capability_applies_to_subject(capability: &CapabilityToken, subject: &Subject) -> bool {
    match (&capability.subject, subject) {
        (Subject::Device(cap_device), Subject::Device(subj_device)) => cap_device == subj_device,
        (Subject::Guardian(cap_guardian), Subject::Guardian(subj_guardian)) => {
            cap_guardian == subj_guardian
        }
        (
            Subject::Session {
                session_id: cap_session,
                ..
            },
            Subject::Session {
                session_id: subj_session,
                ..
            },
        ) => cap_session == subj_session,
        _ => false,
    }
}

/// Check if a capability grants access to perform an action on a resource
fn capability_grants_access(
    capability: &CapabilityToken,
    action: &Action,
    resource: &Resource,
    _context: &PolicyContext,
) -> Result<bool> {
    // Check if capability grants access to the resource
    if !capability.grants_access_to(resource) {
        return Ok(false);
    }

    // Check if capability allows the action
    Ok(capability.allows_action(action))
}

/// Verify that a capability is still valid (not expired or revoked)
fn verify_capability_validity(
    capability: &CapabilityToken,
    context: &PolicyContext,
) -> Result<bool> {
    let current_timestamp = context
        .current_time
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Use the capability's built-in validity check
    Ok(capability.is_valid(current_timestamp).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::CapabilityScope;
    use uuid::Uuid;

    #[test]
    fn test_explicit_denial() {
        let subject = Subject::Session {
            session_id: Uuid::new_v4(),
            issuer: aura_types::DeviceId::new(),
        };
        let action = Action::Admin;
        let resource = Resource::Account(aura_types::AccountId::new());
        #[allow(clippy::disallowed_method)]
        let context = PolicyContext {
            current_time: SystemTime::now(),
            authority_graph: AuthorityGraph::new(),
            capabilities: vec![],
            context_data: std::collections::HashMap::new(),
        };

        let result = evaluate_policy(&subject, &action, &resource, &context).unwrap();
        assert_eq!(result, PolicyEvaluation::Deny);
    }

    #[test]
    fn test_inconclusive_evaluation() {
        let subject = Subject::Device(aura_types::DeviceId::new());
        let action = Action::Read;
        let resource = Resource::Account(aura_types::AccountId::new());
        #[allow(clippy::disallowed_method)]
        let context = PolicyContext {
            current_time: SystemTime::now(),
            authority_graph: AuthorityGraph::new(),
            capabilities: vec![],
            context_data: std::collections::HashMap::new(),
        };

        let result = evaluate_policy(&subject, &action, &resource, &context).unwrap();
        assert_eq!(result, PolicyEvaluation::Inconclusive);
    }
}

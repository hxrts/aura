//! Event authorization logic

use crate::policy::{evaluate_policy, PolicyContext, PolicyEvaluation};
use crate::{Action, AuthorizationError, Resource, Result, Subject};
use aura_authentication::{verify_signature, AuthenticationContext};
use aura_types::{AccountId, DeviceId};

/// Result of event authorization
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventAuthorizationResult {
    /// Event is authorized
    Authorized,

    /// Event requires additional signatures
    RequiresThreshold {
        required_signatures: u16,
        current_signatures: u16,
        missing_devices: Vec<DeviceId>,
    },

    /// Event is denied
    Denied { reason: String },
}

/// Event that requires authorization
#[derive(Debug, Clone)]
pub struct AuthorizationEvent {
    /// Event type identifier
    pub event_type: String,

    /// Account the event affects
    pub account_id: AccountId,

    /// Device that initiated the event
    pub initiator: DeviceId,

    /// Signature on the event
    pub signature: Vec<u8>,

    /// Event payload for authorization checks
    pub payload: Vec<u8>,

    /// Additional context
    pub context: std::collections::HashMap<String, String>,
}

/// Authorize an event based on authentication and authorization policies
pub fn authorize_event(
    event: &AuthorizationEvent,
    auth_context: &AuthenticationContext,
    policy_context: &PolicyContext,
) -> Result<EventAuthorizationResult> {
    // First verify the event signature
    verify_event_signature(event, auth_context)?;

    // Determine the subject, action, and resource from the event
    let subject = Subject::Device(event.initiator);
    let (action, resource) = classify_event(event)?;

    // Evaluate authorization policy
    let policy_result = evaluate_policy(&subject, &action, &resource, policy_context)?;

    match policy_result {
        PolicyEvaluation::Allow => Ok(EventAuthorizationResult::Authorized),

        PolicyEvaluation::Deny => Ok(EventAuthorizationResult::Denied {
            reason: "Access denied by policy".to_string(),
        }),

        PolicyEvaluation::Inconclusive => {
            // Check if this requires threshold authorization
            if requires_threshold_authorization(event)? {
                evaluate_threshold_requirements(event, auth_context, policy_context)
            } else {
                Ok(EventAuthorizationResult::Denied {
                    reason: "No explicit authorization found".to_string(),
                })
            }
        }

        PolicyEvaluation::Conditional { requirements } => {
            // Check if conditional requirements are met
            if verify_conditional_requirements(event, &requirements, auth_context)? {
                Ok(EventAuthorizationResult::Authorized)
            } else {
                Ok(EventAuthorizationResult::Denied {
                    reason: format!("Conditional requirements not met: {:?}", requirements),
                })
            }
        }
    }
}

/// Verify the cryptographic signature on an event
fn verify_event_signature(
    event: &AuthorizationEvent,
    auth_context: &AuthenticationContext,
) -> Result<()> {
    // Get the public key for the initiator device
    let public_key = auth_context
        .get_device_public_key(&event.initiator)
        .map_err(|e| AuthorizationError::AuthenticationError(e))?;

    // Convert signature bytes to Ed25519Signature and verify
    let signature = aura_crypto::Ed25519Signature::from_slice(&event.signature).map_err(|e| {
        AuthorizationError::AuthenticationError(
            aura_authentication::AuthenticationError::CryptoError(format!(
                "Invalid signature: {}",
                e
            )),
        )
    })?;

    // Verify the signature over the event payload
    verify_signature(&public_key, &event.payload, &signature)
        .map_err(|e| AuthorizationError::AuthenticationError(e))?;

    Ok(())
}

/// Classify an event to determine the action and resource being accessed
fn classify_event(event: &AuthorizationEvent) -> Result<(Action, Resource)> {
    let action = match event.event_type.as_str() {
        "key_derivation" => Action::Execute,
        "capability_delegation" => Action::Delegate,
        "capability_revocation" => Action::Revoke,
        "account_update" => Action::Write,
        "device_add" => Action::Admin,
        "device_remove" => Action::Admin,
        "storage_write" => Action::Write,
        "storage_read" => Action::Read,
        "storage_delete" => Action::Delete,
        _ => Action::Custom(event.event_type.clone()),
    };

    let resource = Resource::Account(event.account_id);

    Ok((action, resource))
}

/// Check if an event requires threshold authorization
fn requires_threshold_authorization(event: &AuthorizationEvent) -> Result<bool> {
    // High-impact events require threshold authorization
    let requires_threshold = matches!(
        event.event_type.as_str(),
        "device_add"
            | "device_remove"
            | "account_recovery"
            | "key_rotation"
            | "guardian_change"
            | "threshold_change"
    );

    Ok(requires_threshold)
}

/// Evaluate threshold authorization requirements
fn evaluate_threshold_requirements(
    event: &AuthorizationEvent,
    auth_context: &AuthenticationContext,
    _policy_context: &PolicyContext,
) -> Result<EventAuthorizationResult> {
    // Get threshold configuration for this account
    let threshold_config = auth_context
        .get_threshold_config(&event.account_id)
        .map_err(|e| AuthorizationError::AuthenticationError(e))?;

    // Count valid signatures on this event
    let signature_count = count_valid_signatures(event, auth_context)?;

    if signature_count >= threshold_config.threshold {
        Ok(EventAuthorizationResult::Authorized)
    } else {
        // Determine which devices still need to sign
        let all_devices = &threshold_config.participants;
        let signed_devices = get_signing_devices(event, auth_context)?;
        let missing_devices: Vec<_> = all_devices
            .iter()
            .filter(|device| !signed_devices.contains(device))
            .cloned()
            .collect();

        Ok(EventAuthorizationResult::RequiresThreshold {
            required_signatures: threshold_config.threshold,
            current_signatures: signature_count,
            missing_devices,
        })
    }
}

/// Count the number of valid signatures on an event
fn count_valid_signatures(
    event: &AuthorizationEvent,
    auth_context: &AuthenticationContext,
) -> Result<u16> {
    // For simplicity, assume single signature for now
    // In a full implementation, this would parse multiple signatures
    // from the event and validate each one

    let _public_key = auth_context
        .get_device_public_key(&event.initiator)
        .map_err(|e| AuthorizationError::AuthenticationError(e))?;

    // Simplified: just return 1 if signature is valid
    Ok(1)
}

/// Get the list of devices that have signed this event
fn get_signing_devices(
    event: &AuthorizationEvent,
    _auth_context: &AuthenticationContext,
) -> Result<Vec<DeviceId>> {
    // Simplified: just return the initiator
    // In a full implementation, this would extract all signing devices
    Ok(vec![event.initiator])
}

/// Verify conditional requirements are met
fn verify_conditional_requirements(
    _event: &AuthorizationEvent,
    requirements: &[String],
    _auth_context: &AuthenticationContext,
) -> Result<bool> {
    // Simplified verification
    for requirement in requirements {
        match requirement.as_str() {
            "explicit_high_impact_capability" => {
                // Would check for specific capability tokens
                return Ok(false); // Default to not having the capability
            }
            _ => {
                // Unknown requirement - fail safe
                return Ok(false);
            }
        }
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_classify_event() {
        let event = AuthorizationEvent {
            event_type: "key_derivation".to_string(),
            account_id: AccountId::new(),
            initiator: DeviceId::new(),
            signature: vec![],
            payload: vec![],
            context: std::collections::HashMap::new(),
        };

        let (action, _resource) = classify_event(&event).unwrap();
        assert!(matches!(action, Action::Execute));
    }

    #[test]
    fn test_requires_threshold_authorization() {
        let event = AuthorizationEvent {
            event_type: "device_add".to_string(),
            account_id: AccountId::new(),
            initiator: DeviceId::new(),
            signature: vec![],
            payload: vec![],
            context: std::collections::HashMap::new(),
        };

        assert!(requires_threshold_authorization(&event).unwrap());

        let normal_event = AuthorizationEvent {
            event_type: "storage_read".to_string(),
            account_id: AccountId::new(),
            initiator: DeviceId::new(),
            signature: vec![],
            payload: vec![],
            context: std::collections::HashMap::new(),
        };

        assert!(!requires_threshold_authorization(&normal_event).unwrap());
    }
}

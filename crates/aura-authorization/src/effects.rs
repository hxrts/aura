//! Effect-based authorization for choreographic protocols
//!
//! This module provides algebraic effect integration for authorization operations,
//! enabling choreographic protocols to perform authorization checks as effects.

use crate::{
    Action, AuthorizationError, Resource, Result, Subject,
    capability::token::{CapabilityToken, CapabilityCondition},
    capability::delegation::CapabilityDelegation,
    decisions::access_control::AccessDecision,
};
use aura_types::DeviceId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Authorization effect algebra for choreographic protocols
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthorizationEffect<R, M> {
    /// Check if a subject can perform an action on a resource
    CheckAccess {
        subject: Subject,
        action: Action,
        resource: Resource,
        continuation: fn(AccessDecision) -> M,
    },
    
    /// Create a new capability token
    CreateCapability {
        subject: Subject,
        resource: Resource,
        actions: Vec<Action>,
        issuer: DeviceId,
        delegatable: bool,
        continuation: fn(Result<CapabilityToken>) -> M,
    },
    
    /// Validate a capability token
    ValidateCapability {
        token: CapabilityToken,
        action: Action,
        continuation: fn(Result<()>) -> M,
    },
    
    /// Delegate a capability
    DelegateCapability {
        parent_token: CapabilityToken,
        delegator: Subject,
        delegatee: Subject,
        restrictions: Vec<crate::capability::delegation::DelegationRestriction>,
        continuation: fn(Result<CapabilityDelegation>) -> M,
    },
    
    /// Verify a delegation chain
    VerifyDelegationChain {
        delegation: CapabilityDelegation,
        continuation: fn(Result<()>) -> M,
    },
    
    /// Add a condition to a capability
    AddCondition {
        token: CapabilityToken,
        condition: CapabilityCondition,
        continuation: fn(Result<CapabilityToken>) -> M,
    },
    
    /// Evaluate capability conditions
    EvaluateConditions {
        token: CapabilityToken,
        context: HashMap<String, serde_json::Value>,
        continuation: fn(Result<()>) -> M,
    },
    
    _Phantom(std::marker::PhantomData<R>),
}

/// Effect handler for authorization operations
pub struct AuthorizationEffectHandler {
    /// Access control decision cache
    decision_cache: Arc<RwLock<HashMap<(Subject, Action, Resource), AccessDecision>>>,
    
    /// Capability token store
    capability_store: Arc<RwLock<HashMap<uuid::Uuid, CapabilityToken>>>,
    
    /// Delegation chain store
    delegation_store: Arc<RwLock<HashMap<uuid::Uuid, CapabilityDelegation>>>,
}

impl AuthorizationEffectHandler {
    /// Create a new authorization effect handler
    pub fn new() -> Self {
        Self {
            decision_cache: Arc::new(RwLock::new(HashMap::new())),
            capability_store: Arc::new(RwLock::new(HashMap::new())),
            delegation_store: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Handle an authorization effect
    pub async fn handle_effect<R, M>(&self, effect: AuthorizationEffect<R, M>) -> M
    where
        R: Send + Sync,
        M: Send + Sync,
    {
        match effect {
            AuthorizationEffect::CheckAccess { subject, action, resource, continuation } => {
                let decision = self.check_access_internal(subject, action, resource).await;
                continuation(decision)
            }
            
            AuthorizationEffect::CreateCapability { 
                subject, resource, actions, issuer, delegatable, continuation 
            } => {
                let result = self.create_capability_internal(
                    subject, resource, actions, issuer, delegatable
                ).await;
                continuation(result)
            }
            
            AuthorizationEffect::ValidateCapability { token, action, continuation } => {
                let result = self.validate_capability_internal(token, action).await;
                continuation(result)
            }
            
            AuthorizationEffect::DelegateCapability {
                parent_token, delegator, delegatee, restrictions, continuation
            } => {
                let result = self.delegate_capability_internal(
                    parent_token, delegator, delegatee, restrictions
                ).await;
                continuation(result)
            }
            
            AuthorizationEffect::VerifyDelegationChain { delegation, continuation } => {
                let result = self.verify_delegation_chain_internal(delegation).await;
                continuation(result)
            }
            
            AuthorizationEffect::AddCondition { mut token, condition, continuation } => {
                token.add_condition(condition);
                continuation(Ok(token))
            }
            
            AuthorizationEffect::EvaluateConditions { token, context, continuation } => {
                let result = self.evaluate_conditions_internal(token, context).await;
                continuation(result)
            }
            
            AuthorizationEffect::_Phantom(_) => unreachable!(),
        }
    }
    
    // Internal implementation methods
    
    async fn check_access_internal(
        &self,
        subject: Subject,
        action: Action,
        resource: Resource,
    ) -> AccessDecision {
        // Check cache first
        let cache_key = (subject.clone(), action.clone(), resource.clone());
        let cache = self.decision_cache.read().await;
        if let Some(decision) = cache.get(&cache_key) {
            return decision.clone();
        }
        drop(cache);
        
        // Check capability store
        let capabilities = self.capability_store.read().await;
        let has_capability = capabilities.values().any(|token| {
            token.subject == subject
                && token.resource == resource
                && token.actions.contains(&action)
                && token.is_valid(current_timestamp()).is_ok()
        });
        
        let decision = if has_capability {
            AccessDecision::Allow
        } else {
            AccessDecision::Deny {
                reason: "No valid capability found".to_string(),
            }
        };
        
        // Cache the decision
        let mut cache = self.decision_cache.write().await;
        cache.insert(cache_key, decision.clone());
        
        decision
    }
    
    async fn create_capability_internal(
        &self,
        subject: Subject,
        resource: Resource,
        actions: Vec<Action>,
        issuer: DeviceId,
        delegatable: bool,
    ) -> Result<CapabilityToken> {
        let effects = aura_crypto::Effects::production();
        let mut token = CapabilityToken::new(
            subject,
            resource,
            actions,
            issuer,
            delegatable,
            3, // Default delegation depth
            &effects,
        );
        
        // In production, this would sign the token properly
        // For now, we'll use a placeholder signature
        token.issuer_signature = aura_crypto::Ed25519Signature::default();
        
        // Store the token
        let mut store = self.capability_store.write().await;
        store.insert(token.id, token.clone());
        
        Ok(token)
    }
    
    async fn validate_capability_internal(
        &self,
        token: CapabilityToken,
        action: Action,
    ) -> Result<()> {
        // Check if token is valid
        token.is_valid(current_timestamp())?;
        
        // Check if token includes the requested action
        if !token.actions.contains(&action) {
            return Err(AuthorizationError::InsufficientPermissions(format!(
                "Token does not include action {:?}",
                action
            )));
        }
        
        // Verify signature (placeholder for now)
        // In production: verify token.issuer_signature
        
        Ok(())
    }
    
    async fn delegate_capability_internal(
        &self,
        parent_token: CapabilityToken,
        delegator: Subject,
        delegatee: Subject,
        restrictions: Vec<crate::capability::delegation::DelegationRestriction>,
    ) -> Result<CapabilityDelegation> {
        // Verify delegator has the capability
        if parent_token.subject != delegator {
            return Err(AuthorizationError::InsufficientPermissions(
                "Delegator does not own the capability".to_string()
            ));
        }
        
        // Create delegation
        let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(
            &aura_crypto::Effects::production().random_bytes()
        );
        
        let effects = aura_crypto::Effects::production();
        let delegation = crate::capability::delegation::delegate_capability(
            &parent_token,
            delegator,
            delegatee,
            restrictions,
            &signing_key,
            &effects,
        )?;
        
        // Store delegation
        let mut store = self.delegation_store.write().await;
        store.insert(delegation.delegation_id, delegation.clone());
        
        Ok(delegation)
    }
    
    async fn verify_delegation_chain_internal(
        &self,
        delegation: CapabilityDelegation,
    ) -> Result<()> {
        // Get parent capability
        let capabilities = self.capability_store.read().await;
        let parent = capabilities.get(&delegation.parent_capability_id)
            .ok_or_else(|| AuthorizationError::InvalidDelegationChain(
                "Parent capability not found".to_string()
            ))?;
        
        // Verify delegation (placeholder verification key)
        let verifying_key = aura_crypto::Ed25519VerifyingKey::from_bytes(&[0u8; 32])
            .map_err(|e| AuthorizationError::CryptographicError(e.to_string()))?;
        
        crate::capability::delegation::verify_delegation(
            &delegation,
            parent,
            &verifying_key,
        )
    }
    
    async fn evaluate_conditions_internal(
        &self,
        token: CapabilityToken,
        _context: HashMap<String, serde_json::Value>,
    ) -> Result<()> {
        let current_time = current_timestamp();
        
        for condition in &token.conditions {
            match condition {
                CapabilityCondition::TimeWindow { start, end } => {
                    if current_time < *start || current_time > *end {
                        return Err(AuthorizationError::ConditionNotMet(
                            "Outside time window".to_string()
                        ));
                    }
                }
                
                CapabilityCondition::UsageLimit { max_uses, current_uses } => {
                    if current_uses >= max_uses {
                        return Err(AuthorizationError::UsageLimitExceeded(
                            "Usage limit exceeded".to_string()
                        ));
                    }
                }
                
                // Other conditions would be evaluated here
                _ => {}
            }
        }
        
        Ok(())
    }
}

/// Get current timestamp for production use
fn current_timestamp() -> u64 {
    aura_crypto::current_timestamp_with_effects(&aura_crypto::Effects::production())
        .unwrap_or(0)
}

/// Get current timestamp using effects
fn current_timestamp_with_effects(effects: &aura_crypto::Effects) -> u64 {
    aura_crypto::current_timestamp_with_effects(effects).unwrap_or(0)
}

/// Extension trait for choreographic authorization
#[cfg(feature = "choreographic")]
pub trait AuthorizationChoreographicExt {
    /// Convert an access check into a choreographic effect
    fn check_access_effect<R, M>(
        subject: Subject,
        action: Action,
        resource: Resource,
    ) -> AuthorizationEffect<R, M>
    where
        M: From<AccessDecision>;
        
    /// Convert capability creation into a choreographic effect
    fn create_capability_effect<R, M>(
        subject: Subject,
        resource: Resource,
        actions: Vec<Action>,
        issuer: DeviceId,
    ) -> AuthorizationEffect<R, M>
    where
        M: From<Result<CapabilityToken>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_types::DeviceIdExt;
    
    #[tokio::test]
    async fn test_authorization_effects() {
        let handler = AuthorizationEffectHandler::new();
        let effects = aura_crypto::Effects::test();
        
        let device_id = DeviceId::new_with_effects(&effects);
        let subject = Subject::Device(device_id);
        let resource = Resource::Account(aura_types::AccountId::new_with_effects(&effects));
        let action = Action::Read;
        
        // Create capability
        let create_effect = AuthorizationEffect::CreateCapability {
            subject: subject.clone(),
            resource: resource.clone(),
            actions: vec![action.clone()],
            issuer: device_id,
            delegatable: true,
            continuation: |result| result,
        };
        
        let token = handler.handle_effect(create_effect).await.unwrap();
        
        // Check access
        let check_effect = AuthorizationEffect::CheckAccess {
            subject,
            action,
            resource,
            continuation: |decision| decision,
        };
        
        let decision = handler.handle_effect(check_effect).await;
        assert!(matches!(decision, AccessDecision::Allow));
    }
}
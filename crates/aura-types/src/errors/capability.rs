//! Capability and authorization errors

use super::{ErrorCode, ErrorContext, ErrorSeverity};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Capability and authorization errors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CapabilityError {
    /// Authorization failed
    AuthorizationFailed {
        reason: String,
        action: Option<String>,
        resource: Option<String>,
        context: ErrorContext,
    },
    
    /// Authentication failed
    AuthenticationFailed {
        reason: String,
        method: Option<String>,
        subject: Option<String>,
        context: ErrorContext,
    },
    
    /// Access denied
    AccessDenied {
        resource: String,
        action: String,
        subject: Option<String>,
        context: ErrorContext,
    },
    
    /// Invalid capability
    InvalidCapability {
        reason: String,
        capability_id: Option<String>,
        context: ErrorContext,
    },
    
    /// Capability expired
    CapabilityExpired {
        capability_id: String,
        expired_at: Option<u64>,
        context: ErrorContext,
    },
    
    /// Delegation failed
    DelegationFailed {
        reason: String,
        from: Option<String>,
        to: Option<String>,
        context: ErrorContext,
    },
    
    /// Invalid delegation chain
    InvalidDelegationChain {
        reason: String,
        depth: Option<u8>,
        context: ErrorContext,
    },
    
    /// Policy evaluation failed
    PolicyEvaluationFailed {
        reason: String,
        policy_id: Option<String>,
        context: ErrorContext,
    },
    
    /// Insufficient permissions
    InsufficientPermissions {
        required: String,
        available: Option<String>,
        context: ErrorContext,
    },
    
    /// Trust evaluation failed
    TrustEvaluationFailed {
        reason: String,
        subject: Option<String>,
        trust_score: Option<f64>,
        context: ErrorContext,
    },
    
    /// Invalid subject
    InvalidSubject {
        reason: String,
        subject_type: Option<String>,
        context: ErrorContext,
    },
    
    /// Invalid resource
    InvalidResource {
        reason: String,
        resource_type: Option<String>,
        context: ErrorContext,
    },
    
    /// Invalid scope
    InvalidScope {
        reason: String,
        scope: Option<String>,
        context: ErrorContext,
    },
    
    /// Quota exceeded
    QuotaExceeded {
        resource: String,
        limit: u64,
        current: u64,
        context: ErrorContext,
    },
    
    /// Rate limit exceeded
    RateLimitExceeded {
        operation: String,
        limit: u64,
        window_secs: u64,
        context: ErrorContext,
    },
    
    /// Condition evaluation failed
    ConditionFailed {
        condition_type: String,
        reason: String,
        context: ErrorContext,
    },
    
    /// Authority not recognized
    UnrecognizedAuthority {
        authority: String,
        reason: Option<String>,
        context: ErrorContext,
    },
}

impl fmt::Display for CapabilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthorizationFailed { reason, action, resource, .. } => {
                write!(f, "Authorization failed: {}", reason)?;
                if let (Some(act), Some(res)) = (action, resource) {
                    write!(f, " (action: {} on {})", act, res)?;
                }
                Ok(())
            }
            Self::AuthenticationFailed { reason, method, subject, .. } => {
                write!(f, "Authentication failed: {}", reason)?;
                if let Some(m) = method {
                    write!(f, " (method: {})", m)?;
                }
                if let Some(s) = subject {
                    write!(f, " (subject: {})", s)?;
                }
                Ok(())
            }
            Self::AccessDenied { resource, action, subject, .. } => {
                write!(f, "Access denied for action '{}' on resource '{}'", action, resource)?;
                if let Some(s) = subject {
                    write!(f, " (subject: {})", s)?;
                }
                Ok(())
            }
            Self::InvalidCapability { reason, capability_id, .. } => {
                write!(f, "Invalid capability: {}", reason)?;
                if let Some(id) = capability_id {
                    write!(f, " (id: {})", id)?;
                }
                Ok(())
            }
            Self::CapabilityExpired { capability_id, expired_at, .. } => {
                write!(f, "Capability {} has expired", capability_id)?;
                if let Some(exp) = expired_at {
                    write!(f, " (at: {})", exp)?;
                }
                Ok(())
            }
            Self::DelegationFailed { reason, from, to, .. } => {
                write!(f, "Delegation failed: {}", reason)?;
                if let (Some(from_subject), Some(to_subject)) = (from, to) {
                    write!(f, " (from: {} to: {})", from_subject, to_subject)?;
                }
                Ok(())
            }
            Self::InvalidDelegationChain { reason, depth, .. } => {
                write!(f, "Invalid delegation chain: {}", reason)?;
                if let Some(d) = depth {
                    write!(f, " (depth: {})", d)?;
                }
                Ok(())
            }
            Self::PolicyEvaluationFailed { reason, policy_id, .. } => {
                write!(f, "Policy evaluation failed: {}", reason)?;
                if let Some(id) = policy_id {
                    write!(f, " (policy: {})", id)?;
                }
                Ok(())
            }
            Self::InsufficientPermissions { required, available, .. } => {
                write!(f, "Insufficient permissions: {} required", required)?;
                if let Some(avail) = available {
                    write!(f, " (available: {})", avail)?;
                }
                Ok(())
            }
            Self::TrustEvaluationFailed { reason, subject, trust_score, .. } => {
                write!(f, "Trust evaluation failed: {}", reason)?;
                if let Some(s) = subject {
                    write!(f, " (subject: {})", s)?;
                }
                if let Some(score) = trust_score {
                    write!(f, " (score: {:.2})", score)?;
                }
                Ok(())
            }
            Self::InvalidSubject { reason, subject_type, .. } => {
                write!(f, "Invalid subject: {}", reason)?;
                if let Some(st) = subject_type {
                    write!(f, " (type: {})", st)?;
                }
                Ok(())
            }
            Self::InvalidResource { reason, resource_type, .. } => {
                write!(f, "Invalid resource: {}", reason)?;
                if let Some(rt) = resource_type {
                    write!(f, " (type: {})", rt)?;
                }
                Ok(())
            }
            Self::InvalidScope { reason, scope, .. } => {
                write!(f, "Invalid scope: {}", reason)?;
                if let Some(s) = scope {
                    write!(f, " (scope: {})", s)?;
                }
                Ok(())
            }
            Self::QuotaExceeded { resource, limit, current, .. } => {
                write!(f, "Quota exceeded for {}: {} of {} used", resource, current, limit)
            }
            Self::RateLimitExceeded { operation, limit, window_secs, .. } => {
                write!(f, "Rate limit exceeded for {}: {} per {}s", operation, limit, window_secs)
            }
            Self::ConditionFailed { condition_type, reason, .. } => {
                write!(f, "Condition {} failed: {}", condition_type, reason)
            }
            Self::UnrecognizedAuthority { authority, reason, .. } => {
                write!(f, "Unrecognized authority: {}", authority)?;
                if let Some(r) = reason {
                    write!(f, " ({})", r)?;
                }
                Ok(())
            }
        }
    }
}

impl CapabilityError {
    /// Get the error code for this error
    pub fn error_code(&self) -> ErrorCode {
        match self {
            Self::AuthorizationFailed { .. } => ErrorCode::AuthorizationFailed,
            Self::AuthenticationFailed { .. } => ErrorCode::AuthenticationFailed,
            Self::AccessDenied { .. } => ErrorCode::AccessDenied,
            Self::InvalidCapability { .. } => ErrorCode::InvalidCapability,
            Self::CapabilityExpired { .. } => ErrorCode::CapabilityExpired,
            Self::DelegationFailed { .. } => ErrorCode::DelegationFailed,
            Self::InvalidDelegationChain { .. } => ErrorCode::InvalidDelegationChain,
            Self::PolicyEvaluationFailed { .. } => ErrorCode::PolicyEvaluationFailed,
            Self::InsufficientPermissions { .. } => ErrorCode::InsufficientPermissions,
            Self::TrustEvaluationFailed { .. } => ErrorCode::TrustEvaluationFailed,
            Self::InvalidSubject { .. } => ErrorCode::InvalidSubject,
            Self::InvalidResource { .. } => ErrorCode::InvalidResource,
            Self::InvalidScope { .. } => ErrorCode::InvalidScope,
            Self::QuotaExceeded { .. } => ErrorCode::QuotaExceeded,
            Self::RateLimitExceeded { .. } => ErrorCode::RateLimitExceeded,
            Self::ConditionFailed { .. } => ErrorCode::ConditionFailed,
            Self::UnrecognizedAuthority { .. } => ErrorCode::UnrecognizedAuthority,
        }
    }

    /// Get the severity of this error
    pub fn severity(&self) -> ErrorSeverity {
        match self {
            Self::AuthorizationFailed { .. } => ErrorSeverity::High,
            Self::AuthenticationFailed { .. } => ErrorSeverity::High,
            Self::AccessDenied { .. } => ErrorSeverity::Medium,
            Self::InvalidCapability { .. } => ErrorSeverity::Medium,
            Self::CapabilityExpired { .. } => ErrorSeverity::Low,
            Self::DelegationFailed { .. } => ErrorSeverity::Medium,
            Self::InvalidDelegationChain { .. } => ErrorSeverity::Medium,
            Self::PolicyEvaluationFailed { .. } => ErrorSeverity::Medium,
            Self::InsufficientPermissions { .. } => ErrorSeverity::Medium,
            Self::TrustEvaluationFailed { .. } => ErrorSeverity::Medium,
            Self::InvalidSubject { .. } => ErrorSeverity::Medium,
            Self::InvalidResource { .. } => ErrorSeverity::Medium,
            Self::InvalidScope { .. } => ErrorSeverity::Medium,
            Self::QuotaExceeded { .. } => ErrorSeverity::Low,
            Self::RateLimitExceeded { .. } => ErrorSeverity::Low,
            Self::ConditionFailed { .. } => ErrorSeverity::Medium,
            Self::UnrecognizedAuthority { .. } => ErrorSeverity::High,
        }
    }
}
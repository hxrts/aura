//! Authentication and authorization middleware

use super::{MiddlewareContext, AuraMiddleware};
use crate::effects::Effects;
use aura_types::permissions::CanonicalPermission;
use std::future::Future;
use std::pin::Pin;
use std::collections::HashMap;
use std::marker::PhantomData;

/// Authentication middleware for verifying identity
pub struct AuthMiddleware<Req, Resp, Err>
where
    Req: Send + Sync,
    Resp: Send + Sync,
    Err: std::error::Error + Send + Sync + 'static,
{
    /// Authentication policy
    policy: AuthPolicy,
    
    /// Permission checker
    permission_checker: Box<dyn PermissionChecker<Err>>,
    
    /// Token validator
    token_validator: Box<dyn TokenValidator<Err>>,
    
    /// Phantom data to use type parameters
    _phantom: PhantomData<(Req, Resp)>,
}

impl<Req, Resp, Err> AuthMiddleware<Req, Resp, Err>
where
    Req: Send + Sync,
    Resp: Send + Sync,
    Err: std::error::Error + Send + Sync + 'static,
{
    /// Create new authentication middleware
    pub fn new(
        policy: AuthPolicy,
        permission_checker: Box<dyn PermissionChecker<Err>>,
        token_validator: Box<dyn TokenValidator<Err>>,
    ) -> Self {
        Self {
            policy,
            permission_checker,
            token_validator,
            _phantom: PhantomData,
        }
    }
}

impl<Req, Resp, Err> AuraMiddleware for AuthMiddleware<Req, Resp, Err>
where
    Req: Send + Sync + AuthenticatedRequest + 'static,
    Resp: Send + Sync + 'static,
    Err: std::error::Error + Send + Sync + From<AuthError> + 'static,
{
    type Request = Req;
    type Response = Resp;
    type Error = Err;

    fn process<'a>(
        &'a self,
        request: Self::Request,
        context: &'a MiddlewareContext,
        effects: &'a dyn Effects,
        next: Box<dyn super::traits::MiddlewareHandler<Self::Request, Self::Response, Self::Error>>,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'a>> {
        let policy = self.policy.clone();
        let context = context.clone();

        Box::pin(async move {
            // Extract authentication token from request
            let token = request.auth_token().ok_or(AuthError::MissingToken)?;
            
            // Validate token
            let auth_context = self.token_validator.validate_token(&token, &context, effects).await?;
            
            // Check permissions
            let required_permissions = request.required_permissions();
            self.permission_checker.check_permissions(
                &auth_context,
                &required_permissions,
                &context,
                effects
            ).await?;
            
            // Check policy compliance
            policy.enforce(&auth_context, &context, effects).await?;
            
            // Create authenticated context
            let mut auth_ctx = context;
            auth_ctx.metadata.insert("user_id".to_string(), auth_context.user_id.clone());
            auth_ctx.metadata.insert("device_id".to_string(), auth_context.device_id.clone());
            
            // Continue with authenticated request
            next.handle(request, &auth_ctx, effects).await
        })
    }
}

/// Authentication policy configuration
#[derive(Debug, Clone)]
pub struct AuthPolicy {
    /// Whether authentication is required
    pub require_auth: bool,
    
    /// Allowed authentication methods
    pub allowed_auth_methods: Vec<AuthMethod>,
    
    /// Token expiration tolerance
    pub token_expiration_tolerance: std::time::Duration,
    
    /// Maximum session duration
    pub max_session_duration: std::time::Duration,
    
    /// Whether to allow concurrent sessions
    pub allow_concurrent_sessions: bool,
    
    /// Rate limiting configuration
    pub rate_limits: RateLimits,
    
    /// IP allowlist (if any)
    pub ip_allowlist: Option<Vec<std::net::IpAddr>>,
    
    /// Custom policy rules
    pub custom_rules: Vec<PolicyRule>,
}

impl Default for AuthPolicy {
    fn default() -> Self {
        Self {
            require_auth: true,
            allowed_auth_methods: vec![AuthMethod::SessionToken, AuthMethod::DeviceKey],
            token_expiration_tolerance: std::time::Duration::from_secs(300), // 5 minutes
            max_session_duration: std::time::Duration::from_secs(3600 * 24), // 24 hours
            allow_concurrent_sessions: true,
            rate_limits: RateLimits::default(),
            ip_allowlist: None,
            custom_rules: Vec::new(),
        }
    }
}

impl AuthPolicy {
    /// Enforce the authentication policy
    pub async fn enforce(
        &self,
        auth_context: &AuthContext,
        middleware_context: &MiddlewareContext,
        effects: &dyn Effects,
    ) -> Result<(), AuthError> {
        // Check session duration
        if auth_context.session_age() > self.max_session_duration {
            return Err(AuthError::SessionExpired);
        }
        
        // Check authentication method
        if !self.allowed_auth_methods.contains(&auth_context.auth_method) {
            return Err(AuthError::InvalidAuthMethod);
        }
        
        // Check IP allowlist
        if let Some(allowlist) = &self.ip_allowlist {
            if let Some(client_ip) = auth_context.client_ip {
                if !allowlist.contains(&client_ip) {
                    return Err(AuthError::IpNotAllowed);
                }
            }
        }
        
        // Apply rate limits
        self.rate_limits.check_limits(auth_context, middleware_context, effects).await?;
        
        // Apply custom rules
        for rule in &self.custom_rules {
            rule.apply(auth_context, middleware_context, effects).await?;
        }
        
        Ok(())
    }
}

/// Authentication methods
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthMethod {
    /// Session-based token authentication
    SessionToken,
    
    /// Device key authentication
    DeviceKey,
    
    /// Threshold signature authentication
    ThresholdSignature,
    
    /// Biometric authentication
    Biometric,
    
    /// Multi-factor authentication
    MultiFactorAuth,
}

/// Authentication context
#[derive(Debug, Clone)]
pub struct AuthContext {
    /// Authenticated user ID
    pub user_id: String,
    
    /// Device ID
    pub device_id: String,
    
    /// Session ID
    pub session_id: String,
    
    /// Authentication method used
    pub auth_method: AuthMethod,
    
    /// Session creation time
    pub session_created_at: std::time::Instant,
    
    /// Last activity time
    pub last_activity: std::time::Instant,
    
    /// Client IP address
    pub client_ip: Option<std::net::IpAddr>,
    
    /// User permissions
    pub permissions: Vec<CanonicalPermission>,
    
    /// Additional claims
    pub claims: HashMap<String, serde_json::Value>,
}

impl AuthContext {
    /// Get the age of the current session
    pub fn session_age(&self) -> std::time::Duration {
        self.session_created_at.elapsed()
    }
    
    /// Get time since last activity
    pub fn inactivity_duration(&self) -> std::time::Duration {
        self.last_activity.elapsed()
    }
    
    /// Check if user has a specific permission
    pub fn has_permission(&self, permission: &CanonicalPermission) -> bool {
        self.permissions.contains(permission)
    }
    
    /// Get a custom claim value
    pub fn get_claim(&self, key: &str) -> Option<&serde_json::Value> {
        self.claims.get(key)
    }
}

/// Rate limiting configuration
#[derive(Debug, Clone)]
pub struct RateLimits {
    /// Requests per minute per user
    pub requests_per_minute: Option<u32>,
    
    /// Requests per hour per user
    pub requests_per_hour: Option<u32>,
    
    /// Failed auth attempts per minute
    pub failed_auth_per_minute: Option<u32>,
    
    /// Failed auth attempts per hour
    pub failed_auth_per_hour: Option<u32>,
}

impl Default for RateLimits {
    fn default() -> Self {
        Self {
            requests_per_minute: Some(60),
            requests_per_hour: Some(1000),
            failed_auth_per_minute: Some(5),
            failed_auth_per_hour: Some(20),
        }
    }
}

impl RateLimits {
    /// Check if rate limits are exceeded
    pub async fn check_limits(
        &self,
        auth_context: &AuthContext,
        middleware_context: &MiddlewareContext,
        effects: &dyn Effects,
    ) -> Result<(), AuthError> {
        // TODO: Implement rate limiting logic using effects to track usage
        // This would typically store rate limit counters in a cache or database
        Ok(())
    }
}

/// Custom policy rule
#[derive(Debug, Clone)]
pub struct PolicyRule {
    /// Rule name
    pub name: String,
    
    /// Rule condition (as JSON)
    pub condition: serde_json::Value,
    
    /// Rule action
    pub action: PolicyAction,
}

impl PolicyRule {
    /// Apply the policy rule
    pub async fn apply(
        &self,
        auth_context: &AuthContext,
        middleware_context: &MiddlewareContext,
        effects: &dyn Effects,
    ) -> Result<(), AuthError> {
        // TODO: Implement policy rule evaluation
        // This would parse the condition JSON and evaluate it against the context
        Ok(())
    }
}

/// Policy rule actions
#[derive(Debug, Clone)]
pub enum PolicyAction {
    /// Allow the request
    Allow,
    
    /// Deny the request
    Deny,
    
    /// Require additional authentication
    RequireAdditionalAuth,
    
    /// Log the event
    Log { level: String, message: String },
    
    /// Custom action
    Custom { action: String, parameters: serde_json::Value },
}

/// Token validator trait
pub trait TokenValidator<Err>: Send + Sync 
where
    Err: std::error::Error + Send + Sync + 'static,
{
    /// Validate an authentication token
    fn validate_token<'a>(
        &'a self,
        token: &'a str,
        context: &'a MiddlewareContext,
        effects: &'a dyn Effects,
    ) -> Pin<Box<dyn Future<Output = Result<AuthContext, Err>> + Send + 'a>>;
    
    /// Refresh a token if supported
    fn refresh_token<'a>(
        &'a self,
        token: &'a str,
        context: &'a MiddlewareContext,
        effects: &'a dyn Effects,
    ) -> Pin<Box<dyn Future<Output = Result<String, Err>> + Send + 'a>>;
}

/// Permission checker trait
pub trait PermissionChecker<Err>: Send + Sync 
where
    Err: std::error::Error + Send + Sync + 'static,
{
    /// Check if the authenticated user has the required permissions
    fn check_permissions<'a>(
        &'a self,
        auth_context: &'a AuthContext,
        required_permissions: &'a [CanonicalPermission],
        context: &'a MiddlewareContext,
        effects: &'a dyn Effects,
    ) -> Pin<Box<dyn Future<Output = Result<(), Err>> + Send + 'a>>;
}

/// Trait for requests that support authentication
pub trait AuthenticatedRequest {
    /// Get the authentication token from the request
    fn auth_token(&self) -> Option<String>;
    
    /// Get the required permissions for this request
    fn required_permissions(&self) -> Vec<CanonicalPermission>;
    
    /// Set the authentication context after successful authentication
    fn set_auth_context(&mut self, context: AuthContext);
}

/// Permission alias for convenience
pub type Permission = CanonicalPermission;

/// Authentication errors
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("Authentication token is missing")]
    MissingToken,
    
    #[error("Authentication token is invalid")]
    InvalidToken,
    
    #[error("Authentication token has expired")]
    TokenExpired,
    
    #[error("Session has expired")]
    SessionExpired,
    
    #[error("Invalid authentication method")]
    InvalidAuthMethod,
    
    #[error("Insufficient permissions")]
    InsufficientPermissions,
    
    #[error("Rate limit exceeded")]
    RateLimitExceeded,
    
    #[error("IP address not allowed")]
    IpNotAllowed,
    
    #[error("Policy violation: {message}")]
    PolicyViolation { message: String },
    
    #[error("Authentication failed: {reason}")]
    AuthenticationFailed { reason: String },
}
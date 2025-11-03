//! Authentication middleware for CLI operations

use super::{CliMiddleware, CliHandler, CliOperation, CliContext, AuthMethod};
use crate::CliError;
use serde_json::{json, Value};

/// Middleware for authentication and authorization of CLI operations
pub struct AuthenticationMiddleware {
    /// Default authentication method
    default_auth_method: AuthMethod,
    /// Require authentication for all operations
    require_auth_always: bool,
    /// Operations that require authentication
    protected_operations: Vec<String>,
}

impl AuthenticationMiddleware {
    /// Create new authentication middleware
    pub fn new() -> Self {
        Self {
            default_auth_method: AuthMethod::None,
            require_auth_always: false,
            protected_operations: vec![
                "init".to_string(),
                "account".to_string(),
                "keys".to_string(),
                "frost".to_string(),
            ],
        }
    }
    
    /// Set default authentication method
    pub fn with_default_auth_method(mut self, method: AuthMethod) -> Self {
        self.default_auth_method = method;
        self
    }
    
    /// Require authentication for all operations
    pub fn with_require_auth_always(mut self, require: bool) -> Self {
        self.require_auth_always = require;
        self
    }
    
    /// Add protected operation
    pub fn with_protected_operation(mut self, operation: String) -> Self {
        self.protected_operations.push(operation);
        self
    }
    
    /// Check if operation requires authentication
    fn requires_authentication(&self, operation: &CliOperation) -> bool {
        if self.require_auth_always {
            return true;
        }
        
        match operation {
            CliOperation::Command { args } => {
                if let Some(command) = args.first() {
                    self.protected_operations.contains(command)
                } else {
                    false
                }
            }
            CliOperation::Init { .. } => true,
            CliOperation::Authenticate { .. } => false, // Don't require auth for auth operations
            _ => false,
        }
    }
    
    /// Perform authentication
    fn authenticate(&self, method: &AuthMethod, context: &CliContext) -> Result<Value, CliError> {
        match method {
            AuthMethod::None => {
                Ok(json!({
                    "authenticated": true,
                    "method": "none",
                    "user": "anonymous"
                }))
            }
            
            AuthMethod::Device => {
                // Device-based authentication would integrate with aura-agent
                // For now, simulate successful authentication
                if let Some(device_id) = &context.config.default_device {
                    Ok(json!({
                        "authenticated": true,
                        "method": "device",
                        "device_id": device_id.to_string()
                    }))
                } else {
                    Err(CliError::Authentication(
                        "No default device configured".to_string()
                    ))
                }
            }
            
            AuthMethod::Threshold { required_shares } => {
                // Threshold authentication would integrate with FROST
                // For now, simulate requiring threshold shares
                Ok(json!({
                    "authenticated": false,
                    "method": "threshold",
                    "required_shares": required_shares,
                    "message": "Threshold authentication not implemented in CLI middleware"
                }))
            }
            
            AuthMethod::Session { session_id } => {
                // Session-based authentication
                Ok(json!({
                    "authenticated": true,
                    "method": "session",
                    "session_id": session_id,
                    "message": "Session authentication simulated"
                }))
            }
        }
    }
    
    /// Get authentication method for operation
    fn get_auth_method_for_operation(&self, operation: &CliOperation) -> AuthMethod {
        match operation {
            CliOperation::Command { args } => {
                if let Some(command) = args.first() {
                    match command.as_str() {
                        "frost" | "threshold" => AuthMethod::Threshold { required_shares: 2 },
                        "device" => AuthMethod::Device,
                        _ => self.default_auth_method.clone(),
                    }
                } else {
                    self.default_auth_method.clone()
                }
            }
            _ => self.default_auth_method.clone(),
        }
    }
}

impl Default for AuthenticationMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl CliMiddleware for AuthenticationMiddleware {
    fn process(
        &self,
        operation: CliOperation,
        context: &CliContext,
        next: &dyn CliHandler,
    ) -> Result<Value, CliError> {
        match &operation {
            CliOperation::Authenticate { method } => {
                // Handle explicit authentication request
                self.authenticate(method, context)
            }
            
            _ => {
                // Check if operation requires authentication
                if self.requires_authentication(&operation) {
                    let auth_method = self.get_auth_method_for_operation(&operation);
                    
                    // Perform authentication
                    let auth_result = self.authenticate(&auth_method, context)?;
                    
                    // Check if authentication succeeded
                    if !auth_result["authenticated"].as_bool().unwrap_or(false) {
                        return Ok(json!({
                            "error": true,
                            "message": "Authentication required",
                            "auth_result": auth_result
                        }));
                    }
                    
                    // Add authentication info to context metadata
                    let mut enhanced_context = context.clone();
                    enhanced_context.metadata.insert(
                        "auth_method".to_string(),
                        format!("{:?}", auth_method)
                    );
                    enhanced_context.metadata.insert(
                        "authenticated".to_string(),
                        "true".to_string()
                    );
                    
                    // Proceed with authenticated context
                    next.handle(operation, &enhanced_context)
                } else {
                    // No authentication required, proceed normally
                    next.handle(operation, context)
                }
            }
        }
    }
    
    fn name(&self) -> &str {
        "authentication"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpCliHandler;
    
    #[test]
    fn test_no_auth_required() {
        let middleware = AuthenticationMiddleware::new();
        let handler = NoOpCliHandler;
        let context = CliContext::new("help".to_string(), vec![]);
        
        let result = middleware.process(
            CliOperation::Command { args: vec!["help".to_string()] },
            &context,
            &handler,
        );
        
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_auth_required() {
        let middleware = AuthenticationMiddleware::new();
        let handler = NoOpCliHandler;
        let context = CliContext::new("init".to_string(), vec![]);
        
        let result = middleware.process(
            CliOperation::Command { args: vec!["init".to_string()] },
            &context,
            &handler,
        );
        
        assert!(result.is_ok());
        // Should succeed because default auth method is None
    }
    
    #[test]
    fn test_explicit_authentication() {
        let middleware = AuthenticationMiddleware::new();
        let handler = NoOpCliHandler;
        let context = CliContext::new("auth".to_string(), vec![]);
        
        let result = middleware.process(
            CliOperation::Authenticate { method: AuthMethod::None },
            &context,
            &handler,
        );
        
        assert!(result.is_ok());
        let value = result.unwrap();
        assert_eq!(value["authenticated"], true);
        assert_eq!(value["method"], "none");
    }
    
    #[test]
    fn test_require_auth_always() {
        let middleware = AuthenticationMiddleware::new()
            .with_require_auth_always(true);
        let handler = NoOpCliHandler;
        let context = CliContext::new("help".to_string(), vec![]);
        
        let result = middleware.process(
            CliOperation::Command { args: vec!["help".to_string()] },
            &context,
            &handler,
        );
        
        assert!(result.is_ok());
        // Should still succeed with default None auth method
    }
}
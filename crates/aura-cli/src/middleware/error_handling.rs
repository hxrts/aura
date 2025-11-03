//! Error handling middleware for CLI operations

use super::{CliMiddleware, CliHandler, CliOperation, CliContext};
use crate::CliError;
use serde_json::{json, Value};

/// Middleware for comprehensive error handling and recovery
pub struct ErrorHandlingMiddleware {
    /// Enable detailed error messages
    detailed_errors: bool,
    /// Enable error recovery attempts
    enable_recovery: bool,
    /// Maximum retry attempts
    max_retries: u32,
}

impl ErrorHandlingMiddleware {
    /// Create new error handling middleware
    pub fn new() -> Self {
        Self {
            detailed_errors: true,
            enable_recovery: false,
            max_retries: 0,
        }
    }
    
    /// Enable detailed error messages
    pub fn with_detailed_errors(mut self, detailed: bool) -> Self {
        self.detailed_errors = detailed;
        self
    }
    
    /// Enable error recovery
    pub fn with_recovery(mut self, enable: bool) -> Self {
        self.enable_recovery = enable;
        self
    }
    
    /// Set maximum retry attempts
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self.enable_recovery = max_retries > 0;
        self
    }
    
    /// Format error for user consumption
    fn format_error(&self, error: &CliError) -> Value {
        let error_type = match error {
            CliError::CommandNotFound(_) => "command_not_found",
            CliError::InvalidInput(_) => "invalid_input",
            CliError::Configuration(_) => "configuration_error",
            CliError::FileSystem(_) => "file_system_error",
            CliError::Serialization(_) => "serialization_error",
            CliError::NotImplemented(_) => "not_implemented",
            CliError::Authentication(_) => "authentication_error",
            CliError::Network(_) => "network_error",
            CliError::OperationFailed(_) => "operation_failed",
        };
        
        let message = if self.detailed_errors {
            error.to_string()
        } else {
            self.get_user_friendly_message(error)
        };
        
        json!({
            "error": true,
            "error_type": error_type,
            "message": message,
            "detailed": self.detailed_errors
        })
    }
    
    /// Get user-friendly error message
    fn get_user_friendly_message(&self, error: &CliError) -> String {
        match error {
            CliError::CommandNotFound(_) => "Command not recognized. Use 'help' to see available commands.".to_string(),
            CliError::InvalidInput(_) => "Invalid input provided. Please check your arguments.".to_string(),
            CliError::Configuration(_) => "Configuration error. Please check your settings.".to_string(),
            CliError::FileSystem(_) => "File or directory access error.".to_string(),
            CliError::Serialization(_) => "Data format error.".to_string(),
            CliError::NotImplemented(_) => "Feature not yet implemented.".to_string(),
            CliError::Authentication(_) => "Authentication failed.".to_string(),
            CliError::Network(_) => "Network connection error.".to_string(),
            CliError::OperationFailed(_) => "Operation failed. Please try again.".to_string(),
        }
    }
    
    /// Attempt to recover from error
    fn attempt_recovery(&self, error: &CliError, operation: &CliOperation) -> Option<Value> {
        if !self.enable_recovery {
            return None;
        }
        
        match error {
            CliError::CommandNotFound(cmd) => {
                // Suggest similar commands
                Some(json!({
                    "recovery": "command_suggestion",
                    "suggestions": self.suggest_similar_commands(cmd),
                    "message": "Did you mean one of these commands?"
                }))
            }
            CliError::InvalidInput(_) => {
                // Provide input help
                Some(json!({
                    "recovery": "input_help",
                    "message": "Use 'help <command>' for usage information"
                }))
            }
            _ => None,
        }
    }
    
    /// Suggest similar commands
    fn suggest_similar_commands(&self, cmd: &str) -> Vec<String> {
        let common_commands = vec![
            "init", "status", "help", "version",
            "config", "account", "device", "keys"
        ];
        
        // Simple similarity based on edit distance
        let mut suggestions: Vec<(String, usize)> = common_commands
            .into_iter()
            .map(|c| (c.to_string(), edit_distance(cmd, c)))
            .filter(|(_, dist)| *dist <= 3)
            .collect();
        
        suggestions.sort_by_key(|(_, dist)| *dist);
        suggestions.into_iter().take(3).map(|(cmd, _)| cmd).collect()
    }
}

/// Simple edit distance calculation
fn edit_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let a_len = a_chars.len();
    let b_len = b_chars.len();
    
    let mut dp = vec![vec![0; b_len + 1]; a_len + 1];
    
    for i in 0..=a_len {
        dp[i][0] = i;
    }
    for j in 0..=b_len {
        dp[0][j] = j;
    }
    
    for i in 1..=a_len {
        for j in 1..=b_len {
            let cost = if a_chars[i - 1] == b_chars[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }
    
    dp[a_len][b_len]
}

impl Default for ErrorHandlingMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl CliMiddleware for ErrorHandlingMiddleware {
    fn process(
        &self,
        operation: CliOperation,
        context: &CliContext,
        next: &dyn CliHandler,
    ) -> Result<Value, CliError> {
        let mut retries = 0;
        
        loop {
            let result = next.handle(operation.clone(), context);
            
            match result {
                Ok(value) => return Ok(value),
                Err(error) => {
                    if retries < self.max_retries {
                        retries += 1;
                        eprintln!("Retry attempt {} due to error: {}", retries, error);
                        continue;
                    }
                    
                    // Format error for user
                    let formatted_error = self.format_error(&error);
                    
                    // Attempt recovery
                    if let Some(recovery) = self.attempt_recovery(&error, &operation) {
                        return Ok(json!({
                            "error": formatted_error,
                            "recovery": recovery
                        }));
                    }
                    
                    return Ok(formatted_error);
                }
            }
        }
    }
    
    fn name(&self) -> &str {
        "error_handling"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpCliHandler;
    
    struct FailingHandler;
    
    impl CliHandler for FailingHandler {
        fn handle(&self, _operation: CliOperation, _context: &CliContext) -> Result<Value, CliError> {
            Err(CliError::CommandNotFound("test_command".to_string()))
        }
    }
    
    #[test]
    fn test_error_formatting() {
        let middleware = ErrorHandlingMiddleware::new();
        let handler = FailingHandler;
        let context = CliContext::new("test".to_string(), vec![]);
        
        let result = middleware.process(
            CliOperation::Command { args: vec![] },
            &context,
            &handler,
        );
        
        assert!(result.is_ok());
        let value = result.unwrap();
        assert_eq!(value["error"], true);
        assert_eq!(value["error_type"], "command_not_found");
    }
    
    #[test]
    fn test_error_recovery() {
        let middleware = ErrorHandlingMiddleware::new().with_recovery(true);
        let handler = FailingHandler;
        let context = CliContext::new("test".to_string(), vec![]);
        
        let result = middleware.process(
            CliOperation::Command { args: vec![] },
            &context,
            &handler,
        );
        
        assert!(result.is_ok());
        let value = result.unwrap();
        assert!(value["recovery"].is_object());
    }
    
    #[test]
    fn test_edit_distance() {
        assert_eq!(edit_distance("init", "int"), 1);
        assert_eq!(edit_distance("status", "stat"), 2);
        assert_eq!(edit_distance("help", "halp"), 1);
    }
}
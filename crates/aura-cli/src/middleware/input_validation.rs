//! Input validation middleware for CLI operations

use super::{CliMiddleware, CliHandler, CliOperation, CliContext};
use crate::CliError;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Middleware for comprehensive input validation and sanitization
pub struct InputValidationMiddleware {
    /// Maximum argument length
    max_arg_length: usize,
    /// Maximum number of arguments
    max_arg_count: usize,
    /// Allowed commands
    allowed_commands: Option<HashSet<String>>,
    /// Forbidden patterns in arguments
    forbidden_patterns: Vec<regex::Regex>,
    /// Validation rules per command
    command_rules: HashMap<String, CommandValidationRules>,
}

/// Validation rules for specific commands
#[derive(Debug, Clone)]
pub struct CommandValidationRules {
    /// Required argument count range
    pub arg_count_range: Option<(usize, usize)>,
    /// Required arguments by position
    pub required_args: Vec<ArgValidationRule>,
    /// Optional arguments by name
    pub optional_args: HashMap<String, ArgValidationRule>,
    /// Custom validation function
    pub custom_validator: Option<fn(&[String]) -> Result<(), String>>,
}

/// Validation rule for individual arguments
#[derive(Debug, Clone)]
pub struct ArgValidationRule {
    /// Argument name
    pub name: String,
    /// Argument type
    pub arg_type: ArgType,
    /// Whether the argument is required
    pub required: bool,
    /// Custom validation pattern
    pub pattern: Option<regex::Regex>,
    /// Allowed values
    pub allowed_values: Option<Vec<String>>,
    /// Length constraints
    pub length_constraints: Option<(usize, usize)>,
}

/// Supported argument types
#[derive(Debug, Clone)]
pub enum ArgType {
    /// String argument
    String,
    /// Integer number
    Integer,
    /// Floating point number
    Float,
    /// Boolean flag
    Boolean,
    /// File path
    FilePath,
    /// Directory path
    DirectoryPath,
    /// URL
    Url,
    /// Email address
    Email,
    /// UUID
    Uuid,
    /// JSON string
    Json,
}

impl InputValidationMiddleware {
    /// Create new input validation middleware
    pub fn new() -> Self {
        Self {
            max_arg_length: 1024,
            max_arg_count: 50,
            allowed_commands: None,
            forbidden_patterns: vec![
                // Prevent command injection
                regex::Regex::new(r"[;&|`$()]").unwrap(),
                // Prevent path traversal
                regex::Regex::new(r"\.\.(/|\\)").unwrap(),
            ],
            command_rules: HashMap::new(),
        }
    }
    
    /// Set maximum argument length
    pub fn with_max_arg_length(mut self, max_length: usize) -> Self {
        self.max_arg_length = max_length;
        self
    }
    
    /// Set maximum argument count
    pub fn with_max_arg_count(mut self, max_count: usize) -> Self {
        self.max_arg_count = max_count;
        self
    }
    
    /// Set allowed commands whitelist
    pub fn with_allowed_commands(mut self, commands: HashSet<String>) -> Self {
        self.allowed_commands = Some(commands);
        self
    }
    
    /// Add forbidden pattern
    pub fn with_forbidden_pattern(mut self, pattern: &str) -> Result<Self, regex::Error> {
        let regex = regex::Regex::new(pattern)?;
        self.forbidden_patterns.push(regex);
        Ok(self)
    }
    
    /// Add validation rules for a command
    pub fn with_command_rules(mut self, command: String, rules: CommandValidationRules) -> Self {
        self.command_rules.insert(command, rules);
        self
    }
    
    /// Validate command arguments
    fn validate_arguments(&self, args: &[String]) -> Result<(), CliError> {
        // Check argument count
        if args.len() > self.max_arg_count {
            return Err(CliError::InvalidInput(format!(
                "Too many arguments: {} (max: {})",
                args.len(),
                self.max_arg_count
            )));
        }
        
        // Check each argument
        for (i, arg) in args.iter().enumerate() {
            // Check argument length
            if arg.len() > self.max_arg_length {
                return Err(CliError::InvalidInput(format!(
                    "Argument {} too long: {} characters (max: {})",
                    i,
                    arg.len(),
                    self.max_arg_length
                )));
            }
            
            // Check forbidden patterns
            for pattern in &self.forbidden_patterns {
                if pattern.is_match(arg) {
                    return Err(CliError::InvalidInput(format!(
                        "Argument {} contains forbidden pattern: {}",
                        i,
                        arg
                    )));
                }
            }
        }
        
        Ok(())
    }
    
    /// Validate specific command
    fn validate_command(&self, command: &str, args: &[String]) -> Result<(), CliError> {
        // Check command whitelist
        if let Some(allowed) = &self.allowed_commands {
            if !allowed.contains(command) {
                return Err(CliError::InvalidInput(format!(
                    "Command '{}' is not allowed",
                    command
                )));
            }
        }
        
        // Check command-specific rules
        if let Some(rules) = self.command_rules.get(command) {
            self.validate_command_rules(args, rules)?;
        }
        
        Ok(())
    }
    
    /// Validate against command-specific rules
    fn validate_command_rules(&self, args: &[String], rules: &CommandValidationRules) -> Result<(), CliError> {
        // Check argument count range
        if let Some((min, max)) = rules.arg_count_range {
            if args.len() < min || args.len() > max {
                return Err(CliError::InvalidInput(format!(
                    "Invalid argument count: {} (expected: {}-{})",
                    args.len(),
                    min,
                    max
                )));
            }
        }
        
        // Validate required positional arguments
        for (i, rule) in rules.required_args.iter().enumerate() {
            if i >= args.len() {
                return Err(CliError::InvalidInput(format!(
                    "Missing required argument: {}",
                    rule.name
                )));
            }
            
            self.validate_arg_rule(&args[i], rule)?;
        }
        
        // Run custom validator if present
        if let Some(validator) = rules.custom_validator {
            validator(args).map_err(|e| CliError::InvalidInput(e))?;
        }
        
        Ok(())
    }
    
    /// Validate individual argument against rule
    fn validate_arg_rule(&self, arg: &str, rule: &ArgValidationRule) -> Result<(), CliError> {
        // Check length constraints
        if let Some((min, max)) = rule.length_constraints {
            if arg.len() < min || arg.len() > max {
                return Err(CliError::InvalidInput(format!(
                    "Argument '{}' length {} outside valid range: {}-{}",
                    rule.name,
                    arg.len(),
                    min,
                    max
                )));
            }
        }
        
        // Check allowed values
        if let Some(allowed) = &rule.allowed_values {
            if !allowed.contains(&arg.to_string()) {
                return Err(CliError::InvalidInput(format!(
                    "Argument '{}' value '{}' not in allowed values: {:?}",
                    rule.name,
                    arg,
                    allowed
                )));
            }
        }
        
        // Check pattern
        if let Some(pattern) = &rule.pattern {
            if !pattern.is_match(arg) {
                return Err(CliError::InvalidInput(format!(
                    "Argument '{}' does not match required pattern",
                    rule.name
                )));
            }
        }
        
        // Validate type
        self.validate_arg_type(arg, &rule.arg_type, &rule.name)?;
        
        Ok(())
    }
    
    /// Validate argument type
    fn validate_arg_type(&self, arg: &str, arg_type: &ArgType, name: &str) -> Result<(), CliError> {
        match arg_type {
            ArgType::String => {
                // String is always valid
                Ok(())
            }
            ArgType::Integer => {
                arg.parse::<i64>().map_err(|_| {
                    CliError::InvalidInput(format!("Argument '{}' is not a valid integer: {}", name, arg))
                })?;
                Ok(())
            }
            ArgType::Float => {
                arg.parse::<f64>().map_err(|_| {
                    CliError::InvalidInput(format!("Argument '{}' is not a valid number: {}", name, arg))
                })?;
                Ok(())
            }
            ArgType::Boolean => {
                arg.parse::<bool>().map_err(|_| {
                    CliError::InvalidInput(format!("Argument '{}' is not a valid boolean: {}", name, arg))
                })?;
                Ok(())
            }
            ArgType::FilePath => {
                let path = std::path::Path::new(arg);
                if !path.exists() {
                    return Err(CliError::InvalidInput(format!(
                        "File does not exist: {}",
                        arg
                    )));
                }
                if !path.is_file() {
                    return Err(CliError::InvalidInput(format!(
                        "Path is not a file: {}",
                        arg
                    )));
                }
                Ok(())
            }
            ArgType::DirectoryPath => {
                let path = std::path::Path::new(arg);
                if !path.exists() {
                    return Err(CliError::InvalidInput(format!(
                        "Directory does not exist: {}",
                        arg
                    )));
                }
                if !path.is_dir() {
                    return Err(CliError::InvalidInput(format!(
                        "Path is not a directory: {}",
                        arg
                    )));
                }
                Ok(())
            }
            ArgType::Url => {
                url::Url::parse(arg).map_err(|_| {
                    CliError::InvalidInput(format!("Argument '{}' is not a valid URL: {}", name, arg))
                })?;
                Ok(())
            }
            ArgType::Email => {
                if !arg.contains('@') || !arg.contains('.') {
                    return Err(CliError::InvalidInput(format!(
                        "Argument '{}' is not a valid email: {}",
                        name,
                        arg
                    )));
                }
                Ok(())
            }
            ArgType::Uuid => {
                uuid::Uuid::parse_str(arg).map_err(|_| {
                    CliError::InvalidInput(format!("Argument '{}' is not a valid UUID: {}", name, arg))
                })?;
                Ok(())
            }
            ArgType::Json => {
                serde_json::from_str::<Value>(arg).map_err(|_| {
                    CliError::InvalidInput(format!("Argument '{}' is not valid JSON: {}", name, arg))
                })?;
                Ok(())
            }
        }
    }
    
    /// Sanitize arguments (basic sanitization)
    fn sanitize_arguments(&self, args: &mut [String]) {
        for arg in args {
            // Remove null bytes
            arg.retain(|c| c != '\0');
            
            // Trim whitespace
            *arg = arg.trim().to_string();
        }
    }
}

impl Default for InputValidationMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl CliMiddleware for InputValidationMiddleware {
    fn process(
        &self,
        mut operation: CliOperation,
        context: &CliContext,
        next: &dyn CliHandler,
    ) -> Result<Value, CliError> {
        match &mut operation {
            CliOperation::Command { args } => {
                // Validate arguments
                self.validate_arguments(args)?;
                
                // Validate command if we have args
                if !args.is_empty() {
                    let command = &args[0];
                    let command_args = &args[1..];
                    self.validate_command(command, command_args)?;
                }
                
                // Sanitize arguments
                self.sanitize_arguments(args);
                
                // Continue to next handler
                next.handle(operation, context)
            }
            
            CliOperation::ParseInput { input, .. } => {
                // Validate input length
                if input.len() > self.max_arg_length * 2 {
                    return Err(CliError::InvalidInput(format!(
                        "Input too long: {} characters (max: {})",
                        input.len(),
                        self.max_arg_length * 2
                    )));
                }
                
                // Check forbidden patterns in input
                for pattern in &self.forbidden_patterns {
                    if pattern.is_match(input) {
                        return Err(CliError::InvalidInput(
                            "Input contains forbidden pattern".to_string()
                        ));
                    }
                }
                
                next.handle(operation, context)
            }
            
            _ => {
                // For other operations, pass through without validation
                next.handle(operation, context)
            }
        }
    }
    
    fn name(&self) -> &str {
        "input_validation"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpCliHandler;
    
    #[test]
    fn test_argument_validation() {
        let middleware = InputValidationMiddleware::new()
            .with_max_arg_length(10)
            .with_max_arg_count(3);
        
        let handler = NoOpCliHandler;
        let context = CliContext::new("test".to_string(), vec![]);
        
        // Test too many arguments
        let result = middleware.process(
            CliOperation::Command { 
                args: vec!["cmd".to_string(); 5] 
            },
            &context,
            &handler,
        );
        assert!(result.is_err());
        
        // Test argument too long
        let result = middleware.process(
            CliOperation::Command { 
                args: vec!["cmd".to_string(), "very_long_argument".to_string()] 
            },
            &context,
            &handler,
        );
        assert!(result.is_err());
        
        // Test valid arguments
        let result = middleware.process(
            CliOperation::Command { 
                args: vec!["cmd".to_string(), "arg1".to_string()] 
            },
            &context,
            &handler,
        );
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_forbidden_patterns() {
        let middleware = InputValidationMiddleware::new();
        let handler = NoOpCliHandler;
        let context = CliContext::new("test".to_string(), vec![]);
        
        // Test command injection attempt
        let result = middleware.process(
            CliOperation::Command { 
                args: vec!["cmd".to_string(), "arg; rm -rf /".to_string()] 
            },
            &context,
            &handler,
        );
        assert!(result.is_err());
        
        // Test path traversal attempt
        let result = middleware.process(
            CliOperation::Command { 
                args: vec!["cmd".to_string(), "../../../etc/passwd".to_string()] 
            },
            &context,
            &handler,
        );
        assert!(result.is_err());
    }
    
    #[test]
    fn test_command_whitelist() {
        let allowed_commands = ["init", "status", "help"].iter()
            .map(|s| s.to_string())
            .collect::<HashSet<_>>();
        
        let middleware = InputValidationMiddleware::new()
            .with_allowed_commands(allowed_commands);
        
        let handler = NoOpCliHandler;
        let context = CliContext::new("test".to_string(), vec![]);
        
        // Test allowed command
        let result = middleware.process(
            CliOperation::Command { 
                args: vec!["init".to_string()] 
            },
            &context,
            &handler,
        );
        assert!(result.is_ok());
        
        // Test disallowed command
        let result = middleware.process(
            CliOperation::Command { 
                args: vec!["dangerous_command".to_string()] 
            },
            &context,
            &handler,
        );
        assert!(result.is_err());
    }
}
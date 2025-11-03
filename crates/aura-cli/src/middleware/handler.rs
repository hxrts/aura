//! CLI operation handlers

use super::CliContext;
use crate::CliError;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Operations that can be performed in the CLI system
#[derive(Debug, Clone)]
pub enum CliOperation {
    /// Execute a command with arguments
    Command {
        args: Vec<String>,
    },
    
    /// Display help information
    Help {
        command: Option<String>,
    },
    
    /// Show version information
    Version,
    
    /// Initialize configuration
    Init {
        config_path: std::path::PathBuf,
        force: bool,
    },
    
    /// Load and validate configuration
    LoadConfig {
        config_path: std::path::PathBuf,
    },
    
    /// Parse and validate input
    ParseInput {
        input: String,
        format: InputFormat,
    },
    
    /// Format output for display
    FormatOutput {
        data: Value,
        format: OutputFormat,
    },
    
    /// Report progress of long-running operation
    ReportProgress {
        message: String,
        progress: f64,
        total: Option<f64>,
    },
    
    /// Handle authentication
    Authenticate {
        method: AuthMethod,
    },
}

/// Input format options
#[derive(Debug, Clone)]
pub enum InputFormat {
    /// Command line arguments
    Args,
    /// JSON input
    Json,
    /// YAML input
    Yaml,
    /// File input
    File(std::path::PathBuf),
    /// Interactive input
    Interactive,
}

/// Output format options
#[derive(Debug, Clone)]
pub enum OutputFormat {
    /// Human-readable text
    Text,
    /// JSON output
    Json,
    /// YAML output
    Yaml,
    /// Table format
    Table,
    /// CSV format
    Csv,
}

/// Authentication methods
#[derive(Debug, Clone)]
pub enum AuthMethod {
    /// No authentication required
    None,
    /// Device-based authentication
    Device,
    /// Threshold-based authentication
    Threshold { required_shares: u16 },
    /// Session-based authentication
    Session { session_id: String },
}

/// Result type for CLI operations
pub type CliResult = Result<Value, CliError>;

/// Trait for handling CLI operations
pub trait CliHandler: Send + Sync {
    /// Handle a CLI operation
    fn handle(&self, operation: CliOperation, context: &CliContext) -> CliResult;
}

/// Core CLI handler that processes commands and operations
pub struct CoreCliHandler {
    /// Command registry mapping command names to handlers
    commands: Arc<RwLock<HashMap<String, Box<dyn CommandHandler>>>>,
    /// Configuration cache
    config_cache: Arc<RwLock<Option<CliConfig>>>,
    /// Statistics tracking
    stats: Arc<RwLock<CliStats>>,
}

/// Handler for individual CLI commands
pub trait CommandHandler: Send + Sync {
    /// Execute the command
    fn execute(&self, args: &[String], context: &CliContext) -> CliResult;
    
    /// Get command help text
    fn help(&self) -> String;
    
    /// Get command name
    fn name(&self) -> &str;
}

/// CLI operation statistics
#[derive(Debug, Default)]
pub struct CliStats {
    /// Total commands executed
    pub commands_executed: u64,
    /// Total errors encountered
    pub errors_encountered: u64,
    /// Command execution times
    pub execution_times: HashMap<String, Vec<u64>>,
    /// Last execution timestamp
    pub last_execution: Option<u64>,
}

/// Re-export for convenience
pub use super::CliConfig;

impl CoreCliHandler {
    /// Create a new core CLI handler
    pub fn new() -> Self {
        Self {
            commands: Arc::new(RwLock::new(HashMap::new())),
            config_cache: Arc::new(RwLock::new(None)),
            stats: Arc::new(RwLock::new(CliStats::default())),
        }
    }
    
    /// Register a command handler
    pub fn register_command(&self, handler: Box<dyn CommandHandler>) {
        let mut commands = self.commands.write().unwrap();
        commands.insert(handler.name().to_string(), handler);
    }
    
    /// Get available commands
    pub fn list_commands(&self) -> Vec<String> {
        let commands = self.commands.read().unwrap();
        commands.keys().cloned().collect()
    }
    
    /// Get command help
    pub fn get_command_help(&self, command: &str) -> Option<String> {
        let commands = self.commands.read().unwrap();
        commands.get(command).map(|handler| handler.help())
    }
    
    /// Update statistics
    fn update_stats(&self, command: &str, execution_time: u64, success: bool) {
        let mut stats = self.stats.write().unwrap();
        stats.commands_executed += 1;
        
        if !success {
            stats.errors_encountered += 1;
        }
        
        stats.execution_times
            .entry(command.to_string())
            .or_insert_with(Vec::new)
            .push(execution_time);
        
        stats.last_execution = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        );
    }
    
    /// Get CLI statistics
    pub fn get_stats(&self) -> CliStats {
        let stats = self.stats.read().unwrap();
        CliStats {
            commands_executed: stats.commands_executed,
            errors_encountered: stats.errors_encountered,
            execution_times: stats.execution_times.clone(),
            last_execution: stats.last_execution,
        }
    }
}

impl Default for CoreCliHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl CliHandler for CoreCliHandler {
    fn handle(&self, operation: CliOperation, context: &CliContext) -> CliResult {
        let start_time = std::time::Instant::now();
        
        let result = match operation {
            CliOperation::Command { args } => {
                if args.is_empty() {
                    return Err(CliError::InvalidInput("No command specified".to_string()));
                }
                
                let command_name = &args[0];
                let command_args = &args[1..];
                
                let commands = self.commands.read().unwrap();
                if let Some(handler) = commands.get(command_name) {
                    handler.execute(command_args, context)
                } else {
                    Err(CliError::CommandNotFound(command_name.clone()))
                }
            }
            
            CliOperation::Help { command } => {
                match command {
                    Some(cmd) => {
                        if let Some(help_text) = self.get_command_help(&cmd) {
                            Ok(json!({
                                "command": cmd,
                                "help": help_text
                            }))
                        } else {
                            Err(CliError::CommandNotFound(cmd))
                        }
                    }
                    None => {
                        let commands = self.list_commands();
                        Ok(json!({
                            "available_commands": commands,
                            "usage": "aura <command> [args...]"
                        }))
                    }
                }
            }
            
            CliOperation::Version => {
                Ok(json!({
                    "version": env!("CARGO_PKG_VERSION"),
                    "name": env!("CARGO_PKG_NAME"),
                    "description": env!("CARGO_PKG_DESCRIPTION")
                }))
            }
            
            CliOperation::Init { config_path, force } => {
                // Initialize configuration
                let config = CliConfig::default();
                
                if config_path.exists() && !force {
                    return Err(CliError::Configuration(
                        "Configuration file already exists. Use --force to overwrite.".to_string()
                    ));
                }
                
                // Create config directory if it doesn't exist
                if let Some(parent) = config_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        CliError::FileSystem(format!("Failed to create config directory: {}", e))
                    })?;
                }
                
                // Write configuration
                let config_toml = toml::to_string(&config).map_err(|e| {
                    CliError::Serialization(format!("Failed to serialize config: {}", e))
                })?;
                
                std::fs::write(&config_path, config_toml).map_err(|e| {
                    CliError::FileSystem(format!("Failed to write config file: {}", e))
                })?;
                
                Ok(json!({
                    "status": "success",
                    "config_path": config_path.display().to_string(),
                    "message": "Configuration initialized"
                }))
            }
            
            CliOperation::LoadConfig { config_path } => {
                // Load and cache configuration
                let config_str = std::fs::read_to_string(&config_path).map_err(|e| {
                    CliError::FileSystem(format!("Failed to read config file: {}", e))
                })?;
                
                let config: CliConfig = toml::from_str(&config_str).map_err(|e| {
                    CliError::Configuration(format!("Failed to parse config: {}", e))
                })?;
                
                // Cache the configuration
                let mut cache = self.config_cache.write().unwrap();
                *cache = Some(config.clone());
                
                Ok(json!({
                    "status": "success",
                    "config": config
                }))
            }
            
            CliOperation::ParseInput { input, format } => {
                match format {
                    InputFormat::Json => {
                        let parsed: Value = serde_json::from_str(&input).map_err(|e| {
                            CliError::InvalidInput(format!("Invalid JSON: {}", e))
                        })?;
                        Ok(parsed)
                    }
                    InputFormat::Yaml => {
                        let parsed: Value = serde_yaml::from_str(&input).map_err(|e| {
                            CliError::InvalidInput(format!("Invalid YAML: {}", e))
                        })?;
                        Ok(parsed)
                    }
                    InputFormat::Args => {
                        // Parse shell-style arguments
                        let args = shell_words::split(&input).map_err(|e| {
                            CliError::InvalidInput(format!("Failed to parse arguments: {}", e))
                        })?;
                        Ok(json!(args))
                    }
                    _ => {
                        Err(CliError::NotImplemented(format!("Input format {:?} not implemented", format)))
                    }
                }
            }
            
            CliOperation::FormatOutput { data, format } => {
                match format {
                    OutputFormat::Json => {
                        Ok(data)
                    }
                    OutputFormat::Yaml => {
                        let yaml_str = serde_yaml::to_string(&data).map_err(|e| {
                            CliError::Serialization(format!("Failed to serialize to YAML: {}", e))
                        })?;
                        Ok(json!({"formatted": yaml_str, "format": "yaml"}))
                    }
                    OutputFormat::Text => {
                        // Convert to human-readable text
                        let text = format!("{:#}", data);
                        Ok(json!({"formatted": text, "format": "text"}))
                    }
                    _ => {
                        Err(CliError::NotImplemented(format!("Output format {:?} not implemented", format)))
                    }
                }
            }
            
            CliOperation::ReportProgress { message, progress, total } => {
                let percentage = if let Some(total) = total {
                    if total > 0.0 {
                        (progress / total * 100.0).min(100.0)
                    } else {
                        0.0
                    }
                } else {
                    progress
                };
                
                Ok(json!({
                    "message": message,
                    "progress": progress,
                    "total": total,
                    "percentage": percentage
                }))
            }
            
            CliOperation::Authenticate { method } => {
                match method {
                    AuthMethod::None => {
                        Ok(json!({
                            "authenticated": true,
                            "method": "none"
                        }))
                    }
                    _ => {
                        Err(CliError::NotImplemented(format!("Authentication method {:?} not implemented", method)))
                    }
                }
            }
        };
        
        let execution_time = start_time.elapsed().as_millis() as u64;
        let command_name = match &result {
            Ok(_) => context.command.clone(),
            Err(_) => context.command.clone(),
        };
        
        self.update_stats(&command_name, execution_time, result.is_ok());
        
        result
    }
}

/// No-op handler for testing
pub struct NoOpCliHandler;

impl CliHandler for NoOpCliHandler {
    fn handle(&self, _operation: CliOperation, _context: &CliContext) -> CliResult {
        Ok(json!({"status": "success"}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_core_handler_version() {
        let handler = CoreCliHandler::new();
        let context = CliContext::new("version".to_string(), vec![]);
        
        let result = handler.handle(CliOperation::Version, &context);
        assert!(result.is_ok());
        
        let value = result.unwrap();
        assert!(value["version"].is_string());
        assert!(value["name"].is_string());
    }
    
    #[test]
    fn test_core_handler_help() {
        let handler = CoreCliHandler::new();
        let context = CliContext::new("help".to_string(), vec![]);
        
        let result = handler.handle(CliOperation::Help { command: None }, &context);
        assert!(result.is_ok());
        
        let value = result.unwrap();
        assert!(value["available_commands"].is_array());
        assert!(value["usage"].is_string());
    }
    
    #[test]
    fn test_no_op_handler() {
        let handler = NoOpCliHandler;
        let context = CliContext::new("test".to_string(), vec![]);
        
        let result = handler.handle(CliOperation::Version, &context);
        assert!(result.is_ok());
        
        let value = result.unwrap();
        assert_eq!(value["status"], "success");
    }
}
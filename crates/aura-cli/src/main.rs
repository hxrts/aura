//! Aura CLI Main Entry Point
//!
//! Command-line interface for the Aura threshold identity platform.
//! Built on composable middleware architecture for extensibility and maintainability.

use anyhow::Result;
use clap::{Parser, Subcommand};
use serde_json::json;
use std::path::PathBuf;

use aura_cli::{
    CliStackBuilder, CliContext, CliOperation, CliConfig,
    InputValidationMiddleware, OutputFormattingMiddleware, 
    ProgressReportingMiddleware, ErrorHandlingMiddleware,
    ConfigurationMiddleware, AuthenticationMiddleware
};
use aura_cli::middleware::handler::CoreCliHandler;

#[derive(Parser)]
#[command(name = "aura")]
#[command(about = "Aura - Threshold Identity and Storage Platform", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Config file path
    #[arg(short, long, global = true, default_value = ".aura/config.toml")]
    config: PathBuf,

    /// Output format
    #[arg(long, global = true, default_value = "text")]
    format: String,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new Aura configuration
    Init {
        /// Force overwrite existing configuration
        #[arg(short, long)]
        force: bool,
    },
    
    /// Show version information
    Version,
    
    /// Show help information
    Help {
        /// Command to show help for
        command: Option<String>,
    },
    
    /// Execute a command with arguments
    Command {
        /// Command arguments
        args: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();
    
    let cli = Cli::parse();
    
    // Create CLI context
    let mut context = CliContext::new("aura".to_string(), vec![]);
    context = context.with_verbose(cli.verbose);
    
    // Set up configuration
    let mut config = CliConfig::default();
    config.config_path = cli.config;
    context = context.with_config(config);
    
    // Build middleware stack
    let stack = CliStackBuilder::new()
        .with_middleware(std::sync::Arc::new(ConfigurationMiddleware::new()))
        .with_middleware(std::sync::Arc::new(InputValidationMiddleware::new()))
        .with_middleware(std::sync::Arc::new(AuthenticationMiddleware::new()))
        .with_middleware(std::sync::Arc::new(ProgressReportingMiddleware::new()))
        .with_middleware(std::sync::Arc::new(ErrorHandlingMiddleware::new()))
        .with_middleware(std::sync::Arc::new(OutputFormattingMiddleware::new()))
        .with_handler(std::sync::Arc::new(CoreCliHandler::new()))
        .build();
    
    // Convert CLI command to operation
    let operation = match cli.command {
        Commands::Init { force } => {
            CliOperation::Init {
                config_path: context.config.config_path.clone(),
                force,
            }
        }
        Commands::Version => CliOperation::Version,
        Commands::Help { command } => CliOperation::Help { command },
        Commands::Command { args } => CliOperation::Command { args },
    };
    
    // Execute operation through middleware stack
    match stack.process(operation, &context) {
        Ok(result) => {
            // Format and display result based on requested format
            match cli.format.as_str() {
                "json" => {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                "yaml" => {
                    println!("{}", serde_yaml::to_string(&result)?);
                }
                _ => {
                    // Default text format
                    if let Some(formatted) = result.get("formatted") {
                        if let Some(text) = formatted.as_str() {
                            println!("{}", text);
                        } else {
                            println!("{}", serde_json::to_string_pretty(&result)?);
                        }
                    } else if result.get("error").is_some() {
                        // Handle error output
                        if let Some(message) = result.get("message") {
                            if let Some(msg_str) = message.as_str() {
                                eprintln!("Error: {}", msg_str);
                                std::process::exit(1);
                            }
                        }
                        eprintln!("Error: {}", result);
                        std::process::exit(1);
                    } else {
                        // Pretty print JSON as fallback
                        println!("{}", serde_json::to_string_pretty(&result)?);
                    }
                }
            }
        }
        Err(error) => {
            eprintln!("CLI Error: {}", error);
            std::process::exit(1);
        }
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_cli_parsing() {
        let cli = Cli::try_parse_from(&["aura", "version"]).unwrap();
        assert!(matches!(cli.command, Commands::Version));
        assert!(!cli.verbose);
    }
    
    #[test]
    fn test_cli_with_verbose() {
        let cli = Cli::try_parse_from(&["aura", "--verbose", "version"]).unwrap();
        assert!(cli.verbose);
    }
    
    #[test]
    fn test_cli_init_with_force() {
        let cli = Cli::try_parse_from(&["aura", "init", "--force"]).unwrap();
        if let Commands::Init { force } = cli.command {
            assert!(force);
        } else {
            panic!("Expected Init command");
        }
    }
}
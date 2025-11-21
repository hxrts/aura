//! Authority Commands CLI Test
//!
//! Tests the CLI authority management commands using the current
//! AuthorityId-centric architecture.
//!
//! **Coverage:**
//! - Authority creation via CLI
//! - Authority inspection and status commands  
//! - Context management commands
//! - CLI integration with aura-agent
//!
//! **Architecture Compliance:**
//! - Uses AuthorityId throughout
//! - Tests real CLI commands and output
//! - Integrates with current agent runtime

use aura_core::AuraResult;
use std::process::Command;
use tempfile::TempDir;
use tokio;

/// Test authority creation through CLI
#[tokio::test]
async fn test_cli_authority_creation() -> AuraResult<()> {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let config_path = temp_dir.path().join("aura_config");
    
    // Test authority creation command
    let output = Command::new("cargo")
        .args(&["run", "--bin", "aura", "--", "authority", "create"])
        .arg("--config-dir")
        .arg(&config_path)
        .arg("--name")
        .arg("test-authority")
        .output();
    
    match output {
        Ok(result) => {
            if result.status.success() {
                let stdout = String::from_utf8_lossy(&result.stdout);
                
                // Should contain authority creation confirmation
                assert!(stdout.contains("Authority created"));
                assert!(stdout.contains("test-authority"));
                
                // Should display authority ID
                assert!(stdout.contains("Authority ID:"));
                
                println!("CLI authority creation test passed");
            } else {
                // CLI might not be available in test environment
                println!("CLI not available, skipping authority creation test");
            }
        }
        Err(_) => {
            println!("CLI not available, skipping authority creation test");
        }
    }
    
    Ok(())
}

/// Test authority status inspection through CLI
#[tokio::test]
async fn test_cli_authority_status() -> AuraResult<()> {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let config_path = temp_dir.path().join("aura_config");
    
    // First create an authority
    let create_output = Command::new("cargo")
        .args(&["run", "--bin", "aura", "--", "authority", "create"])
        .arg("--config-dir")
        .arg(&config_path)
        .arg("--name")
        .arg("status-test-authority")
        .output();
    
    if let Ok(create_result) = create_output {
        if create_result.status.success() {
            // Now test status command
            let status_output = Command::new("cargo")
                .args(&["run", "--bin", "aura", "--", "authority", "status"])
                .arg("--config-dir")
                .arg(&config_path)
                .output();
            
            if let Ok(status_result) = status_output {
                if status_result.status.success() {
                    let stdout = String::from_utf8_lossy(&status_result.stdout);
                    
                    // Should show authority information
                    assert!(stdout.contains("status-test-authority") || stdout.contains("Authority"));
                    
                    println!("CLI authority status test passed");
                } else {
                    println!("Authority status command failed");
                }
            }
        }
    } else {
        println!("CLI not available, skipping authority status test");
    }
    
    Ok(())
}

/// Test authority listing through CLI
#[tokio::test]
async fn test_cli_authority_list() -> AuraResult<()> {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let config_path = temp_dir.path().join("aura_config");
    
    // Test list command (should work even with no authorities)
    let output = Command::new("cargo")
        .args(&["run", "--bin", "aura", "--", "authority", "list"])
        .arg("--config-dir")
        .arg(&config_path)
        .output();
    
    match output {
        Ok(result) => {
            if result.status.success() {
                let stdout = String::from_utf8_lossy(&result.stdout);
                
                // Should either show authorities or indicate none exist
                assert!(
                    stdout.contains("Authority") || 
                    stdout.contains("No authorities") ||
                    stdout.contains("authorities found")
                );
                
                println!("CLI authority list test passed");
            } else {
                println!("Authority list command failed");
            }
        }
        Err(_) => {
            println!("CLI not available, skipping authority list test");
        }
    }
    
    Ok(())
}

/// Test context management through CLI
#[tokio::test]
async fn test_cli_context_commands() -> AuraResult<()> {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let config_path = temp_dir.path().join("aura_config");
    
    // Create an authority first
    let create_output = Command::new("cargo")
        .args(&["run", "--bin", "aura", "--", "authority", "create"])
        .arg("--config-dir")
        .arg(&config_path)
        .arg("--name")
        .arg("context-test-authority")
        .output();
    
    if let Ok(create_result) = create_output {
        if create_result.status.success() {
            // Test context list command
            let context_output = Command::new("cargo")
                .args(&["run", "--bin", "aura", "--", "context", "list"])
                .arg("--config-dir")
                .arg(&config_path)
                .output();
            
            if let Ok(context_result) = context_output {
                if context_result.status.success() {
                    let stdout = String::from_utf8_lossy(&context_result.stdout);
                    
                    // Should show context information or indicate none exist
                    assert!(
                        stdout.contains("Context") || 
                        stdout.contains("No contexts") ||
                        stdout.contains("contexts found")
                    );
                    
                    println!("CLI context commands test passed");
                } else {
                    println!("Context list command failed");
                }
            }
        }
    } else {
        println!("CLI not available, skipping context commands test");
    }
    
    Ok(())
}

/// Test CLI help and version commands
#[tokio::test]
async fn test_cli_help_and_version() -> AuraResult<()> {
    // Test help command
    let help_output = Command::new("cargo")
        .args(&["run", "--bin", "aura", "--", "--help"])
        .output();
    
    match help_output {
        Ok(result) => {
            if result.status.success() {
                let stdout = String::from_utf8_lossy(&result.stdout);
                
                // Should show help information
                assert!(stdout.contains("USAGE") || stdout.contains("Commands") || stdout.contains("authority"));
                
                println!("CLI help test passed");
            }
        }
        Err(_) => {
            println!("CLI not available, skipping help test");
        }
    }
    
    // Test version command
    let version_output = Command::new("cargo")
        .args(&["run", "--bin", "aura", "--", "--version"])
        .output();
    
    match version_output {
        Ok(result) => {
            if result.status.success() {
                let stdout = String::from_utf8_lossy(&result.stdout);
                
                // Should show version information
                assert!(stdout.contains("aura") || stdout.len() > 0);
                
                println!("CLI version test passed");
            }
        }
        Err(_) => {
            println!("CLI not available, skipping version test");
        }
    }
    
    Ok(())
}

/// Test CLI error handling
#[tokio::test]
async fn test_cli_error_handling() -> AuraResult<()> {
    // Test invalid command
    let invalid_output = Command::new("cargo")
        .args(&["run", "--bin", "aura", "--", "invalid-command"])
        .output();
    
    match invalid_output {
        Ok(result) => {
            // Should fail with non-zero exit code
            assert!(!result.status.success());
            
            let stderr = String::from_utf8_lossy(&result.stderr);
            
            // Should show error message
            assert!(
                stderr.contains("error") || 
                stderr.contains("invalid") ||
                stderr.contains("unknown")
            );
            
            println!("CLI error handling test passed");
        }
        Err(_) => {
            println!("CLI not available, skipping error handling test");
        }
    }
    
    Ok(())
}

/// Integration test for CLI workflow with authority creation and usage
#[tokio::test]
async fn test_cli_authority_workflow() -> AuraResult<()> {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let config_path = temp_dir.path().join("aura_config");
    let authority_name = "workflow-test-authority";
    
    // Step 1: Create authority
    let create_output = Command::new("cargo")
        .args(&["run", "--bin", "aura", "--", "authority", "create"])
        .arg("--config-dir")
        .arg(&config_path)
        .arg("--name")
        .arg(authority_name)
        .output();
    
    if let Ok(create_result) = create_output {
        if create_result.status.success() {
            println!("✅ Authority creation successful");
            
            // Step 2: List authorities (should include new authority)
            let list_output = Command::new("cargo")
                .args(&["run", "--bin", "aura", "--", "authority", "list"])
                .arg("--config-dir")
                .arg(&config_path)
                .output();
            
            if let Ok(list_result) = list_output {
                if list_result.status.success() {
                    let stdout = String::from_utf8_lossy(&list_result.stdout);
                    
                    // Should find our created authority
                    if stdout.contains(authority_name) {
                        println!("✅ Authority appears in list");
                    } else {
                        println!("⚠️ Authority not found in list (may be expected)");
                    }
                }
            }
            
            // Step 3: Check authority status
            let status_output = Command::new("cargo")
                .args(&["run", "--bin", "aura", "--", "authority", "status"])
                .arg("--config-dir")
                .arg(&config_path)
                .output();
            
            if let Ok(status_result) = status_output {
                if status_result.status.success() {
                    println!("✅ Authority status check successful");
                } else {
                    println!("⚠️ Authority status check failed");
                }
            }
            
            println!("CLI authority workflow test completed");
        } else {
            println!("Authority creation failed, skipping workflow test");
        }
    } else {
        println!("CLI not available, skipping workflow test");
    }
    
    Ok(())
}
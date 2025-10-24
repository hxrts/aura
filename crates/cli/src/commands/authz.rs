// Authorization management commands
//
// Commands for permission management, capability delegation, and access control.
// These commands handle "what you can do" concerns.

use crate::config::Config;
use aura_agent::IntegratedAgent;
use aura_journal::{
    capability::{
        identity::IndividualId,
        types::CapabilityScope,
    },
    DeviceId,
};
use clap::Subcommand;
use tracing::info;

#[derive(Subcommand)]
pub enum AuthzCommand {
    /// List current permissions and capabilities
    List,
    
    /// Grant permissions to an authenticated device
    Grant {
        /// Target device ID (must be authenticated)
        #[arg(long)]
        device_id: String,
        
        /// Operations to grant (comma-separated)
        #[arg(long)]
        operations: String,
        
        /// Expiry timestamp (Unix seconds)
        #[arg(long)]
        expiry: Option<u64>,
    },
    
    /// Revoke permissions from a device
    Revoke {
        /// Target device ID
        #[arg(long)]
        device_id: String,
        
        /// Operations to revoke (comma-separated)
        #[arg(long)]
        operations: String,
        
        /// Reason for revocation
        #[arg(long)]
        reason: String,
    },
    
    /// Check if a device has specific permissions
    Check {
        /// Device ID to check
        #[arg(long)]
        device_id: String,
        
        /// Operation to check permission for
        #[arg(long)]
        operation: String,
    },
    
    /// Delegate capability to another subject (advanced)
    Delegate {
        /// Parent capability scope (namespace:operation)
        #[arg(long)]
        parent: String,
        
        /// Target subject ID
        #[arg(long)]
        subject: String,
        
        /// New capability scope (namespace:operation)
        #[arg(long)]
        scope: String,
        
        /// Optional resource constraint
        #[arg(long)]
        resource: Option<String>,
        
        /// Expiry timestamp (Unix seconds)
        #[arg(long)]
        expiry: Option<u64>,
    },
    
    /// Show permission history for a device
    History {
        /// Device ID to show history for
        #[arg(long)]
        device_id: String,
    },
}

pub async fn handle_authz_command(command: AuthzCommand, config: &Config) -> anyhow::Result<()> {
    let agent = IntegratedAgent::from_config_path(&config.agent_config_path).await?;
    
    match command {
        AuthzCommand::List => {
            info!("Listing current permissions and capabilities");
            
            let permissions = agent.list_permissions().await?;
            println!("Current Permissions:");
            for (device_id, perms) in permissions {
                println!("  Device {}: {:?}", device_id, perms);
            }
        },
        
        AuthzCommand::Grant { device_id, operations, expiry } => {
            info!("Granting permissions to device: {}", device_id);
            
            let ops: Vec<String> = operations.split(',').map(|s| s.trim().to_string()).collect();
            let device_id: DeviceId = device_id.parse()?;
            
            let auth_token = agent.issue_authorization_token(device_id, ops.clone()).await?;
            
            println!("Authorization token granted:");
            println!("  Device: {}", auth_token.authorized_device);
            println!("  Operations: {:?}", auth_token.permitted_operations);
            println!("  Expires: {}", auth_token.expires_at);
        },
        
        AuthzCommand::Revoke { device_id, operations, reason } => {
            info!("Revoking permissions from device: {} - {}", device_id, reason);
            
            let ops: Vec<String> = operations.split(',').map(|s| s.trim().to_string()).collect();
            let device_id: DeviceId = device_id.parse()?;
            
            agent.revoke_permissions(device_id, ops, &reason).await?;
            println!("Permissions revoked successfully");
        },
        
        AuthzCommand::Check { device_id, operation } => {
            info!("Checking permissions for device: {} operation: {}", device_id, operation);
            
            let device_id: DeviceId = device_id.parse()?;
            
            // Create a mock authorization token to check
            let mock_token = aura_agent::types::AuthorizationToken {
                permitted_operations: vec![operation.clone()],
                expires_at: u64::MAX,
                capability_proof: vec![],
                authorized_device: device_id,
            };
            
            let has_permission = agent.check_authorization(&mock_token, &operation).await?;
            
            println!("Permission check result:");
            println!("  Device: {}", device_id);
            println!("  Operation: {}", operation);
            println!("  Authorized: {}", has_permission);
        },
        
        AuthzCommand::Delegate { parent, subject, scope, resource, expiry } => {
            info!("Delegating capability: {} -> {} ({})", parent, subject, scope);
            
            // TODO: Implement capability delegation
            println!("Capability delegation: PENDING");
            println!("  Parent: {}", parent);
            println!("  Subject: {}", subject);
            println!("  Scope: {}", scope);
            if let Some(resource) = resource {
                println!("  Resource: {}", resource);
            }
            if let Some(expiry) = expiry {
                println!("  Expiry: {}", expiry);
            }
        },
        
        AuthzCommand::History { device_id } => {
            info!("Showing permission history for device: {}", device_id);
            
            let device_id: DeviceId = device_id.parse()?;
            let history = agent.get_permission_history(device_id).await?;
            
            println!("Permission History for {}:", device_id);
            for entry in history {
                println!("  {}: {:?}", entry.timestamp, entry.operation);
            }
        },
    }
    
    Ok(())
}
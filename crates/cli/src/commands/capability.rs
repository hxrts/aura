// Capability management commands

use crate::config::Config;
use aura_agent::IntegratedAgent;
use aura_journal::{
    capability::{
        identity::IndividualId,
        types::{CapabilityScope, Subject},
    },
    DeviceId,
};
use clap::Subcommand;
use tracing::info;

#[derive(Subcommand)]
pub enum CapabilityCommand {
    /// List current capabilities
    List,
    
    /// Delegate capability to another subject
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
    
    /// Revoke capability
    Revoke {
        /// Capability ID to revoke
        capability_id: String,
        
        /// Reason for revocation
        #[arg(long)]
        reason: String,
    },
    
    /// Show capability delegation tree
    Tree,
    
    /// Bootstrap root capabilities for new account
    Bootstrap {
        /// Initial devices (comma-separated UUIDs)
        #[arg(long)]
        devices: String,
        
        /// Threshold (M-of-N)
        #[arg(long, default_value = "2")]
        threshold: u16,
    },
    
    /// Create MLS group with capability-driven membership
    CreateGroup {
        /// Group identifier
        group_id: String,
        
        /// Initial members (comma-separated individual IDs)
        #[arg(long)]
        members: String,
    },
    
    /// Show authority graph state
    Authority,
    
    /// Audit capability operations
    Audit {
        /// Number of recent operations to show
        #[arg(long, default_value = "20")]
        limit: usize,
    },
}

pub async fn handle_capability_command(command: CapabilityCommand, config: &Config) -> anyhow::Result<()> {
    match command {
        CapabilityCommand::List => {
            list_capabilities(config).await
        }
        
        CapabilityCommand::Delegate { parent, subject, scope, resource, expiry } => {
            delegate_capability(config, &parent, &subject, &scope, resource.as_deref(), expiry).await
        }
        
        CapabilityCommand::Revoke { capability_id, reason } => {
            revoke_capability(config, &capability_id, &reason).await
        }
        
        CapabilityCommand::Tree => {
            show_capability_tree(config).await
        }
        
        CapabilityCommand::Bootstrap { devices, threshold } => {
            bootstrap_capabilities(config, &devices, threshold).await
        }
        
        CapabilityCommand::CreateGroup { group_id, members } => {
            create_mls_group(config, &group_id, &members).await
        }
        
        CapabilityCommand::Authority => {
            show_authority_graph(config).await
        }
        
        CapabilityCommand::Audit { limit } => {
            audit_capability_operations(config, limit).await
        }
    }
}

async fn list_capabilities(config: &Config) -> anyhow::Result<()> {
    info!("Listing current capabilities");
    
    let agent = create_agent(config).await?;
    let capabilities = agent.capability_agent.list_capabilities();
    
    if capabilities.is_empty() {
        println!("No capabilities found for this identity");
        return Ok(());
    }
    
    println!("Current Capabilities:");
    println!("====================");
    
    for cap in capabilities {
        println!("• {}:{}", cap.namespace, cap.operation);
        if let Some(resource) = &cap.resource {
            println!("  Resource: {}", resource);
        }
        if !cap.params.is_empty() {
            println!("  Parameters:");
            for (key, value) in &cap.params {
                println!("    {}: {}", key, value);
            }
        }
        println!();
    }
    
    Ok(())
}

async fn delegate_capability(
    config: &Config,
    parent: &str,
    subject: &str,
    scope: &str,
    resource: Option<&str>,
    expiry: Option<u64>,
) -> anyhow::Result<()> {
    info!("Delegating capability {} to {}", scope, subject);
    
    let mut agent = create_agent(config).await?;
    
    // Parse parent capability scope
    let parent_scope = parse_capability_scope(parent, None)?;
    
    // Parse new capability scope
    let new_scope = parse_capability_scope(scope, resource)?;
    
    // Create target subject
    let target_subject = Subject::new(subject);
    
    // Delegate capability
    let delegation = agent.capability_agent.delegate_capability(
        parent_scope,
        target_subject,
        new_scope.clone(),
        expiry,
    )?;
    
    println!("✓ Capability delegated successfully");
    println!("  Capability ID: {}", delegation.capability_id.as_hex());
    println!("  Subject: {}", subject);
    println!("  Scope: {}:{}", new_scope.namespace, new_scope.operation);
    if let Some(resource) = &new_scope.resource {
        println!("  Resource: {}", resource);
    }
    if let Some(expiry) = expiry {
        println!("  Expires: {}", expiry);
    }
    
    Ok(())
}

async fn revoke_capability(config: &Config, capability_id: &str, reason: &str) -> anyhow::Result<()> {
    info!("Revoking capability {}: {}", capability_id, reason);
    
    let mut agent = create_agent(config).await?;
    
    // Parse capability ID
    let cap_id_bytes = hex::decode(capability_id)
        .map_err(|_| anyhow::anyhow!("Invalid capability ID hex format"))?;
    
    if cap_id_bytes.len() != 32 {
        return Err(anyhow::anyhow!("Capability ID must be 32 bytes (64 hex characters)"));
    }
    
    let mut cap_id_array = [0u8; 32];
    cap_id_array.copy_from_slice(&cap_id_bytes);
    let cap_id = aura_journal::capability::types::CapabilityId(cap_id_array);
    
    // Revoke capability
    let revocation = agent.capability_agent.revoke_capability(cap_id, reason.to_string())?;
    
    println!("✓ Capability revoked successfully");
    println!("  Capability ID: {}", capability_id);
    println!("  Reason: {}", reason);
    println!("  Revoked at: {}", revocation.revoked_at);
    
    Ok(())
}

async fn show_capability_tree(_config: &Config) -> anyhow::Result<()> {
    println!("Capability Delegation Tree:");
    println!("===========================");
    
    // TODO: Implement capability tree visualization
    println!("(Tree visualization not yet implemented)");
    
    Ok(())
}

async fn bootstrap_capabilities(config: &Config, devices: &str, threshold: u16) -> anyhow::Result<()> {
    info!("Bootstrapping capabilities for {} devices with threshold {}", devices, threshold);
    
    let mut agent = create_agent(config).await?;
    
    // Parse device list
    let device_ids: Result<Vec<DeviceId>, _> = devices
        .split(',')
        .map(|s| s.trim())
        .map(|s| -> anyhow::Result<DeviceId> {
            let uuid = uuid::Uuid::parse_str(s)
                .map_err(|_| anyhow::anyhow!("Invalid device UUID: {}", s))?;
            Ok(DeviceId(uuid))
        })
        .collect();
    
    let device_ids = device_ids?;
    
    if device_ids.is_empty() {
        return Err(anyhow::anyhow!("At least one device must be specified"));
    }
    
    // Bootstrap account
    let effects = aura_crypto::Effects::test();
    agent.bootstrap(device_ids.clone(), threshold, &effects).await?;
    
    println!("✓ Capabilities bootstrapped successfully");
    println!("  Devices: {}", device_ids.len());
    println!("  Threshold: {}", threshold);
    println!("  Root authorities created for each device");
    
    Ok(())
}

async fn create_mls_group(config: &Config, group_id: &str, members: &str) -> anyhow::Result<()> {
    info!("Creating MLS group '{}' with members: {}", group_id, members);
    
    let mut agent = create_agent(config).await?;
    
    // Parse member list
    let member_ids: Vec<IndividualId> = members
        .split(',')
        .map(|s| s.trim())
        .map(|s| IndividualId::new(s))
        .collect();
    
    if member_ids.is_empty() {
        return Err(anyhow::anyhow!("At least one member must be specified"));
    }
    
    // Create MLS group
    agent.network_create_group(group_id, member_ids.clone()).await?;
    
    println!("✓ MLS group created successfully");
    println!("  Group ID: {}", group_id);
    println!("  Members: {}", member_ids.len());
    for member in &member_ids {
        println!("    - {}", member.0);
    }
    
    Ok(())
}

async fn show_authority_graph(_config: &Config) -> anyhow::Result<()> {
    println!("Authority Graph State:");
    println!("=====================");
    
    // TODO: Implement authority graph visualization
    println!("(Authority graph visualization not yet implemented)");
    
    Ok(())
}

async fn audit_capability_operations(_config: &Config, limit: usize) -> anyhow::Result<()> {
    println!("Recent Capability Operations (limit: {}):", limit);
    println!("==========================================");
    
    // TODO: Implement capability audit log
    println!("(Capability audit log not yet implemented)");
    
    Ok(())
}

fn parse_capability_scope(scope_str: &str, resource: Option<&str>) -> anyhow::Result<CapabilityScope> {
    let parts: Vec<&str> = scope_str.split(':').collect();
    if parts.len() != 2 {
        return Err(anyhow::anyhow!("Capability scope must be in format 'namespace:operation'"));
    }
    
    let namespace = parts[0].to_string();
    let operation = parts[1].to_string();
    
    let mut scope = CapabilityScope::simple(&namespace, &operation);
    if let Some(res) = resource {
        scope.resource = Some(res.to_string());
    }
    
    Ok(scope)
}

async fn create_agent(config: &Config) -> anyhow::Result<IntegratedAgent> {
    let device_id = config.device_id;
    let account_id = config.account_id;
    let storage_root = config.data_dir.join("storage");
    let effects = aura_crypto::Effects::test(); // Use test effects for CLI
    
    IntegratedAgent::new(device_id, account_id, storage_root, effects)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create agent: {}", e))
}
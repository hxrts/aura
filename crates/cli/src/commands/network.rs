// Network and CGKA commands

use crate::config::Config;
use aura_agent::IntegratedAgent;
use aura_journal::{
    capability::{identity::IndividualId, types::CapabilityScope},
};
use clap::Subcommand;
use std::collections::BTreeSet;
use tracing::info;

#[derive(Subcommand)]
pub enum NetworkCommand {
    /// Connect to peer
    Connect {
        /// Peer individual ID
        peer_id: String,
        
        /// Peer address
        address: String,
    },
    
    /// Disconnect from peer
    Disconnect {
        /// Peer individual ID
        peer_id: String,
    },
    
    /// List connected peers
    Peers,
    
    /// Create MLS group with network propagation
    CreateGroup {
        /// Group identifier
        group_id: String,
        
        /// Initial members (comma-separated individual IDs)
        #[arg(long)]
        members: String,
    },
    
    /// Send data to group members
    SendData {
        /// Target group ID
        group_id: String,
        
        /// Data to send
        data: String,
        
        /// Context for the data
        #[arg(long, default_value = "message")]
        context: String,
    },
    
    /// Delegate capability to peer via network
    DelegateCapability {
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
        
        /// Target peers (comma-separated, optional for broadcast)
        #[arg(long)]
        peers: Option<String>,
        
        /// Expiry timestamp (Unix seconds)
        #[arg(long)]
        expiry: Option<u64>,
    },
    
    /// Revoke capability via network
    RevokeCapability {
        /// Capability ID to revoke
        capability_id: String,
        
        /// Reason for revocation
        #[arg(long)]
        reason: String,
        
        /// Target peers (comma-separated, optional for broadcast)
        #[arg(long)]
        peers: Option<String>,
    },
    
    /// Show network statistics
    Stats,
    
    /// Show CGKA group status
    Groups,
    
    /// Process capability changes for group
    ProcessChanges {
        /// Group ID to process
        group_id: String,
    },
    
    /// Show pending network operations
    Pending,
}

pub async fn handle_network_command(command: NetworkCommand, config: &Config) -> anyhow::Result<()> {
    match command {
        NetworkCommand::Connect { peer_id, address } => {
            connect_peer(config, &peer_id, &address).await
        }
        
        NetworkCommand::Disconnect { peer_id } => {
            disconnect_peer(config, &peer_id).await
        }
        
        NetworkCommand::Peers => {
            list_peers(config).await
        }
        
        NetworkCommand::CreateGroup { group_id, members } => {
            create_group(config, &group_id, &members).await
        }
        
        NetworkCommand::SendData { group_id, data, context } => {
            send_data(config, &group_id, &data, &context).await
        }
        
        NetworkCommand::DelegateCapability { parent, subject, scope, resource, peers, expiry } => {
            delegate_capability(config, &parent, &subject, &scope, resource.as_deref(), peers.as_deref(), expiry).await
        }
        
        NetworkCommand::RevokeCapability { capability_id, reason, peers } => {
            revoke_capability(config, &capability_id, &reason, peers.as_deref()).await
        }
        
        NetworkCommand::Stats => {
            show_network_stats(config).await
        }
        
        NetworkCommand::Groups => {
            show_groups(config).await
        }
        
        NetworkCommand::ProcessChanges { group_id } => {
            process_capability_changes(config, &group_id).await
        }
        
        NetworkCommand::Pending => {
            show_pending_operations(config).await
        }
    }
}

async fn connect_peer(config: &Config, peer_id: &str, address: &str) -> anyhow::Result<()> {
    info!("Connecting to peer {} at {}", peer_id, address);
    
    let agent = create_agent(config).await?;
    let peer = IndividualId::new(peer_id);
    
    // Connect to peer
    agent.network_connect(peer.clone(), address).await?;
    
    println!("✓ Connected to peer successfully");
    println!("  Peer ID: {}", peer_id);
    println!("  Address: {}", address);
    
    Ok(())
}

async fn disconnect_peer(config: &Config, peer_id: &str) -> anyhow::Result<()> {
    info!("Disconnecting from peer {}", peer_id);
    
    let agent = create_agent(config).await?;
    let peer = IndividualId::new(peer_id);
    
    // Remove peer from transport
    agent.transport.remove_peer(&peer).await;
    
    println!("✓ Disconnected from peer");
    println!("  Peer ID: {}", peer_id);
    
    Ok(())
}

async fn list_peers(config: &Config) -> anyhow::Result<()> {
    info!("Listing connected peers");
    
    let agent = create_agent(config).await?;
    
    // Get connected peers
    let peers = agent.transport.get_peers().await;
    
    if peers.is_empty() {
        println!("No connected peers");
        return Ok(());
    }
    
    println!("Connected Peers:");
    println!("================");
    
    for peer in &peers {
        println!("• {}", peer.0);
    }
    
    println!("Total: {} peers", peers.len());
    
    Ok(())
}

async fn create_group(config: &Config, group_id: &str, members: &str) -> anyhow::Result<()> {
    info!("Creating network MLS group '{}' with members: {}", group_id, members);
    
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
    
    // Create group with network propagation
    agent.network_create_group(group_id, member_ids.clone()).await?;
    
    println!("✓ MLS group created and propagated to network");
    println!("  Group ID: {}", group_id);
    println!("  Members: {}", member_ids.len());
    for member in &member_ids {
        println!("    - {}", member.0);
    }
    
    Ok(())
}

async fn send_data(config: &Config, group_id: &str, data: &str, context: &str) -> anyhow::Result<()> {
    info!("Sending data to group '{}' with context '{}'", group_id, context);
    
    let agent = create_agent(config).await?;
    
    // Get group members from capability graph
    let member_scope = CapabilityScope::with_resource("mls", "member", group_id);
    
    // Send data to group
    let data_bytes = data.as_bytes().to_vec();
    let message_id = agent.transport.send_data(
        data_bytes.clone(),
        format!("{}:{}", context, group_id),
        member_scope,
        None, // Send to all group members
    ).await
        .map_err(|e| anyhow::anyhow!("Failed to send data: {}", e))?;
    
    println!("✓ Data sent to group");
    println!("  Group ID: {}", group_id);
    println!("  Context: {}", context);
    println!("  Size: {} bytes", data_bytes.len());
    println!("  Message ID: {}", message_id);
    
    Ok(())
}

async fn delegate_capability(
    config: &Config,
    parent: &str,
    subject: &str,
    scope: &str,
    resource: Option<&str>,
    peers: Option<&str>,
    expiry: Option<u64>,
) -> anyhow::Result<()> {
    info!("Delegating capability {} to {} via network", scope, subject);
    
    let mut agent = create_agent(config).await?;
    
    // Parse parent capability scope
    let parent_scope = parse_capability_scope(parent, None)?;
    
    // Parse new capability scope
    let new_scope = parse_capability_scope(scope, resource)?;
    
    // Create target subject
    let target_subject = aura_journal::capability::types::Subject::new(subject);
    
    // Parse target peers if provided
    let recipients = if let Some(peer_str) = peers {
        let peer_ids: BTreeSet<IndividualId> = peer_str
            .split(',')
            .map(|s| s.trim())
            .map(|s| IndividualId::new(s))
            .collect();
        Some(peer_ids)
    } else {
        None
    };
    
    // Delegate capability via network
    agent.network_delegate_capability(
        parent_scope,
        target_subject,
        new_scope.clone(),
        expiry,
        recipients.clone(),
    ).await?;
    
    println!("✓ Capability delegated via network");
    println!("  Subject: {}", subject);
    println!("  Scope: {}:{}", new_scope.namespace, new_scope.operation);
    if let Some(resource) = &new_scope.resource {
        println!("  Resource: {}", resource);
    }
    if let Some(peers) = &recipients {
        println!("  Sent to: {} peers", peers.len());
    } else {
        println!("  Broadcast to all peers");
    }
    
    Ok(())
}

async fn revoke_capability(
    config: &Config,
    capability_id: &str,
    reason: &str,
    peers: Option<&str>,
) -> anyhow::Result<()> {
    info!("Revoking capability {} via network: {}", capability_id, reason);
    
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
    
    // Parse target peers if provided
    let recipients = if let Some(peer_str) = peers {
        let peer_ids: BTreeSet<IndividualId> = peer_str
            .split(',')
            .map(|s| s.trim())
            .map(|s| IndividualId::new(s))
            .collect();
        Some(peer_ids)
    } else {
        None
    };
    
    // Revoke capability via network
    agent.network_revoke_capability(cap_id, reason.to_string(), recipients.clone()).await?;
    
    println!("✓ Capability revoked via network");
    println!("  Capability ID: {}", capability_id);
    println!("  Reason: {}", reason);
    if let Some(peers) = &recipients {
        println!("  Sent to: {} peers", peers.len());
    } else {
        println!("  Broadcast to all peers");
    }
    
    Ok(())
}

async fn show_network_stats(config: &Config) -> anyhow::Result<()> {
    info!("Showing network statistics");
    
    let agent = create_agent(config).await?;
    
    // Get network stats
    let network_stats = agent.get_network_stats().await;
    let storage_stats = agent.get_storage_stats().await?;
    
    println!("Network Statistics:");
    println!("==================");
    println!("Connected Peers: {}", network_stats.connected_peers);
    println!("Pending Messages: {}", network_stats.pending_messages);
    
    println!("\nStorage Statistics:");
    println!("==================");
    println!("Total Entries: {}", storage_stats.total_entries);
    println!("Accessible Entries: {}", storage_stats.accessible_entries);
    
    Ok(())
}

async fn show_groups(config: &Config) -> anyhow::Result<()> {
    info!("Showing CGKA groups");
    
    let agent = create_agent(config).await?;
    
    // Get group memberships
    let groups = agent.capability_agent.list_groups();
    
    if groups.is_empty() {
        println!("No group memberships found");
        return Ok(());
    }
    
    println!("CGKA Groups:");
    println!("============");
    
    for group_id in &groups {
        println!("• {}", group_id);
        
        // Get group epoch if available
        if let Some(epoch) = agent.capability_agent.cgka_manager.get_epoch(group_id) {
            println!("  Epoch: {}", epoch.value());
        }
        
        // Get roster if available
        if let Some(roster) = agent.capability_agent.cgka_manager.get_roster(group_id) {
            println!("  Members: {}", roster.member_count());
        }
        
        println!();
    }
    
    println!("Total: {} groups", groups.len());
    
    Ok(())
}

async fn process_capability_changes(config: &Config, group_id: &str) -> anyhow::Result<()> {
    info!("Processing capability changes for group '{}'", group_id);
    
    let mut agent = create_agent(config).await?;
    
    // Process capability changes
    agent.capability_agent.process_capability_changes(group_id)?;
    
    println!("✓ Capability changes processed for group '{}'", group_id);
    println!("  CGKA operations generated and applied");
    
    Ok(())
}

async fn show_pending_operations(config: &Config) -> anyhow::Result<()> {
    info!("Showing pending network operations");
    
    let agent = create_agent(config).await?;
    
    let pending_count = agent.transport.pending_messages_count().await;
    
    println!("Pending Network Operations:");
    println!("==========================");
    println!("Pending Messages: {}", pending_count);
    
    if pending_count > 0 {
        println!("\nUse 'aura network stats' for more detailed information");
    }
    
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
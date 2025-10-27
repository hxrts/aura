// Capability-driven storage commands

use crate::commands::common;
use crate::config::Config;
use anyhow::Context;
use aura_journal::capability::identity::IndividualId;
use clap::Subcommand;
use std::collections::BTreeSet;
use std::path::PathBuf;
use tokio::fs;
use tracing::info;

#[derive(Subcommand)]
pub enum StorageCommand {
    /// Store data with capability protection
    Store {
        /// Entry identifier
        entry_id: String,

        /// File path to store
        file_path: PathBuf,

        /// Required capability scope (namespace:operation)
        #[arg(long)]
        scope: String,

        /// Optional resource constraint
        #[arg(long)]
        resource: Option<String>,

        /// Content type/MIME type
        #[arg(long, default_value = "application/octet-stream")]
        content_type: String,

        /// Access control list (comma-separated individual IDs)
        #[arg(long)]
        acl: Option<String>,

        /// Custom attributes (key=value,key2=value2)
        #[arg(long)]
        attributes: Option<String>,
    },

    /// Retrieve data from storage
    Retrieve {
        /// Entry identifier
        entry_id: String,

        /// Output file path (optional, defaults to stdout)
        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// Delete entry from storage
    Delete {
        /// Entry identifier
        entry_id: String,
    },

    /// List accessible entries
    List,

    /// Show entry metadata
    Metadata {
        /// Entry identifier
        entry_id: String,
    },

    /// Show storage statistics
    Stats,

    /// Audit storage access logs
    Audit {
        /// Number of recent accesses to show
        #[arg(long, default_value = "20")]
        limit: usize,
    },

    /// Cleanup old encryption keys
    Cleanup {
        /// Number of epochs to retain
        #[arg(long, default_value = "10")]
        retain_epochs: usize,
    },
}

pub async fn handle_storage_command(
    command: StorageCommand,
    config: &Config,
) -> anyhow::Result<()> {
    match command {
        StorageCommand::Store {
            entry_id,
            file_path,
            scope,
            resource,
            content_type,
            acl,
            attributes,
        } => {
            store_data(
                config,
                &entry_id,
                &file_path,
                &scope,
                resource.as_deref(),
                &content_type,
                acl.as_deref(),
                attributes.as_deref(),
            )
            .await
        }

        StorageCommand::Retrieve { entry_id, output } => {
            retrieve_data(config, &entry_id, output.as_ref()).await
        }

        StorageCommand::Delete { entry_id } => delete_data(config, &entry_id).await,

        StorageCommand::List => list_entries(config).await,

        StorageCommand::Metadata { entry_id } => show_metadata(config, &entry_id).await,

        StorageCommand::Stats => show_stats(config).await,

        StorageCommand::Audit { limit } => audit_storage(config, limit).await,

        StorageCommand::Cleanup { retain_epochs } => cleanup_storage(config, retain_epochs).await,
    }
}

async fn store_data(
    config: &Config,
    entry_id: &str,
    file_path: &PathBuf,
    scope: &str,
    resource: Option<&str>,
    content_type: &str,
    acl: Option<&str>,
    attributes: Option<&str>,
) -> anyhow::Result<()> {
    info!("Storing file {:?} as entry '{}'", file_path, entry_id);

    let agent = common::create_agent(config).await?;

    // Read file data
    let data = fs::read(file_path).await.context("Failed to read file")?;

    // Parse capability scope
    let required_scope = common::parse_capability_scope(scope, resource)?;

    // Parse ACL if provided
    let acl_set = if let Some(acl_str) = acl {
        let ids: BTreeSet<IndividualId> = acl_str
            .split(',')
            .map(|s| s.trim())
            .map(|s| IndividualId::new(s))
            .collect();
        Some(ids)
    } else {
        None
    };

    // Parse attributes if provided
    let attrs = if let Some(attr_str) = attributes {
        common::parse_attributes(attr_str)?
    } else {
        std::collections::BTreeMap::new()
    };

    // Store data
    agent
        .store(
            entry_id.to_string(),
            data.clone(),
            content_type.to_string(),
            required_scope.clone(),
            acl_set.clone(),
            attrs,
        )
        .await?;

    println!("[OK] Data stored successfully");
    println!("  Entry ID: {}", entry_id);
    println!("  Size: {} bytes", data.len());
    println!("  Content Type: {}", content_type);
    println!(
        "  Scope: {}:{}",
        required_scope.namespace, required_scope.operation
    );
    if let Some(resource) = &required_scope.resource {
        println!("  Resource: {}", resource);
    }
    if let Some(acl) = &acl_set {
        println!("  ACL: {} members", acl.len());
    }

    Ok(())
}

async fn retrieve_data(
    config: &Config,
    entry_id: &str,
    output: Option<&PathBuf>,
) -> anyhow::Result<()> {
    info!("Retrieving entry '{}'", entry_id);

    let agent = common::create_agent(config).await?;

    // Retrieve data
    let data = agent.retrieve(entry_id).await?;

    if let Some(output_path) = output {
        // Write to file
        fs::write(output_path, &data)
            .await
            .context("Failed to write output file")?;

        println!("[OK] Data retrieved and written to {:?}", output_path);
        println!("  Entry ID: {}", entry_id);
        println!("  Size: {} bytes", data.len());
    } else {
        // Write to stdout
        use std::io::{self, Write};
        io::stdout()
            .write_all(&data)
            .context("Failed to write to stdout")?;
    }

    Ok(())
}

async fn delete_data(config: &Config, entry_id: &str) -> anyhow::Result<()> {
    info!("Deleting entry '{}'", entry_id);

    let agent = common::create_agent(config).await?;

    // Delete entry
    agent
        .storage
        .delete(entry_id, &agent.capability_agent.effects)
        .await
        .context("Failed to delete entry")?;

    println!("[OK] Entry deleted successfully");
    println!("  Entry ID: {}", entry_id);

    Ok(())
}

async fn list_entries(config: &Config) -> anyhow::Result<()> {
    info!("Listing accessible entries");

    let agent = common::create_agent(config).await?;

    // List entries
    let entries = agent
        .storage
        .list_entries()
        .await
        .context("Failed to list entries")?;

    if entries.is_empty() {
        println!("No accessible entries found");
        return Ok(());
    }

    println!("Accessible Entries:");
    println!("==================");

    for entry_id in &entries {
        // Get metadata for each entry
        if let Ok(metadata) = agent.storage.get_metadata(entry_id).await {
            println!("• {}", entry_id);
            println!("  Size: {} bytes", metadata.size);
            println!("  Type: {}", metadata.content_type);
            println!(
                "  Created: {} by {}",
                metadata.created_at, metadata.created_by.0
            );
            if metadata.created_at != metadata.modified_at {
                println!(
                    "  Modified: {} by {}",
                    metadata.modified_at, metadata.modified_by.0
                );
            }
            if !metadata.attributes.is_empty() {
                println!("  Attributes:");
                for (key, value) in &metadata.attributes {
                    println!("    {}: {}", key, value);
                }
            }
            println!();
        } else {
            println!("• {} (metadata inaccessible)", entry_id);
        }
    }

    println!("Total: {} entries", entries.len());

    Ok(())
}

async fn show_metadata(config: &Config, entry_id: &str) -> anyhow::Result<()> {
    info!("Showing metadata for entry '{}'", entry_id);

    let agent = common::create_agent(config).await?;

    // Get metadata
    let metadata = agent
        .storage
        .get_metadata(entry_id)
        .await
        .context("Failed to get metadata")?;

    println!("Entry Metadata:");
    println!("===============");
    println!("Entry ID: {}", entry_id);
    println!("Size: {} bytes", metadata.size);
    println!("Content Type: {}", metadata.content_type);
    println!(
        "Created: {} by {}",
        metadata.created_at, metadata.created_by.0
    );
    println!(
        "Modified: {} by {}",
        metadata.modified_at, metadata.modified_by.0
    );
    println!("Content Hash: {}", hex::encode(metadata.content_hash));

    if !metadata.attributes.is_empty() {
        println!("Attributes:");
        for (key, value) in &metadata.attributes {
            println!("  {}: {}", key, value);
        }
    }

    Ok(())
}

async fn show_stats(config: &Config) -> anyhow::Result<()> {
    info!("Showing storage statistics");

    let agent = common::create_agent(config).await?;

    // Get storage stats
    let stats = agent.get_storage_stats().await?;

    println!("Storage Statistics:");
    println!("==================");
    println!("Total Entries: {}", stats.total_entries);
    println!("Accessible Entries: {}", stats.accessible_entries);

    // Get network stats too
    let network_stats = agent.get_network_stats().await;
    println!("\nNetwork Statistics:");
    println!("==================");
    println!("Connected Peers: {}", network_stats.connected_peers);
    println!("Pending Messages: {}", network_stats.pending_messages);

    Ok(())
}

async fn audit_storage(config: &Config, limit: usize) -> anyhow::Result<()> {
    info!("Auditing storage access (limit: {})", limit);

    let agent = common::create_agent(config).await?;

    // Get access logs
    let logs = agent.storage.get_access_logs().await;

    if logs.is_empty() {
        println!("No access logs found");
        return Ok(());
    }

    println!("Recent Storage Access (limit: {}):", limit);
    println!("=================================");

    for log_entry in logs.iter().rev().take(limit) {
        println!(
            "{} | {:?} | {} | {} | {}:{}",
            log_entry.timestamp,
            log_entry.access_type,
            log_entry.entry_id,
            log_entry.individual_id.0,
            log_entry.scope.namespace,
            log_entry.scope.operation
        );
    }

    Ok(())
}

async fn cleanup_storage(config: &Config, retain_epochs: usize) -> anyhow::Result<()> {
    info!("Cleaning up storage (retain {} epochs)", retain_epochs);

    let agent = common::create_agent(config).await?;

    // Run cleanup
    agent.cleanup().await;

    println!("[OK] Storage cleanup complete");
    println!("  Retained {} epochs of encryption keys", retain_epochs);

    Ok(())
}

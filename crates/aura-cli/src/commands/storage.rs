// Capability-driven storage commands

use crate::{config::Config, commands::common};
use anyhow::Context;
use aura_journal::{AccountLedger, types::StorageMetadata};
use aura_store::{ChunkStore, Result as StorageResult};
use aura_types::{content::ChunkId, identifiers::AccountId, time_utils};
use clap::Subcommand;
use std::{collections::HashMap, fs, path::PathBuf};
use tracing::info;

/// Storage management commands
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

/// Handle storage subcommands
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
    common::validate_config(config)?;
    
    info!("Storing data from file: {}", file_path.display());
    
    // Read file data
    let data = fs::read(file_path)
        .with_context(|| format!("Failed to read file: {}", file_path.display()))?;
    
    // Parse ACL and attributes
    let acl_list = if let Some(acl_str) = acl {
        common::parse_peer_list(acl_str)
    } else {
        vec![]
    };
    
    let parsed_attributes = if let Some(attr_str) = attributes {
        common::parse_attributes(attr_str)?
    } else {
        HashMap::new()
    };
    
    // Parse scope
    let (namespace, operation) = common::parse_operation_scope(scope)?;
    
    // Initialize store
    let store_path = common::get_storage_path(config);
    fs::create_dir_all(&store_path)?;
    let store = Store::new(store_path)?;
    
    // Create storage entry
    let entry = StorageEntry {
        id: entry_id.to_string(),
        content_type: content_type.to_string(),
        data,
        metadata: parsed_attributes,
        access_control: acl_list,
        created_at: time_utils::current_unix_timestamp_millis(),
        updated_at: time_utils::current_unix_timestamp_millis(),
    };
    
    // Store the data
    let chunk_id = store.store_entry(&entry).await
        .context("Failed to store data in storage system")?;
    
    println!("✓ Data stored successfully");
    println!("  Entry ID: {}", entry_id);
    println!("  Chunk ID: {}", chunk_id);
    println!("  File: {}", file_path.display());
    println!("  Size: {}", common::format_file_size(entry.data.len() as u64));
    println!("  Content Type: {}", content_type);
    println!("  Scope: {}:{}", namespace, operation);
    if let Some(res) = resource {
        println!("  Resource: {}", res);
    }
    if !acl_list.is_empty() {
        println!("  ACL: {} devices", acl_list.len());
    }
    if !parsed_attributes.is_empty() {
        println!("  Attributes: {} items", parsed_attributes.len());
    }
    
    Ok(())
}

async fn retrieve_data(
    config: &Config,
    entry_id: &str,
    output: Option<&PathBuf>,
) -> anyhow::Result<()> {
    common::validate_config(config)?;
    
    info!("Retrieving data for entry: {}", entry_id);
    
    // Initialize store
    let store_path = common::get_storage_path(config);
    let store = Store::new(store_path)?;
    
    // Retrieve the entry
    let entry = store.retrieve_entry(entry_id).await
        .context("Failed to retrieve data from storage system")?
        .ok_or_else(|| anyhow::anyhow!("Entry not found: {}", entry_id))?;
    
    // Output the data
    match output {
        Some(output_path) => {
            fs::write(output_path, &entry.data)
                .with_context(|| format!("Failed to write to file: {}", output_path.display()))?;
            
            println!("✓ Data retrieved successfully");
            println!("  Entry ID: {}", entry_id);
            println!("  Output File: {}", output_path.display());
            println!("  Size: {}", common::format_file_size(entry.data.len() as u64));
            println!("  Content Type: {}", entry.content_type);
            println!("  Created: {}", common::format_timestamp(entry.created_at));
            println!("  Updated: {}", common::format_timestamp(entry.updated_at));
        }
        None => {
            // Output to stdout
            use std::io::Write;
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            handle.write_all(&entry.data)
                .context("Failed to write data to stdout")?;
        }
    }
    
    Ok(())
}

async fn delete_data(config: &Config, entry_id: &str) -> anyhow::Result<()> {
    common::validate_config(config)?;
    
    info!("Deleting entry '{}'", entry_id);
    
    // Initialize store
    let store_path = common::get_storage_path(config);
    let store = Store::new(store_path)?;
    
    // Delete the entry
    let deleted = store.delete_entry(entry_id).await
        .context("Failed to delete data from storage system")?;
    
    if deleted {
        println!("✓ Entry deleted successfully");
        println!("  Entry ID: {}", entry_id);
    } else {
        println!("⚠ Entry not found: {}", entry_id);
    }
    
    Ok(())
}

async fn list_entries(config: &Config) -> anyhow::Result<()> {
    common::validate_config(config)?;
    
    info!("Listing accessible entries");
    
    // Initialize store
    let store_path = common::get_storage_path(config);
    let store = Store::new(store_path)?;
    
    // List all entries
    let entries = store.list_entries().await
        .context("Failed to list entries from storage system")?;
    
    if entries.is_empty() {
        println!("No entries found in storage");
        return Ok(());
    }
    
    println!("Storage Entries ({} total):", entries.len());
    println!();
    
    for entry in entries {
        println!("Entry ID: {}", entry.id);
        println!("  Content Type: {}", entry.content_type);
        println!("  Size: {}", common::format_file_size(entry.data.len() as u64));
        println!("  Created: {}", common::format_timestamp(entry.created_at));
        println!("  Updated: {}", common::format_timestamp(entry.updated_at));
        
        if !entry.access_control.is_empty() {
            println!("  ACL: {} devices", entry.access_control.len());
        }
        
        if !entry.metadata.is_empty() {
            println!("  Attributes: {}", entry.metadata.len());
            for (key, value) in &entry.metadata {
                println!("    {}: {}", key, value);
            }
        }
        
        println!();
    }
    
    Ok(())
}

async fn show_metadata(config: &Config, entry_id: &str) -> anyhow::Result<()> {
    common::validate_config(config)?;
    
    info!("Showing metadata for entry '{}'", entry_id);
    
    // Initialize store
    let store_path = common::get_storage_path(config);
    let store = Store::new(store_path)?;
    
    // Retrieve the entry
    let entry = store.retrieve_entry(entry_id).await
        .context("Failed to retrieve entry from storage system")?
        .ok_or_else(|| anyhow::anyhow!("Entry not found: {}", entry_id))?;
    
    println!("Entry Metadata:");
    println!("  Entry ID: {}", entry.id);
    println!("  Content Type: {}", entry.content_type);
    println!("  Size: {}", common::format_file_size(entry.data.len() as u64));
    println!("  Created: {}", common::format_timestamp(entry.created_at));
    println!("  Updated: {}", common::format_timestamp(entry.updated_at));
    
    if !entry.access_control.is_empty() {
        println!("  Access Control List:");
        for device_id in &entry.access_control {
            println!("    - {}", device_id);
        }
    } else {
        println!("  Access Control: None (public)");
    }
    
    if !entry.metadata.is_empty() {
        println!("  Custom Attributes:");
        for (key, value) in &entry.metadata {
            println!("    {}: {}", key, value);
        }
    } else {
        println!("  Custom Attributes: None");
    }
    
    Ok(())
}

async fn show_stats(config: &Config) -> anyhow::Result<()> {
    common::validate_config(config)?;
    
    info!("Showing storage statistics");
    
    // Initialize store
    let store_path = common::get_storage_path(config);
    let store = Store::new(store_path)?;
    
    // Get all entries for statistics
    let entries = store.list_entries().await
        .context("Failed to list entries from storage system")?;
    
    // Calculate statistics
    let total_entries = entries.len();
    let total_size: u64 = entries.iter().map(|e| e.data.len() as u64).sum();
    let avg_size = if total_entries > 0 { total_size / total_entries as u64 } else { 0 };
    
    // Content type distribution
    let mut content_types = HashMap::new();
    for entry in &entries {
        *content_types.entry(entry.content_type.clone()).or_insert(0) += 1;
    }
    
    // Size distribution
    let mut size_buckets = [0u32; 5]; // <1KB, 1KB-10KB, 10KB-100KB, 100KB-1MB, >1MB
    for entry in &entries {
        let size = entry.data.len();
        if size < 1024 {
            size_buckets[0] += 1;
        } else if size < 10 * 1024 {
            size_buckets[1] += 1;
        } else if size < 100 * 1024 {
            size_buckets[2] += 1;
        } else if size < 1024 * 1024 {
            size_buckets[3] += 1;
        } else {
            size_buckets[4] += 1;
        }
    }
    
    println!("Storage Statistics:");
    println!("  Total Entries: {}", total_entries);
    println!("  Total Size: {}", common::format_file_size(total_size));
    if total_entries > 0 {
        println!("  Average Size: {}", common::format_file_size(avg_size));
    }
    println!("  Storage Path: {}", store_path.display());
    
    if !content_types.is_empty() {
        println!("\nContent Type Distribution:");
        for (content_type, count) in content_types {
            println!("  {}: {} entries", content_type, count);
        }
    }
    
    if total_entries > 0 {
        println!("\nSize Distribution:");
        println!("  < 1 KB: {} entries", size_buckets[0]);
        println!("  1 KB - 10 KB: {} entries", size_buckets[1]);
        println!("  10 KB - 100 KB: {} entries", size_buckets[2]);
        println!("  100 KB - 1 MB: {} entries", size_buckets[3]);
        println!("  > 1 MB: {} entries", size_buckets[4]);
    }
    
    Ok(())
}

async fn audit_storage(config: &Config, limit: usize) -> anyhow::Result<()> {
    common::validate_config(config)?;
    
    info!("Auditing storage access (limit: {})", limit);
    
    // Initialize store  
    let store_path = common::get_storage_path(config);
    let store = Store::new(store_path)?;
    
    // Get audit logs from the store
    let audit_logs = store.get_audit_logs(limit).await
        .context("Failed to retrieve audit logs from storage system")?;
    
    if audit_logs.is_empty() {
        println!("No audit logs found");
        return Ok(());
    }
    
    println!("Storage Audit Log (showing {} most recent entries):", audit_logs.len());
    println!();
    
    for log_entry in audit_logs {
        println!("Timestamp: {}", common::format_timestamp(log_entry.timestamp));
        println!("  Operation: {}", log_entry.operation);
        println!("  Entry ID: {}", log_entry.entry_id);
        if let Some(device_id) = log_entry.device_id {
            println!("  Device: {}", device_id);
        }
        if let Some(size) = log_entry.size {
            println!("  Size: {}", common::format_file_size(size));
        }
        if !log_entry.metadata.is_empty() {
            println!("  Metadata:");
            for (key, value) in &log_entry.metadata {
                println!("    {}: {}", key, value);
            }
        }
        println!();
    }
    
    Ok(())
}

async fn cleanup_storage(config: &Config, retain_epochs: usize) -> anyhow::Result<()> {
    common::validate_config(config)?;
    
    info!("Cleaning up storage (retain {} epochs)", retain_epochs);
    
    // Initialize store
    let store_path = common::get_storage_path(config);
    let store = Store::new(store_path)?;
    
    // Perform cleanup operation
    let cleanup_result = store.cleanup_old_data(retain_epochs).await
        .context("Failed to cleanup storage")?;
    
    println!("✓ Storage cleanup completed");
    println!("  Retained Epochs: {}", retain_epochs);
    println!("  Cleaned Entries: {}", cleanup_result.cleaned_entries);
    println!("  Reclaimed Space: {}", common::format_file_size(cleanup_result.reclaimed_bytes));
    println!("  Remaining Entries: {}", cleanup_result.remaining_entries);
    
    if cleanup_result.cleaned_entries == 0 {
        println!("  No old data found to clean");
    }
    
    Ok(())
}

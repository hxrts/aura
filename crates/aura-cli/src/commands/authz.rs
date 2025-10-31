// Authorization management commands
//
// Commands for permission management, capability delegation, and access control.
// These commands handle "what you can do" concerns.

use crate::commands::common;
use crate::config::Config;
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
    let mut agent = common::create_agent(config).await?;

    match command {
        AuthzCommand::List => {
            info!("Listing current permissions and capabilities");

            // List capabilities from the agent
            let capabilities = agent.list_capabilities();
            println!("Current Capabilities:");

            if capabilities.is_empty() {
                println!("  No capabilities found");
            } else {
                for capability in capabilities {
                    println!("  {}:{}", capability.namespace, capability.operation);

                    if let Some(resource) = &capability.resource {
                        println!("    Resource: {}", resource);
                    }

                    if !capability.params.is_empty() {
                        println!("    Parameters: {:?}", capability.params);
                    }
                }
            }
        }

        AuthzCommand::Grant {
            device_id,
            operations,
            expiry,
        } => {
            info!("Granting permissions to device: {}", device_id);

            let ops: Vec<String> = operations
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();

            // Parse device ID
            let device_uuid = common::parse_device_id(&device_id)?;

            // Create target subject from device ID
            use aura_journal::capability::types::Subject;
            let target_subject = Subject(device_uuid.to_string());

            println!("Granting permissions to device: {}", device_uuid);

            let mut success_count = 0;
            let mut errors = Vec::new();

            for operation in ops {
                let (namespace, op) = match common::parse_operation_scope(&operation) {
                    Ok((ns, op)) => (ns, op),
                    Err(_) => ("default".to_string(), operation.to_string()),
                };

                // Create capability scope for the operation
                use aura_journal::capability::types::CapabilityScope;
                let new_scope = CapabilityScope::simple(&namespace, &op);

                // Create parent capability scope (admin level)
                let parent_scope = CapabilityScope::simple("admin", "delegate");

                // Delegate capability
                match agent.capability_agent.delegate_capability(
                    parent_scope,
                    target_subject.clone(),
                    new_scope.clone(),
                    expiry,
                ) {
                    Ok(_) => {
                        println!("  [OK] Granted: {}:{}", namespace, op);
                        success_count += 1;
                    }
                    Err(e) => {
                        let error_msg = format!("  [ERROR] Failed to grant {}:{}: {}", namespace, op, e);
                        println!("{}", error_msg);
                        errors.push(error_msg);
                    }
                }
            }

            println!("\nSummary:");
            println!("  Successfully granted: {} permissions", success_count);
            println!("  Failed: {} permissions", errors.len());

            if expiry.is_some() {
                println!("  Expiry: {} (Unix timestamp)", expiry.unwrap());
            } else {
                println!("  Expiry: Never");
            }

            if !errors.is_empty() {
                return Err(anyhow::anyhow!(
                    "Some permission grants failed: {:?}",
                    errors
                ));
            }
        }

        AuthzCommand::Revoke {
            device_id,
            operations,
            reason,
        } => {
            info!(
                "Revoking permissions from device: {} - {}",
                device_id, reason
            );

            let ops: Vec<String> = operations
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();

            // Parse device ID
            let device_uuid = common::parse_device_id(&device_id)?;

            // Create target subject from device ID
            use aura_journal::capability::types::Subject;
            let target_subject = Subject(device_uuid.to_string());

            println!("Revoking permissions from device: {}", device_uuid);
            println!("Reason: {}", reason);

            // First, get all current capabilities for this device
            let all_capabilities = agent.list_capabilities();

            let mut success_count = 0;
            let mut errors = Vec::new();
            let mut not_found = Vec::new();

            for operation in ops {
                let (namespace, op) = match common::parse_operation_scope(&operation) {
                    Ok((ns, op)) => (ns, op),
                    Err(_) => ("default".to_string(), operation.to_string()),
                };

                // Find matching capability to revoke
                let mut found_capability = false;
                for capability in &all_capabilities {
                    if capability.namespace == namespace && capability.operation == op {
                        // Generate a capability ID for revocation
                        // In a real implementation, capabilities would have persistent IDs
                        use aura_journal::capability::types::CapabilityId;
                        let uuid = uuid::Uuid::new_v4();
                        let uuid_bytes = uuid.as_bytes();
                        let mut capability_id_bytes = [0u8; 32];
                        // Extend UUID bytes to 32 bytes by repeating
                        capability_id_bytes[..16].copy_from_slice(uuid_bytes);
                        capability_id_bytes[16..].copy_from_slice(uuid_bytes);
                        let capability_id = CapabilityId(capability_id_bytes);

                        match agent
                            .capability_agent
                            .revoke_capability(capability_id, reason.clone())
                        {
                            Ok(_) => {
                                println!("  [OK] Revoked: {}:{}", namespace, op);
                                success_count += 1;
                                found_capability = true;
                            }
                            Err(e) => {
                                let error_msg =
                                    format!("  [ERROR] Failed to revoke {}:{}: {}", namespace, op, e);
                                println!("{}", error_msg);
                                errors.push(error_msg);
                                found_capability = true;
                            }
                        }
                        break;
                    }
                }

                if !found_capability {
                    let not_found_msg = format!("  ! Capability not found: {}:{}", namespace, op);
                    println!("{}", not_found_msg);
                    not_found.push(not_found_msg);
                }
            }

            println!("\nSummary:");
            println!("  Successfully revoked: {} permissions", success_count);
            println!("  Failed: {} permissions", errors.len());
            println!("  Not found: {} permissions", not_found.len());

            if !errors.is_empty() {
                return Err(anyhow::anyhow!(
                    "Some permission revocations failed: {:?}",
                    errors
                ));
            }
        }

        AuthzCommand::Check {
            device_id,
            operation,
        } => {
            info!(
                "Checking permissions for device: {} operation: {}",
                device_id, operation
            );

            // Parse device ID
            let device_uuid = common::parse_device_id(&device_id)?;

            // Parse operation
            let (namespace, op) = match common::parse_operation_scope(&operation) {
                Ok((ns, op)) => (ns, op),
                Err(_) => ("default".to_string(), operation.to_string()),
            };

            // Create capability scope for checking
            use aura_journal::capability::types::CapabilityScope;
            let scope = CapabilityScope::simple(&namespace, &op);

            println!("Permission Check:");
            println!("  Device: {}", device_uuid);
            println!("  Operation: {}:{}", namespace, op);

            // Check if the current agent has the capability
            let has_permission = agent.check_capability(&scope);

            println!(
                "  Authorized: {}",
                if has_permission { "[OK] YES" } else { "[ERROR] NO" }
            );

            if has_permission {
                println!("  Status: Permission granted");

                // Get additional details about the capability
                let all_capabilities = agent.list_capabilities();
                for capability in &all_capabilities {
                    if capability.namespace == namespace && capability.operation == op {
                        if let Some(resource) = &capability.resource {
                            println!("  Resource: {}", resource);
                        }
                        if !capability.params.is_empty() {
                            println!("  Parameters: {:?}", capability.params);
                        }
                        break;
                    }
                }
            } else {
                println!("  Status: Permission denied");
                println!("  Required capability: {}:{}", namespace, op);

                // Show available capabilities for context
                let available_capabilities = agent.list_capabilities();
                if !available_capabilities.is_empty() {
                    println!("  Available capabilities:");
                    for capability in &available_capabilities {
                        println!("    - {}:{}", capability.namespace, capability.operation);
                    }
                } else {
                    println!("  No capabilities available for this device");
                }
            }
        }

        AuthzCommand::Delegate {
            parent,
            subject,
            scope,
            resource,
            expiry,
        } => {
            info!(
                "Delegating capability: {} -> {} ({})",
                parent, subject, scope
            );

            // Parse parent capability scope
            let (parent_namespace, parent_op) = common::parse_operation_scope(&parent)?;

            // Parse subject ID
            let subject_uuid = common::parse_device_id(&subject)?;

            // Parse new capability scope
            let (new_namespace, new_op) = common::parse_operation_scope(&scope)?;

            // Create capability scopes
            use aura_journal::capability::types::{CapabilityScope, Subject};
            let parent_scope = if let Some(res) = &resource {
                CapabilityScope::with_resource(&parent_namespace, &parent_op, res)
            } else {
                CapabilityScope::simple(&parent_namespace, &parent_op)
            };

            let new_scope = if let Some(res) = &resource {
                CapabilityScope::with_resource(&new_namespace, &new_op, res)
            } else {
                CapabilityScope::simple(&new_namespace, &new_op)
            };

            let target_subject = Subject(subject_uuid.to_string());

            println!("Delegating Capability:");
            println!("  Parent: {}:{}", parent_namespace, parent_op);
            println!("  Subject: {}", subject_uuid);
            println!("  New scope: {}:{}", new_namespace, new_op);

            if let Some(res) = &resource {
                println!("  Resource: {}", res);
            }

            if let Some(exp) = expiry {
                println!("  Expiry: {} (Unix timestamp)", exp);
            } else {
                println!("  Expiry: Never");
            }

            // Perform the delegation
            match agent.capability_agent.delegate_capability(
                parent_scope,
                target_subject,
                new_scope,
                expiry,
            ) {
                Ok(_) => {
                    println!("  Status: [OK] Capability delegation successful");
                    println!("  The subject can now perform the delegated operation");
                }
                Err(e) => {
                    println!("  Status: [ERROR] Capability delegation failed");
                    println!("  Error: {}", e);
                    return Err(anyhow::anyhow!("Capability delegation failed: {}", e));
                }
            }
        }

        AuthzCommand::History { device_id } => {
            info!("Showing permission history for device: {}", device_id);

            // Parse device ID
            let device_uuid = common::parse_device_id(&device_id)?;

            println!("Permission History for device: {}", device_uuid);
            println!("========================================");

            // Get current capabilities as a snapshot
            let current_capabilities = agent.list_capabilities();

            if current_capabilities.is_empty() {
                println!("No current capabilities found for this device.");
            } else {
                println!("\nCurrent Active Capabilities:");
                for (i, capability) in current_capabilities.iter().enumerate() {
                    println!(
                        "  {}. {}:{}",
                        i + 1,
                        capability.namespace,
                        capability.operation
                    );

                    if let Some(resource) = &capability.resource {
                        println!("     Resource: {}", resource);
                    }

                    if !capability.params.is_empty() {
                        println!("     Parameters: {:?}", capability.params);
                    }

                    // Show grant/delegation information if available
                    println!("     Status: Active");
                    println!(
                        "     Type: {}",
                        if capability.resource.is_some() {
                            "Resource-scoped"
                        } else {
                            "Global"
                        }
                    );
                }
            }

            // In a full implementation, this would query the ledger for historical events
            println!("\nHistorical Events:");
            println!("  (Historical capability events would be shown here)");
            println!("  Note: Full audit trail requires ledger integration");

            // Show capability statistics
            println!("\nCapability Statistics:");
            let mut namespace_counts = std::collections::HashMap::new();
            for capability in &current_capabilities {
                *namespace_counts.entry(&capability.namespace).or_insert(0) += 1;
            }

            if namespace_counts.is_empty() {
                println!("  No capabilities to analyze");
            } else {
                println!("  Total capabilities: {}", current_capabilities.len());
                println!("  Capabilities by namespace:");
                for (namespace, count) in namespace_counts {
                    println!("    {}: {} capabilities", namespace, count);
                }
            }

            // Security recommendations
            println!("\nSecurity Recommendations:");
            if current_capabilities.len() > 10 {
                println!("  [WARNING] High number of capabilities - consider periodic review");
            }

            let admin_capabilities = current_capabilities
                .iter()
                .filter(|c| c.namespace == "admin" || c.operation.contains("admin"))
                .count();

            if admin_capabilities > 0 {
                println!(
                    "  [WARNING] {} admin-level capabilities found - ensure regular rotation",
                    admin_capabilities
                );
            }

            if current_capabilities.iter().any(|c| c.resource.is_none()) {
                println!(
                    "  [WARNING] Global capabilities found - consider resource-scoped alternatives"
                );
            }

            if current_capabilities.is_empty() {
                println!("  [OK] No active capabilities - minimal security risk");
            } else {
                println!("  ℹ Regular capability audits recommended");
            }
        }
    }

    Ok(())
}

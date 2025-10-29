//! Capability management operations
//!
//! Handles delegation and revocation of capabilities via network.

#![allow(dead_code)]

use crate::commands::common;
use crate::config::Config;
use anyhow::Context;
use tracing::info;

/// Delegate a capability to a peer via network
pub async fn delegate_capability(
    _config: &Config,
    parent: &str,
    subject: &str,
    scope: &str,
    resource: Option<&str>,
    peers: Option<&str>,
    expiry: Option<u64>,
) -> anyhow::Result<()> {
    info!("Delegating capability {} to {} via network", scope, subject);

    // Parse capability scope for display
    let new_scope = common::parse_capability_scope(scope, resource)?;

    println!("[WARN] Capability delegation not yet implemented in Agent trait");
    println!("  Parent: {}", parent);
    println!("  Subject: {}", subject);
    println!("  Scope: {}", new_scope);
    if let Some(peer_str) = peers {
        let peer_count = common::parse_peer_list(peer_str).len();
        println!("  Peers: {} specified", peer_count);
    }
    if let Some(exp) = expiry {
        println!("  Expiry: {}", exp);
    }

    Ok(())
}

/// Revoke a capability via network
/// Revoke a previously delegated capability
pub async fn revoke_capability(
    _config: &Config,
    capability_id: &str,
    reason: &str,
    peers: Option<&str>,
) -> anyhow::Result<()> {
    info!(
        "Revoking capability {} via network: {}",
        capability_id, reason
    );

    // Validate capability ID format
    let cap_id_bytes = hex::decode(capability_id).context("Invalid capability ID hex format")?;
    if cap_id_bytes.len() != 32 {
        return Err(anyhow::anyhow!(
            "Capability ID must be 32 bytes (64 hex characters)"
        ));
    }

    println!("[WARN] Capability revocation not yet implemented in Agent trait");
    println!("  Capability ID: {}", capability_id);
    println!("  Reason: {}", reason);
    if let Some(peer_str) = peers {
        let peer_count = common::parse_peer_list(peer_str).len();
        println!("  Peers: {} specified", peer_count);
    }

    Ok(())
}

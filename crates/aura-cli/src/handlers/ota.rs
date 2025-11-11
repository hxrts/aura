//! OTA Upgrade CLI Commands
//!
//! Commands for managing over-the-air upgrades including proposal submission,
//! status checking, and upgrade orchestration.

use anyhow::{Result, Context};
use aura_agent::{OtaOrchestrator, UpgradeProposal, UpgradeType, OptInPolicy, SecuritySeverity};
use aura_core::DeviceId;
use aura_protocol::{AuraEffectSystem, ConsoleEffects};
use serde::{Serialize, Deserialize};
use std::time::SystemTime;
use uuid::Uuid;

/// OTA action types for CLI commands
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OtaAction {
    /// Submit new upgrade proposal
    ProposeUpgrade {
        from_version: String,
        to_version: String,
        upgrade_type: String, // "soft", "hard", or "security"
        download_url: String,
        description: String,
    },
    /// Set user opt-in policy
    SetPolicy {
        policy: String, // "auto", "manual", "security", "soft-auto"
    },
    /// Get upgrade status
    Status,
    /// Opt into specific upgrade
    OptIn {
        proposal_id: String,
    },
    /// List all proposals
    ListProposals,
    /// Get upgrade statistics
    Stats,
}

/// Handle OTA commands through effects
pub async fn handle_ota(effects: &AuraEffectSystem, action: &OtaAction) -> Result<()> {
    let device_id = DeviceId::new(); // In production, this would come from config
    let orchestrator = OtaOrchestrator::new("1.0.0".to_string()); // In production, this would be actual version

    match action {
        OtaAction::ProposeUpgrade { 
            from_version, 
            to_version, 
            upgrade_type, 
            download_url, 
            description 
        } => {
            handle_propose_upgrade(
                effects, 
                &orchestrator, 
                from_version, 
                to_version, 
                upgrade_type, 
                download_url, 
                description
            ).await
        }
        OtaAction::SetPolicy { policy } => {
            handle_set_policy(effects, &orchestrator, policy).await
        }
        OtaAction::Status => {
            handle_status(effects, &orchestrator, &device_id).await
        }
        OtaAction::OptIn { proposal_id } => {
            handle_opt_in(effects, &orchestrator, proposal_id, &device_id).await
        }
        OtaAction::ListProposals => {
            handle_list_proposals(effects, &orchestrator).await
        }
        OtaAction::Stats => {
            handle_stats(effects, &orchestrator).await
        }
    }
}

/// Handle upgrade proposal submission
async fn handle_propose_upgrade(
    effects: &AuraEffectSystem,
    orchestrator: &OtaOrchestrator,
    from_version: &str,
    to_version: &str,
    upgrade_type_str: &str,
    download_url: &str,
    description: &str,
) -> Result<()> {
    let upgrade_type = parse_upgrade_type(upgrade_type_str, from_version, to_version)?;
    
    let proposal = UpgradeProposal {
        id: Uuid::new_v4(),
        upgrade_type,
        from_version: from_version.to_string(),
        to_version: to_version.to_string(),
        description: description.to_string(),
        changelog_url: None,
        download_url: download_url.to_string(),
        checksum: [0u8; 32], // In production, this would be calculated
        signature: vec![0u8; 64], // In production, this would be a real signature
        proposed_at: SystemTime::now(),
        proposed_by: DeviceId::new(),
    };

    orchestrator.submit_proposal(proposal.clone()).await
        .context("Failed to submit upgrade proposal")?;

    effects.log_info(
        &format!("✅ Submitted upgrade proposal: {} -> {} ({})", 
                from_version, to_version, proposal.id), 
        &[]
    );

    Ok(())
}

/// Handle policy setting
async fn handle_set_policy(
    effects: &AuraEffectSystem,
    orchestrator: &OtaOrchestrator,
    policy_str: &str,
) -> Result<()> {
    let policy = parse_policy(policy_str)?;
    
    orchestrator.set_opt_in_policy(policy.clone()).await;
    
    effects.log_info(
        &format!("✅ Set OTA opt-in policy: {:?}", policy), 
        &[]
    );

    Ok(())
}

/// Handle status checking
async fn handle_status(
    effects: &AuraEffectSystem,
    orchestrator: &OtaOrchestrator,
    device_id: &DeviceId,
) -> Result<()> {
    effects.log_info("📊 OTA Status:", &[]);
    effects.log_info(&format!("   Current version: {}", orchestrator.current_version()), &[]);
    
    let policy = orchestrator.get_opt_in_policy().await;
    effects.log_info(&format!("   Opt-in policy: {:?}", policy), &[]);
    
    if let Some(status) = orchestrator.get_adoption_status(device_id).await {
        effects.log_info(
            &format!("   Adoption status: {:?} (target: {})", 
                    status.status, status.target_version), 
            &[]
        );
        
        if let Some(error) = &status.error_message {
            effects.log_error(&format!("   Error: {}", error), &[]);
        }
    } else {
        effects.log_info("   No active upgrade", &[]);
    }

    Ok(())
}

/// Handle opt-in to upgrade
async fn handle_opt_in(
    effects: &AuraEffectSystem,
    orchestrator: &OtaOrchestrator,
    proposal_id_str: &str,
    device_id: &DeviceId,
) -> Result<()> {
    let proposal_id = Uuid::parse_str(proposal_id_str)
        .context("Invalid proposal ID format")?;
    
    orchestrator.opt_in_to_upgrade(proposal_id, *device_id).await
        .context("Failed to opt into upgrade")?;
    
    effects.log_info(
        &format!("✅ Opted into upgrade: {}", proposal_id), 
        &[]
    );

    Ok(())
}

/// Handle listing proposals
async fn handle_list_proposals(
    effects: &AuraEffectSystem,
    orchestrator: &OtaOrchestrator,
) -> Result<()> {
    let proposals = orchestrator.get_proposals().await;
    
    if proposals.is_empty() {
        effects.log_info("📋 No upgrade proposals found", &[]);
        return Ok(());
    }

    effects.log_info(&format!("📋 Upgrade Proposals ({}):", proposals.len()), &[]);
    
    for proposal in &proposals {
        let type_str = format_upgrade_type(&proposal.upgrade_type);
        effects.log_info(
            &format!("   {} | {} -> {} | {}", 
                    proposal.id, proposal.from_version, proposal.to_version, type_str),
            &[]
        );
        effects.log_info(
            &format!("     Description: {}", proposal.description),
            &[]
        );
        effects.log_info(
            &format!("     Proposed by: {} at {:?}", 
                    proposal.proposed_by, proposal.proposed_at),
            &[]
        );
    }

    Ok(())
}

/// Handle upgrade statistics
async fn handle_stats(
    effects: &AuraEffectSystem,
    orchestrator: &OtaOrchestrator,
) -> Result<()> {
    let stats = orchestrator.get_upgrade_stats().await;
    
    effects.log_info("📈 Upgrade Statistics:", &[]);
    effects.log_info(&format!("   Active proposals: {}", stats.active_proposals), &[]);
    effects.log_info(&format!("   Total devices: {}", stats.total_devices), &[]);
    effects.log_info(&format!("   Completed upgrades: {}", stats.completed_upgrades), &[]);
    effects.log_info(&format!("   Failed upgrades: {}", stats.failed_upgrades), &[]);
    effects.log_info(&format!("   Success rate: {:.1}%", stats.success_rate * 100.0), &[]);

    Ok(())
}

/// Parse upgrade type from string
fn parse_upgrade_type(type_str: &str, from_version: &str, to_version: &str) -> Result<UpgradeType> {
    match type_str.to_lowercase().as_str() {
        "soft" => Ok(UpgradeType::SoftFork {
            min_version: from_version.to_string(),
            recommended_version: to_version.to_string(),
            deadline: None,
        }),
        "hard" => Ok(UpgradeType::HardFork {
            required_version: to_version.to_string(),
            activation_epoch: 1000, // In production, this would be calculated
            deadline: SystemTime::now() + std::time::Duration::from_secs(7 * 24 * 60 * 60), // 1 week
        }),
        "security" => Ok(UpgradeType::SecurityPatch {
            patch_version: to_version.to_string(),
            vulnerability_id: "CVE-2024-XXXX".to_string(), // In production, this would be real
            severity: SecuritySeverity::High,
        }),
        _ => Err(anyhow::anyhow!("Invalid upgrade type: {}. Use 'soft', 'hard', or 'security'", type_str)),
    }
}

/// Parse opt-in policy from string
fn parse_policy(policy_str: &str) -> Result<OptInPolicy> {
    match policy_str.to_lowercase().as_str() {
        "auto" | "automatic" => Ok(OptInPolicy::Automatic),
        "manual" => Ok(OptInPolicy::Manual),
        "security" => Ok(OptInPolicy::SecurityOnly),
        "soft-auto" => Ok(OptInPolicy::SoftForkAuto),
        _ => Err(anyhow::anyhow!("Invalid policy: {}. Use 'auto', 'manual', 'security', or 'soft-auto'", policy_str)),
    }
}

/// Format upgrade type for display
fn format_upgrade_type(upgrade_type: &UpgradeType) -> String {
    match upgrade_type {
        UpgradeType::SoftFork { .. } => "Soft Fork".to_string(),
        UpgradeType::HardFork { activation_epoch, .. } => format!("Hard Fork (epoch {})", activation_epoch),
        UpgradeType::SecurityPatch { severity, vulnerability_id, .. } => {
            format!("Security Patch ({:?}, {})", severity, vulnerability_id)
        },
    }
}
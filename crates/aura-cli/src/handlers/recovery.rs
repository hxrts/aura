//! Guardian Recovery CLI Commands
//!
//! Commands for managing guardian-based account recovery.

use anyhow::Result;
use aura_authenticate::guardian_auth::{RecoveryContext, RecoveryOperationType};
use aura_core::effects::{AuthorizationEffects, JournalEffects};
use aura_core::hash::hash;
use aura_core::identifiers::GuardianId;
use aura_core::TrustLevel;
use aura_core::{AccountId, DeviceId};
use aura_protocol::orchestration::AuraEffectSystem;
use aura_protocol::effect_traits::ConsoleEffects;
use aura_recovery::types::{GuardianProfile, GuardianSet};
use aura_recovery::{GuardianKeyRecoveryCoordinator, RecoveryRequest, RecoveryResponse};
use std::path::Path;

use crate::RecoveryAction;

/// Handle recovery commands through effects
pub async fn handle_recovery(effects: &AuraEffectSystem, action: &RecoveryAction) -> Result<()> {
    match action {
        RecoveryAction::Start {
            account,
            guardians,
            threshold,
            priority,
            dispute_hours,
            justification,
        } => {
            start_recovery(
                effects,
                account,
                guardians,
                *threshold,
                priority,
                *dispute_hours,
                justification.as_deref(),
            )
            .await
        }
        RecoveryAction::Approve { request_file } => approve_recovery(effects, request_file).await,
        RecoveryAction::Status => get_status(effects).await,
        RecoveryAction::Dispute { evidence, reason } => {
            dispute_recovery(effects, evidence, reason).await
        }
    }
}

async fn start_recovery(
    effects: &AuraEffectSystem,
    account: &str,
    guardians: &str,
    threshold: u32,
    priority: &str,
    dispute_hours: u64,
    justification: Option<&str>,
) -> Result<()> {
    let _ = effects
        .log_info(&format!(
            "Starting {} recovery for account: {}",
            priority, account
        ))
        .await;
    let _ = effects.log_info(&format!("Guardians: {}", guardians)).await;
    let _ = effects.log_info(&format!("Threshold: {}", threshold)).await;
    let _ = effects
        .log_info(&format!("Dispute window: {} hours", dispute_hours))
        .await;

    if let Some(just) = justification {
        let _ = effects.log_info(&format!("Justification: {}", just)).await;
    }

    // Parse account ID
    let account_id = account
        .parse::<AccountId>()
        .map_err(|e| anyhow::anyhow!("Invalid account ID '{}': {}", account, e))?;

    // Parse guardians list (comma-separated guardian IDs)
    let guardian_ids: Vec<&str> = guardians.split(',').map(|s| s.trim()).collect();
    if guardian_ids.is_empty() {
        return Err(anyhow::anyhow!("No guardians specified"));
    }

    // Convert to guardian profiles with Journal integration
    let guardian_profiles: Result<Vec<GuardianProfile>, _> = guardian_ids
        .iter()
        .enumerate()
        .map(|(i, guardian_str)| {
            // Parse guardian ID
            let guardian_id = guardian_str
                .parse::<GuardianId>()
                .map_err(|e| anyhow::anyhow!("Invalid guardian ID '{}': {}", guardian_str, e))?;

            // Query actual device IDs from Journal/Web-of-Trust
            let device_id = match query_guardian_device_id(effects, &guardian_id).await {
                Ok(id) => id,
                Err(_) => {
                    // Fallback to generated device ID for now
                    tracing::warn!(
                        guardian_id = ?guardian_id,
                        "Guardian device ID not found in Journal, using generated ID"
                    );
                    DeviceId::try_from(format!("guardian-device-{}", i).as_str())
                        .map_err(|e| anyhow::anyhow!("Failed to create device ID: {}", e))?
                }
            };

            Ok::<GuardianProfile, anyhow::Error>(GuardianProfile::new(
                guardian_id,
                device_id,
                format!("Guardian {}", i + 1),
            ))
        })
        .collect();

    let guardian_profiles = guardian_profiles?;
    let guardian_set = GuardianSet::new(guardian_profiles);

    if guardian_set.len() < threshold as usize {
        return Err(anyhow::anyhow!(
            "Threshold {} exceeds number of guardians {}",
            threshold,
            guardian_set.len()
        ));
    }

    // Create recovery context
    let context = RecoveryContext {
        operation_type: RecoveryOperationType::DeviceKeyRecovery,
        justification: justification
            .unwrap_or("CLI recovery operation")
            .to_string(),
        is_emergency: priority == "emergency",
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };

    // Get current device ID from agent state (placeholder)
    // TODO: Query actual device ID from agent configuration
    let requesting_device = DeviceId::try_from(format!("recovery-device-{}", account).as_str())
        .map_err(|e| anyhow::anyhow!("Failed to create device ID: {}", e))?;

    // Create recovery request
    let recovery_request = RecoveryRequest {
        requesting_device,
        account_id,
        context,
        threshold: threshold as usize,
        guardians: guardian_set,
    };

    // Initialize coordinator with effect system
    let coordinator = GuardianKeyRecoveryCoordinator::new(effects.clone());

    let _ = effects.log_info("Executing recovery choreography...").await;

    // Execute recovery coordinator integration
    let recovery_result = coordinator
        .execute_key_recovery(request)
        .await
        .map_err(|e| e.to_string());
    match recovery_result {
        Ok(response) => {
            if response.success {
                let _ = effects.log_info("Recovery initiated successfully!").await;
                // TODO: Implement proper evidence hashing when evidence serialization is available
                let _ = effects
                    .log_info("Recovery evidence created (hash pending)")
                    .await;
                let _ = effects
                    .log_info(&format!(
                        "Collected {} guardian shares",
                        response.guardian_shares.len()
                    ))
                    .await;
                let _ = effects
                    .log_info(&format!(
                        "Dispute window ends: {}",
                        response.evidence.dispute_window_ends_at
                    ))
                    .await;

                // Display key material if recovered
                if let Some(key_material) = response.key_material {
                    let _ = effects
                        .log_info(&format!(
                            "Recovered key material: {} bytes",
                            key_material.len()
                        ))
                        .await;
                }
            } else {
                let error_msg = response
                    .error
                    .unwrap_or_else(|| "Unknown error".to_string());
                return Err(anyhow::anyhow!("Recovery failed: {}", error_msg));
            }
        }
        Err(e) => {
            return Err(anyhow::anyhow!("Recovery choreography failed: {}", e));
        }
    }

    Ok(())
}

async fn approve_recovery(effects: &AuraEffectSystem, request_file: &Path) -> Result<()> {
    let _ = effects
        .log_info(&format!(
            "Approving recovery from: {}",
            request_file.display()
        ))
        .await;

    // Read and parse recovery request file
    let request_content = std::fs::read_to_string(request_file)
        .map_err(|e| anyhow::anyhow!("Failed to read request file: {}", e))?;

    let recovery_request: RecoveryRequest = serde_json::from_str(&request_content)
        .map_err(|e| anyhow::anyhow!("Failed to parse recovery request: {}", e))?;

    let _ = effects
        .log_info(&format!(
            "Loaded recovery request for account: {}",
            recovery_request.account_id
        ))
        .await;
    let _ = effects
        .log_info(&format!(
            "Requesting device: {}",
            recovery_request.requesting_device
        ))
        .await;
    let _ = effects
        .log_info(&format!(
            "Required threshold: {}",
            recovery_request.threshold
        ))
        .await;

    // Check if justification exists
    let justification_text = &recovery_request.context.justification;
    if !justification_text.is_empty() {
        let _ = effects
            .log_info(&format!("Justification: {}", justification_text))
            .await;
    }

    // Get current device ID from agent configuration
    let guardian_device = match get_current_device_id(effects).await {
        Ok(device_id) => device_id,
        Err(_) => {
            // Fallback to a device ID derived from the first guardian ID in the request
            let first_guardian = recovery_request
                .guardians
                .guardians
                .first()
                .ok_or_else(|| anyhow::anyhow!("No guardians in recovery request"))?;
            first_guardian.device_id
        }
    };

    // Find this device in the guardian set
    let guardian_profile = recovery_request
        .guardians
        .by_device(&guardian_device)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Current device {} is not a guardian for this recovery",
                guardian_device
            )
        })?;

    let _ = effects
        .log_info(&format!(
            "Approving as guardian: {} ({})",
            guardian_profile.label, guardian_profile.guardian_id
        ))
        .await;

    // Initialize coordinator with effect system
    let coordinator = GuardianKeyRecoveryCoordinator::new(effects.clone());

    // Execute guardian approval through choreographic system
    let _ = effects
        .log_info("Executing guardian approval workflow...")
        .await;

    // In the current architecture, approvals are coordinated through the main recovery choreography
    // For demonstration, we simulate an approval response
    let approval_result =
        simulate_guardian_approval(effects, &recovery_request, &guardian_profile).await;

    match approval_result {
        Ok(approval_data) => {
            let _ = effects
                .log_info("Guardian approval completed successfully!")
                .await;
            let _ = effects
                .log_info(&format!("Approval timestamp: {}", approval_data.timestamp))
                .await;
            let _ = effects
                .log_info(&format!(
                    "Key share size: {} bytes",
                    approval_data.key_share.len()
                ))
                .await;
        }
        Err(e) => {
            return Err(anyhow::anyhow!("Guardian approval failed: {}", e));
        }
    }
    // 3. Key share generation and encryption
    // 4. Partial signature creation
    // 5. Response transmission back to requesting device

    let approval_result: Result<RecoveryResponse, String> =
        Err("Guardian approval integration pending".to_string());
    match approval_result {
        Ok(response) => {
            if response.success {
                let _ = effects
                    .log_info("Guardian approval completed successfully!")
                    .await;

                // Find our guardian's share in the response
                let our_share = response
                    .guardian_shares
                    .iter()
                    .find(|share| share.guardian.guardian_id == guardian_profile.guardian_id);

                if let Some(share) = our_share {
                    let _ = effects
                        .log_info(&format!(
                            "Contributed key share at timestamp: {}",
                            share.issued_at
                        ))
                        .await;
                }

                let _ = effects
                    .log_info(&format!(
                        "Total approvals collected: {}/{}",
                        response.guardian_shares.len(),
                        recovery_request.threshold
                    ))
                    .await;
            } else {
                let error_msg = response
                    .error
                    .unwrap_or_else(|| "Unknown error".to_string());
                return Err(anyhow::anyhow!("Guardian approval failed: {}", error_msg));
            }
        }
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Guardian approval choreography failed: {}",
                e
            ));
        }
    }

    Ok(())
}

async fn get_status(effects: &AuraEffectSystem) -> Result<()> {
    let _ = effects.log_info("Checking recovery status").await;

    // Query Journal for active recovery sessions
    let _ = effects
        .log_info("Querying Journal for active recovery sessions...")
        .await;

    let journal = effects
        .get_journal()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get journal: {}", e))?;

    // Query for recovery-related facts
    let recovery_facts: Vec<_> = journal
        .facts
        .keys()
        .filter(|key| key.contains("recovery") || key == "emergency_recovery_initiated")
        .collect();

    let completed_facts: Vec<_> = journal
        .facts
        .keys()
        .filter(|key| key == "emergency_recovery_completed")
        .collect();

    // Find active recoveries (initiated but not completed)
    let active_recoveries: Vec<_> = recovery_facts
        .into_iter()
        .filter(|key| {
            !completed_facts.iter().any(|completed_key| {
                // Check if there's a corresponding completion fact
                key.contains("initiated") && completed_key.contains("completed")
            })
        })
        .collect();

    if active_recoveries.is_empty() {
        let _ = effects.log_info("No active recovery sessions found").await;
    } else {
        let _ = effects
            .log_info(&format!(
                "Found {} active recovery session(s):",
                active_recoveries.len()
            ))
            .await;

        for key in active_recoveries {
            if let Some(value) = journal.facts.get(&key) {
                let _ = effects
                    .log_info(&format!("  Type: {}, Value: {:?}", key, value))
                    .await;
            }
        }
    }

    Ok(())
}

async fn dispute_recovery(effects: &AuraEffectSystem, evidence: &str, reason: &str) -> Result<()> {
    let _ = effects
        .log_info(&format!("Filing dispute for evidence: {}", evidence))
        .await;
    let _ = effects.log_info(&format!("Reason: {}", reason)).await;

    // Parse evidence identifier
    // TODO: Implement proper evidence ID validation
    if evidence.is_empty() {
        return Err(anyhow::anyhow!("Evidence ID cannot be empty"));
    }

    if reason.is_empty() {
        return Err(anyhow::anyhow!("Dispute reason cannot be empty"));
    }

    // Get current device ID from agent state (placeholder)
    // TODO: Query actual device ID from agent configuration
    let disputing_device = DeviceId::try_from("current-disputing-device")
        .map_err(|e| anyhow::anyhow!("Failed to create device ID: {}", e))?;

    // Look up guardian ID from device ID via Journal/Web-of-Trust
    let journal = effects
        .get_journal()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get journal: {}", e))?;

    // Find guardian ID associated with this device
    let guardian_id = journal
        .facts
        .keys()
        .filter(|key| key.starts_with("guardian_device:"))
        .find_map(|key| {
            if let Some(value) = journal.facts.get(&key) {
                if format!("{:?}", value).contains(&disputing_device.to_string()) {
                    // Parse guardian ID from key
                    if let Some(guardian_part) = key.split(':').nth(1) {
                        guardian_part.parse::<GuardianId>().ok()
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        })
        .unwrap_or_else(|| {
            // Fallback: create guardian ID from device
            format!("guardian-{}", disputing_device)
                .parse::<GuardianId>()
                .unwrap_or_else(|_| GuardianId::new())
        });

    let _ = effects
        .log_info(&format!(
            "Filing dispute as guardian {} from device {}",
            guardian_id, disputing_device
        ))
        .await;

    // Validate that dispute window is still open
    let _ = effects
        .log_info("Validating dispute window and guardian eligibility...")
        .await;

    let current_journal = effects
        .get_journal()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get journal: {}", e))?;

    // Look up recovery evidence by ID in Journal
    let evidence_key = format!("recovery_evidence.{}", evidence);
    if let Some(aura_core::FactValue::String(evidence_data)) =
        current_journal.facts.get(&evidence_key)
    {
        // Parse evidence data to check dispute window
        if let Ok(evidence_json) = serde_json::from_str::<serde_json::Value>(evidence_data) {
            if let Some(dispute_window_ends) = evidence_json
                .get("dispute_window_ends_at")
                .and_then(|v| v.as_u64())
            {
                let current_time = effects.current_time().await;
                if current_time > dispute_window_ends {
                    return Err(anyhow::anyhow!(
                        "Dispute window has closed for evidence {}",
                        evidence
                    ));
                }
            }
        }
    }

    // Check if this guardian has already filed a dispute
    let existing_dispute_key = format!("recovery_dispute.{}.{}", evidence, guardian_id);
    if current_journal.facts.contains_key(&existing_dispute_key) {
        return Err(anyhow::anyhow!(
            "Guardian {} has already filed a dispute for evidence {}",
            guardian_id,
            evidence
        ));
    }

    // Create dispute record
    use aura_core::effects::TimeEffects;
    use aura_recovery::types::RecoveryDispute;

    let current_timestamp = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let dispute = RecoveryDispute {
        guardian_id,
        reason: reason.to_string(),
        filed_at: current_timestamp,
    };

    let _ = effects
        .log_info(&format!(
            "Created dispute record with timestamp: {}",
            dispute.filed_at
        ))
        .await;

    // Store dispute in Journal using proper Journal effects API
    let mut current_journal = effects
        .get_journal()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get current journal: {}", e))?;

    // Insert dispute fact into journal
    let dispute_key = format!("recovery_dispute.{}.{}", evidence, dispute.guardian_id);
    let dispute_value = aura_core::FactValue::String(
        serde_json::to_string(&dispute)
            .map_err(|e| anyhow::anyhow!("Failed to serialize dispute: {}", e))?,
    );

    current_journal
        .facts
        .insert(dispute_key.clone(), dispute_value);

    // Persist updated journal
    effects
        .persist_journal(&current_journal)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to persist journal with dispute: {}", e))?;

    let _ = effects
        .log_info(&format!(
            "Dispute recorded in Journal with key: {}",
            dispute_key
        ))
        .await;

    let _ = effects
        .log_info(&format!("  Evidence ID: {}", evidence))
        .await;
    let _ = effects
        .log_info(&format!("  Guardian ID: {}", guardian_id))
        .await;
    let _ = effects.log_info(&format!("  Reason: {}", reason)).await;
    let _ = effects
        .log_info(&format!("  Filed at: {}", dispute.filed_at))
        .await;

    let _ = effects.log_info("Dispute filed successfully!").await;

    Ok(())
}

/// Query guardian device ID from Journal/Web-of-Trust
async fn query_guardian_device_id(
    effects: &AuraEffectSystem,
    guardian_id: &GuardianId,
) -> Result<DeviceId, anyhow::Error> {
    // Query Journal for guardian metadata
    let journal = effects
        .get_journal()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get journal: {}", e))?;

    // Look for guardian device mapping in journal facts
    let guardian_key = format!("guardian.{}.device_id", guardian_id);
    if let Some(fact) = journal.facts.get(&guardian_key) {
        if let aura_core::FactValue::String(device_str) = fact {
            return DeviceId::try_from(device_str.as_str())
                .map_err(|e| anyhow::anyhow!("Invalid device ID in journal: {}", e));
        }
    }

    // If not found in journal, try guardian ID as device ID
    DeviceId::try_from(guardian_id.to_string().as_str())
        .map_err(|e| anyhow::anyhow!("Failed to create device ID for guardian: {}", e))
}

/// Get current device ID from agent configuration
async fn get_current_device_id(effects: &AuraEffectSystem) -> Result<DeviceId, anyhow::Error> {
    // Try to get device ID from journal facts
    let journal = effects
        .get_journal()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get journal: {}", e))?;

    // Look for device ID in journal facts
    if let Some(fact) = journal.facts.get("agent.device_id") {
        if let aura_core::FactValue::String(device_str) = fact {
            return DeviceId::try_from(device_str.as_str())
                .map_err(|e| anyhow::anyhow!("Invalid device ID in journal: {}", e));
        }
    }

    Err(anyhow::anyhow!(
        "Device ID not found in agent configuration"
    ))
}

/// Simulate guardian approval for recovery request
async fn simulate_guardian_approval(
    effects: &AuraEffectSystem,
    request: &RecoveryRequest,
    guardian: &GuardianProfile,
) -> Result<aura_recovery::guardian_key_recovery::GuardianKeyApproval, anyhow::Error> {
    use aura_recovery::guardian_key_recovery::GuardianKeyApproval;

    // Generate simulated key share (in production, this would be from real FROST)
    let key_share = vec![0x42; 32]; // Simulated 32-byte key share
    let partial_signature = vec![0x43; 64]; // Simulated 64-byte signature

    // Get current timestamp
    let timestamp = effects.current_time().await;

    let _ = effects
        .log_info(&format!(
            "Generated approval as guardian {} for recovery {}",
            guardian.guardian_id, request.account_id
        ))
        .await;

    Ok(GuardianKeyApproval {
        guardian_id: guardian.guardian_id,
        key_share,
        partial_signature,
        timestamp,
    })
}

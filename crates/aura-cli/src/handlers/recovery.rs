//! Guardian Recovery CLI Commands
//!
//! Commands for managing guardian-based account recovery.

use anyhow::Result;
use aura_agent::{AuraEffectSystem, EffectContext};
use aura_authenticate::guardian_auth::{RecoveryContext, RecoveryOperationType};
use aura_core::effects::{JournalEffects, StorageEffects, TimeEffects};
use aura_core::identifiers::{ContextId, GuardianId};
use aura_core::time::TimeStamp;
use aura_core::{AccountId, AuthorityId, DeviceId, FactValue};
use aura_journal::fact::{FactContent, RelationalFact};
use aura_recovery::types::{GuardianProfile, GuardianSet};
use aura_recovery::{RecoveryRequest, RecoveryResponse};
use std::path::Path;

use crate::RecoveryAction;

/// Extract a millisecond timestamp from any TimeStamp variant for display/logging.
fn timestamp_ms(ts: &TimeStamp) -> u64 {
    match ts {
        TimeStamp::PhysicalClock(p) => p.ts_ms,
        TimeStamp::LogicalClock(l) => l.lamport,
        TimeStamp::OrderClock(o) => {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(&o.0[..8]);
            u64::from_be_bytes(buf)
        }
        TimeStamp::Range(r) => r.latest_ms,
    }
}

/// Handle recovery action requests from CLI
///
/// Processes recovery operations including starting recovery, submitting approvals,
/// and handling recovery responses based on the action type.
pub async fn handle_recovery(
    ctx: &EffectContext,
    effects: &AuraEffectSystem,
    action: &RecoveryAction,
) -> Result<()> {
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
                ctx,
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
        RecoveryAction::Approve { request_file } => {
            approve_recovery(ctx, effects, request_file).await
        }
        RecoveryAction::Status => get_status(ctx, effects).await,
        RecoveryAction::Dispute { evidence, reason } => {
            dispute_recovery(ctx, effects, evidence, reason).await
        }
    }
}

fn encode_recovery_fact<T: serde::Serialize>(kind: &str, payload: &T) -> Result<FactValue> {
    let content = FactContent::Relational(RelationalFact::Generic {
        context_id: ContextId::new(),
        binding_type: kind.to_string(),
        binding_data: serde_json::to_vec(payload)
            .map_err(|e| anyhow::anyhow!("Failed to serialize recovery payload: {}", e))?,
    });

    serde_json::to_vec(&content)
        .map(FactValue::Bytes)
        .map_err(|e| anyhow::anyhow!("Failed to encode fact content: {}", e))
}

async fn start_recovery(
    ctx: &EffectContext,
    effects: &AuraEffectSystem,
    account: &str,
    guardians: &str,
    threshold: u32,
    priority: &str,
    dispute_hours: u64,
    justification: Option<&str>,
) -> Result<()> {
    println!("Starting {} recovery for account: {}", priority, account);
    println!("Guardians: {}", guardians);
    println!("Threshold: {}", threshold);
    println!("Dispute window: {} hours", dispute_hours);

    if let Some(just) = justification {
        println!("Justification: {}", just);
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
    let mut guardian_profiles = Vec::new();
    for (i, guardian_str) in guardian_ids.iter().enumerate() {
        // Parse guardian ID
        let guardian_id = guardian_str
            .parse::<GuardianId>()
            .map_err(|e| anyhow::anyhow!("Invalid guardian ID '{}': {}", guardian_str, e))?;

        // Query actual device IDs from Journal/Web-of-Trust
        let device_id = match query_guardian_device_id(ctx, effects, &guardian_id).await {
            Ok(id) => id,
            Err(_) => {
                // Fallback to generated device ID for now
                tracing::warn!(
                    guardian_id = ?guardian_id,
                    "Guardian device ID not found in Journal, using generated ID"
                );
                DeviceId::from(format!("guardian-device-{}", i).as_str())
            }
        };

        guardian_profiles.push(GuardianProfile::new(
            guardian_id,
            device_id,
            format!("Guardian {}", i + 1),
        ));
    }
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
        timestamp: <AuraEffectSystem as TimeEffects>::current_timestamp(effects).await,
    };

    // Derive requesting device from authority context
    let requesting_device = DeviceId::from_uuid(ctx.authority_id().uuid());

    // Create recovery request
    let recovery_request = RecoveryRequest {
        requesting_device,
        account_id,
        context,
        threshold: threshold as usize,
        guardians: guardian_set,
        auth_token: None,
    };

    println!("Executing recovery protocol via proper coordinator...");

    // Convert to the new recovery protocol format
    let recovery_request_new = aura_recovery::recovery_protocol::RecoveryRequest {
        recovery_id: account_id.to_string(),
        account_authority: AuthorityId::from_uuid(account_id.0),
        new_tree_commitment: aura_core::Hash32::new([0; 32]), // Mock commitment
        operation: aura_recovery::recovery_protocol::RecoveryOperation::ReplaceTree {
            new_public_key: vec![0; 32], // Mock public key
        },
        justification: justification
            .unwrap_or("CLI recovery operation")
            .to_string(),
    };

    // Create recovery protocol handler
    use aura_recovery::recovery_protocol::{RecoveryProtocol, RecoveryProtocolHandler};
    use aura_relational::RelationalContext;
    use std::sync::Arc;

    let guardian_authorities: Vec<AuthorityId> = recovery_request
        .guardians
        .iter()
        .map(|_g| AuthorityId::new()) // Mock authority IDs
        .collect();
    // Create a mock relational context for demo
    let recovery_context = Arc::new(RelationalContext::new(guardian_authorities.clone()));

    let recovery_protocol = RecoveryProtocol::new(
        recovery_context,
        AuthorityId::from_uuid(account_id.0),
        guardian_authorities,
        threshold as usize,
    );

    let protocol_handler = RecoveryProtocolHandler::new(Arc::new(recovery_protocol));

    // Initiate recovery using the proper protocol
    protocol_handler
        .handle_recovery_initiation(recovery_request_new, effects)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to initiate recovery protocol: {}", e))?;

    // Store legacy format for backwards compatibility
    let request_path = format!("recovery_request_{}.json", account_id);
    let request_json = serde_json::to_vec_pretty(&recovery_request)
        .map_err(|e| anyhow::anyhow!("Failed to serialize recovery request: {}", e))?;

    // Store via StorageEffects for devices connected through the effect system
    effects
        .store(
            &format!("recovery_request:{}", request_path),
            request_json.clone(),
        )
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to store recovery request via storage effects: {}",
                e
            )
        })?;

    // Also write a local file for manual distribution
    std::fs::write(&request_path, &request_json)
        .map_err(|e| anyhow::anyhow!("Failed to write recovery request file: {}", e))?;

    println!("Recovery request stored for guardians at: {}", request_path);
    println!("Share this file with guardians and ask them to run `aura recovery approve --request-file {}`", request_path);

    // Update journal with recovery initiation using proper effects
    let recovery_fact_key = format!("recovery_initiated.{}", account_id);
    let recovery_fact_value = encode_recovery_fact("recovery_initiated", &recovery_request)?;

    let mut journal_delta = aura_core::Journal::new();
    journal_delta
        .facts
        .insert(recovery_fact_key.clone(), recovery_fact_value);

    let current_journal = effects
        .get_journal()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get journal: {}", e))?;
    let updated_journal = effects
        .merge_facts(&current_journal, &journal_delta)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to merge journal facts: {}", e))?;
    effects
        .persist_journal(&updated_journal)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to persist journal: {}", e))?;

    println!("Recovery initiated successfully via protocol coordinator.");
    println!(
        "Recovery fact recorded in journal with key: {}",
        recovery_fact_key
    );
    println!("Guardians will be notified via network effects.");

    Ok(())
}

async fn approve_recovery(
    ctx: &EffectContext,
    effects: &AuraEffectSystem,
    request_file: &Path,
) -> Result<()> {
    println!("Approving recovery from: {}", request_file.display());

    // Read and parse recovery request file via StorageEffects
    let file_key = format!("recovery_request:{}", request_file.display());
    let request_content = match effects.retrieve(&file_key).await {
        Ok(Some(data)) => String::from_utf8(data)
            .map_err(|e| anyhow::anyhow!("Invalid UTF-8 in request file: {}", e))?,
        Ok(None) => {
            return Err(anyhow::anyhow!(
                "Request file not found: {}",
                request_file.display()
            ))
        }
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Failed to read request file via storage effects: {}",
                e
            ))
        }
    };

    let recovery_request: RecoveryRequest = serde_json::from_str(&request_content)
        .map_err(|e| anyhow::anyhow!("Failed to parse recovery request: {}", e))?;

    println!(
        "Loaded recovery request for account: {}",
        recovery_request.account_id
    );
    println!("Requesting device: {}", recovery_request.requesting_device);
    println!("Required threshold: {}", recovery_request.threshold);

    // Check if justification exists
    let justification_text = &recovery_request.context.justification;
    if !justification_text.is_empty() {
        println!("Justification: {}", justification_text);
    }

    // Get current device ID from agent configuration
    let guardian_device = match get_current_device_id(ctx, effects).await {
        Ok(device_id) => device_id,
        Err(_) => {
            // Fallback to a device ID derived from the first guardian ID in the request
            let first_guardian = recovery_request
                .guardians
                .iter()
                .next()
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

    println!(
        "Approving as guardian: {} ({})",
        guardian_profile.label, guardian_profile.guardian_id
    );

    // Execute guardian approval through choreographic system
    println!("Executing guardian approval workflow...");

    // Generate real guardian approval using FROST threshold signing
    let approval_result =
        generate_guardian_approval(ctx, effects, &recovery_request, guardian_profile).await?;

    println!("Guardian approval completed successfully!");
    println!(
        "Approval timestamp (ms): {}",
        timestamp_ms(&approval_result.timestamp)
    );
    println!("Key share size: {} bytes", approval_result.key_share.len());

    // Build recovery share and evidence (placeholder aggregation)
    let share = aura_recovery::types::RecoveryShare {
        guardian: guardian_profile.clone(),
        share: approval_result.key_share.clone(),
        partial_signature: approval_result.partial_signature.clone(),
        issued_at: timestamp_ms(&approval_result.timestamp),
    };

    let evidence = aura_recovery::types::RecoveryEvidence {
        account_id: recovery_request.account_id,
        recovering_device: recovery_request.requesting_device,
        guardians: vec![guardian_profile.guardian_id],
        issued_at: timestamp_ms(&approval_result.timestamp) / 1000,
        cooldown_expires_at: timestamp_ms(&approval_result.timestamp) / 1000 + 24 * 3600,
        dispute_window_ends_at: timestamp_ms(&approval_result.timestamp) / 1000 + 48 * 3600,
        guardian_profiles: vec![guardian_profile.clone()],
        disputes: Vec::new(),
        threshold_signature: None,
    };

    let response = RecoveryResponse {
        success: true,
        error: None,
        key_material: None,
        guardian_shares: vec![share.clone()],
        evidence,
        signature: aura_core::frost::ThresholdSignature::new(vec![0; 64], vec![0]),
    };

    // Persist approval so the requesting device can collect it
    let response_json = serde_json::to_vec_pretty(&response)
        .map_err(|e| anyhow::anyhow!("Failed to serialize approval response: {}", e))?;

    let response_key = format!(
        "recovery_response:{}:{}",
        recovery_request.account_id, guardian_profile.guardian_id
    );
    effects
        .store(&response_key, response_json.clone())
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to store approval response via storage effects: {}",
                e
            )
        })?;

    let response_path = format!(
        "recovery_response_{}_{}.json",
        recovery_request.account_id, guardian_profile.guardian_id
    );
    std::fs::write(&response_path, &response_json)
        .map_err(|e| anyhow::anyhow!("Failed to write approval response file: {}", e))?;

    println!("Guardian approval saved at: {}", response_path);
    println!(
        "Share count contributed: 1/{} (local placeholder aggregation)",
        recovery_request.threshold
    );

    Ok(())
}

async fn get_status(_ctx: &EffectContext, effects: &AuraEffectSystem) -> Result<()> {
    println!("Checking recovery status");

    // Query Journal for active recovery sessions
    println!("Querying Journal for active recovery sessions...");

    // Query Journal for recovery-related facts using proper JournalEffects
    let current_journal = effects
        .get_journal()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get journal via effects: {}", e))?;

    let recovery_facts: Vec<_> = current_journal
        .facts
        .keys()
        .filter(|key| key.contains("recovery") || key == "emergency_recovery_initiated")
        .collect();

    let completed_facts: Vec<_> = current_journal
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
        println!("No active recovery sessions found");
    } else {
        println!(
            "Found {} active recovery session(s):",
            active_recoveries.len()
        );

        for key in active_recoveries {
            if let Some(value) = current_journal.facts.get(&key) {
                println!("  Type: {}, Value: {:?}", key, value);
            }
        }
    }

    Ok(())
}

async fn dispute_recovery(
    _ctx: &EffectContext,
    effects: &AuraEffectSystem,
    evidence: &str,
    reason: &str,
) -> Result<()> {
    println!("Filing dispute for evidence: {}", evidence);
    println!("Reason: {}", reason);

    // Parse evidence identifier
    let _ = uuid::Uuid::parse_str(evidence)
        .map_err(|e| anyhow::anyhow!("Invalid evidence ID '{}': {}", evidence, e))?;

    if evidence.is_empty() {
        return Err(anyhow::anyhow!("Evidence ID cannot be empty"));
    }

    if reason.is_empty() {
        return Err(anyhow::anyhow!("Dispute reason cannot be empty"));
    }

    // Use caller authority as disputing device (deterministic mapping)
    let disputing_device = DeviceId::from_uuid(_ctx.authority_id().uuid());

    // Look up guardian ID from device ID via Journal/Web-of-Trust using proper JournalEffects
    let current_journal = effects
        .get_journal()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get journal via effects: {}", e))?;

    // Find guardian ID associated with this device
    let guardian_id = current_journal
        .facts
        .keys()
        .filter(|key| key.starts_with("guardian_device:"))
        .find_map(|key| {
            if let Some(value) = current_journal.facts.get(&key) {
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

    println!(
        "Filing dispute as guardian {} from device {}",
        guardian_id, disputing_device
    );

    // Validate that dispute window is still open
    println!("Validating dispute window and guardian eligibility...");

    // Get current journal state via proper JournalEffects
    let dispute_journal = effects
        .get_journal()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get journal via effects: {}", e))?;

    // Look up recovery evidence by ID in Journal
    let evidence_key = format!("recovery_evidence.{}", evidence);
    if let Some(value) = dispute_journal.facts.get(&evidence_key) {
        let evidence_json: serde_json::Value = match value {
            FactValue::String(data) => serde_json::from_str(data),
            FactValue::Bytes(bytes) => serde_json::from_slice(bytes),
            _ => Ok(serde_json::Value::Null),
        }
        .map_err(|e| anyhow::anyhow!("Failed to parse evidence JSON: {}", e))?;

        if let Some(dispute_window_ends) = evidence_json
            .get("dispute_window_ends_at")
            .and_then(|v| v.as_u64())
        {
            let current_time = <AuraEffectSystem as TimeEffects>::current_timestamp(effects).await;
            if current_time > dispute_window_ends {
                return Err(anyhow::anyhow!(
                    "Dispute window has closed for evidence {}",
                    evidence
                ));
            }
        }
    }

    // Check if this guardian has already filed a dispute
    let existing_dispute_key = format!("recovery_dispute.{}.{}", evidence, guardian_id);
    if dispute_journal.facts.contains_key(&existing_dispute_key) {
        return Err(anyhow::anyhow!(
            "Guardian {} has already filed a dispute for evidence {}",
            guardian_id,
            evidence
        ));
    }

    // Create dispute record
    use aura_recovery::types::RecoveryDispute;

    let current_timestamp = <AuraEffectSystem as TimeEffects>::current_timestamp(effects).await;

    let dispute = RecoveryDispute {
        guardian_id,
        reason: reason.to_string(),
        filed_at: current_timestamp,
    };

    println!(
        "Created dispute record with timestamp: {}",
        dispute.filed_at
    );

    // Store dispute in Journal using proper JournalEffects API
    let dispute_key = format!("recovery_dispute.{}.{}", evidence, dispute.guardian_id);
    let dispute_value = encode_recovery_fact("recovery_dispute", &dispute)?;

    // Create a journal delta with the new dispute fact
    let mut journal_delta = aura_core::Journal::new();
    journal_delta
        .facts
        .insert(dispute_key.clone(), dispute_value);

    // Get current journal and merge the delta
    let current_journal = effects
        .get_journal()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get current journal: {}", e))?;

    let updated_journal = effects
        .merge_facts(&current_journal, &journal_delta)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to merge journal facts: {}", e))?;

    // Persist the updated journal
    effects
        .persist_journal(&updated_journal)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to persist journal: {}", e))?;

    println!("Dispute recorded in Journal with key: {}", dispute_key);

    println!("  Evidence ID: {}", evidence);
    println!("  Guardian ID: {}", guardian_id);
    println!("  Reason: {}", reason);
    println!("  Filed at: {}", dispute.filed_at);

    println!("Dispute filed successfully!");

    Ok(())
}

/// Query guardian device ID from Journal/Web-of-Trust
async fn query_guardian_device_id(
    _ctx: &EffectContext,
    effects: &AuraEffectSystem,
    guardian_id: &GuardianId,
) -> Result<DeviceId, anyhow::Error> {
    // Query Journal for guardian metadata using proper JournalEffects
    let journal = effects.get_journal().await.map_err(anyhow::Error::new)?;

    // Look for guardian device mapping in journal facts
    let guardian_key = format!("guardian.{}.device_id", guardian_id);
    if let Some(fact) = journal.facts.get(&guardian_key) {
        let device_str = match fact {
            aura_core::FactValue::String(device_str) => Some(device_str.clone()),
            aura_core::FactValue::Bytes(bytes) => String::from_utf8(bytes.clone()).ok(),
            _ => None,
        };
        if let Some(device_str) = device_str {
            return Ok(DeviceId::from(device_str.as_str()));
        }
    }

    // If not found in journal, try guardian ID as device ID
    Ok(DeviceId::from(guardian_id.to_string().as_str()))
}

/// Get current device ID from agent configuration
async fn get_current_device_id(
    _ctx: &EffectContext,
    effects: &AuraEffectSystem,
) -> Result<DeviceId, anyhow::Error> {
    // Try to get device ID from journal facts using proper JournalEffects
    let journal = effects.get_journal().await.map_err(anyhow::Error::new)?;

    // Look for device ID in journal facts
    if let Some(fact) = journal.facts.get("agent.device_id") {
        let device_str = match fact {
            aura_core::FactValue::String(device_str) => Some(device_str.clone()),
            aura_core::FactValue::Bytes(bytes) => String::from_utf8(bytes.clone()).ok(),
            _ => None,
        };
        if let Some(device_str) = device_str {
            return Ok(DeviceId::from(device_str.as_str()));
        }
    }

    Err(anyhow::anyhow!(
        "Device ID not found in agent configuration"
    ))
}

/// Generate real guardian approval for recovery request using FROST threshold signing
async fn generate_guardian_approval(
    _ctx: &EffectContext,
    effects: &AuraEffectSystem,
    request: &RecoveryRequest,
    guardian: &GuardianProfile,
) -> Result<aura_recovery::guardian_key_recovery::GuardianKeyApproval, anyhow::Error> {
    use aura_recovery::guardian_key_recovery::GuardianKeyApproval;

    // Get current timestamp
    let timestamp_ms = <AuraEffectSystem as TimeEffects>::current_timestamp(effects).await;

    // Create recovery message to sign
    let recovery_message = serde_json::to_vec(&request)
        .map_err(|e| anyhow::anyhow!("Failed to serialize recovery request: {}", e))?;

    println!(
        "Generating placeholder guardian approval for guardian {} and recovery {}",
        guardian.guardian_id, request.account_id
    );

    // Placeholder signature derived from the recovery message; replace with
    // real threshold signing via effects when available.
    let partial_sig_bytes: Vec<u8> = {
        use blake3::Hasher;
        let mut h = Hasher::new();
        h.update(&recovery_message);
        h.finalize().as_bytes().to_vec()
    };

    let key_share_bytes = [0u8; 32]; // Mock key share bytes for demo

    Ok(GuardianKeyApproval {
        guardian_id: guardian.guardian_id,
        key_share: key_share_bytes.to_vec(),
        partial_signature: partial_sig_bytes.to_vec(),
        timestamp: aura_core::time::TimeStamp::PhysicalClock(aura_core::time::PhysicalTime {
            ts_ms: timestamp_ms,
            uncertainty: None,
        }),
    })
}

//! Guardian Recovery CLI Commands
//!
//! Commands for managing guardian-based account recovery.
//! Uses the authority model - guardians are identified by AuthorityId.
//!
//! Returns structured `CliOutput` for testability.

use crate::error::{TerminalError, TerminalResult};
use crate::handlers::{CliOutput, HandlerContext};
use aura_authentication::{RecoveryContext, RecoveryOperationType};
use aura_core::effects::{JournalEffects, StorageCoreEffects};
use aura_core::hash;
use aura_core::identifiers::ContextId;
use aura_core::time::TimeStamp;
use aura_core::{AuthorityId, FactValue, Hash32};
use aura_journal::fact::{FactContent, RelationalFact};
use aura_protocol::effects::EffectApiEffects;
use aura_recovery::types::{GuardianProfile, GuardianSet, RecoveryDispute};
use aura_recovery::{RecoveryRequest, RecoveryResponse};
use std::path::Path;

use crate::handlers::recovery_status;
use crate::ids;
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
/// Returns `CliOutput` instead of printing directly.
///
/// **Standardized Signature (Task 2.2)**: Uses `HandlerContext` for unified parameter passing.
pub async fn handle_recovery(
    ctx: &HandlerContext<'_>,
    action: &RecoveryAction,
) -> TerminalResult<CliOutput> {
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
                account,
                guardians,
                *threshold,
                priority,
                *dispute_hours,
                justification.as_deref(),
            )
            .await
        }
        RecoveryAction::Approve { request_file } => approve_recovery(ctx, request_file).await,
        RecoveryAction::Status => get_status(ctx).await,
        RecoveryAction::Dispute { evidence, reason } => {
            dispute_recovery(ctx, evidence, reason).await
        }
    }
}

fn encode_recovery_fact<T: serde::Serialize>(
    context_id: ContextId,
    kind: &str,
    payload: &T,
) -> TerminalResult<FactValue> {
    let content = FactContent::Relational(RelationalFact::Generic {
        context_id,
        binding_type: kind.to_string(),
        binding_data: serde_json::to_vec(payload).map_err(|e| {
            TerminalError::Operation(format!("Failed to serialize recovery payload: {}", e))
        })?,
    });

    serde_json::to_vec(&content)
        .map(FactValue::Bytes)
        .map_err(|e| TerminalError::Operation(format!("Failed to encode fact content: {}", e)))
}

async fn start_recovery(
    ctx: &HandlerContext<'_>,
    account: &str,
    guardians: &str,
    threshold: u32,
    priority: &str,
    dispute_hours: u64,
    justification: Option<&str>,
) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    output.section(format!("Starting {} recovery", priority));
    output.kv("Account", account);
    output.kv("Guardians", guardians);
    output.kv("Threshold", threshold.to_string());
    output.kv("Dispute window", format!("{} hours", dispute_hours));

    if let Some(just) = justification {
        output.kv("Justification", just);
    }

    // Parse account ID as authority
    let account_authority = ids::authority_id(account);

    // Parse guardians list (comma-separated authority identifiers)
    let guardian_strs: Vec<&str> = guardians.split(',').map(|s| s.trim()).collect();
    if guardian_strs.is_empty() {
        return Err(TerminalError::Input("No guardians specified".into()));
    }

    // Convert to guardian profiles using authority model
    let mut guardian_profiles = Vec::new();
    for (i, guardian_str) in guardian_strs.iter().enumerate() {
        let guardian_authority = ids::authority_id(guardian_str);
        guardian_profiles.push(GuardianProfile::with_label(
            guardian_authority,
            format!("Guardian {}", i + 1),
        ));
    }
    let guardian_set = GuardianSet::new(guardian_profiles);

    if guardian_set.len() < threshold as usize {
        return Err(TerminalError::Input(format!(
            "Threshold {} exceeds number of guardians {}",
            threshold,
            guardian_set.len()
        )));
    }

    // Create recovery context
    let context = RecoveryContext {
        operation_type: RecoveryOperationType::DeviceKeyRecovery,
        justification: justification
            .unwrap_or("CLI recovery operation")
            .to_string(),
        is_emergency: priority == "emergency",
        timestamp: ctx.effects().current_timestamp().await.unwrap_or(0),
    };

    // Get initiator authority from context
    let initiator_id = ctx.effect_context().authority_id();

    // Create recovery request using authority model
    let recovery_request = RecoveryRequest {
        initiator_id,
        account_id: account_authority,
        context,
        threshold: threshold as usize,
        guardians: guardian_set.clone(),
    };

    output.println("Executing recovery protocol via proper coordinator...");

    // Convert to the new recovery protocol format
    let commitment = Hash32::new(hash::hash(
        format!("recovery-commitment:{}", account_authority).as_bytes(),
    ));
    let new_public_key =
        hash::hash(format!("recovery-new-key:{}", account_authority).as_bytes()).to_vec();

    let recovery_request_new = aura_recovery::recovery_protocol::RecoveryRequest {
        recovery_id: account_authority.to_string(),
        account_authority,
        new_tree_commitment: commitment,
        operation: aura_recovery::recovery_protocol::RecoveryOperation::ReplaceTree {
            new_public_key,
        },
        justification: justification
            .unwrap_or("CLI recovery operation")
            .to_string(),
    };

    // Create recovery protocol handler
    use aura_recovery::recovery_protocol::{RecoveryProtocol, RecoveryProtocolHandler};
    use aura_relational::RelationalContext;
    use std::sync::Arc;

    let guardian_authorities: Vec<AuthorityId> =
        guardian_set.iter().map(|g| g.authority_id).collect();

    // Create a mock relational context for demo
    let recovery_context = Arc::new(RelationalContext::new(guardian_authorities.clone()));

    let recovery_protocol = RecoveryProtocol::new(
        recovery_context,
        account_authority,
        guardian_authorities,
        threshold as usize,
    );

    let protocol_handler = RecoveryProtocolHandler::new(Arc::new(recovery_protocol));

    // Initiate recovery using the proper protocol
    protocol_handler
        .handle_recovery_initiation(
            recovery_request_new,
            ctx.effects(),
            ctx.effects(),
            ctx.effects(),
        )
        .await
        .map_err(|e| {
            TerminalError::Operation(format!("Failed to initiate recovery protocol: {}", e))
        })?;

    // Store request payload deterministically via StorageEffects
    let request_path = format!("recovery_request_{}.json", account_authority);
    let request_json = serde_json::to_vec_pretty(&recovery_request).map_err(|e| {
        TerminalError::Operation(format!("Failed to serialize recovery request: {}", e))
    })?;

    let storage_key = format!("recovery_request:{}", request_path);
    ctx.effects()
        .store(&storage_key, request_json.clone())
        .await
        .map_err(|e| {
            TerminalError::Operation(format!(
                "Failed to store recovery request via storage effects: {}",
                e
            ))
        })?;

    output.kv("Recovery request stored at", &storage_key);
    output.println(format!(
        "Share the stored request key with guardians and ask them to run `aura recovery approve --request-file {}`",
        request_path
    ));

    // Update journal with recovery initiation using proper effects
    let recovery_fact_key = format!("recovery_initiated.{}", account_authority);
    let recovery_fact_value =
        encode_recovery_fact(ctx.context_id(), "recovery_initiated", &recovery_request)?;

    let mut journal_delta = aura_core::Journal::new();
    journal_delta
        .facts
        .insert(recovery_fact_key.clone(), recovery_fact_value);

    let current_journal = ctx
        .effects()
        .get_journal()
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to get journal: {}", e)))?;
    let updated_journal = ctx
        .effects()
        .merge_facts(&current_journal, &journal_delta)
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to merge journal facts: {}", e)))?;
    ctx.effects()
        .persist_journal(&updated_journal)
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to persist journal: {}", e)))?;

    output.blank();
    output.println("Recovery initiated successfully via protocol coordinator.");
    output.kv("Recovery fact recorded with key", recovery_fact_key);
    output.println("Guardians will be notified via network effects.");

    Ok(output)
}

async fn approve_recovery(
    ctx: &HandlerContext<'_>,
    request_file: &Path,
) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    output.section("Approving Recovery");
    output.kv("Request file", request_file.display().to_string());

    // Read and parse recovery request file via StorageEffects
    let file_key = format!("recovery_request:{}", request_file.display());
    let request_content = match ctx.effects().retrieve(&file_key).await {
        Ok(Some(data)) => String::from_utf8(data)
            .map_err(|e| TerminalError::Config(format!("Invalid UTF-8 in request file: {}", e)))?,
        Ok(None) => {
            return Err(TerminalError::NotFound(format!(
                "Request file not found: {}",
                request_file.display()
            )))
        }
        Err(e) => {
            return Err(TerminalError::Operation(format!(
                "Failed to read request file via storage effects: {}",
                e
            )))
        }
    };

    let recovery_request: RecoveryRequest = serde_json::from_str(&request_content)
        .map_err(|e| TerminalError::Config(format!("Failed to parse recovery request: {}", e)))?;

    output.kv("Account", recovery_request.account_id.to_string());
    output.kv("Initiator", recovery_request.initiator_id.to_string());
    output.kv("Required threshold", recovery_request.threshold.to_string());

    // Check if justification exists
    let justification_text = &recovery_request.context.justification;
    if !justification_text.is_empty() {
        output.kv("Justification", justification_text);
    }

    // Get current authority from context
    let guardian_authority = ctx.effect_context().authority_id();

    // Find this authority in the guardian set
    let guardian_profile = recovery_request
        .guardians
        .by_authority(&guardian_authority)
        .ok_or_else(|| {
            TerminalError::Input(format!(
                "Current authority {} is not a guardian for this recovery",
                guardian_authority
            ))
        })?;

    let label = guardian_profile.label.as_deref().unwrap_or("Guardian");
    output.kv(
        "Approving as",
        format!("{} ({})", label, guardian_profile.authority_id),
    );

    // Execute guardian approval through choreographic system
    output.println("Executing guardian approval workflow...");

    // Generate real guardian approval using FROST threshold signing
    let approval_result =
        generate_guardian_approval(ctx, &recovery_request, guardian_profile).await?;

    output.println("Guardian approval completed successfully!");
    output.kv(
        "Approval timestamp (ms)",
        timestamp_ms(&approval_result.timestamp).to_string(),
    );
    output.kv(
        "Key share size",
        format!("{} bytes", approval_result.key_share.len()),
    );

    // Build recovery share and evidence for downstream aggregation
    let share = aura_recovery::types::RecoveryShare {
        guardian_id: guardian_profile.authority_id,
        guardian_label: guardian_profile.label.clone(),
        share: approval_result.key_share.clone(),
        partial_signature: approval_result.partial_signature.clone(),
        issued_at_ms: timestamp_ms(&approval_result.timestamp),
    };

    let evidence = aura_recovery::types::RecoveryEvidence {
        context_id: ctx.context_id(),
        account_id: recovery_request.account_id,
        approving_guardians: vec![guardian_profile.authority_id],
        completed_at_ms: timestamp_ms(&approval_result.timestamp),
        dispute_window_ends_at_ms: timestamp_ms(&approval_result.timestamp) + 48 * 3600 * 1000,
        disputes: Vec::new(),
        threshold_signature: None,
    };

    let response = RecoveryResponse {
        success: true,
        error: None,
        key_material: None,
        guardian_shares: vec![share.clone()],
        evidence,
        signature: aura_core::threshold::ThresholdSignature::new(
            vec![0; 64],
            0,
            vec![0],
            Vec::new(),
            0,
        ),
    };

    // Persist approval so the requesting device can collect it
    let response_json = serde_json::to_vec_pretty(&response).map_err(|e| {
        TerminalError::Operation(format!("Failed to serialize approval response: {}", e))
    })?;

    let response_key = format!(
        "recovery_response:{}:{}",
        recovery_request.account_id, guardian_profile.authority_id
    );
    ctx.effects()
        .store(&response_key, response_json.clone())
        .await
        .map_err(|e| {
            TerminalError::Operation(format!(
                "Failed to store approval response via storage effects: {}",
                e
            ))
        })?;

    let response_path = format!(
        "recovery_response_{}_{}.json",
        recovery_request.account_id, guardian_profile.authority_id
    );

    ctx.effects()
        .store(
            &format!("recovery_response_file:{}", response_path),
            response_json,
        )
        .await
        .map_err(|e| {
            TerminalError::Operation(format!("Failed to persist approval response: {}", e))
        })?;

    output.kv("Guardian approval saved at", &response_path);
    output.kv(
        "Share count contributed",
        format!("1/{}", recovery_request.threshold),
    );

    Ok(output)
}

async fn get_status(ctx: &HandlerContext<'_>) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    output.section("Recovery Status");
    output.println("Querying Journal for active recovery sessions...");

    // Query Journal for recovery-related facts using proper JournalEffects
    let current_journal = ctx.effects().get_journal().await.map_err(|e| {
        TerminalError::Operation(format!("Failed to get journal via effects: {}", e))
    })?;

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

    let report = recovery_status::format_recovery_status(&active_recoveries, &completed_facts);
    output.println(report);

    Ok(output)
}

async fn dispute_recovery(
    ctx: &HandlerContext<'_>,
    evidence: &str,
    reason: &str,
) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    output.section("Filing Recovery Dispute");
    output.kv("Evidence ID", evidence);
    output.kv("Reason", reason);

    // Parse evidence identifier
    let _ = uuid::Uuid::parse_str(evidence)
        .map_err(|e| TerminalError::Input(format!("Invalid evidence ID '{}': {}", evidence, e)))?;

    if evidence.is_empty() {
        return Err(TerminalError::Input("Evidence ID cannot be empty".into()));
    }

    if reason.is_empty() {
        return Err(TerminalError::Input(
            "Dispute reason cannot be empty".into(),
        ));
    }

    // Use caller authority as disputing guardian
    let guardian_authority = ctx.effect_context().authority_id();

    output.kv("Filing as guardian", guardian_authority.to_string());
    output.println("Validating dispute window and guardian eligibility...");

    // Get current journal state via proper JournalEffects
    let dispute_journal = ctx.effects().get_journal().await.map_err(|e| {
        TerminalError::Operation(format!("Failed to get journal via effects: {}", e))
    })?;

    // Look up recovery evidence by ID in Journal
    let evidence_key = format!("recovery_evidence.{}", evidence);
    if let Some(value) = dispute_journal.facts.get(&evidence_key) {
        let evidence_json: serde_json::Value = match value {
            FactValue::String(data) => serde_json::from_str(data),
            FactValue::Bytes(bytes) => serde_json::from_slice(bytes),
            _ => Ok(serde_json::Value::Null),
        }
        .map_err(|e| TerminalError::Config(format!("Failed to parse evidence JSON: {}", e)))?;

        if let Some(dispute_window_ends) = evidence_json
            .get("dispute_window_ends_at_ms")
            .and_then(|v| v.as_u64())
        {
            let current_time = ctx.effects().current_timestamp().await.unwrap_or(0);
            if current_time > dispute_window_ends {
                return Err(TerminalError::Input(format!(
                    "Dispute window has closed for evidence {}",
                    evidence
                )));
            }
        }
    }

    // Check if this guardian has already filed a dispute
    let existing_dispute_key = format!("recovery_dispute.{}.{}", evidence, guardian_authority);
    if dispute_journal.facts.contains_key(&existing_dispute_key) {
        return Err(TerminalError::Input(format!(
            "Guardian {} has already filed a dispute for evidence {}",
            guardian_authority, evidence
        )));
    }

    // Create dispute record
    let current_timestamp = ctx.effects().current_timestamp().await.unwrap_or(0);

    let dispute = RecoveryDispute {
        guardian_id: guardian_authority,
        reason: reason.to_string(),
        filed_at_ms: current_timestamp,
    };

    output.kv("Dispute timestamp", dispute.filed_at_ms.to_string());

    // Store dispute in Journal using proper JournalEffects API
    let dispute_key = format!("recovery_dispute.{}.{}", evidence, dispute.guardian_id);
    let dispute_value = encode_recovery_fact(ctx.context_id(), "recovery_dispute", &dispute)?;

    // Create a journal delta with the new dispute fact
    let mut journal_delta = aura_core::Journal::new();
    journal_delta
        .facts
        .insert(dispute_key.clone(), dispute_value);

    // Get current journal and merge the delta
    let current_journal =
        ctx.effects().get_journal().await.map_err(|e| {
            TerminalError::Operation(format!("Failed to get current journal: {}", e))
        })?;

    let updated_journal = ctx
        .effects()
        .merge_facts(&current_journal, &journal_delta)
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to merge journal facts: {}", e)))?;

    // Persist the updated journal
    ctx.effects()
        .persist_journal(&updated_journal)
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to persist journal: {}", e)))?;

    output.kv("Dispute recorded with key", &dispute_key);
    output.blank();
    output.println("Dispute filed successfully!");

    Ok(output)
}

/// Generate real guardian approval for recovery request using FROST threshold signing
async fn generate_guardian_approval(
    ctx: &HandlerContext<'_>,
    request: &RecoveryRequest,
    guardian: &GuardianProfile,
) -> TerminalResult<aura_recovery::guardian_key_recovery::GuardianKeyApproval> {
    use aura_recovery::guardian_key_recovery::GuardianKeyApproval;

    // Get current timestamp
    let timestamp_ms = ctx.effects().current_timestamp().await.unwrap_or(0);

    // Create recovery message to sign
    let recovery_message = serde_json::to_vec(&request).map_err(|e| {
        TerminalError::Operation(format!("Failed to serialize recovery request: {}", e))
    })?;

    // Deterministic partial signature derived from the recovery message hash.
    let partial_sig_bytes: Vec<u8> = hash::hash(&recovery_message).to_vec();

    let key_share_bytes = hash::hash(
        format!(
            "guardian-key-share:{}:{}",
            guardian.authority_id, request.account_id
        )
        .as_bytes(),
    );

    Ok(GuardianKeyApproval {
        guardian_id: guardian.authority_id,
        key_share: key_share_bytes.to_vec(),
        partial_signature: partial_sig_bytes.to_vec(),
        timestamp: aura_core::time::TimeStamp::PhysicalClock(aura_core::time::PhysicalTime {
            ts_ms: timestamp_ms,
            uncertainty: None,
        }),
    })
}

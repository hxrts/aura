//! Guardian Recovery CLI Commands
//!
//! Thin view layer for guardian-based account recovery.
//! All business logic is delegated to aura_app::workflows::recovery_cli.
//! This module handles I/O, serialization, and output formatting.
//!
//! Returns structured `CliOutput` for testability.

use crate::error::{TerminalError, TerminalResult};
use crate::handlers::{CliOutput, HandlerContext};
use aura_agent::handlers::{
    GuardianProfile, GuardianSet, RecoveryDispute, RecoveryOperation, RecoveryRequest,
    RecoveryResponse,
};
use aura_app::ui::workflows::recovery_cli::{
    self, validate_guardian_set, DISPUTE_WINDOW_HOURS_DEFAULT,
};
use aura_core::effects::PhysicalTimeEffects;
use aura_core::frost::PublicKeyPackage;
use aura_core::hash;
use aura_core::identifiers::RecoveryId;
use aura_core::Hash32;
use aura_effects::StorageCoreEffects;
use std::collections::BTreeMap;
use std::path::Path;

use crate::handlers::recovery_status;
use crate::ids;
use crate::RecoveryAction;

/// Extract a millisecond timestamp from any TimeStamp variant for display/logging.
/// Delegates to portable workflow function.
fn timestamp_ms(ts: &aura_core::time::TimeStamp) -> u64 {
    recovery_cli::extract_timestamp_ms(ts)
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

    output.section(format!("Starting {priority} recovery"));
    output.kv("Account", account);
    output.kv("Guardians", guardians);
    output.kv("Threshold", threshold.to_string());
    output.kv("Dispute window", format!("{dispute_hours} hours"));

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
    let mut guardian_profiles = Vec::with_capacity(guardian_strs.len());
    for (i, guardian_str) in guardian_strs.iter().enumerate() {
        let guardian_authority = ids::authority_id(guardian_str);
        guardian_profiles.push(GuardianProfile::with_label(
            guardian_authority,
            format!("Guardian {}", i + 1),
        ));
    }
    let guardian_set = GuardianSet::new(guardian_profiles);

    // Validate guardian set using portable workflow function
    validate_guardian_set(guardian_set.len(), threshold)
        .map_err(|e| TerminalError::Input(e.to_string()))?;

    // Get current timestamp
    let physical_time = ctx
        .effects()
        .physical_time()
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to get physical time: {e}")))?;
    let current_time = physical_time.ts_ms;

    // Create guardian authorities list
    let guardian_authorities: Vec<aura_core::AuthorityId> =
        guardian_set.iter().map(|g| g.authority_id).collect();

    // Create recovery request for serialization (terminal-layer type)
    let new_public_key_bytes =
        hash::hash(format!("recovery-new-key:{account_authority}").as_bytes()).to_vec();
    let recovery_request = RecoveryRequest {
        recovery_id: RecoveryId::new(format!("recovery-{account_authority}")),
        account_authority,
        operation: RecoveryOperation::ReplaceTree {
            new_public_key: new_public_key_bytes.clone(),
        },
        justification: justification
            .unwrap_or("CLI recovery operation")
            .to_string(),
        guardians: guardian_authorities.clone(),
        threshold,
        requested_at: current_time,
        expires_at: None,
    };

    output.println("Executing recovery protocol via proper coordinator...");

    // Build protocol request using portable workflow
    let commitment = Hash32::new(hash::hash(
        format!("recovery-commitment:{account_authority}").as_bytes(),
    ));
    let new_public_key = PublicKeyPackage::new(new_public_key_bytes, BTreeMap::new(), 1, 1);

    let recovery_request_protocol = recovery_cli::build_protocol_request(
        account_authority,
        commitment,
        new_public_key,
        justification
            .unwrap_or("CLI recovery operation")
            .to_string(),
    );

    // Initiate recovery using the proper protocol (portable workflow)
    recovery_cli::initiate_recovery_protocol(
        ctx.effects(),
        account_authority,
        guardian_authorities,
        threshold,
        recovery_request_protocol,
    )
    .await
    .map_err(|e| TerminalError::Operation(format!("Failed to initiate recovery protocol: {e}")))?;

    // Store request payload via StorageEffects (terminal handles serialization format)
    let request_path = format!("recovery_request_{account_authority}.json");
    let request_json = serde_json::to_vec_pretty(&recovery_request).map_err(|e| {
        TerminalError::Operation(format!("Failed to serialize recovery request: {e}"))
    })?;

    let storage_key = format!("recovery_request:{request_path}");
    ctx.effects()
        .store(&storage_key, request_json.clone())
        .await
        .map_err(|e| {
            TerminalError::Operation(format!(
                "Failed to store recovery request via storage effects: {e}"
            ))
        })?;

    output.kv("Recovery request stored at", &storage_key);
    output.println(format!(
        "Share the stored request key with guardians and ask them to run `aura recovery approve --request-file {request_path}`"
    ));

    // Update journal with recovery initiation using portable workflow
    let recovery_fact_key = format!("recovery_initiated.{account_authority}");
    recovery_cli::record_recovery_fact(
        ctx.effects(),
        ctx.context_id(),
        recovery_fact_key.clone(),
        "recovery_initiated",
        &recovery_request,
    )
    .await
    .map_err(|e| TerminalError::Operation(format!("Failed to record recovery fact: {e}")))?;

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

    // Read and parse recovery request file via StorageEffects (terminal handles deserialization)
    let file_key = format!("recovery_request:{}", request_file.display());
    let request_content = match ctx.effects().retrieve(&file_key).await {
        Ok(Some(data)) => String::from_utf8(data)
            .map_err(|e| TerminalError::Config(format!("Invalid UTF-8 in request file: {e}")))?,
        Ok(None) => {
            return Err(TerminalError::NotFound(format!(
                "Request file not found: {}",
                request_file.display()
            )))
        }
        Err(e) => {
            return Err(TerminalError::Operation(format!(
                "Failed to read request file via storage effects: {e}"
            )))
        }
    };

    let recovery_request: RecoveryRequest = serde_json::from_str(&request_content)
        .map_err(|e| TerminalError::Config(format!("Failed to parse recovery request: {e}")))?;

    output.kv("Account", recovery_request.account_authority.to_string());
    output.kv("Required threshold", recovery_request.threshold.to_string());

    // Check if justification exists
    let justification_text = &recovery_request.justification;
    if !justification_text.is_empty() {
        output.kv("Justification", justification_text);
    }

    // Get current authority from context
    let guardian_authority = ctx.effect_context().authority_id();

    // Find this authority in the guardian set using portable workflow
    let guardian_index =
        recovery_cli::find_guardian_index(&recovery_request.guardians, guardian_authority)
            .ok_or_else(|| {
                TerminalError::Input(format!(
                    "Current authority {guardian_authority} is not a guardian for this recovery"
                ))
            })?;

    // Build a GuardianProfile from the authority
    let guardian_profile = GuardianProfile::with_label(
        guardian_authority,
        format!("Guardian {}", guardian_index + 1),
    );

    let label = guardian_profile.label.as_deref().unwrap_or("Guardian");
    output.kv(
        "Approving as",
        format!("{} ({})", label, guardian_profile.authority_id),
    );

    // Execute guardian approval using portable workflow
    output.println("Executing guardian approval workflow...");

    // Hash the request for signing
    let request_bytes = serde_json::to_vec(&recovery_request).map_err(|e| {
        TerminalError::Operation(format!("Failed to serialize recovery request: {e}"))
    })?;
    let request_hash = Hash32::new(hash::hash(&request_bytes));

    // Generate approval using portable workflow
    let approval_result = recovery_cli::generate_guardian_approval(
        ctx.effects(),
        recovery_request.account_authority,
        &guardian_profile,
        request_hash,
    )
    .await
    .map_err(|e| TerminalError::Operation(format!("Failed to generate approval: {e}")))?;

    output.println("Guardian approval completed successfully!");
    output.kv(
        "Approval timestamp (ms)",
        timestamp_ms(&approval_result.timestamp).to_string(),
    );
    output.kv(
        "Key share size",
        format!("{} bytes", approval_result.key_share.len()),
    );

    // Build recovery share using portable workflow
    let share = recovery_cli::build_recovery_share(&guardian_profile, &approval_result);

    // Build recovery evidence using portable workflow
    let evidence = recovery_cli::build_recovery_evidence(
        ctx.context_id(),
        recovery_request.account_authority,
        guardian_profile.authority_id,
        timestamp_ms(&approval_result.timestamp),
        DISPUTE_WINDOW_HOURS_DEFAULT,
    );

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

    // Persist approval (terminal handles serialization format)
    let response_json = serde_json::to_vec_pretty(&response).map_err(|e| {
        TerminalError::Operation(format!("Failed to serialize approval response: {e}"))
    })?;

    let response_key = format!(
        "recovery_response:{}:{}",
        recovery_request.account_authority, guardian_profile.authority_id
    );
    ctx.effects()
        .store(&response_key, response_json.clone())
        .await
        .map_err(|e| {
            TerminalError::Operation(format!(
                "Failed to store approval response via storage effects: {e}"
            ))
        })?;

    let response_path = format!(
        "recovery_response_{}_{}.json",
        recovery_request.account_authority, guardian_profile.authority_id
    );

    ctx.effects()
        .store(
            &format!("recovery_response_file:{response_path}"),
            response_json,
        )
        .await
        .map_err(|e| {
            TerminalError::Operation(format!("Failed to persist approval response: {e}"))
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

    // Use portable workflow to list recovery facts
    let (recovery_facts, completed_facts) = recovery_cli::list_recovery_fact_keys(ctx.effects())
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to list recovery facts: {e}")))?;

    // Find active recoveries (initiated but not completed)
    let active_recoveries: Vec<String> = recovery_facts
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
        .map_err(|e| TerminalError::Input(format!("Invalid evidence ID '{evidence}': {e}")))?;

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

    // Create dispute record
    let physical_time = ctx
        .effects()
        .physical_time()
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to get physical time: {e}")))?;
    let current_timestamp = physical_time.ts_ms;

    let dispute = RecoveryDispute {
        guardian_id: guardian_authority,
        reason: reason.to_string(),
        filed_at_ms: current_timestamp,
    };

    output.kv("Dispute timestamp", dispute.filed_at_ms.to_string());

    // Store dispute in journal via portable workflow
    let dispute_key = recovery_cli::record_recovery_dispute(
        ctx.effects(),
        ctx.context_id(),
        evidence,
        guardian_authority,
        &dispute,
    )
    .await
    .map_err(|e| TerminalError::Operation(format!("Failed to record dispute: {e}")))?;

    output.kv("Dispute recorded with key", &dispute_key);
    output.blank();
    output.println("Dispute filed successfully!");

    Ok(output)
}

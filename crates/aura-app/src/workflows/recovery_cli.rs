//! Portable recovery workflow helpers (journal + protocol wiring).
//!
//! This module contains portable business logic for guardian recovery operations.
//! All functions use effect traits and domain types - no assumptions about
//! serialization formats or I/O mechanisms.
//!
//! Terminal handlers (CLI and TUI) delegate to these functions for ceremony logic.

use std::sync::Arc;

// Re-export threshold constants from central location
pub use crate::thresholds::{validate_guardian_set, MIN_GUARDIANS, MIN_THRESHOLD};

// ============================================================================
// Dispute Window Constants
// ============================================================================

/// Default dispute window in hours (48 hours).
///
/// This is the standard window during which guardians can dispute a recovery
/// before it is finalized. Used when no custom dispute window is specified.
pub const DISPUTE_WINDOW_HOURS_DEFAULT: u64 = 48;

/// Minimum dispute window in hours (1 hour).
///
/// Recovery ceremonies must have at least this long for guardians to respond.
pub const DISPUTE_WINDOW_HOURS_MIN: u64 = 1;

/// Maximum dispute window in hours (30 days = 720 hours).
///
/// Prevents indefinite recovery windows that could block account access.
pub const DISPUTE_WINDOW_HOURS_MAX: u64 = 720;

use crate::workflows::journal::{encode_relational_generic, persist_fact_value};
use aura_core::effects::{JournalEffects, NetworkEffects, PhysicalTimeEffects, TimeEffects};
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::time::{PhysicalTime, TimeStamp};
use aura_core::{hash, AuraError, FactValue, Hash32};
use aura_recovery::guardian_key_recovery::GuardianKeyApproval;
use aura_recovery::recovery_protocol::{
    RecoveryProtocol, RecoveryProtocolHandler, RecoveryRequest,
};
use aura_recovery::types::{GuardianProfile, RecoveryEvidence, RecoveryShare};
use aura_relational::RelationalContext;
use serde::Serialize;

// ============================================================================
// Guardian Set Validation (re-exported from thresholds)
// ============================================================================
// validate_guardian_set is re-exported from crate::thresholds

/// Validate dispute window duration.
///
/// # Arguments
/// * `hours` - Dispute window in hours
///
/// # Returns
/// Ok(hours) clamped to valid range, or the input if within bounds.
pub fn validate_dispute_window(hours: u64) -> Result<u64, AuraError> {
    if hours < DISPUTE_WINDOW_HOURS_MIN {
        return Err(AuraError::invalid(format!(
            "Dispute window must be at least {} hour(s), got {}",
            DISPUTE_WINDOW_HOURS_MIN, hours
        )));
    }

    if hours > DISPUTE_WINDOW_HOURS_MAX {
        return Err(AuraError::invalid(format!(
            "Dispute window cannot exceed {} hours ({} days), got {}",
            DISPUTE_WINDOW_HOURS_MAX,
            DISPUTE_WINDOW_HOURS_MAX / 24,
            hours
        )));
    }

    Ok(hours)
}

/// Check for duplicate guardians in a set.
///
/// Returns the first duplicate found, if any.
pub fn find_duplicate_guardian(guardians: &[AuthorityId]) -> Option<AuthorityId> {
    let mut seen = std::collections::HashSet::new();
    for guardian in guardians {
        if !seen.insert(*guardian) {
            return Some(*guardian);
        }
    }
    None
}

/// Comprehensive guardian set validation including duplicate check.
///
/// Combines `validate_guardian_set` with duplicate detection.
pub fn validate_guardian_set_full(
    guardians: &[AuthorityId],
    threshold: u32,
) -> Result<(), AuraError> {
    // Check for duplicates first
    if let Some(duplicate) = find_duplicate_guardian(guardians) {
        return Err(AuraError::invalid(format!(
            "Duplicate guardian in set: {}",
            duplicate
        )));
    }

    // Then validate counts
    validate_guardian_set(guardians.len(), threshold)
}

// ============================================================================
// Protocol Initiation
// ============================================================================

/// Run the recovery protocol initiation sequence.
pub async fn initiate_recovery_protocol<
    E: PhysicalTimeEffects + NetworkEffects + JournalEffects,
>(
    effects: &E,
    account_authority: AuthorityId,
    guardian_authorities: Vec<AuthorityId>,
    threshold: u32,
    request: RecoveryRequest,
) -> Result<(), AuraError> {
    let recovery_context = Arc::new(RelationalContext::new(guardian_authorities.clone()));
    let recovery_protocol = RecoveryProtocol::new(
        recovery_context,
        account_authority,
        guardian_authorities,
        threshold,
    );
    let protocol_handler = RecoveryProtocolHandler::new(Arc::new(recovery_protocol));
    protocol_handler
        .handle_recovery_initiation(request, effects, effects, effects)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to initiate recovery protocol: {e}")))?;
    Ok(())
}

/// Build a deterministic recovery request for protocol initiation.
pub fn build_protocol_request(
    account_authority: AuthorityId,
    commitment: Hash32,
    new_public_key: Vec<u8>,
    justification: String,
) -> RecoveryRequest {
    RecoveryRequest {
        recovery_id: account_authority.to_string(),
        account_authority,
        new_tree_commitment: commitment,
        operation: aura_recovery::recovery_protocol::RecoveryOperation::ReplaceTree {
            new_public_key,
        },
        justification,
    }
}

// ============================================================================
// Guardian Approval Logic
// ============================================================================

/// Generate guardian approval for a recovery request.
///
/// This produces a deterministic partial signature and key share
/// that the guardian contributes to the recovery ceremony.
///
/// # Arguments
/// * `effects` - Physical time effects for timestamping
/// * `account_authority` - The authority being recovered
/// * `guardian` - The guardian providing approval
/// * `request_hash` - Hash of the recovery request for signing
///
/// # Returns
/// A `GuardianKeyApproval` containing the key share and partial signature.
pub async fn generate_guardian_approval<E: PhysicalTimeEffects>(
    effects: &E,
    account_authority: AuthorityId,
    guardian: &GuardianProfile,
    request_hash: Hash32,
) -> Result<GuardianKeyApproval, AuraError> {
    let physical_time = effects
        .physical_time()
        .await
        .map_err(|e| AuraError::agent(format!("Failed to get physical time: {e}")))?;
    let timestamp_ms = physical_time.ts_ms;

    // Deterministic partial signature derived from the request hash
    let partial_sig_bytes: Vec<u8> = request_hash.as_bytes().to_vec();

    // Deterministic key share derived from guardian + account
    let key_share_bytes = hash::hash(
        format!(
            "guardian-key-share:{}:{}",
            guardian.authority_id, account_authority
        )
        .as_bytes(),
    );

    Ok(GuardianKeyApproval {
        guardian_id: guardian.authority_id,
        key_share: key_share_bytes.to_vec(),
        partial_signature: partial_sig_bytes,
        timestamp: TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: timestamp_ms,
            uncertainty: None,
        }),
    })
}

/// Build a recovery share from a guardian approval.
pub fn build_recovery_share(
    guardian: &GuardianProfile,
    approval: &GuardianKeyApproval,
) -> RecoveryShare {
    RecoveryShare {
        guardian_id: guardian.authority_id,
        guardian_label: guardian.label.clone(),
        share: approval.key_share.clone(),
        partial_signature: approval.partial_signature.clone(),
        issued_at_ms: extract_timestamp_ms(&approval.timestamp),
    }
}

/// Build recovery evidence for a guardian approval.
pub fn build_recovery_evidence(
    context_id: ContextId,
    account_authority: AuthorityId,
    approving_guardian: AuthorityId,
    approval_timestamp_ms: u64,
    dispute_window_hours: u64,
) -> RecoveryEvidence {
    RecoveryEvidence {
        context_id,
        account_id: account_authority,
        approving_guardians: vec![approving_guardian],
        completed_at_ms: approval_timestamp_ms,
        dispute_window_ends_at_ms: approval_timestamp_ms + dispute_window_hours * 3600 * 1000,
        disputes: Vec::new(),
        threshold_signature: None,
    }
}

// ============================================================================
// Journal Operations
// ============================================================================

/// Write a generic recovery fact into the journal.
pub async fn record_recovery_fact<T: Serialize, E: JournalEffects>(
    effects: &E,
    context_id: ContextId,
    fact_key: String,
    kind: &str,
    payload: &T,
) -> Result<(), AuraError> {
    let fact_value = encode_relational_generic(context_id, kind, payload)?;
    persist_fact_value(effects, fact_key, fact_value).await?;

    Ok(())
}

/// List recovery-related fact keys for status reporting.
pub async fn list_recovery_fact_keys<E: JournalEffects>(
    effects: &E,
) -> Result<(Vec<String>, Vec<String>), AuraError> {
    let current = effects
        .get_journal()
        .await
        .map_err(|e| AuraError::agent(format!("Failed to get journal: {e}")))?;

    let recovery_facts: Vec<String> = current
        .facts
        .keys()
        .filter(|key| {
            let key_str = key.as_str();
            key_str.contains("recovery") || key_str == "emergency_recovery_initiated"
        })
        .map(|key| key.as_str().to_string())
        .collect();
    let completed_facts: Vec<String> = current
        .facts
        .keys()
        .filter(|key| key.as_str() == "emergency_recovery_completed")
        .map(|key| key.as_str().to_string())
        .collect();

    Ok((recovery_facts, completed_facts))
}

/// Record a recovery dispute fact after validating the dispute window.
pub async fn record_recovery_dispute<T: Serialize, E: JournalEffects + TimeEffects>(
    effects: &E,
    context_id: ContextId,
    evidence_id: &str,
    guardian_authority: AuthorityId,
    dispute: &T,
) -> Result<String, AuraError> {
    let dispute_journal = effects
        .get_journal()
        .await
        .map_err(|e| AuraError::agent(format!("Failed to get journal: {e}")))?;

    let evidence_key = format!("recovery_evidence.{evidence_id}");
    if let Some(value) = dispute_journal.facts.get(&evidence_key) {
        let evidence_json: serde_json::Value = match value {
            FactValue::String(data) => serde_json::from_str(data),
            FactValue::Bytes(bytes) => serde_json::from_slice(bytes),
            _ => Ok(serde_json::Value::Null),
        }
        .map_err(|e| AuraError::agent(format!("Failed to parse evidence JSON: {e}")))?;

        if let Some(dispute_window_ends) = evidence_json
            .get("dispute_window_ends_at_ms")
            .and_then(|v| v.as_u64())
        {
            let current_time = effects.current_timestamp().await;
            if current_time > dispute_window_ends {
                return Err(AuraError::agent(format!(
                    "Dispute window has closed for evidence {evidence_id}"
                )));
            }
        }
    }

    let existing_dispute_key = format!("recovery_dispute.{evidence_id}.{guardian_authority}");
    if dispute_journal.facts.contains_key(&existing_dispute_key) {
        return Err(AuraError::agent(format!(
            "Guardian {guardian_authority} has already filed a dispute for evidence {evidence_id}"
        )));
    }

    let dispute_key = format!("recovery_dispute.{}.{}", evidence_id, guardian_authority);
    record_recovery_fact(
        effects,
        context_id,
        dispute_key.clone(),
        "recovery_dispute",
        dispute,
    )
    .await?;

    Ok(dispute_key)
}

// ============================================================================
// Utility Functions
// ============================================================================

/// Extract millisecond timestamp from any TimeStamp variant.
pub fn extract_timestamp_ms(ts: &TimeStamp) -> u64 {
    match ts {
        TimeStamp::PhysicalClock(p) => p.ts_ms,
        TimeStamp::LogicalClock(l) => l.lamport,
        TimeStamp::OrderClock(o) => {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(&o.0[..8]);
            u64::from_be_bytes(buf)
        }
        TimeStamp::Range(r) => r.latest_ms(),
    }
}

/// Find guardian index in a list of guardian authorities.
pub fn find_guardian_index(
    guardians: &[AuthorityId],
    guardian_authority: AuthorityId,
) -> Option<usize> {
    guardians.iter().position(|g| *g == guardian_authority)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a unique test AuthorityId from a seed byte.
    fn test_authority(seed: u8) -> AuthorityId {
        // Use unique pattern: seed repeated, but offset by position
        let mut entropy = [0u8; 32];
        for (i, byte) in entropy.iter_mut().enumerate() {
            *byte = seed.wrapping_add(i as u8);
        }
        AuthorityId::new_from_entropy(entropy)
    }

    // Note: validate_guardian_set tests are in crate::thresholds

    #[test]
    fn test_validate_dispute_window_valid() {
        assert_eq!(validate_dispute_window(1).unwrap(), 1);
        assert_eq!(validate_dispute_window(48).unwrap(), 48);
        assert_eq!(validate_dispute_window(720).unwrap(), 720);
    }

    #[test]
    fn test_validate_dispute_window_too_short() {
        let err = validate_dispute_window(0).unwrap_err();
        assert!(err.to_string().contains("at least 1 hour"));
    }

    #[test]
    fn test_validate_dispute_window_too_long() {
        let err = validate_dispute_window(721).unwrap_err();
        assert!(err.to_string().contains("cannot exceed 720 hours"));
    }

    #[test]
    fn test_find_duplicate_guardian_none() {
        let g1 = test_authority(10);
        let g2 = test_authority(20);
        let g3 = test_authority(30);

        assert!(find_duplicate_guardian(&[g1, g2, g3]).is_none());
    }

    #[test]
    fn test_find_duplicate_guardian_found() {
        let g1 = test_authority(10);
        let g2 = test_authority(20);

        // g1 appears twice
        let dup = find_duplicate_guardian(&[g1, g2, g1]);
        assert_eq!(dup, Some(g1));
    }

    #[test]
    fn test_validate_guardian_set_full_with_duplicates() {
        let g1 = test_authority(10);
        let g2 = test_authority(20);

        let err = validate_guardian_set_full(&[g1, g2, g1], 2).unwrap_err();
        assert!(err.to_string().contains("Duplicate guardian"));
    }

    #[test]
    fn test_validate_guardian_set_full_valid() {
        let g1 = test_authority(10);
        let g2 = test_authority(20);
        let g3 = test_authority(30);

        assert!(validate_guardian_set_full(&[g1, g2, g3], 2).is_ok());
    }

    #[test]
    fn test_find_guardian_index_found() {
        let g1 = test_authority(10);
        let g2 = test_authority(20);
        let g3 = test_authority(30);

        assert_eq!(find_guardian_index(&[g1, g2, g3], g1), Some(0));
        assert_eq!(find_guardian_index(&[g1, g2, g3], g2), Some(1));
        assert_eq!(find_guardian_index(&[g1, g2, g3], g3), Some(2));
    }

    #[test]
    fn test_find_guardian_index_not_found() {
        let g1 = test_authority(10);
        let g2 = test_authority(20);
        let g3 = test_authority(30);

        assert_eq!(find_guardian_index(&[g1, g2], g3), None);
    }

    #[test]
    fn test_constants() {
        assert_eq!(DISPUTE_WINDOW_HOURS_DEFAULT, 48);
        assert_eq!(DISPUTE_WINDOW_HOURS_MIN, 1);
        assert_eq!(DISPUTE_WINDOW_HOURS_MAX, 720);
        assert_eq!(MIN_GUARDIANS, 2);
        assert_eq!(MIN_THRESHOLD, 2);
    }
}

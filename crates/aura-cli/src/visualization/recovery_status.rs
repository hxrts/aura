//! Rich visualization for recovery state in CLI.

use aura_recovery::types::RecoveryEvidence;
use aura_recovery::{RecoverySessionState, RecoverySessionStatus};
use std::fmt::Write;
use std::time::SystemTime;

/// Format recovery evidence for CLI display
pub fn format_recovery_evidence(evidence: &RecoveryEvidence) -> String {
    let mut output = String::new();

    writeln!(
        &mut output,
        "╔══════════════════════════════════════════════════════════════╗"
    )
    .unwrap();
    writeln!(
        &mut output,
        "║                    Recovery Evidence                        ║"
    )
    .unwrap();
    writeln!(
        &mut output,
        "╠══════════════════════════════════════════════════════════════╣"
    )
    .unwrap();
    writeln!(
        &mut output,
        "║ Account ID:        {}",
        format_field(&evidence.account_id.to_string(), 39)
    )
    .unwrap();
    writeln!(
        &mut output,
        "║ Recovering Device: {}",
        format_field(
            &evidence.recovering_device.to_string()
                [..16.min(evidence.recovering_device.to_string().len())],
            39
        )
    )
    .unwrap();
    writeln!(
        &mut output,
        "║ Issued At:         {}",
        format_field(&format_timestamp(evidence.issued_at), 39)
    )
    .unwrap();
    writeln!(
        &mut output,
        "║ Guardians:         {} approvals",
        format_field(&evidence.guardians.len().to_string(), 28)
    )
    .unwrap();

    if !evidence.disputes.is_empty() {
        writeln!(
            &mut output,
            "║ Disputes:          {} filed",
            format_field(&evidence.disputes.len().to_string(), 33)
        )
        .unwrap();
    }

    writeln!(
        &mut output,
        "╠══════════════════════════════════════════════════════════════╣"
    )
    .unwrap();
    writeln!(&mut output, "║ Timeline:").unwrap();

    let dispute_ends = format_timestamp(evidence.dispute_window_ends_at);
    let cooldown_ends = format_timestamp(evidence.cooldown_expires_at);

    writeln!(
        &mut output,
        "║   • Dispute window closes: {}",
        format_field(&dispute_ends, 31)
    )
    .unwrap();
    writeln!(
        &mut output,
        "║   • Guardian cooldown expires: {}",
        format_field(&cooldown_ends, 27)
    )
    .unwrap();

    if !evidence.disputes.is_empty() {
        writeln!(
            &mut output,
            "╠══════════════════════════════════════════════════════════════╣"
        )
        .unwrap();
        writeln!(&mut output, "║ Disputes:").unwrap();
        for (idx, dispute) in evidence.disputes.iter().enumerate() {
            writeln!(
                &mut output,
                "║   {}. Guardian {} - \"{}\"",
                idx + 1,
                &dispute.guardian_id.to_string()[..8.min(dispute.guardian_id.to_string().len())],
                &dispute.reason[..40.min(dispute.reason.len())]
            )
            .unwrap();
        }
    }

    writeln!(
        &mut output,
        "╚══════════════════════════════════════════════════════════════╝"
    )
    .unwrap();

    output
}

/// Format recovery session state for CLI display
pub fn format_session_state(session: &RecoverySessionState) -> String {
    let mut output = String::new();

    let status_symbol = match &session.status {
        RecoverySessionStatus::Pending => "⏳",
        RecoverySessionStatus::InDisputeWindow => "⚠️ ",
        RecoverySessionStatus::Completed => "✅",
        RecoverySessionStatus::Cancelled { .. } => "❌",
        RecoverySessionStatus::Failed { .. } => "💥",
    };

    let status_text = match &session.status {
        RecoverySessionStatus::Pending => "Pending Guardian Approvals".to_string(),
        RecoverySessionStatus::InDisputeWindow => "In Dispute Window".to_string(),
        RecoverySessionStatus::Completed => "Completed Successfully".to_string(),
        RecoverySessionStatus::Cancelled { reason } => format!("Cancelled: {}", reason),
        RecoverySessionStatus::Failed { error } => format!("Failed: {}", error),
    };

    writeln!(
        &mut output,
        "┌─────────────────────────────────────────────────────┐"
    )
    .unwrap();
    writeln!(
        &mut output,
        "│ {} Recovery Session                                │",
        status_symbol
    )
    .unwrap();
    writeln!(
        &mut output,
        "├─────────────────────────────────────────────────────┤"
    )
    .unwrap();
    writeln!(&mut output, "│ Status: {}", format_field(&status_text, 42)).unwrap();
    writeln!(
        &mut output,
        "│ Device: {}",
        format_field(
            &session.requesting_device.to_string()
                [..16.min(session.requesting_device.to_string().len())],
            42
        )
    )
    .unwrap();
    writeln!(
        &mut output,
        "│ Created: {}",
        format_field(&format_timestamp(session.created_at), 41)
    )
    .unwrap();
    writeln!(
        &mut output,
        "│ Updated: {}",
        format_field(&format_timestamp(session.updated_at), 41)
    )
    .unwrap();

    writeln!(
        &mut output,
        "└─────────────────────────────────────────────────────┘"
    )
    .unwrap();

    output
}

/// Format multiple recovery sessions as a list
pub fn format_session_list(sessions: &[RecoverySessionState]) -> String {
    if sessions.is_empty() {
        return "No active recovery sessions found.".to_string();
    }

    let mut output = String::new();
    writeln!(
        &mut output,
        "\n📋 Active Recovery Sessions ({}):",
        sessions.len()
    )
    .unwrap();
    writeln!(&mut output).unwrap();

    for (idx, session) in sessions.iter().enumerate() {
        writeln!(&mut output, "Session {}:", idx + 1).unwrap();
        output.push_str(&format_session_state(session));
        writeln!(&mut output).unwrap();
    }

    output
}

/// Format evidence list with summary stats
pub fn format_evidence_list(evidence_list: &[RecoveryEvidence]) -> String {
    if evidence_list.is_empty() {
        return "No recovery evidence found.".to_string();
    }

    let mut output = String::new();
    let total_disputes: usize = evidence_list.iter().map(|e| e.disputes.len()).sum();
    let total_guardians: usize = evidence_list.iter().map(|e| e.guardians.len()).sum();

    writeln!(
        &mut output,
        "\n📜 Recovery History ({} records):",
        evidence_list.len()
    )
    .unwrap();
    writeln!(
        &mut output,
        "   Total guardian approvals: {}, Total disputes: {}\n",
        total_guardians, total_disputes
    )
    .unwrap();

    for (idx, evidence) in evidence_list.iter().take(10).enumerate() {
        writeln!(&mut output, "Record {}:", idx + 1).unwrap();
        output.push_str(&format_recovery_evidence(evidence));
        writeln!(&mut output).unwrap();
    }

    if evidence_list.len() > 10 {
        writeln!(
            &mut output,
            "... and {} more records (showing most recent 10)",
            evidence_list.len() - 10
        )
        .unwrap();
    }

    output
}

/// Format a dashboard view with all recovery information
pub fn format_recovery_dashboard(
    sessions: &[RecoverySessionState],
    evidence_list: &[RecoveryEvidence],
    pending_count: usize,
    total_disputes: usize,
) -> String {
    let mut output = String::new();

    writeln!(
        &mut output,
        "╔══════════════════════════════════════════════════════════════╗"
    )
    .unwrap();
    writeln!(
        &mut output,
        "║              Guardian Recovery Dashboard                     ║"
    )
    .unwrap();
    writeln!(
        &mut output,
        "╠══════════════════════════════════════════════════════════════╣"
    )
    .unwrap();
    writeln!(
        &mut output,
        "║ Active Sessions:    {}",
        format_field(&sessions.len().to_string(), 39)
    )
    .unwrap();
    writeln!(
        &mut output,
        "║ Pending Approvals:  {}",
        format_field(&pending_count.to_string(), 39)
    )
    .unwrap();
    writeln!(
        &mut output,
        "║ Total Disputes:     {}",
        format_field(&total_disputes.to_string(), 39)
    )
    .unwrap();
    writeln!(
        &mut output,
        "║ Evidence Records:   {}",
        format_field(&evidence_list.len().to_string(), 39)
    )
    .unwrap();
    writeln!(
        &mut output,
        "╚══════════════════════════════════════════════════════════════╝"
    )
    .unwrap();
    writeln!(&mut output).unwrap();

    if !sessions.is_empty() {
        output.push_str(&format_session_list(sessions));
    }

    if !evidence_list.is_empty() && evidence_list.len() <= 3 {
        output.push_str(&format_evidence_list(evidence_list));
    } else if !evidence_list.is_empty() {
        writeln!(
            &mut output,
            "Use 'aura recovery history' to view {} evidence records.",
            evidence_list.len()
        )
        .unwrap();
    }

    output
}

/// Helper to format field values with proper padding
fn format_field(value: &str, max_width: usize) -> String {
    if value.len() >= max_width {
        format!("{}...║", &value[..max_width.saturating_sub(3)])
    } else {
        format!("{:<width$}║", value, width = max_width)
    }
}

/// Format unix timestamp as human-readable string
fn format_timestamp(ts: u64) -> String {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    let datetime = UNIX_EPOCH + Duration::from_secs(ts);
    let now = SystemTime::now();

    if let Ok(duration_since) = now.duration_since(datetime) {
        let secs = duration_since.as_secs();
        if secs < 60 {
            return format!("{} seconds ago", secs);
        } else if secs < 3600 {
            return format!("{} minutes ago", secs / 60);
        } else if secs < 86400 {
            return format!("{} hours ago", secs / 3600);
        } else {
            return format!("{} days ago", secs / 86400);
        }
    }

    // Future timestamp or formatting fallback
    format!("timestamp: {}", ts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_field() {
        assert_eq!(format_field("test", 10), "test      ║");
        assert_eq!(format_field("very long text here", 10), "very lo...║");
    }

    #[test]
    fn test_format_timestamp() {
        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let recent = format_timestamp(now - 30);
        assert!(recent.contains("seconds ago"));

        let hours_ago = format_timestamp(now - 7200);
        assert!(hours_ago.contains("hours ago"));
    }
}

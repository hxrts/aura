//! Rich visualization for recovery state in CLI.

#![allow(clippy::disallowed_methods)]

use aura_recovery::types::RecoveryEvidence;
use std::fmt::Write;

/// Format recovery evidence for CLI display
pub fn format_recovery_evidence(evidence: &RecoveryEvidence) -> String {
    let mut output = String::new();

    // Writing to String cannot fail, so we use the _ pattern to ignore Results
    let _ = writeln!(
        &mut output,
        "╔══════════════════════════════════════════════════════════════╗"
    );
    let _ = writeln!(
        &mut output,
        "║                    Recovery Evidence                        ║"
    );
    let _ = writeln!(
        &mut output,
        "╠══════════════════════════════════════════════════════════════╣"
    );
    let _ = writeln!(
        &mut output,
        "║ Account ID:        {}",
        format_field(&evidence.account_id.to_string(), 39)
    );
    let _ = writeln!(
        &mut output,
        "║ Recovering Device: {}",
        format_field(
            &evidence.recovering_device.to_string()
                [..16.min(evidence.recovering_device.to_string().len())],
            39
        )
    );
    let _ = writeln!(
        &mut output,
        "║ Issued At:         {}",
        format_field(&format_timestamp(evidence.issued_at), 39)
    );
    let _ = writeln!(
        &mut output,
        "║ Guardians:         {} approvals",
        format_field(&evidence.guardians.len().to_string(), 28)
    );

    if !evidence.disputes.is_empty() {
        let _ = writeln!(
            &mut output,
            "║ Disputes:          {} filed",
            format_field(&evidence.disputes.len().to_string(), 33)
        );
    }

    let _ = writeln!(
        &mut output,
        "╠══════════════════════════════════════════════════════════════╣"
    );
    let _ = writeln!(&mut output, "║ Timeline:");

    let dispute_ends = format_timestamp(evidence.dispute_window_ends_at);
    let cooldown_ends = format_timestamp(evidence.cooldown_expires_at);

    let _ = writeln!(
        &mut output,
        "║   • Dispute window closes: {}",
        format_field(&dispute_ends, 31)
    );
    let _ = writeln!(
        &mut output,
        "║   • Guardian cooldown expires: {}",
        format_field(&cooldown_ends, 27)
    );

    if !evidence.disputes.is_empty() {
        let _ = writeln!(
            &mut output,
            "╠══════════════════════════════════════════════════════════════╣"
        );
        let _ = writeln!(&mut output, "║ Disputes:");
        for (idx, dispute) in evidence.disputes.iter().enumerate() {
            let _ = writeln!(
                &mut output,
                "║   {}. Guardian {} - \"{}\"",
                idx + 1,
                &dispute.guardian_id.to_string()[..8.min(dispute.guardian_id.to_string().len())],
                &dispute.reason[..40.min(dispute.reason.len())]
            );
        }
    }

    let _ = writeln!(
        &mut output,
        "╚══════════════════════════════════════════════════════════════╝"
    );

    output
}

/* TODO: Re-enable when RecoverySessionState and RecoverySessionStatus are available
/// Format recovery session state for CLI display
pub fn format_session_state(session: &RecoverySessionState) -> String {
    // Implementation commented out until types are available
    "Recovery session formatting not yet implemented".to_string()
}
*/

/* TODO: Re-enable when RecoverySessionState is available
/// Format multiple recovery sessions as a list
pub fn format_session_list(sessions: &[RecoverySessionState]) -> String {
    "Session list formatting not yet implemented".to_string()
}
*/

/// Format evidence list with summary stats
pub fn format_evidence_list(evidence_list: &[RecoveryEvidence]) -> String {
    if evidence_list.is_empty() {
        return "No recovery evidence found.".to_string();
    }

    let mut output = String::new();
    let total_disputes: usize = evidence_list.iter().map(|e| e.disputes.len()).sum();
    let total_guardians: usize = evidence_list.iter().map(|e| e.guardians.len()).sum();

    let _ = writeln!(
        &mut output,
        "\n📜 Recovery History ({} records):",
        evidence_list.len()
    );
    let _ = writeln!(
        &mut output,
        "   Total guardian approvals: {}, Total disputes: {}\n",
        total_guardians, total_disputes
    );

    for (idx, evidence) in evidence_list.iter().take(10).enumerate() {
        let _ = writeln!(&mut output, "Record {}:", idx + 1);
        output.push_str(&format_recovery_evidence(evidence));
        let _ = writeln!(&mut output);
    }

    if evidence_list.len() > 10 {
        let _ = writeln!(
            &mut output,
            "... and {} more records (showing most recent 10)",
            evidence_list.len() - 10
        );
    }

    output
}

/// Format a dashboard view with all recovery information
pub fn format_recovery_dashboard(
    _pending_count: usize,
    evidence_list: &[RecoveryEvidence],
    total_disputes: usize,
) -> String {
    let mut output = String::new();

    let _ = writeln!(
        &mut output,
        "╔══════════════════════════════════════════════════════════════╗"
    );
    let _ = writeln!(
        &mut output,
        "║              Guardian Recovery Dashboard                     ║"
    );
    let _ = writeln!(
        &mut output,
        "╠══════════════════════════════════════════════════════════════╣"
    );
    let _ = writeln!(
        &mut output,
        "║ Total Disputes:     {}",
        format_field(&total_disputes.to_string(), 39)
    );
    let _ = writeln!(
        &mut output,
        "║ Evidence Records:   {}",
        format_field(&evidence_list.len().to_string(), 39)
    );
    let _ = writeln!(
        &mut output,
        "╚══════════════════════════════════════════════════════════════╝"
    );
    let _ = writeln!(&mut output);

    if !evidence_list.is_empty() && evidence_list.len() <= 3 {
        output.push_str(&format_evidence_list(evidence_list));
    } else if !evidence_list.is_empty() {
        let _ = writeln!(
            &mut output,
            "Use 'aura recovery history' to view {} evidence records.",
            evidence_list.len()
        );
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
    use std::time::SystemTime;

    #[test]
    fn test_format_field() {
        assert_eq!(format_field("test", 10), "test      ║");
        assert_eq!(format_field("very long text here", 10), "very lo...║");
    }

    #[test]
    fn test_format_timestamp() {
        let now = match SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            Ok(duration) => duration.as_secs(),
            Err(_) => return, // Skip test if system time is invalid
        };

        let recent = format_timestamp(now - 30);
        assert!(recent.contains("seconds ago"));

        let hours_ago = format_timestamp(now - 7200);
        assert!(hours_ago.contains("hours ago"));
    }
}

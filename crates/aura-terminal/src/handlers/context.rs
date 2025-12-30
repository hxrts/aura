//! Context inspection handlers.
//!
//! Returns structured `CliOutput` for testability.

use crate::cli::context::ContextAction;
use crate::error::{TerminalError, TerminalResult};
use crate::handlers::{CliOutput, HandlerContext};
use aura_core::hash;
use aura_core::types::Epoch;
use serde::Deserialize;
use std::fs;
use std::path::Path;

/// Handle context debugging commands.
///
/// Returns `CliOutput` instead of printing directly.
///
/// **Standardized Signature (Task 2.2)**: Uses `HandlerContext` for unified parameter passing.
#[allow(clippy::unused_async)]
pub async fn handle_context(
    ctx: &HandlerContext<'_>,
    action: &ContextAction,
) -> TerminalResult<CliOutput> {
    match action {
        ContextAction::Inspect {
            context,
            state_file,
        } => inspect_context(ctx, context, state_file),
        ContextAction::Receipts {
            context,
            state_file,
            detailed,
        } => show_receipts(ctx, context, state_file, *detailed),
    }
}

fn inspect_context(
    _ctx: &HandlerContext<'_>,
    context: &str,
    state_file: &Path,
) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    let snapshot = load_context(state_file, context)?;
    let headroom = snapshot
        .flow_budget
        .limit
        .saturating_sub(snapshot.flow_budget.spent);

    output.kv("Context", &snapshot.context);
    output.kv(
        "Flow budget",
        format!(
            "limit={} spent={} headroom={} epoch={}",
            snapshot.flow_budget.limit,
            snapshot.flow_budget.spent,
            headroom,
            snapshot.flow_budget.epoch.value()
        ),
    );

    output.println(format!(
        "Rendezvous envelopes: {} (showing up to 5)",
        snapshot.rendezvous_envelopes.len()
    ));
    for env in snapshot.rendezvous_envelopes.iter().take(5) {
        output.println(format!(
            "  - {:?} via {:?} (last_seen: {})",
            env.role.as_deref().unwrap_or("unknown"),
            env.transport.as_deref().unwrap_or("n/a"),
            env.last_seen.as_deref().unwrap_or("n/a")
        ));
    }

    output.println(format!(
        "Channels: {} active (showing up to 5)",
        snapshot.channels.len()
    ));
    for chan in snapshot.channels.iter().take(5) {
        let anonymized = anonymize(&chan.peer);
        output.println(format!(
            "  - peer={} state={} headroom={} last_event={}",
            anonymized,
            chan.state.as_deref().unwrap_or("unknown"),
            chan.headroom.unwrap_or_default(),
            chan.last_event.as_deref().unwrap_or("n/a")
        ));
    }

    Ok(output)
}

fn show_receipts(
    _ctx: &HandlerContext<'_>,
    context: &str,
    state_file: &Path,
    detailed: bool,
) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    let snapshot = load_context(state_file, context)?;
    if snapshot.receipts.is_empty() {
        output.println(format!(
            "No receipts recorded for context {}",
            snapshot.context
        ));
        return Ok(output);
    }

    output.section(format!(
        "Receipts for context {} ({} total)",
        snapshot.context,
        snapshot.receipts.len()
    ));

    for receipt in &snapshot.receipts {
        let hop = anonymize(&receipt.hop);
        if detailed {
            output.println(format!(
                "  - hop={} cost={} epoch={} status={} chains={}",
                hop,
                receipt.cost,
                receipt.epoch.value(),
                receipt.status.as_deref().unwrap_or("unknown"),
                receipt.chain_hash.as_deref().unwrap_or("n/a")
            ));
        } else {
            output.println(format!(
                "  - hop={} cost={} status={}",
                hop,
                receipt.cost,
                receipt.status.as_deref().unwrap_or("unknown")
            ));
        }
    }

    Ok(output)
}

fn load_context(state_file: &Path, context: &str) -> TerminalResult<ContextSnapshot> {
    let data = fs::read_to_string(state_file)
        .map_err(|e| TerminalError::Config(format!("failed to read {state_file:?}: {e}")))?;
    let file: ContextStateFile = serde_json::from_str(&data)
        .map_err(|e| TerminalError::Config(format!("invalid JSON in {state_file:?}: {e}")))?;
    let ctx = normalize(context);
    file.contexts
        .into_iter()
        .find(|entry| normalize(&entry.context) == ctx)
        .ok_or_else(|| {
            TerminalError::NotFound(format!("context {context} not found in {state_file:?}"))
        })
}

fn normalize(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn anonymize(value: &str) -> String {
    let mut hasher = hash::hasher();
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    let hex = hex::encode(&digest[..12]);
    format!("anon-{}", &hex[..12])
}

#[derive(Debug, Deserialize)]
struct ContextStateFile {
    #[serde(default)]
    contexts: Vec<ContextSnapshot>,
}

#[derive(Debug, Deserialize)]
struct ContextSnapshot {
    pub context: String,
    #[serde(default)]
    pub rendezvous_envelopes: Vec<RendezvousEnvelopeDebug>,
    #[serde(default)]
    pub channels: Vec<ChannelDebug>,
    #[serde(default)]
    pub flow_budget: FlowBudgetDebug,
    #[serde(default)]
    pub receipts: Vec<ReceiptDebug>,
}

#[derive(Debug, Default, Deserialize)]
struct RendezvousEnvelopeDebug {
    #[allow(dead_code)]
    pub envelope_id: Option<String>,
    pub role: Option<String>,
    pub transport: Option<String>,
    pub last_seen: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ChannelDebug {
    pub peer: String,
    pub state: Option<String>,
    pub headroom: Option<u64>,
    pub last_event: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct FlowBudgetDebug {
    pub limit: u64,
    pub spent: u64,
    pub epoch: Epoch,
}

#[derive(Debug, Default, Deserialize)]
struct ReceiptDebug {
    pub hop: String,
    pub cost: u32,
    pub epoch: Epoch,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub chain_hash: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_trims_and_lowercases() {
        assert_eq!(normalize("  HELLO  "), "hello");
        assert_eq!(normalize("World"), "world");
        assert_eq!(normalize(""), "");
    }

    #[test]
    fn test_anonymize_produces_consistent_output() {
        let result1 = anonymize("test-peer-id");
        let result2 = anonymize("test-peer-id");
        assert_eq!(result1, result2);
        assert!(result1.starts_with("anon-"));
        assert_eq!(result1.len(), 17); // "anon-" + 12 hex chars
    }

    #[test]
    fn test_anonymize_different_inputs_produce_different_outputs() {
        let result1 = anonymize("peer-a");
        let result2 = anonymize("peer-b");
        assert_ne!(result1, result2);
    }

    #[test]
    fn test_load_context_parses_valid_json() {
        use std::io::Write;
        let temp_dir = tempfile::tempdir().unwrap();
        let state_file = temp_dir.path().join("state.json");

        let json = r#"{
            "contexts": [
                {
                    "context": "test-context",
                    "flow_budget": { "limit": 100, "spent": 20, "epoch": 1 },
                    "rendezvous_envelopes": [],
                    "channels": [],
                    "receipts": []
                }
            ]
        }"#;

        let mut file = fs::File::create(&state_file).unwrap();
        file.write_all(json.as_bytes()).unwrap();

        let snapshot = load_context(&state_file, "test-context").unwrap();
        assert_eq!(snapshot.context, "test-context");
        assert_eq!(snapshot.flow_budget.limit, 100);
        assert_eq!(snapshot.flow_budget.spent, 20);
    }

    #[test]
    fn test_load_context_case_insensitive() {
        use std::io::Write;
        let temp_dir = tempfile::tempdir().unwrap();
        let state_file = temp_dir.path().join("state.json");

        let json = r#"{
            "contexts": [
                {
                    "context": "MyContext",
                    "flow_budget": { "limit": 50, "spent": 10, "epoch": 2 },
                    "rendezvous_envelopes": [],
                    "channels": [],
                    "receipts": []
                }
            ]
        }"#;

        let mut file = fs::File::create(&state_file).unwrap();
        file.write_all(json.as_bytes()).unwrap();

        // Should find regardless of case
        let snapshot = load_context(&state_file, "mycontext").unwrap();
        assert_eq!(snapshot.context, "MyContext");
    }

    #[test]
    fn test_load_context_not_found() {
        use std::io::Write;
        let temp_dir = tempfile::tempdir().unwrap();
        let state_file = temp_dir.path().join("state.json");

        let json = r#"{ "contexts": [] }"#;

        let mut file = fs::File::create(&state_file).unwrap();
        file.write_all(json.as_bytes()).unwrap();

        let result = load_context(&state_file, "nonexistent");
        assert!(result.is_err());
    }
}

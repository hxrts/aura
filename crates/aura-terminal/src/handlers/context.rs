//! Context inspection handlers.
//!
//! Returns structured `CliOutput` for testability.

use crate::cli::context::ContextAction;
use crate::handlers::{CliOutput, HandlerContext};
use anyhow::{Context as AnyhowContext, Result};
use aura_core::hash;
use serde::Deserialize;
use std::fs;
use std::path::Path;

/// Handle context debugging commands.
///
/// Returns `CliOutput` instead of printing directly.
///
/// **Standardized Signature (Task 2.2)**: Uses `HandlerContext` for unified parameter passing.
pub async fn handle_context(ctx: &HandlerContext<'_>, action: &ContextAction) -> Result<CliOutput> {
    match action {
        ContextAction::Inspect {
            context,
            state_file,
        } => inspect_context(ctx, context, state_file).await,
        ContextAction::Receipts {
            context,
            state_file,
            detailed,
        } => show_receipts(ctx, context, state_file, *detailed).await,
    }
}

async fn inspect_context(
    _ctx: &HandlerContext<'_>,
    context: &str,
    state_file: &Path,
) -> Result<CliOutput> {
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
            snapshot.flow_budget.epoch
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

async fn show_receipts(
    _ctx: &HandlerContext<'_>,
    context: &str,
    state_file: &Path,
    detailed: bool,
) -> Result<CliOutput> {
    let mut output = CliOutput::new();

    let snapshot = load_context(state_file, context)?;
    if snapshot.receipts.is_empty() {
        output.println(format!(
            "No receipts recorded for context {}",
            snapshot.context
        ));
        return Ok(output);
    }

    output.section(&format!(
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
                receipt.epoch,
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

fn load_context(state_file: &Path, context: &str) -> Result<ContextSnapshot> {
    let data = fs::read_to_string(state_file)
        .with_context(|| format!("failed to read {:?}", state_file))?;
    let file: ContextStateFile =
        serde_json::from_str(&data).with_context(|| format!("invalid JSON in {:?}", state_file))?;
    let ctx = normalize(context);
    file.contexts
        .into_iter()
        .find(|entry| normalize(&entry.context) == ctx)
        .with_context(|| format!("context {} not found in {:?}", context, state_file))
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
    pub epoch: u64,
}

#[derive(Debug, Default, Deserialize)]
struct ReceiptDebug {
    pub hop: String,
    pub cost: u32,
    pub epoch: u64,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub chain_hash: Option<String>,
}

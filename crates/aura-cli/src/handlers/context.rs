//! Context inspection handlers.

use crate::commands::context::ContextAction;
use anyhow::{bail, Context as AnyhowContext, Result};
use blake3::Hasher;
use serde::Deserialize;
use std::fs;
use std::path::Path;

/// Handle context debugging commands.
pub async fn handle_context(action: &ContextAction) -> Result<()> {
    match action {
        ContextAction::Inspect {
            context,
            state_file,
        } => inspect_context(context, state_file).await,
        ContextAction::Receipts {
            context,
            state_file,
            detailed,
        } => show_receipts(context, state_file, *detailed).await,
    }
}

async fn inspect_context(context: &str, state_file: &Path) -> Result<()> {
    let snapshot = load_context(state_file, context)?;
    let headroom = snapshot
        .flow_budget
        .limit
        .saturating_sub(snapshot.flow_budget.spent);

    println!("Context: {}", snapshot.context);
    println!(
        "Flow budget: limit={} spent={} headroom={} epoch={}",
        snapshot.flow_budget.limit,
        snapshot.flow_budget.spent,
        headroom,
        snapshot.flow_budget.epoch
    );
    println!(
        "Rendezvous envelopes: {} (showing up to 5)",
        snapshot.rendezvous_envelopes.len()
    );
    for env in snapshot.rendezvous_envelopes.iter().take(5) {
        println!(
            "- {:?} via {:?} (last_seen: {})",
            env.role.as_deref().unwrap_or("unknown"),
            env.transport.as_deref().unwrap_or("n/a"),
            env.last_seen.as_deref().unwrap_or("n/a")
        );
    }
    println!(
        "Channels: {} active (showing up to 5)",
        snapshot.channels.len()
    );
    for chan in snapshot.channels.iter().take(5) {
        let anonymized = anonymize(&chan.peer);
        println!(
            "- peer={} state={} headroom={} last_event={}",
            anonymized,
            chan.state.as_deref().unwrap_or("unknown"),
            chan.headroom.unwrap_or_default(),
            chan.last_event.as_deref().unwrap_or("n/a")
        );
    }
    Ok(())
}

async fn show_receipts(context: &str, state_file: &Path, detailed: bool) -> Result<()> {
    let snapshot = load_context(state_file, context)?;
    if snapshot.receipts.is_empty() {
        println!("No receipts recorded for context {}", snapshot.context);
        return Ok(());
    }

    println!(
        "Receipts for context {} ({} total):",
        snapshot.context,
        snapshot.receipts.len()
    );
    for receipt in &snapshot.receipts {
        let hop = anonymize(&receipt.hop);
        if detailed {
            println!(
                "- hop={} cost={} epoch={} status={} chains={}",
                hop,
                receipt.cost,
                receipt.epoch,
                receipt.status.as_deref().unwrap_or("unknown"),
                receipt.chain_hash.as_deref().unwrap_or("n/a")
            );
        } else {
            println!(
                "- hop={} cost={} status={}",
                hop,
                receipt.cost,
                receipt.status.as_deref().unwrap_or("unknown")
            );
        }
    }
    Ok(())
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
    let mut hasher = Hasher::new();
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    let hex = hex::encode(digest.as_bytes());
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

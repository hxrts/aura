use super::{
    OTACeremonyConfig, OTACeremonyFact, OTACeremonyId, ReadinessCommitment, UpgradeProposal,
};
use aura_core::domain::FactValue;
use aura_core::effects::{JournalEffects, PhysicalTimeEffects};
use aura_core::types::Epoch;
use aura_core::{AuraError, AuraResult, DeviceId};

fn ceremony_id_hex(ceremony_id: OTACeremonyId) -> String {
    hex::encode(ceremony_id.0.as_bytes())
}

fn encoded_devices(devices: &[DeviceId]) -> Vec<String> {
    devices
        .iter()
        .map(|device| hex::encode(device.0.as_bytes()))
        .collect()
}

async fn ceremony_timestamp_ms<E>(effects: &E) -> AuraResult<u64>
where
    E: PhysicalTimeEffects + ?Sized,
{
    effects
        .physical_time()
        .await
        .map(|time| time.ts_ms)
        .map_err(|err| AuraError::internal(format!("Time error: {err}")))
}

async fn persist_ceremony_fact<E>(
    effects: &E,
    key: String,
    fact: &OTACeremonyFact,
) -> AuraResult<()>
where
    E: JournalEffects + ?Sized,
{
    let mut journal = effects.get_journal().await?;
    let fact_bytes =
        serde_json::to_vec(fact).map_err(|err| AuraError::serialization(err.to_string()))?;
    journal.facts.insert(key, FactValue::Bytes(fact_bytes))?;
    effects.persist_journal(&journal).await
}

/// Emit the ceremony-initiated fact through OTA journal semantics.
pub async fn emit_ota_ceremony_initiated_fact<E>(
    effects: &E,
    config: &OTACeremonyConfig,
    ceremony_id: OTACeremonyId,
    proposal: &UpgradeProposal,
) -> AuraResult<()>
where
    E: JournalEffects + PhysicalTimeEffects + ?Sized,
{
    let timestamp_ms = ceremony_timestamp_ms(effects).await?;
    let ceremony_id_hex = ceremony_id_hex(ceremony_id);
    let fact = OTACeremonyFact::CeremonyInitiated {
        ceremony_id: ceremony_id_hex.clone(),
        trace_id: Some(ceremony_id_hex.clone()),
        proposal_id: proposal.proposal_id.to_string(),
        package_id: proposal.package_id.to_string(),
        version: proposal.version.to_string(),
        activation_epoch: proposal.activation_epoch,
        coordinator: hex::encode(proposal.coordinator.0.as_bytes()),
        threshold: config.threshold,
        quorum_size: config.quorum_size,
        timestamp_ms,
    };

    persist_ceremony_fact(effects, format!("ota:initiated:{ceremony_id_hex}"), &fact).await
}

/// Emit the commitment-received fact through OTA journal semantics.
pub async fn emit_ota_commitment_received_fact<E>(
    effects: &E,
    ceremony_id: OTACeremonyId,
    commitment: &ReadinessCommitment,
) -> AuraResult<()>
where
    E: JournalEffects + PhysicalTimeEffects + ?Sized,
{
    let timestamp_ms = ceremony_timestamp_ms(effects).await?;
    let ceremony_id_hex = ceremony_id_hex(ceremony_id);
    let fact = OTACeremonyFact::CommitmentReceived {
        ceremony_id: ceremony_id_hex.clone(),
        trace_id: Some(ceremony_id_hex.clone()),
        device: hex::encode(commitment.device.0.as_bytes()),
        ready: commitment.ready,
        reason: commitment.reason.clone(),
        timestamp_ms,
    };

    let key = format!(
        "ota:commitment:{}:{}",
        ceremony_id_hex,
        hex::encode(commitment.device.0.as_bytes())
    );
    persist_ceremony_fact(effects, key, &fact).await
}

/// Emit the threshold-reached fact through OTA journal semantics.
pub async fn emit_ota_threshold_reached_fact<E>(
    effects: &E,
    ceremony_id: OTACeremonyId,
    ready_count: u32,
    ready_devices: &[DeviceId],
) -> AuraResult<()>
where
    E: JournalEffects + PhysicalTimeEffects + ?Sized,
{
    let timestamp_ms = ceremony_timestamp_ms(effects).await?;
    let ceremony_id_hex = ceremony_id_hex(ceremony_id);
    let fact = OTACeremonyFact::ThresholdReached {
        ceremony_id: ceremony_id_hex.clone(),
        trace_id: Some(ceremony_id_hex.clone()),
        ready_count,
        ready_devices: encoded_devices(ready_devices),
        timestamp_ms,
    };

    persist_ceremony_fact(effects, format!("ota:threshold:{ceremony_id_hex}"), &fact).await
}

/// Emit the ceremony-committed fact through OTA journal semantics.
pub async fn emit_ota_ceremony_committed_fact<E>(
    effects: &E,
    ceremony_id: OTACeremonyId,
    activation_epoch: Epoch,
    ready_devices: &[DeviceId],
    threshold_signature: &[u8],
) -> AuraResult<()>
where
    E: JournalEffects + PhysicalTimeEffects + ?Sized,
{
    let timestamp_ms = ceremony_timestamp_ms(effects).await?;
    let ceremony_id_hex = ceremony_id_hex(ceremony_id);
    let fact = OTACeremonyFact::CeremonyCommitted {
        ceremony_id: ceremony_id_hex.clone(),
        trace_id: Some(ceremony_id_hex.clone()),
        activation_epoch,
        ready_devices: encoded_devices(ready_devices),
        threshold_signature: threshold_signature.to_vec(),
        timestamp_ms,
    };

    persist_ceremony_fact(effects, format!("ota:committed:{ceremony_id_hex}"), &fact).await
}

/// Emit the ceremony-aborted fact through OTA journal semantics.
pub async fn emit_ota_ceremony_aborted_fact<E>(
    effects: &E,
    ceremony_id: OTACeremonyId,
    reason: &str,
) -> AuraResult<()>
where
    E: JournalEffects + PhysicalTimeEffects + ?Sized,
{
    let timestamp_ms = ceremony_timestamp_ms(effects).await?;
    let ceremony_id_hex = ceremony_id_hex(ceremony_id);
    let fact = OTACeremonyFact::CeremonyAborted {
        ceremony_id: ceremony_id_hex.clone(),
        trace_id: Some(ceremony_id_hex.clone()),
        reason: reason.to_string(),
        timestamp_ms,
    };

    persist_ceremony_fact(effects, format!("ota:aborted:{ceremony_id_hex}"), &fact).await
}

//! Snapshot maintenance workflows.

use crate::workflows::journal::{encode_fact_content, persist_fact_value};
use aura_core::effects::JournalEffects;
use aura_core::identifiers::AuthorityId;
use aura_core::AuraError;
use aura_journal::fact::FactContent;
use aura_journal::DomainFact;
use aura_maintenance::{MaintenanceFact, SnapshotProposed};
use aura_protocol::effects::TreeEffects;
use uuid::Uuid;

/// Record a snapshot proposal fact in the local journal.
///
/// Returns the proposal ID (fact key) for the snapshot proposal.
pub async fn propose_snapshot<E>(
    effects: &E,
    device_authority: AuthorityId,
) -> Result<String, AuraError>
where
    E: JournalEffects + TreeEffects,
{
    let current_epoch = effects
        .get_current_epoch()
        .await
        .map_err(|e| AuraError::agent(format!("Failed to load epoch: {e}")))?;

    let state_digest = effects
        .get_current_commitment()
        .await
        .map_err(|e| AuraError::agent(format!("Failed to load commitment: {e}")))?;

    let mut id_bytes = [0u8; 16];
    id_bytes.copy_from_slice(&state_digest.0[..16]);
    let proposal_id = Uuid::from_bytes(id_bytes);

    let proposal = MaintenanceFact::SnapshotProposed(SnapshotProposed::new(
        device_authority,
        proposal_id,
        current_epoch,
        state_digest,
    ));

    let fact_content = FactContent::Relational(proposal.to_generic());

    let fact_value = encode_fact_content(fact_content)?;
    let fact_key = format!("snapshot_proposed:{proposal_id}");
    persist_fact_value(effects, fact_key.clone(), fact_value).await?;

    Ok(fact_key)
}

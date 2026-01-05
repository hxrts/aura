//! Admin maintenance workflows.

use crate::workflows::journal::{encode_fact_content, persist_fact_value};
use aura_core::effects::JournalEffects;
use aura_core::hash;
use aura_core::identifiers::{AccountId, AuthorityId, ContextId};
use aura_core::types::Epoch;
use aura_core::AuraError;
use aura_journal::fact::{FactContent, RelationalFact};
use aura_maintenance::{AdminReplacement, MaintenanceFact};

/// Record an admin replacement fact in the local journal.
pub async fn replace_admin<E: JournalEffects>(
    effects: &E,
    device_authority: AuthorityId,
    account_id: AccountId,
    new_admin_id: AuthorityId,
    activation_epoch: u64,
) -> Result<(), AuraError> {
    let replacement = MaintenanceFact::AdminReplacement(AdminReplacement::new(
        device_authority,
        device_authority,
        new_admin_id,
        Epoch::new(activation_epoch),
    ));

    // Create a context ID from the authority for the relational fact
    let context_id = ContextId::new_from_entropy(hash::hash(&device_authority.to_bytes()));

    // Serialize the MaintenanceFact and wrap in a Generic relational fact
    let payload = serde_json::to_vec(&replacement)
        .map_err(|e| AuraError::agent(format!("Failed to serialize admin replacement: {e}")))?;

    let envelope = aura_core::types::facts::FactEnvelope {
        type_id: aura_core::types::facts::FactTypeId::from("admin-replacement"),
        schema_version: 1,
        encoding: aura_core::types::facts::FactEncoding::Json,
        payload,
    };

    let fact_content = FactContent::Relational(RelationalFact::Generic {
        context_id,
        envelope,
    });

    let fact_value = encode_fact_content(fact_content)?;
    let fact_key = format!("admin_replace:{account_id}");
    persist_fact_value(effects, fact_key, fact_value).await?;

    Ok(())
}

//! Admin maintenance workflows.

use crate::workflows::journal::persist_maintenance_fact;
use aura_core::effects::JournalEffects;
use aura_core::types::identifiers::{AccountId, AuthorityId};
use aura_core::types::Epoch;
use aura_core::AuraError;
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

    persist_maintenance_fact(effects, &replacement, format!("admin_replace:{account_id}")).await
}

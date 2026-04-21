#![allow(missing_docs)]
#![allow(dead_code)]
// Validation helpers are reached through workflow and proof-contract paths that
// vary by target; strict all-target dead-code analysis does not see every use.

use super::proof_issuance::{
    issue_channel_membership_ready_proof, issue_home_created_proof, ChannelMembershipReadyProof,
    HomeCreatedProof,
};
use crate::signal_defs::{HOMES_SIGNAL, HOMES_SIGNAL_NAME};
use crate::ui_contract::AuthoritativeSemanticFact;
use crate::workflows::signals::read_signal;
use crate::AppCore;
use async_lock::RwLock;
use aura_core::{AuraError, ChannelId};
use std::sync::Arc;

#[aura_macros::authoritative_source(kind = "app_core")]
pub(in crate::workflows) async fn authoritative_semantic_facts_snapshot(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<Vec<AuthoritativeSemanticFact>, AuraError> {
    Ok(app_core.read().await.authoritative_semantic_facts())
}

#[aura_macros::authoritative_source(kind = "signal")]
pub(in crate::workflows) async fn prove_home_created(
    app_core: &Arc<RwLock<AppCore>>,
    home_id: ChannelId,
) -> Result<HomeCreatedProof, AuraError> {
    let homes = read_signal(app_core, &*HOMES_SIGNAL, HOMES_SIGNAL_NAME).await?;
    if homes.has_home(&home_id) {
        Ok(issue_home_created_proof(home_id))
    } else {
        Err(AuraError::from(
            crate::workflows::error::WorkflowError::Precondition(
                "home_created proof requires the home to exist in authoritative homes state",
            ),
        ))
    }
}

#[aura_macros::authoritative_source(kind = "signal")]
pub(in crate::workflows) async fn prove_channel_membership_ready(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
) -> Result<ChannelMembershipReadyProof, AuraError> {
    let channel_id_string = channel_id.to_string();
    let facts = authoritative_semantic_facts_snapshot(app_core).await?;
    if facts.iter().any(|fact| {
        matches!(
            fact,
            AuthoritativeSemanticFact::ChannelMembershipReady { channel, .. }
                if channel.id.as_deref() == Some(channel_id_string.as_str())
        )
    }) {
        Ok(issue_channel_membership_ready_proof(channel_id))
    } else {
        Err(AuraError::from(
            crate::workflows::error::WorkflowError::Precondition(
                "ChannelMembershipReady proof requires an authoritative readiness fact",
            ),
        ))
    }
}

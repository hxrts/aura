//! OTA release workflows over maintenance facts (docs/116 §4, docs/808).
//!
//! Release distribution, recommendations and scoped staging are journal
//! facts; the observed OTA state is reduced from them (`OtaView`). Activation
//! and cutover execution belong to the updater/launcher control plane and the
//! runtime's activation ceremony, not to these workflows.

use crate::workflows::journal::{persist_maintenance_fact, read_maintenance_facts};
use aura_core::effects::{JournalEffects, PhysicalTimeEffects, StorageEffects};
use aura_core::time::TimeStamp;
use aura_core::types::identifiers::AuthorityId;
use aura_core::{AuraError, Hash32};
use aura_maintenance::{
    AuraActivationScope, AuraArtifactDescriptor, AuraPolicyScope, AuraReleaseId,
    AuraReleaseManifest, MaintenanceFact, OtaScopeStage, OtaView, ReleasePolicyFact,
    UpgradeExecutionFact,
};
use aura_sync::services::OtaDistributionService;
use std::sync::Arc;

/// The observed OTA state reduced from the local journal.
// OWNERSHIP: observed
pub async fn ota_view<E: JournalEffects>(effects: &E) -> Result<OtaView, AuraError> {
    Ok(OtaView::from_facts(&read_maintenance_facts(effects).await?))
}

/// Why an OTA workflow refused a request.
#[derive(Debug, thiserror::Error)]
pub enum OtaError {
    /// The release has no declaration in the journal.
    #[error("release {0} has not been declared")]
    NotDeclared(Hash32),
    /// An artifact blob matches no descriptor in the manifest.
    #[error("artifact {0} is not described by the manifest")]
    UnknownArtifact(Hash32),
    /// Staging needs the scope's running release and none is recorded.
    #[error("the scope has no completed release; name the release it runs with --from")]
    NoRunningRelease,
    /// The target is the release the scope already runs.
    #[error("the scope already runs this release")]
    AlreadyRunning,
}

impl From<OtaError> for AuraError {
    fn from(error: OtaError) -> Self {
        let not_found = matches!(error, OtaError::NotDeclared(_));
        let message = error.to_string();
        if not_found {
            AuraError::NotFound {
                message,
                source: Some(Arc::new(error)),
            }
        } else {
            AuraError::Invalid {
                message,
                source: Some(Arc::new(error)),
            }
        }
    }
}

async fn now<E: PhysicalTimeEffects>(effects: &E) -> Result<TimeStamp, AuraError> {
    effects
        .physical_time()
        .await
        .map(TimeStamp::PhysicalClock)
        .map_err(|e| super::error::runtime_call("OTA timestamp", e).into())
}

/// Commit OTA facts, each under a key derived from its content so a
/// republished fact is the same journal entry.
async fn commit<E: JournalEffects>(
    effects: &E,
    facts: &[MaintenanceFact],
) -> Result<(), AuraError> {
    for fact in facts {
        let key = Hash32::from_value(fact).map_err(super::error::fact_encoding)?;
        persist_maintenance_fact(effects, fact, format!("ota:{key}")).await?;
    }
    Ok(())
}

/// Publish a signed release manifest and artifact blobs: each blob is paired
/// with the manifest descriptor carrying its content hash, the bundle is
/// verified (provenance-derived release id, manifest signature, artifact hash
/// and size) and stored, and its distribution facts are committed.
pub async fn publish_release<E>(
    effects: &E,
    authority: AuthorityId,
    manifest: &AuraReleaseManifest,
    blobs: Vec<Vec<u8>>,
) -> Result<AuraReleaseId, AuraError>
where
    E: JournalEffects + StorageEffects + PhysicalTimeEffects,
{
    let artifacts = blobs
        .into_iter()
        .map(|bytes| {
            let hash = Hash32::from_bytes(&bytes);
            manifest
                .artifacts
                .iter()
                .find(|descriptor| descriptor.artifact_hash == hash)
                .map(|descriptor| (descriptor.clone(), bytes))
                .ok_or(OtaError::UnknownArtifact(hash))
        })
        .collect::<Result<Vec<(AuraArtifactDescriptor, Vec<u8>)>, OtaError>>()?;
    let published_at = now(effects).await?;
    let publication = OtaDistributionService::new()
        .publish_release_bundle(effects, authority, manifest, &artifacts, &[], published_at)
        .await
        .map_err(|rejection| AuraError::Invalid {
            message: rejection.to_string(),
            source: Some(Arc::new(rejection)),
        })?;
    commit(effects, &publication.facts).await?;
    Ok(manifest.release_id)
}

fn require_declared(view: &OtaView, release_id: AuraReleaseId) -> Result<(), OtaError> {
    if view.releases.contains_key(&release_id) {
        Ok(())
    } else {
        Err(OtaError::NotDeclared(*release_id.as_hash()))
    }
}

/// Recommend a declared release to a policy scope.
pub async fn recommend_release<E>(
    effects: &E,
    authority: AuthorityId,
    release_id: AuraReleaseId,
    scope: AuraPolicyScope,
) -> Result<(), AuraError>
where
    E: JournalEffects + PhysicalTimeEffects,
{
    require_declared(&ota_view(effects).await?, release_id)?;
    let fact = MaintenanceFact::ReleasePolicy(ReleasePolicyFact::RecommendationPublished {
        authority_id: authority,
        release_id,
        scope,
        published_at: now(effects).await?,
    });
    commit(effects, &[fact]).await
}

/// Stage a declared release for an activation scope. The prior release is
/// `from`, or else the release the scope last completed a cutover to.
pub async fn stage_release<E>(
    effects: &E,
    authority: AuthorityId,
    scope: AuraActivationScope,
    release_id: AuraReleaseId,
    from: Option<AuraReleaseId>,
) -> Result<(), AuraError>
where
    E: JournalEffects + PhysicalTimeEffects,
{
    let view = ota_view(effects).await?;
    require_declared(&view, release_id)?;
    let from_release_id = match from {
        Some(from) => from,
        None => view
            .upgrades
            .values()
            .filter(|u| u.scope == scope && u.stage == OtaScopeStage::CutoverCompleted)
            .map(|u| u.to_release_id)
            .max()
            .ok_or(OtaError::NoRunningRelease)?,
    };
    if from_release_id == release_id {
        return Err(OtaError::AlreadyRunning.into());
    }
    let fact = MaintenanceFact::UpgradeExecution(UpgradeExecutionFact::ReleaseStaged {
        authority_id: authority,
        scope,
        from_release_id,
        to_release_id: release_id,
        staged_at: now(effects).await?,
    });
    commit(effects, &[fact]).await
}

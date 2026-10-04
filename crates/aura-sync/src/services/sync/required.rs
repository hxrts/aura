//! Required requested-peer results and exact lexical local session custody.
use super::*;
use aura_core::time::timeout::{execute_with_timeout_budget, TimeoutBudget};
use aura_core::SessionId;

/// Actual local admission/protocol failure. These diagnostics grant no peer trust.
#[derive(Debug, thiserror::Error)]
pub enum RequiredPeerSyncError {
    #[error("requested sync peer {peer} was not admitted by the local rate policy")]
    RateLimited {
        /// Requested peer rejected by the actual local resource policy.
        peer: DeviceId,
    },
    #[error("requested sync peer {peer} session admission failed")]
    /// Actual manager admission failed for this requested peer.
    SessionAdmission {
        /// Requested peer of the failed operation.
        peer: DeviceId,
        /// Original native operation failure.
        #[source]
        source: AuraError,
    },
    #[error("requested sync peer {peer} protocol failed")]
    /// Actual peer protocol failed; source retains its native cause.
    Protocol {
        /// Requested peer of the failed operation.
        peer: DeviceId,
        /// Original native operation failure.
        #[source]
        source: AuraError,
    },
    #[error("original issued sync session {session} disappeared before retirement")]
    /// Exact issued local session disappeared before required retirement.
    SessionLost {
        /// Original actual session identifier.
        session: SessionId,
    },
}

/// Borrowed original manager plus exactly the sessions it actually issued.
/// No peer selector, serialization or observation may reconstruct this owner.
pub(super) struct RequiredSyncSessionsCapability<'service> {
    service: &'service SyncService,
    sessions: Vec<(DeviceId, SessionId)>,
}
impl RequiredSyncSessionsCapability<'_> {
    fn retire(&mut self) -> SyncResult<()> {
        let mut missing = None;
        {
            let mut manager = self.service.session_manager.write();
            for (_, id) in self.sessions.drain(..) {
                if !manager.retire_owned_session(&id) && missing.is_none() {
                    missing = Some(id);
                }
            }
        }
        if let Some(session) = missing {
            let failure = required_error(RequiredPeerSyncError::SessionLost { session });
            let mut retained = self.service.required_cleanup_failure.write();
            if retained.is_none() {
                *retained = Some(failure.clone());
            }
            return Err(failure);
        }
        Ok(())
    }
}
impl Drop for RequiredSyncSessionsCapability<'_> {
    fn drop(&mut self) {
        // Pure exact local retirement is synchronous. This does not claim
        // acknowledged transport teardown or perform required async cleanup.
        if let Err(failure) = self.retire() {
            let mut retained = self.service.required_cleanup_failure.write();
            if retained.is_none() {
                *retained = Some(failure);
            }
        }
    }
}

impl SyncService {
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "RequiredSyncSessionsCapability", capability_type = RequiredSyncSessionsCapability<'_>, family = "proof_issuer")]
    #[aura_macros::authoritative_source(kind = "proof_issuer")]
    pub(super) async fn create_required_sessions(
        &self,
        peers: &[DeviceId],
        original: &TimeoutBudget,
    ) -> Result<RequiredSyncSessionsCapability<'_>, AuraError> {
        let _observation = original.acquire_observation().await;
        let now = self
            .time_effects
            .physical_time()
            .await
            .map_err(time_error_to_aura)?;
        let mut issued = RequiredSyncSessionsCapability {
            service: self,
            sessions: Vec::new(),
        };
        let result = {
            let mut manager = self.session_manager.write();
            let mut failure = None;
            for &peer in peers {
                match manager.create_session_in_original_window(vec![peer], &now, original) {
                    Ok(session) => issued.sessions.push((peer, session)),
                    Err(source) => {
                        failure = Some(required_error(RequiredPeerSyncError::SessionAdmission {
                            peer,
                            source,
                        }));
                        break;
                    }
                }
            }
            failure
        };
        if let Some(failure) = result {
            // Original issued subset is still owned and retired by Drop.
            return Err(failure);
        }
        Ok(issued)
    }

    /// Required execution for each distinct requested peer, preserving its
    /// actual native failure and exact local session owner across cancellation.
    pub(super) async fn sync_requested_peers<E>(
        &self,
        effects: &E,
        peers: Vec<DeviceId>,
        now: MonotonicInstant,
        original: &TimeoutBudget,
    ) -> SyncResult<()>
    where
        E: SyncProtocolEffects,
    {
        if let Some(failure) = self.required_cleanup_failure.read().as_ref() {
            return Err(failure.clone());
        }
        let requested: Vec<_> = peers
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        if requested.is_empty() {
            return Ok(());
        }
        let allowed = self.check_rate_limits(&requested, now).await?;
        for &peer in &requested {
            if !allowed.contains(&peer) {
                return Err(required_error(RequiredPeerSyncError::RateLimited { peer }));
            }
        }
        let mut issued = self.create_required_sessions(&requested, original).await?;
        let operation = execute_with_timeout_budget(effects, original, || async {
            let mut results = Vec::new();
            for &(peer, _) in &issued.sessions {
                let mut protocol = self.journal_sync.read().clone();
                let outcome = protocol.sync_with_peer(effects, peer).await;
                *self.journal_sync.write() = protocol;
                let operations = outcome.map_err(|source| {
                    required_error(RequiredPeerSyncError::Protocol { peer, source })
                })?;
                results.push((peer, Some(operations)));
            }
            self.update_sync_metrics(&results).await?;
            let now = {
                let _observation = original.acquire_observation().await;
                let observed = effects.physical_time().await.map_err(time_error_to_aura)?;
                original.remaining_at(&observed).map_err(required_error)?;
                observed
            };
            let scores: Vec<_> = results.iter().map(|(peer, _)| (*peer, true)).collect();
            Self::update_peer_scores_from_sync(&self.peer_manager, &scores, &now).await?;
            Self::update_auto_sync_metrics(&scores).await?;
            Ok::<(), aura_core::AuraError>(())
        })
        .await
        .map_err(required_error);
        let cleanup = issued.retire();
        match (operation, cleanup) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(source), Ok(())) | (Ok(()), Err(source)) => Err(source),
            (Err(primary), Err(cleanup)) => Err(required_error(RequiredSyncCleanupFailure {
                primary,
                cleanup,
            })),
        }
    }
}
#[derive(Debug, thiserror::Error)]
#[error("{primary}; required sync session retirement also failed: {cleanup}")]
struct RequiredSyncCleanupFailure {
    #[source]
    primary: AuraError,
    cleanup: AuraError,
}
fn required_error(source: impl std::error::Error + Send + Sync + 'static) -> AuraError {
    AuraError::Internal {
        message: "required requested-peer sync failed".into(),
        source: Some(Arc::new(source)),
    }
}

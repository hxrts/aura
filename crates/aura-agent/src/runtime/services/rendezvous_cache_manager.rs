//! Rendezvous cache manager for handler-local state.

use super::state::with_state_mut_validated;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_rendezvous::RendezvousDescriptor;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
struct PendingChannel {
    context_id: ContextId,
    peer: AuthorityId,
    initiated_at: u64,
}

#[derive(Debug, Default)]
struct RendezvousCacheState {
    descriptors: HashMap<(ContextId, AuthorityId), RendezvousDescriptor>,
    pending_channels: HashMap<(ContextId, AuthorityId), PendingChannel>,
}

impl RendezvousCacheState {
    fn validate(&self) -> Result<(), String> {
        for ((ctx, peer), desc) in &self.descriptors {
            if *ctx != desc.context_id || *peer != desc.authority_id {
                return Err(format!(
                    "descriptor key mismatch: ({:?}, {:?}) vs ({:?}, {:?})",
                    ctx, peer, desc.context_id, desc.authority_id
                ));
            }
        }
        for ((ctx, peer), pending) in &self.pending_channels {
            if *ctx != pending.context_id || *peer != pending.peer {
                return Err(format!(
                    "pending channel key mismatch: ({:?}, {:?}) vs ({:?}, {:?})",
                    ctx, peer, pending.context_id, pending.peer
                ));
            }
        }
        Ok(())
    }
}

/// Manages rendezvous handler caches (descriptors + pending channels).
#[derive(Clone, Default)]
pub struct RendezvousCacheManager {
    state: Arc<RwLock<RendezvousCacheState>>,
}

impl RendezvousCacheManager {
    /// Create a new rendezvous cache manager.
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(RendezvousCacheState::default())),
        }
    }

    /// Cache a descriptor.
    pub async fn cache_descriptor(&self, descriptor: RendezvousDescriptor) {
        with_state_mut_validated(
            &self.state,
            |state| {
                state
                    .descriptors
                    .insert((descriptor.context_id, descriptor.authority_id), descriptor);
            },
            |state| state.validate(),
        )
        .await;
    }

    /// Get a cached descriptor.
    pub async fn get_descriptor(
        &self,
        context_id: ContextId,
        peer: AuthorityId,
    ) -> Option<RendezvousDescriptor> {
        self.state
            .read()
            .await
            .descriptors
            .get(&(context_id, peer))
            .cloned()
    }

    /// Track a pending channel establishment.
    pub async fn track_pending_channel(
        &self,
        context_id: ContextId,
        peer: AuthorityId,
        initiated_at: u64,
    ) {
        with_state_mut_validated(
            &self.state,
            |state| {
                state.pending_channels.insert(
                    (context_id, peer),
                    PendingChannel {
                        context_id,
                        peer,
                        initiated_at,
                    },
                );
            },
            |state| state.validate(),
        )
        .await;
    }

    /// Remove a pending channel.
    pub async fn remove_pending_channel(&self, context_id: ContextId, peer: AuthorityId) {
        let _ = with_state_mut_validated(
            &self.state,
            |state| state.pending_channels.remove(&(context_id, peer)),
            |state| state.validate(),
        )
        .await;
    }

    /// Cleanup expired descriptors and stale pending channels.
    pub async fn cleanup_expired(
        &self,
        now_ms: u64,
        pending_max_age_ms: u64,
    ) -> (usize, usize) {
        with_state_mut_validated(
            &self.state,
            |state| {
                let before_desc = state.descriptors.len();
                state
                    .descriptors
                    .retain(|_, descriptor| descriptor.is_valid(now_ms));
                let removed_desc = before_desc - state.descriptors.len();

                let before_pending = state.pending_channels.len();
                state.pending_channels.retain(|_, channel| {
                    now_ms.saturating_sub(channel.initiated_at) < pending_max_age_ms
                });
                let removed_pending = before_pending - state.pending_channels.len();

                (removed_desc, removed_pending)
            },
            |state| state.validate(),
        )
        .await
    }
}

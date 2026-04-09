//! Internal offline-bridge state aliases used by tests and the demo bridge.

use super::{AuthoritativeModerationStatus, InvitationInfo};
#[cfg(test)]
use super::{CeremonyProcessingOutcome, InvitationMutationOutcome};
#[cfg(test)]
use crate::core::IntentError;
use async_lock::Mutex;
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use std::collections::HashMap;
use std::sync::Arc;

pub(crate) type PendingInvitationsState = Arc<Mutex<Option<Vec<InvitationInfo>>>>;
pub(crate) type AmpChannelContexts = Arc<Mutex<HashMap<ChannelId, ContextId>>>;
pub(crate) type MaterializedChannelNameMatches = Arc<Mutex<HashMap<String, Vec<ChannelId>>>>;
pub(crate) type AmpChannelStates = Arc<Mutex<HashMap<(ContextId, ChannelId), bool>>>;
pub(crate) type AmpChannelParticipants =
    Arc<Mutex<HashMap<(ContextId, ChannelId), Vec<AuthorityId>>>>;
pub(crate) type ModerationStatuses =
    Arc<Mutex<HashMap<(ContextId, ChannelId, AuthorityId), AuthoritativeModerationStatus>>>;
#[cfg(test)]
pub(crate) type OfflineAcceptInvitationResult =
    Arc<Mutex<Option<Result<InvitationMutationOutcome, IntentError>>>>;
#[cfg(test)]
pub(crate) type OfflineProcessCeremonyResult =
    Arc<Mutex<Option<Result<CeremonyProcessingOutcome, IntentError>>>>;

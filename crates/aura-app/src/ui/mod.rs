//! UI-facing facade for aura-app.
//!
//! This module exposes the narrow surface that frontends should use:
//! - workflows (commands)
//! - signals (read/subscribe)
//! - core types (AppCore, AppConfig)

use async_lock::RwLock;
use std::sync::Arc;

use crate::AppCore;

/// UI wrapper around `AppCore` to discourage direct access to internals.
#[derive(Clone)]
pub struct UiAppCore {
    inner: Arc<RwLock<AppCore>>,
}

impl UiAppCore {
    pub fn new(inner: Arc<RwLock<AppCore>>) -> Self {
        Self { inner }
    }

    pub fn raw(&self) -> &Arc<RwLock<AppCore>> {
        &self.inner
    }
}

impl From<Arc<RwLock<AppCore>>> for UiAppCore {
    fn from(inner: Arc<RwLock<AppCore>>) -> Self {
        Self::new(inner)
    }
}

pub mod signals {
    pub use crate::signal_defs::{
        register_app_signals, register_app_signals_with_queries, BUDGET_SIGNAL, CHAT_SIGNAL,
        CONNECTION_STATUS_SIGNAL, CONTACTS_SIGNAL, DISCOVERED_PEERS_SIGNAL, ERROR_SIGNAL,
        HOMES_SIGNAL, INVITATIONS_SIGNAL, NEIGHBORHOOD_SIGNAL, NETWORK_STATUS_SIGNAL,
        RECOVERY_SIGNAL, SETTINGS_SIGNAL, SYNC_STATUS_SIGNAL, TRANSPORT_PEERS_SIGNAL,
        UNREAD_COUNT_SIGNAL, DiscoveredPeerMethod,
    };
    pub use crate::signal_defs::{ConnectionStatus, NetworkStatus, SyncStatus};
}

pub mod workflows {
    pub use crate::workflows::budget;
    pub use crate::workflows::admin;
    pub use crate::workflows::amp;
    pub use crate::workflows::ceremonies;
    pub use crate::workflows::contacts;
    pub use crate::workflows::context;
    pub use crate::workflows::invitation;
    pub use crate::workflows::moderation;
    pub use crate::workflows::network;
    pub use crate::workflows::query;
    pub use crate::workflows::recovery_cli;
    pub use crate::workflows::settings;
    pub use crate::workflows::snapshot;
    pub use crate::workflows::steward;
    pub use crate::workflows::sync;
    pub use crate::workflows::system;

    #[cfg(feature = "signals")]
    pub use crate::workflows::messaging;

    #[cfg(feature = "signals")]
    pub use crate::workflows::recovery;
}

pub mod types {
    pub use crate::core::{
        AppConfig, AppCore, Intent, IntentError, InvitationType, Screen, StateSnapshot,
    };
    pub use crate::errors::{AppError, AuthFailure, NetworkErrorCode, SyncStage, ToastSeverity};
    pub use crate::runtime_bridge::{
        BoxedRuntimeBridge, CeremonyKind, InvitationBridgeType, LanPeerInfo, RendezvousStatus,
        RuntimeBridge, RuntimeStatus, SyncStatus as RuntimeSyncStatus,
    };
    pub use crate::workflows::budget::{
        BudgetBreakdown, BudgetError, HomeFlowBudget, HOME_TOTAL_SIZE, KB, MAX_NEIGHBORHOODS,
        MAX_RESIDENTS, MB, NEIGHBORHOOD_DONATION, RESIDENT_ALLOCATION,
    };
    pub use crate::thresholds::{
        default_channel_threshold, default_guardian_threshold, normalize_channel_threshold,
        normalize_guardian_threshold, normalize_recovery_threshold,
    };
    pub use crate::views::{
        AdjacencyType, BanRecord, Channel, ChannelType, ChatState, Contact, ContactsState,
        Guardian, GuardianStatus, HomesState, HomeState, Invitation, InvitationDirection,
        InvitationStatus, InvitationsState, KickRecord, Message, MuteRecord,
        MySuggestion,
        NeighborHome, NeighborhoodState, RecoveryApproval, RecoveryProcess, RecoveryProcessStatus,
        RecoveryState, Resident, ResidentRole, SuggestionPolicy, TraversalPosition,
    };
    pub use crate::views::{chat, contacts, home, invitations, neighborhood, recovery};
    #[cfg(feature = "signals")]
    pub use crate::reactive_state::{ReactiveState, ReactiveVec};
    pub use crate::effects::reactive::{ReactiveHandler, SignalGraph, SignalGraphStats};
    pub use aura_core::identifiers::{AuthorityId, ContextId};
    pub use aura_core::time::TimeStamp;

    // AMP types for channel state inspection
    pub use aura_journal::ChannelEpochState;
}

pub mod authorization {
    pub use crate::authorization::*;
}

pub mod prelude;

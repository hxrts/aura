//! Runtime-owned anonymous established-path service.
//!
//! Owns reusable anonymous path lifecycle: encrypted setup-object creation,
//! replay suppression, expiry-bounded path state, and runtime-local path reuse.
//! The host-local control session remains the live executor until
//! `crate::adaptive_privacy_control::AnonymousPathEstablishProtocol` is wired
//! through admitted VM execution. Owner: `path_manager`. Removal condition:
//! delete the local control state machine once the choreography becomes the
//! canonical runtime execution path.
#![allow(dead_code)] // Cleanup target (2026-07): remove after anonymous-path choreography replaces the local control state machine.

use super::config_profiles::impl_service_config_profiles;
use super::service_registry::ServiceRegistry;
use super::traits::{RuntimeService, RuntimeServiceContext, ServiceError, ServiceHealth};
use crate::runtime::TaskGroup;
use async_trait::async_trait;
use aura_core::effects::{time::PhysicalTimeEffects, RandomEffects, RouteCryptoEffects};
use aura_core::hash::hash;
use aura_core::service::{
    AnonymousHopView, EncryptedAnonymousSetupLayer, EncryptedAnonymousSetupObject, EstablishedPath,
    EstablishedPathRef, LinkProtectionMode, PathProtectionMode, Route, SelectionState,
    ServiceFamily, ServiceProfile,
};
use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_core::util::serialization;
use aura_effects::RealRouteCryptoHandler;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

#[cfg(feature = "choreo-backend-telltale-machine")]
use telltale_machine::{
    CancellationWitness, OwnershipCapability, OwnershipReceipt, OwnershipScope,
    OwnershipTerminalReason, ReadinessWitness,
};

#[allow(dead_code)] // Declaration-layer ingress inventory; sanctioned surfaces call methods directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnonymousPathManagerCommand {
    OpenEstablishSession,
    EstablishEncryptedPath,
    CancelEstablishSession,
    ReuseEstablishedPath,
    CleanupExpiredPaths,
}

/// Configuration for anonymous established-path lifecycle.
#[derive(Debug, Clone)]
pub struct AnonymousPathManagerConfig {
    /// Default TTL for one established path.
    pub path_ttl: Duration,
    /// Cleanup cadence for expired path removal.
    pub cleanup_interval: Duration,
    /// Max replay/setup markers retained in the replay window.
    pub replay_window_entries: usize,
    /// Maximum control-plane lifetime for one anonymous-establish attempt.
    pub establish_timeout: Duration,
    /// Maximum serialized reply layer frame accepted before deserialization.
    pub max_reply_layer_frame_bytes: usize,
    /// Maximum encrypted reply layer payload accepted before AEAD decrypt.
    pub max_reply_layer_ciphertext_bytes: usize,
    /// Maximum decrypted reply layer payload accepted after AEAD decrypt.
    pub max_reply_layer_plaintext_bytes: usize,
}

impl Default for AnonymousPathManagerConfig {
    fn default() -> Self {
        Self {
            path_ttl: Duration::from_secs(30),
            cleanup_interval: Duration::from_secs(10),
            replay_window_entries: 256,
            establish_timeout: Duration::from_secs(5),
            max_reply_layer_frame_bytes: 1024 * 1024,
            max_reply_layer_ciphertext_bytes: 1024 * 1024,
            max_reply_layer_plaintext_bytes: 1024 * 1024,
        }
    }
}

impl_service_config_profiles!(AnonymousPathManagerConfig {
    /// Short deterministic config for tests.
    pub fn for_testing() -> Self {
        Self {
            path_ttl: Duration::from_millis(200),
            cleanup_interval: Duration::from_millis(25),
            replay_window_entries: 16,
            establish_timeout: Duration::from_millis(75),
            max_reply_layer_frame_bytes: 64 * 1024,
            max_reply_layer_ciphertext_bytes: 64 * 1024,
            max_reply_layer_plaintext_bytes: 64 * 1024,
        }
    }
});

/// Summary projection for runtime-local anonymous path state.
#[allow(dead_code)]
// Cleanup target (2026-07): remove if no runtime consumer still needs the local anonymous-path projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnonymousPathProjection {
    pub active_paths: usize,
    pub replay_window_entries: usize,
    pub active_control_sessions: usize,
    pub last_cleanup_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnonymousPathOwnershipCapabilityEvidence {
    pub session_id: u64,
    pub owner_id: String,
    pub generation: u64,
    pub scope_keys: BTreeSet<String>,
    #[cfg(feature = "choreo-backend-telltale-machine")]
    pub telltale: OwnershipCapability,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnonymousPathOwnershipReceiptEvidence {
    pub session_id: u64,
    pub claim_id: u64,
    pub from_owner_id: String,
    pub to_owner_id: String,
    pub to_generation: u64,
    pub scope_keys: BTreeSet<String>,
    #[cfg(feature = "choreo-backend-telltale-machine")]
    pub telltale: OwnershipReceipt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnonymousPathReadinessEvidence {
    pub witness_id: u64,
    pub session_id: u64,
    pub owner_id: String,
    pub generation: u64,
    pub predicate_ref: String,
    #[cfg(feature = "choreo-backend-telltale-machine")]
    pub telltale: ReadinessWitness,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnonymousPathCancellationEvidence {
    pub witness_id: u64,
    pub session_id: u64,
    pub owner_id: String,
    pub generation: u64,
    pub reason: String,
    #[cfg(feature = "choreo-backend-telltale-machine")]
    pub telltale: CancellationWitness,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnonymousPathEstablishStatus {
    Open,
    Ready { path_id: [u8; 32] },
    Cancelled { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnonymousPathEstablishControl {
    pub session_id: u64,
    pub deadline_ms: u64,
    pub ownership_capability: AnonymousPathOwnershipCapabilityEvidence,
    pub ownership_receipt: AnonymousPathOwnershipReceiptEvidence,
    pub readiness_witness: Option<AnonymousPathReadinessEvidence>,
    pub cancellation_witness: Option<AnonymousPathCancellationEvidence>,
    pub status: AnonymousPathEstablishStatus,
}

#[derive(Debug, thiserror::Error)]
pub enum AnonymousPathManagerError {
    #[error("anonymous establish requires at least one relay hop")]
    DirectRouteNotAnonymous,
    #[error("anonymous establish requires route-layer public material for every hop")]
    MissingRouteLayerPublicKey,
    #[error("route-layer cryptography failed: {0}")]
    RouteCrypto(String),
    #[error("transparent anonymous setup replay rejected for control session {session_id}")]
    ReplayRejected {
        session_id: u64,
        control: Box<AnonymousPathEstablishControl>,
    },
    #[error(
        "anonymous reply replay rejected for path {path_id} hop {hop_index} counter {counter}"
    )]
    ReplyReplayRejected {
        path_id: String,
        hop_index: u16,
        counter: u64,
    },
    #[error("anonymous reply layer {kind} too large: {actual} > {max} bytes")]
    ReplyLayerTooLarge {
        kind: &'static str,
        actual: usize,
        max: usize,
    },
    #[error(
        "anonymous establish control session {session_id} timed out at {deadline_ms}, now {now_ms}"
    )]
    EstablishTimedOut {
        session_id: u64,
        deadline_ms: u64,
        now_ms: u64,
        control: Box<AnonymousPathEstablishControl>,
    },
    #[error("anonymous establish control session {session_id} rejected stale owner {attempted_owner} generation {attempted_generation}")]
    StaleOwner {
        session_id: u64,
        attempted_owner: String,
        attempted_generation: u64,
        control: Box<AnonymousPathEstablishControl>,
    },
    #[error("failed to serialize path material: {0}")]
    Serialization(String),
    #[error("established path not found")]
    PathNotFound,
    #[error("established path expired at {valid_until}, now {now_ms}")]
    PathExpired { valid_until: u64, now_ms: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct EncryptedSetupPayload {
    hop_view: AnonymousHopView,
    #[serde(default)]
    next: Option<Box<EncryptedAnonymousSetupLayer>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct EncryptedReplyLayerFrame {
    counter: u64,
    ciphertext: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AnonymousPathControlSessionState {
    session_id: u64,
    deadline_ms: u64,
    owner_id: String,
    generation: u64,
    scope_keys: BTreeSet<String>,
    readiness_witness: Option<AnonymousPathReadinessEvidence>,
    cancellation_witness: Option<AnonymousPathCancellationEvidence>,
    status: AnonymousPathEstablishStatus,
}

struct PathState {
    established_paths: HashMap<[u8; 32], EstablishedPath>,
    replay_seen: HashSet<[u8; 32]>,
    replay_order: VecDeque<[u8; 32]>,
    reply_counters: HashMap<[u8; 32], u64>,
    control_sessions: HashMap<u64, AnonymousPathControlSessionState>,
    next_control_session_id: u64,
    next_control_witness_id: u64,
    last_cleanup_ms: Option<u64>,
    cleanup_tasks: Option<TaskGroup>,
    lifecycle: ServiceHealth,
}

impl Default for PathState {
    fn default() -> Self {
        Self {
            established_paths: HashMap::new(),
            replay_seen: HashSet::new(),
            replay_order: VecDeque::new(),
            reply_counters: HashMap::new(),
            control_sessions: HashMap::new(),
            next_control_session_id: 1,
            next_control_witness_id: 1,
            last_cleanup_ms: None,
            cleanup_tasks: None,
            lifecycle: ServiceHealth::NotStarted,
        }
    }
}

impl PathState {
    fn validate(&self) -> Result<(), super::invariant::InvariantViolation> {
        for (path_id, path) in &self.established_paths {
            if path.path_id != *path_id {
                return Err(super::invariant::InvariantViolation::new(
                    "AnonymousPathManager",
                    "established path id/key mismatch",
                ));
            }
            if path.profile != ServiceProfile::AnonymousPathEstablish {
                return Err(super::invariant::InvariantViolation::new(
                    "AnonymousPathManager",
                    "anonymous path manager stored non-anonymous establish profile",
                ));
            }
            if path.route.hops.is_empty() {
                return Err(super::invariant::InvariantViolation::new(
                    "AnonymousPathManager",
                    "anonymous path manager stored direct route",
                ));
            }
            if path.forward_hop_keys.len() != path.route.hops.len()
                || path.backward_hop_keys.len() != path.route.hops.len()
            {
                return Err(super::invariant::InvariantViolation::new(
                    "AnonymousPathManager",
                    "anonymous path key material length mismatch",
                ));
            }
        }
        for (session_id, control) in &self.control_sessions {
            if control.session_id != *session_id {
                return Err(super::invariant::InvariantViolation::new(
                    "AnonymousPathManager",
                    "anonymous path control session id/key mismatch",
                ));
            }
        }
        Ok(())
    }
}

/// Runtime-owned anonymous established-path lifecycle manager.
#[aura_macros::service_surface(
    families = "Establish",
    object_categories = "transport_protocol,runtime_derived_local,proof_accounting",
    discover = "rendezvous_descriptor_views_and_selected_establish_routes",
    permit = "runtime_capability_budget_and_path_admission",
    transfer = "anonymous_path_manager_setup_and_path_reuse",
    select = "anonymous_path_manager_and_service_registry",
    authoritative = "EstablishedPath,EncryptedAnonymousSetupObject",
    runtime_local = "established_paths,path_replay_window,path_cleanup_state",
    category = "service_surface"
)]
#[aura_macros::actor_owned(
    owner = "anonymous_path_manager",
    domain = "anonymous_path",
    gate = "anonymous_path_command_ingress",
    command = AnonymousPathManagerCommand,
    capacity = 64,
    category = "actor_owned"
)]
#[derive(Clone)]
pub struct AnonymousPathManager {
    config: AnonymousPathManagerConfig,
    registry: Arc<ServiceRegistry>,
    route_crypto: Arc<dyn RouteCryptoEffects + Send + Sync>,
    state: Arc<RwLock<PathState>>,
}

impl AnonymousPathManager {
    pub fn new(config: AnonymousPathManagerConfig, registry: Arc<ServiceRegistry>) -> Self {
        Self::with_route_crypto(config, registry, Arc::new(RealRouteCryptoHandler::new()))
    }

    pub fn with_route_crypto(
        config: AnonymousPathManagerConfig,
        registry: Arc<ServiceRegistry>,
        route_crypto: Arc<dyn RouteCryptoEffects + Send + Sync>,
    ) -> Self {
        Self {
            config,
            registry,
            route_crypto,
            state: Arc::new(RwLock::new(PathState::default())),
        }
    }

    #[allow(dead_code)] // Cleanup target (2026-07): remove if callers never need read-only config access outside tests.
    pub fn config(&self) -> &AnonymousPathManagerConfig {
        &self.config
    }

    #[allow(dead_code)] // Cleanup target (2026-07): remove if projection snapshots remain test-only after choreography migration.
    pub async fn projection(&self) -> AnonymousPathProjection {
        let state = self.state.read().await;
        AnonymousPathProjection {
            active_paths: state.established_paths.len(),
            replay_window_entries: state.replay_order.len(),
            active_control_sessions: state.control_sessions.len(),
            last_cleanup_ms: state.last_cleanup_ms,
        }
    }

    pub async fn open_establish_session(
        &self,
        scope: ContextId,
        destination: AuthorityId,
        route: &Route,
        owner_id: impl Into<String>,
        now_ms: u64,
    ) -> Result<AnonymousPathEstablishControl, AnonymousPathManagerError> {
        if route.hops.is_empty() {
            return Err(AnonymousPathManagerError::DirectRouteNotAnonymous);
        }
        let owner_id = owner_id.into();
        let scope_keys = control_scope_keys(scope, destination, route);
        let mut state = self.state.write().await;
        let session_id = state.next_control_session_id;
        state.next_control_session_id = state.next_control_session_id.saturating_add(1);
        let generation = 1;
        let deadline_ms = now_ms.saturating_add(self.config.establish_timeout.as_millis() as u64);
        let control_state = AnonymousPathControlSessionState {
            session_id,
            deadline_ms,
            owner_id: owner_id.clone(),
            generation,
            scope_keys: scope_keys.clone(),
            readiness_witness: None,
            cancellation_witness: None,
            status: AnonymousPathEstablishStatus::Open,
        };
        state
            .control_sessions
            .insert(session_id, control_state.clone());
        let control = control_from_state(&control_state, "unclaimed".to_string(), 0);
        state.validate().map_err(|error| {
            AnonymousPathManagerError::Serialization(format!("state validation failed: {error}"))
        })?;
        Ok(control)
    }

    pub async fn establish_path(
        &self,
        scope: ContextId,
        destination: AuthorityId,
        route: Route,
        random: &(impl RandomEffects + ?Sized),
        now_ms: u64,
    ) -> Result<(EstablishedPath, EncryptedAnonymousSetupObject), AnonymousPathManagerError> {
        let control = self
            .open_establish_session(scope, destination, &route, "anonymous_path_manager", now_ms)
            .await?;
        let setup_nonce = random.random_bytes_32().await;
        let (path, setup, _) = self
            .establish_path_with_control_and_nonce(
                &control,
                scope,
                destination,
                route,
                setup_nonce,
                now_ms,
            )
            .await?;
        Ok((path, setup))
    }

    pub async fn establish_path_with_control(
        &self,
        control: &AnonymousPathEstablishControl,
        scope: ContextId,
        destination: AuthorityId,
        route: Route,
        random: &(impl RandomEffects + ?Sized),
        now_ms: u64,
    ) -> Result<
        (
            EstablishedPath,
            EncryptedAnonymousSetupObject,
            AnonymousPathEstablishControl,
        ),
        AnonymousPathManagerError,
    > {
        let setup_nonce = random.random_bytes_32().await;
        self.establish_path_with_control_and_nonce(
            control,
            scope,
            destination,
            route,
            setup_nonce,
            now_ms,
        )
        .await
    }

    async fn establish_path_with_control_and_nonce(
        &self,
        control: &AnonymousPathEstablishControl,
        scope: ContextId,
        destination: AuthorityId,
        route: Route,
        setup_nonce: [u8; 32],
        now_ms: u64,
    ) -> Result<
        (
            EstablishedPath,
            EncryptedAnonymousSetupObject,
            AnonymousPathEstablishControl,
        ),
        AnonymousPathManagerError,
    > {
        if route.hops.is_empty() {
            return Err(AnonymousPathManagerError::DirectRouteNotAnonymous);
        }

        let state = self.state.read().await;
        let Some(control_state) = state.control_sessions.get(&control.session_id).cloned() else {
            let cancelled = cancellation_control(
                control.clone(),
                next_witness_id_snapshot(&state),
                "stale_owner".to_string(),
            );
            return Err(AnonymousPathManagerError::StaleOwner {
                session_id: control.session_id,
                attempted_owner: control.ownership_capability.owner_id.clone(),
                attempted_generation: control.ownership_capability.generation,
                control: Box::new(cancelled),
            });
        };
        drop(state);

        if control_state.owner_id != control.ownership_capability.owner_id
            || control_state.generation != control.ownership_capability.generation
        {
            let mut state = self.state.write().await;
            let witness_id = next_witness_id(&mut state);
            let cancelled = cancel_control_session(
                state
                    .control_sessions
                    .get_mut(&control.session_id)
                    .expect("control session exists"),
                witness_id,
                "stale_owner".to_string(),
            );
            return Err(AnonymousPathManagerError::StaleOwner {
                session_id: control.session_id,
                attempted_owner: control.ownership_capability.owner_id.clone(),
                attempted_generation: control.ownership_capability.generation,
                control: Box::new(cancelled),
            });
        }

        if now_ms > control_state.deadline_ms {
            let mut state = self.state.write().await;
            let witness_id = next_witness_id(&mut state);
            let cancelled = cancel_control_session(
                state
                    .control_sessions
                    .get_mut(&control.session_id)
                    .expect("control session exists"),
                witness_id,
                "timeout".to_string(),
            );
            return Err(AnonymousPathManagerError::EstablishTimedOut {
                session_id: control.session_id,
                deadline_ms: control_state.deadline_ms,
                now_ms,
                control: Box::new(cancelled),
            });
        }

        let replay_window_id = replay_window_id(scope, destination, &route, setup_nonce)?;
        let path_id = path_id(scope, destination, &route, setup_nonce)?;
        let valid_until = now_ms.saturating_add(self.config.path_ttl.as_millis() as u64);
        let route_secret_seed = route_secret_seed(path_id, &route)?;
        let hop_keys = self
            .derive_hop_key_materials(route_secret_seed, route.hops.len())
            .await?;
        let forward_hop_keys = hop_keys.iter().map(|keys| keys.0).collect::<Vec<_>>();
        let backward_hop_keys = hop_keys.iter().map(|keys| keys.1).collect::<Vec<_>>();
        let setup_ephemeral_private_key = setup_nonce;
        let setup_ephemeral_public_key = self
            .route_crypto
            .route_public_key(setup_ephemeral_private_key)
            .await
            .map_err(|error| AnonymousPathManagerError::RouteCrypto(error.to_string()))?;
        let setup_peel_keys = self
            .derive_setup_peel_keys(scope, path_id, setup_ephemeral_private_key, &route)
            .await?;
        let path = EstablishedPath {
            path_id,
            scope,
            profile: ServiceProfile::AnonymousPathEstablish,
            route: route.clone(),
            established_at_ms: now_ms,
            valid_until,
            link_protection: LinkProtectionMode::TransportLink,
            path_protection: PathProtectionMode::Encrypted,
            forward_hop_keys: forward_hop_keys.clone(),
            backward_hop_keys: backward_hop_keys.clone(),
        };

        let setup = EncryptedAnonymousSetupObject {
            established_path: path.as_ref(),
            setup_ephemeral_public_key,
            link_protection: LinkProtectionMode::TransportLink,
            path_protection: PathProtectionMode::Encrypted,
            hop_count: route.hops.len() as u8,
            root: self
                .build_encrypted_setup_layers(
                    &route,
                    valid_until,
                    replay_window_id,
                    &setup_peel_keys,
                    &forward_hop_keys,
                    &backward_hop_keys,
                )
                .await?,
        };

        let mut state = self.state.write().await;
        if state.replay_seen.contains(&replay_window_id) {
            let witness_id = next_witness_id(&mut state);
            let cancelled = cancel_control_session(
                state
                    .control_sessions
                    .get_mut(&control.session_id)
                    .expect("control session exists"),
                witness_id,
                "replay_rejected".to_string(),
            );
            return Err(AnonymousPathManagerError::ReplayRejected {
                session_id: control.session_id,
                control: Box::new(cancelled),
            });
        }
        remember_replay_marker(
            &mut state,
            replay_window_id,
            self.config.replay_window_entries,
        );
        state.established_paths.insert(path_id, path.clone());
        let readiness_witness_id = next_witness_id(&mut state);
        let completed_control = complete_control_session(
            state
                .control_sessions
                .get_mut(&control.session_id)
                .expect("control session exists"),
            readiness_witness_id,
            path_id,
        );
        state.validate().map_err(|error| {
            AnonymousPathManagerError::Serialization(format!("state validation failed: {error}"))
        })?;
        drop(state);

        self.registry
            .record_selection_state(
                scope,
                SelectionState {
                    family: ServiceFamily::Establish,
                    selected_authorities: route.hops.iter().map(|hop| hop.authority_id).collect(),
                    epoch: None,
                    bounded_residency_remaining: None,
                },
            )
            .await;

        Ok((path, setup, completed_control))
    }

    pub async fn cancel_establish_session(
        &self,
        control: &AnonymousPathEstablishControl,
        reason: impl Into<String>,
    ) -> Result<AnonymousPathEstablishControl, AnonymousPathManagerError> {
        let mut state = self.state.write().await;
        let witness_id = next_witness_id(&mut state);
        let Some(control_state) = state.control_sessions.get_mut(&control.session_id) else {
            return Err(AnonymousPathManagerError::StaleOwner {
                session_id: control.session_id,
                attempted_owner: control.ownership_capability.owner_id.clone(),
                attempted_generation: control.ownership_capability.generation,
                control: Box::new(cancellation_control(
                    control.clone(),
                    witness_id,
                    "stale_owner".to_string(),
                )),
            });
        };
        if control_state.owner_id != control.ownership_capability.owner_id
            || control_state.generation != control.ownership_capability.generation
        {
            let cancelled =
                cancel_control_session(control_state, witness_id, "stale_owner".to_string());
            return Err(AnonymousPathManagerError::StaleOwner {
                session_id: control.session_id,
                attempted_owner: control.ownership_capability.owner_id.clone(),
                attempted_generation: control.ownership_capability.generation,
                control: Box::new(cancelled),
            });
        }
        Ok(cancel_control_session(
            control_state,
            witness_id,
            reason.into(),
        ))
    }

    pub async fn reuse_established_path(
        &self,
        path_ref: &EstablishedPathRef,
        now_ms: u64,
    ) -> Result<EstablishedPath, AnonymousPathManagerError> {
        let path = self
            .state
            .read()
            .await
            .established_paths
            .get(&path_ref.path_id)
            .cloned()
            .ok_or(AnonymousPathManagerError::PathNotFound)?;

        if !path.is_valid_at(now_ms) {
            return Err(AnonymousPathManagerError::PathExpired {
                valid_until: path.valid_until,
                now_ms,
            });
        }

        Ok(path)
    }

    pub async fn cleanup_expired_paths(&self, now_ms: u64) -> usize {
        let mut state = self.state.write().await;
        let before = state.established_paths.len();
        state
            .established_paths
            .retain(|_, path| path.is_valid_at(now_ms));
        while state.replay_order.len() > self.config.replay_window_entries {
            if let Some(marker) = state.replay_order.pop_front() {
                state.replay_seen.remove(&marker);
            }
        }
        state.last_cleanup_ms = Some(now_ms);
        let removed = before.saturating_sub(state.established_paths.len());
        let _ = state.validate();
        removed
    }

    async fn derive_hop_key_materials(
        &self,
        route_secret_seed: [u8; 32],
        hop_count: usize,
    ) -> Result<Vec<([u8; 32], [u8; 32])>, AnonymousPathManagerError> {
        let mut keys = Vec::with_capacity(hop_count);
        for index in 0..hop_count {
            let material = self
                .route_crypto
                .derive_hop_key_material(route_secret_seed, index as u16)
                .await
                .map_err(|error| AnonymousPathManagerError::RouteCrypto(error.to_string()))?;
            keys.push((material.forward_key, material.backward_key));
        }
        Ok(keys)
    }

    async fn derive_setup_peel_keys(
        &self,
        scope: ContextId,
        path_id: [u8; 32],
        setup_ephemeral_private_key: [u8; 32],
        route: &Route,
    ) -> Result<Vec<[u8; 32]>, AnonymousPathManagerError> {
        let mut keys = Vec::with_capacity(route.hops.len());
        for (index, hop) in route.hops.iter().enumerate() {
            let hop_public_key = hop
                .route_layer_public_key
                .ok_or(AnonymousPathManagerError::MissingRouteLayerPublicKey)?;
            let context = setup_peel_context(scope, path_id, hop.authority_id, index as u16)?;
            let key = self
                .route_crypto
                .derive_route_setup_key(
                    setup_ephemeral_private_key,
                    hop_public_key,
                    context.as_slice(),
                )
                .await
                .map_err(|error| AnonymousPathManagerError::RouteCrypto(error.to_string()))?;
            keys.push(key);
        }
        Ok(keys)
    }

    async fn build_encrypted_setup_layers(
        &self,
        route: &Route,
        valid_until: u64,
        replay_window_id: [u8; 32],
        setup_peel_keys: &[[u8; 32]],
        forward_hop_keys: &[[u8; 32]],
        backward_hop_keys: &[[u8; 32]],
    ) -> Result<Option<Box<EncryptedAnonymousSetupLayer>>, AnonymousPathManagerError> {
        let mut next = None;
        for (index, hop) in route.hops.iter().enumerate().rev() {
            let predecessor = if index == 0 {
                None
            } else {
                Some(route.hops[index - 1].link_endpoint.clone())
            };
            let successor = if index + 1 < route.hops.len() {
                Some(route.hops[index + 1].link_endpoint.clone())
            } else {
                Some(route.destination.clone())
            };
            let payload = EncryptedSetupPayload {
                hop_view: AnonymousHopView {
                    hop_authority: Some(hop.authority_id),
                    predecessor,
                    successor,
                    valid_until,
                    replay_window_id,
                    forward_path_secret: forward_hop_keys[index],
                    backward_path_secret: backward_hop_keys[index],
                },
                next,
            };
            let plaintext = serialization::to_vec(&payload)
                .map_err(|error| AnonymousPathManagerError::Serialization(error.to_string()))?;
            let nonce = setup_layer_nonce(index as u16);
            let aad = setup_layer_aad(hop.authority_id, index as u16)?;
            let ciphertext = self
                .route_crypto
                .encrypt_hop_layer(setup_peel_keys[index], nonce, &aad, &plaintext)
                .await
                .map_err(|error| AnonymousPathManagerError::RouteCrypto(error.to_string()))?;
            next = Some(Box::new(EncryptedAnonymousSetupLayer {
                hop_authority: Some(hop.authority_id),
                nonce,
                ciphertext,
            }));
        }
        Ok(next)
    }

    pub async fn peel_setup_layer(
        &self,
        layer: &EncryptedAnonymousSetupLayer,
        hop_index: usize,
        setup_peel_key: [u8; 32],
    ) -> Result<
        (AnonymousHopView, Option<Box<EncryptedAnonymousSetupLayer>>),
        AnonymousPathManagerError,
    > {
        let aad = setup_layer_aad(
            layer.hop_authority.ok_or_else(|| {
                AnonymousPathManagerError::RouteCrypto("missing hop authority".into())
            })?,
            hop_index as u16,
        )?;
        let plaintext = self
            .route_crypto
            .decrypt_hop_layer(setup_peel_key, layer.nonce, &aad, &layer.ciphertext)
            .await
            .map_err(|error| AnonymousPathManagerError::RouteCrypto(error.to_string()))?;
        let payload = serialization::from_slice::<EncryptedSetupPayload>(&plaintext)
            .map_err(|error| AnonymousPathManagerError::Serialization(error.to_string()))?;
        Ok((payload.hop_view, payload.next))
    }

    pub async fn derive_setup_peel_key_for_hop(
        &self,
        setup: &EncryptedAnonymousSetupObject,
        layer: &EncryptedAnonymousSetupLayer,
        hop_index: usize,
        route_private_key: [u8; 32],
    ) -> Result<[u8; 32], AnonymousPathManagerError> {
        let hop_authority = layer.hop_authority.ok_or_else(|| {
            AnonymousPathManagerError::RouteCrypto("missing hop authority".into())
        })?;
        let context = setup_peel_context(
            setup.established_path.scope,
            setup.established_path.path_id,
            hop_authority,
            hop_index as u16,
        )?;
        self.route_crypto
            .derive_route_setup_key(
                route_private_key,
                setup.setup_ephemeral_public_key,
                context.as_slice(),
            )
            .await
            .map_err(|error| AnonymousPathManagerError::RouteCrypto(error.to_string()))
    }

    pub async fn wrap_reply_payload(
        &self,
        path: &EstablishedPath,
        terminal_payload: &[u8],
    ) -> Result<Vec<u8>, AnonymousPathManagerError> {
        ensure_reply_layer_size(
            "plaintext",
            terminal_payload.len(),
            self.config.max_reply_layer_plaintext_bytes,
        )?;
        let counter = self.next_reply_counter(path.path_id).await?;
        let mut current = terminal_payload.to_vec();
        for index in (0..path.backward_hop_keys.len()).rev() {
            ensure_reply_layer_size(
                "plaintext",
                current.len(),
                self.config.max_reply_layer_plaintext_bytes,
            )?;
            let hop_index = index as u16;
            let nonce = reply_layer_nonce(hop_index, counter);
            let aad = reply_layer_aad(path.path_id, hop_index, counter)?;
            let ciphertext = self
                .route_crypto
                .encrypt_hop_layer(path.backward_hop_keys[index], nonce, &aad, &current)
                .await
                .map_err(|error| AnonymousPathManagerError::RouteCrypto(error.to_string()))?;
            ensure_reply_layer_size(
                "ciphertext",
                ciphertext.len(),
                self.config.max_reply_layer_ciphertext_bytes,
            )?;
            current = serialization::to_vec(&EncryptedReplyLayerFrame {
                counter,
                ciphertext,
            })
            .map_err(|error| AnonymousPathManagerError::Serialization(error.to_string()))?;
            ensure_reply_layer_size(
                "frame",
                current.len(),
                self.config.max_reply_layer_frame_bytes,
            )?;
        }
        Ok(current)
    }

    pub async fn peel_reply_layer(
        &self,
        path: &EstablishedPath,
        hop_index: usize,
        layer: &[u8],
    ) -> Result<Vec<u8>, AnonymousPathManagerError> {
        ensure_reply_layer_size(
            "frame",
            layer.len(),
            self.config.max_reply_layer_frame_bytes,
        )?;
        let frame = serialization::from_slice::<EncryptedReplyLayerFrame>(layer)
            .map_err(|error| AnonymousPathManagerError::Serialization(error.to_string()))?;
        ensure_reply_layer_size(
            "ciphertext",
            frame.ciphertext.len(),
            self.config.max_reply_layer_ciphertext_bytes,
        )?;
        let hop_index_u16 = hop_index as u16;
        let marker = reply_replay_marker(path.path_id, hop_index_u16, frame.counter)?;
        {
            let state = self.state.read().await;
            if state.replay_seen.contains(&marker) {
                return Err(AnonymousPathManagerError::ReplyReplayRejected {
                    path_id: hex::encode(path.path_id),
                    hop_index: hop_index_u16,
                    counter: frame.counter,
                });
            }
        }

        let nonce = reply_layer_nonce(hop_index_u16, frame.counter);
        let aad = reply_layer_aad(path.path_id, hop_index_u16, frame.counter)?;
        let plaintext = self
            .route_crypto
            .decrypt_hop_layer(
                path.backward_hop_keys[hop_index],
                nonce,
                &aad,
                &frame.ciphertext,
            )
            .await
            .map_err(|error| AnonymousPathManagerError::RouteCrypto(error.to_string()))?;
        ensure_reply_layer_size(
            "plaintext",
            plaintext.len(),
            self.config.max_reply_layer_plaintext_bytes,
        )?;

        let mut state = self.state.write().await;
        remember_replay_marker(&mut state, marker, self.config.replay_window_entries);
        Ok(plaintext)
    }

    async fn next_reply_counter(
        &self,
        path_id: [u8; 32],
    ) -> Result<u64, AnonymousPathManagerError> {
        let mut state = self.state.write().await;
        let counter = state.reply_counters.entry(path_id).or_insert(0);
        let current = *counter;
        *counter = counter.checked_add(1).ok_or_else(|| {
            AnonymousPathManagerError::RouteCrypto("reply counter overflow".to_string())
        })?;
        Ok(current)
    }

    fn spawn_cleanup_task(
        &self,
        tasks: TaskGroup,
        time: Arc<dyn PhysicalTimeEffects + Send + Sync>,
    ) {
        let manager = self.clone();
        let interval = self.config.cleanup_interval;
        let _cleanup_task_handle = tasks.spawn_interval_until_named(
            "anonymous_path.cleanup",
            time.clone(),
            interval,
            move || {
                let manager = manager.clone();
                let time = time.clone();
                async move {
                    let now_ms = match time.physical_time().await {
                        Ok(time) => time.ts_ms,
                        Err(error) => {
                            tracing::warn!(
                                error = %error,
                                "Anonymous path cleanup: failed to get time"
                            );
                            return true;
                        }
                    };
                    let _ = manager.cleanup_expired_paths(now_ms).await;
                    true
                }
            },
        );
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl RuntimeService for AnonymousPathManager {
    fn name(&self) -> &'static str {
        "anonymous_path_manager"
    }

    fn dependencies(&self) -> &[&'static str] {
        &["rendezvous_manager"]
    }

    async fn start(&self, ctx: &RuntimeServiceContext) -> Result<(), ServiceError> {
        let task_group = ctx.tasks().group(self.name());
        self.spawn_cleanup_task(task_group.clone(), ctx.time_effects());
        let mut state = self.state.write().await;
        state.cleanup_tasks = Some(task_group);
        state.lifecycle = ServiceHealth::Healthy;
        Ok(())
    }

    async fn stop(&self) -> Result<(), ServiceError> {
        let tasks = {
            let mut state = self.state.write().await;
            state.lifecycle = ServiceHealth::Stopping;
            state.cleanup_tasks.take()
        };

        if let Some(tasks) = tasks {
            tasks.shutdown();
        }

        let mut state = self.state.write().await;
        state.lifecycle = ServiceHealth::Stopped;
        Ok(())
    }

    async fn health(&self) -> ServiceHealth {
        self.state.read().await.lifecycle.clone()
    }
}

fn path_id(
    scope: ContextId,
    destination: AuthorityId,
    route: &Route,
    setup_nonce: [u8; 32],
) -> Result<[u8; 32], AnonymousPathManagerError> {
    let bytes = serialization::to_vec(&(scope, destination, route, setup_nonce, "path"))
        .map_err(|error| AnonymousPathManagerError::Serialization(error.to_string()))?;
    Ok(hash(&bytes))
}

fn control_scope_keys(
    scope: ContextId,
    destination: AuthorityId,
    route: &Route,
) -> BTreeSet<String> {
    let mut keys = BTreeSet::from([
        format!("context:{scope}"),
        format!("destination:{destination}"),
        "service:establish".to_string(),
    ]);
    for hop in &route.hops {
        keys.insert(format!("hop:{}", hop.authority_id));
    }
    keys
}

fn control_from_state(
    state: &AnonymousPathControlSessionState,
    from_owner_id: String,
    from_generation: u64,
) -> AnonymousPathEstablishControl {
    AnonymousPathEstablishControl {
        session_id: state.session_id,
        deadline_ms: state.deadline_ms,
        ownership_capability: ownership_capability_evidence(
            state.session_id,
            &state.owner_id,
            state.generation,
            &state.scope_keys,
        ),
        ownership_receipt: ownership_receipt_evidence(
            state.session_id,
            from_owner_id,
            from_generation,
            &state.owner_id,
            state.generation,
            &state.scope_keys,
        ),
        readiness_witness: state.readiness_witness.clone(),
        cancellation_witness: state.cancellation_witness.clone(),
        status: state.status.clone(),
    }
}

fn next_witness_id(state: &mut PathState) -> u64 {
    let next = state.next_control_witness_id;
    state.next_control_witness_id = state.next_control_witness_id.saturating_add(1);
    next
}

fn next_witness_id_snapshot(state: &PathState) -> u64 {
    state.next_control_witness_id
}

fn ownership_capability_evidence(
    session_id: u64,
    owner_id: &str,
    generation: u64,
    scope_keys: &BTreeSet<String>,
) -> AnonymousPathOwnershipCapabilityEvidence {
    AnonymousPathOwnershipCapabilityEvidence {
        session_id,
        owner_id: owner_id.to_string(),
        generation,
        scope_keys: scope_keys.clone(),
        #[cfg(feature = "choreo-backend-telltale-machine")]
        telltale: OwnershipCapability {
            session_id: session_id as usize,
            owner_id: owner_id.to_string(),
            generation,
            scope: OwnershipScope::Fragments(scope_keys.clone()),
        },
    }
}

fn ownership_receipt_evidence(
    session_id: u64,
    from_owner_id: String,
    from_generation: u64,
    to_owner_id: &str,
    to_generation: u64,
    scope_keys: &BTreeSet<String>,
) -> AnonymousPathOwnershipReceiptEvidence {
    AnonymousPathOwnershipReceiptEvidence {
        session_id,
        claim_id: to_generation,
        from_owner_id: from_owner_id.clone(),
        to_owner_id: to_owner_id.to_string(),
        to_generation,
        scope_keys: scope_keys.clone(),
        #[cfg(feature = "choreo-backend-telltale-machine")]
        telltale: OwnershipReceipt {
            session_id: session_id as usize,
            claim_id: to_generation,
            from_owner_id,
            from_generation,
            to_owner_id: to_owner_id.to_string(),
            to_generation,
            scope: OwnershipScope::Fragments(scope_keys.clone()),
        },
    }
}

fn readiness_evidence(
    witness_id: u64,
    session_id: u64,
    owner_id: &str,
    generation: u64,
    scope_keys: &BTreeSet<String>,
    predicate_ref: &str,
) -> AnonymousPathReadinessEvidence {
    AnonymousPathReadinessEvidence {
        witness_id,
        session_id,
        owner_id: owner_id.to_string(),
        generation,
        predicate_ref: predicate_ref.to_string(),
        #[cfg(feature = "choreo-backend-telltale-machine")]
        telltale: ReadinessWitness {
            witness_id,
            session_id: session_id as usize,
            owner_id: owner_id.to_string(),
            generation,
            scope: OwnershipScope::Fragments(scope_keys.clone()),
            predicate_ref: predicate_ref.to_string(),
        },
    }
}

fn cancellation_evidence(
    witness_id: u64,
    session_id: u64,
    owner_id: &str,
    generation: u64,
    reason: &str,
) -> AnonymousPathCancellationEvidence {
    AnonymousPathCancellationEvidence {
        witness_id,
        session_id,
        owner_id: owner_id.to_string(),
        generation,
        reason: reason.to_string(),
        #[cfg(feature = "choreo-backend-telltale-machine")]
        telltale: CancellationWitness {
            witness_id,
            session_id: session_id as usize,
            owner_id: owner_id.to_string(),
            generation,
            reason: OwnershipTerminalReason::TransferCommitFailed {
                owner_id: owner_id.to_string(),
                claim_id: generation,
                reason: reason.to_string(),
            },
        },
    }
}

fn complete_control_session(
    state: &mut AnonymousPathControlSessionState,
    witness_id: u64,
    path_id: [u8; 32],
) -> AnonymousPathEstablishControl {
    state.status = AnonymousPathEstablishStatus::Ready { path_id };
    state.readiness_witness = Some(readiness_evidence(
        witness_id,
        state.session_id,
        &state.owner_id,
        state.generation,
        &state.scope_keys,
        "anonymous_path_establish_ready",
    ));
    control_from_state(state, "unclaimed".to_string(), 0)
}

fn cancel_control_session(
    state: &mut AnonymousPathControlSessionState,
    witness_id: u64,
    reason: String,
) -> AnonymousPathEstablishControl {
    state.status = AnonymousPathEstablishStatus::Cancelled {
        reason: reason.clone(),
    };
    state.cancellation_witness = Some(cancellation_evidence(
        witness_id,
        state.session_id,
        &state.owner_id,
        state.generation,
        &reason,
    ));
    control_from_state(state, "unclaimed".to_string(), 0)
}

fn cancellation_control(
    mut control: AnonymousPathEstablishControl,
    witness_id: u64,
    reason: String,
) -> AnonymousPathEstablishControl {
    control.status = AnonymousPathEstablishStatus::Cancelled {
        reason: reason.clone(),
    };
    control.cancellation_witness = Some(cancellation_evidence(
        witness_id,
        control.session_id,
        &control.ownership_capability.owner_id,
        control.ownership_capability.generation,
        &reason,
    ));
    control
}

fn replay_window_id(
    scope: ContextId,
    destination: AuthorityId,
    route: &Route,
    setup_nonce: [u8; 32],
) -> Result<[u8; 32], AnonymousPathManagerError> {
    let bytes = serialization::to_vec(&(scope, destination, route, setup_nonce, "replay"))
        .map_err(|error| AnonymousPathManagerError::Serialization(error.to_string()))?;
    Ok(hash(&bytes))
}

fn route_secret_seed(
    path_id: [u8; 32],
    route: &Route,
) -> Result<[u8; 32], AnonymousPathManagerError> {
    if route
        .hops
        .iter()
        .any(|hop| hop.route_layer_public_key.is_none())
    {
        return Err(AnonymousPathManagerError::MissingRouteLayerPublicKey);
    }
    let mut material = path_id.to_vec();
    for (index, hop) in route.hops.iter().enumerate() {
        material.extend_from_slice(&(index as u64).to_le_bytes());
        material.extend_from_slice(&hop.authority_id.to_bytes());
        material.extend_from_slice(&hop.route_layer_public_key.expect("validated above"));
    }
    Ok(hash(&material))
}

fn setup_layer_nonce(hop_index: u16) -> [u8; 12] {
    let mut nonce = [0u8; 12];
    nonce[..2].copy_from_slice(&hop_index.to_le_bytes());
    nonce[2..].copy_from_slice(b"asetup-hop");
    nonce
}

fn reply_layer_nonce(hop_index: u16, counter: u64) -> [u8; 12] {
    let mut nonce = [0u8; 12];
    nonce[..2].copy_from_slice(&hop_index.to_le_bytes());
    nonce[2..10].copy_from_slice(&counter.to_le_bytes());
    nonce[10..].copy_from_slice(b"rp");
    nonce
}

fn setup_layer_aad(
    authority_id: AuthorityId,
    hop_index: u16,
) -> Result<Vec<u8>, AnonymousPathManagerError> {
    serialization::to_vec(&(b"aura.setup.layer.v1", authority_id, hop_index))
        .map_err(|error| AnonymousPathManagerError::Serialization(error.to_string()))
}

fn setup_peel_context(
    scope: ContextId,
    path_id: [u8; 32],
    authority_id: AuthorityId,
    hop_index: u16,
) -> Result<Vec<u8>, AnonymousPathManagerError> {
    serialization::to_vec(&(
        b"aura.route.setup.peel.v1",
        scope,
        path_id,
        authority_id,
        hop_index,
    ))
    .map_err(|error| AnonymousPathManagerError::Serialization(error.to_string()))
}

fn reply_layer_aad(
    path_id: [u8; 32],
    hop_index: u16,
    counter: u64,
) -> Result<Vec<u8>, AnonymousPathManagerError> {
    serialization::to_vec(&(b"aura.reply.layer.v2", path_id, hop_index, counter))
        .map_err(|error| AnonymousPathManagerError::Serialization(error.to_string()))
}

fn reply_replay_marker(
    path_id: [u8; 32],
    hop_index: u16,
    counter: u64,
) -> Result<[u8; 32], AnonymousPathManagerError> {
    let material = serialization::to_vec(&(b"aura.reply.replay.v1", path_id, hop_index, counter))
        .map_err(|error| AnonymousPathManagerError::Serialization(error.to_string()))?;
    Ok(hash(&material))
}

fn ensure_reply_layer_size(
    kind: &'static str,
    actual: usize,
    max: usize,
) -> Result<(), AnonymousPathManagerError> {
    if actual > max {
        return Err(AnonymousPathManagerError::ReplyLayerTooLarge { kind, actual, max });
    }
    Ok(())
}

fn remember_replay_marker(state: &mut PathState, marker: [u8; 32], max_entries: usize) {
    state.replay_seen.insert(marker);
    state.replay_order.push_back(marker);
    while state.replay_order.len() > max_entries {
        if let Some(evicted) = state.replay_order.pop_front() {
            state.replay_seen.remove(&evicted);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::RandomCoreEffects;
    use aura_core::service::{
        LinkEndpoint, LinkProtectionMode, LinkProtocol, MoveEnvelope, MovePathBinding,
        PathProtectionMode, RelayHop, Route,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TestRandom {
        next: AtomicUsize,
        seeds: Vec<[u8; 32]>,
    }

    impl TestRandom {
        fn single(seed: u8) -> Self {
            Self {
                next: AtomicUsize::new(0),
                seeds: vec![[seed; 32]],
            }
        }

        fn sequence(seeds: impl IntoIterator<Item = u8>) -> Self {
            Self {
                next: AtomicUsize::new(0),
                seeds: seeds.into_iter().map(|seed| [seed; 32]).collect(),
            }
        }

        fn draws(&self) -> usize {
            self.next.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl RandomCoreEffects for TestRandom {
        async fn random_bytes(&self, len: usize) -> Vec<u8> {
            let seed = self.random_bytes_32().await;
            seed.into_iter().cycle().take(len).collect()
        }

        async fn random_bytes_32(&self) -> [u8; 32] {
            let index = self.next.fetch_add(1, Ordering::SeqCst);
            self.seeds
                .get(index)
                .copied()
                .unwrap_or_else(|| [index as u8; 32])
        }

        async fn random_u64(&self) -> u64 {
            u64::from_le_bytes(self.random_bytes_32().await[..8].try_into().unwrap())
        }
    }

    fn authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn context(seed: u8) -> ContextId {
        ContextId::new_from_entropy([seed; 32])
    }

    fn endpoint(port: u16) -> LinkEndpoint {
        LinkEndpoint::direct(LinkProtocol::Tcp, format!("127.0.0.1:{port}"))
    }

    async fn route_for(manager: &AnonymousPathManager) -> Route {
        Route {
            hops: vec![
                RelayHop {
                    authority_id: authority(2),
                    link_endpoint: endpoint(7000),
                    route_layer_public_key: Some(
                        manager
                            .route_crypto
                            .route_public_key([2; 32])
                            .await
                            .expect("derive first route public key"),
                    ),
                },
                RelayHop {
                    authority_id: authority(3),
                    link_endpoint: endpoint(7001),
                    route_layer_public_key: Some(
                        manager
                            .route_crypto
                            .route_public_key([3; 32])
                            .await
                            .expect("derive second route public key"),
                    ),
                },
            ],
            destination: endpoint(9000),
        }
    }

    #[tokio::test]
    async fn encrypted_anonymous_path_establishment_creates_reusable_path() {
        let registry = Arc::new(ServiceRegistry::new());
        let manager =
            AnonymousPathManager::new(AnonymousPathManagerConfig::for_testing(), registry);
        let random = TestRandom::single(7);
        let route = route_for(&manager).await;

        let (path, setup) = manager
            .establish_path(context(1), authority(9), route, &random, 100)
            .await
            .expect("establish encrypted path");

        assert_eq!(path.profile, ServiceProfile::AnonymousPathEstablish);
        assert_eq!(setup.hop_count, 2);
        assert!(setup.has_root_layer());
        let path_ref = path.as_ref();
        let first_move = MoveEnvelope::opaque(
            MovePathBinding::Established(path_ref.clone()),
            vec![1, 2, 3],
        );
        let second_move = MoveEnvelope::opaque(
            MovePathBinding::Established(path_ref.clone()),
            vec![4, 5, 6],
        );
        assert_eq!(first_move.binding, second_move.binding);
        assert_eq!(path.link_protection, LinkProtectionMode::TransportLink);
        assert_eq!(path.path_protection, PathProtectionMode::Encrypted);
        assert_eq!(setup.link_protection, LinkProtectionMode::TransportLink);
        assert_eq!(setup.path_protection, PathProtectionMode::Encrypted);
        let reused = manager
            .reuse_established_path(&path_ref, 101)
            .await
            .expect("reuse established path");
        assert_eq!(reused, path);
    }

    #[tokio::test]
    async fn establish_path_consumes_one_fresh_nonce_from_random_effects() {
        let registry = Arc::new(ServiceRegistry::new());
        let manager =
            AnonymousPathManager::new(AnonymousPathManagerConfig::for_testing(), registry);
        let random = TestRandom::single(7);
        let route = route_for(&manager).await;

        manager
            .establish_path(context(1), authority(9), route, &random, 100)
            .await
            .expect("establish encrypted path");

        assert_eq!(random.draws(), 1);
    }

    #[tokio::test]
    async fn fresh_establish_entropy_changes_path_replay_and_hop_keys() {
        let registry = Arc::new(ServiceRegistry::new());
        let manager =
            AnonymousPathManager::new(AnonymousPathManagerConfig::for_testing(), registry);
        let random = TestRandom::sequence([7, 8]);
        let route = route_for(&manager).await;

        let (first_path, first_setup) = manager
            .establish_path(context(1), authority(9), route.clone(), &random, 100)
            .await
            .expect("first establish");
        let (second_path, second_setup) = manager
            .establish_path(context(1), authority(9), route, &random, 100)
            .await
            .expect("second establish");

        assert_ne!(first_path.path_id, second_path.path_id);
        assert_ne!(
            first_setup.established_path.path_id,
            second_setup.established_path.path_id
        );
        let (first_view, _) = manager
            .peel_setup_layer(
                first_setup.root.as_ref().expect("first root"),
                0,
                manager
                    .derive_setup_peel_key_for_hop(
                        &first_setup,
                        first_setup.root.as_ref().expect("first root"),
                        0,
                        [2; 32],
                    )
                    .await
                    .expect("derive first setup peel key"),
            )
            .await
            .expect("peel first setup");
        let (second_view, _) = manager
            .peel_setup_layer(
                second_setup.root.as_ref().expect("second root"),
                0,
                manager
                    .derive_setup_peel_key_for_hop(
                        &second_setup,
                        second_setup.root.as_ref().expect("second root"),
                        0,
                        [2; 32],
                    )
                    .await
                    .expect("derive second setup peel key"),
            )
            .await
            .expect("peel second setup");
        assert_ne!(first_view.replay_window_id, second_view.replay_window_id);
        assert_ne!(first_path.forward_hop_keys, second_path.forward_hop_keys);
        assert_ne!(first_path.backward_hop_keys, second_path.backward_hop_keys);
        assert_eq!(random.draws(), 2);
    }

    #[tokio::test]
    async fn encrypted_setup_peels_forward_one_hop_at_a_time() {
        let registry = Arc::new(ServiceRegistry::new());
        let manager =
            AnonymousPathManager::new(AnonymousPathManagerConfig::for_testing(), registry);
        let random = TestRandom::single(7);
        let route = route_for(&manager).await;

        let (_path, setup) = manager
            .establish_path(context(1), authority(9), route, &random, 100)
            .await
            .expect("establish encrypted path");
        let root = setup.root.as_ref().expect("root layer");
        let first_setup_key = manager
            .derive_setup_peel_key_for_hop(&setup, root, 0, [2; 32])
            .await
            .expect("derive first setup peel key");
        let (first_view, next) = manager
            .peel_setup_layer(root, 0, first_setup_key)
            .await
            .expect("peel first layer");
        assert_eq!(first_view.hop_authority, Some(authority(2)));
        assert_eq!(first_view.successor, Some(endpoint(7001)));
        let next = next.expect("second encrypted layer");
        let wrong_setup_key = manager
            .derive_setup_peel_key_for_hop(&setup, &next, 1, [2; 32])
            .await
            .expect("derive wrong setup peel key");
        let wrong_key_error = manager
            .peel_setup_layer(&next, 1, wrong_setup_key)
            .await
            .expect_err("wrong hop key should not inspect deeper layer");
        assert!(matches!(
            wrong_key_error,
            AnonymousPathManagerError::RouteCrypto(_)
        ));
        let second_setup_key = manager
            .derive_setup_peel_key_for_hop(&setup, &next, 1, [3; 32])
            .await
            .expect("derive second setup peel key");
        let (second_view, terminal) = manager
            .peel_setup_layer(&next, 1, second_setup_key)
            .await
            .expect("peel second layer");
        assert_eq!(second_view.hop_authority, Some(authority(3)));
        assert_eq!(second_view.successor, Some(endpoint(9000)));
        assert!(terminal.is_none());
    }

    #[tokio::test]
    async fn encrypted_reply_layers_relayer_backwards_without_exposing_inner_payload() {
        let registry = Arc::new(ServiceRegistry::new());
        let manager =
            AnonymousPathManager::new(AnonymousPathManagerConfig::for_testing(), registry);
        let random = TestRandom::single(4);
        let route = route_for(&manager).await;

        let (path, _) = manager
            .establish_path(context(1), authority(9), route, &random, 100)
            .await
            .expect("establish encrypted path");
        let wrapped = manager
            .wrap_reply_payload(&path, b"terminal-reply")
            .await
            .expect("wrap backward reply");
        let first_hop = manager
            .peel_reply_layer(&path, 0, &wrapped)
            .await
            .expect("first backward peel");
        assert_ne!(first_hop, b"terminal-reply");
        let terminal = manager
            .peel_reply_layer(&path, 1, &first_hop)
            .await
            .expect("second backward peel");
        assert_eq!(terminal, b"terminal-reply");
    }

    #[tokio::test]
    async fn reply_layers_use_fresh_counters_for_reused_paths() {
        let registry = Arc::new(ServiceRegistry::new());
        let manager =
            AnonymousPathManager::new(AnonymousPathManagerConfig::for_testing(), registry);
        let random = TestRandom::single(4);
        let route = route_for(&manager).await;

        let (path, _) = manager
            .establish_path(context(1), authority(9), route, &random, 100)
            .await
            .expect("establish encrypted path");
        let first = manager
            .wrap_reply_payload(&path, b"first-reply")
            .await
            .expect("wrap first reply");
        let second = manager
            .wrap_reply_payload(&path, b"second-reply")
            .await
            .expect("wrap second reply");

        assert_ne!(first, second);
    }

    #[tokio::test]
    async fn reply_layers_reject_replayed_frames() {
        let registry = Arc::new(ServiceRegistry::new());
        let manager =
            AnonymousPathManager::new(AnonymousPathManagerConfig::for_testing(), registry);
        let random = TestRandom::single(4);
        let route = route_for(&manager).await;

        let (path, _) = manager
            .establish_path(context(1), authority(9), route, &random, 100)
            .await
            .expect("establish encrypted path");
        let wrapped = manager
            .wrap_reply_payload(&path, b"terminal-reply")
            .await
            .expect("wrap backward reply");

        let _ = manager
            .peel_reply_layer(&path, 0, &wrapped)
            .await
            .expect("first peel succeeds");
        let replay = manager
            .peel_reply_layer(&path, 0, &wrapped)
            .await
            .expect_err("replayed reply frame must be rejected");

        assert!(matches!(
            replay,
            AnonymousPathManagerError::ReplyReplayRejected { .. }
        ));
    }

    #[tokio::test]
    async fn reply_layers_reject_oversized_ciphertext_before_decrypt() {
        let registry = Arc::new(ServiceRegistry::new());
        let mut config = AnonymousPathManagerConfig::for_testing();
        config.max_reply_layer_frame_bytes = 512;
        config.max_reply_layer_ciphertext_bytes = 16;
        let manager = AnonymousPathManager::new(config, registry);
        let random = TestRandom::single(4);
        let route = route_for(&manager).await;

        let (path, _) = manager
            .establish_path(context(1), authority(9), route, &random, 100)
            .await
            .expect("establish encrypted path");
        let oversized = serialization::to_vec(&EncryptedReplyLayerFrame {
            counter: 1,
            ciphertext: vec![0; 17],
        })
        .expect("serialize oversized frame");

        let err = manager
            .peel_reply_layer(&path, 0, &oversized)
            .await
            .expect_err("oversized reply ciphertext must be rejected before decrypt");

        assert!(matches!(
            err,
            AnonymousPathManagerError::ReplyLayerTooLarge {
                kind: "ciphertext",
                actual: 17,
                max: 16
            }
        ));
    }

    #[tokio::test]
    async fn reply_layers_reject_oversized_terminal_payload_before_encrypt() {
        let registry = Arc::new(ServiceRegistry::new());
        let mut config = AnonymousPathManagerConfig::for_testing();
        config.max_reply_layer_plaintext_bytes = 4;
        let manager = AnonymousPathManager::new(config, registry);
        let random = TestRandom::single(4);
        let route = route_for(&manager).await;

        let (path, _) = manager
            .establish_path(context(1), authority(9), route, &random, 100)
            .await
            .expect("establish encrypted path");
        let err = manager
            .wrap_reply_payload(&path, b"too-large")
            .await
            .expect_err("oversized terminal payload must be rejected before encrypt");

        assert!(matches!(
            err,
            AnonymousPathManagerError::ReplyLayerTooLarge {
                kind: "plaintext",
                actual: 9,
                max: 4
            }
        ));
    }

    #[tokio::test]
    async fn encrypted_anonymous_path_rejects_replay() {
        let registry = Arc::new(ServiceRegistry::new());
        let manager =
            AnonymousPathManager::new(AnonymousPathManagerConfig::for_testing(), registry);
        let route = route_for(&manager).await;

        let first_control = manager
            .open_establish_session(context(1), authority(9), &route, "owner-a", 100)
            .await
            .expect("open first establish session");
        manager
            .establish_path_with_control_and_nonce(
                &first_control,
                context(1),
                authority(9),
                route.clone(),
                [5; 32],
                100,
            )
            .await
            .expect("first establish");
        let second_control = manager
            .open_establish_session(context(1), authority(9), &route, "owner-b", 101)
            .await
            .expect("open second establish session");
        let error = manager
            .establish_path_with_control_and_nonce(
                &second_control,
                context(1),
                authority(9),
                route,
                [5; 32],
                101,
            )
            .await
            .expect_err("duplicate setup should reject");
        assert!(matches!(
            error,
            AnonymousPathManagerError::ReplayRejected { .. }
        ));
    }

    #[tokio::test]
    async fn encrypted_anonymous_path_rejects_expired_reuse() {
        let registry = Arc::new(ServiceRegistry::new());
        let manager =
            AnonymousPathManager::new(AnonymousPathManagerConfig::for_testing(), registry);
        let random = TestRandom::single(4);
        let route = route_for(&manager).await;

        let (path, _) = manager
            .establish_path(context(1), authority(9), route, &random, 100)
            .await
            .expect("establish path");
        let error = manager
            .reuse_established_path(&path.as_ref(), 400)
            .await
            .expect_err("expired path should reject");
        assert!(matches!(
            error,
            AnonymousPathManagerError::PathExpired { .. }
        ));
    }

    #[tokio::test]
    async fn encrypted_anonymous_path_requires_non_direct_route() {
        let registry = Arc::new(ServiceRegistry::new());
        let manager =
            AnonymousPathManager::new(AnonymousPathManagerConfig::for_testing(), registry);
        let random = TestRandom::single(1);

        let error = manager
            .establish_path(
                context(1),
                authority(9),
                Route::direct(endpoint(9000)),
                &random,
                100,
            )
            .await
            .expect_err("direct route should reject");
        assert!(matches!(
            error,
            AnonymousPathManagerError::DirectRouteNotAnonymous
        ));
    }

    #[tokio::test]
    async fn anonymous_path_control_success_is_protocol_visible() {
        let registry = Arc::new(ServiceRegistry::new());
        let manager =
            AnonymousPathManager::new(AnonymousPathManagerConfig::for_testing(), registry);
        let route = route_for(&manager).await;
        let control = manager
            .open_establish_session(context(1), authority(9), &route, "path-owner", 100)
            .await
            .expect("open establish session");
        let (_path, _setup, completed) = manager
            .establish_path_with_control(
                &control,
                context(1),
                authority(9),
                route,
                &TestRandom::single(8),
                110,
            )
            .await
            .expect("complete controlled establish");
        assert!(matches!(
            completed.status,
            AnonymousPathEstablishStatus::Ready { .. }
        ));
        assert!(completed.readiness_witness.is_some());
        assert_eq!(completed.ownership_capability.owner_id, "path-owner");
    }

    #[tokio::test]
    async fn anonymous_path_control_rejects_stale_owner() {
        let registry = Arc::new(ServiceRegistry::new());
        let manager =
            AnonymousPathManager::new(AnonymousPathManagerConfig::for_testing(), registry);
        let route = route_for(&manager).await;
        let mut control = manager
            .open_establish_session(context(1), authority(9), &route, "owner-a", 100)
            .await
            .expect("open establish session");
        control.ownership_capability.generation = 99;
        let error = manager
            .establish_path_with_control(
                &control,
                context(1),
                authority(9),
                route,
                &TestRandom::single(9),
                110,
            )
            .await
            .expect_err("stale owner should reject");
        match error {
            AnonymousPathManagerError::StaleOwner { control, .. } => {
                assert!(control.cancellation_witness.is_some());
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn anonymous_path_control_timeout_is_protocol_visible() {
        let registry = Arc::new(ServiceRegistry::new());
        let manager =
            AnonymousPathManager::new(AnonymousPathManagerConfig::for_testing(), registry);
        let route = route_for(&manager).await;
        let control = manager
            .open_establish_session(context(1), authority(9), &route, "owner-a", 100)
            .await
            .expect("open establish session");
        let error = manager
            .establish_path_with_control(
                &control,
                context(1),
                authority(9),
                route,
                &TestRandom::single(6),
                250,
            )
            .await
            .expect_err("timed out control should reject");
        match error {
            AnonymousPathManagerError::EstablishTimedOut { control, .. } => {
                assert!(control.cancellation_witness.is_some());
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn anonymous_path_control_supports_explicit_cancellation() {
        let registry = Arc::new(ServiceRegistry::new());
        let manager =
            AnonymousPathManager::new(AnonymousPathManagerConfig::for_testing(), registry);
        let route = route_for(&manager).await;
        let control = manager
            .open_establish_session(context(1), authority(9), &route, "owner-a", 100)
            .await
            .expect("open establish session");
        let cancelled = manager
            .cancel_establish_session(&control, "operator_cancelled")
            .await
            .expect("cancel control");
        assert!(matches!(
            cancelled.status,
            AnonymousPathEstablishStatus::Cancelled { .. }
        ));
        assert!(cancelled.cancellation_witness.is_some());
    }
}

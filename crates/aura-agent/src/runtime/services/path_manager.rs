//! Runtime-owned anonymous established-path service.
//!
//! Owns reusable anonymous path lifecycle: transparent setup-object creation,
//! replay suppression, expiry-bounded path state, and runtime-local path reuse.
#![allow(dead_code)]

use super::config_profiles::impl_service_config_profiles;
use super::service_registry::ServiceRegistry;
use super::traits::{RuntimeService, RuntimeServiceContext, ServiceError, ServiceHealth};
use crate::runtime::TaskGroup;
use async_trait::async_trait;
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::hash::hash;
use aura_core::service::{
    EstablishedPath, EstablishedPathRef, LinkProtectionMode, PathProtectionMode, Route,
    SelectionState, ServiceFamily, ServiceProfile, TransparentAnonymousSetupLayer,
    TransparentAnonymousSetupObject,
};
use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_core::util::serialization;
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
    EstablishTransparentPath,
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
}

impl Default for AnonymousPathManagerConfig {
    fn default() -> Self {
        Self {
            path_ttl: Duration::from_secs(30),
            cleanup_interval: Duration::from_secs(10),
            replay_window_entries: 256,
            establish_timeout: Duration::from_secs(5),
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
        }
    }
});

/// Summary projection for runtime-local anonymous path state.
#[allow(dead_code)]
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
    #[error("transparent anonymous setup replay rejected for control session {session_id}")]
    ReplayRejected {
        session_id: u64,
        control: Box<AnonymousPathEstablishControl>,
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
    authoritative = "EstablishedPath,TransparentAnonymousSetupObject",
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
    state: Arc<RwLock<PathState>>,
}

impl AnonymousPathManager {
    pub fn new(config: AnonymousPathManagerConfig, registry: Arc<ServiceRegistry>) -> Self {
        Self {
            config,
            registry,
            state: Arc::new(RwLock::new(PathState::default())),
        }
    }

    #[allow(dead_code)]
    pub fn config(&self) -> &AnonymousPathManagerConfig {
        &self.config
    }

    #[allow(dead_code)]
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

    pub async fn establish_transparent_path(
        &self,
        scope: ContextId,
        destination: AuthorityId,
        route: Route,
        setup_nonce: [u8; 32],
        now_ms: u64,
    ) -> Result<(EstablishedPath, TransparentAnonymousSetupObject), AnonymousPathManagerError> {
        let control = self
            .open_establish_session(scope, destination, &route, "anonymous_path_manager", now_ms)
            .await?;
        let (path, setup, _) = self
            .establish_transparent_path_with_control(
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

    pub async fn establish_transparent_path_with_control(
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
            TransparentAnonymousSetupObject,
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
        let forward_hop_keys = derive_hop_key_stream(path_id, route.hops.len(), b"forward");
        let backward_hop_keys = derive_hop_key_stream(path_id, route.hops.len(), b"backward");
        let path = EstablishedPath {
            path_id,
            scope,
            profile: ServiceProfile::AnonymousPathEstablish,
            route: route.clone(),
            established_at_ms: now_ms,
            valid_until,
            link_protection: LinkProtectionMode::TransportLink,
            path_protection: PathProtectionMode::TransparentDebug,
            forward_hop_keys: forward_hop_keys.clone(),
            backward_hop_keys: backward_hop_keys.clone(),
        };

        let setup = TransparentAnonymousSetupObject {
            established_path: path.as_ref(),
            link_protection: LinkProtectionMode::TransportLink,
            path_protection: PathProtectionMode::TransparentDebug,
            root: build_setup_layers(
                &route,
                valid_until,
                replay_window_id,
                &forward_hop_keys,
                &backward_hop_keys,
            ),
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
    Ok(hash(&bytes).into())
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
    Ok(hash(&bytes).into())
}

fn derive_hop_key_stream(path_id: [u8; 32], hops: usize, domain: &[u8]) -> Vec<[u8; 32]> {
    (0..hops)
        .map(|index| hash(&[domain, &path_id, &(index as u64).to_le_bytes()].concat()).into())
        .collect()
}

fn build_setup_layers(
    route: &Route,
    valid_until: u64,
    replay_window_id: [u8; 32],
    forward_hop_keys: &[[u8; 32]],
    backward_hop_keys: &[[u8; 32]],
) -> Option<Box<TransparentAnonymousSetupLayer>> {
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
        next = Some(Box::new(TransparentAnonymousSetupLayer {
            hop_authority: Some(hop.authority_id),
            predecessor,
            successor,
            valid_until,
            replay_window_id,
            forward_path_secret: forward_hop_keys[index],
            backward_path_secret: backward_hop_keys[index],
            next,
        }));
    }
    next
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
    use aura_core::service::{
        LinkEndpoint, LinkProtectionMode, LinkProtocol, MoveEnvelope, MovePathBinding,
        PathProtectionMode, RelayHop, Route,
    };

    fn authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn context(seed: u8) -> ContextId {
        ContextId::new_from_entropy([seed; 32])
    }

    fn endpoint(port: u16) -> LinkEndpoint {
        LinkEndpoint::direct(LinkProtocol::Tcp, format!("127.0.0.1:{port}"))
    }

    fn route() -> Route {
        Route {
            hops: vec![
                RelayHop {
                    authority_id: authority(2),
                    link_endpoint: endpoint(7000),
                },
                RelayHop {
                    authority_id: authority(3),
                    link_endpoint: endpoint(7001),
                },
            ],
            destination: endpoint(9000),
        }
    }

    #[tokio::test]
    async fn transparent_anonymous_path_establishment_creates_reusable_path() {
        let registry = Arc::new(ServiceRegistry::new());
        let manager =
            AnonymousPathManager::new(AnonymousPathManagerConfig::for_testing(), registry);

        let (path, setup) = manager
            .establish_transparent_path(context(1), authority(9), route(), [7; 32], 100)
            .await
            .expect("establish transparent path");

        assert_eq!(path.profile, ServiceProfile::AnonymousPathEstablish);
        assert_eq!(setup.hop_count(), 2);
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
        assert_eq!(path.path_protection, PathProtectionMode::TransparentDebug);
        assert_eq!(setup.link_protection, LinkProtectionMode::TransportLink);
        assert_eq!(setup.path_protection, PathProtectionMode::TransparentDebug);
        let reused = manager
            .reuse_established_path(&path_ref, 101)
            .await
            .expect("reuse established path");
        assert_eq!(reused, path);
    }

    #[tokio::test]
    async fn transparent_anonymous_path_rejects_replay() {
        let registry = Arc::new(ServiceRegistry::new());
        let manager =
            AnonymousPathManager::new(AnonymousPathManagerConfig::for_testing(), registry);

        manager
            .establish_transparent_path(context(1), authority(9), route(), [5; 32], 100)
            .await
            .expect("first establish");
        let error = manager
            .establish_transparent_path(context(1), authority(9), route(), [5; 32], 101)
            .await
            .expect_err("duplicate setup should reject");
        assert!(matches!(
            error,
            AnonymousPathManagerError::ReplayRejected { .. }
        ));
    }

    #[tokio::test]
    async fn transparent_anonymous_path_rejects_expired_reuse() {
        let registry = Arc::new(ServiceRegistry::new());
        let manager =
            AnonymousPathManager::new(AnonymousPathManagerConfig::for_testing(), registry);

        let (path, _) = manager
            .establish_transparent_path(context(1), authority(9), route(), [4; 32], 100)
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
    async fn transparent_anonymous_path_requires_non_direct_route() {
        let registry = Arc::new(ServiceRegistry::new());
        let manager =
            AnonymousPathManager::new(AnonymousPathManagerConfig::for_testing(), registry);

        let error = manager
            .establish_transparent_path(
                context(1),
                authority(9),
                Route::direct(endpoint(9000)),
                [1; 32],
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
        let control = manager
            .open_establish_session(context(1), authority(9), &route(), "path-owner", 100)
            .await
            .expect("open establish session");
        let (_path, _setup, completed) = manager
            .establish_transparent_path_with_control(
                &control,
                context(1),
                authority(9),
                route(),
                [8; 32],
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
        let mut control = manager
            .open_establish_session(context(1), authority(9), &route(), "owner-a", 100)
            .await
            .expect("open establish session");
        control.ownership_capability.generation = 99;
        let error = manager
            .establish_transparent_path_with_control(
                &control,
                context(1),
                authority(9),
                route(),
                [9; 32],
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
        let control = manager
            .open_establish_session(context(1), authority(9), &route(), "owner-a", 100)
            .await
            .expect("open establish session");
        let error = manager
            .establish_transparent_path_with_control(
                &control,
                context(1),
                authority(9),
                route(),
                [6; 32],
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
        let control = manager
            .open_establish_session(context(1), authority(9), &route(), "owner-a", 100)
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

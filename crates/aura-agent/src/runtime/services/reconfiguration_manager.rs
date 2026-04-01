//! Runtime reconfiguration manager for link/delegate operations.

use crate::core::default_context_id_for_authority;
use crate::reconfiguration::{CoherenceStatus, ReconfigurationController, SessionFootprintClass};
use crate::runtime::{
    AuraDelegationCoherence, AuraDelegationWitness, AuraEffectSystem, AuraLinkBoundary,
    OwnedVmSession, RuntimeBoundaryError, RuntimeReconfigurationEvent, RuntimeSessionOwner,
    SessionIngressError, SessionOwnerCapabilityScope,
};
use aura_core::effects::PhysicalTimeEffects;
use aura_core::time::{ProvenancedTime, TimeStamp};
use aura_core::{
    AuthorityId, ComposedBundle, ContextId, DelegationReceipt, OwnershipCategory, SessionFootprint,
    SessionId,
};
use aura_effects::RuntimeCapabilityHandler;
use aura_journal::fact::{ProtocolRelationalFact, RelationalFact, SessionDelegationFact};
use aura_mpst::CompositionManifest;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

#[cfg(feature = "choreo-backend-telltale-machine")]
use telltale_machine::{
    OwnershipReceipt, OwnershipScope, ReconfigurationRuntimeSnapshot, RuntimeUpgradeExecution,
    RuntimeUpgradeRequest,
};

#[allow(dead_code)] // Declaration-layer ingress inventory; runtime actor wiring lands incrementally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReconfigurationManagerCommand {
    RegisterBundle,
    LinkBundles,
    DelegateSession,
    TransferActiveSessionOwnership,
}

/// Runtime-owned reconfiguration state and lifecycle methods.
#[derive(Clone)]
#[aura_macros::actor_owned(
    owner = "reconfiguration_manager",
    domain = "runtime_reconfiguration",
    gate = "reconfiguration_command_ingress",
    command = ReconfigurationManagerCommand,
    capacity = 32,
    category = "actor_owned"
)]
pub struct ReconfigurationManager {
    shared: Arc<ReconfigurationShared>,
    runtime_capabilities: RuntimeCapabilityHandler,
}

struct ReconfigurationShared {
    controller: RwLock<ReconfigurationController>,
}

/// Typed runtime delegation request for one session ownership transfer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionDelegationTransfer {
    pub context_id: Option<ContextId>,
    pub session_id: SessionId,
    pub from_authority: AuthorityId,
    pub to_authority: AuthorityId,
    pub bundle_id: String,
    pub link_boundary: Option<AuraLinkBoundary>,
    pub capability_scope: SessionOwnerCapabilityScope,
    #[cfg(feature = "choreo-backend-telltale-machine")]
    pub runtime_upgrade_request: Option<RuntimeUpgradeRequest>,
}

/// Typed result for one successful runtime delegation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionDelegationOutcome {
    pub receipt: DelegationReceipt,
    pub witness: AuraDelegationWitness,
}

/// Typed runtime errors for one live session ownership handoff.
#[derive(Debug, Error)]
pub enum ActiveSessionDelegationError {
    #[error(
        "active session handoff target {active_session_id} does not match delegation session {transfer_session_id}"
    )]
    SessionMismatch {
        active_session_id: SessionId,
        transfer_session_id: SessionId,
    },
    #[error("failed to transfer live session owner for session {session_id}: {source}")]
    OwnerTransfer {
        session_id: SessionId,
        #[source]
        source: SessionIngressError,
    },
    #[error("live delegation failed for session {session_id}: {source}")]
    Reconfiguration {
        session_id: SessionId,
        #[source]
        source: ReconfigurationManagerError,
    },
    #[error(
        "live delegation rollback failed for session {session_id} after reconfiguration error: {source}; rollback: {rollback}"
    )]
    RollbackFailed {
        session_id: SessionId,
        #[source]
        source: ReconfigurationManagerError,
        rollback: SessionIngressError,
    },
}

/// Typed runtime errors for reconfiguration and delegation.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ReconfigurationManagerError {
    #[error("{operation} requires protocol-critical runtime surfaces {surfaces:?}: {message}")]
    MissingCapability {
        operation: &'static str,
        surfaces: &'static [&'static str],
        message: String,
    },
    #[error("register bundle `{bundle_id}` failed: {message}")]
    RegisterBundle { bundle_id: String, message: String },
    #[error("link bundles `{left}` + `{right}` into `{linked}` failed: {message}")]
    LinkBundles {
        left: String,
        right: String,
        linked: String,
        message: String,
    },
    #[error("delegation timestamp unavailable for session {session_id}: {message}")]
    DelegationTimestamp {
        session_id: SessionId,
        message: String,
    },
    #[error("delegation requires pre-registered bundle `{bundle_id}`")]
    BundleNotRegistered { bundle_id: String },
    #[error(
        "delegation for session {session_id} rejected link boundary for bundle `{bundle_id}`: {source}"
    )]
    InvalidLinkBoundary {
        session_id: SessionId,
        bundle_id: String,
        #[source]
        source: RuntimeBoundaryError,
    },
    #[error("session delegation failed for session {session_id}: {message}")]
    DelegateSession {
        session_id: SessionId,
        message: String,
    },
    #[cfg(feature = "choreo-backend-telltale-machine")]
    #[error("runtime upgrade for bundle `{bundle_id}` failed: {message}")]
    RuntimeUpgrade { bundle_id: String, message: String },
    #[error("failed to persist delegation fact for session {session_id}: {message}")]
    PersistDelegationFact {
        session_id: SessionId,
        message: String,
    },
    #[error(
        "reconfiguration coherence violation after delegation for session {session_id}: {details}"
    )]
    CoherenceViolation {
        session_id: SessionId,
        details: String,
    },
}

impl SessionDelegationTransfer {
    pub const OWNERSHIP_CATEGORY: OwnershipCategory = OwnershipCategory::MoveOwned;

    #[must_use]
    pub fn new(
        session_id: SessionId,
        from_authority: AuthorityId,
        to_authority: AuthorityId,
        bundle_id: impl Into<String>,
    ) -> Self {
        let bundle_id = bundle_id.into();
        let link_boundary = AuraLinkBoundary::for_bundle_id(bundle_id.clone());
        Self {
            context_id: None,
            session_id,
            from_authority,
            to_authority,
            bundle_id,
            capability_scope: link_boundary.capability_scope.clone(),
            link_boundary: Some(link_boundary),
            #[cfg(feature = "choreo-backend-telltale-machine")]
            runtime_upgrade_request: None,
        }
    }

    #[must_use]
    pub fn with_context(mut self, context_id: ContextId) -> Self {
        self.context_id = Some(context_id);
        self
    }

    #[must_use]
    pub fn with_link_boundary(mut self, link_boundary: AuraLinkBoundary) -> Self {
        self.capability_scope = link_boundary.capability_scope.clone();
        self.link_boundary = Some(link_boundary);
        self
    }

    #[must_use]
    pub fn with_capability_scope(mut self, capability_scope: SessionOwnerCapabilityScope) -> Self {
        self.capability_scope = capability_scope;
        self
    }

    #[cfg(feature = "choreo-backend-telltale-machine")]
    #[must_use]
    pub fn with_runtime_upgrade_request(
        mut self,
        runtime_upgrade_request: RuntimeUpgradeRequest,
    ) -> Self {
        self.runtime_upgrade_request = Some(runtime_upgrade_request);
        self
    }
}

impl Default for ReconfigurationManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ReconfigurationManager {
    pub const OWNERSHIP_CATEGORY: OwnershipCategory = OwnershipCategory::MoveOwned;
    const REQUIRED_RUNTIME_SURFACES: &'static [&'static str] = &[
        "ownership_receipt",
        "semantic_handoff",
        "reconfiguration_transition",
    ];

    /// Create a new manager.
    #[must_use]
    pub fn new() -> Self {
        Self::with_runtime_capabilities(default_runtime_capability_handler())
    }

    /// Create a new manager with explicit runtime capability admission state.
    #[must_use]
    pub fn with_runtime_capabilities(runtime_capabilities: RuntimeCapabilityHandler) -> Self {
        let mut controller = ReconfigurationController::new();
        for bundle in generated_linkable_bundles() {
            controller
                .register_bundle(bundle)
                .expect("generated reconfiguration bundles must be unique and valid");
        }
        Self {
            shared: Arc::new(ReconfigurationShared {
                controller: RwLock::new(controller),
            }),
            runtime_capabilities,
        }
    }

    async fn require_reconfiguration_capability(
        &self,
        operation: &'static str,
    ) -> Result<(), ReconfigurationManagerError> {
        self.runtime_capabilities
            .require_protocol_critical_surfaces(Self::REQUIRED_RUNTIME_SURFACES)
            .map_err(|error| ReconfigurationManagerError::MissingCapability {
                operation,
                surfaces: Self::REQUIRED_RUNTIME_SURFACES,
                message: error.to_string(),
            })
    }

    /// Register one composable bundle in the runtime lifecycle.
    pub async fn register_bundle(
        &self,
        bundle: ComposedBundle,
    ) -> Result<(), ReconfigurationManagerError> {
        let bundle_id = bundle.bundle_id.clone();
        let mut controller = self.shared.controller.write().await;
        controller.register_bundle(bundle).map_err(|e| {
            ReconfigurationManagerError::RegisterBundle {
                bundle_id: bundle_id.clone(),
                message: e.to_string(),
            }
        })?;
        tracing::info!(bundle_id = %bundle_id, "registered reconfiguration bundle");
        Ok(())
    }

    /// Snapshot one registered bundle by id.
    pub async fn bundle(&self, bundle_id: &str) -> Option<ComposedBundle> {
        let controller = self.shared.controller.read().await;
        controller.bundle(bundle_id).cloned()
    }

    /// Snapshot one authority footprint by id.
    pub async fn footprint(&self, authority: AuthorityId) -> Option<SessionFootprint> {
        let controller = self.shared.controller.read().await;
        controller.footprint(&authority).cloned()
    }

    /// Link two bundles into one composed runtime bundle.
    pub async fn link_bundles(
        &self,
        bundle_a: &str,
        bundle_b: &str,
        linked_bundle_id: impl Into<String>,
    ) -> Result<ComposedBundle, ReconfigurationManagerError> {
        self.require_reconfiguration_capability("bundle linking")
            .await?;
        let linked_bundle_id = linked_bundle_id.into();
        let mut controller = self.shared.controller.write().await;
        let linked = controller
            .link(bundle_a, bundle_b, linked_bundle_id.clone())
            .map_err(|e| ReconfigurationManagerError::LinkBundles {
                left: bundle_a.to_string(),
                right: bundle_b.to_string(),
                linked: linked_bundle_id.clone(),
                message: e.to_string(),
            })?;
        tracing::info!(
            left = %bundle_a,
            right = %bundle_b,
            linked = %linked_bundle_id,
            sessions = linked.session_footprint.all_sessions().len(),
            boundaries = ?AuraLinkBoundary::for_bundle(&linked),
            "linked reconfiguration bundle"
        );
        Ok(linked)
    }

    /// Record that `authority` currently owns `session_id` natively.
    pub async fn record_native_session(&self, authority: AuthorityId, session_id: SessionId) {
        let mut controller = self.shared.controller.write().await;
        controller.footprint_extend(authority, session_id, SessionFootprintClass::Native);
    }

    /// Delegate one live session by moving the active owner first and rolling it back on failure.
    pub async fn delegate_active_session(
        &self,
        effects: &AuraEffectSystem,
        session: &mut OwnedVmSession,
        transfer: SessionDelegationTransfer,
        next_owner_label: impl Into<String>,
    ) -> Result<SessionDelegationOutcome, ActiveSessionDelegationError> {
        let active_session_id = session.owner().session_id.into_aura_session_id();
        if active_session_id != transfer.session_id {
            return Err(ActiveSessionDelegationError::SessionMismatch {
                active_session_id,
                transfer_session_id: transfer.session_id,
            });
        }

        let next_owner_label = next_owner_label.into();
        let next_boundary = self
            .resolve_delegation_boundary(
                transfer.session_id,
                &transfer.bundle_id,
                transfer.link_boundary.clone(),
                &transfer.capability_scope,
            )
            .await
            .map_err(|source| ActiveSessionDelegationError::Reconfiguration {
                session_id: transfer.session_id,
                source,
            })?;

        let previous_owner_label = session.owner().owner_label.clone();
        let previous_boundary = session.routing_boundary().clone();
        let previous_owner = session.owner().clone();

        session
            .transfer_owner_in_place(next_owner_label, next_boundary.clone())
            .map_err(|source| ActiveSessionDelegationError::OwnerTransfer {
                session_id: transfer.session_id,
                source,
            })?;
        #[cfg(feature = "choreo-backend-telltale-machine")]
        let ownership_receipt = build_active_session_ownership_receipt(
            session.vm_session_id(),
            &previous_owner,
            session.owner(),
        );

        let delegation = self
            .delegate_session(effects, transfer.with_link_boundary(next_boundary))
            .await;
        match delegation {
            Ok(mut outcome) => {
                #[cfg(feature = "choreo-backend-telltale-machine")]
                {
                    outcome.witness = outcome.witness.with_ownership_receipt(ownership_receipt);
                }
                Ok(outcome)
            }
            Err(source) => {
                let rollback =
                    session.transfer_owner_in_place(previous_owner_label, previous_boundary);
                match rollback {
                    Ok(()) => Err(ActiveSessionDelegationError::Reconfiguration {
                        session_id: active_session_id,
                        source,
                    }),
                    Err(rollback) => Err(ActiveSessionDelegationError::RollbackFailed {
                        session_id: active_session_id,
                        source,
                        rollback,
                    }),
                }
            }
        }
    }

    /// Delegate one session with audit fact persistence and coherence checks.
    pub async fn delegate_session(
        &self,
        effects: &AuraEffectSystem,
        transfer: SessionDelegationTransfer,
    ) -> Result<SessionDelegationOutcome, ReconfigurationManagerError> {
        self.require_reconfiguration_capability("session delegation")
            .await?;
        let SessionDelegationTransfer {
            context_id,
            session_id,
            from_authority,
            to_authority,
            bundle_id,
            link_boundary,
            capability_scope,
            #[cfg(feature = "choreo-backend-telltale-machine")]
            runtime_upgrade_request,
        } = transfer;
        let timestamp = effects.physical_time().await.map_err(|e| {
            ReconfigurationManagerError::DelegationTimestamp {
                session_id,
                message: e.to_string(),
            }
        })?;
        let delegated_at = ProvenancedTime {
            stamp: TimeStamp::PhysicalClock(timestamp.clone()),
            proofs: vec![],
            origin: None,
        };

        let resolved_context_id =
            context_id.unwrap_or_else(|| default_context_id_for_authority(from_authority));

        let boundary = self
            .resolve_delegation_boundary(session_id, &bundle_id, link_boundary, &capability_scope)
            .await?;

        let (receipt, runtime_upgrade_execution, runtime_upgrade_snapshot, controller_snapshot) = {
            let mut controller = self.shared.controller.write().await;
            let controller_snapshot = controller.clone();
            let receipt = controller
                .delegate(
                    session_id,
                    from_authority,
                    to_authority,
                    Some(bundle_id.clone()),
                    delegated_at,
                )
                .map_err(|e| ReconfigurationManagerError::DelegateSession {
                    session_id,
                    message: e.to_string(),
                })?;
            #[cfg(feature = "choreo-backend-telltale-machine")]
            let runtime_upgrade_execution = if let Some(request) = runtime_upgrade_request.as_ref()
            {
                match controller.execute_runtime_upgrade(&bundle_id, request) {
                    Ok(execution) => Some(execution),
                    Err(error) => {
                        *controller = controller_snapshot.clone();
                        return Err(ReconfigurationManagerError::RuntimeUpgrade {
                            bundle_id: bundle_id.clone(),
                            message: error.to_string(),
                        });
                    }
                }
            } else {
                None
            };
            #[cfg(not(feature = "choreo-backend-telltale-machine"))]
            let runtime_upgrade_execution = None;
            #[cfg(feature = "choreo-backend-telltale-machine")]
            let runtime_upgrade_snapshot = if runtime_upgrade_request.is_some() {
                match controller.runtime_upgrade_snapshot(&bundle_id) {
                    Ok(snapshot) => Some(snapshot),
                    Err(error) => {
                        *controller = controller_snapshot.clone();
                        return Err(ReconfigurationManagerError::RuntimeUpgrade {
                            bundle_id: bundle_id.clone(),
                            message: error.to_string(),
                        });
                    }
                }
            } else {
                None
            };
            #[cfg(not(feature = "choreo-backend-telltale-machine"))]
            let runtime_upgrade_snapshot = None;
            (
                receipt,
                runtime_upgrade_execution,
                runtime_upgrade_snapshot,
                controller_snapshot,
            )
        };

        let fact = RelationalFact::Protocol(ProtocolRelationalFact::SessionDelegation(
            SessionDelegationFact {
                context_id: resolved_context_id,
                session_id,
                from_authority,
                to_authority,
                bundle_id: Some(bundle_id.clone()),
                timestamp,
            },
        ));

        if let Err(error) = effects.commit_relational_facts(vec![fact]).await {
            let mut controller = self.shared.controller.write().await;
            *controller = controller_snapshot;
            return Err(ReconfigurationManagerError::PersistDelegationFact {
                session_id,
                message: error.to_string(),
            });
        }

        let witness = AuraDelegationWitness::new(
            resolved_context_id,
            session_id,
            from_authority,
            to_authority,
            bundle_id.clone(),
            boundary,
            capability_scope,
        )
        .with_coherence(AuraDelegationCoherence::Preserved);
        #[cfg(feature = "choreo-backend-telltale-machine")]
        let witness = {
            let witness = if let Some(request) = runtime_upgrade_request {
                witness.with_runtime_upgrade_request(request)
            } else {
                witness
            };
            let witness = if let Some(snapshot) = runtime_upgrade_snapshot {
                witness.with_runtime_upgrade_snapshot(snapshot)
            } else {
                witness
            };
            if let Some(execution) = runtime_upgrade_execution {
                witness.with_runtime_upgrade_execution(execution)
            } else {
                witness
            }
        };

        tracing::info!(
            event = RuntimeReconfigurationEvent::DelegationPersisted.as_event_name(),
            session_id = %session_id,
            from_authority = %from_authority,
            to_authority = %to_authority,
            bundle_id = %bundle_id,
            witness = ?witness,
            "delegated session and persisted audit fact"
        );

        Ok(SessionDelegationOutcome { receipt, witness })
    }

    /// Verify global coherence and emit diagnostics on violations.
    pub async fn verify_coherence(&self) -> CoherenceStatus {
        let controller = self.shared.controller.read().await;
        let status = controller.verify_coherence();
        if let CoherenceStatus::Violations(violations) = &status {
            tracing::error!(
                violations = ?violations,
                "reconfiguration coherence check failed"
            );
        }
        status
    }

    #[cfg(feature = "choreo-backend-telltale-machine")]
    pub async fn seed_runtime_upgrade_membership<I, S>(
        &self,
        bundle_id: &str,
        members: I,
    ) -> Result<(), ReconfigurationManagerError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut controller = self.shared.controller.write().await;
        controller
            .seed_runtime_upgrade_membership(bundle_id, members)
            .map_err(|error| ReconfigurationManagerError::RuntimeUpgrade {
                bundle_id: bundle_id.to_string(),
                message: error.to_string(),
            })
    }

    #[cfg(feature = "choreo-backend-telltale-machine")]
    pub async fn runtime_upgrade_snapshot(
        &self,
        bundle_id: &str,
    ) -> Result<ReconfigurationRuntimeSnapshot, ReconfigurationManagerError> {
        let controller = self.shared.controller.read().await;
        controller
            .runtime_upgrade_snapshot(bundle_id)
            .map_err(|error| ReconfigurationManagerError::RuntimeUpgrade {
                bundle_id: bundle_id.to_string(),
                message: error.to_string(),
            })
    }

    #[cfg(feature = "choreo-backend-telltale-machine")]
    pub async fn execute_runtime_upgrade(
        &self,
        bundle_id: &str,
        request: &RuntimeUpgradeRequest,
    ) -> Result<RuntimeUpgradeExecution, ReconfigurationManagerError> {
        self.require_reconfiguration_capability("runtime upgrade")
            .await?;
        let mut controller = self.shared.controller.write().await;
        controller
            .execute_runtime_upgrade(bundle_id, request)
            .map_err(|error| ReconfigurationManagerError::RuntimeUpgrade {
                bundle_id: bundle_id.to_string(),
                message: error.to_string(),
            })
    }
}

#[allow(clippy::result_large_err)]
fn validate_link_boundary(
    session_id: SessionId,
    bundle_id: &str,
    boundary: &AuraLinkBoundary,
    capability_scope: &SessionOwnerCapabilityScope,
) -> Result<(), ReconfigurationManagerError> {
    if boundary.bundle_id.as_deref() != Some(bundle_id) {
        return Err(ReconfigurationManagerError::InvalidLinkBoundary {
            session_id,
            bundle_id: bundle_id.to_string(),
            source: RuntimeBoundaryError::LinkBoundaryBundleMismatch {
                session_id,
                bundle_id: bundle_id.to_string(),
                boundary_bundle_id: boundary.bundle_id.clone(),
            },
        });
    }

    if &boundary.capability_scope != capability_scope {
        return Err(ReconfigurationManagerError::InvalidLinkBoundary {
            session_id,
            bundle_id: bundle_id.to_string(),
            source: RuntimeBoundaryError::LinkBoundaryScopeMismatch {
                session_id,
                bundle_id: bundle_id.to_string(),
                boundary_scope: boundary.capability_scope.clone(),
                capability_scope: capability_scope.clone(),
            },
        });
    }

    Ok(())
}

fn composed_bundle_from_manifest(manifest: &CompositionManifest) -> Vec<ComposedBundle> {
    let mut bundles = BTreeMap::<String, ComposedBundle>::new();
    for spec in &manifest.link_specs {
        let bundle = bundles.entry(spec.bundle_id.clone()).or_insert_with(|| {
            ComposedBundle::new(
                spec.bundle_id.clone(),
                vec![manifest.protocol_id.clone()],
                BTreeSet::new(),
                BTreeSet::new(),
                SessionFootprint::new(),
            )
        });
        if !bundle
            .protocol_ids
            .iter()
            .any(|id| id == &manifest.protocol_id)
        {
            bundle.protocol_ids.push(manifest.protocol_id.clone());
        }
        bundle.exports.extend(spec.exports.iter().cloned());
        bundle.imports.extend(spec.imports.iter().cloned());
    }
    bundles.into_values().collect()
}

fn generated_linkable_bundles() -> Vec<ComposedBundle> {
    let manifests = [
        aura_invitation::protocol::device_enrollment::telltale_session_types_invitation_device_enrollment::vm_artifacts::composition_manifest(),
        aura_recovery::guardian_membership::telltale_session_types_guardian_membership_change::vm_artifacts::composition_manifest(),
        aura_sync::protocols::epochs::telltale_session_types_epoch_rotation::vm_artifacts::composition_manifest(),
    ];
    let mut grouped = BTreeMap::<String, ComposedBundle>::new();

    for manifest in manifests {
        for bundle in composed_bundle_from_manifest(&manifest) {
            let entry = grouped.entry(bundle.bundle_id.clone()).or_insert_with(|| {
                ComposedBundle::new(
                    bundle.bundle_id.clone(),
                    Vec::new(),
                    BTreeSet::new(),
                    BTreeSet::new(),
                    SessionFootprint::new(),
                )
            });
            entry.protocol_ids.extend(bundle.protocol_ids);
            entry.exports.extend(bundle.exports);
            entry.imports.extend(bundle.imports);
        }
    }

    grouped.into_values().collect()
}

impl ReconfigurationManager {
    async fn resolve_delegation_boundary(
        &self,
        session_id: SessionId,
        bundle_id: &str,
        requested_boundary: Option<AuraLinkBoundary>,
        capability_scope: &SessionOwnerCapabilityScope,
    ) -> Result<AuraLinkBoundary, ReconfigurationManagerError> {
        let controller = self.shared.controller.read().await;
        let Some(bundle) = controller.bundle(bundle_id).cloned() else {
            return Err(ReconfigurationManagerError::BundleNotRegistered {
                bundle_id: bundle_id.to_string(),
            });
        };
        let boundary = requested_boundary.unwrap_or_else(|| AuraLinkBoundary::for_bundle(&bundle));
        validate_link_boundary(session_id, bundle_id, &boundary, capability_scope)?;
        Ok(boundary)
    }
}

fn default_runtime_capability_handler() -> RuntimeCapabilityHandler {
    #[cfg(feature = "choreo-backend-telltale-machine")]
    {
        let contracts = telltale_machine::runtime_contracts::RuntimeContracts::full();
        RuntimeCapabilityHandler::from_protocol_machine_runtime_contracts(&contracts)
    }

    #[cfg(not(feature = "choreo-backend-telltale-machine"))]
    {
        RuntimeCapabilityHandler::from_pairs([("reconfiguration", true)])
    }
}

#[cfg(feature = "choreo-backend-telltale-machine")]
fn build_active_session_ownership_receipt(
    vm_session_id: telltale_machine::SessionId,
    previous_owner: &RuntimeSessionOwner,
    next_owner: &RuntimeSessionOwner,
) -> OwnershipReceipt {
    OwnershipReceipt {
        session_id: vm_session_id,
        claim_id: next_owner.capability.generation,
        from_owner_id: previous_owner.owner_label.clone(),
        from_generation: previous_owner.capability.generation,
        to_owner_id: next_owner.owner_label.clone(),
        to_generation: next_owner.capability.generation,
        scope: telltale_scope_for_capability_scope(&next_owner.capability.scope),
    }
}

#[cfg(feature = "choreo-backend-telltale-machine")]
fn telltale_scope_for_capability_scope(scope: &SessionOwnerCapabilityScope) -> OwnershipScope {
    match scope {
        SessionOwnerCapabilityScope::Session => OwnershipScope::Session,
        SessionOwnerCapabilityScope::Fragments(fragments) => {
            OwnershipScope::Fragments(fragments.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;
    use crate::runtime::{
        open_owned_manifest_vm_session_admitted, AuraEffectSystem, AuraLinkBoundary,
        AuraVmSchedulerSignals,
    };
    use aura_protocol::effects::{ChoreographicRole, RoleIndex};
    use std::collections::BTreeSet as StdBTreeSet;
    use std::sync::Arc;
    use uuid::Uuid;

    fn authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn session(seed: u8) -> SessionId {
        SessionId::from_uuid(Uuid::from_bytes([seed; 16]))
    }

    fn epoch_rotation_vm_artifacts(
        participant_authority: AuthorityId,
        coordinator_authority: AuthorityId,
    ) -> (
        Vec<ChoreographicRole>,
        CompositionManifest,
        aura_mpst::upstream::types::GlobalType,
        BTreeMap<String, aura_mpst::upstream::types::LocalTypeR>,
    ) {
        let roles = vec![
            ChoreographicRole::for_authority(
                coordinator_authority,
                RoleIndex::new(0).expect("coordinator"),
            ),
            ChoreographicRole::for_authority(
                participant_authority,
                RoleIndex::new(0).expect("participant"),
            ),
        ];
        (
            roles,
            aura_sync::protocols::epochs::telltale_session_types_epoch_rotation::vm_artifacts::composition_manifest(),
            aura_sync::protocols::epochs::telltale_session_types_epoch_rotation::vm_artifacts::global_type(),
            aura_sync::protocols::epochs::telltale_session_types_epoch_rotation::vm_artifacts::local_types(),
        )
    }

    #[test]
    fn session_delegation_transfer_keeps_context_separate_from_bundle_evidence() {
        let session_id = SessionId::new_from_entropy([1; 32]);
        let from_authority = AuthorityId::new_from_entropy([2; 32]);
        let to_authority = AuthorityId::new_from_entropy([3; 32]);
        let context_id = ContextId::new_from_entropy([4; 32]);

        let transfer =
            SessionDelegationTransfer::new(session_id, from_authority, to_authority, "bundle-a")
                .with_context(context_id);

        assert_eq!(transfer.session_id, session_id);
        assert_eq!(transfer.from_authority, from_authority);
        assert_eq!(transfer.to_authority, to_authority);
        assert_eq!(transfer.context_id, Some(context_id));
        assert_eq!(transfer.bundle_id, "bundle-a");
        assert_eq!(
            transfer
                .link_boundary
                .as_ref()
                .and_then(|boundary| boundary.bundle_id.as_deref()),
            Some("bundle-a")
        );
        assert!(matches!(
            transfer.capability_scope,
            SessionOwnerCapabilityScope::Fragments(_)
        ));
    }

    #[tokio::test]
    async fn active_session_handoff_moves_owner_fragment_and_footprint_together() {
        let from_authority = authority(1);
        let to_authority = authority(2);
        let session_id = session(77);
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(
                &AgentConfig::default(),
                from_authority,
            )
            .expect("simulation effect system"),
        );
        let manager = ReconfigurationManager::new();
        manager
            .record_native_session(from_authority, session_id)
            .await;

        let (roles, manifest, global_type, local_types) =
            epoch_rotation_vm_artifacts(from_authority, authority(9));
        let bundle_id = manifest
            .link_specs
            .first()
            .expect("epoch manifest should expose one link boundary")
            .bundle_id
            .clone();
        let mut active_session = open_owned_manifest_vm_session_admitted(
            effects.clone(),
            session_id.uuid(),
            roles,
            &manifest,
            "Participant",
            &global_type,
            &local_types,
            AuraVmSchedulerSignals::default(),
        )
        .await
        .expect("open active session");
        let stale_owner = active_session.owner().clone();

        let outcome = manager
            .delegate_active_session(
                effects.as_ref(),
                &mut active_session,
                SessionDelegationTransfer::new(
                    session_id,
                    from_authority,
                    to_authority,
                    bundle_id.clone(),
                ),
                "delegated-owner",
            )
            .await
            .expect("delegate active session");

        assert_eq!(active_session.owner().owner_label, "delegated-owner");
        assert_eq!(
            active_session.owner().capability.scope,
            SessionOwnerCapabilityScope::Fragments(StdBTreeSet::from([format!(
                "bundle:{bundle_id}"
            ),]))
        );
        assert_eq!(
            outcome.witness.link_boundary.bundle_id.as_deref(),
            Some(bundle_id.as_str())
        );
        #[cfg(feature = "choreo-backend-telltale-machine")]
        {
            let ownership_receipt = outcome
                .witness
                .ownership_receipt
                .as_ref()
                .expect("active handoff should publish an ownership receipt");
            assert_eq!(ownership_receipt.session_id, active_session.vm_session_id());
            assert_eq!(ownership_receipt.from_owner_id, stale_owner.owner_label);
            assert_eq!(ownership_receipt.to_owner_id, "delegated-owner");
            assert_eq!(
                ownership_receipt.scope,
                telltale_machine::OwnershipScope::Fragments(StdBTreeSet::from([format!(
                    "bundle:{bundle_id}"
                ),]))
            );
            assert_eq!(
                ownership_receipt.to_generation,
                stale_owner.capability.generation.saturating_add(1)
            );
        }
        assert_eq!(manager.verify_coherence().await, CoherenceStatus::Coherent);
        assert!(manager
            .footprint(from_authority)
            .await
            .expect("from footprint")
            .delegated_out_sessions
            .contains(&session_id));
        assert!(manager
            .footprint(to_authority)
            .await
            .expect("to footprint")
            .delegated_in_sessions
            .contains(&session_id));

        let fragment_snapshot = effects.vm_fragment_snapshot();
        assert!(
            fragment_snapshot
                .iter()
                .all(|(_, owner)| owner.owner_label == "delegated-owner"),
            "fragment ownership must move with the live session handoff"
        );
        assert!(
            effects
                .assert_owned_choreography_session(active_session.owner())
                .is_err(),
            "fragment-scoped delegated owner should no longer authorize full-session ingress"
        );
        effects
            .assert_owned_choreography_boundary(
                active_session.owner(),
                active_session.routing_boundary(),
            )
            .expect("delegated owner should retain its delegated boundary");
        assert!(
            effects
                .assert_owned_choreography_boundary(
                    &stale_owner,
                    active_session.routing_boundary(),
                )
                .is_err(),
            "stale owner must be rejected after live handoff"
        );

        active_session
            .transfer_owner_in_place(
                "owner-a",
                AuraLinkBoundary::for_scope(SessionOwnerCapabilityScope::Session),
            )
            .expect("restore full-session owner for cleanup");
        active_session
            .close()
            .await
            .expect("close restored session");
    }

    #[tokio::test]
    async fn active_session_handoff_rolls_back_owner_and_fragments_on_failure() {
        let from_authority = authority(1);
        let to_authority = authority(3);
        let session_id = session(78);
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(
                &AgentConfig::default(),
                from_authority,
            )
            .expect("simulation effect system"),
        );
        let manager = ReconfigurationManager::with_runtime_capabilities(
            RuntimeCapabilityHandler::from_pairs([("reconfiguration", false)]),
        );
        manager
            .record_native_session(from_authority, session_id)
            .await;

        let (roles, manifest, global_type, local_types) =
            epoch_rotation_vm_artifacts(from_authority, authority(9));
        let bundle_id = manifest
            .link_specs
            .first()
            .expect("epoch manifest should expose one link boundary")
            .bundle_id
            .clone();
        let mut active_session = open_owned_manifest_vm_session_admitted(
            effects.clone(),
            session_id.uuid(),
            roles,
            &manifest,
            "Participant",
            &global_type,
            &local_types,
            AuraVmSchedulerSignals::default(),
        )
        .await
        .expect("open active session");
        let original_owner = active_session.owner().clone();

        let error = manager
            .delegate_active_session(
                effects.as_ref(),
                &mut active_session,
                SessionDelegationTransfer::new(session_id, from_authority, to_authority, bundle_id),
                "delegated-owner",
            )
            .await
            .expect_err("missing reconfiguration capability must fail closed");

        assert!(matches!(
            error,
            ActiveSessionDelegationError::Reconfiguration { .. }
        ));
        assert_eq!(
            active_session.owner().owner_label,
            original_owner.owner_label
        );
        assert_eq!(
            active_session.owner().capability.scope,
            active_session.routing_boundary().capability_scope
        );
        effects
            .assert_owned_choreography_boundary(
                active_session.owner(),
                active_session.routing_boundary(),
            )
            .expect("original owner must be restored after rollback");
        assert!(
            manager.footprint(to_authority).await.is_none(),
            "failed handoff must not create delegated-in ownership"
        );
        let fragment_snapshot = effects.vm_fragment_snapshot();
        assert!(
            fragment_snapshot
                .iter()
                .all(|(_, owner)| owner.owner_label == original_owner.owner_label),
            "fragment ownership must roll back with the owner"
        );

        active_session
            .transfer_owner_in_place(
                original_owner.owner_label,
                AuraLinkBoundary::for_scope(SessionOwnerCapabilityScope::Session),
            )
            .expect("restore full-session owner for cleanup");
        active_session
            .close()
            .await
            .expect("close rolled-back session");
    }
}

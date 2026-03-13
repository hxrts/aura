//! Runtime reconfiguration manager for link/delegate operations.

use crate::core::default_context_id_for_authority;
use crate::reconfiguration::{CoherenceStatus, ReconfigurationController, SessionFootprintClass};
use crate::runtime::AuraEffectSystem;
use aura_core::effects::{CapabilityKey, PhysicalTimeEffects, RuntimeCapabilityEffects};
use aura_core::time::{ProvenancedTime, TimeStamp};
use aura_core::{
    AuthorityId, ComposedBundle, ContextId, DelegationReceipt, SessionFootprint, SessionId,
};
use aura_effects::RuntimeCapabilityHandler;
use aura_journal::fact::{ProtocolRelationalFact, RelationalFact, SessionDelegationFact};
use aura_mpst::CompositionManifest;
use aura_protocol::admission::CAPABILITY_RECONFIGURATION;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Runtime-owned reconfiguration state and lifecycle methods.
#[derive(Clone)]
pub struct ReconfigurationManager {
    controller: Arc<RwLock<ReconfigurationController>>,
    runtime_capabilities: RuntimeCapabilityHandler,
}

/// Typed runtime delegation request for one session ownership transfer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionDelegationTransfer {
    pub context_id: Option<ContextId>,
    pub session_id: SessionId,
    pub from_authority: AuthorityId,
    pub to_authority: AuthorityId,
    pub bundle_id: String,
}

impl SessionDelegationTransfer {
    #[must_use]
    pub fn new(
        session_id: SessionId,
        from_authority: AuthorityId,
        to_authority: AuthorityId,
        bundle_id: impl Into<String>,
    ) -> Self {
        Self {
            context_id: None,
            session_id,
            from_authority,
            to_authority,
            bundle_id: bundle_id.into(),
        }
    }

    #[must_use]
    pub fn with_context(mut self, context_id: ContextId) -> Self {
        self.context_id = Some(context_id);
        self
    }
}

impl Default for ReconfigurationManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ReconfigurationManager {
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
            controller: Arc::new(RwLock::new(controller)),
            runtime_capabilities,
        }
    }

    async fn require_reconfiguration_capability(&self, operation: &str) -> Result<(), String> {
        self.runtime_capabilities
            .require_capabilities(&[CapabilityKey::new(CAPABILITY_RECONFIGURATION)])
            .await
            .map_err(|error| {
                format!(
                    "{operation} requires runtime capability `{CAPABILITY_RECONFIGURATION}`: {error}"
                )
            })
    }

    /// Register one composable bundle in the runtime lifecycle.
    pub async fn register_bundle(&self, bundle: ComposedBundle) -> Result<(), String> {
        let bundle_id = bundle.bundle_id.clone();
        let mut controller = self.controller.write().await;
        controller
            .register_bundle(bundle)
            .map_err(|e| format!("register bundle failed: {e}"))?;
        tracing::info!(bundle_id = %bundle_id, "registered reconfiguration bundle");
        Ok(())
    }

    /// Snapshot one registered bundle by id.
    pub async fn bundle(&self, bundle_id: &str) -> Option<ComposedBundle> {
        let controller = self.controller.read().await;
        controller.bundle(bundle_id).cloned()
    }

    /// Link two bundles into one composed runtime bundle.
    pub async fn link_bundles(
        &self,
        bundle_a: &str,
        bundle_b: &str,
        linked_bundle_id: impl Into<String>,
    ) -> Result<ComposedBundle, String> {
        self.require_reconfiguration_capability("bundle linking")
            .await?;
        let linked_bundle_id = linked_bundle_id.into();
        let mut controller = self.controller.write().await;
        let linked = controller
            .link(bundle_a, bundle_b, linked_bundle_id.clone())
            .map_err(|e| format!("link bundles failed: {e}"))?;
        tracing::info!(
            left = %bundle_a,
            right = %bundle_b,
            linked = %linked_bundle_id,
            sessions = linked.session_footprint.all_sessions().len(),
            "linked reconfiguration bundle"
        );
        Ok(linked)
    }

    /// Record that `authority` currently owns `session_id` natively.
    pub async fn record_native_session(&self, authority: AuthorityId, session_id: SessionId) {
        let mut controller = self.controller.write().await;
        controller.footprint_extend(authority, session_id, SessionFootprintClass::Native);
    }

    /// Delegate one session with audit fact persistence and coherence checks.
    pub async fn delegate_session(
        &self,
        effects: &AuraEffectSystem,
        transfer: SessionDelegationTransfer,
    ) -> Result<DelegationReceipt, String> {
        self.require_reconfiguration_capability("session delegation")
            .await?;
        let timestamp = effects
            .physical_time()
            .await
            .map_err(|e| format!("delegation timestamp unavailable: {e}"))?;
        let delegated_at = ProvenancedTime {
            stamp: TimeStamp::PhysicalClock(timestamp.clone()),
            proofs: vec![],
            origin: None,
        };

        let SessionDelegationTransfer {
            context_id,
            session_id,
            from_authority,
            to_authority,
            bundle_id,
        } = transfer;

        let receipt = {
            let mut controller = self.controller.write().await;
            if controller.bundle(&bundle_id).is_none() {
                return Err(format!(
                    "delegation requires pre-registered bundle `{bundle_id}`"
                ));
            }
            let from_known = controller
                .footprint(&from_authority)
                .map(|footprint| footprint.contains(session_id))
                .unwrap_or(false);
            if !from_known {
                tracing::warn!(
                    session_id = %session_id,
                    from_authority = %from_authority,
                    "delegation source not present in footprint; recording native ownership before delegation"
                );
                controller.footprint_extend(
                    from_authority,
                    session_id,
                    SessionFootprintClass::Native,
                );
            }

            controller
                .delegate(
                    session_id,
                    from_authority,
                    to_authority,
                    Some(bundle_id.clone()),
                    delegated_at,
                )
                .map_err(|e| format!("session delegation failed: {e}"))?
        };

        let fact = RelationalFact::Protocol(ProtocolRelationalFact::SessionDelegation(
            SessionDelegationFact {
                context_id: context_id
                    .unwrap_or_else(|| default_context_id_for_authority(from_authority)),
                session_id,
                from_authority,
                to_authority,
                bundle_id: Some(bundle_id.clone()),
                timestamp,
            },
        ));

        effects
            .commit_relational_facts(vec![fact])
            .await
            .map_err(|e| format!("failed to persist delegation fact: {e}"))?;

        tracing::info!(
            session_id = %session_id,
            from_authority = %from_authority,
            to_authority = %to_authority,
            bundle_id = %bundle_id,
            "delegated session and persisted audit fact"
        );

        let coherence = self.verify_coherence().await;
        if let CoherenceStatus::Violations(violations) = coherence {
            tracing::error!(
                violations = ?violations,
                "reconfiguration coherence violation after delegation"
            );
            return Err(format!(
                "reconfiguration coherence violation after delegation: {}",
                violations.join("; ")
            ));
        }

        Ok(receipt)
    }

    /// Verify global coherence and emit diagnostics on violations.
    pub async fn verify_coherence(&self) -> CoherenceStatus {
        let controller = self.controller.read().await;
        let status = controller.verify_coherence();
        if let CoherenceStatus::Violations(violations) = &status {
            tracing::error!(
                violations = ?violations,
                "reconfiguration coherence check failed"
            );
        }
        status
    }
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

fn default_runtime_capability_handler() -> RuntimeCapabilityHandler {
    #[cfg(feature = "choreo-backend-telltale-vm")]
    {
        let contracts = telltale_vm::runtime_contracts::RuntimeContracts::full();
        RuntimeCapabilityHandler::from_runtime_contracts(&contracts)
    }

    #[cfg(not(feature = "choreo-backend-telltale-vm"))]
    {
        RuntimeCapabilityHandler::from_pairs([(CAPABILITY_RECONFIGURATION, true)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}

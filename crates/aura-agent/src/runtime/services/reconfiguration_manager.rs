//! Runtime reconfiguration manager for link/delegate operations.

use crate::core::default_context_id_for_authority;
use crate::reconfiguration::{CoherenceStatus, ReconfigurationController, SessionFootprintClass};
use crate::runtime::AuraEffectSystem;
use aura_core::effects::PhysicalTimeEffects;
use aura_core::time::{ProvenancedTime, TimeStamp};
use aura_core::{
    AuthorityId, ComposedBundle, ContextId, DelegationReceipt, SessionFootprint, SessionId,
};
use aura_journal::fact::{ProtocolRelationalFact, RelationalFact, SessionDelegationFact};
use std::collections::BTreeSet;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Runtime-owned reconfiguration state and lifecycle methods.
#[derive(Clone, Default)]
pub struct ReconfigurationManager {
    controller: Arc<RwLock<ReconfigurationController>>,
}

impl ReconfigurationManager {
    /// Create a new manager.
    #[must_use]
    pub fn new() -> Self {
        Self {
            controller: Arc::new(RwLock::new(ReconfigurationController::new())),
        }
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

    /// Link two bundles into one composed runtime bundle.
    pub async fn link_bundles(
        &self,
        bundle_a: &str,
        bundle_b: &str,
        linked_bundle_id: impl Into<String>,
    ) -> Result<ComposedBundle, String> {
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
        context_id: Option<ContextId>,
        session_id: SessionId,
        from_authority: AuthorityId,
        to_authority: AuthorityId,
        bundle_id: Option<String>,
    ) -> Result<DelegationReceipt, String> {
        let timestamp = effects
            .physical_time()
            .await
            .map_err(|e| format!("delegation timestamp unavailable: {e}"))?;
        let delegated_at = ProvenancedTime {
            stamp: TimeStamp::PhysicalClock(timestamp.clone()),
            proofs: vec![],
            origin: None,
        };

        let receipt = {
            let mut controller = self.controller.write().await;
            if let Some(bundle_id) = bundle_id.as_ref() {
                if controller.bundle(bundle_id).is_none() {
                    // Runtime-originated migration/handoff flows can delegate before
                    // static link registration. Track an ephemeral bundle so
                    // delegation remains auditable and does not fail closed.
                    let bundle = ComposedBundle::new(
                        bundle_id.clone(),
                        vec![],
                        BTreeSet::new(),
                        BTreeSet::new(),
                        SessionFootprint::new(),
                    );
                    controller
                        .register_bundle(bundle)
                        .map_err(|e| format!("register delegation bundle failed: {e}"))?;
                    tracing::info!(
                        bundle_id = %bundle_id,
                        "registered ephemeral reconfiguration bundle for delegation flow"
                    );
                }
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
                    bundle_id.clone(),
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
                bundle_id: bundle_id.clone(),
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
            bundle_id = bundle_id.as_deref().unwrap_or("none"),
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

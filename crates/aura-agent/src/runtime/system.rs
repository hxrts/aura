//! Runtime System
//!
//! Main runtime system that orchestrates all agent operations.

use super::services::{
    CeremonyTracker, ContextManager, FlowBudgetManager, ReceiptManager, RendezvousManager,
    SocialManager, SyncServiceManager,
};
use super::{
    AuraEffectSystem, ChoreographyAdapter, EffectContext, EffectExecutor, LifecycleManager,
};
use crate::core::AgentConfig;
use aura_core::identifiers::AuthorityId;
use std::sync::Arc;

/// Main runtime system for the agent
pub struct RuntimeSystem {
    /// Effect executor
    #[allow(dead_code)] // Will be used for effect dispatch
    effect_executor: EffectExecutor,

    /// Effect system (immutable after construction, handlers have internal mutability)
    effect_system: Arc<AuraEffectSystem>,

    /// Context manager
    context_manager: ContextManager,

    /// Flow budget manager
    flow_budget_manager: FlowBudgetManager,

    /// Receipt manager
    receipt_manager: ReceiptManager,

    /// Choreography adapter
    choreography_adapter: ChoreographyAdapter,

    /// Lifecycle manager
    lifecycle_manager: LifecycleManager,

    /// Sync service manager (optional, for background journal synchronization)
    sync_manager: Option<SyncServiceManager>,

    /// Rendezvous manager (optional, for peer discovery and channel establishment)
    rendezvous_manager: Option<RendezvousManager>,

    /// Social manager (optional, for social topology and relay selection)
    social_manager: Option<SocialManager>,

    /// Ceremony tracker (for guardian ceremony coordination)
    ceremony_tracker: CeremonyTracker,

    /// Configuration
    #[allow(dead_code)] // Will be used for runtime configuration
    config: AgentConfig,

    /// Authority ID
    authority_id: AuthorityId,
}

impl RuntimeSystem {
    /// Create a new runtime system
    #[allow(clippy::too_many_arguments)]
    #[allow(dead_code)] // Factory retained for future runtime wiring
    pub(crate) fn new(
        effect_executor: EffectExecutor,
        effect_system: Arc<AuraEffectSystem>,
        context_manager: ContextManager,
        flow_budget_manager: FlowBudgetManager,
        receipt_manager: ReceiptManager,
        choreography_adapter: ChoreographyAdapter,
        lifecycle_manager: LifecycleManager,
        config: AgentConfig,
        authority_id: AuthorityId,
    ) -> Self {
        Self {
            effect_executor,
            effect_system,
            context_manager,
            flow_budget_manager,
            receipt_manager,
            choreography_adapter,
            lifecycle_manager,
            sync_manager: None,
            rendezvous_manager: None,
            social_manager: None,
            ceremony_tracker: CeremonyTracker::new(),
            config,
            authority_id,
        }
    }

    /// Create a new runtime system with sync service
    #[allow(clippy::too_many_arguments)]
    #[allow(dead_code)] // Factory retained for future sync-enabled runtime
    pub(crate) fn new_with_sync(
        effect_executor: EffectExecutor,
        effect_system: Arc<AuraEffectSystem>,
        context_manager: ContextManager,
        flow_budget_manager: FlowBudgetManager,
        receipt_manager: ReceiptManager,
        choreography_adapter: ChoreographyAdapter,
        lifecycle_manager: LifecycleManager,
        sync_manager: SyncServiceManager,
        config: AgentConfig,
        authority_id: AuthorityId,
    ) -> Self {
        Self {
            effect_executor,
            effect_system,
            context_manager,
            flow_budget_manager,
            receipt_manager,
            choreography_adapter,
            lifecycle_manager,
            sync_manager: Some(sync_manager),
            rendezvous_manager: None,
            social_manager: None,
            ceremony_tracker: CeremonyTracker::new(),
            config,
            authority_id,
        }
    }

    /// Create a new runtime system with rendezvous service
    #[allow(clippy::too_many_arguments)]
    #[allow(dead_code)] // Factory retained for future rendezvous-enabled runtime
    pub(crate) fn new_with_rendezvous(
        effect_executor: EffectExecutor,
        effect_system: Arc<AuraEffectSystem>,
        context_manager: ContextManager,
        flow_budget_manager: FlowBudgetManager,
        receipt_manager: ReceiptManager,
        choreography_adapter: ChoreographyAdapter,
        lifecycle_manager: LifecycleManager,
        rendezvous_manager: RendezvousManager,
        config: AgentConfig,
        authority_id: AuthorityId,
    ) -> Self {
        Self {
            effect_executor,
            effect_system,
            context_manager,
            flow_budget_manager,
            receipt_manager,
            choreography_adapter,
            lifecycle_manager,
            sync_manager: None,
            rendezvous_manager: Some(rendezvous_manager),
            social_manager: None,
            ceremony_tracker: CeremonyTracker::new(),
            config,
            authority_id,
        }
    }

    /// Create a new runtime system with all services
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_with_services(
        effect_executor: EffectExecutor,
        effect_system: Arc<AuraEffectSystem>,
        context_manager: ContextManager,
        flow_budget_manager: FlowBudgetManager,
        receipt_manager: ReceiptManager,
        choreography_adapter: ChoreographyAdapter,
        lifecycle_manager: LifecycleManager,
        sync_manager: Option<SyncServiceManager>,
        rendezvous_manager: Option<RendezvousManager>,
        social_manager: Option<SocialManager>,
        config: AgentConfig,
        authority_id: AuthorityId,
    ) -> Self {
        Self {
            effect_executor,
            effect_system,
            context_manager,
            flow_budget_manager,
            receipt_manager,
            choreography_adapter,
            lifecycle_manager,
            sync_manager,
            rendezvous_manager,
            social_manager,
            ceremony_tracker: CeremonyTracker::new(),
            config,
            authority_id,
        }
    }

    /// Get the ceremony tracker
    pub fn ceremony_tracker(&self) -> &CeremonyTracker {
        &self.ceremony_tracker
    }

    /// Get the authority ID
    pub fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }

    /// Get the effect system
    ///
    /// Returns a shared reference to the effect system. The effect system is
    /// immutable after construction; individual handlers manage their own
    /// internal state as needed.
    pub fn effects(&self) -> Arc<AuraEffectSystem> {
        self.effect_system.clone()
    }

    /// Get the context manager
    pub fn contexts(&self) -> &ContextManager {
        &self.context_manager
    }

    /// Get the flow budget manager
    pub fn flow_budgets(&self) -> &FlowBudgetManager {
        &self.flow_budget_manager
    }

    /// Get the receipt manager
    pub fn receipts(&self) -> &ReceiptManager {
        &self.receipt_manager
    }

    /// Get the choreography adapter
    pub fn choreography(&self) -> &ChoreographyAdapter {
        &self.choreography_adapter
    }

    /// Get the lifecycle manager
    pub fn lifecycle(&self) -> &LifecycleManager {
        &self.lifecycle_manager
    }

    /// Get the sync service manager (if enabled)
    pub fn sync(&self) -> Option<&SyncServiceManager> {
        self.sync_manager.as_ref()
    }

    /// Check if sync service is enabled
    pub fn has_sync(&self) -> bool {
        self.sync_manager.is_some()
    }

    /// Get the rendezvous manager (if enabled)
    pub fn rendezvous(&self) -> Option<&RendezvousManager> {
        self.rendezvous_manager.as_ref()
    }

    /// Check if rendezvous service is enabled
    pub fn has_rendezvous(&self) -> bool {
        self.rendezvous_manager.is_some()
    }

    /// Get the social manager (if enabled)
    pub fn social(&self) -> Option<&SocialManager> {
        self.social_manager.as_ref()
    }

    /// Check if social service is enabled
    pub fn has_social(&self) -> bool {
        self.social_manager.is_some()
    }

    /// Shutdown the runtime system
    pub async fn shutdown(self, ctx: &EffectContext) -> Result<(), String> {
        // Stop rendezvous service if running
        if let Some(rendezvous_manager) = &self.rendezvous_manager {
            if let Err(e) = rendezvous_manager.stop().await {
                tracing::warn!("Failed to stop rendezvous service during shutdown: {}", e);
            }
        }

        // Stop sync service if running
        if let Some(sync_manager) = &self.sync_manager {
            if let Err(e) = sync_manager.stop().await {
                tracing::warn!("Failed to stop sync service during shutdown: {}", e);
            }
        }

        self.lifecycle_manager.shutdown(ctx).await
    }
}

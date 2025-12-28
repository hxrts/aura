//! Runtime System
//!
//! Main runtime system that orchestrates all agent operations.

use super::services::{
    CeremonyTracker, ContextManager, FlowBudgetManager, ReceiptManager, RendezvousManager,
    RuntimeTaskRegistry, SocialManager, SyncServiceManager,
};
use super::{
    AuraEffectSystem, ChoreographyAdapter, EffectContext, EffectExecutor, LifecycleManager,
};
use crate::core::AgentConfig;
use crate::fact_registry::build_fact_registry;
use crate::reactive::{FactSource, ReactivePipeline, SchedulerConfig};
use aura_core::effects::task::TaskSpawner;
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::identifiers::AuthorityId;
use aura_core::DeviceId;
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

    /// Runtime task registry for background work
    runtime_tasks: Arc<RuntimeTaskRegistry>,

    /// Configuration
    #[allow(dead_code)] // Will be used for runtime configuration
    config: AgentConfig,

    /// Authority ID
    authority_id: AuthorityId,

    /// Reactive scheduler pipeline (facts → scheduler → view updates).
    ///
    /// This is optional while the single fact pipeline work is being completed.
    reactive_pipeline: Option<ReactivePipeline>,
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
            runtime_tasks: Arc::new(RuntimeTaskRegistry::new()),
            config,
            authority_id,
            reactive_pipeline: None,
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
            runtime_tasks: Arc::new(RuntimeTaskRegistry::new()),
            config,
            authority_id,
            reactive_pipeline: None,
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
            runtime_tasks: Arc::new(RuntimeTaskRegistry::new()),
            config,
            authority_id,
            reactive_pipeline: None,
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
            runtime_tasks: Arc::new(RuntimeTaskRegistry::new()),
            config,
            authority_id,
            reactive_pipeline: None,
        }
    }

    /// Get the ceremony tracker
    pub fn ceremony_tracker(&self) -> &CeremonyTracker {
        &self.ceremony_tracker
    }

    /// Get the runtime task registry.
    pub fn tasks(&self) -> Arc<RuntimeTaskRegistry> {
        self.runtime_tasks.clone()
    }

    /// Get the runtime task spawner as a trait object.
    pub fn task_spawner(&self) -> Arc<dyn TaskSpawner> {
        self.runtime_tasks.clone()
    }

    /// Get the authority ID
    pub fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }

    /// Device id for this runtime instance.
    pub fn device_id(&self) -> DeviceId {
        self.config.device_id
    }

    /// Get the effect system
    ///
    /// Returns a shared reference to the effect system. The effect system is
    /// immutable after construction; individual handlers manage their own
    /// internal state as needed.
    pub fn effects(&self) -> Arc<AuraEffectSystem> {
        self.effect_system.clone()
    }

    /// Start the reactive scheduler pipeline (facts → scheduler → view updates).
    ///
    /// This is a best-effort wiring step; the pipeline becomes fully useful once
    /// the runtime publishes typed facts into it (work/002.md C2).
    pub async fn start_reactive_pipeline(&mut self) -> Result<(), String> {
        if self.reactive_pipeline.is_some() {
            return Ok(());
        }

        let time_effects: Arc<dyn PhysicalTimeEffects> =
            Arc::new(self.effect_system.time_effects().clone());

        let pipeline = ReactivePipeline::start(
            SchedulerConfig::default(),
            build_fact_registry(),
            time_effects,
            self.authority_id,
            self.effect_system.reactive_handler(),
        );

        // Attach the scheduler ingestion channel to the effect system so all journal commits
        // go through the single typed-fact pipeline (work/002.md C2).
        self.effect_system.attach_fact_sink(pipeline.fact_sender());

        // Replay existing persisted facts to seed scheduler-driven UI signals.
        let existing = self
            .effect_system
            .load_committed_facts(self.authority_id)
            .await
            .map_err(|e| e.to_string())?;
        if !existing.is_empty() {
            let _ = pipeline
                .fact_sender()
                .send(FactSource::Journal(existing))
                .await;
        }

        self.reactive_pipeline = Some(pipeline);
        Ok(())
    }

    /// Access the running reactive pipeline (if started).
    pub fn reactive_pipeline(&self) -> Option<&ReactivePipeline> {
        self.reactive_pipeline.as_ref()
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
        let RuntimeSystem {
            lifecycle_manager,
            sync_manager,
            rendezvous_manager,
            reactive_pipeline,
            runtime_tasks,
            ..
        } = self;

        // Stop reactive pipeline (scheduler task) if running.
        if let Some(pipeline) = reactive_pipeline {
            pipeline.shutdown().await;
        }

        // Stop background runtime tasks (invitation monitors, subscriptions).
        runtime_tasks.shutdown();

        // Stop rendezvous service if running
        if let Some(rendezvous_manager) = &rendezvous_manager {
            if let Err(e) = rendezvous_manager.stop().await {
                tracing::warn!("Failed to stop rendezvous service during shutdown: {}", e);
            }
        }

        // Stop sync service if running
        if let Some(sync_manager) = &sync_manager {
            if let Err(e) = sync_manager.stop().await {
                tracing::warn!("Failed to stop sync service during shutdown: {}", e);
            }
        }

        lifecycle_manager.shutdown(ctx).await
    }
}

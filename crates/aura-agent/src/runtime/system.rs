//! Runtime System
//!
//! Main runtime system that orchestrates all agent operations.

use super::{EffectExecutor, AuraEffectSystem, LifecycleManager, ChoreographyAdapter};
use super::services::{ContextManager, FlowBudgetManager, ReceiptManager};
use crate::core::{AgentConfig, AuthorityContext, AgentResult, AgentError};
use aura_core::identifiers::AuthorityId;

/// Main runtime system for the agent
pub struct RuntimeSystem {
    /// Effect executor
    effect_executor: EffectExecutor,
    
    /// Effect system
    effect_system: AuraEffectSystem,
    
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
    
    /// Configuration
    config: AgentConfig,
    
    /// Authority ID
    authority_id: AuthorityId,
}

impl RuntimeSystem {
    /// Create a new runtime system
    pub(crate) fn new(
        effect_executor: EffectExecutor,
        effect_system: AuraEffectSystem,
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
            config,
            authority_id,
        }
    }
    
    /// Get the authority ID
    pub fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }
    
    /// Get the effect system
    pub fn effects(&self) -> &AuraEffectSystem {
        &self.effect_system
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
    
    /// Shutdown the runtime system
    pub async fn shutdown(self) -> Result<(), String> {
        self.lifecycle_manager.shutdown().await
    }
}
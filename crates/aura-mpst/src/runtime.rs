//! Aura MPST Runtime
//!
//! This module provides the runtime infrastructure for executing choreographic
//! protocols with Aura-specific extensions (guards, journal coupling, leakage tracking).

use crate::{
    CapabilityGuard, ContextIsolation, JournalAnnotation, LeakageTracker, MpstError, MpstResult,
};
use async_trait::async_trait;
use aura_core::{Cap, ContextId, DeviceId, Journal, JournalEffects};
use rumpsteak_aura_choreography::effects::{
    ChoreoHandler, ChoreographyError, ExtensibleHandler, ExtensionRegistry, Label,
    Result as ChoreoResult,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Endpoint for choreographic protocol execution
#[derive(Debug, Clone)]
pub struct AuraEndpoint {
    /// Device ID for this endpoint
    pub device_id: DeviceId,
    /// Context ID for isolation
    pub context_id: ContextId,
    /// Connection state tracking
    pub connections: HashMap<DeviceId, ConnectionState>,
    /// Transport metadata
    pub metadata: HashMap<String, String>,
}

/// Connection state for peer devices
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConnectionState {
    /// Connection is active and ready
    Active,
    /// Connection is establishing
    Connecting,
    /// Connection is closed
    Closed,
    /// Connection failed
    Failed(String),
}

impl AuraEndpoint {
    /// Create a new endpoint
    pub fn new(device_id: DeviceId, context_id: ContextId) -> Self {
        Self {
            device_id,
            context_id,
            connections: HashMap::new(),
            metadata: HashMap::new(),
        }
    }

    /// Add connection for a peer
    pub fn add_connection(&mut self, peer: DeviceId, state: ConnectionState) {
        self.connections.insert(peer, state);
    }

    /// Get connection state for a peer
    pub fn connection_state(&self, peer: DeviceId) -> Option<&ConnectionState> {
        self.connections.get(&peer)
    }

    /// Check if connected to peer
    pub fn is_connected_to(&self, peer: DeviceId) -> bool {
        matches!(self.connection_state(peer), Some(ConnectionState::Active))
    }

    /// Add metadata
    pub fn add_metadata(&mut self, key: String, value: String) {
        self.metadata.insert(key, value);
    }
}

/// Aura runtime for MPST protocols
#[derive(Debug, Clone)]
pub struct AuraRuntime {
    /// Device ID for this runtime instance
    pub device_id: DeviceId,
    /// Current capabilities
    pub capabilities: Cap,
    /// Current journal state
    pub journal: Journal,
    /// Capability guards
    pub guards: HashMap<String, CapabilityGuard>,
    /// Journal annotations
    pub annotations: HashMap<String, JournalAnnotation>,
    /// Leakage tracker
    pub leakage_tracker: LeakageTracker,
    /// Context isolation manager
    pub context_isolation: ContextIsolation,
}

impl AuraRuntime {
    /// Create a new Aura runtime
    pub fn new(device_id: DeviceId, capabilities: Cap, journal: Journal) -> Self {
        Self {
            device_id,
            capabilities,
            journal,
            guards: HashMap::new(),
            annotations: HashMap::new(),
            leakage_tracker: LeakageTracker::new(),
            context_isolation: ContextIsolation::new(),
        }
    }

    /// Add a capability guard
    pub fn add_guard(&mut self, name: impl Into<String>, guard: CapabilityGuard) {
        self.guards.insert(name.into(), guard);
    }

    /// Add a journal annotation
    pub fn add_annotation(&mut self, name: impl Into<String>, annotation: JournalAnnotation) {
        self.annotations.insert(name.into(), annotation);
    }

    /// Check all capability guards
    pub fn check_guards(&self) -> MpstResult<()> {
        for (name, guard) in &self.guards {
            guard.enforce(&self.capabilities).map_err(|_| {
                MpstError::capability_guard_failed(format!("Guard '{}' failed", name))
            })?;
        }
        Ok(())
    }

    /// Apply journal annotations
    pub async fn apply_annotations(&mut self, effects: &impl JournalEffects) -> MpstResult<()> {
        for (name, annotation) in &self.annotations {
            self.journal = annotation
                .apply(effects, &self.journal)
                .await
                .map_err(|e| {
                    MpstError::journal_coupling_failed(format!(
                        "Annotation '{}' failed: {}",
                        name, e
                    ))
                })?;
        }
        Ok(())
    }

    /// Update capabilities
    pub fn update_capabilities(&mut self, new_caps: Cap) {
        self.capabilities = new_caps;
    }

    /// Get current journal state
    pub fn journal(&self) -> &Journal {
        &self.journal
    }

    /// Get current capabilities
    pub fn capabilities(&self) -> &Cap {
        &self.capabilities
    }

    /// Get device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Access leakage tracker
    pub fn leakage_tracker(&mut self) -> &mut LeakageTracker {
        &mut self.leakage_tracker
    }

    /// Access context isolation
    pub fn context_isolation(&mut self) -> &mut ContextIsolation {
        &mut self.context_isolation
    }

    /// Validate runtime state
    pub fn validate(&self) -> MpstResult<()> {
        // Check context isolation
        self.context_isolation
            .validate()
            .map_err(|e| MpstError::context_isolation_violated(e.to_string()))?;

        // Check capability guards
        self.check_guards()?;

        Ok(())
    }
}

/// Runtime factory for creating configured Aura runtimes
pub struct AuraRuntimeFactory {
    /// Default capabilities for new runtimes
    pub default_capabilities: Cap,
    /// Default journal for new runtimes
    pub default_journal: Journal,
}

impl AuraRuntimeFactory {
    /// Create a new runtime factory
    pub fn new(default_capabilities: Cap, default_journal: Journal) -> Self {
        Self {
            default_capabilities,
            default_journal,
        }
    }

    /// Create a new runtime for a device
    pub fn create_runtime(&self, device_id: DeviceId) -> AuraRuntime {
        AuraRuntime::new(
            device_id,
            self.default_capabilities.clone(),
            self.default_journal.clone(),
        )
    }

    /// Create runtime with custom capabilities
    pub fn create_runtime_with_caps(&self, device_id: DeviceId, capabilities: Cap) -> AuraRuntime {
        AuraRuntime::new(device_id, capabilities, self.default_journal.clone())
    }

    /// Create runtime with custom journal
    pub fn create_runtime_with_journal(
        &self,
        device_id: DeviceId,
        journal: Journal,
    ) -> AuraRuntime {
        AuraRuntime::new(device_id, self.default_capabilities.clone(), journal)
    }
}

impl Default for AuraRuntimeFactory {
    fn default() -> Self {
        Self::new(
            Cap::top(),     // Most permissive capabilities by default
            Journal::new(), // Empty journal by default
        )
    }
}

/// Protocol execution context
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    /// Protocol name
    pub protocol_name: String,
    /// Execution session ID
    pub session_id: uuid::Uuid,
    /// Participants in this execution
    pub participants: Vec<DeviceId>,
    /// Protocol metadata
    pub metadata: HashMap<String, String>,
}

impl ExecutionContext {
    /// Create a new execution context
    pub fn new(protocol_name: impl Into<String>, participants: Vec<DeviceId>) -> Self {
        Self {
            protocol_name: protocol_name.into(),
            session_id: uuid::Uuid::from_bytes([0u8; 16]), // Deterministic zero UUID
            participants,
            metadata: HashMap::new(),
        }
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Check if device is a participant
    pub fn includes_device(&self, device_id: DeviceId) -> bool {
        self.participants.contains(&device_id)
    }
}

/// Protocol execution trait with Aura extensions
#[async_trait]
pub trait ProtocolExecution {
    /// Execute protocol with runtime validation
    async fn execute(
        &mut self,
        runtime: &mut AuraRuntime,
        context: &ExecutionContext,
        effects: &impl ProtocolEffects,
    ) -> MpstResult<()>;

    /// Validate protocol constraints
    fn validate(&self, runtime: &AuraRuntime) -> MpstResult<()>;

    /// Get protocol requirements
    fn requirements(&self) -> ProtocolRequirements;
}

/// Combined effects interface for protocol execution
#[async_trait]
pub trait ProtocolEffects: JournalEffects + Send + Sync {
    // Protocol-specific effect operations can be added here
}

/// Protocol requirements specification
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProtocolRequirements {
    /// Required capabilities
    pub required_capabilities: Vec<Cap>,
    /// Minimum participants
    pub min_participants: usize,
    /// Maximum participants
    pub max_participants: Option<usize>,
    /// Required leakage budgets
    pub leakage_requirements: Vec<String>,
}

impl ProtocolRequirements {
    /// Create new protocol requirements
    pub fn new() -> Self {
        Self {
            required_capabilities: Vec::new(),
            min_participants: 1,
            max_participants: None,
            leakage_requirements: Vec::new(),
        }
    }

    /// Add capability requirement
    pub fn require_capability(mut self, cap: Cap) -> Self {
        self.required_capabilities.push(cap);
        self
    }

    /// Set participant limits
    pub fn participants(mut self, min: usize, max: Option<usize>) -> Self {
        self.min_participants = min;
        self.max_participants = max;
        self
    }

    /// Add leakage requirement
    pub fn require_leakage_budget(mut self, budget_name: impl Into<String>) -> Self {
        self.leakage_requirements.push(budget_name.into());
        self
    }

    /// Validate requirements against runtime
    pub fn validate(&self, runtime: &AuraRuntime, context: &ExecutionContext) -> MpstResult<()> {
        // Check participant count
        if context.participants.len() < self.min_participants {
            return Err(MpstError::capability_guard_failed(format!(
                "Not enough participants: {} < {}",
                context.participants.len(),
                self.min_participants
            )));
        }

        if let Some(max) = self.max_participants {
            if context.participants.len() > max {
                return Err(MpstError::capability_guard_failed(format!(
                    "Too many participants: {} > {}",
                    context.participants.len(),
                    max
                )));
            }
        }

        // Check capabilities
        use aura_core::MeetSemiLattice;
        for required_cap in &self.required_capabilities {
            if runtime.capabilities.meet(required_cap) != *required_cap {
                return Err(MpstError::capability_guard_failed(
                    "Insufficient capabilities for protocol".to_string(),
                ));
            }
        }

        Ok(())
    }
}

impl Default for ProtocolRequirements {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::DeviceId;

    #[test]
    fn test_aura_runtime_creation() {
        let device_id = DeviceId::new();
        let caps = Cap::top();
        let journal = Journal::new();

        let runtime = AuraRuntime::new(device_id, caps.clone(), journal.clone());
        assert_eq!(runtime.device_id, device_id);
        assert_eq!(runtime.capabilities, caps);
        assert_eq!(runtime.journal, journal);
    }

    #[test]
    fn test_runtime_factory() {
        let factory = AuraRuntimeFactory::default();
        let device_id = DeviceId::new();

        let runtime = factory.create_runtime(device_id);
        assert_eq!(runtime.device_id, device_id);
    }

    #[test]
    fn test_execution_context() {
        let participants = vec![DeviceId::new(), DeviceId::new()];
        let context = ExecutionContext::new("test_protocol", participants.clone());

        assert_eq!(context.protocol_name, "test_protocol");
        assert_eq!(context.participants.len(), 2);
        assert!(context.includes_device(participants[0]));
    }

    #[test]
    fn test_protocol_requirements() {
        let requirements = ProtocolRequirements::new()
            .participants(2, Some(5))
            .require_capability(Cap::top());

        assert_eq!(requirements.min_participants, 2);
        assert_eq!(requirements.max_participants, Some(5));
        assert_eq!(requirements.required_capabilities.len(), 1);
    }
}

/// Aura handler implementing rumpsteak-aura ChoreoHandler
pub struct AuraHandler {
    /// Runtime state
    runtime: AuraRuntime,
    /// Extension registry for Aura-specific annotations
    extension_registry: ExtensionRegistry<AuraEndpoint>,
    /// Role mapping from choreographic names to device IDs
    role_mapping: HashMap<String, DeviceId>,
    /// Flow contexts for budget management
    flow_contexts: HashMap<DeviceId, ContextId>,
    /// Execution mode
    execution_mode: ExecutionMode,
}

/// Execution mode for AuraHandler
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionMode {
    /// Testing mode with in-memory effects
    Testing,
    /// Production mode with real network and storage
    Production,
    /// Simulation mode with fault injection
    Simulation,
}

impl AuraHandler {
    /// Create a new Aura handler for testing
    pub fn for_testing(device_id: DeviceId) -> Result<Self, MpstError> {
        let runtime = AuraRuntime::new(device_id, Cap::top(), Journal::new());
        let extension_registry = Self::create_extension_registry();

        Ok(Self {
            runtime,
            extension_registry,
            role_mapping: HashMap::new(),
            flow_contexts: HashMap::new(),
            execution_mode: ExecutionMode::Testing,
        })
    }

    /// Create a new Aura handler for production
    pub fn for_production(device_id: DeviceId) -> Result<Self, MpstError> {
        let runtime = AuraRuntime::new(device_id, Cap::top(), Journal::new());
        let extension_registry = Self::create_extension_registry();

        Ok(Self {
            runtime,
            extension_registry,
            role_mapping: HashMap::new(),
            flow_contexts: HashMap::new(),
            execution_mode: ExecutionMode::Production,
        })
    }

    /// Create a new Aura handler for simulation
    pub fn for_simulation(device_id: DeviceId) -> Result<Self, MpstError> {
        let runtime = AuraRuntime::new(device_id, Cap::top(), Journal::new());
        let extension_registry = Self::create_extension_registry();

        Ok(Self {
            runtime,
            extension_registry,
            role_mapping: HashMap::new(),
            flow_contexts: HashMap::new(),
            execution_mode: ExecutionMode::Simulation,
        })
    }

    /// Create the extension registry with Aura handlers
    fn create_extension_registry() -> ExtensionRegistry<AuraEndpoint> {
        use rumpsteak_aura_choreography::effects::extension::ExtensionError;

        let mut registry = ExtensionRegistry::new();

        // Register ValidateCapability handler
        registry.register::<crate::extensions::ValidateCapability, _>(
            |endpoint: &mut AuraEndpoint,
             ext: &dyn rumpsteak_aura_choreography::effects::ExtensionEffect| {
                Box::pin(async move {
                    let validate_cap = ext
                        .as_any()
                        .downcast_ref::<crate::extensions::ValidateCapability>()
                        .ok_or_else(|| ExtensionError::TypeMismatch {
                            expected: "ValidateCapability",
                            actual: ext.type_name(),
                        })?;

                    // TODO: Implement capability validation logic
                    // For now, just log the capability check
                    tracing::debug!(
                        device_id = ?endpoint.device_id,
                        capability = %validate_cap.capability,
                        role = %validate_cap.role,
                        "Validating capability"
                    );

                    Ok(())
                })
            },
        );

        // Register ChargeFlowCost handler
        registry.register::<crate::extensions::ChargeFlowCost, _>(
            |endpoint: &mut AuraEndpoint,
             ext: &dyn rumpsteak_aura_choreography::effects::ExtensionEffect| {
                Box::pin(async move {
                    let flow_cost = ext
                        .as_any()
                        .downcast_ref::<crate::extensions::ChargeFlowCost>()
                        .ok_or_else(|| ExtensionError::TypeMismatch {
                            expected: "ChargeFlowCost",
                            actual: ext.type_name(),
                        })?;

                    // TODO: Implement flow cost charging logic
                    // For now, just log the flow cost charge
                    tracing::debug!(
                        device_id = ?endpoint.device_id,
                        cost = flow_cost.cost,
                        operation = %flow_cost.operation,
                        role = %flow_cost.role,
                        "Charging flow cost"
                    );

                    Ok(())
                })
            },
        );

        // Register JournalFact handler
        registry.register::<crate::extensions::JournalFact, _>(
            |endpoint: &mut AuraEndpoint,
             ext: &dyn rumpsteak_aura_choreography::effects::ExtensionEffect| {
                Box::pin(async move {
                    let journal_fact = ext
                        .as_any()
                        .downcast_ref::<crate::extensions::JournalFact>()
                        .ok_or_else(|| ExtensionError::TypeMismatch {
                            expected: "JournalFact",
                            actual: ext.type_name(),
                        })?;

                    // TODO: Implement journal fact recording logic
                    // For now, just log the journal fact
                    tracing::debug!(
                        device_id = ?endpoint.device_id,
                        fact = %journal_fact.fact,
                        operation = %journal_fact.operation,
                        role = %journal_fact.role,
                        "Recording journal fact"
                    );

                    Ok(())
                })
            },
        );

        // Register JournalMerge handler
        registry.register::<crate::extensions::JournalMerge, _>(
            |endpoint: &mut AuraEndpoint,
             ext: &dyn rumpsteak_aura_choreography::effects::ExtensionEffect| {
                Box::pin(async move {
                    let journal_merge = ext
                        .as_any()
                        .downcast_ref::<crate::extensions::JournalMerge>()
                        .ok_or_else(|| ExtensionError::TypeMismatch {
                            expected: "JournalMerge",
                            actual: ext.type_name(),
                        })?;

                    // TODO: Implement journal merge logic
                    // For now, just log the journal merge operation
                    tracing::debug!(
                        device_id = ?endpoint.device_id,
                        merge_type = %journal_merge.merge_type,
                        roles = ?journal_merge.roles,
                        "Executing journal merge"
                    );

                    Ok(())
                })
            },
        );

        // Register ExecuteGuardChain handler
        registry.register::<crate::extensions::ExecuteGuardChain, _>(
            |endpoint: &mut AuraEndpoint,
             ext: &dyn rumpsteak_aura_choreography::effects::ExtensionEffect| {
                Box::pin(async move {
                    let guard_chain = ext
                        .as_any()
                        .downcast_ref::<crate::extensions::ExecuteGuardChain>()
                        .ok_or_else(|| ExtensionError::TypeMismatch {
                            expected: "ExecuteGuardChain",
                            actual: ext.type_name(),
                        })?;

                    // TODO: Implement guard chain execution logic
                    // For now, just log the guard chain execution
                    tracing::debug!(
                        device_id = ?endpoint.device_id,
                        guards = ?guard_chain.guards,
                        operation = %guard_chain.operation,
                        role = %guard_chain.role,
                        "Executing guard chain"
                    );

                    Ok(())
                })
            },
        );

        // Register CompositeExtension handler
        registry.register::<crate::extensions::CompositeExtension, _>(
            |endpoint: &mut AuraEndpoint,
             ext: &dyn rumpsteak_aura_choreography::effects::ExtensionEffect| {
                Box::pin(async move {
                    let composite = ext
                        .as_any()
                        .downcast_ref::<crate::extensions::CompositeExtension>()
                        .ok_or_else(|| ExtensionError::TypeMismatch {
                            expected: "CompositeExtension",
                            actual: ext.type_name(),
                        })?;

                    tracing::debug!(
                        device_id = ?endpoint.device_id,
                        extension_count = composite.extensions.len(),
                        operation = %composite.operation,
                        role = %composite.role,
                        "Executing composite extension"
                    );

                    // Execute all contained extensions in sequence
                    for contained_ext in composite.extensions() {
                        match contained_ext {
                            crate::extensions::ConcreteExtension::ValidateCapability(ext) => {
                                tracing::debug!(
                                    capability = %ext.capability,
                                    role = %ext.role,
                                    "Executing ValidateCapability from composite"
                                );
                                // TODO: Implement actual capability validation logic
                            }
                            crate::extensions::ConcreteExtension::ChargeFlowCost(ext) => {
                                tracing::debug!(
                                    cost = ext.cost,
                                    operation = %ext.operation,
                                    role = %ext.role,
                                    "Executing ChargeFlowCost from composite"
                                );
                                // TODO: Implement actual flow cost charging logic
                            }
                            crate::extensions::ConcreteExtension::JournalFact(ext) => {
                                tracing::debug!(
                                    fact = %ext.fact,
                                    operation = %ext.operation,
                                    role = %ext.role,
                                    "Executing JournalFact from composite"
                                );
                                // TODO: Implement actual journal fact recording logic
                            }
                            crate::extensions::ConcreteExtension::ExecuteGuardChain(ext) => {
                                tracing::debug!(
                                    guards = ?ext.guards,
                                    operation = %ext.operation,
                                    role = %ext.role,
                                    "Executing ExecuteGuardChain from composite"
                                );
                                // TODO: Implement actual guard chain execution logic
                            }
                            crate::extensions::ConcreteExtension::JournalMerge(ext) => {
                                tracing::debug!(
                                    merge_type = %ext.merge_type,
                                    roles = ?ext.roles,
                                    "Executing JournalMerge from composite"
                                );
                                // TODO: Implement actual journal merge logic
                            }
                        }
                    }

                    Ok(())
                })
            },
        );

        registry
    }

    /// Add role mapping from choreographic role name to device ID
    pub fn add_role_mapping(&mut self, role_name: String, device_id: DeviceId) {
        self.role_mapping.insert(role_name, device_id);
    }

    /// Set flow context for a peer device
    pub fn set_flow_context_for_peer(&mut self, peer: DeviceId, context_id: ContextId) {
        self.flow_contexts.insert(peer, context_id);
    }

    /// Get device ID for this handler
    pub fn device_id(&self) -> DeviceId {
        self.runtime.device_id
    }

    /// Access runtime (mutable)
    pub fn runtime_mut(&mut self) -> &mut AuraRuntime {
        &mut self.runtime
    }

    /// Access runtime (immutable)
    pub fn runtime(&self) -> &AuraRuntime {
        &self.runtime
    }
}

#[async_trait]
impl ChoreoHandler for AuraHandler {
    type Role = DeviceId;
    type Endpoint = AuraEndpoint;

    async fn send<M: Serialize + Send + Sync>(
        &mut self,
        endpoint: &mut Self::Endpoint,
        to: Self::Role,
        msg: &M,
    ) -> ChoreoResult<()> {
        // Validate connection
        if !endpoint.is_connected_to(to) {
            return Err(ChoreographyError::Transport(format!(
                "No active connection to device {}",
                to
            )));
        }

        // Check capability guards (simplified - full implementation in Priority 2)
        self.runtime.check_guards().map_err(|e| {
            ChoreographyError::ProtocolViolation(format!("Guard check failed: {}", e))
        })?;

        // TODO: Apply extension effects for this message
        // TODO: Charge flow budgets
        // TODO: Update journal with facts

        // Simulate message sending based on execution mode
        match self.execution_mode {
            ExecutionMode::Testing => {
                println!(
                    "TEST SEND: {} -> {}: {} bytes",
                    endpoint.device_id,
                    to,
                    serde_json::to_string(msg).unwrap_or_default().len()
                );
            }
            ExecutionMode::Production => {
                // TODO: Integrate with actual NetworkEffects
                println!("PROD SEND: {} -> {}: message", endpoint.device_id, to);
            }
            ExecutionMode::Simulation => {
                // TODO: Add fault injection
                println!("SIM SEND: {} -> {}: message", endpoint.device_id, to);
            }
        }

        Ok(())
    }

    async fn recv<M: for<'de> serde::Deserialize<'de> + Send>(
        &mut self,
        endpoint: &mut Self::Endpoint,
        from: Self::Role,
    ) -> ChoreoResult<M> {
        // Validate connection
        if !endpoint.is_connected_to(from) {
            return Err(ChoreographyError::Transport(format!(
                "No active connection to device {}",
                from
            )));
        }

        // Simulate message receiving based on execution mode
        match self.execution_mode {
            ExecutionMode::Testing => {
                println!(
                    "TEST RECV: {} <- {}: waiting for message",
                    endpoint.device_id, from
                );
                // TODO: Return mock message for testing
                return Err(ChoreographyError::Transport(
                    "Mock message receiving not implemented".to_string(),
                ));
            }
            ExecutionMode::Production => {
                // TODO: Integrate with actual NetworkEffects
                return Err(ChoreographyError::Transport(
                    "Production message receiving not implemented".to_string(),
                ));
            }
            ExecutionMode::Simulation => {
                // TODO: Add fault injection
                return Err(ChoreographyError::Transport(
                    "Simulation message receiving not implemented".to_string(),
                ));
            }
        }
    }

    async fn choose(
        &mut self,
        _endpoint: &mut Self::Endpoint,
        _who: Self::Role,
        _label: Label,
    ) -> ChoreoResult<()> {
        // TODO: Implement choice selection
        Ok(())
    }

    async fn offer(
        &mut self,
        _endpoint: &mut Self::Endpoint,
        _from: Self::Role,
    ) -> ChoreoResult<Label> {
        // TODO: Implement choice offering
        Err(ChoreographyError::Transport(
            "Choice offering not implemented".to_string(),
        ))
    }

    async fn with_timeout<F, T>(
        &mut self,
        _endpoint: &mut Self::Endpoint,
        _at: Self::Role,
        _dur: Duration,
        _body: F,
    ) -> ChoreoResult<T>
    where
        F: std::future::Future<Output = ChoreoResult<T>> + Send,
    {
        // TODO: Implement timeout support
        Err(ChoreographyError::Transport(
            "Timeout not implemented".to_string(),
        ))
    }
}

impl ExtensibleHandler for AuraHandler {
    type Endpoint = AuraEndpoint;

    fn extension_registry(&self) -> &ExtensionRegistry<Self::Endpoint> {
        &self.extension_registry
    }
}

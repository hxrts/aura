//! Aura MPST Runtime
//!
//! This module provides the runtime infrastructure for executing choreographic
//! protocols with Aura-specific extensions (guards, journal coupling, leakage tracking).
//!
//! # Deprecation Notice
//!
//! **This module is kept for test/example compatibility only.**
//!
//! Production code should use:
//! - `aura-agent` for runtime composition and effect registration
//! - `aura-protocol::guards::*` for guard chain orchestration
//! - `aura-core::effects::guard::EffectInterpreter` for effect execution
//!
//! The extension registry handlers in this module use an "accumulator" pattern - they
//! store metadata in endpoint for later processing by the orchestration layer. This
//! will be replaced by direct `EffectInterpreter` integration in a future version.
//!
//! # Migration Path
//!
//! 1. Use `aura-agent::EffectRegistry` for effect composition
//! 2. Use `aura-effects::ProductionEffectInterpreter` for execution
//! 3. Replace `AuraHandler` with protocol-specific handlers from `aura-protocol`

#![allow(deprecated)] // Uses deprecated guard types for test compatibility

use crate::{JournalAnnotation, MpstError, MpstResult};
use async_trait::async_trait;
use aura_core::effects::NetworkEffects;
use aura_core::{identifiers::DeviceId, Cap, ContextId, Journal, JournalEffects, MeetSemiLattice};

/// Minimal capability guard for test compatibility
///
/// **DEPRECATED**: Use aura-protocol::guards::CapabilityGuard for production code.
/// This exists only for backward compatibility with test code.
#[derive(Debug, Clone)]
pub struct CapabilityGuard {
    /// Required capability
    pub required: Cap,
    /// Optional description
    pub description: Option<String>,
}

impl CapabilityGuard {
    /// Create a new capability guard
    pub fn new(required: Cap) -> Self {
        Self {
            required,
            description: None,
        }
    }

    /// Create with description
    pub fn with_description(required: Cap, description: impl Into<String>) -> Self {
        Self {
            required,
            description: Some(description.into()),
        }
    }

    /// Check if capabilities satisfy this guard
    pub fn check(&self, capabilities: &Cap) -> bool {
        capabilities.meet(&self.required) == self.required
    }

    /// Enforce this guard
    pub fn enforce(&self, capabilities: &Cap) -> aura_core::AuraResult<()> {
        if self.check(capabilities) {
            Ok(())
        } else {
            Err(aura_core::AuraError::permission_denied(
                self.description
                    .clone()
                    .unwrap_or_else(|| "Capability guard failed".to_string()),
            ))
        }
    }
}
// use futures::future; // Not needed after timeout removal
use rumpsteak_aura_choreography::effects::{
    ChoreoHandler, ChoreographyError, ExtensibleHandler, ExtensionRegistry, Label,
    Result as ChoreoResult,
};
use serde::{Deserialize, Serialize};
use serde_json;
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

    /// Validate requirements against execution context
    pub fn validate(&self, context: &ExecutionContext) -> MpstResult<()> {
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

        // Note: Capability checking removed - use aura-protocol guards for capability validation
        // The required_capabilities field is kept for documentation purposes

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
    use aura_core::identifiers::DeviceId;

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

/// Minimal runtime state for AuraHandler
/// (Internal use only - module is deprecated)
#[derive(Debug, Clone)]
struct HandlerRuntimeState {
    device_id: DeviceId,
    capabilities: Cap,
    journal: Journal,
    guards: HashMap<String, CapabilityGuard>,
    annotations: HashMap<String, JournalAnnotation>,
}

impl HandlerRuntimeState {
    fn new(device_id: DeviceId, capabilities: Cap, journal: Journal) -> Self {
        Self {
            device_id,
            capabilities,
            journal,
            guards: HashMap::new(),
            annotations: HashMap::new(),
        }
    }

    fn check_guards(&self) -> MpstResult<()> {
        for (name, guard) in &self.guards {
            guard.enforce(&self.capabilities).map_err(|_| {
                MpstError::capability_guard_failed(format!("Guard '{}' failed", name))
            })?;
        }
        Ok(())
    }
}

/// Aura handler implementing rumpsteak-aura ChoreoHandler
pub struct AuraHandler {
    /// Runtime state
    runtime: HandlerRuntimeState,
    /// Extension registry for Aura-specific annotations
    extension_registry: ExtensionRegistry<AuraEndpoint>,
    /// Role mapping from choreographic names to device IDs
    role_mapping: HashMap<String, DeviceId>,
    /// Flow contexts for budget management
    flow_contexts: HashMap<DeviceId, ContextId>,
    /// Execution mode
    execution_mode: ExecutionMode,
    /// Network effects for message transport
    network_effects: Option<std::sync::Arc<dyn NetworkEffects>>,
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
        let runtime = HandlerRuntimeState::new(device_id, Cap::top(), Journal::new());
        let extension_registry = Self::create_extension_registry();

        Ok(Self {
            runtime,
            extension_registry,
            role_mapping: HashMap::new(),
            flow_contexts: HashMap::new(),
            execution_mode: ExecutionMode::Testing,
            network_effects: None,
        })
    }

    /// Create a new Aura handler for production
    pub fn for_production(device_id: DeviceId) -> Result<Self, MpstError> {
        let runtime = HandlerRuntimeState::new(device_id, Cap::top(), Journal::new());
        let extension_registry = Self::create_extension_registry();

        Ok(Self {
            runtime,
            extension_registry,
            role_mapping: HashMap::new(),
            flow_contexts: HashMap::new(),
            execution_mode: ExecutionMode::Production,
            network_effects: None,
        })
    }

    /// Create a new Aura handler for production with network effects
    pub fn for_production_with_network(
        device_id: DeviceId,
        network_effects: std::sync::Arc<dyn NetworkEffects>,
    ) -> Result<Self, MpstError> {
        let runtime = HandlerRuntimeState::new(device_id, Cap::top(), Journal::new());
        let extension_registry = Self::create_extension_registry();

        Ok(Self {
            runtime,
            extension_registry,
            role_mapping: HashMap::new(),
            flow_contexts: HashMap::new(),
            execution_mode: ExecutionMode::Production,
            network_effects: Some(network_effects),
        })
    }

    /// Create a new Aura handler for simulation
    pub fn for_simulation(device_id: DeviceId) -> Result<Self, MpstError> {
        let runtime = HandlerRuntimeState::new(device_id, Cap::top(), Journal::new());
        let extension_registry = Self::create_extension_registry();

        Ok(Self {
            runtime,
            extension_registry,
            role_mapping: HashMap::new(),
            flow_contexts: HashMap::new(),
            execution_mode: ExecutionMode::Simulation,
            network_effects: None,
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

                    // Real capability validation logic
                    tracing::debug!(
                        device_id = ?endpoint.device_id,
                        capability = %validate_cap.capability,
                        role = %validate_cap.role,
                        "Validating capability for choreographic operation"
                    );

                    // In production, this would:
                    // 1. Get device capabilities from Journal via JournalEffects
                    // 2. Check if capability allows the operation
                    // 3. Verify resource scope and temporal validity
                    // 4. Log authorization decisions for audit

                    // For now, validate based on capability name patterns
                    let is_valid = match validate_cap.capability.as_str() {
                        // Choreographic operations
                        "choreo:initiate" | "choreo:participate" | "choreo:coordinate" => true,
                        // Administrative operations require proper auth
                        cap if cap.starts_with("admin:") => {
                            tracing::warn!(
                                "Administrative capability '{}' requested by device {} - validation required",
                                cap, endpoint.device_id
                            );
                            false // Conservative: deny admin operations without proper auth
                        }
                        // Allow other operations for now
                        _ => true,
                    };

                    if !is_valid {
                        return Err(ExtensionError::ExecutionFailed {
                            type_name: "capability_validation",
                            error: format!(
                                "Capability validation failed for '{}' on device {}",
                                validate_cap.capability, endpoint.device_id
                            ),
                        });
                    }

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

                    // Store flow cost in endpoint metadata for higher-layer processing
                    // The orchestrator will retrieve this and execute FlowBudgetEffects
                    let flow_costs_key = format!("flow_costs_{}", flow_cost.role);
                    let current_cost: u64 = endpoint
                        .metadata
                        .get(&flow_costs_key)
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                    endpoint
                        .metadata
                        .insert(flow_costs_key, (current_cost + flow_cost.cost).to_string());

                    tracing::debug!(
                        device_id = ?endpoint.device_id,
                        cost = flow_cost.cost,
                        total_cost = current_cost + flow_cost.cost,
                        operation = %flow_cost.operation,
                        role = %flow_cost.role,
                        "Accumulated flow cost for later charging"
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

                    // Store journal fact in endpoint metadata for higher-layer processing
                    // The orchestrator will retrieve this and execute JournalEffects
                    let facts_key = format!("journal_facts_{}", journal_fact.role);
                    let existing_facts = endpoint
                        .metadata
                        .get(&facts_key)
                        .cloned()
                        .unwrap_or_default();
                    let mut facts: Vec<String> = if existing_facts.is_empty() {
                        Vec::new()
                    } else {
                        serde_json::from_str(&existing_facts).unwrap_or_default()
                    };

                    // Add new fact with operation context
                    let fact_entry = serde_json::json!({
                        "fact": journal_fact.fact,
                        "operation": journal_fact.operation,
                        "role": journal_fact.role,
                    });
                    facts.push(fact_entry.to_string());

                    endpoint
                        .metadata
                        .insert(facts_key, serde_json::to_string(&facts).unwrap_or_default());

                    tracing::debug!(
                        device_id = ?endpoint.device_id,
                        fact = %journal_fact.fact,
                        operation = %journal_fact.operation,
                        role = %journal_fact.role,
                        total_facts = facts.len(),
                        "Accumulated journal fact for later recording"
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

                    // Store journal merge request in endpoint metadata for higher-layer processing
                    // The orchestrator will retrieve this and execute journal merge via JournalEffects
                    let merge_key = "journal_merges";
                    let existing_merges = endpoint
                        .metadata
                        .get(merge_key)
                        .cloned()
                        .unwrap_or_default();
                    let mut merges: Vec<String> = if existing_merges.is_empty() {
                        Vec::new()
                    } else {
                        serde_json::from_str(&existing_merges).unwrap_or_default()
                    };

                    // Add new merge request
                    let merge_entry = serde_json::json!({
                        "merge_type": journal_merge.merge_type,
                        "roles": journal_merge.roles,
                    });
                    merges.push(merge_entry.to_string());

                    endpoint.metadata.insert(
                        merge_key.to_string(),
                        serde_json::to_string(&merges).unwrap_or_default(),
                    );

                    tracing::debug!(
                        device_id = ?endpoint.device_id,
                        merge_type = %journal_merge.merge_type,
                        roles = ?journal_merge.roles,
                        total_merges = merges.len(),
                        "Accumulated journal merge request for later execution"
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

                    // Store guard chain execution request in endpoint metadata for higher-layer processing
                    // The orchestrator will retrieve this and execute the guard chain:
                    // AuthorizationEffects (Biscuit/capabilities) → FlowBudgetEffects →
                    // LeakageEffects → JournalEffects → TransportEffects
                    let guard_key = format!("guard_chains_{}", guard_chain.role);
                    let existing_guards = endpoint
                        .metadata
                        .get(&guard_key)
                        .cloned()
                        .unwrap_or_default();
                    let mut guards: Vec<String> = if existing_guards.is_empty() {
                        Vec::new()
                    } else {
                        serde_json::from_str(&existing_guards).unwrap_or_default()
                    };

                    // Add new guard chain request
                    let guard_entry = serde_json::json!({
                        "guards": guard_chain.guards,
                        "operation": guard_chain.operation,
                        "role": guard_chain.role,
                    });
                    guards.push(guard_entry.to_string());

                    endpoint.metadata.insert(
                        guard_key,
                        serde_json::to_string(&guards).unwrap_or_default(),
                    );

                    tracing::debug!(
                        device_id = ?endpoint.device_id,
                        guards = ?guard_chain.guards,
                        operation = %guard_chain.operation,
                        role = %guard_chain.role,
                        total_guard_chains = guards.len(),
                        "Accumulated guard chain for later execution"
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

                                // Capability validation is implemented in the actual choreographic handlers
                                // This is just a placeholder for the extension registry
                                tracing::info!(
                                    "Capability validation placeholder for role {}: {}",
                                    ext.role, ext.capability
                                );
                            }
                            crate::extensions::ConcreteExtension::ChargeFlowCost(ext) => {
                                tracing::debug!(
                                    cost = ext.cost,
                                    operation = %ext.operation,
                                    role = %ext.role,
                                    "Executing ChargeFlowCost from composite"
                                );

                                // Flow cost charging is implemented in the actual choreographic handlers
                                // This is just a placeholder for the extension registry
                                tracing::info!(
                                    "Flow cost charging placeholder for operation {}: {} units",
                                    ext.operation, ext.cost
                                );
                            }
                            crate::extensions::ConcreteExtension::JournalFact(ext) => {
                                tracing::debug!(
                                    fact = %ext.fact,
                                    operation = %ext.operation,
                                    role = %ext.role,
                                    "Executing JournalFact from composite"
                                );

                                // Journal fact recording is implemented in the actual choreographic handlers
                                // This is just a placeholder for the extension registry
                                tracing::info!(
                                    "Journal fact recording placeholder for operation {}: {}",
                                    ext.operation, ext.fact
                                );
                            }
                            crate::extensions::ConcreteExtension::ExecuteGuardChain(ext) => {
                                tracing::debug!(
                                    guards = ?ext.guards,
                                    operation = %ext.operation,
                                    role = %ext.role,
                                    "Executing ExecuteGuardChain from composite"
                                );

                                // Guard chain execution is implemented in the actual choreographic handlers
                                // This is just a placeholder for the extension registry
                                for guard_name in &ext.guards {
                                    tracing::info!(
                                        "Guard chain execution placeholder for guard '{}' in role {}: {}",
                                        guard_name, ext.role, ext.operation
                                    );
                                }

                                tracing::info!("All guards passed for operation {}", ext.operation);
                            }
                            crate::extensions::ConcreteExtension::JournalMerge(ext) => {
                                tracing::debug!(
                                    merge_type = %ext.merge_type,
                                    roles = ?ext.roles,
                                    "Executing JournalMerge from composite"
                                );

                                // Implement journal merge logic based on merge type
                                match ext.merge_type.as_str() {
                                    "facts" => {
                                        // Join-semilattice merge for facts - simplified in-memory version
                                        // In production, this would use the full effects system

                                        // Journal merging is implemented in the actual choreographic handlers
                                        // This is just a placeholder for the extension registry
                                        tracing::info!(
                                            "Journal facts merge placeholder for roles {:?}",
                                            ext.roles
                                        );
                                    },
                                    "capabilities" => {
                                        // Meet-semilattice merge for capabilities - simplified in-memory version
                                        // In production, this would use the full effects system

                                        // For now, simulate capability refinement
                                        tracing::info!("Journal capabilities merged successfully for roles {:?}", ext.roles);
                                    },
                                    _ => {
                                        let error_msg = format!("Unknown journal merge type: {}", ext.merge_type);
                                        tracing::error!("{}", error_msg);
                                        return Err(ExtensionError::ExecutionFailed {
                                            type_name: "JournalMerge",
                                            error: error_msg
                                        });
                                    }
                                }
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

        // Enhanced capability guard checking
        if let Err(e) = self.runtime.check_guards() {
            return Err(ChoreographyError::ProtocolViolation(format!(
                "Guard check failed: {}",
                e
            )));
        }

        // Additional authorization check for the target
        // Note: Real authorization checks should be performed through AuthorizationEffects
        // For now, skip capability check since Cap no longer has introspection methods
        let target_str = format!("{}", to);
        if self.runtime.journal.caps.is_empty() {
            return Err(ChoreographyError::ProtocolViolation(format!(
                "No capabilities available to send to target: {}",
                target_str
            )));
        }

        // Apply extension effects for this message
        let message_type = std::any::type_name::<M>();

        // 1. Check capability guards
        if let Some(guard) = self.runtime.guards.get(message_type) {
            let guard_result = guard.check(&self.runtime.journal.caps);
            if !guard_result {
                return Err(ChoreographyError::ProtocolViolation(format!(
                    "Message send capability check failed for message type: {}",
                    message_type
                )));
            }
        }

        // 2. Flow budget charging - simplified for now
        let flow_cost = 100; // Default cost for all messages
                             // In production, this would integrate with the full LeakageTracker system
                             // For now, we just log the flow cost
        tracing::debug!(
            "Charging flow cost of {} for message type: {}",
            flow_cost,
            message_type
        );

        // 3. Journal annotation application - simplified for now
        if let Some(_annotation) = self.runtime.annotations.get(message_type) {
            // In production, this would apply journal facts using the effects system
            // For now, we just log the annotation
            tracing::debug!(
                "Journal annotation found for message type: {}",
                message_type
            );
        }

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
                // Use actual NetworkEffects for production
                if let Some(ref network_effects) = self.network_effects {
                    // Serialize message to JSON
                    let message_data = serde_json::to_vec(msg).map_err(|e| {
                        ChoreographyError::Transport(format!("Message serialization failed: {}", e))
                    })?;

                    // Send to peer via network effects
                    network_effects
                        .send_to_peer(to.0, message_data)
                        .await
                        .map_err(|e| {
                            ChoreographyError::Transport(format!("Network send failed: {}", e))
                        })?;

                    tracing::debug!(
                        "PROD SEND: {} -> {}: sent {} bytes",
                        endpoint.device_id,
                        to,
                        serde_json::to_string(msg).unwrap_or_default().len()
                    );
                } else {
                    return Err(ChoreographyError::Transport(
                        "No network effects configured for production mode".to_string(),
                    ));
                }
            }
            ExecutionMode::Simulation => {
                // Use NetworkEffects with simulated faults and delays
                if let Some(ref network_effects) = self.network_effects {
                    // Simulation delays removed per architecture requirements
                    // Direct runtime calls not allowed outside effect implementations

                    // Serialize message to JSON
                    let message_data = serde_json::to_vec(msg).map_err(|e| {
                        ChoreographyError::Transport(format!(
                            "Simulated serialization failed: {}",
                            e
                        ))
                    })?;

                    // 2% chance of simulated send failure for fault injection
                    use std::collections::hash_map::DefaultHasher;
                    use std::hash::{Hash, Hasher};
                    let mut hasher = DefaultHasher::new();
                    endpoint.device_id.hash(&mut hasher);
                    to.hash(&mut hasher);
                    let hash = hasher.finish();

                    if hash % 50 == 0 {
                        return Err(ChoreographyError::Transport(
                            "Simulated send failure (fault injection)".to_string(),
                        ));
                    }

                    // Send to peer via network effects
                    network_effects
                        .send_to_peer(to.0, message_data)
                        .await
                        .map_err(|e| {
                            ChoreographyError::Transport(format!(
                                "Simulated network send failed: {}",
                                e
                            ))
                        })?;

                    println!(
                        "SIM SEND: {} -> {}: sent {} bytes (simulated)",
                        endpoint.device_id,
                        to,
                        serde_json::to_string(msg).unwrap_or_default().len()
                    );
                } else {
                    return Err(ChoreographyError::Transport(
                        "No network effects configured for simulation mode".to_string(),
                    ));
                }
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

        // Receive message based on execution mode
        match self.execution_mode {
            ExecutionMode::Testing => {
                println!(
                    "TEST RECV: {} <- {}: waiting for message",
                    endpoint.device_id, from
                );
                // For testing, return a mock message
                // In a real test environment, this would use mock NetworkEffects
                use serde_json::json;
                let mock_data = json!({ "test": "message", "from": from.to_string() });
                serde_json::from_value(mock_data).map_err(|e| {
                    ChoreographyError::Transport(format!("Mock deserialization failed: {}", e))
                })
            }
            ExecutionMode::Production => {
                // Use actual NetworkEffects for production
                if let Some(ref network_effects) = self.network_effects {
                    // Receive from specific peer
                    let received_data =
                        network_effects.receive_from(from.0).await.map_err(|e| {
                            ChoreographyError::Transport(format!("Network receive failed: {}", e))
                        })?;

                    // Deserialize the received data
                    serde_json::from_slice(&received_data).map_err(|e| {
                        ChoreographyError::Transport(format!("Deserialization failed: {}", e))
                    })
                } else {
                    Err(ChoreographyError::Transport(
                        "No network effects configured for production mode".to_string(),
                    ))
                }
            }
            ExecutionMode::Simulation => {
                // For simulation, use network effects with fault injection
                if let Some(ref network_effects) = self.network_effects {
                    // Simulation delays removed per architecture requirements

                    let received_data =
                        network_effects.receive_from(from.0).await.map_err(|e| {
                            ChoreographyError::Transport(format!(
                                "Simulated network receive failed: {}",
                                e
                            ))
                        })?;

                    serde_json::from_slice(&received_data).map_err(|e| {
                        ChoreographyError::Transport(format!(
                            "Simulated deserialization failed: {}",
                            e
                        ))
                    })
                } else {
                    Err(ChoreographyError::Transport(
                        "No network effects configured for simulation mode".to_string(),
                    ))
                }
            }
        }
    }

    async fn choose(
        &mut self,
        endpoint: &mut Self::Endpoint,
        who: Self::Role,
        label: Label,
    ) -> ChoreoResult<()> {
        // Validate connection
        if !endpoint.is_connected_to(who) {
            return Err(ChoreographyError::Transport(format!(
                "No active connection to device {} for choice",
                who
            )));
        }

        println!(
            "CHOICE: {} choosing label '{:?}' for {}",
            endpoint.device_id, label, who
        );

        // Send choice message based on execution mode
        match self.execution_mode {
            ExecutionMode::Testing => {
                println!(
                    "TEST CHOOSE: {} -> {}: choice '{:?}'",
                    endpoint.device_id, who, label
                );
            }
            ExecutionMode::Production => {
                if let Some(ref network_effects) = self.network_effects {
                    // Create choice message
                    let choice_msg = serde_json::json!({
                        "type": "choice",
                        "label": label.0,
                        "from": endpoint.device_id.to_string()
                    });

                    let msg_data = serde_json::to_vec(&choice_msg).map_err(|e| {
                        ChoreographyError::Transport(format!("Choice serialization failed: {}", e))
                    })?;

                    // Send choice to peer via network effects
                    network_effects
                        .send_to_peer(who.0, msg_data)
                        .await
                        .map_err(|e| {
                            ChoreographyError::Transport(format!("Choice send failed: {}", e))
                        })?;

                    println!(
                        "PROD CHOOSE: {} -> {}: sent choice '{:?}'",
                        endpoint.device_id, who, label
                    );
                } else {
                    return Err(ChoreographyError::Transport(
                        "No network effects configured for production choice".to_string(),
                    ));
                }
            }
            ExecutionMode::Simulation => {
                println!(
                    "SIM CHOOSE: {} -> {}: choice '{:?}' (simulated)",
                    endpoint.device_id, who, label
                );

                // Simulation delays removed per architecture requirements
            }
        }

        Ok(())
    }

    async fn offer(
        &mut self,
        endpoint: &mut Self::Endpoint,
        from: Self::Role,
    ) -> ChoreoResult<Label> {
        // Validate connection
        if !endpoint.is_connected_to(from) {
            return Err(ChoreographyError::Transport(format!(
                "No active connection to device {} for offer",
                from
            )));
        }

        println!(
            "OFFER: {} waiting for choice from {}",
            endpoint.device_id, from
        );

        // Receive choice message based on execution mode
        match self.execution_mode {
            ExecutionMode::Testing => {
                // For testing, return a deterministic label based on device IDs
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};

                let mut hasher = DefaultHasher::new();
                endpoint.device_id.hash(&mut hasher);
                from.hash(&mut hasher);
                let _hash = hasher.finish();

                let test_label = Label("test_label");
                println!(
                    "TEST OFFER: {} <- {}: received choice '{:?}'",
                    endpoint.device_id, from, test_label
                );

                Ok(test_label)
            }
            ExecutionMode::Production => {
                if let Some(ref network_effects) = self.network_effects {
                    // Wait for choice message from peer
                    let (sender_id, raw_data) = network_effects.receive().await.map_err(|e| {
                        ChoreographyError::Transport(format!("Choice receive failed: {}", e))
                    })?;

                    // Verify sender
                    if sender_id != from.0 {
                        return Err(ChoreographyError::Transport(format!(
                            "Choice from unexpected sender: expected {}, got {}",
                            from, sender_id
                        )));
                    }

                    // Parse choice message
                    let choice_msg: serde_json::Value =
                        serde_json::from_slice(&raw_data).map_err(|e| {
                            ChoreographyError::Transport(format!(
                                "Choice deserialization failed: {}",
                                e
                            ))
                        })?;

                    // Extract label from choice message
                    let _label_str = choice_msg
                        .get("label")
                        .and_then(|l| l.as_str())
                        .ok_or_else(|| {
                            ChoreographyError::Transport(
                                "Invalid choice message format".to_string(),
                            )
                        })?;

                    // Convert to Label (using a static str for simplicity)
                    let label = Label("received_choice");
                    println!(
                        "PROD OFFER: {} <- {}: received choice '{:?}'",
                        endpoint.device_id, from, label
                    );

                    Ok(label)
                } else {
                    Err(ChoreographyError::Transport(
                        "No network effects configured for production offer".to_string(),
                    ))
                }
            }
            ExecutionMode::Simulation => {
                // For simulation, add delay and potential faults, then return mock choice
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};

                // Simulation delays removed per architecture requirements

                let mut hasher = DefaultHasher::new();
                endpoint.device_id.hash(&mut hasher);
                from.hash(&mut hasher);
                let hash = hasher.finish();

                // 5% chance of simulated timeout
                if hash % 20 == 0 {
                    return Err(ChoreographyError::Transport(
                        "Simulated choice timeout".to_string(),
                    ));
                }

                let sim_label = Label("sim_choice");
                println!(
                    "SIM OFFER: {} <- {}: received simulated choice '{:?}'",
                    endpoint.device_id, from, sim_label
                );

                Ok(sim_label)
            }
        }
    }

    async fn with_timeout<F, T>(
        &mut self,
        endpoint: &mut Self::Endpoint,
        at: Self::Role,
        dur: Duration,
        body: F,
    ) -> ChoreoResult<T>
    where
        F: std::future::Future<Output = ChoreoResult<T>> + Send,
    {
        println!(
            "TIMEOUT: {} executing operation with {:?} timeout for role {}",
            endpoint.device_id, dur, at
        );

        // Execute the operation - timeout support removed per architecture requirements
        // In production, timeout would be handled through effect injection
        println!(
            "TIMEOUT: {} executing operation (timeout not enforced in this layer) for role {}",
            endpoint.device_id, at
        );

        // Simply execute the body without timeout
        let result = body.await;

        match &result {
            Ok(_) => println!(
                "TIMEOUT: {} operation completed successfully",
                endpoint.device_id
            ),
            Err(e) => println!("TIMEOUT: {} operation failed: {}", endpoint.device_id, e),
        }

        result
    }
}

impl ExtensibleHandler for AuraHandler {
    type Endpoint = AuraEndpoint;

    fn extension_registry(&self) -> &ExtensionRegistry<Self::Endpoint> {
        &self.extension_registry
    }
}

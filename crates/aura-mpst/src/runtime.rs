//! Aura MPST Runtime
//!
//! This module provides the runtime infrastructure for executing choreographic
//! protocols with Aura-specific extensions (guards, journal coupling, leakage tracking).

use crate::{
    CapabilityGuard, ContextIsolation, JournalAnnotation, LeakageTracker, MpstError, MpstResult,
};
use async_trait::async_trait;
use aura_core::{Cap, DeviceId, Journal, JournalEffects};
use std::collections::HashMap;

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
    #[allow(clippy::disallowed_methods)]
    pub fn new(protocol_name: impl Into<String>, participants: Vec<DeviceId>) -> Self {
        Self {
            protocol_name: protocol_name.into(),
            session_id: uuid::Uuid::from_bytes([0u8; 16]),
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

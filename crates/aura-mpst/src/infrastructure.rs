//! Core Protocol Infrastructure
//!
//! This module provides the foundational infrastructure for executing
//! choreographic protocols with Aura extensions. It focuses on the framework
//! and runtime support needed by application protocols.

use crate::{
    runtime::{AuraRuntime, ExecutionContext, ProtocolRequirements},
    CapabilityGuard, JournalAnnotation, MpstError, MpstResult,
};
use async_trait::async_trait;
use aura_core::{Cap, DeviceId, Journal};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Protocol infrastructure manager
#[derive(Debug)]
pub struct ProtocolInfrastructure {
    /// Registered protocol types
    protocols: HashMap<String, ProtocolDefinition>,
    /// Active execution contexts
    active_contexts: HashMap<uuid::Uuid, ExecutionContext>,
    /// Global capability guards
    global_guards: HashMap<String, CapabilityGuard>,
    /// Global journal annotations
    global_annotations: HashMap<String, JournalAnnotation>,
    /// Privacy enforcement settings
    privacy_settings: PrivacySettings,
}

/// Protocol definition metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolDefinition {
    /// Protocol name
    pub name: String,
    /// Protocol version
    pub version: String,
    /// Requirements for this protocol
    pub requirements: ProtocolRequirements,
    /// Description
    pub description: Option<String>,
    /// Choreography source (for reference)
    pub choreography_source: Option<String>,
}

/// Privacy enforcement settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacySettings {
    /// Enforce context isolation
    pub enforce_context_isolation: bool,
    /// Enforce leakage budgets
    pub enforce_leakage_budgets: bool,
    /// Enable privacy contract validation
    pub validate_privacy_contracts: bool,
    /// Default leakage budget per device per day
    pub default_daily_budget: u64,
}

impl Default for PrivacySettings {
    fn default() -> Self {
        Self {
            enforce_context_isolation: true,
            enforce_leakage_budgets: true,
            validate_privacy_contracts: true,
            default_daily_budget: 10_000, // 10KB per day default
        }
    }
}

impl ProtocolInfrastructure {
    /// Create new protocol infrastructure
    pub fn new() -> Self {
        Self {
            protocols: HashMap::new(),
            active_contexts: HashMap::new(),
            global_guards: HashMap::new(),
            global_annotations: HashMap::new(),
            privacy_settings: PrivacySettings::default(),
        }
    }

    /// Register a protocol type
    pub fn register_protocol(&mut self, definition: ProtocolDefinition) -> MpstResult<()> {
        if self.protocols.contains_key(&definition.name) {
            return Err(MpstError::protocol_analysis_error(format!(
                "Protocol '{}' already registered",
                definition.name
            )));
        }

        // Validate requirements
        definition.requirements.validate(
            &AuraRuntime::new(DeviceId::new(), Cap::top(), Journal::new()),
            &ExecutionContext::new(&definition.name, vec![DeviceId::new()]),
        )?;

        self.protocols.insert(definition.name.clone(), definition);
        Ok(())
    }

    /// Get protocol definition
    pub fn get_protocol(&self, name: &str) -> Option<&ProtocolDefinition> {
        self.protocols.get(name)
    }

    /// Create execution context for protocol
    pub fn create_context(
        &mut self,
        protocol_name: &str,
        participants: Vec<DeviceId>,
    ) -> MpstResult<ExecutionContext> {
        let protocol = self.get_protocol(protocol_name).ok_or_else(|| {
            MpstError::protocol_analysis_error(format!(
                "Protocol '{}' not registered",
                protocol_name
            ))
        })?;

        // Validate participant count
        if participants.len() < protocol.requirements.min_participants {
            return Err(MpstError::protocol_analysis_error(format!(
                "Not enough participants for protocol '{}': {} < {}",
                protocol_name,
                participants.len(),
                protocol.requirements.min_participants
            )));
        }

        if let Some(max) = protocol.requirements.max_participants {
            if participants.len() > max {
                return Err(MpstError::protocol_analysis_error(format!(
                    "Too many participants for protocol '{}': {} > {}",
                    protocol_name,
                    participants.len(),
                    max
                )));
            }
        }

        let context = ExecutionContext::new(protocol_name, participants);
        self.active_contexts
            .insert(context.session_id, context.clone());

        Ok(context)
    }

    /// Add global capability guard
    pub fn add_global_guard(&mut self, name: impl Into<String>, guard: CapabilityGuard) {
        self.global_guards.insert(name.into(), guard);
    }

    /// Add global journal annotation
    pub fn add_global_annotation(
        &mut self,
        name: impl Into<String>,
        annotation: JournalAnnotation,
    ) {
        self.global_annotations.insert(name.into(), annotation);
    }

    /// Configure privacy settings
    pub fn configure_privacy(&mut self, settings: PrivacySettings) {
        self.privacy_settings = settings;
    }

    /// Validate protocol execution
    pub fn validate_execution(
        &self,
        runtime: &AuraRuntime,
        context: &ExecutionContext,
    ) -> MpstResult<()> {
        // Get protocol definition
        let protocol = self.get_protocol(&context.protocol_name).ok_or_else(|| {
            MpstError::protocol_analysis_error(format!(
                "Unknown protocol: {}",
                context.protocol_name
            ))
        })?;

        // Validate requirements
        protocol.requirements.validate(runtime, context)?;

        // Apply global guards
        for (name, guard) in &self.global_guards {
            guard.enforce(runtime.capabilities()).map_err(|_| {
                MpstError::capability_guard_failed(format!("Global guard '{}' failed", name))
            })?;
        }

        // Privacy validation would be implemented here
        // Note: This is a placeholder for actual context isolation validation
        if self.privacy_settings.enforce_context_isolation {
            // TODO: Implement actual context isolation validation
            // runtime.context_isolation().validate()
        }

        Ok(())
    }

    /// Get active contexts
    pub fn active_contexts(&self) -> &HashMap<uuid::Uuid, ExecutionContext> {
        &self.active_contexts
    }

    /// Complete execution context
    pub fn complete_context(&mut self, session_id: uuid::Uuid) {
        self.active_contexts.remove(&session_id);
    }

    /// Get privacy settings
    pub fn privacy_settings(&self) -> &PrivacySettings {
        &self.privacy_settings
    }

    /// List registered protocols
    pub fn list_protocols(&self) -> Vec<&ProtocolDefinition> {
        self.protocols.values().collect()
    }
}

impl Default for ProtocolInfrastructure {
    fn default() -> Self {
        Self::new()
    }
}

/// Protocol coordinator for managing multi-device execution
#[derive(Debug)]
pub struct ProtocolCoordinator {
    /// Infrastructure reference
    infrastructure: ProtocolInfrastructure,
    /// Device-specific runtimes
    runtimes: HashMap<DeviceId, AuraRuntime>,
}

impl ProtocolCoordinator {
    /// Create new protocol coordinator
    pub fn new(infrastructure: ProtocolInfrastructure) -> Self {
        Self {
            infrastructure,
            runtimes: HashMap::new(),
        }
    }

    /// Add device runtime
    pub fn add_runtime(&mut self, device_id: DeviceId, runtime: AuraRuntime) {
        self.runtimes.insert(device_id, runtime);
    }

    /// Get runtime for device
    pub fn get_runtime(&self, device_id: DeviceId) -> Option<&AuraRuntime> {
        self.runtimes.get(&device_id)
    }

    /// Get mutable runtime for device
    pub fn get_runtime_mut(&mut self, device_id: DeviceId) -> Option<&mut AuraRuntime> {
        self.runtimes.get_mut(&device_id)
    }

    /// Coordinate protocol execution across devices
    pub async fn coordinate_protocol(
        &mut self,
        protocol_name: &str,
        participants: Vec<DeviceId>,
    ) -> MpstResult<ExecutionContext> {
        // Create execution context
        let context = self
            .infrastructure
            .create_context(protocol_name, participants.clone())?;

        // Validate all participant runtimes
        for device_id in &participants {
            if let Some(runtime) = self.get_runtime(*device_id) {
                self.infrastructure.validate_execution(runtime, &context)?;
            } else {
                return Err(MpstError::protocol_analysis_error(format!(
                    "No runtime found for device {}",
                    device_id
                )));
            }
        }

        tracing::info!(
            "Coordinated protocol execution: {} with {} participants",
            protocol_name,
            participants.len()
        );

        Ok(context)
    }

    /// Complete coordinated execution
    pub fn complete_execution(&mut self, context: &ExecutionContext) {
        self.infrastructure.complete_context(context.session_id);
        tracing::info!("Completed protocol execution: {}", context.protocol_name);
    }

    /// Get infrastructure
    pub fn infrastructure(&self) -> &ProtocolInfrastructure {
        &self.infrastructure
    }

    /// Get mutable infrastructure  
    pub fn infrastructure_mut(&mut self) -> &mut ProtocolInfrastructure {
        &mut self.infrastructure
    }
}

/// Choreography execution framework
#[async_trait]
pub trait ChoreographyFramework {
    /// Execute a choreography with the given context and runtime
    async fn execute_choreography(
        &mut self,
        runtime: &mut AuraRuntime,
        context: &ExecutionContext,
        coordinator: &mut ProtocolCoordinator,
    ) -> MpstResult<()>;

    /// Validate choreography before execution
    fn validate_choreography(&self, runtime: &AuraRuntime) -> MpstResult<()>;

    /// Get choreography metadata
    fn metadata(&self) -> ChoreographyMetadata;
}

/// Choreography metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChoreographyMetadata {
    /// Choreography name
    pub name: String,
    /// Expected participants
    pub participants: Vec<String>,
    /// Guard requirements
    pub guard_requirements: Vec<String>,
    /// Journal annotations used
    pub journal_annotations: Vec<String>,
    /// Privacy leakage points
    pub leakage_points: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::DeviceId;

    #[test]
    fn test_protocol_infrastructure_creation() {
        let infrastructure = ProtocolInfrastructure::new();
        assert_eq!(infrastructure.protocols.len(), 0);
        assert_eq!(infrastructure.active_contexts.len(), 0);
    }

    #[test]
    fn test_protocol_registration() {
        let mut infrastructure = ProtocolInfrastructure::new();

        let definition = ProtocolDefinition {
            name: "test_protocol".to_string(),
            version: "1.0.0".to_string(),
            requirements: ProtocolRequirements::new().participants(2, Some(4)),
            description: Some("Test protocol".to_string()),
            choreography_source: None,
        };

        assert!(infrastructure.register_protocol(definition).is_ok());
        assert!(infrastructure.get_protocol("test_protocol").is_some());
    }

    #[test]
    fn test_execution_context_creation() {
        let mut infrastructure = ProtocolInfrastructure::new();

        let definition = ProtocolDefinition {
            name: "test_protocol".to_string(),
            version: "1.0.0".to_string(),
            requirements: ProtocolRequirements::new().participants(1, None),
            description: None,
            choreography_source: None,
        };

        infrastructure.register_protocol(definition).unwrap();

        let participants = vec![DeviceId::new()];
        let context = infrastructure
            .create_context("test_protocol", participants)
            .unwrap();

        assert_eq!(context.protocol_name, "test_protocol");
        assert_eq!(context.participants.len(), 1);
    }

    #[test]
    fn test_privacy_settings() {
        let settings = PrivacySettings::default();
        assert!(settings.enforce_context_isolation);
        assert!(settings.enforce_leakage_budgets);
        assert_eq!(settings.default_daily_budget, 10_000);
    }

    #[test]
    fn test_protocol_coordinator() {
        let infrastructure = ProtocolInfrastructure::new();
        let mut coordinator = ProtocolCoordinator::new(infrastructure);

        let device_id = DeviceId::new();
        let runtime = AuraRuntime::new(device_id, Cap::top(), Journal::new());

        coordinator.add_runtime(device_id, runtime);
        assert!(coordinator.get_runtime(device_id).is_some());
    }
}

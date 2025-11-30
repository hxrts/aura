//! Effect system registry and builder infrastructure
//!
//! Provides builder pattern infrastructure for assembling effect systems
//! and managing protocol requirements in the authority-centric architecture.

use aura_core::effects::ExecutionMode;
use aura_core::identifiers::AuthorityId;
use aura_core::AuraError;
use std::collections::{HashMap, HashSet};

/// Builder for assembling effect systems
#[derive(Debug)]
#[allow(dead_code)] // Part of future effect system API
pub struct EffectBuilder {
    authority_id: AuthorityId,
    execution_mode: ExecutionMode,
    bundles: Vec<EffectBundle>,
    requirements: ProtocolRequirements,
}

impl EffectBuilder {
    /// Create a new effect builder for the given authority
    #[allow(dead_code)] // Part of future effect system API
    pub fn new(authority_id: AuthorityId, execution_mode: ExecutionMode) -> Self {
        Self {
            authority_id,
            execution_mode,
            bundles: Vec::new(),
            requirements: ProtocolRequirements::new(),
        }
    }

    /// Add an effect bundle
    #[allow(dead_code)] // Part of future effect system API
    pub fn with_bundle(mut self, bundle: EffectBundle) -> Self {
        self.bundles.push(bundle);
        self
    }

    /// Add protocol requirements
    #[allow(dead_code)] // Part of future effect system API
    pub fn with_requirements(mut self, requirements: ProtocolRequirements) -> Self {
        self.requirements.merge(requirements);
        self
    }

    /// Build the effect system
    #[allow(dead_code)] // Part of future effect system API
    pub fn build(self) -> Result<super::AuraEffectSystem, AuraError> {
        // Validate requirements are met by bundles
        self.validate_requirements()?;

        let config = crate::core::AgentConfig::default();
        super::AuraEffectSystem::new(config).map_err(|e| AuraError::agent(e.to_string()))
    }

    #[allow(dead_code)] // Part of future effect system API
    fn validate_requirements(&self) -> Result<(), AuraError> {
        // Requirements enforcement deferred until ProtocolRequirements is populated by callers.
        Ok(())
    }
}

/// Bundle of related effects for a specific domain
#[derive(Debug, Clone)]
#[allow(dead_code)] // Part of future effect system API
pub struct EffectBundle {
    pub name: String,
    pub effects: Vec<String>,
    pub dependencies: Vec<String>,
}

impl EffectBundle {
    /// Create a new effect bundle
    #[allow(dead_code)] // Part of future effect system API
    pub fn new(name: String) -> Self {
        Self {
            name,
            effects: Vec::new(),
            dependencies: Vec::new(),
        }
    }

    /// Add an effect to the bundle
    #[allow(dead_code)] // Part of future effect system API
    pub fn with_effect(mut self, effect: String) -> Self {
        self.effects.push(effect);
        self
    }

    /// Add a dependency to the bundle
    #[allow(dead_code)] // Part of future effect system API
    pub fn with_dependency(mut self, dependency: String) -> Self {
        self.dependencies.push(dependency);
        self
    }
}

/// Protocol requirements specification
#[derive(Debug, Clone)]
#[allow(dead_code)] // Part of future effect system API
pub struct ProtocolRequirements {
    pub required_effects: HashSet<String>,
    pub optional_effects: HashSet<String>,
    pub runtime_constraints: HashMap<String, String>,
}

impl ProtocolRequirements {
    /// Create new empty requirements
    #[allow(dead_code)] // Part of future effect system API
    pub fn new() -> Self {
        Self {
            required_effects: HashSet::new(),
            optional_effects: HashSet::new(),
            runtime_constraints: HashMap::new(),
        }
    }

    /// Add a required effect
    #[allow(dead_code)] // Part of future effect system API
    pub fn require_effect(mut self, effect: String) -> Self {
        self.required_effects.insert(effect);
        self
    }

    /// Add an optional effect
    #[allow(dead_code)] // Part of future effect system API
    pub fn optional_effect(mut self, effect: String) -> Self {
        self.optional_effects.insert(effect);
        self
    }

    /// Add a runtime constraint
    #[allow(dead_code)] // Part of future effect system API
    pub fn with_constraint(mut self, name: String, value: String) -> Self {
        self.runtime_constraints.insert(name, value);
        self
    }

    /// Merge another requirements set
    #[allow(dead_code)] // Part of future effect system API
    pub fn merge(&mut self, other: ProtocolRequirements) {
        self.required_effects.extend(other.required_effects);
        self.optional_effects.extend(other.optional_effects);
        self.runtime_constraints.extend(other.runtime_constraints);
    }
}

impl Default for ProtocolRequirements {
    fn default() -> Self {
        Self::new()
    }
}

/// Quick builder for common effect system configurations
#[allow(dead_code)] // Part of future effect system API
pub struct QuickBuilder;

impl QuickBuilder {
    /// Build a production effect system
    #[allow(dead_code)] // Part of future effect system API
    pub fn production(authority_id: AuthorityId) -> EffectBuilder {
        EffectBuilder::new(authority_id, ExecutionMode::Production)
            .with_bundle(
                EffectBundle::new("crypto".to_string())
                    .with_effect("frost_keygen".to_string())
                    .with_effect("frost_signing".to_string()),
            )
            .with_bundle(
                EffectBundle::new("storage".to_string())
                    .with_effect("read".to_string())
                    .with_effect("write".to_string()),
            )
            .with_bundle(
                EffectBundle::new("transport".to_string())
                    .with_effect("send".to_string())
                    .with_effect("receive".to_string()),
            )
    }

    /// Build a testing effect system
    #[allow(dead_code)] // Part of future effect system API
    pub fn testing(authority_id: AuthorityId) -> EffectBuilder {
        EffectBuilder::new(authority_id, ExecutionMode::Testing)
            .with_bundle(EffectBundle::new("mock_crypto".to_string()))
            .with_bundle(EffectBundle::new("mock_storage".to_string()))
            .with_bundle(EffectBundle::new("mock_transport".to_string()))
    }

    /// Build a simulation effect system
    #[allow(dead_code)] // Part of future effect system API
    pub fn simulation(authority_id: AuthorityId, seed: u64) -> EffectBuilder {
        EffectBuilder::new(authority_id, ExecutionMode::Simulation { seed })
            .with_bundle(EffectBundle::new("sim_crypto".to_string()))
            .with_bundle(EffectBundle::new("sim_storage".to_string()))
            .with_bundle(EffectBundle::new("sim_transport".to_string()))
    }
}

//! Aura-specific extension effects for rumpsteak-aura integration
//!
//! This module provides extension types that implement rumpsteak's ExtensionEffect
//! trait to integrate Aura's capability guards, flow cost management, and journal
//! coupling into choreographic protocols.

use crate::ids::RoleId;
use rumpsteak_aura_choreography::effects::ExtensionEffect;
use serde::{Deserialize, Serialize};
use std::any::{Any, TypeId};

/// Extension for validating capabilities before protocol operations
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidateCapability {
    /// The capability string to validate
    pub capability: String,
    /// Role name as string to avoid generic conflicts
    pub role: RoleId,
}

impl ExtensionEffect for ValidateCapability {
    fn type_id(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn type_name(&self) -> &'static str {
        "ValidateCapability"
    }

    fn participating_role_ids(&self) -> Vec<Box<dyn Any>> {
        // Role-specific extension - only this role participates
        vec![Box::new(self.role.clone())]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn clone_box(&self) -> Box<dyn ExtensionEffect> {
        Box::new(self.clone())
    }
}

/// Extension for executing guard chain validations
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecuteGuardChain {
    /// List of guard strings to execute
    pub guards: Vec<String>,
    /// Role executing the guard chain
    pub role: RoleId,
    /// Operation being guarded
    pub operation: String,
}

impl ExtensionEffect for ExecuteGuardChain {
    fn type_id(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn type_name(&self) -> &'static str {
        "ExecuteGuardChain"
    }

    fn participating_role_ids(&self) -> Vec<Box<dyn Any>> {
        vec![Box::new(self.role.clone())]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn clone_box(&self) -> Box<dyn ExtensionEffect> {
        Box::new(self.clone())
    }
}

/// Extension for tracking flow costs in protocols
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChargeFlowCost {
    /// The flow cost amount to charge for this operation
    pub cost: u64,
    /// The specific operation being performed that incurs this cost
    pub operation: String,
    /// The role that is being charged for this operation
    pub role: RoleId,
}

impl ExtensionEffect for ChargeFlowCost {
    fn type_id(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn type_name(&self) -> &'static str {
        "ChargeFlowCost"
    }

    fn participating_role_ids(&self) -> Vec<Box<dyn Any>> {
        vec![Box::new(self.role.clone())]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn clone_box(&self) -> Box<dyn ExtensionEffect> {
        Box::new(self.clone())
    }
}

/// Extension for journal fact recording
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JournalFact {
    /// The fact to record in the journal
    pub fact: String,
    /// The role that is recording this fact
    pub role: RoleId,
    /// The operation that generated this fact
    pub operation: String,
}

impl ExtensionEffect for JournalFact {
    fn type_id(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn type_name(&self) -> &'static str {
        "JournalFact"
    }

    fn participating_role_ids(&self) -> Vec<Box<dyn Any>> {
        vec![Box::new(self.role.clone())]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn clone_box(&self) -> Box<dyn ExtensionEffect> {
        Box::new(self.clone())
    }
}

/// Extension for journal merge operations
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JournalMerge {
    /// The type of merge operation to perform (e.g., "anti_entropy", "consensus")
    pub merge_type: String,
    /// Multiple roles that can participate in the merge operation
    pub roles: Vec<RoleId>,
}

impl ExtensionEffect for JournalMerge {
    fn type_id(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn type_name(&self) -> &'static str {
        "JournalMerge"
    }

    fn participating_role_ids(&self) -> Vec<Box<dyn Any>> {
        // Multiple roles participate in merge operations
        self.roles
            .iter()
            .map(|r| Box::new(r.clone()) as Box<dyn Any>)
            .collect()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn clone_box(&self) -> Box<dyn ExtensionEffect> {
        Box::new(self.clone())
    }
}

/// Wrapper enum for concrete extension types
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ConcreteExtension {
    /// Capability validation extension
    ValidateCapability(ValidateCapability),
    /// Guard chain execution extension
    ExecuteGuardChain(ExecuteGuardChain),
    /// Flow cost tracking extension
    ChargeFlowCost(ChargeFlowCost),
    /// Journal fact recording extension
    JournalFact(JournalFact),
    /// Journal merge operation extension
    JournalMerge(JournalMerge),
}

/// Composite extension for handling multiple annotations on a single message
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompositeExtension {
    /// The list of concrete extensions to execute for this operation
    pub extensions: Vec<ConcreteExtension>,
    /// The primary role executing this composite extension
    pub role: RoleId,
    /// The operation that this composite extension is attached to
    pub operation: String,
}

impl ExtensionEffect for CompositeExtension {
    fn type_id(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    fn type_name(&self) -> &'static str {
        "CompositeExtension"
    }

    fn participating_role_ids(&self) -> Vec<Box<dyn Any>> {
        // Collect all participating roles from contained extensions
        let mut all_roles = vec![Box::new(self.role.clone()) as Box<dyn Any>];

        for ext in &self.extensions {
            match ext {
                ConcreteExtension::ValidateCapability(e) => {
                    all_roles.extend(e.participating_role_ids());
                }
                ConcreteExtension::ExecuteGuardChain(e) => {
                    all_roles.extend(e.participating_role_ids());
                }
                ConcreteExtension::ChargeFlowCost(e) => {
                    all_roles.extend(e.participating_role_ids());
                }
                ConcreteExtension::JournalFact(e) => {
                    all_roles.extend(e.participating_role_ids());
                }
                ConcreteExtension::JournalMerge(e) => {
                    all_roles.extend(e.participating_role_ids());
                }
            }
        }

        // Deduplicate roles (basic approach)
        all_roles.sort_by_key(|r| format!("{r:?}"));
        all_roles.dedup_by_key(|r| format!("{r:?}"));

        all_roles
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn clone_box(&self) -> Box<dyn ExtensionEffect> {
        Box::new(self.clone())
    }
}

impl CompositeExtension {
    /// Create a new composite extension
    pub fn new(role: RoleId, operation: String) -> Self {
        Self {
            extensions: Vec::new(),
            role,
            operation,
        }
    }

    /// Add an extension to the composite
    pub fn add_extension(mut self, extension: ConcreteExtension) -> Self {
        self.extensions.push(extension);
        self
    }

    /// Add a capability guard
    pub fn with_capability_guard(self, capability: String) -> Self {
        let ext = ValidateCapability {
            capability,
            role: self.role.clone(),
        };
        self.add_extension(ConcreteExtension::ValidateCapability(ext))
    }

    /// Add flow cost charging
    pub fn with_flow_cost(self, cost: u64) -> Self {
        let ext = ChargeFlowCost {
            cost,
            operation: self.operation.clone(),
            role: self.role.clone(),
        };
        self.add_extension(ConcreteExtension::ChargeFlowCost(ext))
    }

    /// Add journal fact recording
    pub fn with_journal_fact(self, fact: String) -> Self {
        let ext = JournalFact {
            fact,
            operation: self.operation.clone(),
            role: self.role.clone(),
        };
        self.add_extension(ConcreteExtension::JournalFact(ext))
    }

    /// Add guard chain execution
    pub fn with_guard_chain(self, guards: Vec<String>) -> Self {
        let ext = ExecuteGuardChain {
            guards,
            operation: self.operation.clone(),
            role: self.role.clone(),
        };
        self.add_extension(ConcreteExtension::ExecuteGuardChain(ext))
    }

    /// Get all contained extensions
    pub fn extensions(&self) -> &[ConcreteExtension] {
        &self.extensions
    }
}

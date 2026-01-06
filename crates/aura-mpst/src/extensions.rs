//! Aura-specific extension effects for rumpsteak-aura integration
//!
//! This module provides extension types that implement rumpsteak's ExtensionEffect
//! trait to integrate Aura's capability guards, flow cost management, and journal
//! coupling into choreographic protocols.

use crate::ids::RoleId;
use serde::{Deserialize, Serialize};

/// Extension for validating capabilities before protocol operations
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidateCapability {
    /// The capability string to validate
    pub capability: String,
    /// Role name as string to avoid generic conflicts
    pub role: RoleId,
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


/// Extension for journal merge operations
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JournalMerge {
    /// The type of merge operation to perform (e.g., "anti_entropy", "consensus")
    pub merge_type: String,
    /// Multiple roles that can participate in the merge operation
    pub roles: Vec<RoleId>,
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

// Note: Aura extensions are currently handled at compile-time in aura-macros.
// Runtime ExtensionEffect wiring will be reintroduced after MPST integration.

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

//! Aura-specific extension effects for rumpsteak-aura integration
//!
//! This module provides extension types that implement rumpsteak's ExtensionEffect
//! trait to integrate Aura's capability guards, flow cost management, and journal
//! coupling into choreographic protocols.

use rumpsteak_aura_choreography::effects::*;
use std::any::{Any, TypeId};
use serde::{Serialize, Deserialize};

/// Extension for validating capabilities before protocol operations
#[derive(Clone, Debug, Serialize, Deserialize)]
#[allow(dead_code)] // Used by generated macro code
pub struct ValidateCapability {
    pub capability: String,
    pub role: String, // Role name as string to avoid generic conflicts
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

/// Extension for tracking flow costs in protocols
#[derive(Clone, Debug, Serialize, Deserialize)]
#[allow(dead_code)] // Used by generated macro code
pub struct ChargeFlowCost {
    pub cost: u64,
    pub operation: String,
    pub role: String,
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
#[allow(dead_code)] // Used by generated macro code
pub struct JournalFact {
    pub fact: String,
    pub role: String,
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
#[allow(dead_code)] // Used by generated macro code
pub struct JournalMerge {
    pub merge_type: String,
    pub roles: Vec<String>, // Multiple roles can participate in merge
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
        self.roles.iter().map(|r| Box::new(r.clone()) as Box<dyn Any>).collect()
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
#[allow(dead_code)] // Used by generated macro code
pub struct ExecuteGuardChain {
    pub guards: Vec<String>,
    pub role: String,
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
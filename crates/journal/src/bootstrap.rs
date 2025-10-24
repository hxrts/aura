// Account bootstrap with capability-based authorization

use crate::capability::{
    events::CapabilityDelegation,
    identity::{IdentityCapabilityManager, IndividualId, ThresholdCapabilityAuth},
    types::CapabilityScope,
};
use crate::{AccountId, DeviceId, ThresholdSig};
use ed25519_dalek::Signature;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tracing::{info, debug};

/// Bootstrap configuration for new account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapConfig {
    /// Account identifier
    pub account_id: AccountId,
    /// Initial devices that will have root authority
    pub initial_devices: Vec<DeviceId>,
    /// Threshold configuration (M-of-N)
    pub threshold: u16,
    /// Total participants
    pub total_participants: u16,
    /// Root capability scopes to create
    pub root_scopes: Vec<CapabilityScope>,
}

impl BootstrapConfig {
    /// Create new bootstrap configuration
    pub fn new(
        account_id: AccountId,
        initial_devices: Vec<DeviceId>,
        threshold: u16,
    ) -> Self {
        let total_participants = initial_devices.len() as u16;
        
        // Define default root capability scopes
        let root_scopes = vec![
            CapabilityScope::simple("admin", "device"),      // Device management
            CapabilityScope::simple("admin", "guardian"),    // Guardian management  
            CapabilityScope::simple("capability", "delegate"), // Capability delegation
            CapabilityScope::simple("capability", "revoke"),   // Capability revocation
            CapabilityScope::simple("mls", "admin"),         // MLS group administration
            CapabilityScope::simple("storage", "admin"),     // Storage administration
        ];
        
        Self {
            account_id,
            initial_devices,
            threshold,
            total_participants,
            root_scopes,
        }
    }
    
    /// Add custom root scope
    pub fn add_root_scope(&mut self, scope: CapabilityScope) {
        self.root_scopes.push(scope);
    }
}

/// Account bootstrap result
#[derive(Debug, Clone)]
pub struct BootstrapResult {
    /// Genesis capability delegations
    pub genesis_delegations: Vec<CapabilityDelegation>,
    /// Identity mappings created
    pub identity_mappings: BTreeMap<DeviceId, IndividualId>,
    /// Root authorities established
    pub root_authorities: Vec<(DeviceId, Vec<CapabilityScope>)>,
}

/// Bootstrap manager for capability-based accounts
pub struct BootstrapManager {
    /// Identity-capability manager
    identity_manager: IdentityCapabilityManager,
}

impl BootstrapManager {
    /// Create new bootstrap manager
    pub fn new() -> Self {
        Self {
            identity_manager: IdentityCapabilityManager::new(),
        }
    }
    
    /// Bootstrap new account with capability-based authorization
    pub fn bootstrap_account(&mut self, config: BootstrapConfig, effects: &aura_crypto::Effects) -> crate::Result<BootstrapResult> {
        info!("Bootstrapping account {} with {} devices (threshold {}/{})", 
              config.account_id.0, config.initial_devices.len(), 
              config.threshold, config.total_participants);
        
        let mut result = BootstrapResult {
            genesis_delegations: Vec::new(),
            identity_mappings: BTreeMap::new(),
            root_authorities: Vec::new(),
        };
        
        // Create threshold signature for genesis operations
        let genesis_threshold_sig = self.create_genesis_threshold_signature(&config)?;
        
        // Bootstrap each device with root authorities
        for device_id in &config.initial_devices {
            let (delegations, individual_id) = self.bootstrap_device(
                *device_id,
                &config,
                &genesis_threshold_sig,
                effects,
            )?;
            
            // Collect authorities for this device
            let scopes: Vec<_> = delegations.iter().map(|d| d.scope.clone()).collect();
            
            result.genesis_delegations.extend(delegations);
            result.identity_mappings.insert(*device_id, individual_id);
            result.root_authorities.push((*device_id, scopes));
        }
        
        info!("Bootstrap complete: created {} genesis delegations for {} devices",
              result.genesis_delegations.len(), config.initial_devices.len());
        
        Ok(result)
    }
    
    /// Bootstrap individual device with root capabilities
    fn bootstrap_device(
        &mut self,
        device_id: DeviceId,
        config: &BootstrapConfig,
        threshold_sig: &ThresholdSig,
        effects: &aura_crypto::Effects,
    ) -> crate::Result<(Vec<CapabilityDelegation>, IndividualId)> {
        debug!("Bootstrapping device: {}", device_id.0);
        
        // Create individual identity for device
        let individual_id = IndividualId::from_device(&device_id);
        
        // Register identity mapping
        self.identity_manager.register_device_identity(device_id, individual_id.clone());
        
        let mut delegations = Vec::new();
        
        // Create genesis delegation for each root scope
        for scope in &config.root_scopes {
            let operation_hash = self.compute_genesis_operation_hash(
                &config.account_id,
                &individual_id,
                scope,
            );
            
            let threshold_auth = ThresholdCapabilityAuth::new(
                operation_hash,
                threshold_sig.clone(),
                scope.clone(),
                individual_id.clone(),
            );
            
            let delegation = self.identity_manager.create_genesis_delegation(
                individual_id.to_subject(),
                scope.clone(),
                threshold_auth,
                device_id,
                effects,
            )?;
            
            delegations.push(delegation);
        }
        
        debug!("Created {} genesis delegations for device {}", 
               delegations.len(), device_id.0);
        
        Ok((delegations, individual_id))
    }
    
    /// Create threshold signature for genesis operations
    fn create_genesis_threshold_signature(&self, config: &BootstrapConfig) -> crate::Result<ThresholdSig> {
        // In production, this would involve actual threshold signature ceremony
        // For now, create a placeholder signature
        
        let dummy_signature = Signature::from_bytes(&[0u8; 64]);
        let signers: Vec<u8> = (0..config.initial_devices.len() as u8).collect();
        
        // Create signature shares for each signer
        let signature_shares: Vec<Vec<u8>> = signers.iter().map(|_| {
            dummy_signature.to_bytes().to_vec()
        }).collect();
        
        Ok(ThresholdSig {
            signature: dummy_signature,
            signers,
            signature_shares,
        })
    }
    
    /// Compute deterministic hash for genesis operation
    fn compute_genesis_operation_hash(
        &self,
        account_id: &AccountId,
        individual_id: &IndividualId,
        scope: &CapabilityScope,
    ) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"genesis:");
        hasher.update(account_id.0.as_bytes());
        hasher.update(b":");
        hasher.update(individual_id.0.as_bytes());
        hasher.update(b":");
        hasher.update(scope.namespace.as_bytes());
        hasher.update(b":");
        hasher.update(scope.operation.as_bytes());
        
        if let Some(resource) = &scope.resource {
            hasher.update(b":");
            hasher.update(resource.as_bytes());
        }
        
        *hasher.finalize().as_bytes()
    }
    
    /// Create MLS group with capability-driven membership
    pub fn create_mls_group(
        &mut self,
        group_id: &str,
        initial_members: Vec<IndividualId>,
        effects: &aura_crypto::Effects,
    ) -> crate::Result<Vec<CapabilityDelegation>> {
        info!("Creating MLS group '{}' with {} initial members", 
              group_id, initial_members.len());
        
        let mut delegations = Vec::new();
        let mls_scope = CapabilityScope::with_resource("mls", "member", group_id);
        
        // Create dummy threshold signature for MLS group creation
        let threshold_sig = ThresholdSig {
            signature: Signature::from_bytes(&[0u8; 64]),
            signers: vec![0],
            signature_shares: vec![Signature::from_bytes(&[0u8; 64]).to_bytes().to_vec()],
        };
        
        for individual_id in initial_members {
            let operation_hash = self.compute_mls_operation_hash(group_id, &individual_id);
            
            let threshold_auth = ThresholdCapabilityAuth::new(
                operation_hash,
                threshold_sig.clone(),
                mls_scope.clone(),
                individual_id.clone(),
            );
            
            let delegation = self.identity_manager.create_genesis_delegation(
                individual_id.to_subject(),
                mls_scope.clone(),
                threshold_auth,
                DeviceId::from_string_with_effects("mls-bootstrap", effects),
                effects,
            )?;
            
            delegations.push(delegation);
        }
        
        info!("Created MLS group with {} member capabilities", delegations.len());
        
        Ok(delegations)
    }
    
    /// Compute hash for MLS group operation
    fn compute_mls_operation_hash(&self, group_id: &str, individual_id: &IndividualId) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"mls-member:");
        hasher.update(group_id.as_bytes());
        hasher.update(b":");
        hasher.update(individual_id.0.as_bytes());
        *hasher.finalize().as_bytes()
    }
    
    /// Get identity manager (for integration with other systems)
    pub fn identity_manager(&self) -> &IdentityCapabilityManager {
        &self.identity_manager
    }
    
    /// Get mutable identity manager
    pub fn identity_manager_mut(&mut self) -> &mut IdentityCapabilityManager {
        &mut self.identity_manager
    }
}

impl Default for BootstrapManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Utility functions for bootstrap validation
pub mod validation {
    use super::*;
    
    /// Validate bootstrap configuration
    pub fn validate_bootstrap_config(config: &BootstrapConfig) -> Result<(), String> {
        if config.initial_devices.is_empty() {
            return Err("Bootstrap configuration must have at least one device".to_string());
        }
        
        if config.threshold == 0 {
            return Err("Threshold must be greater than 0".to_string());
        }
        
        if config.threshold > config.total_participants {
            return Err("Threshold cannot exceed total participants".to_string());
        }
        
        if config.total_participants as usize != config.initial_devices.len() {
            return Err("Total participants must match number of initial devices".to_string());
        }
        
        // Check for duplicate devices
        let mut unique_devices = std::collections::BTreeSet::new();
        for device in &config.initial_devices {
            if !unique_devices.insert(device) {
                return Err(format!("Duplicate device in configuration: {}", device.0));
            }
        }
        
        Ok(())
    }
    
    /// Validate bootstrap result
    pub fn validate_bootstrap_result(
        config: &BootstrapConfig,
        result: &BootstrapResult,
    ) -> Result<(), String> {
        // Check that we have delegations for all devices
        if result.identity_mappings.len() != config.initial_devices.len() {
            return Err("Identity mappings count mismatch".to_string());
        }
        
        // Check that all devices have root authorities
        for device_id in &config.initial_devices {
            if !result.identity_mappings.contains_key(device_id) {
                return Err(format!("Missing identity mapping for device: {}", device_id.0));
            }
        }
        
        // Validate genesis delegations
        let expected_delegations = config.initial_devices.len() * config.root_scopes.len();
        if result.genesis_delegations.len() != expected_delegations {
            return Err(format!(
                "Expected {} genesis delegations, got {}",
                expected_delegations,
                result.genesis_delegations.len()
            ));
        }
        
        Ok(())
    }
}
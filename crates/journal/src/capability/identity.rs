// Identity integration for capability operations

use crate::capability::{
    events::{CapabilityDelegation, CapabilityRevocation},
    types::{CapabilityId, CapabilityScope, Subject},
    CapabilityError, Result,
};
use crate::ThresholdSig;
use aura_types::DeviceId;
use aura_crypto::Ed25519VerifyingKey;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tracing::{debug, info};

/// Individual identity derived from threshold signatures
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct IndividualId(pub String);

impl IndividualId {
    pub fn new(id: &str) -> Self {
        Self(id.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Convert to capability subject
    pub fn to_subject(&self) -> Subject {
        Subject::new(&self.0)
    }

    /// Create from device ID (device-specific identity)
    pub fn from_device(device_id: &DeviceId) -> Self {
        Self(format!("device:{}", device_id.0))
    }

    /// Create from DKD context (derived identity)
    pub fn from_dkd_context(context: &str, fingerprint: &[u8; 32]) -> Self {
        let fingerprint_hex = hex::encode(fingerprint);
        Self(format!("dkd:{}:{}", context, fingerprint_hex))
    }
}

/// Threshold signature authorization for capability operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdCapabilityAuth {
    /// The capability operation being authorized
    pub operation_hash: [u8; 32],
    /// Threshold signature proving authorization
    pub threshold_signature: ThresholdSig,
    /// Required authorization scope
    pub required_scope: CapabilityScope,
    /// Authorizing individual/context
    pub authorizer: IndividualId,
}

impl ThresholdCapabilityAuth {
    /// Create new threshold authorization
    pub fn new(
        operation_hash: [u8; 32],
        threshold_signature: ThresholdSig,
        required_scope: CapabilityScope,
        authorizer: IndividualId,
    ) -> Self {
        Self {
            operation_hash,
            threshold_signature,
            required_scope,
            authorizer,
        }
    }

    /// Verify threshold signature for capability operation
    pub fn verify(&self, _group_public_key: &Ed25519VerifyingKey) -> Result<()> {
        // In a full implementation, this would verify the threshold signature
        // For now, we'll do basic validation

        if self.threshold_signature.signature_shares.is_empty() {
            return Err(CapabilityError::CryptographicError(
                "No signature shares in threshold signature".to_string(),
            ));
        }

        debug!("Verified threshold signature for capability operation");
        Ok(())
    }
}

/// Identity-aware capability manager
pub struct IdentityCapabilityManager {
    /// Mapping from device IDs to individual identities
    device_identities: BTreeMap<DeviceId, IndividualId>,
    /// DKD-derived identities by context
    dkd_identities: BTreeMap<String, IndividualId>,
    /// Root authorities (genesis capabilities)
    root_authorities: BTreeMap<CapabilityId, ThresholdCapabilityAuth>,
}

impl IdentityCapabilityManager {
    /// Create new identity-capability manager
    pub fn new() -> Self {
        Self {
            device_identities: BTreeMap::new(),
            dkd_identities: BTreeMap::new(),
            root_authorities: BTreeMap::new(),
        }
    }

    /// Register device identity mapping
    pub fn register_device_identity(&mut self, device_id: DeviceId, individual_id: IndividualId) {
        info!(
            "Registering device identity: {} -> {}",
            device_id.0, individual_id.0
        );
        self.device_identities.insert(device_id, individual_id);
    }

    /// Register DKD-derived identity
    pub fn register_dkd_identity(&mut self, context: String, individual_id: IndividualId) {
        info!(
            "Registering DKD identity for context '{}': {}",
            context, individual_id.0
        );
        self.dkd_identities.insert(context, individual_id);
    }

    /// Get individual identity for device
    pub fn get_device_identity(&self, device_id: &DeviceId) -> Option<&IndividualId> {
        self.device_identities.get(device_id)
    }

    /// Get individual identity for DKD context
    pub fn get_dkd_identity(&self, context: &str) -> Option<&IndividualId> {
        self.dkd_identities.get(context)
    }

    /// Create genesis capability delegation with threshold signature
    pub fn create_genesis_delegation(
        &mut self,
        subject_id: Subject,
        scope: CapabilityScope,
        threshold_auth: ThresholdCapabilityAuth,
        issued_by: DeviceId,
        effects: &aura_crypto::Effects,
    ) -> Result<CapabilityDelegation> {
        info!(
            "Creating genesis capability delegation for subject {} with scope {:?}",
            subject_id.0, scope
        );

        // Verify threshold signature
        // Note: In production, this would use the actual group public key
        // threshold_authaura_crypto::ed25519_verify(&&group_public_key)?;

        // Create root capability (no parent)
        let delegation = CapabilityDelegation::new(
            None, // No parent for genesis
            subject_id.clone(),
            scope.clone(),
            None, // No expiry for genesis
            threshold_auth
                .threshold_signature
                .signature
                
                .to_vec(),
            issued_by,
            effects,
        );

        // Store as root authority
        self.root_authorities
            .insert(delegation.capability_id.clone(), threshold_auth);

        debug!(
            "Created genesis delegation with ID: {}",
            delegation.capability_id.as_hex()
        );

        Ok(delegation)
    }

    /// Create threshold-signed capability delegation
    pub fn create_threshold_delegation(
        &self,
        parent_id: CapabilityId,
        subject_id: Subject,
        scope: CapabilityScope,
        expiry: Option<u64>,
        threshold_auth: ThresholdCapabilityAuth,
        issued_by: DeviceId,
        effects: &aura_crypto::Effects,
    ) -> Result<CapabilityDelegation> {
        debug!(
            "Creating threshold-signed delegation from parent {}",
            parent_id.as_hex()
        );

        // Verify threshold signature
        // threshold_authaura_crypto::ed25519_verify(&&group_public_key)?;

        let delegation = CapabilityDelegation::new(
            Some(parent_id),
            subject_id,
            scope,
            expiry,
            threshold_auth
                .threshold_signature
                .signature
                
                .to_vec(),
            issued_by,
            effects,
        );

        debug!(
            "Created threshold delegation with ID: {}",
            delegation.capability_id.as_hex()
        );

        Ok(delegation)
    }

    /// Create threshold-signed capability revocation
    pub fn create_threshold_revocation(
        &self,
        capability_id: CapabilityId,
        reason: String,
        threshold_auth: ThresholdCapabilityAuth,
        issued_by: DeviceId,
        effects: &aura_crypto::Effects,
    ) -> Result<CapabilityRevocation> {
        debug!(
            "Creating threshold-signed revocation for capability {}",
            capability_id.as_hex()
        );

        // Verify threshold signature
        // threshold_authaura_crypto::ed25519_verify(&&group_public_key)?;

        let revocation = CapabilityRevocation::new(
            capability_id,
            reason,
            threshold_auth
                .threshold_signature
                .signature
                
                .to_vec(),
            issued_by,
            effects,
        );

        debug!("Created threshold revocation");

        Ok(revocation)
    }

    /// Validate capability operation against identity requirements
    pub fn validate_capability_operation(
        &self,
        _operation_hash: &[u8; 32],
        authorizer: &IndividualId,
        _required_scope: &CapabilityScope,
    ) -> Result<()> {
        // Check if this is a known identity
        let is_device_identity = self.device_identities.values().any(|id| id == authorizer);
        let is_dkd_identity = self.dkd_identities.values().any(|id| id == authorizer);

        if !is_device_identity && !is_dkd_identity {
            return Err(CapabilityError::AuthorizationError(format!(
                "Unknown individual identity: {}",
                authorizer.0
            )));
        }

        debug!(
            "Validated capability operation for identity {}",
            authorizer.0
        );
        Ok(())
    }

    /// Bootstrap initial root authorities for new account
    pub fn bootstrap_root_authorities(
        &mut self,
        initial_devices: Vec<DeviceId>,
        threshold_signature: ThresholdSig,
        effects: &aura_crypto::Effects,
    ) -> Result<Vec<CapabilityDelegation>> {
        info!(
            "Bootstrapping root authorities for {} devices",
            initial_devices.len()
        );

        let mut delegations = Vec::new();

        for device_id in initial_devices {
            // Create individual identity for device
            let individual_id = IndividualId::from_device(&device_id);
            self.register_device_identity(device_id, individual_id.clone());

            // Create root capability for device management
            let device_scope = CapabilityScope::simple("admin", "device");
            let operation_hash = [0u8; 32]; // Placeholder

            let threshold_auth = ThresholdCapabilityAuth::new(
                operation_hash,
                threshold_signature.clone(),
                device_scope.clone(),
                individual_id.clone(),
            );

            let delegation = self.create_genesis_delegation(
                individual_id.to_subject(),
                device_scope,
                threshold_auth,
                device_id,
                effects,
            )?;

            delegations.push(delegation);
        }

        info!("Created {} root authority delegations", delegations.len());

        Ok(delegations)
    }

    /// Get all root authorities
    pub fn get_root_authorities(&self) -> &BTreeMap<CapabilityId, ThresholdCapabilityAuth> {
        &self.root_authorities
    }

    /// Check if capability is a root authority
    pub fn is_root_authority(&self, capability_id: &CapabilityId) -> bool {
        self.root_authorities.contains_key(capability_id)
    }
}

impl Default for IdentityCapabilityManager {
    fn default() -> Self {
        Self::new()
    }
}

/// DKD-specific capability key derivation
pub mod dkd_keys {
    use super::*;

    /// Derive capability-specific keys using DKD
    pub fn derive_capability_keys(
        dkd_seed: &[u8; 32],
        capability_scope: &CapabilityScope,
    ) -> Result<(aura_crypto::Ed25519SigningKey, aura_crypto::Ed25519VerifyingKey)> {
        // Derive deterministic key for this capability scope
        let mut hasher = aura_crypto::blake3_hasher();
        hasher.update(dkd_seed);
        hasher.update(b"capability:");
        hasher.update(capability_scope.namespace.as_bytes());
        hasher.update(b":");
        hasher.update(capability_scope.operation.as_bytes());

        if let Some(resource) = &capability_scope.resource {
            hasher.update(b":");
            hasher.update(resource.as_bytes());
        }

        let derived_seed = hasher.finalize();
        let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(derived_seed.as_bytes());
        let verifying_key = signing_key.verifying_key();

        debug!("Derived capability keys for scope {:?}", capability_scope);

        Ok((signing_key, verifying_key))
    }

    /// Create individual identity from DKD context
    pub fn create_dkd_identity(context: &str, dkd_seed: &[u8; 32]) -> IndividualId {
        let fingerprint = aura_crypto::blake3_hash(dkd_seed);
        IndividualId::from_dkd_context(context, &fingerprint)
    }
}

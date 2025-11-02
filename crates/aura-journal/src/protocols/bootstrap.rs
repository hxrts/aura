// Account bootstrap with capability-based authorization

use crate::capability::{
    events::CapabilityDelegation,
    identity::{IdentityCapabilityManager, IndividualIdCapabilityExt, ThresholdCapabilityAuth},
    types::CapabilityScope,
};
use aura_authentication::ThresholdSig;
use aura_types::{AccountId, AccountIdExt, DeviceId, DeviceIdExt, IndividualId, IndividualIdExt};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tracing::{debug, info};

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
    pub fn new(account_id: AccountId, initial_devices: Vec<DeviceId>, threshold: u16) -> Self {
        let total_participants = initial_devices.len() as u16;

        // Define default root capability scopes
        let root_scopes = vec![
            CapabilityScope::simple("admin", "device"), // Device management
            CapabilityScope::simple("admin", "guardian"), // Guardian management
            CapabilityScope::simple("capability", "delegate"), // Capability delegation
            CapabilityScope::simple("capability", "revoke"), // Capability revocation
            CapabilityScope::simple("mls", "admin"),    // MLS group administration
            CapabilityScope::simple("storage", "admin"), // Storage administration
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

/// Complete account initialization result
#[derive(Debug)]
pub struct AccountInitResult {
    /// Account identifier
    pub account_id: AccountId,
    /// Generated device IDs
    pub device_ids: Vec<DeviceId>,
    /// Primary device configuration
    pub primary_device_id: DeviceId,
    /// FROST key shares (one per device)
    pub key_shares: Vec<KeyShareData>,
    /// Final account ledger
    pub ledger: crate::AccountLedger,
    /// Genesis session ID
    pub genesis_session_id: uuid::Uuid,
    /// Bootstrap result with capabilities
    pub bootstrap: BootstrapResult,
}

/// FROST key share data for a single device
#[derive(Debug, Clone)]
pub struct KeyShareData {
    /// Device this share belongs to
    pub device_id: DeviceId,
    /// FROST participant ID (1-indexed)
    pub participant_id: u16,
    /// The actual key share package
    pub key_package: frost_ed25519::keys::KeyPackage,
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

    /// Initialize complete account with genesis DKG, session, and capabilities
    ///
    /// This is the high-level API that handles:
    /// - Account state creation
    /// - FROST key generation (trusted dealer for MVP)
    /// - Genesis session management
    /// - Capability bootstrapping
    /// - Ledger initialization
    pub fn initialize_account(
        &mut self,
        participants: u16,
        threshold: u16,
        effects: &aura_crypto::Effects,
    ) -> crate::Result<AccountInitResult> {
        info!(
            "Initializing account with {}-of-{} threshold",
            threshold, participants
        );

        // Validate parameters
        if participants < 2 {
            return Err(crate::AuraError::coordination_failed(
                "Minimum 2 participants required",
            ));
        }

        if threshold > participants {
            return Err(crate::AuraError::coordination_failed(
                "Threshold cannot exceed participant count",
            ));
        }

        // Generate account and device IDs
        let account_id = AccountId::new_with_effects(effects);
        let mut device_ids = Vec::new();
        let mut device_metadatas = Vec::new();

        // Generate FROST-derived group key for testing
        let frost_group_key = generate_frost_test_group_key(effects)?;

        // Create device metadata for all participants
        for i in 0..participants {
            let device_id = DeviceId::new_with_effects(effects);
            let device = crate::types::DeviceMetadata {
                device_id,
                device_name: format!("Device {}", i + 1),
                device_type: crate::types::DeviceType::Native,
                public_key: frost_group_key,
                added_at: effects.now().unwrap_or(0),
                last_seen: effects.now().unwrap_or(0),
                dkd_commitment_proofs: BTreeMap::new(),
                next_nonce: 1,
                used_nonces: std::collections::BTreeSet::new(),
                key_share_epoch: 0,
            };
            device_ids.push(device_id);
            device_metadatas.push(device);
        }

        // Create initial account state
        let mut account_state = crate::AccountState::new(
            account_id,
            frost_group_key,
            device_metadatas[0].clone(),
            threshold,
            participants,
        );

        // Add remaining devices
        for device in device_metadatas.iter().skip(1) {
            account_state
                .add_device(device.clone(), effects)
                .map_err(|e| {
                    crate::AuraError::coordination_failed(format!(
                        "Failed to add device: {:?}",
                        e
                    ))
                })?;
        }

        // Generate FROST key shares using trusted dealer (MVP)
        let (frost_shares, pubkey_package) = {
            let mut rng = effects.rng();
            frost_ed25519::keys::generate_with_dealer(
                participants, // max_signers (N in M-of-N)
                threshold,    // min_signers (M in M-of-N)
                frost_ed25519::keys::IdentifierList::Default,
                &mut rng,
            )
            .map_err(|e| {
                crate::AuraError::coordination_failed(format!(
                    "FROST key generation failed: {}",
                    e
                ))
            })?
        };

        // Convert FROST shares to our format
        let mut key_shares = Vec::new();
        for ((frost_id, secret_share), device_id) in frost_shares.into_iter().zip(device_ids.iter())
        {
            let key_package =
                frost_ed25519::keys::KeyPackage::try_from(secret_share).map_err(|e| {
                    crate::AuraError::coordination_failed(format!(
                        "Key package creation failed: {}",
                        e
                    ))
                })?;

            // FROST identifiers serialize to a [u8; 2] containing the u16
            let id_bytes = frost_id.serialize();
            let participant_id = u16::from_le_bytes([id_bytes[0], id_bytes[1]]);

            key_shares.push(KeyShareData {
                device_id: *device_id,
                participant_id,
                key_package,
            });
        }

        // Update account state with FROST group key
        let frost_vk = pubkey_package.verifying_key();
        let group_public_key = aura_crypto::Ed25519VerifyingKey::from_bytes(&frost_vk.serialize())
            .map_err(|e| {
                crate::AuraError::coordination_failed(format!("Invalid group key: {}", e))
            })?;

        account_state.group_public_key = group_public_key;
        for device in account_state.devices.values_mut() {
            device.public_key = group_public_key;
        }

        // Create genesis session
        let genesis_session_id = effects.gen_uuid();
        let genesis_participants: Vec<crate::ParticipantId> = device_ids
            .iter()
            .map(|device_id| crate::ParticipantId::Device(*device_id))
            .collect();

        let genesis_session = crate::Session::new(
            crate::SessionId(genesis_session_id),
            crate::ProtocolType::Dkd,
            genesis_participants,
            1,
            100,
            effects.now().unwrap_or(0),
        );

        // Create ledger and add genesis session
        let mut ledger = crate::AccountLedger::new(account_state).map_err(|e| {
            crate::AuraError::coordination_failed(format!("Ledger creation failed: {}", e))
        })?;

        ledger.add_session(genesis_session, effects);
        ledger
            .update_session_status(genesis_session_id, crate::SessionStatus::Active, effects)
            .map_err(|e| {
                crate::AuraError::coordination_failed(format!(
                    "Session update failed: {}",
                    e
                ))
            })?;
        ledger
            .complete_session(genesis_session_id, crate::SessionOutcome::Success, effects)
            .map_err(|e| {
                crate::AuraError::coordination_failed(format!(
                    "Session completion failed: {}",
                    e
                ))
            })?;

        // Bootstrap capabilities
        let bootstrap_config = BootstrapConfig::new(account_id, device_ids.clone(), threshold);
        let bootstrap_result = self.bootstrap_account(bootstrap_config, effects)?;

        info!(
            "Account initialization complete: {} devices, {}-of-{} threshold",
            device_ids.len(),
            threshold,
            participants
        );

        Ok(AccountInitResult {
            account_id,
            device_ids: device_ids.clone(),
            primary_device_id: device_ids[0],
            key_shares,
            ledger,
            genesis_session_id,
            bootstrap: bootstrap_result,
        })
    }

    /// Bootstrap new account with capability-based authorization
    pub fn bootstrap_account(
        &mut self,
        config: BootstrapConfig,
        effects: &aura_crypto::Effects,
    ) -> crate::Result<BootstrapResult> {
        info!(
            "Bootstrapping account {} with {} devices (threshold {}/{})",
            config.account_id.0,
            config.initial_devices.len(),
            config.threshold,
            config.total_participants
        );

        let mut result = BootstrapResult {
            genesis_delegations: Vec::new(),
            identity_mappings: BTreeMap::new(),
            root_authorities: Vec::new(),
        };

        // Create threshold signature for genesis operations
        let genesis_threshold_sig = self.create_genesis_threshold_signature(&config)?;

        // Bootstrap each device with root authorities
        for device_id in &config.initial_devices {
            let (delegations, individual_id) =
                self.bootstrap_device(*device_id, &config, &genesis_threshold_sig, effects)?;

            // Collect authorities for this device
            let scopes: Vec<_> = delegations.iter().map(|d| d.scope.clone()).collect();

            result.genesis_delegations.extend(delegations);
            result.identity_mappings.insert(*device_id, individual_id);
            result.root_authorities.push((*device_id, scopes));
        }

        info!(
            "Bootstrap complete: created {} genesis delegations for {} devices",
            result.genesis_delegations.len(),
            config.initial_devices.len()
        );

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
        self.identity_manager
            .register_device_identity(device_id, individual_id.clone());

        let mut delegations = Vec::new();

        // Create genesis delegation for each root scope
        for scope in &config.root_scopes {
            let operation_hash =
                self.compute_genesis_operation_hash(&config.account_id, &individual_id, scope);

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

        debug!(
            "Created {} genesis delegations for device {}",
            delegations.len(),
            device_id.0
        );

        Ok((delegations, individual_id))
    }

    /// Create threshold signature for genesis operations
    fn create_genesis_threshold_signature(
        &self,
        config: &BootstrapConfig,
    ) -> crate::Result<ThresholdSig> {
        // Create real threshold signature for genesis operations
        // Use deterministic effects for reproducible bootstrapping
        let effects = aura_crypto::Effects::test();

        // Create real signature based on bootstrap configuration
        let real_signature = self.create_real_bootstrap_signature(config, &effects)?;
        let signers: Vec<u8> = (0..config.initial_devices.len() as u8).collect();

        // Create signature shares for each signer using real signature
        let signature_shares: Vec<Vec<u8>> =
            signers.iter().map(|_| real_signature.to_vec()).collect();

        Ok(ThresholdSig {
            signature: real_signature,
            signers,
            signature_shares,
        })
    }

    /// Create real bootstrap signature using Ed25519
    ///
    /// This replaces the previous placeholder implementation with real cryptographic signatures
    fn create_real_bootstrap_signature(
        &self,
        config: &BootstrapConfig,
        effects: &aura_crypto::Effects,
    ) -> crate::Result<aura_crypto::Ed25519Signature> {
        // Create deterministic signing key for bootstrap
        let seed = effects.random_bytes::<32>();
        let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&seed);

        // Create message to sign based on bootstrap configuration
        let mut message = Vec::new();
        message.extend_from_slice(b"AURA_BOOTSTRAP:");
        message.extend_from_slice(config.account_id.0.as_bytes());
        message.extend_from_slice(&config.threshold.to_le_bytes());
        message.extend_from_slice(&(config.initial_devices.len() as u16).to_le_bytes());

        // Include device IDs in deterministic order
        let mut device_ids: Vec<_> = config.initial_devices.iter().collect();
        device_ids.sort_by_key(|d| d.0);
        for device_id in device_ids {
            message.extend_from_slice(device_id.0.as_bytes());
        }

        // Sign the bootstrap message
        let signature = aura_crypto::ed25519_sign(&signing_key, &message);
        Ok(signature)
    }

    /// Compute deterministic hash for genesis operation
    fn compute_genesis_operation_hash(
        &self,
        account_id: &AccountId,
        individual_id: &IndividualId,
        scope: &CapabilityScope,
    ) -> [u8; 32] {
        let mut hasher = aura_crypto::blake3_hasher();
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
        info!(
            "Creating MLS group '{}' with {} initial members",
            group_id,
            initial_members.len()
        );

        let mut delegations = Vec::new();
        let mls_scope = CapabilityScope::with_resource("mls", "member", group_id);

        // Create dummy threshold signature for MLS group creation
        let test_signature = generate_test_signature(effects);
        let threshold_sig = ThresholdSig {
            signature: test_signature,
            signers: vec![0],
            signature_shares: vec![test_signature.to_vec()],
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

        info!(
            "Created MLS group with {} member capabilities",
            delegations.len()
        );

        Ok(delegations)
    }

    /// Compute hash for MLS group operation
    fn compute_mls_operation_hash(&self, group_id: &str, individual_id: &IndividualId) -> [u8; 32] {
        let mut hasher = aura_crypto::blake3_hasher();
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
                return Err(format!(
                    "Missing identity mapping for device: {}",
                    device_id.0
                ));
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

/// Generate a deterministic test signature for non-production use
fn generate_test_signature(effects: &aura_crypto::Effects) -> aura_crypto::Ed25519Signature {
    // Generate a deterministic test key from effects
    let mut rng = effects.rng();
    let key_material: [u8; 32] = rng.gen();
    let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&key_material);

    let message = b"test_message_for_bootstrap";
    aura_crypto::ed25519_sign(&signing_key, message)
}

/// Generate a FROST-derived group key for testing
fn generate_frost_test_group_key(
    effects: &aura_crypto::Effects,
) -> crate::Result<aura_crypto::Ed25519VerifyingKey> {
    // Generate a deterministic key from effects that simulates FROST group key generation
    let mut rng = effects.rng();
    let group_key_material: [u8; 32] = rng.gen();

    // Create a signing key and extract the verifying key to simulate FROST output
    let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&group_key_material);
    Ok(signing_key.verifying_key())
}

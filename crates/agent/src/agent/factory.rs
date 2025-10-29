//! Agent factory and construction
//!
//! This module provides factory methods for creating agents with various configurations.

use crate::agent::capabilities::KeyShare;
use crate::agent::core::AgentCore;
use crate::error::{AgentError, Result};
use crate::{Storage, Transport};
use aura_crypto::Effects;
use aura_journal::{AccountLedger, AccountState};
use aura_protocol::LocalSessionRuntime;
use aura_types::{AccountId, DeviceId};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Factory for creating and configuring agents
pub struct AgentFactory;

impl AgentFactory {
    /// Create a basic agent with provided dependencies
    ///
    /// This creates an agent with the minimal required components. The agent will need
    /// to be bootstrapped before it can perform operations.
    pub fn create_with_dependencies<T: Transport, S: Storage>(
        device_id: DeviceId,
        account_id: AccountId,
        transport: Arc<T>,
        storage: Arc<S>,
    ) -> Result<AgentCore<T, S>> {
        use aura_journal::types::DeviceMetadata;
        use aura_journal::DeviceType;

        // Create initial key share (will be properly initialized during bootstrap)
        let key_share = KeyShare {
            device_id,
            share_data: vec![],
        };

        // Create effects for initialization
        let effects = Effects::production();

        // Generate device signing key
        let device_signing_key = aura_crypto::generate_ed25519_key();
        let device_public_key = device_signing_key.verifying_key();

        // Create initial device metadata
        let initial_device = DeviceMetadata {
            device_id,
            device_name: "Primary Device".to_string(),
            device_type: DeviceType::Native,
            public_key: device_public_key,
            added_at: effects.now().unwrap_or(0),
            last_seen: effects.now().unwrap_or(0),
            dkd_commitment_proofs: Default::default(),
            next_nonce: 1,
            used_nonces: Default::default(),
        };

        // Create initial account state
        let initial_state = AccountState::new(
            account_id,
            device_public_key, // group_public_key
            initial_device,
            1, // threshold (will be updated during bootstrap)
            1, // total_participants (will be updated during bootstrap)
        );

        // Create account ledger
        let ledger = AccountLedger::new(initial_state).map_err(|e| {
            AgentError::agent_invalid_state(format!("Failed to create ledger: {}", e))
        })?;

        // Create session runtime with generated key
        let session_runtime =
            LocalSessionRuntime::new_with_generated_key(device_id, account_id, effects.clone());

        // Create agent core
        let agent = AgentCore::new(
            device_id,
            account_id,
            key_share,
            Arc::new(RwLock::new(ledger)),
            transport,
            storage,
            effects,
            Arc::new(RwLock::new(session_runtime)),
        );

        Ok(agent)
    }

    // TODO: Re-add test helper once MockTransport and MockStorage are implemented
    // pub async fn create_test(...) { ... }
}

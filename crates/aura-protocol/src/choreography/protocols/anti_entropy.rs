//! Anti-Entropy Choreography for State Synchronization
//!
//! This module implements choreographic protocols for digest-based
//! state reconciliation using the protocol guide design principles.
//!
//! ## Protocol Flow
//!
//! 1. Requester → Responder: Send digest request
//! 2. Responder → Requester: Send state digest (bloom filter)
//! 3. Requester → Responder: Request missing operations
//! 4. Responder → Requester: Send missing operations

use crate::choreography::common::ChoreographyError;
use crate::effects::{ConsoleEffects, CryptoEffects, RandomEffects};
use aura_core::{DeviceId, SessionId};
use rumpsteak_choreography::choreography;
use serde::{Deserialize, Serialize};

/// Anti-entropy choreography configuration
#[derive(Debug, Clone)]
pub struct AntiEntropyConfig {
    pub participants: Vec<DeviceId>,
    pub max_ops_per_sync: usize,
}

/// Anti-entropy choreography result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiEntropyResult {
    pub ops_sent: usize,
    pub ops_received: usize,
    pub success: bool,
}

/// Anti-entropy error types
#[derive(Debug, thiserror::Error)]
pub enum AntiEntropyError {
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Communication error: {0}")]
    Communication(String),
    #[error("Sync failed: {0}")]
    SyncFailed(String),
    #[error("Handler error: {0}")]
    Handler(#[from] crate::handlers::AuraHandlerError),
}

/// Message types for anti-entropy choreography

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DigestRequest {
    pub session_id: SessionId,
    pub requester_id: DeviceId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DigestResponse {
    pub session_id: SessionId,
    pub bloom_filter: Vec<u8>,
    pub operation_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissingOpsRequest {
    pub session_id: SessionId,
    pub missing_cids: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissingOpsResponse {
    pub session_id: SessionId,
    pub operations: Vec<Vec<u8>>,
    pub ops_count: usize,
}

/// Anti-entropy synchronization choreography
///
/// Two-party protocol for digest-based state reconciliation
choreography! {
    protocol AntiEntropy {
        roles: Requester, Responder;

        // Phase 1: Request digest
        Requester -> Responder: RequestDigest(DigestRequest);

        // Phase 2: Send digest
        Responder -> Requester: SendDigest(DigestResponse);

        // Phase 3: Request missing operations
        Requester -> Responder: RequestMissingOps(MissingOpsRequest);

        // Phase 4: Send missing operations
        Responder -> Requester: SendMissingOps(MissingOpsResponse);
    }
}

/// Execute anti-entropy sync protocol
pub async fn execute_anti_entropy(
    device_id: DeviceId,
    config: AntiEntropyConfig,
    is_requester: bool,
    effect_system: &crate::effects::system::AuraEffectSystem,
) -> Result<AntiEntropyResult, AntiEntropyError> {
    // Validate configuration
    if config.participants.len() != 2 {
        return Err(AntiEntropyError::InvalidConfig(format!(
            "Anti-entropy requires exactly 2 participants, got {}",
            config.participants.len()
        )));
    }

    // Create handler adapter
    let composite_handler = match effect_system.execution_mode() {
        crate::handlers::ExecutionMode::Testing => {
            crate::handlers::CompositeHandler::for_testing(device_id.into())
        }
        crate::handlers::ExecutionMode::Production => {
            crate::handlers::CompositeHandler::for_production(device_id.into())
        }
        crate::handlers::ExecutionMode::Simulation { seed: _ } => {
            crate::handlers::CompositeHandler::for_simulation(device_id.into())
        }
    };

    let mut adapter =
        crate::choreography::runtime::AuraHandlerAdapter::new(composite_handler, device_id);

    // Execute appropriate role
    if is_requester {
        let responder_id = config
            .participants
            .iter()
            .find(|&&id| id != device_id)
            .copied()
            .ok_or_else(|| AntiEntropyError::InvalidConfig("No responder found".to_string()))?;

        requester_session(&mut adapter, responder_id, &config).await
    } else {
        let requester_id = config
            .participants
            .iter()
            .find(|&&id| id != device_id)
            .copied()
            .ok_or_else(|| AntiEntropyError::InvalidConfig("No requester found".to_string()))?;

        responder_session(&mut adapter, requester_id, &config).await
    }
}

/// Requester's role in anti-entropy sync
async fn requester_session(
    adapter: &mut crate::choreography::runtime::AuraHandlerAdapter,
    responder_id: DeviceId,
    config: &AntiEntropyConfig,
) -> Result<AntiEntropyResult, AntiEntropyError> {
    let session_id = SessionId::new();

    // Phase 1: Request digest
    let digest_request = DigestRequest {
        session_id: session_id.clone(),
        requester_id: adapter.device_id(),
    };

    adapter
        .send(responder_id, digest_request)
        .await
        .map_err(|e| {
            AntiEntropyError::Communication(format!("Failed to send digest request: {}", e))
        })?;

    // Phase 2: Receive digest
    let digest_response: DigestResponse = adapter
        .recv_from(responder_id)
        .await
        .map_err(|e| AntiEntropyError::Communication(format!("Failed to receive digest: {}", e)))?;

    if digest_response.session_id != session_id {
        return Err(AntiEntropyError::SyncFailed(
            "Session ID mismatch".to_string(),
        ));
    }

    // Phase 3: Calculate missing operations (TODO fix - Simplified - use hash of bloom filter)
    let local_bloom = adapter.effects().random_bytes(32).await;
    let combined = [&local_bloom[..], &digest_response.bloom_filter[..]].concat();
    let diff_hash = adapter.effects().hash(&combined).await;

    // Simulate missing CIDs based on difference
    let missing_cids = vec![diff_hash.to_vec()];

    let missing_request = MissingOpsRequest {
        session_id: session_id.clone(),
        missing_cids,
    };

    adapter
        .send(responder_id, missing_request)
        .await
        .map_err(|e| {
            AntiEntropyError::Communication(format!("Failed to send missing ops request: {}", e))
        })?;

    // Phase 4: Receive missing operations
    let missing_response: MissingOpsResponse =
        adapter.recv_from(responder_id).await.map_err(|e| {
            AntiEntropyError::Communication(format!("Failed to receive missing ops: {}", e))
        })?;

    Ok(AntiEntropyResult {
        ops_sent: 0,
        ops_received: missing_response.ops_count,
        success: true,
    })
}

/// Responder's role in anti-entropy sync
async fn responder_session(
    adapter: &mut crate::choreography::runtime::AuraHandlerAdapter,
    requester_id: DeviceId,
    _config: &AntiEntropyConfig,
) -> Result<AntiEntropyResult, AntiEntropyError> {
    // Phase 1: Receive digest request
    let digest_request: DigestRequest = adapter.recv_from(requester_id).await.map_err(|e| {
        AntiEntropyError::Communication(format!("Failed to receive digest request: {}", e))
    })?;

    // Phase 2: Generate and send digest
    let bloom_filter = adapter.effects().random_bytes(32).await;

    let digest_response = DigestResponse {
        session_id: digest_request.session_id.clone(),
        bloom_filter: bloom_filter.to_vec(),
        operation_count: 10, // TODO fix - Simplified
    };

    adapter
        .send(requester_id, digest_response)
        .await
        .map_err(|e| AntiEntropyError::Communication(format!("Failed to send digest: {}", e)))?;

    // Phase 3: Receive missing ops request
    let missing_request: MissingOpsRequest =
        adapter.recv_from(requester_id).await.map_err(|e| {
            AntiEntropyError::Communication(format!("Failed to receive missing ops request: {}", e))
        })?;

    // Phase 4: Send missing operations
    let operations: Vec<Vec<u8>> = missing_request
        .missing_cids
        .iter()
        .map(|cid| {
            // TODO fix - Simplified: generate operation data based on CID
            let op_data = [cid.as_slice(), b"_operation_data"].concat();
            op_data
        })
        .collect();

    let ops_count = operations.len();

    let missing_response = MissingOpsResponse {
        session_id: missing_request.session_id,
        operations,
        ops_count,
    };

    adapter
        .send(requester_id, missing_response)
        .await
        .map_err(|e| {
            AntiEntropyError::Communication(format!("Failed to send missing ops: {}", e))
        })?;

    Ok(AntiEntropyResult {
        ops_sent: ops_count,
        ops_received: 0,
        success: true,
    })
}

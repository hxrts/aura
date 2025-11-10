//! Example: Capability-Guarded DKD Protocol
//!
//! This example demonstrates how to use the ProtocolGuard framework to implement
//! capability-guarded protocol execution with delta fact application and privacy
//! budget tracking, as specified in Phase 2.3 of the refactor plan.
//!
//! ## Key Features Demonstrated
//!
//! 1. **Capability Guards**: `need(σ) ≤ C` checking before protocol execution
//! 2. **Delta Facts**: Atomic journal fact application after successful execution
//! 3. **Privacy Budget**: Leakage tracking across adversary classes
//! 4. **Effect Integration**: Seamless integration with AuraEffectSystem
//!
//! ## Usage Pattern
//!
//! ```rust,ignore
//! use crate::examples::guarded_dkd::execute_guarded_dkd;
//! use crate::guards::{ProtocolGuard, LeakageBudget};
//! use aura_wot::Capability;
//!
//! // Create DKD configuration
//! let config = DkdConfig {
//!     participants: vec![alice_id, bob_id],
//!     threshold: 2,
//!     app_id: "example_app".to_string(),
//!     context: "user_signing".to_string(),
//!     derivation_path: vec![44, 0, 0],
//! };
//!
//! // Execute with capability guards
//! let result = execute_guarded_dkd(effect_system, config).await?;
//! println!("DKD completed with {} facts applied", result.applied_deltas.len());
//! ```

use crate::{
    effects::system::AuraEffectSystem,
    guards::{guard, GuardedExecutionResult, LeakageBudget, ProtocolGuard},
};
use aura_core::{AuraError, AuraResult, DeviceId, SessionId};
use aura_wot::Capability;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use tracing::{debug, error, info};

/// DKD protocol configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkdConfig {
    /// Participating devices
    pub participants: Vec<DeviceId>,
    /// Threshold for key derivation
    pub threshold: u32,
    /// Application identifier
    pub app_id: String,
    /// Derivation context
    pub context: String,
    /// BIP44 derivation path
    pub derivation_path: Vec<u32>,
}

/// DKD protocol execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkdResult {
    /// Session identifier
    pub session_id: SessionId,
    /// Derived keys per participant
    pub derived_keys: HashMap<DeviceId, [u8; 32]>,
    /// Execution success status
    pub success: bool,
    /// Protocol execution time (milliseconds)
    pub execution_time_ms: u64,
    /// Number of protocol rounds completed
    pub rounds_completed: u32,
}

/// Execute capability-guarded DKD protocol
///
/// This function demonstrates the complete guard framework integration:
/// - Capability evaluation before execution
/// - Protocol execution with effect system
/// - Delta fact application to journal
/// - Privacy budget consumption tracking
///
/// ## Capabilities Required
/// - `Execute { operation: "dkd" }` - Permission to execute DKD protocols
/// - `Write { resource_pattern: "journal:session_*" }` - Permission to write session facts
///
/// ## Delta Facts Applied
/// - Session attestation with threshold signature
/// - Key derivation completion fact
/// - Participant confirmation facts
///
/// ## Privacy Budget
/// - External: 2 bits (protocol timing observable)
/// - Neighbor: 1 bit (network traffic patterns)
/// - In-group: 0 bits (no intra-group leakage)
pub async fn execute_guarded_dkd(
    effect_system: &mut AuraEffectSystem,
    config: DkdConfig,
) -> AuraResult<GuardedExecutionResult<DkdResult>> {
    info!(
        participants = config.participants.len(),
        threshold = config.threshold,
        app_id = %config.app_id,
        context = %config.context,
        "Starting capability-guarded DKD execution"
    );

    // Create protocol guard with comprehensive requirements
    let guard = guard! {
        operation: "dkd_execution",
        capabilities: [
            Capability::Execute { operation: "dkd".to_string() },
            Capability::Write { resource_pattern: "journal:session_*".to_string() },
            Capability::Read { resource_pattern: "crypto:keys".to_string() }
        ],
        deltas: [
            create_session_attestation_fact(&config),
            create_derivation_completion_fact(&config),
            create_participant_confirmation_fact(&config, effect_system.device_id())
        ],
        leakage: (2, 1, 0) // external, neighbor, in-group
    };

    // Execute DKD protocol with full guard enforcement
    guard
        .execute_with_effects(effect_system, || async {
            execute_dkd_protocol_implementation(config, effect_system).await
        })
        .await
}

/// Core DKD protocol implementation (TODO fix - Simplified for example)
///
/// This represents the actual protocol logic that would normally involve:
/// - Commitment and reveal phases
/// - Threshold signature computation
/// - Key derivation with BIP44 paths
/// - Participant coordination
async fn execute_dkd_protocol_implementation(
    config: DkdConfig,
    effect_system: &AuraEffectSystem,
) -> AuraResult<DkdResult> {
    let start_time = std::time::Instant::now();
    let session_id = SessionId::new();

    debug!(
        session_id = %session_id,
        participants = ?config.participants,
        "Executing DKD protocol implementation"
    );

    // Phase 1: Validate configuration
    validate_dkd_config(&config)?;

    // Phase 2: Derive keys using deterministic process
    let derived_keys = derive_participant_keys(&config, effect_system).await?;

    // Phase 3: Verify key derivation consistency
    verify_derivation_consistency(&derived_keys, &config)?;

    let execution_time = start_time.elapsed().as_millis() as u64;
    let rounds_completed = 3; // TODO fix - Simplified: commitment, reveal, verification

    info!(
        session_id = %session_id,
        execution_time_ms = execution_time,
        derived_keys_count = derived_keys.len(),
        "DKD protocol execution completed successfully"
    );

    Ok(DkdResult {
        session_id,
        derived_keys,
        success: true,
        execution_time_ms: execution_time,
        rounds_completed,
    })
}

/// Validate DKD configuration parameters
fn validate_dkd_config(config: &DkdConfig) -> AuraResult<()> {
    if config.participants.is_empty() {
        return Err(AuraError::configuration_error(
            "DKD requires at least one participant",
        ));
    }

    if config.threshold == 0 {
        return Err(AuraError::configuration_error(
            "DKD threshold must be greater than zero",
        ));
    }

    if config.threshold as usize > config.participants.len() {
        return Err(AuraError::configuration_error(
            "DKD threshold cannot exceed participant count",
        ));
    }

    if config.app_id.is_empty() {
        return Err(AuraError::configuration_error("DKD app_id cannot be empty"));
    }

    debug!(
        participants = config.participants.len(),
        threshold = config.threshold,
        app_id = %config.app_id,
        "DKD configuration validated successfully"
    );

    Ok(())
}

/// Derive keys for all participants (TODO fix - Simplified implementation)
async fn derive_participant_keys(
    config: &DkdConfig,
    effect_system: &AuraEffectSystem,
) -> AuraResult<HashMap<DeviceId, [u8; 32]>> {
    let mut derived_keys = HashMap::new();

    // TODO fix - Simplified key derivation - in reality this would involve:
    // - Threshold secret sharing
    // - Multi-round commitment/reveal protocol
    // - BIP44 derivation with proper entropy
    // - Verification of derivation consistency

    for (index, &device_id) in config.participants.iter().enumerate() {
        // Create deterministic key based on config and device
        let key_material = create_deterministic_key(
            &config.app_id,
            &config.context,
            &config.derivation_path,
            device_id,
            index,
        );

        derived_keys.insert(device_id, key_material);

        debug!(
            device_id = %device_id,
            key_index = index,
            "Derived key for participant"
        );
    }

    Ok(derived_keys)
}

/// Create deterministic key material (placeholder implementation)
fn create_deterministic_key(
    app_id: &str,
    context: &str,
    derivation_path: &[u32],
    device_id: DeviceId,
    index: usize,
) -> [u8; 32] {
    // TODO fix - Simplified deterministic key generation
    // In practice, this would use proper cryptographic key derivation

    let mut key = [0u8; 32];
    let combined = format!(
        "{}:{}:{:?}:{}:{}",
        app_id, context, derivation_path, device_id, index
    );
    let hash = blake3::hash(combined.as_bytes());
    key.copy_from_slice(&hash.as_bytes()[..32]);
    key
}

/// Verify derivation consistency across participants
fn verify_derivation_consistency(
    derived_keys: &HashMap<DeviceId, [u8; 32]>,
    config: &DkdConfig,
) -> AuraResult<()> {
    if derived_keys.len() != config.participants.len() {
        return Err(AuraError::internal(
            "Key derivation count mismatch with participant count",
        ));
    }

    for device_id in &config.participants {
        if !derived_keys.contains_key(device_id) {
            return Err(AuraError::internal(&format!(
                "Missing derived key for participant: {}",
                device_id
            )));
        }
    }

    debug!(
        derived_keys_count = derived_keys.len(),
        participants_count = config.participants.len(),
        "Key derivation consistency verified"
    );

    Ok(())
}

// Delta fact creation functions

/// Create session attestation fact for journal
fn create_session_attestation_fact(config: &DkdConfig) -> JsonValue {
    serde_json::json!({
        "type": "session_attestation",
        "session_id": SessionId::new().to_string(),
        "protocol": "dkd",
        "participants": config.participants,
        "threshold": config.threshold,
        "app_id": config.app_id,
        "context": config.context,
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        "attestation": {
            "protocol_version": "1.0",
            "compliance_verified": true,
            "guard_evaluation_passed": true
        }
    })
}

/// Create derivation completion fact for journal
fn create_derivation_completion_fact(config: &DkdConfig) -> JsonValue {
    serde_json::json!({
        "type": "intent_finalization",
        "intent_id": format!("dkd_{}_{}", config.app_id, config.context),
        "result": {
            "status": "completed",
            "derivation_path": config.derivation_path,
            "participant_count": config.participants.len(),
            "threshold": config.threshold,
            "completion_timestamp": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        }
    })
}

/// Create participant confirmation fact for journal
fn create_participant_confirmation_fact(config: &DkdConfig, device_id: DeviceId) -> JsonValue {
    serde_json::json!({
        "type": "device_registration",
        "device_id": device_id.to_string(),
        "metadata": {
            "protocol": "dkd",
            "session_role": "participant",
            "app_id": config.app_id,
            "context": config.context,
            "confirmation_timestamp": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            "guard_compliance": true
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::ExecutionMode;

    #[tokio::test]
    async fn test_dkd_config_validation() {
        // Test empty participants
        let config = DkdConfig {
            participants: vec![],
            threshold: 1,
            app_id: "test".to_string(),
            context: "test".to_string(),
            derivation_path: vec![0],
        };
        assert!(validate_dkd_config(&config).is_err());

        // Test zero threshold
        let config = DkdConfig {
            participants: vec![DeviceId::new()],
            threshold: 0,
            app_id: "test".to_string(),
            context: "test".to_string(),
            derivation_path: vec![0],
        };
        assert!(validate_dkd_config(&config).is_err());

        // Test valid configuration
        let config = DkdConfig {
            participants: vec![DeviceId::new(), DeviceId::new()],
            threshold: 2,
            app_id: "test".to_string(),
            context: "test".to_string(),
            derivation_path: vec![44, 0, 0],
        };
        assert!(validate_dkd_config(&config).is_ok());
    }

    #[tokio::test]
    async fn test_deterministic_key_generation() {
        let device_id = DeviceId::new();
        let key1 = create_deterministic_key("app1", "context1", &[44, 0, 0], device_id, 0);
        let key2 = create_deterministic_key("app1", "context1", &[44, 0, 0], device_id, 0);

        // Same inputs should produce same key
        assert_eq!(key1, key2);

        let key3 = create_deterministic_key("app2", "context1", &[44, 0, 0], device_id, 0);

        // Different inputs should produce different keys
        assert_ne!(key1, key3);
    }

    #[tokio::test]
    async fn test_guard_example_compilation() {
        // This test verifies that the guard framework compiles correctly
        let device_id = DeviceId::new();
        let mut effect_system = AuraEffectSystem::new(device_id, ExecutionMode::Testing);

        let config = DkdConfig {
            participants: vec![device_id],
            threshold: 1,
            app_id: "test_app".to_string(),
            context: "test_context".to_string(),
            derivation_path: vec![44, 0, 0],
        };

        // Note: This would fail at runtime due to missing capability evaluation,
        // but demonstrates that the API compiles correctly
        let guard = ProtocolGuard::new("test_dkd")
            .require_capability(Capability::Execute {
                operation: "dkd".to_string(),
            })
            .leakage_budget(LeakageBudget::new(1, 0, 0));

        // Verify guard was constructed correctly
        assert_eq!(guard.operation_id, "test_dkd");
        assert_eq!(guard.required_capabilities.len(), 1);
        assert_eq!(guard.leakage_budget.external, 1);
    }
}

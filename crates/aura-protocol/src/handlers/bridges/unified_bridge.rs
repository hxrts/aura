//! Unified Type-Erased Aura Handler Bridge
//!
//! This module provides a unified bridge that wraps any type implementing all
//! effect traits into a type-erased AuraHandler. This enables dynamic composition
//! and runtime flexibility while maintaining type safety through the effect system.

use async_trait::async_trait;
use std::fmt;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::effects::*;
use crate::handlers::{
    context_immutable::AuraContext, AuraHandler, AuraHandlerError, EffectType, ExecutionMode,
};
use aura_core::hash::hash;
use aura_core::LocalSessionType;
use std::time::Duration;

/// Type-erased bridge for dynamic handler composition
///
/// This bridge wraps any type that implements AuraEffects and provides
/// a unified AuraHandler interface. It enables:
/// - Dynamic composition of effect handlers
/// - Runtime reconfiguration of effect implementations
/// - Type-safe effect dispatch through trait objects
/// - Middleware composition and decoration
pub struct UnifiedAuraHandlerBridge {
    /// The wrapped effect implementation
    effects: Arc<Mutex<dyn AuraEffects>>,
    /// Execution mode for this bridge
    execution_mode: ExecutionMode,
    /// Supported effect types (cached for performance)
    supported_effects: Vec<EffectType>,
}

impl fmt::Debug for UnifiedAuraHandlerBridge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UnifiedAuraHandlerBridge")
            .field("execution_mode", &self.execution_mode)
            .field("supported_effects", &self.supported_effects)
            .field("effects", &"<dyn AuraEffects>")
            .finish()
    }
}

impl UnifiedAuraHandlerBridge {
    /// Create a new unified bridge wrapping the given effects implementation
    pub fn new(effects: impl AuraEffects + 'static, execution_mode: ExecutionMode) -> Self {
        let supported_effects = EffectType::all()
            .into_iter()
            .filter(|&effect_type| {
                // All effect types that are part of AuraEffects are supported
                match effect_type {
                    EffectType::Crypto
                    | EffectType::Network
                    | EffectType::Storage
                    | EffectType::Time
                    | EffectType::Console
                    | EffectType::Random
                    | EffectType::EffectApi
                    | EffectType::Journal
                    | EffectType::Tree
                    | EffectType::Choreographic
                    | EffectType::System => true,
                    // Agent effects would require additional trait bounds
                    _ => false,
                }
            })
            .collect();

        Self {
            effects: Arc::new(Mutex::new(effects)),
            execution_mode,
            supported_effects,
        }
    }

    /// Create a unified bridge from an Arc-wrapped effects implementation
    pub fn from_arc(effects: Arc<Mutex<dyn AuraEffects>>, execution_mode: ExecutionMode) -> Self {
        let supported_effects = EffectType::all()
            .into_iter()
            .filter(|&effect_type| match effect_type {
                EffectType::Crypto
                | EffectType::Network
                | EffectType::Storage
                | EffectType::Time
                | EffectType::Console
                | EffectType::Random
                | EffectType::EffectApi
                | EffectType::Journal
                | EffectType::Tree
                | EffectType::Choreographic
                | EffectType::System => true,
                _ => false,
            })
            .collect();

        Self {
            effects,
            execution_mode,
            supported_effects,
        }
    }

    /// Get a reference to the wrapped effects implementation
    pub fn effects(&self) -> Arc<Mutex<dyn AuraEffects>> {
        Arc::clone(&self.effects)
    }

    /// Execute a typed effect operation through the bridge
    ///
    /// This method provides a type-safe way to execute effects while
    /// maintaining the flexibility of the type-erased interface.
    pub async fn execute_typed_effect<P, R>(
        &mut self,
        effect_type: EffectType,
        operation: &str,
        params: P,
        ctx: &mut AuraContext,
    ) -> Result<R, AuraHandlerError>
    where
        P: serde::Serialize + Send,
        R: serde::de::DeserializeOwned + Send,
    {
        // Serialize parameters
        let param_bytes =
            bincode::serialize(&params).map_err(|e| AuraHandlerError::EffectSerialization {
                effect_type,
                operation: operation.to_string(),
                source: e.into(),
            })?;

        // Execute through the handler interface
        let result_bytes = self
            .execute_effect(effect_type, operation, &param_bytes, ctx)
            .await?;

        // Deserialize the result
        bincode::deserialize(&result_bytes).map_err(|e| AuraHandlerError::EffectDeserialization {
            effect_type,
            operation: operation.to_string(),
            source: e.into(),
        })
    }
}

#[async_trait]
impl AuraHandler for UnifiedAuraHandlerBridge {
    async fn execute_effect(
        &self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        _ctx: &AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError> {
        // Check if effect type is supported
        if !self.supports_effect(effect_type) {
            return Err(AuraHandlerError::UnsupportedEffect { effect_type });
        }

        // Lock the effects implementation
        let effects_guard = self.effects.lock().await;

        // Dispatch to the appropriate effect based on type
        match effect_type {
            EffectType::Network => {
                self.execute_network_effect(&*effects_guard, operation, parameters)
                    .await
            }
            EffectType::Storage => {
                self.execute_storage_effect(&*effects_guard, operation, parameters)
                    .await
            }
            EffectType::Crypto => {
                self.execute_crypto_effect(&*effects_guard, operation, parameters)
                    .await
            }
            EffectType::Time => {
                self.execute_time_effect(&*effects_guard, operation, parameters)
                    .await
            }
            EffectType::Console => {
                self.execute_console_effect(&*effects_guard, operation, parameters)
                    .await
            }
            EffectType::Random => {
                self.execute_random_effect(&*effects_guard, operation, parameters)
                    .await
            }
            EffectType::EffectApi => {
                self.execute_effect_api_effect(&*effects_guard, operation, parameters)
                    .await
            }
            EffectType::Journal => {
                self.execute_journal_effect(&*effects_guard, operation, parameters)
                    .await
            }
            EffectType::Tree => {
                self.execute_tree_effect(&*effects_guard, operation, parameters)
                    .await
            }
            EffectType::Choreographic => {
                self.execute_choreographic_effect(&*effects_guard, operation, parameters)
                    .await
            }
            EffectType::System => {
                self.execute_system_effect(&*effects_guard, operation, parameters)
                    .await
            }
            _ => Err(AuraHandlerError::UnsupportedEffect { effect_type }),
        }
    }

    async fn execute_session(
        &self,
        _session: LocalSessionType,
        _ctx: &AuraContext,
    ) -> Result<(), AuraHandlerError> {
        // Session execution would typically use choreographic effects
        // For now, provide a basic implementation that delegates to choreographic effects
        let effects_guard = self.effects.lock().await;

        // Convert session to choreographic operations (simplified)
        // In a full implementation, this would use the session type algebra
        // to compile session types to choreographic effects

        // For demonstration, we'll emit a choreographic event
        let event = ChoreographyEvent::PhaseStarted {
            phase: "session_start".to_string(),
            participants: vec![], // Would be derived from session type
        };

        effects_guard.emit_choreo_event(event).await.map_err(|e| {
            AuraHandlerError::SessionExecution {
                source: Box::new(e),
            }
        })
    }

    fn supports_effect(&self, effect_type: EffectType) -> bool {
        self.supported_effects.contains(&effect_type)
    }

    fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }
}

impl UnifiedAuraHandlerBridge {
    /// Execute network effects through the wrapped implementation
    async fn execute_network_effect(
        &self,
        effects: &dyn AuraEffects,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "send_to_peer" => {
                let (peer_id, message): (uuid::Uuid, Vec<u8>) = bincode::deserialize(parameters)
                    .map_err(|e| AuraHandlerError::ParameterDeserializationFailed {
                        source: e.into(),
                    })?;

                effects.send_to_peer(peer_id, message).await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;

                Ok(bincode::serialize(&()).unwrap_or_default())
            }
            "broadcast" => {
                let message: Vec<u8> = bincode::deserialize(parameters).map_err(|e| {
                    AuraHandlerError::ParameterDeserializationFailed { source: e.into() }
                })?;

                effects.broadcast(message).await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;

                Ok(bincode::serialize(&()).unwrap_or_default())
            }
            "receive" => {
                let result =
                    effects
                        .receive()
                        .await
                        .map_err(|e| AuraHandlerError::ExecutionFailed {
                            source: Box::new(e),
                        })?;

                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            "connected_peers" => {
                let result = effects.connected_peers().await;
                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnsupportedOperation {
                effect_type: EffectType::Network,
                operation: operation.to_string(),
            }),
        }
    }

    /// Execute storage effects through the wrapped implementation
    async fn execute_storage_effect(
        &self,
        effects: &dyn AuraEffects,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "store" => {
                let (key, value): (String, Vec<u8>) =
                    bincode::deserialize(parameters).map_err(|e| {
                        AuraHandlerError::ParameterDeserializationFailed { source: e.into() }
                    })?;

                effects.store(&key, value).await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;

                Ok(bincode::serialize(&()).unwrap_or_default())
            }
            "retrieve" => {
                let key: String = bincode::deserialize(parameters).map_err(|e| {
                    AuraHandlerError::ParameterDeserializationFailed { source: e.into() }
                })?;

                let result = effects.retrieve(&key).await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;

                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            "remove" => {
                let key: String = bincode::deserialize(parameters).map_err(|e| {
                    AuraHandlerError::ParameterDeserializationFailed { source: e.into() }
                })?;

                let result =
                    effects
                        .remove(&key)
                        .await
                        .map_err(|e| AuraHandlerError::ExecutionFailed {
                            source: Box::new(e),
                        })?;

                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnsupportedOperation {
                effect_type: EffectType::Storage,
                operation: operation.to_string(),
            }),
        }
    }

    /// Execute crypto effects through the wrapped implementation
    async fn execute_crypto_effect(
        &self,
        effects: &dyn AuraEffects,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "hash" => {
                let data: Vec<u8> = bincode::deserialize(parameters).map_err(|e| {
                    AuraHandlerError::ParameterDeserializationFailed { source: e.into() }
                })?;

                let result = hash(&data);
                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            "random_bytes" => {
                let len: usize = bincode::deserialize(parameters).map_err(|e| {
                    AuraHandlerError::ParameterDeserializationFailed { source: e.into() }
                })?;

                let result = effects.random_bytes(len).await;
                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            "ed25519_generate_keypair" => {
                let result = effects.ed25519_generate_keypair().await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;

                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            "ed25519_sign" => {
                let (message, private_key): (Vec<u8>, Vec<u8>) = bincode::deserialize(parameters)
                    .map_err(|e| {
                    AuraHandlerError::ParameterDeserializationFailed { source: e.into() }
                })?;

                let result = effects
                    .ed25519_sign(&message, &private_key)
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;

                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnsupportedOperation {
                effect_type: EffectType::Crypto,
                operation: operation.to_string(),
            }),
        }
    }

    /// Execute time effects through the wrapped implementation
    async fn execute_time_effect(
        &self,
        effects: &dyn AuraEffects,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "current_epoch" => {
                let result = effects
                    .physical_time()
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed { source: e.into() })?
                    .ts_ms;
                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            "current_timestamp" => {
                let result = effects
                    .physical_time()
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed { source: e.into() })?
                    .ts_ms;
                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            "sleep_ms" => {
                let ms: u64 = bincode::deserialize(parameters).map_err(|e| {
                    AuraHandlerError::ParameterDeserializationFailed { source: e.into() }
                })?;

                tokio::time::sleep(Duration::from_millis(ms)).await;
                Ok(bincode::serialize(&()).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnsupportedOperation {
                effect_type: EffectType::Time,
                operation: operation.to_string(),
            }),
        }
    }

    /// Execute console effects through the wrapped implementation
    async fn execute_console_effect(
        &self,
        effects: &dyn AuraEffects,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "log_info" => {
                let (message, fields): (String, Vec<(String, String)>) =
                    bincode::deserialize(parameters).map_err(|e| {
                        AuraHandlerError::ParameterDeserializationFailed { source: e.into() }
                    })?;

                // Format fields into the message (ConsoleEffects only takes message string)
                let formatted_message = if fields.is_empty() {
                    message
                } else {
                    let fields_str: Vec<String> =
                        fields.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
                    format!("{} [{}]", message, fields_str.join(", "))
                };

                effects.log_info(&formatted_message).await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                Ok(bincode::serialize(&()).unwrap_or_default())
            }
            "log_error" => {
                let (message, fields): (String, Vec<(String, String)>) =
                    bincode::deserialize(parameters).map_err(|e| {
                        AuraHandlerError::ParameterDeserializationFailed { source: e.into() }
                    })?;

                // Format fields into the message (ConsoleEffects only takes message string)
                let formatted_message = if fields.is_empty() {
                    message
                } else {
                    let fields_str: Vec<String> =
                        fields.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
                    format!("{} [{}]", message, fields_str.join(", "))
                };

                effects.log_error(&formatted_message).await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                Ok(bincode::serialize(&()).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnsupportedOperation {
                effect_type: EffectType::Console,
                operation: operation.to_string(),
            }),
        }
    }

    /// Execute random effects through the wrapped implementation
    async fn execute_random_effect(
        &self,
        effects: &dyn AuraEffects,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "random_bytes" => {
                let len: usize = bincode::deserialize(parameters).map_err(|e| {
                    AuraHandlerError::ParameterDeserializationFailed { source: e.into() }
                })?;

                let result = effects.random_bytes(len).await;
                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            "random_bytes_32" => {
                let result = effects.random_bytes_32().await;
                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            "random_u64" => {
                let result = effects.random_u64().await;
                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnsupportedOperation {
                effect_type: EffectType::Random,
                operation: operation.to_string(),
            }),
        }
    }

    /// Execute effect_api effects through the wrapped implementation
    async fn execute_effect_api_effect(
        &self,
        effects: &dyn AuraEffects,
        operation: &str,
        _parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "current_epoch" => {
                let result = crate::effects::effect_api::EffectApiEffects::current_epoch(effects)
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnsupportedOperation {
                effect_type: EffectType::EffectApi,
                operation: operation.to_string(),
            }),
        }
    }

    /// Execute journal effects through the wrapped implementation
    async fn execute_journal_effect(
        &self,
        effects: &dyn AuraEffects,
        operation: &str,
        _parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "get_journal_state" | "get_journal" => {
                let result =
                    effects
                        .get_journal()
                        .await
                        .map_err(|e| AuraHandlerError::ExecutionFailed {
                            source: Box::new(e),
                        })?;
                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnsupportedOperation {
                effect_type: EffectType::Journal,
                operation: operation.to_string(),
            }),
        }
    }

    /// Execute tree effects through the wrapped implementation
    async fn execute_tree_effect(
        &self,
        effects: &dyn AuraEffects,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "get_current_state" => {
                let result = effects.get_current_state().await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            "get_current_commitment" => {
                let result = effects.get_current_commitment().await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnsupportedOperation {
                effect_type: EffectType::Tree,
                operation: operation.to_string(),
            }),
        }
    }

    /// Execute choreographic effects through the wrapped implementation
    async fn execute_choreographic_effect(
        &self,
        effects: &dyn AuraEffects,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "send_to_role_bytes" => {
                let (role, message): (ChoreographicRole, Vec<u8>) =
                    bincode::deserialize(parameters).map_err(|e| {
                        AuraHandlerError::ParameterDeserializationFailed { source: e.into() }
                    })?;

                effects
                    .send_to_role_bytes(role, message)
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;

                Ok(bincode::serialize(&()).unwrap_or_default())
            }
            "broadcast_bytes" => {
                let message: Vec<u8> = bincode::deserialize(parameters).map_err(|e| {
                    AuraHandlerError::ParameterDeserializationFailed { source: e.into() }
                })?;

                effects.broadcast_bytes(message).await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;

                Ok(bincode::serialize(&()).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnsupportedOperation {
                effect_type: EffectType::Choreographic,
                operation: operation.to_string(),
            }),
        }
    }

    /// Execute system effects through the wrapped implementation
    async fn execute_system_effect(
        &self,
        effects: &dyn AuraEffects,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "log" => {
                let (level, component, message): (String, String, String) =
                    bincode::deserialize(parameters).map_err(|e| {
                        AuraHandlerError::ParameterDeserializationFailed { source: e.into() }
                    })?;

                effects
                    .log(&level, &component, &message)
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;

                Ok(bincode::serialize(&()).unwrap_or_default())
            }
            "health_check" => {
                let result = effects.health_check().await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnsupportedOperation {
                effect_type: EffectType::System,
                operation: operation.to_string(),
            }),
        }
    }
}

/// Factory for creating unified handler bridges
pub struct UnifiedHandlerBridgeFactory;

impl UnifiedHandlerBridgeFactory {
    /// Create a unified bridge from any AuraEffects implementation
    pub fn create_bridge<T>(effects: T, execution_mode: ExecutionMode) -> UnifiedAuraHandlerBridge
    where
        T: AuraEffects + 'static,
    {
        UnifiedAuraHandlerBridge::new(effects, execution_mode)
    }

    /// Create a unified bridge from an Arc<Mutex<dyn AuraEffects>>
    pub fn create_bridge_from_arc(
        effects: Arc<Mutex<dyn AuraEffects>>,
        execution_mode: ExecutionMode,
    ) -> UnifiedAuraHandlerBridge {
        UnifiedAuraHandlerBridge::from_arc(effects, execution_mode)
    }
}

#[cfg(all(test, feature = "fixture_effects"))]
mod tests {
    use super::*;
    use aura_core::identifiers::DeviceId;
    use aura_macros::aura_test;
    use aura_testkit::*;
    use uuid::Uuid;

    #[aura_test]
    async fn test_unified_bridge_creation() -> aura_core::AuraResult<()> {
        let fixture = create_test_fixture().await?;
        let device_id = fixture.device_id();
        let effect_system = (*fixture.effects()).clone();
        let bridge = UnifiedAuraHandlerBridge::new(effect_system, ExecutionMode::Testing);

        assert_eq!(bridge.execution_mode(), ExecutionMode::Testing);
        assert!(bridge.supports_effect(EffectType::Network));
        assert!(bridge.supports_effect(EffectType::Storage));
        assert!(bridge.supports_effect(EffectType::Crypto));
        assert!(bridge.supports_effect(EffectType::System));
        Ok(())
    }

    #[aura_test]
    async fn test_unified_bridge_effect_execution() -> aura_core::AuraResult<()> {
        let fixture = create_test_fixture().await?;
        let device_id = fixture.device_id();
        let effect_system = (*fixture.effects()).clone();
        let bridge = UnifiedAuraHandlerBridge::new(effect_system, ExecutionMode::Testing);
        let ctx = AuraContext::for_testing(device_id);

        // Test system effect execution
        let params = (
            "INFO".to_string(),
            "test".to_string(),
            "test message".to_string(),
        );
        let param_bytes = bincode::serialize(&params).unwrap();

        let result = bridge
            .execute_effect(EffectType::System, "log", &param_bytes, &ctx)
            .await;

        assert!(result.is_ok());
        Ok(())
    }

    #[aura_test]
    async fn test_unified_bridge_typed_execution() -> aura_core::AuraResult<()> {
        let fixture = create_test_fixture().await?;
        let device_id = fixture.device_id();
        let effect_system = (*fixture.effects()).clone();
        let mut bridge = UnifiedAuraHandlerBridge::new(effect_system, ExecutionMode::Testing);
        let mut ctx = AuraContext::for_testing(device_id);

        // Test typed effect execution
        let params = (
            "INFO".to_string(),
            "test".to_string(),
            "typed test".to_string(),
        );
        let result: Result<(), AuraHandlerError> = bridge
            .execute_typed_effect(EffectType::System, "log", params, &mut ctx)
            .await;

        assert!(result.is_ok());
        Ok(())
    }
}

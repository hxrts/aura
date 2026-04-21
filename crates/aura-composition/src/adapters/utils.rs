//! Shared serialization helpers for adapter operation parameters/results.

use crate::registry::HandlerError;
use serde::de::DeserializeOwned;
use serde::Serialize;

/// Wrap an effect execution failure in the canonical adapter error shape.
pub fn execution_failed<E>(source: E) -> HandlerError
where
    E: std::error::Error + Send + Sync + 'static,
{
    HandlerError::ExecutionFailed {
        source: Box::new(source),
    }
}

/// Return the canonical empty byte payload for a successful void operation.
pub fn void_result() -> Vec<u8> {
    Vec::new()
}

/// Deserialize typed operation parameters with consistent adapter error mapping.
pub fn deserialize_operation_params<T: DeserializeOwned>(
    effect_type: aura_core::EffectType,
    operation: &str,
    parameters: &[u8],
) -> Result<T, HandlerError> {
    aura_core::util::serialization::from_slice(parameters).map_err(|e| {
        HandlerError::EffectDeserialization {
            effect_type,
            operation: operation.to_string(),
            source: Box::new(e),
        }
    })
}

/// Serialize typed operation results with consistent adapter error mapping.
pub fn serialize_operation_result<T: Serialize>(
    effect_type: aura_core::EffectType,
    operation: &str,
    result: &T,
) -> Result<Vec<u8>, HandlerError> {
    aura_core::util::serialization::to_vec(result).map_err(|e| HandlerError::EffectSerialization {
        effect_type,
        operation: operation.to_string(),
        source: Box::new(e),
    })
}

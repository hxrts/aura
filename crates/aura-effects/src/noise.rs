//! Noise Protocol Implementation
//!
//! Implementation of the NoiseEffects trait using the `snow` crate.

use async_trait::async_trait;
use aura_core::effects::noise::{
    HandshakeState, NoiseEffects, NoiseError, NoiseParams, TransportState,
};
use aura_core::AuraError;
use snow::{Builder, TransportState as SnowTransportState};

/// Real implementation of Noise effects using the `snow` crate.
#[derive(Debug, Default, Clone)]
pub struct RealNoiseHandler;

impl RealNoiseHandler {
    /// Create a new instance of the handler.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl NoiseEffects for RealNoiseHandler {
    async fn create_handshake_state(
        &self,
        params: NoiseParams,
    ) -> Result<HandshakeState, NoiseError> {
        // Noise_IKpsk2_25519_ChaChaPoly_BLAKE2s
        // IK:
        //   <- s
        //   ...
        //   -> e, es, s, ss
        //   <- e, ee, se, psk
        let params_str = "Noise_IKpsk2_25519_ChaChaPoly_BLAKE2s";
        let builder = Builder::new(
            params_str
                .parse()
                .map_err(|e| AuraError::crypto(format!("Invalid Noise params: {e}")))?,
        );

        let builder = builder
            .local_private_key(&params.local_private_key)
            .remote_public_key(&params.remote_public_key)
            .psk(1, &params.psk); // psk at index 1 (2nd message) for IKpsk2

        let state = if params.is_initiator {
            builder.build_initiator()
        } else {
            builder.build_responder()
        };

        let state = state
            .map_err(|e| AuraError::crypto(format!("Failed to build handshake state: {e}")))?;

        Ok(HandshakeState(Box::new(state)))
    }

    async fn write_message(
        &self,
        state: HandshakeState,
        payload: &[u8],
    ) -> Result<(Vec<u8>, HandshakeState), NoiseError> {
        // Unbox the state
        let mut inner_state = state
            .0
            .downcast::<snow::HandshakeState>()
            .map_err(|_| AuraError::internal("Invalid handshake state type"))?;

        let mut output = vec![0u8; 65535]; // Max noise message size
        let len = inner_state
            .write_message(payload, &mut output)
            .map_err(|e| AuraError::crypto(format!("Handshake write failed: {e}")))?;

        output.truncate(len);

        Ok((output, HandshakeState(inner_state)))
    }

    async fn read_message(
        &self,
        state: HandshakeState,
        message: &[u8],
    ) -> Result<(Vec<u8>, HandshakeState), NoiseError> {
        // Unbox the state
        let mut inner_state = state
            .0
            .downcast::<snow::HandshakeState>()
            .map_err(|_| AuraError::internal("Invalid handshake state type"))?;

        let mut payload = vec![0u8; 65535];
        let len = inner_state
            .read_message(message, &mut payload)
            .map_err(|e| AuraError::crypto(format!("Handshake read failed: {e}")))?;

        payload.truncate(len);

        Ok((payload, HandshakeState(inner_state)))
    }

    async fn into_transport_mode(
        &self,
        state: HandshakeState,
    ) -> Result<TransportState, NoiseError> {
        // Unbox the state
        let inner_state = state
            .0
            .downcast::<snow::HandshakeState>()
            .map_err(|_| AuraError::internal("Invalid handshake state type"))?;

        let transport = inner_state.into_transport_mode().map_err(|e| {
            AuraError::crypto(format!("Failed to transition to transport mode: {e}"))
        })?;

        Ok(TransportState(Box::new(transport)))
    }

    async fn encrypt_transport_message(
        &self,
        state: &mut TransportState,
        payload: &[u8],
    ) -> Result<Vec<u8>, NoiseError> {
        let inner_state = state
            .0
            .downcast_mut::<SnowTransportState>()
            .ok_or_else(|| AuraError::internal("Invalid transport state type"))?;

        let mut output = vec![0u8; payload.len() + 16]; // Approx overhead + payload
                                                        // Note: snow::TransportState::write_message handles encryption
        let len = inner_state
            .write_message(payload, &mut output)
            .map_err(|e| AuraError::crypto(format!("Transport encrypt failed: {e}")))?;

        output.truncate(len);
        Ok(output)
    }

    async fn decrypt_transport_message(
        &self,
        state: &mut TransportState,
        message: &[u8],
    ) -> Result<Vec<u8>, NoiseError> {
        let inner_state = state
            .0
            .downcast_mut::<SnowTransportState>()
            .ok_or_else(|| AuraError::internal("Invalid transport state type"))?;

        let mut output = vec![0u8; message.len()];
        let len = inner_state
            .read_message(message, &mut output)
            .map_err(|e| AuraError::crypto(format!("Transport decrypt failed: {e}")))?;

        output.truncate(len);
        Ok(output)
    }
}

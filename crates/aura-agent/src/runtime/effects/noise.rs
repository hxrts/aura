//! Noise Effects Implementation for Runtime
//!
//! Delegates to the stateless `aura_effects::noise::RealNoiseHandler`.

use crate::runtime::AuraEffectSystem;
use async_trait::async_trait;
use aura_core::effects::noise::{
    HandshakeState, NoiseEffects, NoiseError, NoiseParams, TransportState,
};
use aura_effects::noise::RealNoiseHandler;

#[async_trait]
impl NoiseEffects for AuraEffectSystem {
    async fn create_handshake_state(
        &self,
        params: NoiseParams,
    ) -> Result<HandshakeState, NoiseError> {
        RealNoiseHandler::new().create_handshake_state(params).await
    }

    async fn write_message(
        &self,
        state: HandshakeState,
        payload: &[u8],
    ) -> Result<(Vec<u8>, HandshakeState), NoiseError> {
        RealNoiseHandler::new().write_message(state, payload).await
    }

    async fn read_message(
        &self,
        state: HandshakeState,
        message: &[u8],
    ) -> Result<(Vec<u8>, HandshakeState), NoiseError> {
        RealNoiseHandler::new().read_message(state, message).await
    }

    async fn into_transport_mode(
        &self,
        state: HandshakeState,
    ) -> Result<TransportState, NoiseError> {
        RealNoiseHandler::new().into_transport_mode(state).await
    }

    async fn encrypt_transport_message(
        &self,
        state: &mut TransportState,
        payload: &[u8],
    ) -> Result<Vec<u8>, NoiseError> {
        RealNoiseHandler::new()
            .encrypt_transport_message(state, payload)
            .await
    }

    async fn decrypt_transport_message(
        &self,
        state: &mut TransportState,
        message: &[u8],
    ) -> Result<Vec<u8>, NoiseError> {
        RealNoiseHandler::new()
            .decrypt_transport_message(state, message)
            .await
    }
}

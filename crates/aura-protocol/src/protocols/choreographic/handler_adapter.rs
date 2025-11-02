//! Adapter bridging Rumpsteak ChoreoHandler with Aura's middleware system
//!
//! This module provides the core integration between Rumpsteak's choreographic
//! programming framework and Aura's protocol execution infrastructure. It enables
//! choreographic protocols to leverage Aura's middleware stack while maintaining
//! type safety and session type guarantees.
//!
//! # Architecture
//!
//! The adapter pattern allows choreographic protocols to:
//! - Use Aura's transport and storage capabilities
//! - Benefit from middleware (tracing, metrics, error recovery)
//! - Integrate with Aura's effects system for deterministic testing
//! - Maintain session type safety across the protocol/transport boundary
//!
//! # Example
//!
//! ```rust,ignore
//! use aura_protocol::protocols::choreographic::{
//!     RumpsteakAdapter, BridgedRole, BridgedEndpoint,
//! };
//!
//! // Create adapter wrapping Aura handler
//! let adapter = RumpsteakAdapter::new(aura_handler, effects, context);
//!
//! // Use as ChoreoHandler in choreographic protocols
//! let role = BridgedRole { device_id, role_index: 0 };
//! let endpoint = BridgedEndpoint::new(context);
//!
//! adapter.send(&mut endpoint, role, &message).await?;
//! ```

use crate::context::BaseContext;
use crate::effects::ProtocolEffects;
use crate::middleware::handler::AuraProtocolHandler;
use async_trait::async_trait;
use rumpsteak_choreography::{ChoreoHandler, ChoreographyError, Label};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

/// Adapter that wraps an AuraProtocolHandler to implement ChoreoHandler
///
/// This adapter bridges between Aura's protocol infrastructure and Rumpsteak's
/// choreographic handler interface, enabling type-safe distributed protocols
/// with full middleware support.
///
/// # Type Parameters
///
/// - `H`: The underlying Aura protocol handler
/// - `E`: The protocol effects for deterministic execution
pub struct RumpsteakAdapter<H: AuraProtocolHandler, E: ProtocolEffects> {
    handler: H,
    effects: E,
    context: BaseContext,
}

impl<H: AuraProtocolHandler, E: ProtocolEffects> RumpsteakAdapter<H, E> {
    /// Create a new Rumpsteak adapter
    ///
    /// # Arguments
    ///
    /// - `handler`: The Aura protocol handler to wrap
    /// - `effects`: Protocol effects for time, randomness, etc.
    /// - `context`: Base context containing session information
    pub fn new(handler: H, effects: E, context: BaseContext) -> Self {
        Self {
            handler,
            effects,
            context,
        }
    }
}

/// Bridge type to convert between Role representations
///
/// In choreographic protocols, roles identify participants. This type
/// bridges between Rumpsteak's abstract roles and Aura's concrete device IDs.
///
/// # Fields
///
/// - `device_id`: The Aura device identifier
/// - `role_index`: The role's position in the choreography (for ordering)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BridgedRole {
    /// The device ID of the participant
    pub device_id: Uuid,
    /// The role index in the choreography
    pub role_index: usize,
}

/// Bridge endpoint that integrates with BaseContext
///
/// The endpoint provides access to session state and context information
/// needed during choreographic protocol execution.
pub struct BridgedEndpoint {
    /// The base context containing session state
    pub context: BaseContext,
}

impl BridgedEndpoint {
    /// Create a new bridged endpoint
    ///
    /// # Arguments
    ///
    /// - `context`: The base context for this session
    pub fn new(context: BaseContext) -> Self {
        Self { context }
    }

    /// Get current session epoch from context
    ///
    /// The epoch is used for session management and credential refresh.
    /// Returns the current epoch number from the session state.
    pub fn current_epoch(&self) -> u64 {
        // TODO: Integrate with actual BaseContext epoch tracking
        // For now, return a placeholder
        1
    }

    /// Bump session epoch in context
    pub async fn bump_epoch(&mut self, _new_epoch: u64) -> Result<(), ChoreographyError> {
        // TODO: Integrate with actual BaseContext epoch management
        // This would update the CRDT with the new epoch
        Ok(())
    }
}

#[async_trait]
impl<H, E> ChoreoHandler for RumpsteakAdapter<H, E>
where
    H: AuraProtocolHandler<DeviceId = Uuid, Message = Vec<u8>> + Send,
    E: ProtocolEffects + Send,
{
    type Role = BridgedRole;
    type Endpoint = BridgedEndpoint;

    async fn send<M>(
        &mut self,
        _ep: &mut Self::Endpoint,
        to: Self::Role,
        msg: &M,
    ) -> Result<(), ChoreographyError>
    where
        M: Serialize + Send + Sync,
    {
        // Serialize message
        let serialized =
            bincode::serialize(msg).map_err(|e| ChoreographyError::Serialization(e.to_string()))?;

        // Use AuraProtocolHandler to send through middleware stack
        self.handler
            .send_message(to.device_id, serialized)
            .await
            .map_err(|e| ChoreographyError::Transport(format!("{:?}", e)))?;

        Ok(())
    }

    async fn recv<M>(
        &mut self,
        _ep: &mut Self::Endpoint,
        from: Self::Role,
    ) -> Result<M, ChoreographyError>
    where
        M: for<'de> Deserialize<'de>,
    {
        // Receive through middleware stack
        let serialized = self
            .handler
            .receive_message(from.device_id)
            .await
            .map_err(|e| ChoreographyError::Transport(format!("{:?}", e)))?;

        // Deserialize
        bincode::deserialize(&serialized)
            .map_err(|e| ChoreographyError::Serialization(e.to_string()))
    }

    async fn choose(
        &mut self,
        _ep: &mut Self::Endpoint,
        to: Self::Role,
        choice: Label,
    ) -> Result<(), ChoreographyError> {
        // Serialize the label string
        let serialized = bincode::serialize(&choice.0)
            .map_err(|e| ChoreographyError::Serialization(e.to_string()))?;

        // Send as a regular message through middleware stack
        self.handler
            .send_message(to.device_id, serialized)
            .await
            .map_err(|e| ChoreographyError::Transport(format!("{:?}", e)))?;

        Ok(())
    }

    async fn offer(
        &mut self,
        _ep: &mut Self::Endpoint,
        from: Self::Role,
    ) -> Result<Label, ChoreographyError> {
        // Receive the serialized label through middleware stack
        let serialized = self
            .handler
            .receive_message(from.device_id)
            .await
            .map_err(|e| ChoreographyError::Transport(format!("{:?}", e)))?;

        // Deserialize to String
        let label_string: String = bincode::deserialize(&serialized)
            .map_err(|e| ChoreographyError::Serialization(e.to_string()))?;

        // Enforce a reasonable size limit after deserialization
        const MAX_LABEL_LENGTH: usize = 256;
        if label_string.len() > MAX_LABEL_LENGTH {
            return Err(ChoreographyError::ProtocolViolation(format!(
                "Label too long: {} bytes (max: {})",
                label_string.len(),
                MAX_LABEL_LENGTH
            )));
        }

        // Convert to &'static str by leaking (labels are small and long-lived)
        let label_str: &'static str = Box::leak(label_string.into_boxed_str());

        Ok(Label(label_str))
    }

    async fn with_timeout<F, T>(
        &mut self,
        _ep: &mut Self::Endpoint,
        _role: Self::Role,
        duration: Duration,
        future: F,
    ) -> Result<T, ChoreographyError>
    where
        F: std::future::Future<Output = Result<T, ChoreographyError>> + Send,
    {
        // Use tokio timeout
        match tokio::time::timeout(duration, future).await {
            Ok(result) => result,
            Err(_) => Err(ChoreographyError::Timeout(duration)),
        }
    }
}

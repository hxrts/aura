//! Transport effects trait definitions
//!
//! This module defines the trait interface for transport layer operations that form the final
//! step in the guard chain sequence. TransportEffects handles actual network packet emission
//! after authorization, flow budget charging, leakage recording, and journal fact merging.
//!
//! # Effect Classification
//!
//! - **Category**: Infrastructure Effect
//! - **Implementation**: `aura-effects` (Layer 3)
//! - **Usage**: Guard chain final step (actual network transmission after all checks pass)
//!
//! This is an infrastructure effect that must be implemented in `aura-effects`
//! with stateless handlers. Integrates with guard chain in aura-protocol.

use crate::{AuraError, AuthorityId, ContextId};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Receipt produced by successful guard chain execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportReceipt {
    /// Context this receipt applies to
    pub context: ContextId,
    /// Source authority that generated the receipt
    pub src: AuthorityId,
    /// Destination authority for the message
    pub dst: AuthorityId,
    /// Epoch during which the receipt was generated
    pub epoch: u64,
    /// Flow budget cost charged for this operation
    pub cost: u32,
    /// Unique nonce to prevent replay
    pub nonce: u64,
    /// Hash chain linking to previous receipt
    pub prev: [u8; 32],
    /// Signature over receipt data
    pub sig: Vec<u8>,
}

/// Envelope containing the actual message data and metadata for transport
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportEnvelope {
    /// Destination authority identifier
    pub destination: AuthorityId,
    /// Source authority identifier
    pub source: AuthorityId,
    /// Context identifier for this message
    pub context: ContextId,
    /// Encrypted message payload
    pub payload: Vec<u8>,
    /// Message metadata (content-type, version, etc.)
    pub metadata: std::collections::HashMap<String, String>,
    /// Receipt proving guard chain execution
    pub receipt: Option<TransportReceipt>,
}

/// Transport operation errors
#[derive(Debug, thiserror::Error, Serialize, Deserialize)]
pub enum TransportError {
    /// Failed to send message to destination
    #[error("Transport send failed to {destination}: {reason}")]
    SendFailed {
        destination: AuthorityId,
        reason: String,
    },
    /// Failed to receive message
    #[error("Transport receive failed: {reason}")]
    ReceiveFailed { reason: String },
    /// No message available for receive
    #[error("No message available")]
    NoMessage,
    /// Invalid envelope format
    #[error("Invalid envelope: {reason}")]
    InvalidEnvelope { reason: String },
    /// Receipt validation failed
    #[error("Receipt validation failed: {reason}")]
    ReceiptValidationFailed { reason: String },
    /// Destination unreachable
    #[error("Destination unreachable: {destination}")]
    DestinationUnreachable { destination: AuthorityId },
    /// Transport protocol error
    #[error("Transport protocol error: {details}")]
    ProtocolError { details: String },
    /// Channel not established
    #[error("Secure channel not established for context {context}")]
    ChannelNotEstablished { context: ContextId },
}

impl From<TransportError> for AuraError {
    fn from(err: TransportError) -> Self {
        AuraError::network(err.to_string())
    }
}

/// Transport effects trait for network packet emission
///
/// This trait represents the final step in the guard chain sequence. It handles actual
/// network communication after all authorization, flow budget, leakage, and journal
/// constraints have been satisfied.
///
/// The transport layer operates on encrypted envelopes that have passed through the
/// complete guard chain. It is responsible for reliable delivery over secure channels
/// established through rendezvous or other channel establishment protocols.
///
/// ## Guard Chain Integration
///
/// TransportEffects is invoked only after successful execution of:
/// 1. CapGuard - Authorization and capability verification
/// 2. FlowGuard - Flow budget charging and receipt generation  
/// 3. LeakageGuard - Privacy leakage budget accounting
/// 4. JournalCoupler - Atomic fact commitment to journals
///
/// ## Channel Management
///
/// Transport operations assume that secure channels have been established through
/// rendezvous protocols. Each context has at most one active channel per peer pair.
/// Channel establishment is handled by higher-level protocols.
///
/// ## Receipt Handling
///
/// Receipts prove that the sender executed the complete guard chain sequence.
/// Recipients can validate receipts to ensure proper authorization and charging.
/// Receipt chains provide audit trails for multi-hop forwarding scenarios.
#[async_trait]
pub trait TransportEffects: Send + Sync {
    /// Send an envelope to a destination authority
    ///
    /// This method assumes:
    /// - Authorization has been verified by CapGuard
    /// - Flow budget has been charged by FlowGuard  
    /// - Leakage budget has been accounted by LeakageGuard
    /// - Facts have been committed by JournalCoupler
    /// - Secure channel exists for the context
    ///
    /// The envelope contains the encrypted payload and receipt proving guard chain execution.
    /// Transport implementations handle reliable delivery over the established secure channel.
    async fn send_envelope(&self, envelope: TransportEnvelope) -> Result<(), TransportError>;

    /// Receive the next available envelope
    ///
    /// Returns an envelope from any established secure channel. The envelope includes
    /// the encrypted payload and any attached receipts. Higher-level protocols handle
    /// decryption and receipt validation.
    async fn receive_envelope(&self) -> Result<TransportEnvelope, TransportError>;

    /// Receive envelope from a specific authority within a context
    ///
    /// Filters received envelopes to match the specified source authority and context.
    /// Returns `NoMessage` error if no matching envelope is available.
    async fn receive_envelope_from(
        &self,
        source: AuthorityId,
        context: ContextId,
    ) -> Result<TransportEnvelope, TransportError>;

    /// Check if a secure channel is established for the given context and peer
    async fn is_channel_established(&self, context: ContextId, peer: AuthorityId) -> bool;

    /// Get statistics about transport operation
    async fn get_transport_stats(&self) -> TransportStats;
}

/// Statistics about transport layer operations
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TransportStats {
    /// Number of envelopes sent successfully
    pub envelopes_sent: u64,
    /// Number of envelopes received successfully  
    pub envelopes_received: u64,
    /// Number of send failures
    pub send_failures: u64,
    /// Number of receive failures
    pub receive_failures: u64,
    /// Number of active secure channels
    pub active_channels: u32,
    /// Average envelope size in bytes
    pub avg_envelope_size: u32,
}

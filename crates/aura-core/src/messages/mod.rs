//! Layer 1: Common Message Types and Envelope
//!
//! Foundational message infrastructure for all protocol layers:
//! **WireEnvelope** (versioned, serialization-safe), **MessageError** (unified error handling),
//! **TypedMessage** (tagged message validation).
//!
//! **Design Principles**:
//! - **Version safety**: All messages carry WIRE_FORMAT_VERSION for backward compatibility
//! - **Type safety**: TypedMessage with AuthStrength/AuthTag for message authentication
//! - **Size limits**: message_too_large_error prevents DoS via oversized payloads
//! - **Deterministic serialization**: enables replay detection and commitment verification
//!
//! Domain-specific messages (FROST, rendezvous, recovery) live in their protocol crates (Layer 5).

pub mod common_envelope;
pub mod common_error;
pub mod constants;
pub mod typed_messages;

// Re-export commonly used types
pub use common_envelope::WireEnvelope;
pub use common_error::{
    cid_mismatch_error, invalid_envelope_size_error, invalid_message_format_error,
    message_deserialization_error, message_serialization_error, message_too_large_error,
    unsupported_version_error, MessageError, MessageResult,
};
pub use constants::WIRE_FORMAT_VERSION;

// Re-export typed message system types
pub use typed_messages::{
    AuthStrength, AuthTag, MessageValidation, MessageValidator, Msg, SemanticVersion, TypedMessage,
};

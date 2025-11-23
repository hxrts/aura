//! Layer 2: Transport Layer Message Types
//!
//! Message types for transport layer protocols: social coordination (peer discovery),
//! rendezvous (NAT traversal and peer location).
//!
//! **Integration** (per docs/110_rendezvous.md):
//! Messages are encoded in aura-core WireEnvelope, flow through guard chain for
//! authorization and flow budgets, then transmitted via protocol-specific handlers
//! (Layer 3 effects in aura-effects/transport).

pub mod social_rendezvous;
pub mod social_types;

// Re-export commonly used types (social_types re-exports social_rendezvous)
pub use social_types::*;

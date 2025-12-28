//! Layer 2: Transport Layer Message Types
//!
//! Message types for transport layer protocols: rendezvous (NAT traversal and peer location)
//! and peer discovery.
//!
//! **Integration** (per docs/110_rendezvous.md):
//! Messages are encoded in aura-core WireEnvelope, flow through guard chain for
//! authorization and flow budgets, then transmitted via protocol-specific handlers
//! (Layer 3 effects in aura-effects/transport).

pub mod rendezvous;
pub mod types;

// Re-export commonly used types (types re-exports rendezvous)
pub use types::*;

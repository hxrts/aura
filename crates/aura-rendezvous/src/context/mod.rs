//! Context-Aware Rendezvous System
//!
//! This module implements rendezvous using ContextId instead of device identifiers,
//! aligning with the authority-centric architecture.

mod envelope;
mod receipt;
pub mod rendezvous;

pub use envelope::{ContextEnvelope, ContextTransportOffer};
pub use receipt::RendezvousReceipt;
pub use rendezvous::{
    ContextRendezvousCoordinator, ContextRendezvousDescriptor, ContextTransportBridge,
};

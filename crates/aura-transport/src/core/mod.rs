//! Core Transport Layer

pub mod factory;
pub mod implementations;
pub mod traits;

// Re-export core traits and implementations
pub use factory::TransportFactory;
pub use implementations::MemoryTransport;
pub use traits::Transport;

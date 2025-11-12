//! Network effect handlers
//!
//! Provides different implementations of NetworkEffects for various execution contexts.

pub mod memory;
pub mod mock;
pub mod tcp;
// Note: these modules have dependency issues, commenting out for now
// pub mod transport_integrated;
// pub mod websocket;

pub use memory::MemoryNetworkHandler;
pub use mock::MockNetworkHandler;
pub use tcp::TcpNetworkHandler;

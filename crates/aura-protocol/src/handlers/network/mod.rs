//! Network effect handlers
//!
//! Provides different implementations of NetworkEffects for various execution contexts.

pub mod memory;
pub mod real;
pub mod simulated;
#[cfg(feature = "aura-transport")]
pub mod transport_integrated;
pub mod websocket;

pub use memory::MemoryNetworkHandler;
pub use real::RealNetworkHandler;
pub use simulated::SimulatedNetworkHandler;
#[cfg(feature = "aura-transport")]
pub use transport_integrated::TransportIntegratedHandler;
pub use websocket::WebSocketNetworkHandler;
